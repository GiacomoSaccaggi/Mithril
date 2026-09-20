//! Glean enterprise search tool.

use std::collections::HashMap;
use std::future::Future;

use crate::providers::glean::GleanProvider;
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

pub struct GleanSearchTool {
    instance_url: String,
    cookies: String,
}

impl GleanSearchTool {
    pub fn new(instance_url: String, cookies: String) -> Self {
        Self {
            instance_url,
            cookies,
        }
    }
}

impl Tool for GleanSearchTool {
    fn name(&self) -> &'static str {
        "glean_search"
    }

    fn description(&self) -> &'static str {
        "Search enterprise documents and knowledge via Glean"
    }

    fn parameters(&self) -> Vec<ToolParam> {
        vec![
            ToolParam {
                name: "query".to_string(),
                param_type: "string".to_string(),
                description: "Search query".to_string(),
                required: true,
            },
            ToolParam {
                name: "page_size".to_string(),
                param_type: "string".to_string(),
                description: "Number of results, default 5".to_string(),
                required: false,
            },
        ]
    }

    fn execute(&self, args: &HashMap<String, String>) -> ToolResult {
        let query = match args.get("query") {
            Some(q) if !q.is_empty() => q.clone(),
            _ => return ToolResult::err("Missing required parameter: query"),
        };

        let page_size: usize = args
            .get("page_size")
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        let provider = GleanProvider::new(&self.instance_url, &self.cookies);

        let result = block_on_async(async move { provider.search(&query, page_size).await });

        match result {
            Ok(results) => {
                if results.is_empty() {
                    return ToolResult::ok("No results found.");
                }

                let output = results
                    .iter()
                    .map(|r| {
                        format!(
                            "### {}\n{}\nSource: {} | {}\n---",
                            r.title, r.snippet, r.datasource, r.url
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");

                ToolResult::ok(output)
            }
            Err(e) => ToolResult::err(format!("Glean search failed: {}", e)),
        }
    }
}
