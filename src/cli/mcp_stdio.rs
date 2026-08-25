//! MCP server over stdin/stdout.
//!
//! Auth: if api_token is configured, the first line must be:
//!   `{"jsonrpc":"2.0","method":"mithril/auth","params":{"token":"TOKEN"},"id":0}`
//! Claude Desktop and Kiro send the API token as an env var or first message.
//! In practice, stdio MCP is used by local trusted clients — token check is optional
//! but available via MITHRIL_API_TOKEN env var.

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::api::mcp::dispatch_mcp;
use crate::config::MithrilConfig;
use crate::operators::{file::FileOperator, scan::ScanOperator};
use crate::tools;

pub async fn run() -> Result<()> {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // H1: load api_token — also check MITHRIL_API_TOKEN env var override
    let config = MithrilConfig::load()?;
    let required_token: Option<String> = std::env::var("MITHRIL_API_TOKEN")
        .ok()
        .or_else(|| config.api_token.clone());

    let tool_registry = std::sync::Arc::new(tools::create_default_registry(&cwd));
    let file_operator = std::sync::Arc::new(FileOperator::new(&cwd));
    let scan_operator = std::sync::Arc::new(ScanOperator::new(&cwd));

    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut authenticated = required_token.is_none(); // true if no token required

    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 { break; } // EOF

        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }

        // H1: check for auth message before processing any other request
        if !authenticated {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if val["method"].as_str() == Some("mithril/auth") {
                    let provided = val["params"]["token"].as_str().unwrap_or("");
                    if required_token.as_deref() == Some(provided) {
                        authenticated = true;
                        let resp = r#"{"jsonrpc":"2.0","id":0,"result":{"authenticated":true}}"#;
                        stdout.write_all(resp.as_bytes()).await?;
                        stdout.write_all(b"\n").await?;
                        stdout.flush().await?;
                    } else {
                        let resp = r#"{"jsonrpc":"2.0","id":0,"error":{"code":-32001,"message":"unauthorized"}}"#;
                        stdout.write_all(resp.as_bytes()).await?;
                        stdout.write_all(b"\n").await?;
                        stdout.flush().await?;
                    }
                    continue;
                }
            }
            // Any non-auth request rejected when not authenticated
            let resp = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32001,"message":"unauthorized: send mithril/auth first"}}"#;
            stdout.write_all(resp.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
            continue;
        }

        let response = dispatch_mcp(trimmed, &tool_registry, &file_operator, &scan_operator);
        stdout.write_all(response.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}
