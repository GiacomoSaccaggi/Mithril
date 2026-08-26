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

// ── GLOB TOOL ─────────────────────────────────────────────────────────────────

pub struct GlobTool {
    base_path: std::path::PathBuf,
}

impl GlobTool {
    pub fn new(base_path: impl Into<std::path::PathBuf>) -> Self {
        Self { base_path: base_path.into() }
    }
}

impl Tool for GlobTool {
    fn name(&self) -> &'static str { "glob_files" }
    fn description(&self) -> &'static str {
        "Find files matching a glob pattern (e.g. **/*.rs, src/**/*.{ts,tsx}). Returns matching file paths sorted by modification time."
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("pattern", "Glob pattern to match (e.g. **/*.rs, src/**/*.ts)", true),
            p("limit", "Maximum number of results (default: 50)", false),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let pattern = match args.get("pattern") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::err("Missing 'pattern'"),
        };
        let limit: usize = args.get("limit")
            .and_then(|s| s.parse().ok())
            .unwrap_or(50);

        let full_pattern = self.base_path.join(pattern).to_string_lossy().to_string();

        match glob::glob(&full_pattern) {
            Ok(paths) => {
                let mut files: Vec<String> = paths
                    .filter_map(|p| p.ok())
                    .filter(|p| p.is_file())
                    .filter_map(|p| pathdiff::diff_paths(&p, &self.base_path))
                    .map(|p| p.to_string_lossy().to_string())
                    .take(limit)
                    .collect();
                files.sort();
                if files.is_empty() {
                    ToolResult::ok("No files matched the pattern.")
                } else {
                    ToolResult::ok(files.join("\n"))
                }
            }
            Err(e) => ToolResult::err(format!("Invalid glob pattern: {}", e)),
        }
    }
}

// ── TODO TOOL ─────────────────────────────────────────────────────────────────

pub struct TodoWriteTool;
impl Default for TodoWriteTool {
    fn default() -> Self { Self }
}

impl TodoWriteTool {
    pub fn new() -> Self { Self }
}

impl Tool for TodoWriteTool {
    fn name(&self) -> &'static str { "todo_write" }
    fn description(&self) -> &'static str {
        "Create or update a todo list for tracking multi-step tasks. Actions: create (new list), add (add items), complete (mark done), list (show all)."
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("action", "One of: create, add, complete, list", true),
            p("items", "Comma-separated task descriptions (for create/add)", false),
            p("ids", "Comma-separated task IDs to mark complete (for complete)", false),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let action = match args.get("action") {
            Some(v) => v.as_str(),
            None => return ToolResult::err("Missing 'action'"),
        };

        let todo_path = std::path::Path::new(".mithril").join("todos.json");
        let _ = std::fs::create_dir_all(".mithril");

        // Load existing todos
        let mut todos: Vec<(String, bool)> = std::fs::read_to_string(&todo_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        match action {
            "create" => {
                let items = match args.get("items") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::err("Missing 'items' for create"),
                };
                todos = items.split(',')
                    .map(|s| (s.trim().to_string(), false))
                    .collect();
                let json = serde_json::to_string_pretty(&todos).unwrap_or_default();
                let _ = std::fs::write(&todo_path, &json);
                ToolResult::ok(format!("Created todo list with {} items.", todos.len()))
            }
            "add" => {
                let items = match args.get("items") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::err("Missing 'items' for add"),
                };
                for item in items.split(',') {
                    todos.push((item.trim().to_string(), false));
                }
                let json = serde_json::to_string_pretty(&todos).unwrap_or_default();
                let _ = std::fs::write(&todo_path, &json);
                ToolResult::ok(format!("Added items. Total: {} tasks.", todos.len()))
            }
            "complete" => {
                let ids = match args.get("ids") {
                    Some(v) if !v.is_empty() => v,
                    _ => return ToolResult::err("Missing 'ids' for complete"),
                };
                for id_str in ids.split(',') {
                    if let Ok(id) = id_str.trim().parse::<usize>() {
                        if id > 0 && id <= todos.len() {
                            todos[id - 1].1 = true;
                        }
                    }
                }
                let json = serde_json::to_string_pretty(&todos).unwrap_or_default();
                let _ = std::fs::write(&todo_path, &json);
                let done = todos.iter().filter(|t| t.1).count();
                ToolResult::ok(format!("Updated. {}/{} complete.", done, todos.len()))
            }
            "list" => {
                if todos.is_empty() {
                    ToolResult::ok("No todos. Use action=create to start a list.")
                } else {
                    let list: String = todos.iter().enumerate()
                        .map(|(i, (desc, done))| {
                            let mark = if *done { "[x]" } else { "[ ]" };
                            format!("{}. {} {}", i + 1, mark, desc)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    ToolResult::ok(list)
                }
            }
            _ => ToolResult::err("Unknown action. Use: create, add, complete, list"),
        }
    }
}

// ── QUESTION TOOL ─────────────────────────────────────────────────────────────

pub struct QuestionTool;
impl Default for QuestionTool {
    fn default() -> Self { Self }
}

impl QuestionTool {
    pub fn new() -> Self { Self }
}

impl Tool for QuestionTool {
    fn name(&self) -> &'static str { "question" }
    fn description(&self) -> &'static str {
        "Ask the user a question and wait for their answer. Use this when you need clarification, user preferences, or a decision before proceeding."
    }
    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            p("question", "The question to ask the user", true),
            p("options", "Comma-separated list of options (optional, user can type freely)", false),
        ]
    }
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        use std::io::{self, Write};

        let question = match args.get("question") {
            Some(v) if !v.is_empty() => v,
            _ => return ToolResult::err("Missing 'question'"),
        };

        // Print the question
        eprintln!();
        eprintln!("  \x1b[1;36m? {}\x1b[0m", question);

        // Print options if provided
        if let Some(options) = args.get("options") {
            if !options.is_empty() {
                for (i, opt) in options.split(',').enumerate() {
                    eprintln!("    \x1b[33m{}\x1b[0m) {}", i + 1, opt.trim());
                }
            }
        }

        eprint!("  \x1b[2m> \x1b[0m");
        io::stderr().flush().ok();

        // Read answer from stdin
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                let answer = input.trim().to_string();
                if answer.is_empty() {
                    ToolResult::ok("(no answer provided)")
                } else {
                    ToolResult::ok(format!("User answered: {}", answer))
                }
            }
            Err(e) => ToolResult::err(format!("Failed to read input: {}", e)),
        }
    }
}


