//! Orchestrator — GGUF entry classifier + agent free-flow + GGUF worker.
//!
//! # Flow
//!
//! 1. GGUF classifies user message → picks first agent
//! 2. Agents execute and communicate via NEXT:/TASK: protocol
//! 3. Agents can delegate trivial tasks to GGUF (free local worker)
//! 4. Rust enforces: can_call permissions, max_rounds, token_budget
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
    Done { agent: String },
    BudgetWarning { used: u64, limit: u64 },
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
        let labels: Vec<String> = self.config.agents.iter()
            .filter(|a| a.when.is_some())
            .map(|a| format!("- {} — {}", a.name, a.when.as_deref().unwrap_or(&a.role)))
            .collect();

        let prompt = format!(
            "Which agent should handle this? Reply with ONLY the agent name, nothing else.\n\nAgents:\n{}\n\nMessage: {}",
            labels.join("
"), user_message
        );

        let controller = providers::create_provider_with_model(
            &self.config.controller.provider,
            self.config.controller.model.as_deref(),
            &self.mithril_config,
        )?;

        let response = controller.chat(&[
            ChatMessage::system("You are a classifier. Reply with ONLY one agent name. No explanation."),
            ChatMessage::user(&prompt),
        ]).await?;

        let usage = TokenUsage::estimate(&prompt, &response);
        self.tokens.record("gguf", &usage);

        // Find which agent name appears in the response
        let resp_lower = response.trim().to_lowercase();
        let matched = self.config.agents.iter()
            .find(|a| resp_lower.contains(&a.name.to_lowercase()))
            .map(|a| a.name.clone());

        Ok(matched.unwrap_or_else(|| self.config.agents[0].name.clone()))
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
    /// Returns Some(agent_name) if a valid agent is mentioned, None otherwise.
    fn extract_agent_mention(&self, message: &str) -> Option<String> {
        let trimmed = message.trim();
        if !trimmed.starts_with('#') {
            return None;
        }
        // Extract the word after #
        let mention = trimmed[1..].split_whitespace().next()?;
        let mention_lower = mention.to_lowercase();

        // Check if it matches any configured agent
        self.config.agents.iter()
            .find(|a| a.name.to_lowercase() == mention_lower)
            .map(|a| a.name.clone())
    }

    async fn run_agent_loop(&mut self, first_agent: &str, initial_task: &str) -> Result<String> {
        let mut current_agent_name = first_agent.to_string();
        let mut current_task = initial_task.to_string();

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

            // Execute
            let result = if current_agent_name == "gguf" {
                self.execute_gguf(&current_task).await?
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
                    // Permission check
                    if name != "gguf" {
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

                    if name == "gguf" {
                        // GGUF worker: execute and return result to CALLING agent
                        self.trace.push(TraceEntry::GgufCall { task_preview: task_preview.clone() });

                        let gguf_result = self.execute_gguf(&task).await?;
                        // Feed back to same agent with gguf result as new task
                        current_task = format!("Result from gguf: {}\n\nContinue your previous task.", gguf_result);
                        // Don't change current_agent_name — stays with the caller
                    } else {
                        // Agent-to-agent delegation
                        self.trace.push(TraceEntry::Delegation {
                            from: current_agent_name.clone(),
                            to: name.clone(),
                            task_preview: task_preview.clone(),
                        });

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
            AgentType::Provider => format!("{}/{}", 
                agent.provider.as_deref().unwrap_or("?"),
                agent.model.as_deref().unwrap_or("default")),
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
            AgentType::Provider => self.execute_provider_agent(agent, task).await,
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

    async fn execute_gguf(&mut self, task: &str) -> Result<String> {
        let controller = providers::create_provider_with_model(
            &self.config.controller.provider,
            self.config.controller.model.as_deref(),
            &self.mithril_config,
        )?;

        // GGUF worker has access to tools for simple file operations
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        let registry = crate::tools::create_default_registry(&cwd);
        let tool_defs = if self.plan_mode {
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

        let mut messages = vec![
            ChatMessage::system("You are a local assistant. Complete the task concisely. You have access to file tools."),
            ChatMessage::user(task),
        ];

        let result = agent_loop::run_agentic_loop(
            controller.as_ref(),
            &mut messages,
            &tool_defs,
            &registry,
            5, // max 5 iterations for GGUF (it's for simple tasks)
            TraceMode::Silent,
        ).await?;

        let usage = TokenUsage::estimate(task, &result.response);
        self.tokens.record("gguf", &usage);
        Ok(result.response)
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

// ── System prompt builder ────────────────────────────────────────────────────

fn build_agent_system_prompt(agent: &FellowshipAgent, all_agents: &[FellowshipAgent]) -> String {
    let can_call_desc: Vec<String> = agent.can_call.iter()
        .filter_map(|name| {
            if name == "gguf" {
                Some("  - gguf — free local model for trivial tasks (formatting, simple edits, quick answers)".to_string())
            } else {
                all_agents.iter()
                    .find(|a| &a.name == name)
                    .map(|a| format!("  - {} — {}", a.name, a.role))
            }
        })
        .collect();

    let can_call_section = if can_call_desc.is_empty() {
        "You cannot call other agents. Complete the task yourself.".to_string()
    } else {
        format!("You can delegate to:\n{}", can_call_desc.join("
"))
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
         - Use NEXT: gguf for trivial sub-tasks (saves money)\n\
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
                binary: None, args: None, when: None, tools: None,
            },
            FellowshipAgent {
                name: "reviewer".to_string(),
                role: "Reviews code".to_string(),
                can_call: vec![],
                provider: Some("gemini".to_string()),
                model: None, agent_type: AgentType::Provider,
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

}
