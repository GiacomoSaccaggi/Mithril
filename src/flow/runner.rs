//! FlowRunner — executes the Planner→Tools loop.
//!
//! Algorithm:
//!   1. Build tool definitions for this agent (filtered by tools list)
//!   2. Send user message + history to planner via chat_with_tools()
//!   3. If response = ToolCalls → execute each tool, add results to history, goto 2
//!   4. If response = Text → print final response, done
//!   5. If max_iterations reached → print whatever we have, done

#![allow(dead_code)]
use std::io::{self, Write};
use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;

use crate::config::MithrilConfig;
use crate::flow::config::{AgentConfig, FlowConfig};
use crate::providers::{self, ChatMessage, ToolCallResult, ToolDefinition};
use crate::tools::registry::ToolRegistry;

pub struct FlowRunner {
    config: FlowConfig,
    registry: Arc<ToolRegistry>,
    cwd: String,
}

impl FlowRunner {
    pub fn new(config: FlowConfig, cwd: &str) -> Self {
        let registry = Arc::new(crate::tools::create_default_registry(cwd));
        Self { config, registry, cwd: cwd.to_string() }
    }

    /// Run the flow with the given user message. Prints trace to stdout.
    pub async fn run(&self, user_message: &str) -> Result<String> {
        let mithril_config = MithrilConfig::load()?;

        print_trace_header(&self.config.name);

        // Build tool definitions for planner
        let planner_tools = self.build_tool_defs(&self.config.planner);

        // Create planner provider
        let planner_provider = providers::create_provider(
            &self.config.planner.provider,
            &mithril_config,
        )?;

        // Conversation history — starts with system prompt + user message
        let mut history: Vec<ChatMessage> = vec![
            ChatMessage::system(&self.config.planner.system_prompt),
            ChatMessage::user(user_message),
        ];

        let mut final_response = String::new();
        let max_iter = self.config.max_iterations;

        for iteration in 0..max_iter {
            print_trace_agent(
                &self.config.planner.name,
                &self.config.planner.provider,
                iteration,
                max_iter,
            );

            // Call planner
            let result = planner_provider
                .chat_with_tools(&history, &planner_tools)
                .await
                .map_err(|e| anyhow::anyhow!("Planner error: {}", e))?;

            match result {
                ToolCallResult::Text(response) => {
                    // Planner is done — print final response
                    print_trace_done(&self.config.planner.name);
                    println!("\n{}", response);
                    println!();
                    final_response = response;
                    break;
                }

                ToolCallResult::ToolCalls(calls) => {
                    if calls.is_empty() {
                        println!("  {} Model returned no tools and no text. Stopping.", "⚠️".yellow());
                        break;
                    }

                    // Execute each tool call, collect results
                    let mut tool_results: Vec<String> = Vec::new();
                    for call in &calls {
                        let result_text = self.execute_tool(call);
                        print_trace_tool(&call.name, &result_text);
                        tool_results.push(format!(
                            "Tool `{}` returned:\n{}",
                            call.name, result_text
                        ));
                    }

                    // Feed results back.
                    // We use a SYSTEM message so the model treats it as
                    // authoritative context rather than user input.
                    // This avoids role-alternation issues with Gemini/OpenAI.
                    history.push(ChatMessage {
                        role: "system".to_string(),
                        content: format!(
                            "[Tool execution results — use these to continue your work]\n\n{}",
                            tool_results.join("\n\n---\n\n")
                        ),
                    });
                }
            }

            if iteration == max_iter - 1 {
                println!("\n{}", "⚠️  Max iterations reached.".yellow());
            }
        }

        Ok(final_response)
    }

    /// Execute a single tool call and return its output as a string.
    /// Catches panics to prevent a misbehaving tool from crashing the flow.
    fn execute_tool(&self, call: &crate::providers::ToolCall) -> String {
        let tool = match self.registry.get(&call.name) {
            Some(t) => t,
            None => return format!("Error: tool '{}' not found in registry", call.name),
        };
        let args = call.arguments.clone();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tool.execute(&args)
        })) {
            Ok(result) => result.output,
            Err(_) => format!("Error: tool '{}' panicked during execution", call.name),
        }
    }

    /// Build ToolDefinition list for an agent, filtered by its tools list.
    fn build_tool_defs(&self, agent: &AgentConfig) -> Vec<ToolDefinition> {
        let all_tools = self.registry.all();
        all_tools.iter()
            .filter(|t| {
                agent.tools.contains(&"*".to_string())
                    || agent.tools.contains(&t.name().to_string())
            })
            .map(|t| ToolDefinition::from_registry_tool(*t))
            .collect()
    }
}

// ── Trace display ─────────────────────────────────────────────────────────────

fn print_trace_header(flow_name: &str) {
    println!();
    println!("  {} Running flow: {}", "⚡".bold(), flow_name.cyan().bold());
    println!("  {}", "─".repeat(50).dimmed());
    println!();
}

fn print_trace_agent(name: &str, provider: &str, iteration: u32, _max: u32) {
    let iter_label = if iteration == 0 {
        String::new()
    } else {
        format!(" (iter {})", iteration + 1)
    };
    print!("  {} {}{} [{}]  ",
        "◆".cyan(),
        name.bold(),
        iter_label.dimmed(),
        provider.dimmed()
    );
    io::stdout().flush().ok();
}

fn print_trace_tool(tool_name: &str, result: &str) {
    let preview = if result.len() > 100 {
        format!("{}…", &result[..100])
    } else {
        result.to_string()
    };
    // Replace newlines with spaces for single-line preview
    let preview = preview.replace('\n', " ").trim().to_string();
    println!("  {} {} → {}", "⚙".dimmed(), tool_name.yellow(), preview.dimmed());
}

fn print_trace_done(name: &str) {
    println!("{}", "done".green().dimmed());
    println!("  {} {} finished", "✓".green(), name.bold());
}
