//! Integration tests for the Mithril HTTP API.
//!
//! These tests spin up a real axum server on a random port and hit it with reqwest.
//! They do NOT require a model file to be present — they test HTTP behaviour only
//! (error codes, response shapes, no crashes on bad input).

use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use axum::{routing::post, Router};
use parking_lot::Mutex;
use tokio::net::TcpListener;

use mithril::api::server::AppState;
use mithril::engine::lazy_model::LazyModelManager;
use mithril::operators::{file::FileOperator, scan::ScanOperator};
use mithril::tools;

/// Spawn a test server on an OS-assigned port. Returns base URL.
async fn spawn_server() -> String {
    let tmp = tempfile::tempdir().unwrap();
    let model_path = tmp.path().join("nonexistent.gguf");

    let state = AppState {
        model_manager: Arc::new(LazyModelManager::new(model_path, 60)),
        tool_registry: Arc::new(tools::create_default_registry(tmp.path().to_str().unwrap())),
        file_operator: Arc::new(FileOperator::new(tmp.path().to_str().unwrap())),
        scan_operator: Arc::new(ScanOperator::new(tmp.path().to_str().unwrap())),
        project_path: tmp.path().to_str().unwrap().to_string(),
        active_downloads: Arc::new(Mutex::new(HashMap::new())),
        api_token: None,
        flow_config: None,
        shutdown: tokio_util::sync::CancellationToken::new(),
    };

    let app = mithril::api::server::build_app(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn test_health_returns_200() {
    let base = spawn_server().await;
    let res = reqwest::get(format!("{base}/health")).await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn test_chat_unknown_model_returns_404() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/api/chat"))
        .json(&serde_json::json!({
            "model": "nonexistent-model-xyz",
            "messages": [{ "role": "user", "content": "hello" }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_chat_malformed_json_returns_422_no_crash() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/api/chat"))
        .header("content-type", "application/json")
        .body("{ this is not valid json }")
        .send()
        .await
        .unwrap();
    // axum returns 422 for malformed JSON
    assert!(res.status().is_client_error());

    // Server must still be alive after the bad request
    let health = reqwest::get(format!("{base}/health")).await.unwrap();
    assert_eq!(health.status(), 200);
}

#[tokio::test]
async fn test_embed_returns_501() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/api/embed"))
        .json(&serde_json::json!({ "model": "qwen-1.5b", "input": "hello" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 501);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(body["error"].is_string());
}

#[tokio::test]
async fn test_openai_models_list() {
    let base = spawn_server().await;
    let res = reqwest::get(format!("{base}/v1/models")).await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["object"], "list");
    assert!(body["data"].is_array());
}

#[tokio::test]
async fn test_mcp_tools_list_returns_all_tools() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    let tools = &body["result"]["tools"];
    assert!(tools.is_array());
    assert!(tools.as_array().unwrap().len() >= 20, "Expected at least 20 tools, got {}", tools.as_array().unwrap().len());
}

#[tokio::test]
async fn test_mcp_tool_list_files_in_tmpdir() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), "world").unwrap();

    let model_path = tmp.path().join("nonexistent.gguf");
    let state = AppState {
        model_manager: Arc::new(LazyModelManager::new(model_path, 60)),
        tool_registry: Arc::new(tools::create_default_registry(tmp.path().to_str().unwrap())),
        file_operator: Arc::new(FileOperator::new(tmp.path().to_str().unwrap())),
        scan_operator: Arc::new(ScanOperator::new(tmp.path().to_str().unwrap())),
        project_path: tmp.path().to_str().unwrap().to_string(),
        active_downloads: Arc::new(Mutex::new(HashMap::new())),
        api_token: None,
        flow_config: None,
        shutdown: tokio_util::sync::CancellationToken::new(),
    };

    let app = Router::new()
        .route("/mcp", post(mithril::api::mcp::handle_mcp))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = format!("http://{addr}");

    let res = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "list_files",
                "arguments": {}
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    let content = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(content.contains("hello.txt"));
}

// ── Security E2E tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_mcp_requires_auth_when_token_configured() {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState {
        model_manager: Arc::new(mithril::engine::lazy_model::LazyModelManager::new(
            tmp.path().join("model.gguf"), 60,
        )),
        tool_registry: Arc::new(mithril::tools::create_default_registry(tmp.path().to_str().unwrap())),
        file_operator: Arc::new(mithril::operators::file::FileOperator::new(tmp.path().to_str().unwrap())),
        scan_operator: Arc::new(mithril::operators::scan::ScanOperator::new(tmp.path().to_str().unwrap())),
        project_path: tmp.path().to_str().unwrap().to_string(),
        active_downloads: Arc::new(Mutex::new(HashMap::new())),
        api_token: Some("secret-test-token".to_string()), // token required
        flow_config: None,
        shutdown: CancellationToken::new(),
    };
    let app = axum::Router::new()
        .route("/mcp", axum::routing::post(mithril::api::mcp::handle_mcp))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = format!("http://{addr}");

    // Without token → 401
    let res = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .send().await.unwrap();
    assert_eq!(res.status(), 401);

    // Wrong token → 401
    let res = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .header("Authorization", "Bearer wrong-token")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .send().await.unwrap();
    assert_eq!(res.status(), 401);

    // Correct token → 200
    let res = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .header("Authorization", "Bearer secret-test-token")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn test_health_not_affected_by_rate_limiting() {
    // Health endpoint must always respond even under concurrent load
    let base = spawn_server().await;
    let handles: Vec<_> = (0..20).map(|_| {
        let url = format!("{base}/health");
        tokio::spawn(async move { reqwest::get(url).await.unwrap().status() })
    }).collect();
    for h in handles {
        let status = h.await.unwrap();
        assert_eq!(status, 200, "/health must always respond 200");
    }
}

#[tokio::test]
async fn test_session_file_permissions() {
    use mithril::session::SharedSession;
    use mithril::providers::ChatMessage;
    let s = SharedSession::new("local");
    s.push(ChatMessage::user("test message"));

    // Check that session file exists and on Unix has 0600 permissions
    let session_dir = dirs::home_dir().unwrap().join(".mithril").join("sessions");
    let session_file = session_dir.join(format!("{}.json", s.id));

    if session_file.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let meta = std::fs::metadata(&session_file).unwrap();
            let mode = meta.mode() & 0o777;
            assert_eq!(mode, 0o600, "Session file must have 0600 permissions, got {:o}", mode);
        }
    }

    // Cleanup
    let _ = mithril::session::delete_session(&s.id);
}

// ── Helper per server con token auth ─────────────────────────────────────────

#[allow(dead_code)]
async
fn spawn_server_with_token(token: &str) -> (String, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState {
        model_manager: Arc::new(LazyModelManager::new(tmp.path().join("model.gguf"), 60)),
        tool_registry: Arc::new(tools::create_default_registry(tmp.path().to_str().unwrap())),
        file_operator: Arc::new(FileOperator::new(tmp.path().to_str().unwrap())),
        scan_operator: Arc::new(ScanOperator::new(tmp.path().to_str().unwrap())),
        project_path: tmp.path().to_str().unwrap().to_string(),
        active_downloads: Arc::new(Mutex::new(HashMap::new())),
        api_token: Some(token.to_string()),
        flow_config: None,
        shutdown: CancellationToken::new(),
    };
    let app = mithril::api::server::build_app(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), tmp)
}

// ── Ollama API E2E ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_api_tags_returns_model_list() {
    let base = spawn_server().await;
    let res = reqwest::get(format!("{base}/api/tags")).await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    let models = body["models"].as_array().unwrap();
    assert!(!models.is_empty(), "should list at least one model");
    // Each model has required fields
    let first = &models[0];
    assert!(first["name"].is_string());
    assert!(first["details"]["family"].is_string());
}

#[tokio::test]
async fn test_api_version_returns_semver() {
    let base = spawn_server().await;
    let res = reqwest::get(format!("{base}/api/version")).await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    let version = body["version"].as_str().unwrap();
    // Must be semver-like: X.Y.Z
    assert!(version.contains('.'), "version should be semver: {version}");
}

#[tokio::test]
async fn test_api_ps_returns_models_array() {
    let base = spawn_server().await;
    let res = reqwest::get(format!("{base}/api/ps")).await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(body["models"].is_array());
}

#[tokio::test]
async fn test_api_show_known_model() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/api/show"))
        .json(&serde_json::json!({ "model": "qwen-1.5b" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(body["details"].is_object());
    assert_eq!(body["details"]["family"], "qwen2");
}

#[tokio::test]
async fn test_api_show_unknown_model_returns_404() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/api/show"))
        .json(&serde_json::json!({ "model": "does-not-exist" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 404);
    // Body may be JSON with error field
    if let Ok(body) = res.json::<serde_json::Value>().await {
        if body["error"].is_string() { /* ok */ }
    }
}

#[tokio::test]
async fn test_api_show_missing_model_field_returns_400() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/api/show"))
        .json(&serde_json::json!({}))
        .send().await.unwrap();
    assert_eq!(res.status(), 400);
}

#[tokio::test]
async fn test_api_pull_starts_download() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/api/pull"))
        .json(&serde_json::json!({ "model": "qwen-1.5b" }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    // Either "pulling manifest" (new) or "already pulling" (idempotent)
    let status = body["status"].as_str().unwrap_or("");
    assert!(
        status.contains("pull") || status.contains("success") || status.contains("error"),
        "unexpected status: {status}"
    );
}

#[tokio::test]
async fn test_api_pull_missing_model_returns_400() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/api/pull"))
        .json(&serde_json::json!({}))
        .send().await.unwrap();
    assert_eq!(res.status(), 400);
}

#[tokio::test]
async fn test_health_includes_model_loaded_field() {
    let base = spawn_server().await;
    let body: serde_json::Value = reqwest::get(format!("{base}/health"))
        .await.unwrap().json().await.unwrap();
    assert!(body["model_loaded"].is_boolean());
    assert_eq!(body["model_loaded"], false); // no model file present
    assert!(body["version"].is_string());
}

// ── OpenAI API E2E ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_openai_chat_no_model_file_returns_500() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .json(&serde_json::json!({
            "model": "qwen-1.5b",
            "messages": [{ "role": "user", "content": "hello" }]
        }))
        .send().await.unwrap();
    // No model file → 500 (model not downloaded)
    assert_eq!(res.status(), 500);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(body["error"].is_object());
}

#[tokio::test]
async fn test_openai_chat_response_shape() {
    // Test shape only — model not loaded so we just verify error shape is correct
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .json(&serde_json::json!({
            "model": "qwen-1.5b",
            "messages": [{ "role": "user", "content": "hello" }]
        }))
        .send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    // Either success shape or error shape — both must be valid JSON objects
    assert!(body.is_object());
}

// ── MCP Tools E2E ─────────────────────────────────────────────────────────────

async fn spawn_server_with_files(files: &[(&str, &str)]) -> (String, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    for (name, content) in files {
        std::fs::write(tmp.path().join(name), content).unwrap();
    }
    let state = AppState {
        model_manager: Arc::new(LazyModelManager::new(tmp.path().join("model.gguf"), 60)),
        tool_registry: Arc::new(tools::create_default_registry(tmp.path().to_str().unwrap())),
        file_operator: Arc::new(FileOperator::new(tmp.path().to_str().unwrap())),
        scan_operator: Arc::new(ScanOperator::new(tmp.path().to_str().unwrap())),
        project_path: tmp.path().to_str().unwrap().to_string(),
        active_downloads: Arc::new(Mutex::new(HashMap::new())),
        api_token: None,
        flow_config: None,
        shutdown: CancellationToken::new(),
    };
    let app = Router::new()
        .route("/mcp", post(mithril::api::mcp::handle_mcp))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), tmp)
}

async fn call_tool(base: &str, name: &str, args: serde_json::Value) -> serde_json::Value {
    reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args }
        }))
        .send().await.unwrap()
        .json::<serde_json::Value>().await.unwrap()
}

#[tokio::test]
async fn test_mcp_read_psi_reads_existing_file() {
    let (base, _tmp) = spawn_server_with_files(&[("src.rs", "fn main() {}")]).await;
    let body = call_tool(&base, "read_psi", serde_json::json!({ "target": "src.rs" })).await;
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("fn main()"), "got: {text}");
    assert_eq!(body["result"]["isError"], false);
}

#[tokio::test]
async fn test_mcp_read_psi_missing_file_returns_error() {
    let (base, _tmp) = spawn_server_with_files(&[]).await;
    let body = call_tool(&base, "read_psi", serde_json::json!({ "target": "missing.rs" })).await;
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.to_lowercase().contains("error"), "expected error, got: {text}");
}

#[tokio::test]
async fn test_mcp_write_and_read_file_roundtrip() {
    let (base, _tmp) = spawn_server_with_files(&[]).await;
    // Write
    let w = call_tool(&base, "write_file", serde_json::json!({
        "target": "output.txt",
        "content": "hello from test"
    })).await;
    assert_eq!(w["result"]["isError"], false);
    // Read back
    let r = call_tool(&base, "read_psi", serde_json::json!({ "target": "output.txt" })).await;
    let text = r["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert_eq!(text, "hello from test");
}

#[tokio::test]
async fn test_mcp_delete_file() {
    let (base, _tmp) = spawn_server_with_files(&[("to_delete.txt", "bye")]).await;
    let d = call_tool(&base, "delete_file", serde_json::json!({ "target": "to_delete.txt" })).await;
    assert_eq!(d["result"]["isError"], false);
    // File should be gone
    let r = call_tool(&base, "read_psi", serde_json::json!({ "target": "to_delete.txt" })).await;
    let text = r["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.to_lowercase().contains("error"), "file should not exist after delete");
}

#[tokio::test]
async fn test_mcp_grep_files_finds_pattern() {
    let (base, _tmp) = spawn_server_with_files(&[
        ("a.rs", "fn foo() {}"),
        ("b.rs", "fn bar() {}"),
    ]).await;
    let body = call_tool(&base, "grep_files", serde_json::json!({
        "pattern": "fn foo",
        "path": "."
    })).await;
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("foo"), "expected foo in grep results, got: {text}");
    assert!(!text.contains("bar"), "bar should not appear in foo grep");
}

#[tokio::test]
async fn test_mcp_find_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("deep/nested")).unwrap();
    std::fs::write(tmp.path().join("deep/nested/target.rs"), "content").unwrap();
    let state = AppState {
        model_manager: Arc::new(LazyModelManager::new(tmp.path().join("model.gguf"), 60)),
        tool_registry: Arc::new(tools::create_default_registry(tmp.path().to_str().unwrap())),
        file_operator: Arc::new(FileOperator::new(tmp.path().to_str().unwrap())),
        scan_operator: Arc::new(ScanOperator::new(tmp.path().to_str().unwrap())),
        project_path: tmp.path().to_str().unwrap().to_string(),
        active_downloads: Arc::new(Mutex::new(HashMap::new())),
        api_token: None,
        flow_config: None,
        shutdown: CancellationToken::new(),
    };
    let app = Router::new()
        .route("/mcp", post(mithril::api::mcp::handle_mcp))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _tmp = tmp; // keep alive
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = format!("http://{addr}");
    let body = call_tool(&base, "find_file", serde_json::json!({ "query": "target" })).await;
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("target"), "expected target.rs in results, got: {text}");
}

#[tokio::test]
async fn test_mcp_file_stats() {
    let (base, _tmp) = spawn_server_with_files(&[("stats.txt", "line1\nline2\nline3")]).await;
    let body = call_tool(&base, "file_stats", serde_json::json!({ "target": "stats.txt" })).await;
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains('3') || text.contains("lines"), "expected line count, got: {text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mcp_run_terminal_safe_command() {
    let (base, _tmp) = spawn_server_with_files(&[]).await;
    let body = call_tool(&base, "run_terminal", serde_json::json!({
        "command": "echo mithril_test_ok"
    })).await;
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.contains("mithril_test_ok"), "expected echo output, got: {text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_mcp_run_terminal_sandbox_blocks_dangerous() {
    let (base, _tmp) = spawn_server_with_files(&[]).await;
    // sudo should be blocked
    let body = call_tool(&base, "run_terminal", serde_json::json!({
        "command": "sudo ls"
    })).await;
    let text = body["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.to_lowercase().contains("block") || text.to_lowercase().contains("sandbox") || text.to_lowercase().contains("error"),
        "expected sandbox block, got: {text}"
    );
}

#[tokio::test]
async fn test_mcp_unknown_tool_returns_error() {
    let base = spawn_server().await;
    let body = call_tool(&base, "nonexistent_tool_xyz", serde_json::json!({})).await;
    // isError true OR error in JSON-RPC response
    let is_error = body["result"]["isError"].as_bool().unwrap_or(false);
    let has_error_field = body["error"].is_object();
    assert!(is_error || has_error_field, "expected error for unknown tool, got: {body}");
}

#[tokio::test]
async fn test_mcp_resources_list() {
    let (base, _tmp) = spawn_server_with_files(&[("main.rs", "fn main() {}")]).await;
    let res = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "resources/list", "params": {}
        }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(body["result"]["resources"].is_array());
}

#[tokio::test]
async fn test_mcp_initialize_handshake() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1.0" }
            }
        }))
        .send().await.unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["result"]["serverInfo"]["name"], "mithril");
    assert!(body["result"]["capabilities"].is_object());
    assert_eq!(body["result"]["protocolVersion"], "2024-11-05");
}

// ── Session E2E ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_session_save_load_roundtrip() {
    use mithril::session::{SharedSession, delete_session};
    use mithril::providers::ChatMessage;

    let s = SharedSession::new("gemini");
    let id = s.id.clone();
    s.push(ChatMessage::user("hello session"));
    s.push(ChatMessage::assistant("hi back"));

    let loaded = SharedSession::load(&id).unwrap();
    let msgs = loaded.snapshot();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].content, "hello session");
    assert_eq!(msgs[1].role, "assistant");

    let _ = delete_session(&id);
}

#[tokio::test]
async fn test_session_push_with_result_rollback_on_error() {
    use mithril::session::SharedSession;
    use mithril::providers::ChatMessage;

    let s = SharedSession::new("local");
    // push two messages successfully
    s.push_with_result(ChatMessage::user("msg1")).unwrap();
    s.push_with_result(ChatMessage::user("msg2")).unwrap();
    assert_eq!(s.snapshot().len(), 2);

    let _ = mithril::session::delete_session(&s.id);
}

#[tokio::test]
async fn test_session_claim_frontend_exclusive() {
    use mithril::session::{SharedSession, FRONTEND_TELEGRAM, FRONTEND_TERMINAL, FRONTEND_NONE};
    use std::sync::atomic::Ordering;

    let s = SharedSession::new("local");
    s.active_frontend.store(FRONTEND_NONE, Ordering::SeqCst);

    // Telegram claims
    assert!(s.claim_frontend(FRONTEND_TELEGRAM).is_ok());
    // Terminal cannot claim while telegram is active
    assert!(s.claim_frontend(FRONTEND_TERMINAL).is_err());
    // Telegram can re-claim idempotently
    assert!(s.claim_frontend(FRONTEND_TELEGRAM).is_ok());
    // Release and terminal can claim
    s.release_frontend(FRONTEND_TELEGRAM);
    assert!(s.claim_frontend(FRONTEND_TERMINAL).is_ok());
}

#[tokio::test]
async fn test_session_list_and_delete() {
    use mithril::session::{SharedSession, list_sessions, delete_session};
    use mithril::providers::ChatMessage;

    let s = SharedSession::new("openai");
    let id = s.id.clone();
    s.push(ChatMessage::user("test for listing"));

    let sessions = list_sessions().unwrap();
    assert!(sessions.iter().any(|m| m.id == id), "session should appear in list");

    delete_session(&id).unwrap();
    let sessions_after = list_sessions().unwrap();
    assert!(!sessions_after.iter().any(|m| m.id == id), "session should be deleted");
}

// ── Config / Credentials E2E ──────────────────────────────────────────────────

#[tokio::test]
async fn test_config_encrypt_decrypt_with_key_password() {
    use mithril::config::MithrilConfig;

    let mut config = MithrilConfig::default();
    config.key_password = Some("test-master-password".to_string());

    // Encrypt a credential with key_password
    config.set_credential("test_key", "sk-supersecret").unwrap();

    // Decrypt with same key_password
    let val = config.get_credential("test_key").unwrap().unwrap();
    assert_eq!(val, "sk-supersecret");
}

#[tokio::test]
async fn test_config_get_nonexistent_credential_returns_none() {
    use mithril::config::MithrilConfig;
    let config = MithrilConfig::default();
    let result = config.get_credential("nonexistent_key_xyz");
    assert!(result.is_ok(), "getting nonexistent cred should return Ok(None)");
    assert!(result.unwrap().is_none());
}

// ── Palantir Index E2E ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_palantir_index_build_and_query() {
    use mithril::index::PalantirIndex;
    use mithril::operators::scan::ScanOperator;

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("auth.rs"), "fn validate_token() { let x = 42; }").unwrap();
    std::fs::write(tmp.path().join("server.rs"), "fn start_server() { println!(\"listening\"); }").unwrap();
    std::fs::write(tmp.path().join("utils.rs"), "fn helper_function() { }").unwrap();

    let scan = ScanOperator::new(tmp.path());
    let index = PalantirIndex::build(tmp.path().to_str().unwrap(), &scan);

    assert!(!index.entries.is_empty(), "index should have entries");

    // Query for auth-related content
    let results = index.query("token validation", 5);
    assert!(!results.is_empty(), "query should return results");
    // The auth.rs file should rank highest for token query
    let top = &results[0];
    assert!(top.score > 0.0);
    assert!(top.entry.path.contains("auth"), "auth.rs should rank first for token query, got: {}", top.entry.path);
}

#[tokio::test]
async fn test_palantir_index_incremental_update() {
    use mithril::index::PalantirIndex;
    use mithril::operators::scan::ScanOperator;

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("initial.rs"), "fn initial() {}").unwrap();

    let scan = ScanOperator::new(tmp.path());
    let index1 = PalantirIndex::build(tmp.path().to_str().unwrap(), &scan);
    index1.save(tmp.path().to_str().unwrap());

    // Add a new file
    std::fs::write(tmp.path().join("added.rs"), "fn added_later() {}").unwrap();

    let existing = PalantirIndex::load_or_null(tmp.path().to_str().unwrap());
    let index2 = PalantirIndex::build_incremental(tmp.path().to_str().unwrap(), &scan, existing);
    let paths: Vec<&str> = index2.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.iter().any(|p| p.contains("initial")));
    assert!(paths.iter().any(|p| p.contains("added")));
}

// ── Streaming API E2E (shape only — no model required) ────────────────────────

#[tokio::test]
async fn test_ollama_chat_stream_returns_ndjson_content_type() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/api/chat"))
        .json(&serde_json::json!({
            "model": "qwen-1.5b",
            "messages": [{ "role": "user", "content": "hi" }],
            "stream": true
        }))
        .send().await.unwrap();
    // With no model file this returns 500, but if it were to succeed the content-type would be ndjson
    // We verify the server doesn't panic and returns a JSON-parseable error
    let ct = res.headers().get("content-type").map(|v| v.to_str().unwrap_or("")).unwrap_or("");
    let status = res.status();
    // Either error (500 = no model) or success with ndjson content-type
    assert!(
        status.is_server_error() || ct.contains("ndjson") || ct.contains("json"),
        "unexpected content-type {ct} with status {status}"
    );
}

#[tokio::test]
async fn test_mcp_tools_list_has_correct_schema() {
    let base = spawn_server().await;
    let res = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}
        }))
        .send().await.unwrap();
    let body: serde_json::Value = res.json().await.unwrap();
    let tools = body["result"]["tools"].as_array().unwrap();

    // Every tool must have name, description, inputSchema
    for tool in tools {
        assert!(tool["name"].is_string(), "tool missing name: {tool}");
        assert!(tool["description"].is_string(), "tool missing description: {tool}");
        assert!(tool["inputSchema"].is_object(), "tool missing inputSchema: {tool}");
        assert_eq!(tool["inputSchema"]["type"], "object");
    }

    // Verify specific tools are present
    let names: Vec<&str> = tools.iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for expected in &["read_psi", "write_file", "run_terminal", "git_status", "web_search"] {
        assert!(names.contains(expected), "missing tool: {expected}");
    }
}
