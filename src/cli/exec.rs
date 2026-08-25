//! Headless exec mode — run the agentic loop non-interactively.
//!
//! Usage: `mithril exec [OPTIONS] "prompt"`
//! Suitable for CI/CD, scripts, git hooks.

use anyhow::Result;
use clap::Args;

use crate::config::MithrilConfig;
use crate::providers::{self, ChatMessage};
use super::agent_loop::{self, TraceMode};

const DEFAULT_SYSTEM_PROMPT: &str = "You are a coding assistant with access to filesystem tools. \
Execute the user's request using the available tools. Be direct and efficient. \
After completing the task, summarize what you did.";

#[derive(Args)]
pub struct ExecArgs {
    /// The prompt/instruction to execute
    pub prompt: String,

    /// Provider to use (default from config)
    #[arg(long, short)]
    pub provider: Option<String>,

    /// Model override
    #[arg(long)]
    pub model: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Maximum tool-calling iterations
    #[arg(long, default_value = "10")]
    pub max_iterations: u32,

    /// Suppress trace output, only print final response
    #[arg(long, short)]
    pub quiet: bool,

    /// System prompt override
    #[arg(long)]
    pub system: Option<String>,
}

pub async fn run(args: ExecArgs) -> Result<()> {
    let config = MithrilConfig::load()?;
    let provider_name = args
        .provider
        .as_deref()
        .unwrap_or(&config.default_provider);

    let provider = providers::create_provider(provider_name, &config)?;

    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let registry = crate::tools::create_default_registry(&cwd);
    let tool_defs = agent_loop::build_tool_defs(&registry);

    // Load steering and build system prompt
    let steering = super::steering::load_steering(&cwd);
    let base_system = args
        .system
        .as_deref()
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    let system = if steering.is_empty() {
        base_system.to_string()
    } else {
        format!("{}\n\n{}", steering, base_system)
    };
    let mut messages = vec![
        ChatMessage::system(&system),
        ChatMessage::user(&args.prompt),
    ];

    let trace_mode = if args.quiet || args.json {
        TraceMode::Silent
    } else {
        TraceMode::Full
    };

    let result = agent_loop::run_agentic_loop(
        provider.as_ref(),
        &mut messages,
        &tool_defs,
        &registry,
        args.max_iterations,
        trace_mode,
    )
    .await?;

    if args.json {
        let output = serde_json::json!({
            "response": result.response,
            "iterations": result.iterations,
            "tools_called": result.tool_calls.iter().map(|tc| {
                serde_json::json!({
                    "name": tc.name,
                    "success": tc.success
                })
            }).collect::<Vec<_>>()
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if !args.quiet {
        println!("{}", result.response);
    } else {
        // --quiet: just the response, no formatting
        print!("{}", result.response);
    }

    // Exit code: 0 = success, 2 = max iterations reached
    if result.response.contains("[Max iterations reached") {
        std::process::exit(2);
    }

    Ok(())
}
