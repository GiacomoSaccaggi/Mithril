//! Fellowship — multi-agent orchestration config with JEV decision controller.
//!
//! Architecture:
//! - JEV decision controller (`jevos-v4`) routes entry requests and acts as a fast zero-shot oracle
//! - Agents communicate via NEXT:/TASK: protocol for free-flow delegation
//! - Intermediate `type: decision` JEV agents act as routers (`mode: route`), quality gates (`mode: gate`), or consultants (`mode: consult`) without ever speaking directly to the user
//! - Rust enforces: can_call permissions, max_rounds, token_budget

#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Top-level fellowship configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FellowshipConfig {
    /// Human-readable name
    pub name: String,

    /// Human-readable description
    #[serde(default)]
    pub description: Option<String>,

    /// GGUF controller: local model for entry classification + cheap worker
    pub controller: ControllerConfig,

    /// Maximum rounds before stopping (safety limit)
    #[serde(default = "default_max_rounds")]
    pub max_rounds: u32,

    /// Token budget for the entire session (optional)
    #[serde(default)]
    pub token_budget: Option<u64>,

    /// Available agents
    pub agents: Vec<FellowshipAgent>,

    /// Token tracking settings
    #[serde(default)]
    pub token_tracking: TokenTrackingConfig,
}

/// JEV decision controller configuration — fast model for entry classification and zero-shot decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerConfig {
    /// Provider to use (typically "local" or "jev")
    pub provider: String,

    /// Model override (defaults to "jevos-v4" when provider is "local")
    #[serde(default)]
    pub model: Option<String>,

    /// Number of recent rounds to include in context for agents
    #[serde(default = "default_context_window")]
    pub context_window: u32,
}

impl ControllerConfig {
    /// Returns the effective model name, defaulting to `"jevos-v4"` for the `"local"` provider.
    pub fn effective_model(&self) -> Option<&str> {
        if let Some(ref m) = self.model {
            Some(m.as_str())
        } else if self.provider == "local" {
            Some("jevos-v4")
        } else {
            None
        }
    }
}

fn default_max_rounds() -> u32 { 15 }
fn default_context_window() -> u32 { 2 }

/// An agent in the fellowship.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FellowshipAgent {
    /// Display name (used in NEXT: protocol)
    pub name: String,

    /// Provider for internal/decision agents ("gemini", "groq", "openai", "anthropic", "local", "jev")
    #[serde(default)]
    pub provider: Option<String>,

    /// Model override (e.g. "jevos-v4" for decision agents)
    #[serde(default)]
    pub model: Option<String>,

    /// Agent type: "provider" (API-based), "external" (subprocess), or "decision" (intermediate JEV node)
    #[serde(rename = "type", default)]
    pub agent_type: AgentType,

    /// Operational mode when `agent_type == AgentType::Decision` ("route", "gate", "consult")
    #[serde(default)]
    pub mode: Option<DecisionMode>,

    /// Optional explicit decision choices (key -> criteria description) for `Decision` agents
    #[serde(default)]
    pub choices: Option<HashMap<String, String>>,

    /// Binary path for external agents (e.g. "kiro-cli")
    #[serde(default)]
    pub binary: Option<String>,

    /// Arguments for external agents
    #[serde(default)]
    pub args: Option<Vec<String>>,

    /// Description of what this agent does (or decision criteria for `Decision` agents)
    pub role: String,

    /// When this agent should be selected (used by JEV entry classifier)
    #[serde(default)]
    pub when: Option<String>,

    /// Which agents this agent can call (+ "controller" / "jev" / "gguf" is always allowed)
    #[serde(default)]
    pub can_call: Vec<String>,

    /// MCP tools this agent is allowed to use (None = all, empty = none)
    #[serde(default)]
    pub tools: Option<Vec<String>>,
}

/// Agent type: internal (API/local provider), external (subprocess CLI), or decision (JEV intermediate node).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    #[default]
    Provider,
    External,
    Decision,
}

/// Operational mode for `AgentType::Decision` intermediate agents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DecisionMode {
    /// Dynamically routes to one of the agents in `can_call` via `decide_choice`.
    Route,
    /// Evaluates previous agent output ("pass" vs "retry"): forwards to `can_call[0]` on pass, or returns to caller on retry.
    Gate,
    /// Answers a decision/classification question and returns the verdict to the calling agent.
    Consult,
}

impl FellowshipAgent {
    /// Resolve the operational mode when `agent_type == AgentType::Decision`.
    /// Defaults to `DecisionMode::Route` when `can_call` is non-empty, or `DecisionMode::Consult` when `can_call` is empty.
    pub fn effective_decision_mode(&self) -> DecisionMode {
        if let Some(ref mode) = self.mode {
            return mode.clone();
        }
        if self.can_call.is_empty() {
            DecisionMode::Consult
        } else {
            DecisionMode::Route
        }
    }

    /// Returns true if this agent can be selected as the initial entry agent for a user request
    /// (via `classify_entry` or `#agent` mention).
    /// Internal `Decision` agents in `Consult` or `Gate` mode are strictly agent-to-agent and never entry-eligible.
    /// A `Decision` agent in `Route` mode is only entry-eligible if it has `can_call` targets and an explicit `when` clause.
    pub fn is_entry_eligible(&self) -> bool {
        match self.agent_type {
            AgentType::Provider | AgentType::External => true,
            AgentType::Decision => {
                self.effective_decision_mode() == DecisionMode::Route
                    && !self.can_call.is_empty()
                    && self.when.is_some()
            }
        }
    }
}

/// Token tracking settings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenTrackingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub show_per_agent: bool,
}

fn default_true() -> bool { true }

// ── Config Loading ───────────────────────────────────────────────────────────

impl FellowshipConfig {
    /// Load from a specific path.
    pub fn load_from(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read fellowship config: {}", path.display()))?;
        serde_yaml::from_str(&content)
            .context("Failed to parse fellowship YAML")
    }

    /// Try to find and load fellowship config.
    /// Checks: .mithril/fellowship.yaml (project) → ~/.mithril/fellowship.yaml (global)
    pub fn try_load() -> Option<Self> {
        // Project-level
        let project = Path::new(".mithril/fellowship.yaml");
        if project.exists() {
            return Self::load_from(project).ok();
        }

        // Global
        if let Some(home) = dirs::home_dir() {
            let global = home.join(".mithril/fellowship.yaml");
            if global.exists() {
                return Self::load_from(&global).ok();
            }
        }

        None
    }

    /// Get an agent by name.
    /// Load markdown agent definitions from .mithril/agents/*.md
    /// Each .md file with YAML frontmatter becomes an agent.
    pub fn load_markdown_agents(&mut self) {
        let agents_dir = std::path::Path::new(".mithril/agents");
        if !agents_dir.exists() {
            return;
        }
        let entries = match std::fs::read_dir(agents_dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if let Some(agent) = Self::parse_markdown_agent(&content, &path) {
                // Don't override existing agents
                if !self.agents.iter().any(|a| a.name == agent.name) {
                    self.agents.push(agent);
                }
            }
        }
    }

    /// Parse a markdown agent file with YAML frontmatter.
    /// Format:
    /// ```text
    /// ---
    /// description: What this agent does
    /// model: gemini-3.6-flash
    /// provider: gemini
    /// mode: subagent
    /// ---
    /// System prompt content here
    /// ```
    fn parse_markdown_agent(content: &str, path: &std::path::Path) -> Option<FellowshipAgent> {
        if !content.starts_with("---") {
            return None;
        }
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            return None;
        }
        let frontmatter = parts[1].trim();
        let prompt = parts[2].trim();

        // Parse frontmatter as simple key-value
        let mut description = String::new();
        let mut provider = None;
        let mut model = None;
        let mut tools: Option<Vec<String>> = None;

        for line in frontmatter.lines() {
            let line = line.trim();
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');
                match key {
                    "description" => description = value.to_string(),
                    "provider" => provider = Some(value.to_string()),
                    "model" => model = Some(value.to_string()),
                    "tools" => {
                        if value == "*" {
                            tools = Some(vec!["*".to_string()]);
                        } else {
                            tools = Some(value.split(',').map(|s| s.trim().to_string()).collect());
                        }
                    }
                    _ => {}
                }
            }
        }

        // Agent name from filename
        let name = path.file_stem()?.to_str()?.to_string();

        Some(FellowshipAgent {
            name,
            provider,
            model,
            agent_type: AgentType::Provider,
            mode: None,
            choices: None,
            binary: None,
            args: None,
            role: if prompt.is_empty() { description.clone() } else { prompt.to_string() },
            when: Some(description),
            can_call: vec![],
            tools,
        })
    }

    pub fn get_agent(&self, name: &str) -> Option<&FellowshipAgent> {
        self.agents.iter().find(|a| a.name == name)
    }
}

// ── Multi-Fellowship Loading ─────────────────────────────────────────────────

/// List all available fellowship configs from project + global locations.
/// Returns vec of (name, config) pairs. "default" is the unnamed one.
pub fn list_fellowships() -> Vec<(String, FellowshipConfig)> {
    let mut results = Vec::new();

    // Project-level default
    let project_default = Path::new(".mithril/fellowship.yaml");
    if project_default.exists() {
        if let Ok(config) = FellowshipConfig::load_from(project_default) {
            results.push(("default".to_string(), config));
        }
    }

    // Project-level fellowships directory
    let project_dir = Path::new(".mithril/fellowships");
    if project_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(project_dir) {
            let mut files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|ext| ext == "yaml" || ext == "yml").unwrap_or(false))
                .collect();
            files.sort_by_key(|e| e.path());
            for entry in files {
                let stem = entry.path().file_stem().unwrap_or_default().to_string_lossy().to_string();
                if let Ok(config) = FellowshipConfig::load_from(&entry.path()) {
                    results.push((stem, config));
                }
            }
        }
    }

    // Global default
    if let Some(home) = dirs::home_dir() {
        let global_default = home.join(".mithril/fellowship.yaml");
        if global_default.exists() && !results.iter().any(|(n, _)| n == "default") {
            if let Ok(config) = FellowshipConfig::load_from(&global_default) {
                results.push(("default".to_string(), config));
            }
        }
        // Global fellowships directory
        let global_dir = home.join(".mithril/fellowships");
        if global_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&global_dir) {
                let mut files: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map(|ext| ext == "yaml" || ext == "yml").unwrap_or(false))
                    .collect();
                files.sort_by_key(|e| e.path());
                for entry in files {
                    let stem = entry.path().file_stem().unwrap_or_default().to_string_lossy().to_string();
                    if !results.iter().any(|(n, _)| n == &stem) {
                        if let Ok(config) = FellowshipConfig::load_from(&entry.path()) {
                            results.push((stem, config));
                        }
                    }
                }
            }
        }
    }

    results
}

/// Load a fellowship by name. Searches project then global.
pub fn load_by_name(name: &str) -> Result<FellowshipConfig> {
    if name == "default" || name.is_empty() {
        return FellowshipConfig::try_load()
            .ok_or_else(|| anyhow::anyhow!("No default fellowship found. Run `mithril fellowship init`"));
    }

    // Project fellowships dir
    let project_path = std::path::PathBuf::from(format!(".mithril/fellowships/{}.yaml", name));
    if project_path.exists() {
        return FellowshipConfig::load_from(&project_path);
    }
    let project_path_yml = std::path::PathBuf::from(format!(".mithril/fellowships/{}.yml", name));
    if project_path_yml.exists() {
        return FellowshipConfig::load_from(&project_path_yml);
    }

    // Global
    if let Some(home) = dirs::home_dir() {
        let global_path = home.join(format!(".mithril/fellowships/{}.yaml", name));
        if global_path.exists() {
            return FellowshipConfig::load_from(&global_path);
        }
    }

    // Try matching by config.name field
    for (stem, config) in list_fellowships() {
        if config.name == name || stem == name {
            return Ok(config);
        }
    }

    anyhow::bail!("Fellowship '{}' not found. Run `mithril fellowships` to see available.", name)
}

// ── Default Template ─────────────────────────────────────────────────────────

/// Generate a default fellowship.yaml content.
pub fn default_template() -> String {
    r#"# Mithril Fellowship — JEV Decision Controller + Agent Free-Flow
# JEV (jevos-v4) routes entry and acts as a zero-shot decision oracle;
# agents collaborate via NEXT:/TASK: protocol and intermediate `type: decision` nodes.

name: "my-fellowship"
description: "JEV routes entry, agents collaborate freely"

controller:
  provider: local
  model: jevos-v4
  context_window: 2

max_rounds: 15
token_budget: 50000

agents:
  - name: "worker"
    provider: gemini
    model: gemini-3.6-flash
    role: "General worker. Codes, analyzes, explains, implements."
    when: "any coding, analysis, or implementation task"
    can_call: ["reviewer", "controller"]
    tools: ["*"]

  - name: "reviewer"
    provider: gemini
    model: gemini-2.5-pro
    role: "Senior reviewer. Deep analysis and quality checks. EXPENSIVE."
    when: "explicit review request or complex architecture decisions"
    can_call: ["worker"]
    tools: ["read_psi", "grep_files", "git_diff"]

  # Optional intermediate JEV decision agent (never speaks directly to the user):
  # - name: "quality_gate"
  #   type: decision
  #   mode: gate
  #   provider: local
  #   model: jevos-v4
  #   role: "Verify whether the implementation is complete and verified."
  #   choices:
  #     pass: "Implementation is complete and verified."
  #     retry: "Implementation is incomplete or missing checks."
  #   can_call: ["reviewer"]

token_tracking:
  enabled: true
  show_per_agent: true
"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_fellowship_yaml() {
        let yaml = default_template();
        let config: FellowshipConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.name, "my-fellowship");
        assert_eq!(config.description, Some("JEV routes entry, agents collaborate freely".to_string()));
        assert_eq!(config.controller.provider, "local");
        assert_eq!(config.controller.model, Some("jevos-v4".to_string()));
        assert_eq!(config.controller.context_window, 2);
        assert_eq!(config.max_rounds, 15);
        assert_eq!(config.token_budget, Some(50000));
        assert_eq!(config.agents.len(), 2);
        assert_eq!(config.agents[0].name, "worker");
        assert_eq!(config.agents[1].name, "reviewer");
    }

    #[test]
    fn test_agent_can_call() {
        let yaml = default_template();
        let config: FellowshipConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.agents[0].can_call, vec!["reviewer", "controller"]);
        assert_eq!(config.agents[1].can_call, vec!["worker"]);
    }

    #[test]
    fn test_agent_when_field() {
        let yaml = default_template();
        let config: FellowshipConfig = serde_yaml::from_str(&yaml).unwrap();
        assert!(config.agents[0].when.as_ref().unwrap().contains("coding"));
        assert!(config.agents[1].when.as_ref().unwrap().contains("review"));
    }

    #[test]
    fn test_get_agent() {
        let yaml = default_template();
        let config: FellowshipConfig = serde_yaml::from_str(&yaml).unwrap();
        assert!(config.get_agent("worker").is_some());
        assert!(config.get_agent("reviewer").is_some());
        assert!(config.get_agent("nonexistent").is_none());
    }

    #[test]
    fn test_default_max_rounds() {
        assert_eq!(default_max_rounds(), 15);
    }

    #[test]
    fn test_default_context_window() {
        assert_eq!(default_context_window(), 2);
    }

    #[test]
    fn test_default_true() {
        assert!(default_true());
    }

    #[test]
    fn test_agent_type_default() {
        let agent_type = AgentType::default();
        assert_eq!(agent_type, AgentType::Provider);
    }

    #[test]
    fn test_agent_type_serialization() {
        assert_eq!(serde_yaml::to_string(&AgentType::Provider).unwrap().trim(), "provider");
        assert_eq!(serde_yaml::to_string(&AgentType::External).unwrap().trim(), "external");
        assert_eq!(serde_yaml::to_string(&AgentType::Decision).unwrap().trim(), "decision");
    }

    #[test]
    fn test_controller_config_defaults() {
        let yaml = r#"
provider: local
"#;
        let config: ControllerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.provider, "local");
        assert!(config.model.is_none());
        assert_eq!(config.effective_model(), Some("jevos-v4"));
        assert_eq!(config.context_window, 2);
    }

    #[test]
    fn test_controller_config_custom() {
        let yaml = r#"
provider: local
model: fable-5
context_window: 4
"#;
        let config: ControllerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.model, Some("fable-5".to_string()));
        assert_eq!(config.context_window, 4);
    }

    #[test]
    fn test_fellowship_agent_provider_type() {
        let yaml = r#"
name: test-agent
provider: gemini
role: "Test agent"
"#;
        let agent: FellowshipAgent = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(agent.agent_type, AgentType::Provider);
        assert!(agent.binary.is_none());
    }

    #[test]
    fn test_fellowship_agent_external_type() {
        let yaml = r#"
name: ext-agent
type: external
binary: /usr/bin/test
args: ["--flag"]
role: "External agent"
"#;
        let agent: FellowshipAgent = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(agent.agent_type, AgentType::External);
        assert_eq!(agent.binary, Some("/usr/bin/test".to_string()));
        assert_eq!(agent.args.unwrap(), vec!["--flag"]);
    }

    #[test]
    fn test_fellowship_agent_with_tools() {
        let yaml = r#"
name: agent
role: "Agent with tools"
tools: ["read_psi", "grep_files"]
"#;
        let agent: FellowshipAgent = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(agent.tools.unwrap(), vec!["read_psi", "grep_files"]);
    }

    #[test]
    fn test_fellowship_agent_with_can_call() {
        let yaml = r#"
name: agent
role: "Delegating agent"
can_call: ["worker1", "worker2", "gguf"]
"#;
        let agent: FellowshipAgent = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(agent.can_call, vec!["worker1", "worker2", "gguf"]);
    }

    #[test]
    fn test_fellowship_agent_with_when() {
        let yaml = r#"
name: coder
role: "Implements code"
when: "coding, implementation, bug fixes"
"#;
        let agent: FellowshipAgent = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(agent.when, Some("coding, implementation, bug fixes".to_string()));
    }

    #[test]
    fn test_token_tracking_config_default() {
        let config = TokenTrackingConfig::default();
        assert!(!config.enabled);
        assert!(!config.show_per_agent);
    }

    #[test]
    fn test_token_tracking_config_yaml() {
        let yaml = r#"
enabled: true
show_per_agent: true
"#;
        let config: TokenTrackingConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.enabled);
        assert!(config.show_per_agent);
    }

    #[test]
    fn test_default_template_parseable() {
        let yaml = default_template();
        let result: Result<FellowshipConfig, _> = serde_yaml::from_str(&yaml);
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_from_file() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("fellowship.yaml");
        std::fs::write(&config_path, default_template()).unwrap();

        let config = FellowshipConfig::load_from(&config_path).unwrap();
        assert_eq!(config.name, "my-fellowship");
    }

    #[test]
    fn test_load_from_nonexistent() {
        let result = FellowshipConfig::load_from(Path::new("/nonexistent/path.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_fellowship_config_clone() {
        let yaml = default_template();
        let config: FellowshipConfig = serde_yaml::from_str(&yaml).unwrap();
        let cloned = config.clone();
        assert_eq!(cloned.name, config.name);
        assert_eq!(cloned.agents.len(), config.agents.len());
    }

    #[test]
    fn test_controller_config_clone() {
        let config = ControllerConfig {
            provider: "local".to_string(),
            model: Some("qwen-1.5b".to_string()),
            context_window: 3,
        };
        let cloned = config.clone();
        assert_eq!(cloned.provider, config.provider);
        assert_eq!(cloned.model, config.model);
        assert_eq!(cloned.context_window, config.context_window);
    }

    #[test]
    fn test_fellowship_agent_clone() {
        let yaml = r#"
name: agent
provider: gemini
role: "Test"
can_call: ["other"]
"#;
        let agent: FellowshipAgent = serde_yaml::from_str(yaml).unwrap();
        let cloned = agent.clone();
        assert_eq!(cloned.name, agent.name);
        assert_eq!(cloned.can_call, agent.can_call);
    }

    #[test]
    fn test_fellowship_config_no_token_budget() {
        let yaml = r#"
name: "simple"
controller:
  provider: local
max_rounds: 10
agents:
  - name: worker
    provider: gemini
    role: "Does work"
"#;
        let config: FellowshipConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.token_budget.is_none());
        assert_eq!(config.max_rounds, 10);
    }

    #[test]
    fn test_fellowship_config_minimal() {
        let yaml = r#"
name: "minimal"
controller:
  provider: local
agents:
  - name: worker
    role: "Does work"
"#;
        let config: FellowshipConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name, "minimal");
        assert_eq!(config.max_rounds, 15); // default
        assert!(config.token_budget.is_none());
        assert!(config.agents[0].provider.is_none());
    }

    #[test]
    fn test_can_call_empty_by_default() {
        let yaml = r#"
name: agent
role: "Test"
"#;
        let agent: FellowshipAgent = serde_yaml::from_str(yaml).unwrap();
        assert!(agent.can_call.is_empty());
    }

    #[test]
    fn test_decision_agent_modes_and_choices() {
        let yaml = r#"
name: "jev-workflow"
controller:
  provider: local
  model: jevos-v4
agents:
  - name: "triage_router"
    type: decision
    mode: route
    provider: local
    model: jevos-v4
    role: "Route the task to the right specialist"
    can_call: ["coder", "reviewer"]

  - name: "quality_gate"
    type: decision
    mode: gate
    provider: local
    model: jevos-v4
    role: "Verify code completeness and tests"
    choices:
      pass: "All checks pass"
      retry: "Missing tests or incomplete"
    can_call: ["reviewer"]

  - name: "risk_oracle"
    type: decision
    mode: consult
    provider: jev
    role: "Assess whether this change is high risk or low risk"
    choices:
      high: "Touches authentication or database migrations"
      low: "Isolated UI or formatting change"
"#;
        let config: FellowshipConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.agents.len(), 3);

        let router = &config.agents[0];
        assert_eq!(router.agent_type, AgentType::Decision);
        assert_eq!(router.mode, Some(DecisionMode::Route));
        assert_eq!(router.effective_decision_mode(), DecisionMode::Route);
        assert!(!router.is_entry_eligible()); // No `when` clause -> internal router

        let gate = &config.agents[1];
        assert_eq!(gate.agent_type, AgentType::Decision);
        assert_eq!(gate.mode, Some(DecisionMode::Gate));
        assert_eq!(gate.effective_decision_mode(), DecisionMode::Gate);
        assert_eq!(gate.choices.as_ref().unwrap().get("pass").map(String::as_str), Some("All checks pass"));
        assert!(!gate.is_entry_eligible());

        let oracle = &config.agents[2];
        assert_eq!(oracle.agent_type, AgentType::Decision);
        assert_eq!(oracle.mode, Some(DecisionMode::Consult));
        assert_eq!(oracle.effective_decision_mode(), DecisionMode::Consult);
        assert!(!oracle.is_entry_eligible());
    }

    #[test]
    fn test_effective_decision_mode_defaults() {
        let route_implicit: FellowshipAgent = serde_yaml::from_str(r#"
name: implicit_router
type: decision
role: "Routes between agents"
can_call: ["a", "b"]
"#).unwrap();
        assert_eq!(route_implicit.effective_decision_mode(), DecisionMode::Route);

        let consult_implicit: FellowshipAgent = serde_yaml::from_str(r#"
name: implicit_oracle
type: decision
role: "Answers a yes/no question"
"#).unwrap();
        assert_eq!(consult_implicit.effective_decision_mode(), DecisionMode::Consult);

        let entry_router: FellowshipAgent = serde_yaml::from_str(r#"
name: entry_router
type: decision
mode: route
role: "Entry triage router"
when: "complex multi-step triage"
can_call: ["a", "b"]
"#).unwrap();
        assert!(entry_router.is_entry_eligible());
    }
}

/// List fellowships without panicking (for API layer).
pub fn try_list_fellowships() -> anyhow::Result<Vec<(String, FellowshipConfig)>> {
    Ok(list_fellowships())
}
