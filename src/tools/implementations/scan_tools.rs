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

