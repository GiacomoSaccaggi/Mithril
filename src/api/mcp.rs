use std::collections::HashMap;
use std::sync::Arc;

use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::api::server::AppState;
use crate::operators::{file::FileOperator, scan::ScanOperator};
use crate::tools::registry::ToolRegistry;

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_VERSION: &str = "0.3.0";

/// Port of McpRouter.kt — dispatches JSON-RPC 2.0 requests.
pub async fn handle_mcp(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
    // H5 + C2: constant-time bearer token check to prevent timing attacks
    if let Some(required_token) = &state.api_token {
        use subtle::ConstantTimeEq;
        let provided = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        // Pad both to the same length before comparison to avoid length leakage
        let provided_bytes = provided.as_bytes();
        let required_bytes = required_token.as_bytes();
        let authorized = if provided_bytes.len() == required_bytes.len() {
            provided_bytes.ct_eq(required_bytes).into()
        } else {
            // Still do a dummy comparison to avoid early-exit timing leak
            let dummy = vec![0u8; required_bytes.len()];
            let _ = dummy.as_slice().ct_eq(required_bytes);
            false
        };
        if !authorized {
            return Err((
                axum::http::StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "unauthorized" })),
            ));
        }
    }
    let response = dispatch_mcp_value(
        &body,
        &state.tool_registry,
        &state.file_operator,
        &state.scan_operator,
    );
    Ok(Json(response))
}

/// Dispatch from a raw JSON string (used by mcp_stdio).
pub fn dispatch_mcp(
    json_str: &str,
    tool_registry: &Arc<ToolRegistry>,
    file_operator: &Arc<FileOperator>,
    scan_operator: &Arc<ScanOperator>,
) -> String {
    let body: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            return serde_json::to_string(&error_response(
                Value::Null,
                -32700,
                &format!("Parse error: {e}"),
            ))
            .unwrap_or_default();
        }
    };

    let result = dispatch_mcp_value(&body, tool_registry, file_operator, scan_operator);
    serde_json::to_string(&result).unwrap_or_default()
}

fn dispatch_mcp_value(
    body: &Value,
    tool_registry: &ToolRegistry,
    file_operator: &FileOperator,
    scan_operator: &ScanOperator,
) -> Value {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body["method"].as_str().unwrap_or("");
    let params = body.get("params");

    // Notifications require no response
    if body.get("id").is_none() && method.starts_with("notifications/") {
        return Value::Null;
    }

    match method {
        "initialize" => handle_initialize(&id),
        "notifications/initialized" => Value::Null,
        "tools/list" => handle_tools_list(&id, tool_registry),
        "tools/call" => handle_tools_call(&id, params, tool_registry),
        "resources/list" => handle_resources_list(&id, scan_operator),
        "resources/read" => handle_resources_read(&id, params, file_operator),
        "" => error_response(id, -32600, "Invalid request: missing method"),
        _ => error_response(id, -32601, &format!("Method not found: {method}")),
    }
}

fn handle_initialize(id: &Value) -> Value {
    result_response(id.clone(), json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {
            "tools": {},
            "resources": {}
        },
        "serverInfo": {
            "name": "mithril",
            "version": SERVER_VERSION
        }
    }))
}

fn handle_tools_list(id: &Value, tool_registry: &ToolRegistry) -> Value {
    result_response(id.clone(), json!({
        "tools": tool_registry.to_mcp_tool_list()
    }))
}

fn handle_tools_call(id: &Value, params: Option<&Value>, tool_registry: &ToolRegistry) -> Value {
    let params = match params {
        Some(p) => p,
        None => return error_response(id.clone(), -32602, "Invalid params: missing params"),
    };

    let tool_name = match params["name"].as_str() {
        Some(n) => n,
        None => return error_response(id.clone(), -32602, "Invalid params: missing 'name'"),
    };

    let arguments: HashMap<String, String> = params["arguments"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| {
                    v.as_str().map(|s| (k.clone(), s.to_string()))
                        .or_else(|| Some((k.clone(), v.to_string())))
                })
                .collect()
        })
        .unwrap_or_default();

    let tool = match tool_registry.get(tool_name) {
        Some(t) => t,
        None => return error_response(id.clone(), -32602, &format!("Unknown tool: {tool_name}")),
    };

    let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tool.execute(&arguments)
    })) {
        Ok(r) => r,
        Err(_) => return error_response(id.clone(), -32603, &format!("Internal error executing '{tool_name}'")),
    };

    result_response(id.clone(), json!({
        "content": [{ "type": "text", "text": result.output }],
        "isError": !result.success
    }))
}

fn handle_resources_list(id: &Value, scan_operator: &ScanOperator) -> Value {
    let listing = scan_operator.list_files(None, None);
    let resources: Vec<Value> = listing
        .lines()
        .filter(|l| !l.is_empty())
        .take(100)
        .map(|path| {
            json!({
                "uri": format!("file:///{path}"),
                "name": path,
                "mimeType": guess_mime_type(path)
            })
        })
        .collect();

    result_response(id.clone(), json!({ "resources": resources }))
}

fn handle_resources_read(id: &Value, params: Option<&Value>, file_operator: &FileOperator) -> Value {
    let params = match params {
        Some(p) => p,
        None => return error_response(id.clone(), -32602, "Invalid params: missing params"),
    };

    let uri = match params["uri"].as_str() {
        Some(u) => u,
        None => return error_response(id.clone(), -32602, "Invalid params: missing 'uri'"),
    };

    let relative_path = uri.trim_start_matches("file:///");
    let content = file_operator.read_file(relative_path);

    if content.starts_with("Error:") {
        return error_response(id.clone(), -32602, &content);
    }

    result_response(id.clone(), json!({
        "contents": [{
            "uri": uri,
            "mimeType": guess_mime_type(relative_path),
            "text": content
        }]
    }))
}

fn guess_mime_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "kt" | "kts" => "text/x-kotlin",
        "java" => "text/x-java",
        "py" => "text/x-python",
        "js" | "mjs" | "cjs" => "text/javascript",
        "ts" | "tsx" => "text/typescript",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "md" => "text/markdown",
        "yaml" | "yml" => "text/yaml",
        "sh" | "bash" | "zsh" => "text/x-shellscript",
        "rs" => "text/x-rust",
        "go" => "text/x-go",
        "c" | "h" => "text/x-c",
        "cpp" | "cc" | "hpp" => "text/x-c++",
        "cs" => "text/x-csharp",
        "rb" => "text/x-ruby",
        "swift" => "text/x-swift",
        "toml" => "text/x-toml",
        "gradle" => "text/x-groovy",
        _ => "text/plain",
    }
}

fn result_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}
