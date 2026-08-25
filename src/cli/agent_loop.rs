//! Shared agentic loop — used by both interactive chat and headless exec.
//!
//! Algorithm:
//!   1. Send messages + tool definitions to provider via chat_with_tools()
//!   2. If response = Text → done
//!   3. If response = ToolCalls → execute each, feed results back, goto 1
//!   4. If max_iterations reached → return last response or error

use std::collections::HashMap;

use anyhow::Result;
use colored::Colorize;

use crate::providers::{ChatMessage, ChatProvider, ToolCallResult, ToolDefinition};
use crate::tools::registry::ToolRegistry;

/// How much trace output to show during the loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TraceMode {
    /// No output at all (for --json / --quiet)
    Silent,
    /// Brief inline traces (for interactive chat)
    Inline,
    /// Full traces with iteration numbers (for exec verbose)
    Full,
}

/// Record of a single tool invocation.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub name: String,
    #[allow(dead_code)]
    pub args: HashMap<String, String>,
    pub success: bool,
    pub output: String,
}

/// Result of running the agentic loop.
#[derive(Debug)]
pub struct AgentResult {
    pub response: String,
    pub iterations: u32,
    pub tool_calls: Vec<ToolCallRecord>,
}

/// Run the agentic loop: provider → tool calls → execute → feed back → repeat.
///
/// `messages` is mutated in-place: tool results are appended as system messages.
/// The final assistant response is NOT appended (caller decides what to do with it).

/// Check if a tool requires permission in interactive mode.
/// Uses the configurable permissions system.
/// Respects MITHRIL_NO_CONFIRM=1 env var (set by --no-confirm flag).
fn needs_permission(tool_name: &str) -> bool {
    if std::env::var("MITHRIL_NO_CONFIRM").unwrap_or_default() == "1" {
        return false;
    }
    let config = crate::config::MithrilConfig::load().unwrap_or_default();
    config.permissions.needs_confirmation(tool_name)
}

/// Check if a tool is completely denied by config.
fn is_tool_denied(tool_name: &str) -> bool {
    let config = crate::config::MithrilConfig::load().unwrap_or_default();
    config.permissions.is_denied(tool_name)
}

/// Ask the user for permission to execute a dangerous tool.
/// Returns true if approved, false if denied.
fn ask_permission(tool_name: &str, args: &std::collections::HashMap<String, String>) -> bool {
    use std::io::{self, Write};

    let detail = match tool_name {
        "write_file" | "edit_file" | "apply_patch" | "delete_file" => {
            args.get("target").map(|t| t.as_str()).unwrap_or("?")
        }
        "run_terminal" => {
            args.get("command").map(|t| t.as_str()).unwrap_or("?")
        }
        _ => "?",
    };

    eprint!(
        "  \x1b[33m⚠ {}\x1b[0m {} \x1b[2m[y/N]\x1b[0m ",
        tool_name,
        detail
    );
    io::stderr().flush().ok();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_ok() {
        let answer = input.trim().to_lowercase();
        answer == "y" || answer == "yes"
    } else {
        false
    }
}

pub async fn run_agentic_loop(
    provider: &dyn ChatProvider,
    messages: &mut Vec<ChatMessage>,
    tool_defs: &[ToolDefinition],
    registry: &ToolRegistry,
    max_iterations: u32,
    trace_mode: TraceMode,
) -> Result<AgentResult> {
    let mut all_tool_calls: Vec<ToolCallRecord> = Vec::new();

    for iteration in 0..max_iterations {
        if trace_mode == TraceMode::Full {
            eprintln!(
                "  {} iteration {}/{}",
                "◆".cyan(),
                iteration + 1,
                max_iterations
            );
        }

        let result = crate::providers::retry_with_backoff(3, || async {
            provider.chat_with_tools(messages, tool_defs).await
        }).await?;

        match result {
            ToolCallResult::Text(response) => {
                return Ok(AgentResult {
                    response,
                    iterations: iteration + 1,
                    tool_calls: all_tool_calls,
                });
            }

            ToolCallResult::ToolCalls(calls) => {
                if calls.is_empty() {
                    // Model returned empty tool calls — treat as done with empty response
                    return Ok(AgentResult {
                        response: String::new(),
                        iterations: iteration + 1,
                        tool_calls: all_tool_calls,
                    });
                }

                let mut results_text = Vec::new();

                for call in &calls {
                    // Deny gate: if tool is completely disabled by config, skip it
                    if is_tool_denied(&call.name) {
                        let denied_msg = format!(
                            "⛔ Tool '{}' is disabled by configuration (permissions: deny)",
                            call.name
                        );
                        results_text.push(format!("Tool `{}` returned:\n{}", call.name, denied_msg));
                        all_tool_calls.push(ToolCallRecord {
                            name: call.name.clone(),
                            args: call.arguments.clone(),
                            success: false,
                            output: denied_msg,
                        });
                        continue;
                    }

                    // Permission gate for dangerous tools (interactive mode only)
                    if trace_mode == TraceMode::Inline && needs_permission(&call.name)
                        && !ask_permission(&call.name, &call.arguments) {
                            let denied = crate::tools::registry::ToolResult::err(
                                format!("⛔ Permission denied by user for '{}'", call.name)
                            );
                            results_text.push(format!(
                                "Tool `{}` returned:\n{}",
                                call.name, denied.output
                            ));
                            all_tool_calls.push(ToolCallRecord {
                                name: call.name.clone(),
                                args: call.arguments.clone(),
                                success: false,
                                output: denied.output.clone(),
                            });
                            continue;
                        }
                    let tool_result = execute_tool(registry, call);

                    // Trace output
                    match trace_mode {
                        TraceMode::Silent => {}
                        TraceMode::Inline => {
                            let preview = truncate(&tool_result.output, 80);
                            eprintln!(
                                "  {} {} → {}",
                                "⚙".dimmed(),
                                call.name.yellow(),
                                preview.dimmed()
                            );
                        }
                        TraceMode::Full => {
                            let preview = truncate(&tool_result.output, 200);
                            eprintln!(
                                "  {} {} ({}) → {}",
                                "⚙".dimmed(),
                                call.name.yellow(),
                                if tool_result.success { "ok".green() } else { "err".red() },
                                preview.dimmed()
                            );
                        }
                    }

                    results_text.push(format!(
                        "Tool `{}` returned:\n{}",
                        call.name, tool_result.output
                    ));

                    all_tool_calls.push(ToolCallRecord {
                        name: call.name.clone(),
                        args: call.arguments.clone(),
                        success: tool_result.success,
                        output: tool_result.output.clone(),
                    });
                }

                // ── Doom loop detection (Balrog's Bane) ─────────────────
                // If the last 3 tool calls are the same tool failing with
                // the same error, the agent is stuck. Break the loop.
                if all_tool_calls.len() >= 3 {
                    let tail = &all_tool_calls[all_tool_calls.len() - 3..];
                    let same_tool = tail.iter().all(|r| r.name == tail[0].name);
                    let all_failed = tail.iter().all(|r| !r.success);
                    let same_err = tail[0].output == tail[1].output && tail[1].output == tail[2].output;
                    if same_tool && all_failed && same_err {
                        if trace_mode != TraceMode::Silent {
                            eprintln!(
                                "  🔥 Balrog detected! Agent stuck in doom loop (3× {}). Breaking free.",
                                tail[0].name
                            );
                        }
                        return Ok(AgentResult {
                            response: format!(
                                "I got stuck repeating the same failing action (`{}` failed 3 times with: {}). Stopping to avoid wasting resources.",
                                tail[0].name, truncate(&tail[0].output, 100)
                            ),
                            iterations: iteration + 1,
                            tool_calls: all_tool_calls,
                        });
                    }
                }

                // Feed results back as a system message
                messages.push(ChatMessage::system(format!(
                    "[Tool execution results]\n\n{}",
                    results_text.join("\n\n---\n\n")
                )));
            }
        }
    }

    // Max iterations reached — return what we have
    if trace_mode != TraceMode::Silent {
        eprintln!("  {} Max iterations ({}) reached.", "⚠".yellow(), max_iterations);
    }

    Ok(AgentResult {
        response: String::from("[Max iterations reached without final response]"),
        iterations: max_iterations,
        tool_calls: all_tool_calls,
    })
}

/// Execute a single tool call. Catches panics.
fn execute_tool(
    registry: &ToolRegistry,
    call: &crate::providers::ToolCall,
) -> crate::tools::registry::ToolResult {
    let tool = match registry.get(&call.name) {
        Some(t) => t,
        None => {
            return crate::tools::registry::ToolResult::err(format!(
                "Tool '{}' not found in registry",
                call.name
            ));
        }
    };

    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tool.execute(&call.arguments)
    })) {
        Ok(result) => result,
        Err(_) => crate::tools::registry::ToolResult::err(format!(
            "Tool '{}' panicked during execution",
            call.name
        )),
    }
}

/// Build tool definitions from all tools in the registry.
pub fn build_tool_defs(registry: &ToolRegistry) -> Vec<ToolDefinition> {
    registry
        .all()
        .iter()
        .map(|t| ToolDefinition::from_registry_tool(*t))
        .collect()
}

fn truncate(s: &str, max: usize) -> String {
    let oneline = s.replace('\n', " ");
    let trimmed = oneline.trim();
    if trimmed.len() > max {
        format!("{}…", &trimmed[..max])
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        let result = truncate("this is a very long string that should be truncated", 20);
        assert!(result.len() <= 23); // 20 chars + 3-byte "…"
        assert!(result.ends_with("…"));
    }

    #[test]
    fn test_truncate_newlines_collapsed() {
        let result = truncate("line1\nline2\nline3", 50);
        assert!(!result.contains('\n'));
        assert!(result.contains("line1 line2 line3"));
    }

    #[test]
    fn test_needs_permission_dangerous_tools() {
        assert!(needs_permission("write_file"));
        assert!(needs_permission("edit_file"));
        assert!(needs_permission("apply_patch"));
        assert!(needs_permission("delete_file"));
        assert!(needs_permission("run_terminal"));
    }

    #[test]
    fn test_needs_permission_safe_tools() {
        assert!(!needs_permission("read_psi"));
        assert!(!needs_permission("grep_files"));
        assert!(!needs_permission("list_files"));
        assert!(!needs_permission("web_search"));
        assert!(!needs_permission("git_status"));
        assert!(!needs_permission("search_symbols"));
        assert!(!needs_permission("lore_read"));
    }

    #[test]
    fn test_build_tool_defs_not_empty() {
        let registry = crate::tools::create_default_registry(".");
        let defs = build_tool_defs(&registry);
        assert!(defs.len() > 10); // should have at least 15+ tools
    }

    #[test]
    fn test_build_tool_defs_has_edit_file() {
        let registry = crate::tools::create_default_registry(".");
        let defs = build_tool_defs(&registry);
        assert!(defs.iter().any(|d| d.name == "edit_file"));
    }

    #[test]
    fn test_build_tool_defs_has_lore_tools() {
        let registry = crate::tools::create_default_registry(".");
        let defs = build_tool_defs(&registry);
        assert!(defs.iter().any(|d| d.name == "lore_write"));
        assert!(defs.iter().any(|d| d.name == "lore_read"));
    }

    #[test]
    fn test_build_tool_defs_has_code_intelligence() {
        let registry = crate::tools::create_default_registry(".");
        let defs = build_tool_defs(&registry);
        assert!(defs.iter().any(|d| d.name == "search_symbols"));
        assert!(defs.iter().any(|d| d.name == "document_outline"));
    }
}
