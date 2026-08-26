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
    use super::super::file_tools::{ReadPsiTool, WriteFileTool};
    use crate::tools::registry::Tool;
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

