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
            run_formatter_if_configured(target);
            ToolResult::ok(format!("✅ Written to {target}"))
        } else {
            ToolResult::err(format!("Failed to write to {target}"))
        }
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
            run_formatter_if_configured(target);
            ToolResult::ok(format!("✅ Applied {} edit(s) to {}", blocks.len(), target))
        } else {
            ToolResult::err(format!("Failed to write modified content to {}", target))
        }
    }
}

/// Parse search/replace blocks from the edits string.
/// Returns an error if any block is malformed (missing ======= or >>>>>>> REPLACE).
pub fn parse_edit_blocks(s: &str) -> Result<Vec<(String, String)>, String> {
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
pub fn apply_unified_patch(content: &str, patch: &str) -> Result<String, String> {
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
pub fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
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



// ── FORMATTER HELPER ──────────────────────────────────────────────────────────

/// Run configured formatter for a file based on its extension.
fn run_formatter_if_configured(file_path: &str) {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e));

    if let Some(ext) = ext {
        if let Ok(config) = crate::config::MithrilConfig::load() {
            if let Some(cmd_template) = config.formatters.get(&ext) {
                let cmd = cmd_template.replace("{file}", file_path);
                let _ = std::process::Command::new("sh")
                    .args(["-c", &cmd])
                    .output();
            }
        }
    }
}


