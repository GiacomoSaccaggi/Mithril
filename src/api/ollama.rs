#![allow(dead_code)]
use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::Response,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::server::AppState;
use crate::engine::{
    chat_template::{format_chat, get_stop_tokens, ChatMessage},
    model_catalog::{find_model, MODELS},
};

#[derive(Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<MessageInput>,
    pub stream: Option<bool>,
    pub options: Option<Options>,
}

#[derive(Deserialize)]
pub struct GenerateRequest {
    pub model: String,
    pub prompt: String,
    pub system: Option<String>,
    pub stream: Option<bool>,
    pub options: Option<Options>,
}

#[derive(Deserialize)]
pub struct MessageInput {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize, Default)]
pub struct Options {
    pub temperature: Option<f32>,
    pub num_predict: Option<u32>,
}

pub async fn list_models(State(_state): State<AppState>) -> Json<Value> {
    use crate::config::MithrilConfig;

    let model_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".mithril/models");

    // Local GGUF models
    let mut models: Vec<Value> = MODELS.iter().map(|m| {
        let file = model_dir.join(m.file_name);
        let size = file.metadata().map(|md| md.len()).unwrap_or(0);
        json!({
            "name": format!("{}:{}", m.family, m.parameter_size.to_lowercase()),
            "model": m.id,
            "size": size,
            "details": {
                "family": m.family,
                "parameter_size": m.parameter_size,
                "quantization_level": m.quantization,
                "format": "gguf"
            }
        })
    }).collect();

    // Cloud models — only add if API key is configured
    if let Ok(config) = MithrilConfig::load() {
        let cloud = [
            ("gemini", &config.providers.gemini.model, "gemini"),
            ("openai", &config.providers.openai.model, "openai"),
            ("anthropic", &config.providers.anthropic.model, "anthropic"),
        ];
        for (key, model_name, family) in cloud {
            if config.get_credential(key).ok().flatten().is_some() {
                models.push(json!({
                    "name": format!("{}:latest", model_name),
                    "model": model_name,
                    "size": 0,
                    "details": {
                        "family": family,
                        "parameter_size": "cloud",
                        "quantization_level": "none",
                        "format": "api"
                    }
                }));
            }
        }
    }

    // Fellowship virtual models — appear as selectable "models" for clients
    if let Ok(fellowships) = crate::flow::fellowship::try_list_fellowships() {
        for (name, config) in fellowships {
            let agent_count = config.agents.len();
            let desc = config.description.as_deref().unwrap_or("fellowship");
            models.push(json!({
                "name": format!("{}:latest", config.name),
                "model": config.name,
                "size": 0,
                "details": {
                    "family": "mithril-fellowship",
                    "parameter_size": format!("{} agents", agent_count),
                    "quantization_level": "orchestrated",
                    "format": "fellowship"
                },
                "description": desc,
            }));
        }
    }

    Json(json!({ "models": models }))
}

pub async fn version() -> Json<Value> {
    Json(json!({ "version": "0.3.0" }))
}

pub async fn running_models(State(state): State<AppState>) -> Json<Value> {
    let running = if state.model_manager.is_loaded() {
        vec![json!({ "name": "qwen2:1.5b", "model": "qwen-1.5b" })]
    } else {
        vec![]
    };
    Json(json!({ "models": running }))
}

/// Route a chat request through the FlowRunner when flow mode is active.
async fn chat_flow(
    state: AppState,
    req: ChatRequest,
    flow_config: crate::flow::FlowConfig,
) -> Result<Response, (StatusCode, Json<Value>)> {
    // Extract the last user message as the task for the flow
    let user_message = req.messages.iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    if user_message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "no user message" }))));
    }

    let stream = req.stream.unwrap_or(false);
    let model_name = req.model.clone();

    let runner = crate::flow::FlowRunner::new(flow_config, &state.project_path);
    let response = runner.run(&user_message).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?;

    if stream {
        let line = format!("{}
", serde_json::to_string(&json!({
            "model": model_name,
            "created_at": Utc::now().to_rfc3339(),
            "message": { "role": "assistant", "content": response },
            "done": true
        })).unwrap_or_default());
        Ok(Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(Body::from(line))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?)
    } else {
        let body = serde_json::to_string(&json!({
            "model": model_name,
            "created_at": Utc::now().to_rfc3339(),
            "message": { "role": "assistant", "content": response },
            "done": true
        })).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?;
        Ok(Response::builder()
            .header("content-type", "application/json")
            .body(Body::from(body))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?)
    }
}

/// Detect if a model name refers to a cloud provider.
/// Supported: "gemini", "gemini-*", "openai", "gpt-*", "anthropic", "claude-*"
fn detect_cloud_provider(model: &str) -> Option<&'static str> {
    let m = model.to_lowercase();
    if m == "gemini" || m.starts_with("gemini-") { return Some("gemini"); }
    if m == "openai" || m.starts_with("gpt-") || m.starts_with("o1") || m.starts_with("o3") { return Some("openai"); }
    if m == "anthropic" || m.starts_with("claude-") { return Some("anthropic"); }
    None
}

pub async fn chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    // If a flow config is loaded, route through FlowRunner
    if let Some(flow_config) = &state.flow_config {
        return chat_flow(state.clone(), req, flow_config.clone()).await;
    }

    // Route to cloud provider if model name matches
    if let Some(provider_name) = detect_cloud_provider(&req.model) {
        return chat_cloud(state, req, provider_name).await;
    }

    let model_info = find_model(&req.model).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("model not found: {}. Use a local model (qwen-1.5b, llama-8b, ...) or a cloud model (gemini, gemini-1.5-flash, gpt-4o, claude-3-5-sonnet).", req.model) })),
        )
    })?;

    let messages: Vec<ChatMessage> = req.messages.iter()
        .map(|m| ChatMessage::new(&m.role, &m.content))
        .collect();

    let formatted = format_chat(model_info.chat_template, &messages);
    let stops = get_stop_tokens(model_info.chat_template);
    let temperature = req.options.as_ref().and_then(|o| o.temperature).unwrap_or(0.7);
    let max_tokens = req.options.as_ref().and_then(|o| o.num_predict).unwrap_or(2048);
    let stream = req.stream.unwrap_or(false);
    let model_name = req.model.clone();

    if stream {
        // std::sync::mpsc for the !Send inference thread, bridged to tokio::mpsc for the async stream
        let (std_tx, std_rx) = std::sync::mpsc::sync_channel::<Option<String>>(64);
        let (tok_tx, mut tok_rx) = tokio::sync::mpsc::channel::<Option<String>>(64);

        // Start inference (sends tokens into std_tx on a detached thread)
        state.model_manager.infer_streaming(&formatted, &stops, temperature, max_tokens, std_tx);

        // Bridge: std::mpsc -> tokio::mpsc (runs on the blocking thread pool)
        // M3: break on send error (receiver dropped = client disconnected)
        tokio::task::spawn_blocking(move || {
            while let Ok(msg) = std_rx.recv() {
                let done = msg.is_none();
                if tok_tx.blocking_send(msg).is_err() { break; }
                if done { break; }
            }
        });

        let body = Body::from_stream(async_stream::stream! {
            loop {
                match tok_rx.recv().await {
                    Some(Some(piece)) => {
                        let chunk = json!({
                            "model": model_name,
                            "created_at": Utc::now().to_rfc3339(),
                            "message": { "role": "assistant", "content": piece },
                            "done": false
                        });
                        yield Ok::<axum::body::Bytes, std::convert::Infallible>(
                            format!("{}\n", serde_json::to_string(&chunk).unwrap_or_default()).into()
                        );
                    }
                    Some(None) | None => {
                        let done_chunk = json!({
                            "model": model_name,
                            "created_at": Utc::now().to_rfc3339(),
                            "message": { "role": "assistant", "content": "" },
                            "done": true
                        });
                        yield Ok::<axum::body::Bytes, std::convert::Infallible>(
                            format!("{}\n", serde_json::to_string(&done_chunk).unwrap_or_default()).into()
                        );
                        break;
                    }
                }
            }
        });

        Ok(Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(body)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?)
    } else {
        let response = state.model_manager
            .infer(&formatted, &stops, temperature, max_tokens)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?;

        let body = serde_json::to_string(&json!({
            "model": req.model,
            "created_at": Utc::now().to_rfc3339(),
            "message": { "role": "assistant", "content": response },
            "done": true
        }))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?;

        Ok(Response::builder()
            .header("content-type", "application/json")
            .body(Body::from(body))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?)
    }
}

pub async fn generate(
    State(state): State<AppState>,
    Json(req): Json<GenerateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let model_info = find_model(&req.model).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("model not found: {}", req.model) })),
        )
    })?;

    let mut messages = vec![];
    if let Some(sys) = &req.system {
        messages.push(ChatMessage::new("system", sys));
    }
    messages.push(ChatMessage::new("user", &req.prompt));

    let formatted = format_chat(model_info.chat_template, &messages);
    let stops = get_stop_tokens(model_info.chat_template);
    let temperature = req.options.as_ref().and_then(|o| o.temperature).unwrap_or(0.7);
    let max_tokens = req.options.as_ref().and_then(|o| o.num_predict).unwrap_or(2048);

    let response = state.model_manager
        .infer(&formatted, &stops, temperature, max_tokens)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?;

    Ok(Json(json!({
        "model": req.model,
        "created_at": Utc::now().to_rfc3339(),
        "response": response,
        "done": true,
        "done_reason": "stop"
    })))
}

pub async fn show_model(Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let model_name = match body["model"].as_str() {
        Some(m) => m,
        None => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing 'model' field" }))),
    };
    if let Some(m) = find_model(model_name) {
        (StatusCode::OK, Json(json!({
            "modelfile": "",
            "parameters": "",
            "template": "",
            "details": {
                "family": m.family,
                "parameter_size": m.parameter_size,
                "quantization_level": m.quantization,
                "format": "gguf"
            }
        })))
    } else {
        (StatusCode::NOT_FOUND, Json(json!({ "error": format!("model not found: {model_name}") })))
    }
}

/// Handle chat requests routed to a cloud provider (Gemini, OpenAI, Anthropic).
async fn chat_cloud(
    _state: AppState,
    req: ChatRequest,
    provider_name: &'static str,
) -> Result<Response, (StatusCode, Json<Value>)> {
    use crate::config::MithrilConfig;
    use crate::providers::{self, ChatMessage as ProviderMessage};

    let config = MithrilConfig::load().map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": format!("config load failed: {e}") })),
    ))?;

    let provider = providers::create_provider(provider_name, &config).map_err(|e| (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": format!("provider not available: {}", e) })),
    ))?;

    let messages: Vec<ProviderMessage> = req.messages.iter().map(|m| ProviderMessage {
        role: m.role.clone(),
        content: m.content.clone(),
    }).collect();

    let model_name = req.model.clone();
    let use_stream = req.stream.unwrap_or(false);

    let response = provider.chat(&messages).await.map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    ))?;

    if use_stream {
        let line = format!("{}
", serde_json::to_string(&json!({
            "model": model_name,
            "created_at": Utc::now().to_rfc3339(),
            "message": { "role": "assistant", "content": response },
            "done": true
        })).unwrap_or_default());
        Ok(Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(Body::from(line))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?)
    } else {
        let body = serde_json::to_string(&json!({
            "model": model_name,
            "created_at": Utc::now().to_rfc3339(),
            "message": { "role": "assistant", "content": response },
            "done": true
        })).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?;
        Ok(Response::builder()
            .header("content-type", "application/json")
            .body(Body::from(body))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?)
    }
}

pub async fn pull_model(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let model = match body["model"].as_str() {
        Some(m) => m.to_string(),
        None => return (StatusCode::BAD_REQUEST, Json(json!({ "error": "missing 'model' field" }))),
    };

    // Check if already downloading
    {
        let downloads = state.active_downloads.lock();
        if downloads.contains_key(&model) {
            return (StatusCode::OK, Json(json!({ "status": "already pulling", "model": model })));
        }
    }

    // Register as in-progress
    state.active_downloads.lock().insert(model.clone(), "pulling".to_string());

    let model_clone = model.clone();
    let downloads = state.active_downloads.clone();
    tokio::spawn(async move {
        // Use headless variant — run() uses indicatif/stdout, unsafe in background tasks
        let result = crate::cli::download::run_headless(&model_clone).await;
        let status = if result.is_ok() { "success" } else { "error" };
        downloads.lock().insert(model_clone, status.to_string());
    });

    (StatusCode::OK, Json(json!({ "status": "pulling manifest", "model": model })))
}

pub async fn embed(Json(_body): Json<Value>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "embeddings are not supported in this build of Mithril"
        })),
    )
}
