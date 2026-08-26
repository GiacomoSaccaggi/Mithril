#![allow(unused_imports)]
use std::collections::HashMap;
use std::future::Future;

use crate::operators::{
    file::FileOperator, git::GitOperator, scan::ScanOperator,
    terminal::TerminalOperator, web::WebOperator,
};
use crate::tools::registry::{Tool, ToolParam, ToolResult};

/// Bridge a future onto a tokio runtime.
fn block_on_async<F, T>(fut: F) -> T
where
    F: Future<Output = T>,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => tokio::runtime::Runtime::new()
            .expect("failed to create tokio runtime")
            .block_on(fut),
    }
}

fn p(name: &str, description: &str, required: bool) -> ToolParam {
    ToolParam {
        name: name.to_string(),
        param_type: "string".to_string(),
        description: description.to_string(),
        required,
    }
}

// ── SESSION TOOLS ─────────────────────────────────────────────────────────────
// These tools are only registered when a SharedSession is active.
// They allow Junie and other MCP clients to read/write the shared conversation.

use crate::session::SharedSession;

pub struct SessionReadTool {
    session: std::sync::Arc<parking_lot::Mutex<Vec<crate::providers::ChatMessage>>>,
}

impl SessionReadTool {
    pub fn new(session: &SharedSession) -> Self {
        Self { session: std::sync::Arc::clone(&session.messages) }
    }
}

impl Tool for SessionReadTool {
    fn name(&self) -> &'static str { "session_read" }
    fn description(&self) -> &'static str {
        "Read the current shared conversation history as a JSON array of {role, content} messages. \
         Use this to get context from an ongoing terminal or Telegram session."
    }
    fn parameters(&self) -> Vec<ToolParam> { vec![] }
    fn execute(&self, _: &HashMap<String, String>) -> ToolResult {
        let messages = self.session.lock();
        match serde_json::to_string_pretty(&*messages) {
            Ok(json) => ToolResult::ok(json),
            Err(e) => ToolResult::err(format!("Failed to serialize session: {e}")),
        }
    }
}

pub struct SessionWriteTool {
    session: std::sync::Arc<parking_lot::Mutex<Vec<crate::providers::ChatMessage>>>,
    session_handle: SharedSession,
}

impl SessionWriteTool {
    pub fn new(session: &SharedSession) -> Self {
        Self {
            session: std::sync::Arc::clone(&session.messages),
            session_handle: session.clone(),
        }
    }
}

impl Tool for SessionWriteTool {
    fn name(&self) -> &'static str { "session_write" }
    fn description(&self) -> &'static str {
        "Append a message to the shared conversation history. \
         Use role 'user' to inject a user message or 'assistant' to inject context. \
         This persists across terminal and Telegram frontends."
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("role", "Message role: 'user' or 'assistant'", true),
            p("content", "Message content to append", true),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let role = match args.get("role") {
            Some(r) => r.clone(),
            None => return ToolResult::err("Missing 'role'"),
        };
        let content = match args.get("content") {
            Some(c) => c.clone(),
            None => return ToolResult::err("Missing 'content'"),
        };
        if role != "user" && role != "assistant" {
            return ToolResult::err("role must be 'user' or 'assistant'");
        }
        let msg = crate::providers::ChatMessage { role, content };
        self.session_handle.push(msg);
        ToolResult::ok(format!("Message added to session (now {} messages)", self.session.lock().len()))
    }
}

// ── LORE TOOLS (persistent memory across sessions) ──────────────────────────
// The Lore is Mithril's long-term memory: notes, TODOs, and context that
// survive across sessions. Stored at .mithril/lore.md in the project root.

pub struct LoreWriteTool;
impl Default for LoreWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

impl LoreWriteTool {
    pub fn new() -> Self { Self }
}

impl Tool for LoreWriteTool {
    fn name(&self) -> &'static str { "lore_write" }
    fn description(&self) -> &'static str {
        "Write a note to the project's persistent memory (Lore). Use this to record TODOs, decisions, known issues, or anything that should survive across sessions. Notes are timestamped and appended to .mithril/lore.md."
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("content", "The note/TODO/decision to record", true),
            p("category", "Category tag: todo, decision, issue, note (optional, default: note)", false),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let content = match args.get("content") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::err("Missing 'content'"),
        };
        let category = args.get("category")
            .map(|s| s.as_str())
            .unwrap_or("note");

        let lore_path = std::path::Path::new(".mithril").join("lore.md");

        // Create .mithril/ if needed
        if let Some(parent) = lore_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // Timestamp
        let now = chrono_lite_now();

        let entry = format!("\n## [{category}] {now}\n\n{content}\n");

        // Append
        use std::io::Write;
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&lore_path)
        {
            Ok(mut f) => {
                if f.write_all(entry.as_bytes()).is_ok() {
                    ToolResult::ok(format!("📜 Lore recorded: [{}] {}", category, truncate_str_impl(content, 60)))
                } else {
                    ToolResult::err("Failed to write to lore file")
                }
            }
            Err(e) => ToolResult::err(format!("Cannot open lore file: {}", e)),
        }
    }
}

pub struct LoreReadTool;
impl Default for LoreReadTool {
    fn default() -> Self {
        Self::new()
    }
}

impl LoreReadTool {
    pub fn new() -> Self { Self }
}

impl Tool for LoreReadTool {
    fn name(&self) -> &'static str { "lore_read" }
    fn description(&self) -> &'static str {
        "Read the project's persistent memory (Lore). Returns all recorded notes, TODOs, decisions, and issues that were saved across sessions."
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("category", "Filter by category: todo, decision, issue, note, or 'all' (optional, default: all)", false),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let category_filter = args.get("category")
            .map(|s| s.as_str())
            .unwrap_or("all");

        let lore_path = std::path::Path::new(".mithril").join("lore.md");
        if !lore_path.exists() {
            return ToolResult::ok("📜 The Lore is empty. No records yet.");
        }

        match std::fs::read_to_string(&lore_path) {
            Ok(content) => {
                if category_filter == "all" {
                    if content.trim().is_empty() {
                        ToolResult::ok("📜 The Lore is empty. No records yet.")
                    } else {
                        ToolResult::ok(content)
                    }
                } else {
                    // Filter sections by category
                    let marker = format!("[{}]", category_filter);
                    let filtered: Vec<&str> = content
                        .split("\n## ")
                        .filter(|s| s.contains(&marker))
                        .collect();
                    if filtered.is_empty() {
                        ToolResult::ok(format!("📜 No lore entries with category '{}'.", category_filter))
                    } else {
                        ToolResult::ok(filtered.iter().map(|s| format!("## {}", s)).collect::<Vec<_>>().join("\n"))
                    }
                }
            }
            Err(e) => ToolResult::err(format!("Cannot read lore: {}", e)),
        }
    }
}

/// Simple timestamp without pulling in chrono crate
fn chrono_lite_now() -> String {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Simple ISO-ish format from epoch
    let days = secs / 86400;
    let years_approx = 1970 + days / 365;
    let remainder_days = days % 365;
    let months_approx = remainder_days / 30 + 1;
    let day_approx = remainder_days % 30 + 1;
    let hour = (secs % 86400) / 3600;
    let min = (secs % 3600) / 60;
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC",
        years_approx, months_approx, day_approx, hour, min)
}

fn truncate_str_impl(s: &str, max: usize) -> String {
    if s.len() > max { format!("{}…", &s[..max]) } else { s.to_string() }
}


#[cfg(test)]
#[allow(unused_imports)]
mod edit_tests {
    use super::*;
    use super::super::file_tools::{parse_hunk_header, parse_edit_blocks, apply_unified_patch};

    #[test]
    fn test_parse_single_edit_block() {
        let input = "<<<<<<< SEARCH\nold text\n=======\nnew text\n>>>>>>> REPLACE";
        let blocks = parse_edit_blocks(input).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, "old text");
        assert_eq!(blocks[0].1, "new text");
    }

    #[test]
    fn test_parse_multiple_edit_blocks() {
        let input = "<<<<<<< SEARCH\nfirst\n=======\nFIRST\n>>>>>>> REPLACE\n<<<<<<< SEARCH\nsecond\n=======\nSECOND\n>>>>>>> REPLACE";
        let blocks = parse_edit_blocks(input).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].0, "first");
        assert_eq!(blocks[0].1, "FIRST");
        assert_eq!(blocks[1].0, "second");
        assert_eq!(blocks[1].1, "SECOND");
    }

    #[test]
    fn test_parse_multiline_search_replace() {
        let input = "<<<<<<< SEARCH\nline 1\nline 2\nline 3\n=======\nreplaced 1\nreplaced 2\n>>>>>>> REPLACE";
        let blocks = parse_edit_blocks(input).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, "line 1\nline 2\nline 3");
        assert_eq!(blocks[0].1, "replaced 1\nreplaced 2");
    }

    #[test]
    fn test_parse_empty_replacement() {
        let input = "<<<<<<< SEARCH\ndelete this\n=======\n>>>>>>> REPLACE";
        let blocks = parse_edit_blocks(input).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, "delete this");
        assert_eq!(blocks[0].1, "");
    }

    #[test]
    fn test_parse_missing_separator_returns_error() {
        let input = "<<<<<<< SEARCH\nno separator here\n>>>>>>> REPLACE";
        let result = parse_edit_blocks(input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing ======="));
    }

    #[test]
    fn test_parse_missing_close_marker_returns_error() {
        let input = "<<<<<<< SEARCH\nsome text\n=======\nreplacement\n";
        let result = parse_edit_blocks(input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing >>>>>>> REPLACE"));
    }

    #[test]
    fn test_parse_no_blocks_returns_empty() {
        let input = "just some random text";
        let blocks = parse_edit_blocks(input).unwrap();
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn test_apply_unified_patch_add_line() {
        let content = "line 1\nline 2\nline 3";
        let patch = "@@ -1,3 +1,4 @@\n line 1\n+inserted\n line 2\n line 3";
        let result = apply_unified_patch(content, patch).unwrap();
        assert!(result.contains("inserted"));
        assert!(result.contains("line 1"));
    }

    #[test]
    fn test_apply_unified_patch_remove_line() {
        let content = "line 1\nline 2\nline 3";
        let patch = "@@ -1,3 +1,2 @@\n line 1\n-line 2\n line 3";
        let result = apply_unified_patch(content, patch).unwrap();
        assert!(!result.contains("line 2"));
        assert!(result.contains("line 1"));
        assert!(result.contains("line 3"));
    }

    #[test]
    fn test_apply_unified_patch_replace_line() {
        let content = "hello\nworld\nfoo";
        let patch = "@@ -1,3 +1,3 @@\n hello\n-world\n+WORLD\n foo";
        let result = apply_unified_patch(content, patch).unwrap();
        assert!(result.contains("WORLD"));
        assert!(!result.contains("world"));
    }

    #[test]
    fn test_parse_hunk_header() {
        let result = parse_hunk_header("@@ -7,6 +7,8 @@");
        assert_eq!(result, Some((7, 7)));
    }

    #[test]
    fn test_parse_hunk_header_with_context() {
        let result = parse_hunk_header("@@ -1,3 +1,4 @@ fn main");
        assert_eq!(result, Some((1, 1)));
    }

    #[test]
    fn test_chrono_lite_now_format() {
        let ts = chrono_lite_now();
        // Should be roughly "YYYY-MM-DD HH:MM UTC"
        assert!(ts.contains("UTC"));
        assert!(ts.len() > 15);
    }

    #[test]
    fn test_truncate_str_impl_short() {
        assert_eq!(truncate_str_impl("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_impl_long() {
        let result = truncate_str_impl("a very long string that exceeds the limit", 10);
        assert_eq!(result.len(), 13); // 10 + 3-byte "…"
        assert!(result.ends_with("…"));
    }
}

