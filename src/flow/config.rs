//! Flow configuration — parsed from `.mithril-flow.yaml`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level flow config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowConfig {
    /// Human-readable name for this flow.
    #[serde(default = "default_flow_name")]
    pub name: String,

    /// Version string (informational).
    #[serde(default = "default_version")]
    pub version: String,

    /// The planner agent — reasons about the task and decides what tools to call.
    pub planner: AgentConfig,

    /// Optional worker agent — reserved for future multi-agent routing.
    /// Currently unused: the planner calls tools directly.
    /// Will be used in Phase 2 (Celebrimbot full flow).
    #[serde(default)]
    pub worker: Option<AgentConfig>,

    /// Maximum number of planner→tools→planner iterations before giving up.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

/// Configuration for a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Display name shown in the trace.
    pub name: String,

    /// Provider to use: "gemini", "openai", "anthropic", "local".
    pub provider: String,

    /// Override the model configured for this provider (optional).
    #[serde(default)]
    pub model: Option<String>,

    /// System prompt injected at the start of every conversation.
    pub system_prompt: String,

    /// MCP tool names this agent is allowed to use.
    /// Empty = no tools. Use ["*"] to allow all tools.
    #[serde(default)]
    pub tools: Vec<String>,
}

fn default_flow_name() -> String { "default".to_string() }
fn default_version() -> String { "1.0".to_string() }
fn default_max_iterations() -> u32 { 10 }

impl FlowConfig {
    /// Load from a specific path.
    pub fn load_from(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read flow config: {}", path.display()))?;
        serde_yaml::from_str(&content)
            .context("Failed to parse flow config YAML")
    }
}

/// Load flow config for the CLI `mithril flow` command.
/// Falls back to a built-in default if no file is found,
/// so the command always works without requiring a config file.
pub fn load_flow_config(explicit_path: Option<&str>) -> Result<FlowConfig> {
    // 1. Explicit path
    if let Some(p) = explicit_path {
        return FlowConfig::load_from(Path::new(p));
    }

    // 2. Project root
    let local = Path::new(".mithril-flow.yaml");
    if local.exists() {
        return FlowConfig::load_from(local);
    }

    // 3. Global fallback
    if let Some(home) = dirs::home_dir() {
        let global = home.join(".mithril/flows/default.yaml");
        if global.exists() {
            return FlowConfig::load_from(&global);
        }
    }

    // 4. Built-in default (CLI only — server uses try_load_flow_config instead)
    Ok(FlowConfig {
        name: "default".into(),
        version: "1.0".into(),
        max_iterations: 10,
        planner: AgentConfig {
            name: "Planner".into(),
            provider: "gemini".into(),
            model: None,
            system_prompt: default_planner_prompt(),
            tools: vec!["*".into()],
        },
        worker: None,
    })
}

fn default_planner_prompt() -> String {
    r#"You are a senior software engineer assistant with access to the project's filesystem and tools.

Your workflow:
1. Read the relevant files to understand the codebase.
2. Plan your approach step by step.
3. Execute the necessary changes using the available tools.
4. Verify your work by reading back what you wrote.
5. Summarize what you did.

Always read files before modifying them. Write clean, idiomatic code that matches the existing style."#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_config_is_valid() {
        let config = load_flow_config(None).unwrap();
        assert_eq!(config.planner.provider, "gemini");
        assert!(!config.planner.system_prompt.is_empty());
        assert!(config.max_iterations > 0);
    }

    #[test]
    fn test_parse_minimal_yaml() {
        let yaml = r#"
planner:
  name: TestPlanner
  provider: gemini
  system_prompt: "You are helpful."
  tools: ["read_psi", "write_file"]
"#;
        let config: FlowConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.planner.name, "TestPlanner");
        assert_eq!(config.planner.tools, vec!["read_psi", "write_file"]);
    }

    #[test]
    fn test_default_flow_name() {
        assert_eq!(default_flow_name(), "default");
    }

    #[test]
    fn test_default_version() {
        assert_eq!(default_version(), "1.0");
    }

    #[test]
    fn test_default_max_iterations() {
        assert_eq!(default_max_iterations(), 10);
    }

    #[test]
    fn test_parse_full_yaml() {
        let yaml = r#"
name: "My Custom Flow"
version: "2.0"
max_iterations: 20

planner:
  name: SmartPlanner
  provider: openai
  model: gpt-4
  system_prompt: "Custom prompt"
  tools: ["*"]

worker:
  name: Worker
  provider: local
  system_prompt: "Worker prompt"
  tools: []
"#;
        let config: FlowConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name, "My Custom Flow");
        assert_eq!(config.version, "2.0");
        assert_eq!(config.max_iterations, 20);
        assert_eq!(config.planner.name, "SmartPlanner");
        assert_eq!(config.planner.provider, "openai");
        assert_eq!(config.planner.model, Some("gpt-4".to_string()));
        assert!(config.worker.is_some());
    }

    #[test]
    fn test_parse_uses_defaults() {
        let yaml = r#"
planner:
  name: P
  provider: local
  system_prompt: "test"
"#;
        let config: FlowConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name, "default");
        assert_eq!(config.version, "1.0");
        assert_eq!(config.max_iterations, 10);
        assert!(config.planner.tools.is_empty());
        assert!(config.planner.model.is_none());
    }

    #[test]
    fn test_load_from_file() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("test-flow.yaml");

        let yaml = r#"
name: "File Flow"
planner:
  name: FilePlanner
  provider: anthropic
  system_prompt: "From file"
"#;
        std::fs::write(&config_path, yaml).unwrap();

        let config = FlowConfig::load_from(&config_path).unwrap();
        assert_eq!(config.name, "File Flow");
        assert_eq!(config.planner.provider, "anthropic");
    }

    #[test]
    fn test_load_from_nonexistent_file() {
        let result = FlowConfig::load_from(Path::new("/nonexistent/path.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_invalid_yaml() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("invalid.yaml");
        std::fs::write(&config_path, "not: valid: yaml: {[}").unwrap();

        let result = FlowConfig::load_from(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_flow_config_explicit_path() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("explicit.yaml");

        let yaml = r#"
name: "Explicit"
planner:
  name: P
  provider: groq
  system_prompt: "test"
"#;
        std::fs::write(&config_path, yaml).unwrap();

        let config = load_flow_config(Some(config_path.to_str().unwrap())).unwrap();
        assert_eq!(config.name, "Explicit");
    }

    #[test]
    fn test_agent_config_all_fields() {
        let yaml = r#"
name: TestAgent
provider: gemini
model: gemini-pro
system_prompt: "You are a test agent."
tools: ["read_psi", "grep_files"]
"#;
        let agent: AgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(agent.name, "TestAgent");
        assert_eq!(agent.provider, "gemini");
        assert_eq!(agent.model, Some("gemini-pro".to_string()));
        assert_eq!(agent.tools.len(), 2);
    }

    #[test]
    fn test_agent_config_wildcard_tools() {
        let yaml = r#"
name: AllTools
provider: local
system_prompt: "test"
tools: ["*"]
"#;
        let agent: AgentConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(agent.tools, vec!["*"]);
    }

    #[test]
    fn test_flow_config_without_worker() {
        let yaml = r#"
planner:
  name: P
  provider: local
  system_prompt: "test"
"#;
        let config: FlowConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.worker.is_none());
    }

    #[test]
    fn test_flow_config_serialization() {
        let config = FlowConfig {
            name: "Test".to_string(),
            version: "1.0".to_string(),
            max_iterations: 5,
            planner: AgentConfig {
                name: "Planner".to_string(),
                provider: "gemini".to_string(),
                model: None,
                system_prompt: "test".to_string(),
                tools: vec!["read_psi".to_string()],
            },
            worker: None,
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("name: Test"));
        assert!(yaml.contains("max_iterations: 5"));
    }

    #[test]
    fn test_default_planner_prompt_contains_key_instructions() {
        let prompt = default_planner_prompt();
        assert!(prompt.contains("software engineer"));
        assert!(prompt.contains("Read"));
        assert!(prompt.contains("tools"));
    }

    #[test]
    fn test_flow_config_clone() {
        let config = FlowConfig {
            name: "Clone Test".to_string(),
            version: "1.0".to_string(),
            max_iterations: 10,
            planner: AgentConfig {
                name: "P".to_string(),
                provider: "local".to_string(),
                model: None,
                system_prompt: "test".to_string(),
                tools: vec![],
            },
            worker: None,
        };

        let cloned = config.clone();
        assert_eq!(cloned.name, config.name);
        assert_eq!(cloned.max_iterations, config.max_iterations);
    }

    #[test]
    fn test_zero_max_iterations() {
        let yaml = r#"
max_iterations: 0
planner:
  name: P
  provider: local
  system_prompt: "test"
"#;
        let config: FlowConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.max_iterations, 0);
    }

    #[test]
    fn test_large_max_iterations() {
        let yaml = r#"
max_iterations: 1000
planner:
  name: P
  provider: local
  system_prompt: "test"
"#;
        let config: FlowConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.max_iterations, 1000);
    }
}
