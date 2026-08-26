//! Multi-agent flow system.
//!
//! - `FlowRunner`: linear Planner → ToolExecutor loop (mithril flow).
//! - `Orchestrator`: GGUF entry classifier + agent free-flow (mithril chat).

#![allow(unused_imports)]

pub mod config;
pub mod runner;
pub mod tokens;
pub mod fellowship;
pub mod orchestrator;
pub mod agent_loop;

pub use config::{FlowConfig, load_flow_config};
pub use runner::FlowRunner;
pub use tokens::{TokenUsage, SessionTokens};
pub use fellowship::FellowshipConfig;
pub use orchestrator::Orchestrator;

use crate::providers::ToolDefinition;
use crate::tools::registry::ToolRegistry;

/// How much trace output to show during agent execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceMode {
    /// No output at all (for --json / --quiet)
    Silent,
    /// Brief inline traces (for interactive chat)
    Inline,
    /// Full traces with iteration numbers (for exec verbose)
    Full,
}

/// Convert all tools in a registry to provider-compatible definitions.
pub fn build_tool_defs(registry: &ToolRegistry) -> Vec<ToolDefinition> {
    registry
        .all()
        .iter()
        .map(|t| ToolDefinition::from_registry_tool(*t))
        .collect()
}

/// Build tool definitions filtered by an allowed tools list.
/// If filter contains "*", returns all tools.
pub fn build_filtered_tool_defs(registry: &ToolRegistry, allowed: &[String]) -> Vec<ToolDefinition> {
    if allowed.iter().any(|t| t == "*") {
        return build_tool_defs(registry);
    }
    registry
        .all()
        .iter()
        .filter(|t| allowed.iter().any(|a| a == t.name()))
        .map(|t| ToolDefinition::from_registry_tool(*t))
        .collect()
}
