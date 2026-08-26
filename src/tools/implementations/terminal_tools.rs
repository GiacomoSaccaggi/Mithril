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

