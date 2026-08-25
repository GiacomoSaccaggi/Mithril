#![allow(dead_code)]
/// All 15 tool implementations. Port of Tools.kt.
use std::collections::HashMap;
use std::future::Future;

use crate::operators::{
    file::FileOperator, git::GitOperator, scan::ScanOperator,
    terminal::TerminalOperator, web::WebOperator,
};
use crate::tools::registry::{Tool, ToolParam, ToolResult};

/// Bridge a future onto a tokio runtime.
/// - If we're already inside a tokio runtime (e.g. axum handler), uses `block_in_place`
///   which yields the current thread to the runtime while blocking.
/// - If there's no runtime (e.g. unit tests), spins up a temporary one.
/// Never panics.
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

// ── FILE TOOLS ──────────────────────────────────────────────────────────────

pub struct ReadPsiTool(pub FileOperator);

impl ReadPsiTool {
    pub fn new(op: FileOperator) -> Self { Self(op) }
}

impl Tool for ReadPsiTool {
    fn name(&self) -> &'static str { "read_psi" }
    fn description(&self) -> &'static str { "Read the content of a file" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![p("target", "Relative file path", true)]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let path = match args.get("target") {
            Some(v) => v,
            None => return ToolResult::err("Missing 'target'"),
        };
        let content = self.0.read_file(path);
        let success = !content.starts_with("Error:");
        ToolResult { success, output: content }
    }
}

pub struct DeleteFileTool(pub FileOperator);

impl DeleteFileTool {
    pub fn new(op: FileOperator) -> Self { Self(op) }
}

impl Tool for DeleteFileTool {
    fn name(&self) -> &'static str { "delete_file" }
    fn description(&self) -> &'static str { "Delete a file" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![p("target", "Relative file path", true)]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let target = match args.get("target") {
            Some(v) => v,
            None => return ToolResult::err("No target file specified"),
        };
        if self.0.delete_file(target) {
            ToolResult::ok(format!("✅ Deleted {target}"))
        } else {
            ToolResult::err(format!("Failed to delete {target}"))
        }
    }
}

pub struct WriteFileTool(pub FileOperator);

impl WriteFileTool {
    pub fn new(op: FileOperator) -> Self { Self(op) }
}

impl Tool for WriteFileTool {
    fn name(&self) -> &'static str { "write_file" }
    fn description(&self) -> &'static str {
        "Write raw content to a file (creates or overwrites). Use this when you already have the final content."
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("target", "Relative file path", true),
            p("content", "Full file content to write", true),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let target = match args.get("target") {
            Some(v) => v,
            None => return ToolResult::err("Missing 'target'"),
        };
        let content = match args.get("content") {
            Some(v) => v,
            None => return ToolResult::err("Missing 'content'"),
        };
        if self.0.write_file(target, content) {
            ToolResult::ok(format!("✅ Written to {target}"))
        } else {
            ToolResult::err(format!("Failed to write to {target}"))
        }
    }
}

// ── TERMINAL TOOLS ───────────────────────────────────────────────────────────

pub struct RunTerminalTool(pub TerminalOperator);

impl RunTerminalTool {
    pub fn new(op: TerminalOperator) -> Self { Self(op) }
}

impl Tool for RunTerminalTool {
    fn name(&self) -> &'static str { "run_terminal" }
    fn description(&self) -> &'static str { "Execute a shell command" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![p("command", "Shell command to execute", true)]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let command = match args.get("command") {
            Some(v) => v.clone(),
            None => return ToolResult::err("No command provided"),
        };
        let op = self.0.clone();
        let result = block_on_async(async move { op.execute(&command).await });
        // exit_code != 0 means the command ran but returned an error — we report it
        // as success=false with the output so the LLM can read stderr and self-correct.
        // success=false only means "the operation produced an error", not "MCP failure".
        let output = if result.exit_code != 0 {
            format!("[exit {}]\n{}", result.exit_code, result.output)
        } else {
            result.output
        };
        ToolResult {
            success: result.exit_code == 0,
            output,
        }
    }
}

// ── WEB TOOLS ────────────────────────────────────────────────────────────────

pub struct WebSearchTool(pub WebOperator);

impl WebSearchTool {
    pub fn new(op: WebOperator) -> Self { Self(op) }
}

impl Tool for WebSearchTool {
    fn name(&self) -> &'static str { "web_search" }
    fn description(&self) -> &'static str { "Search the web via DuckDuckGo" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![p("query", "Search query", true)]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let query = args.get("query")
            .or_else(|| args.get("instruction"))
            .cloned()
            .unwrap_or_default();
        if query.is_empty() {
            return ToolResult::err("No search query provided");
        }
        let op = self.0.clone();
        let result = block_on_async(async move { op.search(&query).await });
        let success = !result.starts_with("Error:");
        ToolResult { success, output: result }
    }
}

pub struct FetchPageTool(pub WebOperator);

impl FetchPageTool {
    pub fn new(op: WebOperator) -> Self { Self(op) }
}

impl Tool for FetchPageTool {
    fn name(&self) -> &'static str { "fetch_page" }
    fn description(&self) -> &'static str { "Fetch and read a URL" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![p("target", "URL to fetch", true)]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let url = match args.get("target") {
            Some(v) => v.clone(),
            None => return ToolResult::err("No URL provided"),
        };
        let op = self.0.clone();
        let result = block_on_async(async move { op.fetch_page(&url).await });
        let success = !result.starts_with("Error:");
        ToolResult { success, output: result }
    }
}

// ── SCAN TOOLS ───────────────────────────────────────────────────────────────

pub struct ListFilesTool(pub ScanOperator);

impl ListFilesTool {
    pub fn new(op: ScanOperator) -> Self { Self(op) }
}

impl Tool for ListFilesTool {
    fn name(&self) -> &'static str { "list_files" }
    fn description(&self) -> &'static str {
        "List project files, optionally filtered by path and extension"
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("target", "Sub-path to list (optional)", false),
            p("extension", "File extension filter (optional)", false),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let result = self.0.list_files(
            args.get("target").map(|s| s.as_str()),
            args.get("extension").map(|s| s.as_str()),
        );
        let success = !result.starts_with("Error:");
        ToolResult { success, output: result }
    }
}

pub struct GrepFilesTool(pub ScanOperator);

impl GrepFilesTool {
    pub fn new(op: ScanOperator) -> Self { Self(op) }
}

impl Tool for GrepFilesTool {
    fn name(&self) -> &'static str { "grep_files" }
    fn description(&self) -> &'static str { "Regex search across project files" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("pattern", "Regex pattern to search", true),
            p("extension", "File extension filter (optional)", false),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let pattern = match args.get("pattern") {
            Some(v) => v,
            None => return ToolResult::err("No grep pattern provided"),
        };
        let result = self.0.grep_files(pattern, args.get("extension").map(|s| s.as_str()));
        let success = !result.starts_with("Error:");
        ToolResult { success, output: result }
    }
}

pub struct FindFileTool(pub ScanOperator);

impl FindFileTool {
    pub fn new(op: ScanOperator) -> Self { Self(op) }
}

impl Tool for FindFileTool {
    fn name(&self) -> &'static str { "find_file" }
    fn description(&self) -> &'static str { "Find files by name fragment" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![p("query", "File name fragment to search", true)]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let name = args.get("query")
            .or_else(|| args.get("target"))
            .cloned()
            .unwrap_or_default();
        if name.is_empty() {
            return ToolResult::err("No file name provided");
        }
        let result = self.0.find_by_name(&name);
        let success = !result.starts_with("Error:") && !result.starts_with("No files matching");
        ToolResult { success, output: result }
    }
}

pub struct FileStatsTool(pub ScanOperator);

impl FileStatsTool {
    pub fn new(op: ScanOperator) -> Self { Self(op) }
}

impl Tool for FileStatsTool {
    fn name(&self) -> &'static str { "file_stats" }
    fn description(&self) -> &'static str { "Show line count and size of a file" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![p("target", "Relative file path", true)]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let target = match args.get("target") {
            Some(v) => v,
            None => return ToolResult::err("No target file specified"),
        };
        let result = self.0.file_stats(target);
        let success = !result.starts_with("Error:");
        ToolResult { success, output: result }
    }
}

// ── GIT TOOLS ────────────────────────────────────────────────────────────────

pub struct GitStatusTool(pub GitOperator);
impl GitStatusTool { pub fn new(op: GitOperator) -> Self { Self(op) } }
impl Tool for GitStatusTool {
    fn name(&self) -> &'static str { "git_status" }
    fn description(&self) -> &'static str { "Show working tree status" }
    fn parameters(&self) -> Vec<ToolParam> { vec![] }
    fn execute(&self, _: &HashMap<String, String>) -> ToolResult {
        ToolResult::ok(self.0.status())
    }
}

pub struct GitLogTool(pub GitOperator);
impl GitLogTool { pub fn new(op: GitOperator) -> Self { Self(op) } }
impl Tool for GitLogTool {
    fn name(&self) -> &'static str { "git_log" }
    fn description(&self) -> &'static str { "Show recent commit history" }
    fn parameters(&self) -> Vec<ToolParam> { vec![] }
    fn execute(&self, _: &HashMap<String, String>) -> ToolResult {
        ToolResult::ok(self.0.log(10))
    }
}

pub struct GitDiffTool(pub GitOperator);
impl GitDiffTool { pub fn new(op: GitOperator) -> Self { Self(op) } }
impl Tool for GitDiffTool {
    fn name(&self) -> &'static str { "git_diff" }
    fn description(&self) -> &'static str {
        "Show uncommitted changes, optionally for a specific file"
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![p("target", "File path (optional, omit for full diff)", false)]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        ToolResult::ok(self.0.diff(args.get("target").map(|s| s.as_str())))
    }
}

pub struct GitBlameTool(pub GitOperator);
impl GitBlameTool { pub fn new(op: GitOperator) -> Self { Self(op) } }
impl Tool for GitBlameTool {
    fn name(&self) -> &'static str { "git_blame" }
    fn description(&self) -> &'static str { "Show per-line authorship of a file" }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![p("target", "Relative file path", true)]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let target = match args.get("target") {
            Some(v) => v,
            None => return ToolResult::err("No target file specified"),
        };
        ToolResult::ok(self.0.blame(target))
    }
}

pub struct GitBranchTool(pub GitOperator);
impl GitBranchTool { pub fn new(op: GitOperator) -> Self { Self(op) } }
impl Tool for GitBranchTool {
    fn name(&self) -> &'static str { "git_branch" }
    fn description(&self) -> &'static str { "Show current branch name" }
    fn parameters(&self) -> Vec<ToolParam> { vec![] }
    fn execute(&self, _: &HashMap<String, String>) -> ToolResult {
        ToolResult::ok(self.0.branch())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_write_and_read_tool() {
        let dir = tempdir().unwrap();
        let file_op = FileOperator::new(dir.path());

        let write = WriteFileTool::new(file_op.clone());
        let mut args = HashMap::new();
        args.insert("target".into(), "test.txt".into());
        args.insert("content".into(), "hello".into());
        let result = write.execute(&args);
        assert!(result.success);

        let read = ReadPsiTool::new(file_op);
        let mut args2 = HashMap::new();
        args2.insert("target".into(), "test.txt".into());
        let result2 = read.execute(&args2);
        assert!(result2.success);
        assert_eq!(result2.output, "hello");
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

// ── EDIT FILE TOOL ───────────────────────────────────────────────────────────

pub struct EditFileTool(pub FileOperator);

impl EditFileTool {
    pub fn new(op: FileOperator) -> Self { Self(op) }
}

impl Tool for EditFileTool {
    fn name(&self) -> &'static str { "edit_file" }
    fn description(&self) -> &'static str {
        "Apply targeted edits to a file using search/replace blocks. Each block finds exact text and replaces it. Preferred over write_file for modifying existing files. Format: <<<<<<< SEARCH\\nexact text to find\\n=======\\nreplacement text\\n>>>>>>> REPLACE"
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("target", "Relative file path to edit", true),
            p("edits", "One or more search/replace blocks", true),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let target = match args.get("target") {
            Some(v) => v,
            None => return ToolResult::err("Missing 'target'"),
        };
        let edits_str = match args.get("edits") {
            Some(v) => v,
            None => return ToolResult::err("Missing 'edits'"),
        };

        // Read current file
        let content = self.0.read_file(target);
        if content.starts_with("Error:") {
            return ToolResult::err(content);
        }

        // Parse edit blocks
        let blocks = match parse_edit_blocks(edits_str) {
            Ok(b) if b.is_empty() => {
                return ToolResult::err("No edit blocks found. Use format: <<<<<<< SEARCH\n...\n=======\n...\n>>>>>>> REPLACE");
            }
            Ok(b) => b,
            Err(e) => return ToolResult::err(e),
        };

        // Apply edits atomically — verify all searches exist first
        let mut modified = content.clone();
        for (i, (search, replace)) in blocks.iter().enumerate() {
            if search.is_empty() {
                return ToolResult::err(format!("Edit block {} has empty search text", i + 1));
            }
            match modified.find(search.as_str()) {
                Some(pos) => {
                    modified = format!(
                        "{}{}{}",
                        &modified[..pos],
                        replace,
                        &modified[pos + search.len()..]
                    );
                }
                None => {
                    let preview = if search.len() > 60 {
                        format!("{}…", &search[..60])
                    } else {
                        search.clone()
                    };
                    return ToolResult::err(format!(
                        "Edit block {} failed: search text not found in '{}': \"{}\"",
                        i + 1,
                        target,
                        preview
                    ));
                }
            }
        }

        // Write back
        if self.0.write_file(target, &modified) {
            ToolResult::ok(format!("✅ Applied {} edit(s) to {}", blocks.len(), target))
        } else {
            ToolResult::err(format!("Failed to write modified content to {}", target))
        }
    }
}

/// Parse search/replace blocks from the edits string.
/// Returns an error if any block is malformed (missing ======= or >>>>>>> REPLACE).
fn parse_edit_blocks(s: &str) -> Result<Vec<(String, String)>, String> {
    let mut blocks = Vec::new();
    let chunks: Vec<&str> = s.split("<<<<<<< SEARCH").collect();

    // First chunk is text before the first marker (usually empty), skip it
    for (i, chunk) in chunks.iter().enumerate().skip(1) {
        let chunk = chunk.trim_start_matches('\n');

        // Split on the separator
        let parts: Vec<&str> = chunk.splitn(2, "=======").collect();
        if parts.len() != 2 {
            return Err(format!(
                "Edit block {} is malformed: missing ======= separator",
                i
            ));
        }

        let search = parts[0].trim_end_matches('\n');

        // Get the replace part (everything before >>>>>>> REPLACE)
        let replace_raw = parts[1].trim_start_matches('\n');
        let replace = match replace_raw.find(">>>>>>> REPLACE") {
            Some(pos) => replace_raw[..pos].trim_end_matches('\n'),
            None => {
                return Err(format!(
                    "Edit block {} is malformed: missing >>>>>>> REPLACE closing marker",
                    i
                ));
            }
        };

        blocks.push((search.to_string(), replace.to_string()));
    }

    Ok(blocks)
}

// ── CODE INTELLIGENCE TOOLS ─────────────────────────────────────────────────

pub struct SearchSymbolsTool(pub ScanOperator);

impl SearchSymbolsTool {
    pub fn new(op: ScanOperator) -> Self { Self(op) }
}

impl Tool for SearchSymbolsTool {
    fn name(&self) -> &'static str { "search_symbols" }
    fn description(&self) -> &'static str {
        "Search for symbol definitions (functions, classes, structs, traits, etc.) across the project. Returns file paths and line numbers where matching symbols are defined."
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("query", "Symbol name or pattern to search for", true),
            p("extension", "File extension filter (optional, e.g. 'rs', 'py')", false),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let query = match args.get("query") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::err("Missing 'query'"),
        };
        let ext_filter = args.get("extension").map(|s| s.as_str());
        let result = search_symbols_impl(&self.0, query, ext_filter);
        if result.is_empty() {
            ToolResult::ok(format!("No symbols matching '{}' found.", query))
        } else {
            ToolResult::ok(result)
        }
    }
}

pub struct DocumentOutlineTool(pub FileOperator);

impl DocumentOutlineTool {
    pub fn new(op: FileOperator) -> Self { Self(op) }
}

impl Tool for DocumentOutlineTool {
    fn name(&self) -> &'static str { "document_outline" }
    fn description(&self) -> &'static str {
        "Get the structural outline of a file: all function, class, struct, trait, and method definitions with line numbers. Useful for understanding file organization without reading the full content."
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![p("target", "File path to outline", true)]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let target = match args.get("target") {
            Some(v) => v,
            None => return ToolResult::err("Missing 'target'"),
        };
        let content = self.0.read_file(target);
        if content.starts_with("Error:") {
            return ToolResult::err(content);
        }
        let outline = extract_outline(&content, target);
        if outline.is_empty() {
            ToolResult::ok("(no symbols found in this file)")
        } else {
            ToolResult::ok(outline)
        }
    }
}

/// Search for symbol definitions across project files.
fn search_symbols_impl(scan_op: &ScanOperator, query: &str, ext_filter: Option<&str>) -> String {
    use regex::Regex;

    let files = scan_op.walk_source_files();
    let query_lower = query.to_lowercase();
    let mut results: Vec<String> = Vec::new();

    let symbol_pattern = r"(?:pub\s+)?(?:async\s+)?(?:fn|struct|enum|trait|impl|mod|class|def|function|interface|type|const|let|var|object|fun|val)\s+(\w+)";
    let re = match Regex::new(symbol_pattern) {
        Ok(r) => r,
        Err(_) => return "Error: regex compilation failed".to_string(),
    };

    for file_path in &files {
        if let Some(ext) = ext_filter {
            if !file_path.ends_with(&format!(".{}", ext)) {
                continue;
            }
        }

        let full_path = scan_op.base_path().join(file_path);
        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for (line_num, line) in content.lines().enumerate() {
            for cap in re.captures_iter(line) {
                if let Some(m) = cap.get(1) {
                    let symbol_name = m.as_str();
                    if symbol_name.to_lowercase().contains(&query_lower) {
                        results.push(format!(
                            "{}:{} → {}",
                            file_path,
                            line_num + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }

        if results.len() > 50 {
            results.push("... (truncated, too many results)".to_string());
            break;
        }
    }

    results.join("\n")
}

/// Extract a structural outline from file content.
fn extract_outline(content: &str, path: &str) -> String {
    use regex::Regex;

    let ext = path.rsplit('.').next().unwrap_or("");
    let pattern = match ext {
        "rs" => r"^\s*(pub\s+)?(async\s+)?(fn|struct|enum|trait|impl|mod)\s+(\w+)",
        "py" => r"^(\s*)(class|def)\s+(\w+)",
        "js" | "ts" | "jsx" | "tsx" => r"^\s*(export\s+)?(async\s+)?(function|class|const|interface|type)\s+(\w+)",
        "go" => r"^(func|type)\s+(\w+)",
        "java" | "kt" | "scala" => r"^\s*(public|private|protected)?\s*(class|interface|fun|val|var|object|enum)\s+(\w+)",
        _ => r"^\s*(pub\s+)?(fn|function|def|class|struct|interface|type)\s+(\w+)",
    };

    let re = match Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => return String::new(),
    };

    let mut lines: Vec<String> = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        if re.is_match(line) {
            let indent = line.len() - line.trim_start().len();
            let prefix = if indent > 0 { "  " } else { "" };
            lines.push(format!(
                "{:4} │ {}{}",
                line_num + 1,
                prefix,
                line.trim()
            ));
        }
    }

    lines.join("\n")
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

// ── PATCH TOOL (unified diff apply) ─────────────────────────────────────────
// For when the LLM wants to express changes as a unified diff patch.

pub struct PatchTool(pub FileOperator);
impl PatchTool {
    pub fn new(op: FileOperator) -> Self { Self(op) }
}

impl Tool for PatchTool {
    fn name(&self) -> &'static str { "apply_patch" }
    fn description(&self) -> &'static str {
        "Apply a unified diff patch to a file. Use when expressing changes as +/- lines is more natural than search/replace blocks. The patch format uses --- a/file, +++ b/file, @@ markers, -old, +new lines."
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("target", "File path to patch", true),
            p("patch", "Unified diff patch content (only the hunks, no file headers needed)", true),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let target = match args.get("target") {
            Some(v) => v,
            None => return ToolResult::err("Missing 'target'"),
        };
        let patch_str = match args.get("patch") {
            Some(v) => v,
            None => return ToolResult::err("Missing 'patch'"),
        };

        let content = self.0.read_file(target);
        if content.starts_with("Error:") {
            return ToolResult::err(content);
        }

        match apply_unified_patch(&content, patch_str) {
            Ok(result) => {
                if self.0.write_file(target, &result) {
                    ToolResult::ok(format!("✅ Patch applied to {}", target))
                } else {
                    ToolResult::err(format!("Failed to write patched content to {}", target))
                }
            }
            Err(e) => ToolResult::err(format!("Patch failed: {}", e)),
        }
    }
}

/// Apply a simplified unified diff patch.
/// Supports context lines, additions (+), and deletions (-).
fn apply_unified_patch(content: &str, patch: &str) -> Result<String, String> {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let patch_lines: Vec<&str> = patch.lines().collect();

    let mut i = 0;
    while i < patch_lines.len() {
        let line = patch_lines[i];

        // Skip file headers and hunk markers (we apply sequentially)
        if line.starts_with("---") || line.starts_with("+++") {
            i += 1;
            continue;
        }

        // Parse @@ -start,count +start,count @@
        if line.starts_with("@@") {
            if let Some(hunk_info) = parse_hunk_header(line) {
                let start_line = hunk_info.0.saturating_sub(1); // 0-indexed
                let mut pos = start_line;
                i += 1;

                while i < patch_lines.len() && !patch_lines[i].starts_with("@@") {
                    let pline = patch_lines[i];
                    if let Some(stripped) = pline.strip_prefix('-') {
                        // Deletion: remove this line at pos
                        if pos < lines.len() && lines[pos].trim() == stripped.trim() {
                            lines.remove(pos);
                        } else if pos < lines.len() {
                            // Fuzzy: try to find it nearby
                            let found = (pos..lines.len().min(pos + 5))
                                .find(|&j| lines[j].trim() == stripped.trim());
                            if let Some(j) = found {
                                lines.remove(j);
                                pos = j;
                            } else {
                                return Err(format!(
                                    "Line {} not found for deletion near line {}: '{}'",
                                    pos + 1, start_line + 1, stripped.trim()
                                ));
                            }
                        }
                    } else if let Some(stripped) = pline.strip_prefix('+') {
                        // Addition: insert at pos
                        lines.insert(pos, stripped.to_string());
                        pos += 1;
                    } else {
                        // Context line (space prefix or no prefix): advance
                        pos += 1;
                    }
                    i += 1;
                }
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    Ok(lines.join("\n"))
}

/// Parse a hunk header like "@@ -7,6 +7,8 @@" into (old_start, new_start)
fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    // Format: @@ -OLD_START,COUNT +NEW_START,COUNT @@
    let stripped = line.trim_start_matches("@@ ").trim_end_matches(" @@")
        .trim_end_matches(|c: char| c != '@');
    let _stripped = stripped.trim_end_matches(" @@").trim_end_matches(|c: char| !c.is_ascii_digit() && c != ',' && c != '+' && c != '-' && c != ' ');

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 { return None; }

    let old_part = parts[1].trim_start_matches('-');
    let new_part = parts[2].trim_start_matches('+');

    let old_start: usize = old_part.split(',').next()?.parse().ok()?;
    let new_start: usize = new_part.split(',').next()?.parse().ok()?;

    Some((old_start, new_start))
}

#[cfg(test)]
mod edit_tests {
    use super::*;

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

#[cfg(test)]
mod tool_execution_tests {
    use super::*;
    use tempfile::tempdir;

    fn make_file_op(dir: &std::path::Path) -> FileOperator {
        FileOperator::new(dir)
    }

    #[test]
    fn test_read_psi_success() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();
        let tool = ReadPsiTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "test.txt".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert_eq!(result.output, "hello world");
    }

    #[test]
    fn test_read_psi_not_found() {
        let dir = tempdir().unwrap();
        let tool = ReadPsiTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "missing.txt".into());
        let result = tool.execute(&args);
        assert!(!result.success);
    }

    #[test]
    fn test_read_psi_missing_arg() {
        let dir = tempdir().unwrap();
        let tool = ReadPsiTool::new(make_file_op(dir.path()));
        let result = tool.execute(&HashMap::new());
        assert!(!result.success);
        assert!(result.output.contains("Missing"));
    }

    #[test]
    fn test_write_file_creates() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "new.txt".into());
        args.insert("content".into(), "created content".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert_eq!(std::fs::read_to_string(dir.path().join("new.txt")).unwrap(), "created content");
    }

    #[test]
    fn test_delete_file_success() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("del.txt"), "x").unwrap();
        let tool = DeleteFileTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "del.txt".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert!(!dir.path().join("del.txt").exists());
    }

    #[test]
    fn test_edit_file_tool_success() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("edit.txt"), "foo\nbar\nbaz").unwrap();
        let tool = EditFileTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "edit.txt".into());
        args.insert("edits".into(), "<<<<<<< SEARCH\nbar\n=======\nBAR\n>>>>>>> REPLACE".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert!(result.output.contains("Applied 1 edit"));
        let content = std::fs::read_to_string(dir.path().join("edit.txt")).unwrap();
        assert_eq!(content, "foo\nBAR\nbaz");
    }

    #[test]
    fn test_edit_file_tool_not_found_search() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("e.txt"), "hello").unwrap();
        let tool = EditFileTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "e.txt".into());
        args.insert("edits".into(), "<<<<<<< SEARCH\nNOPE\n=======\nYES\n>>>>>>> REPLACE".into());
        let result = tool.execute(&args);
        assert!(!result.success);
        assert!(result.output.contains("not found"));
    }

    #[test]
    fn test_edit_file_multiple_edits() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("m.txt"), "aaa\nbbb\nccc").unwrap();
        let tool = EditFileTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "m.txt".into());
        args.insert("edits".into(), "<<<<<<< SEARCH\naaa\n=======\nAAA\n>>>>>>> REPLACE\n<<<<<<< SEARCH\nccc\n=======\nCCC\n>>>>>>> REPLACE".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert!(result.output.contains("Applied 2 edit"));
    }

    #[test]
    fn test_patch_tool_add_line() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("p.txt"), "line 1\nline 2\nline 3").unwrap();
        let tool = PatchTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "p.txt".into());
        args.insert("patch".into(), "@@ -1,3 +1,4 @@\n line 1\n+inserted\n line 2\n line 3".into());
        let result = tool.execute(&args);
        assert!(result.success);
        let content = std::fs::read_to_string(dir.path().join("p.txt")).unwrap();
        assert!(content.contains("inserted"));
    }

    #[test]
    fn test_lore_write_creates_file() {
        let dir = tempdir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let tool = LoreWriteTool::new();
        let mut args = HashMap::new();
        args.insert("content".into(), "remember this".into());
        args.insert("category".into(), "todo".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert!(dir.path().join(".mithril/lore.md").exists());
        let lore = std::fs::read_to_string(dir.path().join(".mithril/lore.md")).unwrap();
        assert!(lore.contains("remember this"));
        assert!(lore.contains("[todo]"));
    }

    #[test]
    fn test_lore_read_empty() {
        let dir = tempdir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let tool = LoreReadTool::new();
        let result = tool.execute(&HashMap::new());
        assert!(result.success);
        assert!(result.output.contains("empty"));
    }

    #[test]
    fn test_document_outline_rust_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\nstruct Foo {}\nimpl Foo {\n    fn bar() {}\n}").unwrap();
        let tool = DocumentOutlineTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "lib.rs".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert!(result.output.contains("hello"));
        assert!(result.output.contains("Foo"));
    }

    #[test]
    fn test_list_files_tool() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();
        let tool = ListFilesTool::new(crate::operators::scan::ScanOperator::new(dir.path()));
        let result = tool.execute(&HashMap::new());
        assert!(result.success);
    }

    #[test]
    fn test_grep_files_tool() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("x.rs"), "fn hello_world() {}").unwrap();
        let tool = GrepFilesTool::new(crate::operators::scan::ScanOperator::new(dir.path()));
        let mut args = HashMap::new();
        args.insert("pattern".into(), "hello".into());
        let result = tool.execute(&args);
        assert!(result.success);
    }

    #[test]
    fn test_file_stats_tool() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("s.txt"), "line1\nline2\nline3\n").unwrap();
        let tool = FileStatsTool::new(crate::operators::scan::ScanOperator::new(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "s.txt".into());
        let result = tool.execute(&args);
        assert!(result.success);
    }
}
