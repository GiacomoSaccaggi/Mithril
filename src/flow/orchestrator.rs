//! Orchestrator — JEV decision controller + agent free-flow + intermediate decision agents.
//!
//! # Flow
//!
//! 1. JEV controller (`jevos-v4`) classifies user message → picks first entry-eligible agent
//! 2. Agents execute and communicate via NEXT:/TASK: protocol
//! 3. Agents can query the controller (`NEXT: controller` / `NEXT: jev`) as a fast zero-shot decision oracle
//! 4. Intermediate `type: decision` agents route (`mode: route`), gate (`mode: gate`), or consult (`mode: consult`) without ever speaking directly to the user
//! 5. Rust enforces: can_call permissions, max_rounds, token_budget
//!
//! ```mermaid
//! sequenceDiagram
//!     participant U as User
//!     participant O as Orchestrator
//!     participant G as GGUF
//!     participant A as Agent
//!     participant T as Tools
//!
//!     U->>O: message
//!     O->>G: classify
//!     G-->>O: agent name
//!     O->>A: system_prompt + task
//!     A->>T: tool calls
//!     T-->>A: results
//!     A-->>O: NEXT DONE
//!     O-->>U: final response
//! ```

use anyhow::{anyhow, Result};
use colored::Colorize;

use crate::config::MithrilConfig;
use crate::providers::{self, ChatMessage, ChatProvider};
use crate::flow::{self, TraceMode};
use super::agent_loop;

use super::fellowship::*;
use super::tokens::*;

/// A single round of context passed to agents.
#[derive(Debug, Clone)]
pub struct RoundEntry {
    pub agent_name: String,
    #[allow(dead_code)]
    pub task: String,
    pub response_summary: String,
}

/// Trace entry for UI display.
#[derive(Debug, Clone)]
pub enum TraceEntry {
    Entry { agent: String },
    AgentStart { agent: String, provider: String },
    ToolCall { name: String, success: bool, preview: String },
    Delegation { from: String, to: String, task_preview: String },
    GgufCall { task_preview: String },
    DecisionStep { agent: String, mode: String, outcome: String },
    Done { agent: String },
    BudgetWarning { used: u64, limit: u64 },
}

/// Outcome of executing an intermediate `AgentType::Decision` node.
/// Guarantees that a Decision agent always transitions to another agent and never terminates the loop.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionTransition {
    pub next_agent: String,
    pub next_task: String,
    pub mode: String,
    pub verdict: String,
}

/// Result of orchestrator processing a request.
#[derive(Debug)]
pub struct OrchestratorResult {
    pub response: String,
    pub rounds: u32,
    #[allow(dead_code)]
    pub agents_involved: Vec<String>,
    pub tokens: SessionTokens,
    pub trace: Vec<TraceEntry>,
}

/// Directive parsed from agent output.
#[derive(Debug, Clone)]
enum Directive {
    Done(String),
    CallAgent { name: String, task: String },
    None,
}

/// The orchestrator.
pub struct Orchestrator {
    config: FellowshipConfig,
    mithril_config: MithrilConfig,
    tokens: SessionTokens,
    trace_mode: TraceMode,
    trace: Vec<TraceEntry>,
    rounds: u32,
    agents_involved: Vec<String>,
    context_history: Vec<RoundEntry>,
    /// When true, only read-only tools are exposed to agents.
    pub plan_mode: bool,
}

impl Orchestrator {
    pub fn new(mut config: FellowshipConfig, mithril_config: MithrilConfig, trace_mode: TraceMode) -> Self {
        // Load markdown agent definitions from .mithril/agents/*.md
        config.load_markdown_agents();

        Self {
            config,
            mithril_config,
            tokens: SessionTokens::new(),
            trace_mode,
            trace: Vec::new(),
            rounds: 0,
            agents_involved: Vec::new(),
            context_history: Vec::new(),
            plan_mode: false,
        }
    }

    pub async fn handle_request(&mut self, user_message: &str) -> Result<OrchestratorResult> {
        self.trace.clear();
        self.rounds = 0;
        self.agents_involved.clear();
        self.context_history.clear();

        // Step 1: Check for @agent mention (skip GGUF classification)
        let first_agent = if let Some(mentioned) = self.extract_agent_mention(user_message) {
            mentioned
        } else {
            self.classify_entry(user_message).await?
        };
        self.trace.push(TraceEntry::Entry { agent: first_agent.clone() });

        // Trace entry already pushed — callers print it

        // Step 2: Agent loop
        let response = self.run_agent_loop(&first_agent, user_message).await?;

        Ok(OrchestratorResult {
            response,
            rounds: self.rounds,
            agents_involved: self.agents_involved.clone(),
            tokens: self.tokens.clone(),
            trace: self.trace.clone(),
        })
    }

    async fn classify_entry(&mut self, user_message: &str) -> Result<String> {
        let candidates = self.build_entry_candidates();

        let controller = providers::create_provider_with_model(
            &self.config.controller.provider,
            self.config.controller.effective_model(),
            &self.mithril_config,
        )?;

        let response = controller.decide_choice(
            "Which agent should handle this request?",
            user_message,
            &candidates,
        ).await?;

        let usage = TokenUsage::estimate(user_message, &response);
        self.tokens.record("controller", &usage);

        Ok(self.match_entry_candidate(&response))
    }

    /// Build the map of candidate agents eligible for initial user entry routing.
    /// Excludes internal `Decision` agents (`Consult`, `Gate`, and `Route` without `when`).
    fn build_entry_candidates(&self) -> std::collections::HashMap<String, String> {
        let mut candidates = std::collections::HashMap::new();
        for agent in self.config.agents.iter().filter(|a| a.is_entry_eligible()) {
            let desc = agent.when.as_deref().unwrap_or(&agent.role);
            candidates.insert(agent.name.clone(), desc.to_string());
        }
        if candidates.is_empty() {
            for agent in &self.config.agents {
                let desc = agent.when.as_deref().unwrap_or(&agent.role);
                candidates.insert(agent.name.clone(), desc.to_string());
            }
        }
        candidates
    }

    /// Match the controller's response to an entry-eligible agent, falling back to the first eligible agent.
    fn match_entry_candidate(&self, response: &str) -> String {
        let resp_lower = response.trim().to_lowercase();
        let eligible: Vec<&FellowshipAgent> = self.config.agents.iter()
            .filter(|a| a.is_entry_eligible())
            .collect();

        if let Some(matched) = eligible.iter().find(|a| resp_lower.contains(&a.name.to_lowercase())) {
            return matched.name.clone();
        }
        if let Some(first) = eligible.first() {
            return first.name.clone();
        }
        self.config.agents.first().map(|a| a.name.clone()).unwrap_or_default()
    }

    /// Query the Palantír BM25 index for relevant files based on user message.
    /// Returns a brief context string with file paths and snippets.
    fn query_palantir(&self, query: &str) -> String {
        use crate::index::palantir::PalantirIndex;
        
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        
        let index = match PalantirIndex::load_or_null(&cwd) {
            Some(idx) => idx,
            None => return String::new(),
        };
        
        let results = index.query(query, 3);
        if results.is_empty() {
            return String::new();
        }
        
        results.iter()
            .map(|r| format!("- {} (score: {:.2})", r.entry.path, r.score))
            .collect::<Vec<_>>()
            .join("
")
    }

    /// Check if user message starts with #agentname and extract the agent name.
    /// Returns Some(agent_name) if a valid entry-eligible agent is mentioned, None otherwise.
    fn extract_agent_mention(&self, message: &str) -> Option<String> {
        let trimmed = message.trim();
        if !trimmed.starts_with('#') {
            return None;
        }
        // Extract the word after #
        let mention = trimmed[1..].split_whitespace().next()?;
        let mention_lower = mention.to_lowercase();

        // Check if it matches any entry-eligible configured agent
        self.config.agents.iter()
            .filter(|a| a.is_entry_eligible())
            .find(|a| a.name.to_lowercase() == mention_lower)
            .map(|a| a.name.clone())
    }

    async fn run_agent_loop(&mut self, first_agent: &str, initial_task: &str) -> Result<String> {
        let mut current_agent_name = first_agent.to_string();
        let mut current_task = initial_task.to_string();
        let mut previous_agent_name: Option<String> = None;

        loop {
            // Safety: max_rounds
            if self.rounds >= self.config.max_rounds {
                return Ok(format!("[Max rounds reached after {} iterations]", self.rounds));
            }

            // Safety: token_budget
            if let Some(budget) = self.config.token_budget {
                let used = self.tokens.total().total();
                if used >= budget {
                    self.trace.push(TraceEntry::BudgetWarning { used, limit: budget });
                    if self.trace_mode != TraceMode::Silent {
                        eprintln!("  {} Token budget exhausted ({}/{})", "⚠".yellow(), used, budget);
                    }
                    return Ok("[Token budget exhausted — stopping]".to_string());
                }
            }

            self.rounds += 1;
            if !self.agents_involved.contains(&current_agent_name) {
                self.agents_involved.push(current_agent_name.clone());
            }

            // Check if current agent is an intermediate Decision agent (never user-facing)
            if !is_controller_target(&current_agent_name) {
                let agent = self.config.get_agent(&current_agent_name)
                    .ok_or_else(|| anyhow!("Agent '{}' not found", current_agent_name))?
                    .clone();

                if agent.agent_type == AgentType::Decision {
                    let transition = self.execute_decision_agent(
                        &agent,
                        &current_task,
                        previous_agent_name.as_deref(),
                    ).await?;

                    let outcome = format!("{} -> {}", transition.verdict, transition.next_agent);
                    self.trace.push(TraceEntry::DecisionStep {
                        agent: agent.name.clone(),
                        mode: transition.mode.clone(),
                        outcome: outcome.clone(),
                    });

                    self.context_history.push(RoundEntry {
                        agent_name: agent.name.clone(),
                        task: if current_task.len() > 200 { format!("{}...", &current_task[..200]) } else { current_task.clone() },
                        response_summary: format!("[decision:{}] {}", transition.mode, outcome),
                    });

                    current_agent_name = transition.next_agent;
                    current_task = transition.next_task;
                    continue;
                }
            }

            // Execute standard agent or controller oracle
            let result = if is_controller_target(&current_agent_name) {
                self.execute_controller_decision(&current_task).await?
            } else {
                let agent = self.config.get_agent(&current_agent_name)
                    .ok_or_else(|| anyhow!("Agent '{}' not found", current_agent_name))?
                    .clone();
                self.execute_agent(&agent, &current_task).await?
            };

            // Add to context history
            let summary = if result.len() > 500 { format!("{}...", &result[..500]) } else { result.clone() };
            self.context_history.push(RoundEntry {
                agent_name: current_agent_name.clone(),
                task: if current_task.len() > 200 { format!("{}...", &current_task[..200]) } else { current_task.clone() },
                response_summary: summary,
            });

            // Parse directive
            let directive = parse_agent_directive(&result);

            match directive {
                Directive::Done(response) => {
                    self.trace.push(TraceEntry::Done { agent: current_agent_name.clone() });

                    return Ok(response);
                }
                Directive::CallAgent { name, task } => {
                    // Permission check (controller / jev / gguf oracle is always allowed)
                    if !is_controller_target(&name) {
                        let caller = self.config.get_agent(&current_agent_name);
                        if let Some(caller) = caller {
                            if !caller.can_call.contains(&name) {
                                if self.trace_mode != TraceMode::Silent {
                                    eprintln!("  {} {} cannot call {} (not in can_call)", "⚠".yellow(), current_agent_name, name);
                                }
                                // Not authorized — treat full output as response
                                return Ok(result);
                            }
                        }
                    }

                    let task_preview = if task.len() > 60 { format!("{}...", &task[..60]) } else { task.clone() };

                    if is_controller_target(&name) {
                        // Controller / JEV decision oracle: execute zero-shot decision and return verdict to CALLING agent
                        self.trace.push(TraceEntry::GgufCall { task_preview: task_preview.clone() });

                        let verdict = self.execute_controller_decision(&task).await?;
                        // Feed back to same agent with controller verdict as new task
                        current_task = format!("Result from {}: {}\n\nContinue your previous task.", name, verdict);
                        // Don't change current_agent_name — stays with the caller
                    } else {
                        // Agent-to-agent delegation (including delegating to a Decision agent)
                        self.trace.push(TraceEntry::Delegation {
                            from: current_agent_name.clone(),
                            to: name.clone(),
                            task_preview: task_preview.clone(),
                        });

                        previous_agent_name = Some(current_agent_name);
                        current_agent_name = name;
                        current_task = task;
                    }
                }
                Directive::None => {
                    // No NEXT tag — entire output is the final response
                    self.trace.push(TraceEntry::Done { agent: current_agent_name.clone() });

                    return Ok(result);
                }
            }
        }
    }

    async fn execute_agent(&mut self, agent: &FellowshipAgent, task: &str) -> Result<String> {
        let provider_info = match agent.agent_type {
            AgentType::Provider | AgentType::Decision => format!("{}/{}", 
                agent.provider.as_deref().unwrap_or("local"),
                agent.model.as_deref().unwrap_or("jevos-v4")),
            AgentType::External => agent.binary.as_deref().unwrap_or("?").to_string(),
        };

        self.trace.push(TraceEntry::AgentStart {
            agent: agent.name.clone(),
            provider: provider_info.clone(),
        });

        if self.trace_mode == TraceMode::Full {
            eprintln!("  {} {} ({})", "▶".dimmed(), agent.name.cyan(), provider_info.dimmed());
        }

        match agent.agent_type {
            AgentType::External => self.execute_external(agent, task).await,
            AgentType::Provider | AgentType::Decision => self.execute_provider_agent(agent, task).await,
        }
    }

    async fn execute_provider_agent(&mut self, agent: &FellowshipAgent, task: &str) -> Result<String> {
        let provider_name = agent.provider.as_deref()
            .ok_or_else(|| anyhow!("Agent '{}' has no provider configured", agent.name))?;

        let provider = providers::create_provider_with_model(
            provider_name,
            agent.model.as_deref(),
            &self.mithril_config,
        )?;

        // Build system prompt with protocol
        let system = build_agent_system_prompt(agent, &self.config.agents);

        // Build context from history
        let context_window = self.config.controller.context_window as usize;
        let recent: Vec<String> = self.context_history.iter()
            .rev().take(context_window).rev()
            .map(|r| format!("[{}]: {}", r.agent_name, r.response_summary))
            .collect();

        let mut messages = vec![ChatMessage::system(&system)];
        if !recent.is_empty() {
            messages.push(ChatMessage::system(format!("Recent context:\n{}", recent.join("
"))));
        }
        messages.push(ChatMessage::user(task));

        // Use agentic loop if agent has tools
        let has_tools = agent.tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false);

        let response = if has_tools {
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string());
            let registry = crate::tools::create_default_registry(&cwd);
            let tool_defs = if self.plan_mode {
                // Plan mode: only read-only tools
                const PLAN_TOOLS: &[&str] = &[
                    "read_psi", "list_files", "grep_files", "find_file", "file_stats",
                    "git_status", "git_log", "git_diff", "git_blame", "git_branch",
                    "search_symbols", "document_outline", "web_search", "fetch_page", "lore_read",
                ];
                flow::build_tool_defs(&registry).into_iter()
                    .filter(|t| PLAN_TOOLS.contains(&t.name.as_str()))
                    .collect()
            } else {
                flow::build_tool_defs(&registry)
            };

            let result = agent_loop::run_agentic_loop(
                provider.as_ref(),
                &mut messages,
                &tool_defs,
                &registry,
                10,
                TraceMode::Silent, // agent's internal tool loop is silent — we show our own trace
            ).await?;

            // Record tool calls in trace
            for tc in &result.tool_calls {
                let preview = if tc.output.len() > 60 { format!("{}...", &tc.output[..60]) } else { tc.output.clone() };
                self.trace.push(TraceEntry::ToolCall {
                    name: tc.name.clone(),
                    success: tc.success,
                    preview: preview.replace('\n', " "),
                });
            }

            let usage = TokenUsage::estimate(task, &result.response);
            self.tokens.record(&agent.name, &usage);
            result.response
        } else {
            let response = provider.chat(&messages).await?;
            let usage = TokenUsage::estimate(task, &response);
            self.tokens.record(&agent.name, &usage);
            response
        };

        Ok(response)
    }

    async fn execute_external(&mut self, agent: &FellowshipAgent, task: &str) -> Result<String> {
        let binary = agent.binary.as_deref()
            .ok_or_else(|| anyhow!("External agent '{}' has no binary configured", agent.name))?;

        let mut cmd = std::process::Command::new(binary);
        if let Some(ref args) = agent.args {
            for arg in args {
                cmd.arg(arg);
            }
        }
        cmd.arg(task);

        let output = tokio::task::spawn_blocking(move || cmd.output()).await??;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("{} failed: {}", agent.name, stderr);
        }

        let usage = TokenUsage::estimate(task, &stdout);
        self.tokens.record(&agent.name, &usage);
        Ok(stdout)
    }

    /// Execute an intermediate `AgentType::Decision` agent (`Route`, `Gate`, or `Consult`).
    /// Never returns a direct response to the user; always computes a `DecisionTransition`
    /// to either a target in `can_call` or back to the calling agent.
    async fn execute_decision_agent(
        &mut self,
        agent: &FellowshipAgent,
        task: &str,
        previous_agent: Option<&str>,
    ) -> Result<DecisionTransition> {
        let provider_name = agent.provider.as_deref()
            .unwrap_or(&self.config.controller.provider);
        let model_override = agent.model.as_deref().or_else(|| {
            if provider_name == "local" {
                self.config.controller.effective_model()
            } else {
                None
            }
        });

        let provider_info = format!("{}/{} [decision]", provider_name, model_override.unwrap_or("default"));
        self.trace.push(TraceEntry::AgentStart {
            agent: agent.name.clone(),
            provider: provider_info.clone(),
        });

        if self.trace_mode == TraceMode::Full {
            eprintln!("  {} {} ({})", "▶".dimmed(), agent.name.cyan(), provider_info.dimmed());
        }

        let provider = providers::create_provider_with_model(
            provider_name,
            model_override,
            &self.mithril_config,
        )?;

        let context_window = self.config.controller.context_window as usize;
        let recent: Vec<String> = self.context_history.iter()
            .rev().take(context_window).rev()
            .map(|r| format!("[{}]: {}", r.agent_name, r.response_summary))
            .collect();

        let state = if recent.is_empty() {
            task.to_string()
        } else {
            format!("{}\n\nCurrent input:\n{}", recent.join("\n"), task)
        };

        // Determine effective mode (fall back to Consult if Route/Gate has empty can_call)
        let effective_mode = match agent.effective_decision_mode() {
            DecisionMode::Route | DecisionMode::Gate if agent.can_call.is_empty() => DecisionMode::Consult,
            mode => mode,
        };

        let raw_verdict = match effective_mode {
            DecisionMode::Route => {
                let candidates = self.build_route_candidates(agent);
                provider.decide_choice(&agent.role, &state, &candidates).await?
            }
            DecisionMode::Gate => {
                let criteria = Self::build_gate_criteria(agent);
                provider.decide_choice(&agent.role, &state, &criteria).await?
            }
            DecisionMode::Consult => {
                if let Some(ref choices) = agent.choices {
                    if !choices.is_empty() {
                        provider.decide_choice(&agent.role, &state, choices).await?
                    } else {
                        let messages = vec![
                            ChatMessage::system(format!("{}\n\n{}", agent.role, recent.join("\n")).trim().to_string()),
                            ChatMessage::user(task),
                        ];
                        provider.chat(&messages).await?
                    }
                } else if let Some((question, inline_choices)) = parse_inline_choices(task) {
                    provider.decide_choice(&question, &state, &inline_choices).await?
                } else {
                    let messages = vec![
                        ChatMessage::system(format!("{}\n\n{}", agent.role, recent.join("\n")).trim().to_string()),
                        ChatMessage::user(task),
                    ];
                    provider.chat(&messages).await?
                }
            }
        };

        let usage = TokenUsage::estimate(&state, &raw_verdict);
        self.tokens.record(&agent.name, &usage);

        Ok(self.resolve_decision_transition(agent, task, &raw_verdict, previous_agent))
    }

    /// Build the candidate map for a `DecisionMode::Route` agent from its `can_call` list and optional `choices`.
    pub fn build_route_candidates(&self, agent: &FellowshipAgent) -> std::collections::HashMap<String, String> {
        let mut candidates = std::collections::HashMap::new();
        for target_name in &agent.can_call {
            if let Some(desc) = agent.choices.as_ref().and_then(|c| c.get(target_name)) {
                candidates.insert(target_name.clone(), desc.clone());
            } else if let Some(target_agent) = self.config.get_agent(target_name) {
                let desc = target_agent.when.as_deref().unwrap_or(&target_agent.role);
                candidates.insert(target_name.clone(), desc.to_string());
            } else {
                candidates.insert(target_name.clone(), target_name.clone());
            }
        }
        candidates
    }

    /// Build the pass/retry criteria map for a `DecisionMode::Gate` agent.
    pub fn build_gate_criteria(agent: &FellowshipAgent) -> std::collections::HashMap<String, String> {
        if let Some(ref choices) = agent.choices {
            if !choices.is_empty() {
                return choices.clone();
            }
        }
        let mut criteria = std::collections::HashMap::new();
        criteria.insert(
            "pass".to_string(),
            "Output satisfies criteria and is ready to proceed".to_string(),
        );
        criteria.insert(
            "retry".to_string(),
            "Output needs revision, is incomplete, or fails criteria".to_string(),
        );
        criteria
    }

    /// Pure deterministic resolution of a `Decision` agent's transition given its raw JEV verdict.
    /// Guarantees that `Decision` agents never terminate the loop to answer the user directly.
    pub fn resolve_decision_transition(
        &self,
        agent: &FellowshipAgent,
        task: &str,
        raw_verdict: &str,
        previous_agent: Option<&str>,
    ) -> DecisionTransition {
        let verdict = raw_verdict.trim().to_string();
        let verdict_lower = verdict.to_lowercase();

        let fallback_caller = || -> String {
            if let Some(prev) = previous_agent {
                if !prev.is_empty() {
                    return prev.to_string();
                }
            }
            self.config.agents.iter()
                .find(|a| a.agent_type != AgentType::Decision)
                .or_else(|| self.config.agents.first())
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "worker".to_string())
        };

        let effective_mode = match agent.effective_decision_mode() {
            DecisionMode::Route | DecisionMode::Gate if agent.can_call.is_empty() => DecisionMode::Consult,
            mode => mode,
        };

        match effective_mode {
            DecisionMode::Route => {
                let chosen = agent.can_call.iter()
                    .find(|target| verdict_lower.contains(&target.to_lowercase()))
                    .cloned()
                    .unwrap_or_else(|| agent.can_call[0].clone());

                DecisionTransition {
                    next_agent: chosen,
                    next_task: task.to_string(),
                    mode: "route".to_string(),
                    verdict,
                }
            }
            DecisionMode::Gate => {
                // Check if verdict explicitly names a target agent in can_call
                let direct_target = agent.can_call.iter()
                    .find(|target| verdict_lower == target.to_lowercase())
                    .cloned();

                let is_rejected = verdict_lower.contains("retry")
                    || verdict_lower.contains("reject")
                    || verdict_lower.contains("fail")
                    || verdict_lower.contains("no");

                let is_passed = direct_target.is_some()
                    || (!is_rejected && (
                        verdict_lower.contains("pass")
                        || verdict_lower.contains("approve")
                        || verdict_lower.contains("accept")
                        || verdict_lower.contains("ok")
                        || verdict_lower.contains("yes")
                    ));

                if is_passed {
                    let next_agent = direct_target.unwrap_or_else(|| agent.can_call[0].clone());
                    DecisionTransition {
                        next_agent,
                        next_task: task.to_string(),
                        mode: "gate".to_string(),
                        verdict: "pass".to_string(),
                    }
                } else {
                    let caller = fallback_caller();
                    let next_task = format!(
                        "Gate '{}' rejected output (verdict: {}). Criteria: {}\n\nPlease revise and try again.",
                        agent.name, verdict, agent.role
                    );
                    DecisionTransition {
                        next_agent: caller,
                        next_task,
                        mode: "gate".to_string(),
                        verdict: if verdict.is_empty() { "retry".to_string() } else { verdict },
                    }
                }
            }
            DecisionMode::Consult => {
                let caller = fallback_caller();
                let next_task = format!(
                    "Result from {}: {}\n\nContinue your task.",
                    agent.name, verdict
                );
                DecisionTransition {
                    next_agent: caller,
                    next_task,
                    mode: "consult".to_string(),
                    verdict,
                }
            }
        }
    }

    /// Execute a zero-shot decision using the Fellowship controller (JEV model).
    /// Supports both structured choice decisions (when `CHOICES:` / `OPTIONS:` are provided in `task`)
    /// and direct `<evidence>` zero-shot decisions.
    async fn execute_controller_decision(&mut self, task: &str) -> Result<String> {
        let controller = providers::create_provider_with_model(
            &self.config.controller.provider,
            self.config.controller.effective_model(),
            &self.mithril_config,
        )?;

        let context_window = self.config.controller.context_window as usize;
        let recent_evidence: String = self.context_history.iter()
            .rev().take(context_window).rev()
            .map(|r| format!("[{}]: {}", r.agent_name, r.response_summary))
            .collect::<Vec<_>>()
            .join("\n");

        let response = if let Some((question, choices)) = parse_inline_choices(task) {
            let state = if recent_evidence.is_empty() {
                question.clone()
            } else {
                format!("{}\n\n{}", recent_evidence, question)
            };
            controller.decide_choice(&question, &state, &choices).await?
        } else {
            let evidence = if recent_evidence.is_empty() {
                "Evaluate the request and provide a direct, concise decision verdict.".to_string()
            } else {
                recent_evidence
            };
            let messages = vec![
                ChatMessage::system(evidence),
                ChatMessage::user(task),
            ];
            controller.chat(&messages).await?
        };

        let usage = TokenUsage::estimate(task, &response);
        self.tokens.record("controller", &usage);
        Ok(response.trim().to_string())
    }

    #[allow(dead_code)]
    async fn execute_gguf(&mut self, task: &str) -> Result<String> {
        self.execute_controller_decision(task).await
    }

    #[allow(dead_code)]
    pub fn token_usage(&self) -> &SessionTokens {
        &self.tokens
    }

    /// Get the current round count
    #[allow(dead_code)]
    pub fn current_round(&self) -> u32 {
        self.rounds
    }

    /// Get the max rounds from config
    pub fn max_rounds(&self) -> u32 {
        self.config.max_rounds
    }
}

// ── Protocol parsing ─────────────────────────────────────────────────────────

fn parse_agent_directive(output: &str) -> Directive {
    let lines: Vec<&str> = output.lines().collect();
    
    // Find the last NEXT: line
    let next_idx = lines.iter().rposition(|l| l.trim().starts_with("NEXT:"));
    let next_idx = match next_idx {
        Some(i) => i,
        None => return Directive::None,
    };

    let next_value = lines[next_idx].trim().strip_prefix("NEXT:").unwrap_or("").trim();

    if next_value.eq_ignore_ascii_case("DONE") {
        // Look for RESPONSE: anywhere in the output
        let response_idx = lines.iter().position(|l| l.trim().starts_with("RESPONSE:"));

        let response = if let Some(r_i) = response_idx {
            // Take RESPONSE: content + everything after it (excluding NEXT: lines)
            let first = lines[r_i].trim().strip_prefix("RESPONSE:").unwrap_or("").trim();
            let mut parts: Vec<&str> = vec![first];
            for line in &lines[r_i + 1..] {
                if !line.trim().starts_with("NEXT:") {
                    parts.push(line);
                }
            }
            parts.join("
").trim().to_string()
        } else {
            // No RESPONSE: — take everything BEFORE the NEXT: line
            lines[..next_idx].join("
").trim().to_string()
        };

        Directive::Done(response)
    } else if !next_value.is_empty() {
        // Agent delegation — look for TASK: anywhere in output
        let task_idx = lines.iter().position(|l| l.trim().starts_with("TASK:"));

        let task = if let Some(t_i) = task_idx {
            // Take TASK: content + everything after it (excluding NEXT: lines)
            let first = lines[t_i].trim().strip_prefix("TASK:").unwrap_or("").trim();
            let mut parts: Vec<&str> = vec![first];
            for line in &lines[t_i + 1..] {
                if !line.trim().starts_with("NEXT:") {
                    parts.push(line);
                }
            }
            parts.join("
").trim().to_string()
        } else {
            // No TASK: — use full output
            output.to_string()
        };

        Directive::CallAgent { name: next_value.to_lowercase(), task }
    } else {
        Directive::None
    }
}

// ── Controller & Decision helpers ────────────────────────────────────────────

/// Returns true if the delegation target refers to the Fellowship JEV decision controller.
pub fn is_controller_target(name: &str) -> bool {
    matches!(name.trim().to_lowercase().as_str(), "controller" | "jev" | "gguf")
}

/// Parse optional inline choices from a `TASK:` payload.
/// Format:
/// ```text
/// Question or context here
/// CHOICES:
/// - yes: Criteria for yes
/// - no: Criteria for no
/// ```
fn parse_inline_choices(task: &str) -> Option<(String, std::collections::HashMap<String, String>)> {
    let lines: Vec<&str> = task.lines().collect();
    let header_idx = lines.iter().position(|l| {
        let t = l.trim().to_uppercase();
        t == "CHOICES:" || t == "OPTIONS:"
    })?;

    let question = lines[..header_idx].join("\n").trim().to_string();
    let mut choices = std::collections::HashMap::new();
    for line in &lines[header_idx + 1..] {
        let trimmed = line.trim().trim_start_matches('-').trim_start_matches('*').trim();
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_string();
            let val = v.trim().to_string();
            if !key.is_empty() && !val.is_empty() {
                choices.insert(key, val);
            }
        }
    }

    if choices.is_empty() {
        None
    } else {
        Some((if question.is_empty() { task.to_string() } else { question }, choices))
    }
}

// ── System prompt builder ────────────────────────────────────────────────────

fn build_agent_system_prompt(agent: &FellowshipAgent, all_agents: &[FellowshipAgent]) -> String {
    let can_call_desc: Vec<String> = agent.can_call.iter()
        .filter_map(|name| {
            if is_controller_target(name) {
                Some(format!("  - {} — fast zero-shot JEV decision oracle (pass question/evidence in TASK: to get an immediate verdict)", name))
            } else {
                all_agents.iter()
                    .find(|a| &a.name == name)
                    .map(|a| {
                        if a.agent_type == AgentType::Decision {
                            let mode_label = match a.effective_decision_mode() {
                                DecisionMode::Route => "decision: route",
                                DecisionMode::Gate => "decision: gate",
                                DecisionMode::Consult => "decision: consult",
                            };
                            format!("  - {} [{}] — {}", a.name, mode_label, a.role)
                        } else {
                            format!("  - {} — {}", a.name, a.role)
                        }
                    })
            }
        })
        .collect();

    let can_call_section = if can_call_desc.is_empty() {
        "You cannot call other agents. Complete the task yourself.".to_string()
    } else {
        format!("You can delegate to:\n{}", can_call_desc.join("\n"))
    };

    format!(
        "You are '{}'. {}\n\n\
         ## Communication Protocol\n\
         When you finish your work, end your response with ONE of these:\n\n\
         If the task is COMPLETE and ready for the user:\n\
         NEXT: DONE\n\
         RESPONSE: your final answer here\n\n\
         If you need another agent to continue:\n\
         NEXT: agent_name\n\
         TASK: clear instructions for that agent\n\n\
         If you don't include NEXT:, your entire response goes directly to the user.\n\n\
         ## Available Agents\n\
         {}\n\n\
         ## Rules\n\
         - Always include relevant tool output in your response. Show the actual data, not just a summary.\n\
         - When a tool returns results the user asked for, include them verbatim in your RESPONSE.\n\
         - Use NEXT: controller (or NEXT: jev) for fast zero-shot decisions or triage\n\
         - Only call agents you actually need\n\
         - Be concise in TASK: descriptions\n\
         - If you can complete the task yourself, do it and say NEXT: DONE\n\
\
         ## Output Formatting\n\
         You are running inside a terminal. Do NOT use markdown.\n\
         No code fences, no bold markers, no heading markers.\n\
         Use plain text. For lists use bullet points. For code indent with spaces.",
        agent.name, agent.role, can_call_section
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_directive_done_with_response() {
        let output = "I've completed the task.\nNEXT: DONE\nRESPONSE: The bug has been fixed.";
        match parse_agent_directive(output) {
            Directive::Done(r) => assert_eq!(r, "The bug has been fixed."),
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_parse_directive_done_without_response() {
        let output = "Here is my analysis of the code.\n\nThe architecture looks solid.\nNEXT: DONE";
        match parse_agent_directive(output) {
            Directive::Done(r) => assert!(r.contains("architecture looks solid")),
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_parse_directive_call_agent() {
        let output = "I need the coder to implement this.\nNEXT: coder\nTASK: implement JWT auth in auth.rs";
        match parse_agent_directive(output) {
            Directive::CallAgent { name, task } => {
                assert_eq!(name, "coder");
                assert_eq!(task, "implement JWT auth in auth.rs");
            }
            _ => panic!("Expected CallAgent"),
        }
    }

    #[test]
    fn test_parse_directive_call_gguf() {
        let output = "Let me delegate this formatting.\nNEXT: gguf\nTASK: format this as markdown bullet points";
        match parse_agent_directive(output) {
            Directive::CallAgent { name, task } => {
                assert_eq!(name, "gguf");
                assert!(task.contains("markdown"));
            }
            _ => panic!("Expected CallAgent with gguf"),
        }
    }

    #[test]
    fn test_parse_directive_none() {
        let output = "Here is my direct answer to the user. No delegation needed.";
        match parse_agent_directive(output) {
            Directive::None => {}
            _ => panic!("Expected None"),
        }
    }

    #[test]
    fn test_parse_directive_case_insensitive_done() {
        let output = "Done.\nNEXT: done\nRESPONSE: All good.";
        match parse_agent_directive(output) {
            Directive::Done(r) => assert_eq!(r, "All good."),
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_parse_directive_no_task_tag() {
        let output = "Delegating.\nNEXT: reviewer";
        match parse_agent_directive(output) {
            Directive::CallAgent { name, task } => {
                assert_eq!(name, "reviewer");
                // Without TASK: tag, uses full output
                assert!(task.contains("Delegating"));
            }
            _ => panic!("Expected CallAgent"),
        }
    }

    #[test]
    fn test_build_agent_system_prompt_with_can_call() {
        let agents = vec![
            FellowshipAgent {
                name: "coder".to_string(),
                role: "Writes code".to_string(),
                can_call: vec!["reviewer".to_string(), "gguf".to_string()],
                provider: Some("groq".to_string()),
                model: None, agent_type: AgentType::Provider,
                mode: None, choices: None,
                binary: None, args: None, when: None, tools: None,
            },
            FellowshipAgent {
                name: "reviewer".to_string(),
                role: "Reviews code".to_string(),
                can_call: vec![],
                provider: Some("gemini".to_string()),
                model: None, agent_type: AgentType::Provider,
                mode: None, choices: None,
                binary: None, args: None, when: None, tools: None,
            },
        ];
        let prompt = build_agent_system_prompt(&agents[0], &agents);
        assert!(prompt.contains("coder"));
        assert!(prompt.contains("reviewer — Reviews code"));
        assert!(prompt.contains("gguf"));
        assert!(prompt.contains("NEXT: DONE"));
        assert!(prompt.contains("NEXT: agent_name"));
    }

    #[test]
    fn test_build_agent_system_prompt_no_can_call() {
        let agents = vec![
            FellowshipAgent {
                name: "terminal".to_string(),
                role: "Final step".to_string(),
                can_call: vec![],
                provider: Some("gemini".to_string()),
                model: None, agent_type: AgentType::Provider,
                mode: None, choices: None,
                binary: None, args: None, when: None, tools: None,
            },
        ];
        let prompt = build_agent_system_prompt(&agents[0], &agents);
        assert!(prompt.contains("cannot call other agents"));
    }

    #[test]
    fn test_directive_debug() {
        let done = Directive::Done("test".to_string());
        let call = Directive::CallAgent { name: "agent".to_string(), task: "task".to_string() };
        let none = Directive::None;
        // Just ensure Debug is implemented and doesn't panic
        let _ = format!("{:?}", done);
        let _ = format!("{:?}", call);
        let _ = format!("{:?}", none);
    }

    #[test]
    fn test_directive_clone() {
        let call = Directive::CallAgent { name: "agent".to_string(), task: "task".to_string() };
        let cloned = call.clone();
        match cloned {
            Directive::CallAgent { name, task } => {
                assert_eq!(name, "agent");
                assert_eq!(task, "task");
            }
            _ => panic!("Expected CallAgent"),
        }
    }

    #[test]
    fn test_round_entry_debug_clone() {
        let entry = RoundEntry {
            agent_name: "worker".to_string(),
            task: "do something".to_string(),
            response_summary: "done".to_string(),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.agent_name, "worker");
        let _ = format!("{:?}", entry);
    }

    #[test]
    fn test_trace_entry_variants() {
        let entries = vec![
            TraceEntry::Entry { agent: "worker".to_string() },
            TraceEntry::AgentStart { agent: "worker".to_string(), provider: "gemini".to_string() },
            TraceEntry::ToolCall { name: "read_psi".to_string(), success: true, preview: "...".to_string() },
            TraceEntry::Delegation { from: "worker".to_string(), to: "reviewer".to_string(), task_preview: "review".to_string() },
            TraceEntry::GgufCall { task_preview: "format".to_string() },
            TraceEntry::DecisionStep { agent: "router".to_string(), mode: "route".to_string(), outcome: "pass -> reviewer".to_string() },
            TraceEntry::Done { agent: "worker".to_string() },
            TraceEntry::BudgetWarning { used: 1000, limit: 5000 },
        ];
        for entry in entries {
            let cloned = entry.clone();
            let _ = format!("{:?}", cloned);
        }
    }

    #[test]
    fn test_orchestrator_result_debug() {
        let result = OrchestratorResult {
            response: "done".to_string(),
            rounds: 3,
            agents_involved: vec!["worker".to_string()],
            tokens: SessionTokens::new(),
            trace: vec![],
        };
        let _ = format!("{:?}", result);
    }

    #[test]
    fn test_parse_directive_with_prose_before_next() {
        // Sometimes agents might mention NEXT: in prose before the actual directive
        let output = "I'll use the NEXT: protocol as instructed.\n\nAnalysis complete.\nNEXT: DONE\nRESPONSE: The code looks good.";
        match parse_agent_directive(output) {
            Directive::Done(r) => assert_eq!(r, "The code looks good."),
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_parse_directive_multiline_task() {
        let output = "Delegating.\nNEXT: coder\nTASK: implement this feature";
        match parse_agent_directive(output) {
            Directive::CallAgent { name, task } => {
                assert_eq!(name, "coder");
                assert_eq!(task, "implement this feature");
            }
            _ => panic!("Expected CallAgent"),
        }
    }
    #[test]
    fn test_parse_directive_multiline_response() {
        let output = "Here are the files:\n- src/main.rs\n- src/lib.rs\n\nNEXT: DONE";
        match parse_agent_directive(output) {
            Directive::Done(r) => {
                assert!(r.contains("src/main.rs"));
                assert!(r.contains("src/lib.rs"));
            }
            _ => panic!("Expected Done with multiline response"),
        }
    }

    #[test]
    fn test_parse_directive_empty_response() {
        let output = "NEXT: DONE";
        match parse_agent_directive(output) {
            Directive::Done(r) => assert!(r.is_empty()),
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_parse_directive_response_after_next() {
        let output = "NEXT: DONE\nRESPONSE: The fix is applied.";
        match parse_agent_directive(output) {
            Directive::Done(r) => assert_eq!(r, "The fix is applied."),
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_parse_directive_multiline_response_after_next() {
        let output = "NEXT: DONE\nRESPONSE: Here are the results:\n- Item 1\n- Item 2";
        match parse_agent_directive(output) {
            Directive::Done(r) => {
                assert!(r.contains("Item 1"));
                assert!(r.contains("Item 2"));
            }
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_parse_directive_task_multiline() {
        let output = "I need help.\nNEXT: worker\nTASK: Do these things:\n1. Read the file\n2. Fix the bug";
        match parse_agent_directive(output) {
            Directive::CallAgent { name, task } => {
                assert_eq!(name, "worker");
                assert!(task.contains("Read the file"));
                assert!(task.contains("Fix the bug"));
            }
            _ => panic!("Expected CallAgent"),
        }
    }

    #[test]
    fn test_parse_directive_prose_before_next_multiline() {
        let output = "I analyzed the code and found 3 issues.\nThe main problem is in auth.rs.\n\nHere is my fix:\n- Changed line 42\n- Added validation\n\nNEXT: DONE";
        match parse_agent_directive(output) {
            Directive::Done(r) => {
                assert!(r.contains("3 issues"));
                assert!(r.contains("Changed line 42"));
            }
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_parse_directive_task_multiline_bullets() {
        let output = "NEXT: reviewer\nTASK: Review auth.rs for:\n- SQL injection\n- XSS vulnerabilities";
        match parse_agent_directive(output) {
            Directive::CallAgent { name, task } => {
                assert_eq!(name, "reviewer");
                assert!(task.contains("SQL injection"));
                assert!(task.contains("XSS"));
            }
            _ => panic!("Expected CallAgent"),
        }
    }

    #[test]
    fn test_entry_candidate_filtering_and_mentions() {
        let yaml = r#"
name: "test-fellowship"
controller:
  provider: local
  model: jevos-v4
agents:
  - name: "coder"
    provider: gemini
    role: "Writes code"
    when: "coding tasks"
  - name: "quality_gate"
    type: decision
    mode: gate
    provider: local
    model: jevos-v4
    role: "Verify tests"
    can_call: ["coder"]
  - name: "oracle"
    type: decision
    mode: consult
    provider: local
    model: jevos-v4
    role: "Answers questions"
"#;
        let config: FellowshipConfig = serde_yaml::from_str(yaml).unwrap();
        let orch = Orchestrator::new(config, MithrilConfig::default(), TraceMode::Silent);

        let candidates = orch.build_entry_candidates();
        assert!(candidates.contains_key("coder"));
        assert!(!candidates.contains_key("quality_gate"));
        assert!(!candidates.contains_key("oracle"));

        // #agent mentions should allow entry-eligible agents and ignore internal-only decision agents
        assert_eq!(orch.extract_agent_mention("#coder fix bug"), Some("coder".to_string()));
        assert_eq!(orch.extract_agent_mention("#quality_gate check"), None);
        assert_eq!(orch.extract_agent_mention("#oracle question"), None);
    }

    #[test]
    fn test_controller_targets_and_inline_choices() {
        assert!(is_controller_target("controller"));
        assert!(is_controller_target("jev"));
        assert!(is_controller_target("gguf"));
        assert!(!is_controller_target("coder"));

        let task = "Should we refactor this module?\nCHOICES:\n- yes: High complexity and duplicated logic\n- no: Already clean and tested";
        let (question, choices) = parse_inline_choices(task).unwrap();
        assert_eq!(question, "Should we refactor this module?");
        assert_eq!(choices.get("yes").map(String::as_str), Some("High complexity and duplicated logic"));
        assert_eq!(choices.get("no").map(String::as_str), Some("Already clean and tested"));
    }

    #[test]
    fn test_decision_agent_route_mode_and_fallback() {
        let yaml = r#"
name: "route-test"
controller:
  provider: local
  model: jevos-v4
agents:
  - name: "coder"
    provider: gemini
    role: "Writes code"
    can_call: ["triage_router"]
  - name: "triage_router"
    type: decision
    mode: route
    provider: local
    model: jevos-v4
    role: "Route based on whether security or performance review is needed"
    can_call: ["sec_reviewer", "perf_reviewer"]
  - name: "sec_reviewer"
    provider: gemini
    role: "Security audit"
    when: "auth, crypto, or SQL changes"
  - name: "perf_reviewer"
    provider: gemini
    role: "Performance optimization"
    when: "hot loops, allocations, or latency"
"#;
        let config: FellowshipConfig = serde_yaml::from_str(yaml).unwrap();
        let orch = Orchestrator::new(config.clone(), MithrilConfig::default(), TraceMode::Silent);
        let router = config.get_agent("triage_router").unwrap();

        let candidates = orch.build_route_candidates(router);
        assert_eq!(candidates.get("sec_reviewer").map(String::as_str), Some("auth, crypto, or SQL changes"));
        assert_eq!(candidates.get("perf_reviewer").map(String::as_str), Some("hot loops, allocations, or latency"));

        // Selecting perf_reviewer
        let t1 = orch.resolve_decision_transition(router, "Check loop allocations", "perf_reviewer", Some("coder"));
        assert_eq!(t1.next_agent, "perf_reviewer");
        assert_eq!(t1.next_task, "Check loop allocations");
        assert_eq!(t1.mode, "route");

        // Unexpected model output falls back deterministically to can_call[0] ("sec_reviewer")
        let t2 = orch.resolve_decision_transition(router, "Check unknown", "unexpected_choice", None);
        assert_eq!(t2.next_agent, "sec_reviewer");
        assert_eq!(t2.mode, "route");
    }

    #[test]
    fn test_decision_agent_gate_mode_pass_and_retry() {
        let yaml = r#"
name: "gate-test"
controller:
  provider: local
  model: jevos-v4
agents:
  - name: "coder"
    provider: gemini
    role: "Writes code"
    can_call: ["quality_gate"]
  - name: "quality_gate"
    type: decision
    mode: gate
    provider: local
    model: jevos-v4
    role: "Verify tests are included and passing"
    choices:
      pass: "Includes unit tests"
      retry: "Missing unit tests"
    can_call: ["reviewer"]
  - name: "reviewer"
    provider: gemini
    role: "Final review"
"#;
        let config: FellowshipConfig = serde_yaml::from_str(yaml).unwrap();
        let orch = Orchestrator::new(config.clone(), MithrilConfig::default(), TraceMode::Silent);
        let gate = config.get_agent("quality_gate").unwrap();

        // Pass transitions forward to can_call[0] ("reviewer")
        let pass_transition = orch.resolve_decision_transition(
            gate,
            "Implemented feature with 4 unit tests",
            "pass",
            Some("coder"),
        );
        assert_eq!(pass_transition.next_agent, "reviewer");
        assert_eq!(pass_transition.next_task, "Implemented feature with 4 unit tests");
        assert_eq!(pass_transition.mode, "gate");
        assert_eq!(pass_transition.verdict, "pass");

        // Retry bounces back to caller ("coder") with rejection message
        let retry_transition = orch.resolve_decision_transition(
            gate,
            "Implemented feature without tests",
            "retry",
            Some("coder"),
        );
        assert_eq!(retry_transition.next_agent, "coder");
        assert!(retry_transition.next_task.contains("Gate 'quality_gate' rejected output"));
        assert!(retry_transition.next_task.contains("Verify tests are included and passing"));
        assert_eq!(retry_transition.mode, "gate");
        assert_eq!(retry_transition.verdict, "retry");
    }

    #[test]
    fn test_decision_agent_consult_mode_returns_to_caller() {
        let yaml = r#"
name: "consult-test"
controller:
  provider: local
  model: jevos-v4
agents:
  - name: "architect"
    provider: gemini
    role: "Designs architecture"
    can_call: ["risk_oracle"]
  - name: "risk_oracle"
    type: decision
    mode: consult
    provider: local
    model: jevos-v4
    role: "Classify migration risk level"
    choices:
      high: "Breaking schema change"
      low: "Additive nullable column"
"#;
        let config: FellowshipConfig = serde_yaml::from_str(yaml).unwrap();
        let orch = Orchestrator::new(config.clone(), MithrilConfig::default(), TraceMode::Silent);
        let oracle = config.get_agent("risk_oracle").unwrap();

        let consult_transition = orch.resolve_decision_transition(
            oracle,
            "Adding nullable column last_login_at",
            "low",
            Some("architect"),
        );
        assert_eq!(consult_transition.next_agent, "architect");
        assert!(consult_transition.next_task.contains("Result from risk_oracle: low"));
        assert_eq!(consult_transition.mode, "consult");
        assert_eq!(consult_transition.verdict, "low");

        // Prompt for architect should annotate risk_oracle with [decision: consult]
        let architect = config.get_agent("architect").unwrap();
        let prompt = build_agent_system_prompt(architect, &config.agents);
        assert!(prompt.contains("risk_oracle [decision: consult]"));
    }

}
