//! App state for the Mithril TUI.

use std::collections::VecDeque;

/// A displayable message in the chat panel.
#[derive(Debug, Clone)]
pub struct ChatLine {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum Role {
    User,
    Assistant,
    System,
    Tool { name: String, success: bool },
    AgentTrace { agent: String, detail: String },
    Summary { rounds: u32, tokens: String },
}

/// Which panel is currently focused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Input,
    Chat,
    Sidebar,
}

/// Agent mode — determines which tools are available.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentMode {
    /// Full access: all tools enabled (file writes, terminal, etc.)
    Build,
    /// Read-only: only observation tools (read_psi, list_files, grep, git_status, etc.)
    Plan,
}

impl AgentMode {
    pub fn label(&self) -> &'static str {
        match self {
            AgentMode::Build => "BUILD",
            AgentMode::Plan => "PLAN",
        }
    }

    pub fn toggle(&self) -> Self {
        match self {
            AgentMode::Build => AgentMode::Plan,
            AgentMode::Plan => AgentMode::Build,
        }
    }


}

/// App state — owned by the TUI event loop.
pub struct App {
    /// Chat messages displayed in the main panel
    pub messages: Vec<ChatLine>,
    /// Input buffer (multiline)
    pub input: String,
    /// Input cursor position
    pub cursor: usize,
    /// Input history (previous sent messages)
    pub input_history: VecDeque<String>,
    pub history_index: Option<usize>,
    /// Chat scroll offset (from bottom)
    pub scroll_offset: u16,
    /// Current focus
    pub focus: Focus,
    /// Sidebar visible
    pub sidebar_visible: bool,
    /// Fellowship name
    pub fellowship_name: String,
    /// Session ID (truncated)
    pub session_id: String,
    /// Files touched in this session
    pub files_touched: Vec<String>,
    /// Tool call count
    pub tool_call_count: u32,
    /// Iteration count
    pub iteration_count: u32,
    /// Is the agent currently thinking?
    pub thinking: bool,
    /// Should the app exit?
    pub should_exit: bool,
    /// Agent mode (Plan = read-only, Build = full access)
    pub mode: AgentMode,
    /// Command suggestions (shown when input starts with /)
    pub suggestions: Vec<&'static str>,
    /// Selected suggestion index
    pub suggestion_index: usize,
    /// Current orchestrator round
    pub current_round: u32,
    /// Max rounds from fellowship config
    pub max_rounds: u32,
}

/// All available slash commands with descriptions.
pub const COMMANDS: &[(&str, &str)] = &[
    ("/exit", "Exit chat"),
    ("/clear", "Clear conversation"),
    ("/compact", "Compress conversation to free context"),
    ("/fellowship", "Switch fellowship or show current config"),
    ("/undo", "Undo last action (conversation + files)"),
    ("/redo", "Redo undone action"),
    ("/session", "Show session info"),
    ("/history", "Show message history"),
    ("/help", "Show help"),
];

impl App {
    pub fn new(fellowship_name: &str, _model: &str, session_id: &str) -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            input_history: VecDeque::with_capacity(50),
            history_index: None,
            scroll_offset: 0,
            focus: Focus::Input,
            sidebar_visible: true,
            fellowship_name: fellowship_name.to_string(),
            session_id: session_id[..8.min(session_id.len())].to_string(),
            files_touched: Vec::new(),
            tool_call_count: 0,
            iteration_count: 0,
            thinking: false,
            should_exit: false,
            mode: AgentMode::Build,
            suggestions: Vec::new(),
            suggestion_index: 0,
            current_round: 0,
            max_rounds: 0,
        }
    }

    /// Update command suggestions based on current input.
    pub fn update_suggestions(&mut self) {
        if self.input.starts_with('/') && !self.input.contains(' ') {
            let prefix = &self.input;
            self.suggestions = COMMANDS.iter()
                .filter(|(cmd, _)| cmd.starts_with(prefix))
                .map(|(cmd, _)| *cmd)
                .collect();
            if self.suggestion_index >= self.suggestions.len() {
                self.suggestion_index = 0;
            }
        } else {
            self.suggestions.clear();
            self.suggestion_index = 0;
        }
    }

    /// Accept the current suggestion (replace input with it).
    pub fn accept_suggestion(&mut self) {
        if let Some(cmd) = self.suggestions.get(self.suggestion_index) {
            self.input = format!("{} ", cmd);
            self.cursor = self.input.len();
            self.suggestions.clear();
        }
    }

    /// Estimate total tokens used (chars / 4 for all messages).
    pub fn estimated_tokens_display(&self) -> String {
        let total_chars: usize = self.messages.iter().map(|m| m.content.len()).sum();
        let tokens = total_chars / 4;
        if tokens >= 1000 {
            format!("{:.1}k tokens", tokens as f64 / 1000.0)
        } else {
            format!("{} tokens", tokens)
        }
    }

    pub fn push_message(&mut self, role: Role, content: &str) {
        self.messages.push(ChatLine {
            role,
            content: content.to_string(),
        });
        // Auto-scroll to bottom
        self.scroll_offset = 0;
    }

    #[allow(dead_code)]
    pub fn push_tool_trace(&mut self, name: &str, success: bool, preview: &str) {
        self.messages.push(ChatLine {
            role: Role::Tool {
                name: name.to_string(),
                success,
            },
            content: preview.to_string(),
        });
        self.tool_call_count += 1;
        self.scroll_offset = 0;
    }

    #[allow(dead_code)]
    pub fn track_file(&mut self, path: &str) {
        if !self.files_touched.contains(&path.to_string()) {
            self.files_touched.push(path.to_string());
        }
    }

    pub fn submit_input(&mut self) -> Option<String> {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return None;
        }
        self.input_history.push_front(text.clone());
        if self.input_history.len() > 50 {
            self.input_history.pop_back();
        }
        self.input.clear();
        self.cursor = 0;
        self.history_index = None;
        Some(text)
    }

    pub fn history_up(&mut self) {
        let idx = self.history_index.map(|i| i + 1).unwrap_or(0);
        if idx < self.input_history.len() {
            self.history_index = Some(idx);
            self.input = self.input_history[idx].clone();
            self.cursor = self.input.len();
        }
    }

    pub fn history_down(&mut self) {
        match self.history_index {
            Some(0) => {
                self.history_index = None;
                self.input.clear();
                self.cursor = 0;
            }
            Some(i) => {
                self.history_index = Some(i - 1);
                self.input = self.input_history[i - 1].clone();
                self.cursor = self.input.len();
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_app() {
        let app = App::new("my-fellowship", "llama-4", "abcdef123456");
        assert_eq!(app.fellowship_name, "my-fellowship");
        assert_eq!(app.session_id, "abcdef12");
        assert!(!app.should_exit);
        assert!(app.messages.is_empty());
        assert_eq!(app.focus, Focus::Input);
    }

    #[test]
    fn test_push_message() {
        let mut app = App::new("test", "model", "session123");
        app.push_message(Role::User, "hello");
        app.push_message(Role::Assistant, "hi there");
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[0].role, Role::User);
        assert_eq!(app.messages[1].content, "hi there");
    }

    #[test]
    fn test_push_tool_trace() {
        let mut app = App::new("test", "model", "session123");
        app.push_tool_trace("read_psi", true, "file content...");
        assert_eq!(app.tool_call_count, 1);
        assert_eq!(app.messages.len(), 1);
        match &app.messages[0].role {
            Role::Tool { name, success } => {
                assert_eq!(name, "read_psi");
                assert!(*success);
            }
            _ => panic!("Expected Tool role"),
        }
    }

    #[test]
    fn test_track_file_deduplicates() {
        let mut app = App::new("test", "model", "sess");
        app.track_file("src/main.rs");
        app.track_file("src/lib.rs");
        app.track_file("src/main.rs"); // duplicate
        assert_eq!(app.files_touched.len(), 2);
    }

    #[test]
    fn test_submit_input() {
        let mut app = App::new("test", "model", "sess");
        app.input = "hello world".to_string();
        app.cursor = 11;
        let result = app.submit_input();
        assert_eq!(result, Some("hello world".to_string()));
        assert!(app.input.is_empty());
        assert_eq!(app.cursor, 0);
        assert_eq!(app.input_history.len(), 1);
    }

    #[test]
    fn test_submit_empty_input() {
        let mut app = App::new("test", "model", "sess");
        app.input = "   ".to_string();
        let result = app.submit_input();
        assert_eq!(result, None);
    }

    #[test]
    fn test_history_navigation() {
        let mut app = App::new("test", "model", "sess");
        app.input = "first".to_string();
        app.submit_input();
        app.input = "second".to_string();
        app.submit_input();

        // Navigate up
        app.history_up();
        assert_eq!(app.input, "second");
        app.history_up();
        assert_eq!(app.input, "first");

        // Navigate down
        app.history_down();
        assert_eq!(app.input, "second");
        app.history_down();
        assert!(app.input.is_empty());
    }

    #[test]
    fn test_history_up_at_limit() {
        let mut app = App::new("test", "model", "sess");
        app.input = "only".to_string();
        app.submit_input();
        app.history_up();
        app.history_up(); // should not crash
        assert_eq!(app.input, "only");
    }

    #[test]
    fn test_scroll_offset_auto_reset() {
        let mut app = App::new("test", "model", "sess");
        app.scroll_offset = 5;
        app.push_message(Role::User, "new message");
        assert_eq!(app.scroll_offset, 0); // auto-scroll to bottom
    }

    #[test]
    fn test_agent_mode_toggle() {
        assert_eq!(AgentMode::Build.toggle(), AgentMode::Plan);
        assert_eq!(AgentMode::Plan.toggle(), AgentMode::Build);
    }

    #[test]
    fn test_agent_mode_labels() {
        assert_eq!(AgentMode::Build.label(), "BUILD");
        assert_eq!(AgentMode::Plan.label(), "PLAN");
    }



    #[test]
    fn test_app_default_mode_is_build() {
        let app = App::new("test", "model", "sess");
        assert_eq!(app.mode, AgentMode::Build);
    }
}
