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

pub use config::{FlowConfig, load_flow_config};
pub use runner::FlowRunner;
pub use tokens::{TokenUsage, SessionTokens};
pub use fellowship::FellowshipConfig;
pub use orchestrator::Orchestrator;
