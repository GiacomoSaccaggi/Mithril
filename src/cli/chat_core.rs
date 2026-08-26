//! Centralized chat logic shared between REPL and TUI frontends.
//!
//! Both frontends delegate command execution and message processing here.
//! The frontend is responsible only for input/output rendering.

use std::collections::HashMap;
use std::sync::Arc;


use crate::config::MithrilConfig;
use crate::flow::fellowship::FellowshipConfig;
use crate::flow::orchestrator::{Orchestrator, OrchestratorResult, TraceEntry};
use crate::flow::TraceMode;
use crate::providers::ChatMessage;
use crate::session::SharedSession;

/// Default system prompt for all chat sessions.
pub const DEFAULT_SYSTEM_PROMPT: &str = "You are Mithril, an AI coding assistant running in the user's terminal. You have access to tools for reading files, editing code, running commands, searching the web, and navigating the codebase. Use these tools proactively to help the user. When asked about files or code, USE the read_psi tool to read them. When asked to modify code, USE the edit_file tool. When asked to run something, USE the run_terminal tool. Always act — don't just describe what you would do.";

/// Centralized command list — used by both REPL and TUI for help text and autocomplete.
pub const COMMANDS: &[(&str, &str)] = &[
    ("/exit", "Exit chat"),
    ("/clear", "Clear conversation"),
    ("/compact", "Compact conversation history"),
    ("/fellowship", "Show/switch fellowship"),
    ("/undo", "Undo last action"),
    ("/redo", "Redo undone action"),
    ("/plan", "Plan mode (read-only tools)"),
    ("/build", "Build mode (all tools)"),
    ("/session", "Show session info"),
    ("/history", "Show message history"),
    ("/share", "Export session as markdown"),
    ("/telegram", "Start Telegram bot (shared session)"),
    ("/help", "Show this help"),
];

/// Result of executing a command or processing a message.
pub enum ChatAction {
    /// Show a message to the user.
    Message(String),
    /// Agent response with traces.
    Response(OrchestratorResult),
    /// Error from the orchestrator.
    Error(String),
    /// Mode changed (plan_mode value).
    ModeChanged(bool),
    /// Conversation cleared.
    Cleared,
    /// Telegram bot started.
    TelegramStarted,
    /// Undo performed.
    Undone(bool),
    /// Redo performed.
    Redone(bool),
    /// Exit the chat.
    Exit,
    /// Nothing to do (command handled silently).
    #[allow(dead_code)]
    None,
}

/// Shared chat state — used by both REPL and TUI.
pub struct ChatCore {
    pub orchestrator: Orchestrator,
    pub session: SharedSession,
    pub config: MithrilConfig,
    pub fellowship_config: FellowshipConfig,
    pub plan_mode: bool,
    pub undo_stack: Vec<Vec<ChatMessage>>,
    pub redo_stack: Vec<Vec<ChatMessage>>,
}

impl ChatCore {
    pub fn new(
        fellowship_config: FellowshipConfig,
        config: MithrilConfig,
        session: SharedSession,
        trace_mode: TraceMode,
    ) -> Self {
        let orchestrator = Orchestrator::new(fellowship_config.clone(), config.clone(), trace_mode);
        Self {
            orchestrator,
            session,
            config,
            fellowship_config,
            plan_mode: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Initialize session with system prompt if empty.
    pub fn init_session(&self) {
        if self.session.snapshot().is_empty() {
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string());
            let steering = super::steering::load_steering(&cwd);
            let default_system = DEFAULT_SYSTEM_PROMPT;
            if steering.is_empty() {
                self.session.push(ChatMessage::system(default_system));
            } else {
                self.session.push(ChatMessage::system(format!("{}\n\n{}", default_system, steering)));
            }
        }
    }

    /// Execute a / command. Returns the action to take.
    pub async fn execute_command(&mut self, input: &str) -> ChatAction {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let command = parts.first().copied().unwrap_or("");

        // Custom commands from .mithril/commands.yaml
        if let Ok(custom_cmds) = std::fs::read_to_string(".mithril/commands.yaml") {
            if let Ok(cmds) = serde_yaml::from_str::<HashMap<String, String>>(&custom_cmds) {
                let cmd_key = command.trim_start_matches('/');
                if let Some(expansion) = cmds.get(cmd_key) {
                    let output = std::process::Command::new("sh")
                        .args(["-c", expansion])
                        .output();
                    let result = match output {
                        Ok(o) => {
                            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
                            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                            format!("⚡ {}\n{}{}", expansion, stdout, stderr)
                        }
                        Err(e) => format!("✗ Command failed: {}", e),
                    };
                    return ChatAction::Message(result);
                }
            }
        }

        match command {
            "/exit" | "/quit" | "/q" => ChatAction::Exit,

            "/clear" | "/c" => {
                self.session.messages.lock().clear();
                let _ = self.session.save();
                ChatAction::Cleared
            }

            "/plan" => {
                self.plan_mode = true;
                ChatAction::ModeChanged(true)
            }

            "/build" => {
                self.plan_mode = false;
                ChatAction::ModeChanged(false)
            }

            "/undo" => {
                if let Some(snapshot) = self.undo_stack.pop() {
                    self.redo_stack.push(self.session.snapshot());
                    let mut msgs = self.session.messages.lock();
                    msgs.clear();
                    msgs.extend(snapshot);
                    drop(msgs);
                    let _ = self.session.save();
                    let shadow = crate::operators::shadow::ShadowOperator::new(".", 10);
                    let _ = shadow.undo_last_session();
                    ChatAction::Undone(true)
                } else {
                    ChatAction::Undone(false)
                }
            }

            "/redo" => {
                if let Some(snapshot) = self.redo_stack.pop() {
                    self.undo_stack.push(self.session.snapshot());
                    let mut msgs = self.session.messages.lock();
                    msgs.clear();
                    msgs.extend(snapshot);
                    drop(msgs);
                    let _ = self.session.save();
                    ChatAction::Redone(true)
                } else {
                    ChatAction::Redone(false)
                }
            }

            "/fellowship" | "/f" => {
                let fellowships = crate::flow::fellowship::list_fellowships();
                let mut info = format!("Current: {}\n\nAvailable:\n", self.fellowship_config.name);
                for (_key, cfg) in &fellowships {
                    let desc = cfg.description.as_deref().unwrap_or("");
                    info.push_str(&format!("  ● {} — {}\n", cfg.name, desc));
                }
                info.push_str("\nUse: mithril chat <name> to switch");
                ChatAction::Message(info)
            }

            "/session" => {
                let meta = self.session.meta();
                let info = format!(
                    "Session: {}\nMessages: {}\nFrontend: {}",
                    meta.id, meta.message_count, self.session.active_frontend_name()
                );
                ChatAction::Message(info)
            }

            "/history" => {
                let messages = self.session.messages.lock();
                if messages.is_empty() {
                    ChatAction::Message("(no messages yet)".to_string())
                } else {
                    let mut history = String::new();
                    for (i, m) in messages.iter().enumerate() {
                        let role = match m.role.as_str() {
                            "user" => "user",
                            "assistant" => "asst",
                            "system" => "sys",
                            _ => "?",
                        };
                        let preview = if m.content.len() > 80 {
                            format!("{}…", &m.content[..80])
                        } else {
                            m.content.clone()
                        };
                        history.push_str(&format!("{}. [{}] {}\n", i + 1, role, preview));
                    }
                    ChatAction::Message(history)
                }
            }

            "/compact" => {
                let msg_count = self.session.messages.lock().len();
                if msg_count < 4 {
                    return ChatAction::Message("Not enough messages to compact (need at least 4).".to_string());
                }
                let snap = self.session.snapshot();
                let compact_provider = crate::providers::create_provider(
                    &self.config.default_provider, &self.config
                );
                let compact_result = match compact_provider {
                    Ok(p) => super::compact::compact_history(p.as_ref(), &snap).await,
                    Err(e) => Err(e),
                };
                match compact_result {
                    Ok(summary) => {
                        let mut msgs = self.session.messages.lock();
                        super::compact::apply_compaction(&mut msgs, &summary);
                        let _ = self.session.save();
                        ChatAction::Message(format!("Compacted to {} messages.", msgs.len()))
                    }
                    Err(e) => ChatAction::Error(format!("Compaction failed: {}", e)),
                }
            }

            "/share" => {
                let msgs = self.session.snapshot();
                if msgs.is_empty() {
                    return ChatAction::Message("Nothing to share (empty conversation).".to_string());
                }
                let mut md = String::from("# Mithril Chat Session\n\n");
                for m in &msgs {
                    match m.role.as_str() {
                        "user" => md.push_str(&format!("## User\n\n{}\n\n", m.content)),
                        "assistant" => md.push_str(&format!("## Assistant\n\n{}\n\n", m.content)),
                        _ => {}
                    }
                }
                let share_dir = std::path::Path::new("tmp");
                let _ = std::fs::create_dir_all(share_dir);
                let filename = format!("session_{}.md", &self.session.id[..8]);
                let path = share_dir.join(&filename);
                match std::fs::write(&path, &md) {
                    Ok(_) => {
                        let _ = std::process::Command::new("sh")
                            .args(["-c", &format!("echo '{}' | pbcopy", path.display())])
                            .output();
                        ChatAction::Message(format!("Exported to: {}", path.display()))
                    }
                    Err(e) => ChatAction::Error(format!("Failed to export: {}", e)),
                }
            }

            "/telegram" => {
                match self.config.get_credential("telegram") {
                    Ok(Some(token)) => {
                        let tg_session = self.session.clone();
                        let cancel = tokio_util::sync::CancellationToken::new();
                        let tg_config_arc = Arc::new(self.config.clone());
                        tokio::spawn(async move {
                            if let Err(e) = crate::cli::telegram::run_with_session(
                                token, tg_session, tg_config_arc, cancel
                            ).await {
                                eprintln!("  Telegram error: {}", e);
                            }
                        });
                        ChatAction::TelegramStarted
                    }
                    Ok(None) => ChatAction::Message(
                        "Telegram token not configured.\nRun: mithril config set telegram <token>".to_string()
                    ),
                    Err(e) => ChatAction::Error(format!("Config error: {}", e)),
                }
            }

            "/help" | "/h" | "/?" => {
                let mut help = String::from("Commands:\n");
                for (cmd, desc) in COMMANDS {
                    help.push_str(&format!("  {:<14} {}\n", cmd, desc));
                }
                help.push_str("\nTips:\n");
                help.push_str("  @path/to/file   Attach file content\n");
                help.push_str("  #agent          Route to specific agent\n");
                help.push_str("  \\              End line with \\ for multiline\n");
                ChatAction::Message(help)
            }

            _ => ChatAction::Message(format!("Unknown command: {}\nType /help for available commands.", command)),
        }
    }

    /// Process a user message: expand @files, save checkpoint, call orchestrator.
    pub async fn process_message(&mut self, input: &str) -> ChatAction {
        // Expand @file references
        let expanded = expand_file_references(input);
        let expanded = expanded.as_str();

        // Save undo checkpoint
        self.undo_stack.push(self.session.snapshot());
        self.redo_stack.clear();

        // Push user message
        self.session.push(ChatMessage::user(expanded));

        // Call orchestrator
        self.orchestrator.plan_mode = self.plan_mode;
        match self.orchestrator.handle_request(expanded).await {
            Ok(result) => {
                self.session.push(ChatMessage::assistant(&result.response));

                // Auto-title on first message
                if self.session.get_title().is_none() {
                    let title = input.chars().take(50).collect::<String>();
                    let title = if title.len() >= 50 {
                        format!("{}...", &title[..47])
                    } else {
                        title
                    };
                    self.session.set_title(&title);
                    let _ = self.session.save();
                }

                ChatAction::Response(result)
            }
            Err(e) => {
                // Remove the user message on error
                self.session.messages.lock().pop();
                ChatAction::Error(e.to_string())
            }
        }
    }

    /// Get agent names for autocomplete.
    pub fn agent_names(&self) -> Vec<String> {
        self.fellowship_config.agents.iter().map(|a| a.name.clone()).collect()
    }
}

/// Expand @file references in a message. Replaces @path/to/file with file content.
pub fn expand_file_references(input: &str) -> String {
    let mut result = input.to_string();
    let mut offset = 0;

    while let Some(at_pos) = result[offset..].find('@') {
        let at_pos = at_pos + offset;
        // Must be at start or after whitespace
        if at_pos > 0 && !result.as_bytes()[at_pos - 1].is_ascii_whitespace() {
            offset = at_pos + 1;
            continue;
        }

        let after = &result[at_pos + 1..];
        let (path_str, end_offset) = if let Some(quoted) = after.strip_prefix('"') {
            // Quoted path: @"path with spaces.rs"
            if let Some(end_quote) = quoted.find('"') {
                (&quoted[..end_quote], at_pos + 2 + end_quote + 1)
            } else {
                offset = at_pos + 1;
                continue;
            }
        } else {
            // Unquoted: take until whitespace
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            if end == 0 {
                offset = at_pos + 1;
                continue;
            }
            (&after[..end], at_pos + 1 + end)
        };

        // Try to read the file
        if let Ok(content) = std::fs::read_to_string(path_str) {
            let replacement = format!("[file: {}]\n```\n{}\n```", path_str, content.trim_end());
            result.replace_range(at_pos..end_offset, &replacement);
            offset = at_pos + replacement.len();
        } else {
            offset = end_offset;
        }
    }

    result
}

/// Format trace entries as display strings.
pub fn format_trace(trace: &TraceEntry) -> String {
    match trace {
        TraceEntry::Entry { agent } => format!("⚡ gguf → {}", agent),
        TraceEntry::AgentStart { agent, provider } => format!("▶ {} ({})", agent, provider),
        TraceEntry::ToolCall { name, success, preview } =>
            format!("  {} {} → {}", if *success { "⚙" } else { "✗" }, name, preview),
        TraceEntry::Delegation { from, to, task_preview } =>
            format!("🔀 {} → {}: {}", from, to, task_preview),
        TraceEntry::GgufCall { task_preview } => format!("⚙ → gguf: {}", task_preview),
        TraceEntry::Done { agent } => format!("✓ {} → DONE", agent),
        TraceEntry::BudgetWarning { used, limit } => format!("⚠ budget ({}/{})", used, limit),
    }
}

/// Strip markdown formatting for terminal display.
pub fn strip_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut in_code_block = false;

    for line in text.lines() {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let stripped = if let Some(s) = line.strip_prefix("### ") { s }
            else if let Some(s) = line.strip_prefix("## ") { s }
            else if let Some(s) = line.strip_prefix("# ") { s }
            else { line };
        let stripped = stripped.replace("**", "").replace("__", "").replace(['*', '`'], "");
        if let Some(rest) = stripped.strip_prefix("- ") {
            out.push_str("  • ");
            out.push_str(rest);
        } else {
            out.push_str(&stripped);
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_list_complete() {
        assert!(COMMANDS.len() >= 13);
        // All essential commands present
        let cmd_names: Vec<&str> = COMMANDS.iter().map(|(c, _)| *c).collect();
        assert!(cmd_names.contains(&"/exit"));
        assert!(cmd_names.contains(&"/help"));
        assert!(cmd_names.contains(&"/telegram"));
        assert!(cmd_names.contains(&"/share"));
        assert!(cmd_names.contains(&"/plan"));
        assert!(cmd_names.contains(&"/build"));
        assert!(cmd_names.contains(&"/undo"));
        assert!(cmd_names.contains(&"/redo"));
    }

    #[test]
    fn test_expand_file_references_passthrough() {
        assert_eq!(expand_file_references("hello world"), "hello world");
        assert_eq!(expand_file_references("#worker do stuff"), "#worker do stuff");
    }

    #[test]
    fn test_expand_file_nonexistent() {
        let result = expand_file_references("@nonexistent_xyz.rs please read");
        // File doesn't exist — @ref remains unchanged
        assert!(result.contains("@nonexistent_xyz.rs"));
    }

    #[test]
    fn test_expand_file_real() {
        // Cargo.toml should exist
        let result = expand_file_references("@Cargo.toml");
        assert!(result.contains("[file: Cargo.toml]"));
        assert!(result.contains("[package]"));
    }

    #[test]
    fn test_expand_at_in_middle_of_word() {
        // @ in middle of word should NOT expand
        let result = expand_file_references("email@test.com");
        assert_eq!(result, "email@test.com");
    }

    #[test]
    fn test_strip_markdown_headings() {
        assert_eq!(strip_markdown("# Title"), "Title");
        assert_eq!(strip_markdown("## Subtitle"), "Subtitle");
        assert_eq!(strip_markdown("### Section"), "Section");
    }

    #[test]
    fn test_strip_markdown_bold() {
        assert_eq!(strip_markdown("**bold** text"), "bold text");
    }

    #[test]
    fn test_strip_markdown_code_block() {
        let input = "before\n```rust\nlet x = 1;\n```\nafter";
        let result = strip_markdown(input);
        assert!(result.contains("    let x = 1;"));
        assert!(!result.contains("```"));
    }

    #[test]
    fn test_strip_markdown_list() {
        let result = strip_markdown("- item one\n- item two");
        assert!(result.contains("• item one"));
        assert!(result.contains("• item two"));
    }

    #[test]
    fn test_format_trace_entries() {
        use crate::flow::orchestrator::TraceEntry;
        
        let entry = TraceEntry::Entry { agent: "worker".to_string() };
        assert!(format_trace(&entry).contains("worker"));

        let done = TraceEntry::Done { agent: "reviewer".to_string() };
        assert!(format_trace(&done).contains("DONE"));

        let tool = TraceEntry::ToolCall { name: "read_psi".to_string(), success: true, preview: "file.rs".to_string() };
        assert!(format_trace(&tool).contains("read_psi"));
    }

    #[test]
    fn test_default_system_prompt() {
        assert!(DEFAULT_SYSTEM_PROMPT.contains("Mithril"));
        assert!(DEFAULT_SYSTEM_PROMPT.contains("tools"));
    }
}
