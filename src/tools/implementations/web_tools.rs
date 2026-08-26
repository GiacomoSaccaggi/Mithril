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

