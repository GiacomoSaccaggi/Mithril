#![allow(dead_code)]
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    extract::State,
    http::StatusCode,
    response::Response,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::server::AppState;
use crate::config::MithrilConfig;
use crate::engine::{
    chat_template::{format_chat, get_stop_tokens, ChatMessage},
    lazy_model::LazyModelManager,
    model_catalog::{find_model, MODELS},
};
use crate::providers::resolver::{
    default_model_dir, detect_cloud_provider, is_cloud_provider_ready, list_ready_models,
    resolve_chat_target, resolve_embed_provider, resolve_rerank_provider, ResolveError,
    ResolvedChatTarget,
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
pub struct EmbedRequest {
    pub model: String,
    pub input: serde_json::Value, // string or array of strings
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


/// Handle chat requests routed through a fellowship (multi-agent orchestration) with streaming.
async fn chat_fellowship(
    _state: AppState,
    req: ChatRequest,
    fellowship_config: crate::flow::fellowship::FellowshipConfig,
) -> Result<Response, (StatusCode, Json<Value>)> {
    use crate::config::MithrilConfig;
    use crate::flow::orchestrator::Orchestrator;
    use crate::flow::TraceMode;
    use crate::providers::ChatMessage as ProviderMessage;

    let config = MithrilConfig::load().map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": format!("config load failed: {e}") })),
    ))?;

    let mut orchestrator = Orchestrator::new(fellowship_config, config, TraceMode::Silent);

    let messages: Vec<ProviderMessage> = req.messages.iter().map(|m| ProviderMessage {
        role: m.role.clone(),
        content: m.content.clone(),
    }).collect();

    // Feed conversation history to orchestrator (last user message is the request)
    let user_message = messages.last()
        .map(|m| m.content.clone())
        .unwrap_or_default();

    let model_name = req.model.clone();
    let use_stream = req.stream.unwrap_or(false);

    // Run orchestrator
    let result = orchestrator.handle_request(&user_message).await.map_err(|e| (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    ))?;

    let response_text = result.response;

    if use_stream {
        // Stream the response token by token (simulate streaming by splitting into words)
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
        let text = response_text.clone();
        tokio::spawn(async move {
            // Split into small chunks to simulate streaming
            for word in text.split_inclusive(|c: char| c.is_whitespace()) {
                if tx.send(word.to_string()).await.is_err() { break; }
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            }
        });

        let body = Body::from_stream(async_stream::stream! {
            loop {
                match rx.recv().await {
                    Some(piece) => {
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
                    None => {
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
        let body = serde_json::to_string(&json!({
            "model": model_name,
            "created_at": Utc::now().to_rfc3339(),
            "message": { "role": "assistant", "content": response_text },
            "done": true
        })).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?;
        Ok(Response::builder()
            .header("content-type", "application/json")
            .body(Body::from(body))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?)
    }
}

fn resolve_model_dir(state: &AppState) -> PathBuf {
    state
        .model_manager
        .model_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(default_model_dir)
}

pub async fn list_models(State(state): State<AppState>) -> Json<Value> {
    let model_dir = resolve_model_dir(&state);
    let config = MithrilConfig::load().unwrap_or_default();
    let models = list_ready_models(&config, &model_dir);
    Json(json!({ "models": models }))
}

pub async fn version() -> Json<Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION") }))
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

pub async fn chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    // If a flow config is loaded, route through FlowRunner
    if let Some(flow_config) = &state.flow_config {
        return chat_flow(state.clone(), req, flow_config.clone()).await;
    }

    let config = MithrilConfig::load().unwrap_or_default();
    let model_dir = resolve_model_dir(&state);

    let target = resolve_chat_target(&req.model, &config, &model_dir).map_err(|err| match err {
        ResolveError::UnknownModel(m) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": format!("model not found: {}. Use a local model, cloud model, or fellowship name.", m)
            })),
        ),
        ResolveError::NoProviderAvailable(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": msg })),
        ),
    })?;

    match target {
        ResolvedChatTarget::Fellowship(fellowship_config) => {
            chat_fellowship(state, req, fellowship_config).await
        }
        ResolvedChatTarget::Cloud {
            provider_name,
            model_override,
        } => chat_cloud(state, req, provider_name, model_override).await,
        ResolvedChatTarget::Local(model_info) => {
            let messages: Vec<ChatMessage> = req
                .messages
                .iter()
                .map(|m| ChatMessage::new(&m.role, &m.content))
                .collect();

            let formatted = format_chat(model_info.chat_template, &messages);
            let stops = get_stop_tokens(model_info.chat_template);
            let temperature = req
                .options
                .as_ref()
                .and_then(|o| o.temperature)
                .unwrap_or(0.7);
            let max_tokens = req
                .options
                .as_ref()
                .and_then(|o| o.num_predict)
                .unwrap_or(2048);
            let stream = req.stream.unwrap_or(false);
            let model_name = req.model.clone();

            let manager: Arc<LazyModelManager> = if model_info.id == MODELS[0].id
                && state
                    .model_manager
                    .model_path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    == Some(model_info.file_name)
            {
                Arc::clone(&state.model_manager)
            } else {
                Arc::new(LazyModelManager::new(
                    model_dir.join(model_info.file_name),
                    60,
                ))
            };

            if stream {
                // std::sync::mpsc for the !Send inference thread, bridged to tokio::mpsc for the async stream
                let (std_tx, std_rx) = std::sync::mpsc::sync_channel::<Option<String>>(64);
                let (tok_tx, mut tok_rx) = tokio::sync::mpsc::channel::<Option<String>>(64);

                // Start inference (sends tokens into std_tx on a detached thread)
                manager.infer_streaming(&formatted, &stops, temperature, max_tokens, std_tx);

                // Bridge: std::mpsc -> tokio::mpsc (runs on the blocking thread pool)
                // M3: break on send error (receiver dropped = client disconnected)
                tokio::task::spawn_blocking(move || {
                    while let Ok(msg) = std_rx.recv() {
                        let done = msg.is_none();
                        if tok_tx.blocking_send(msg).is_err() {
                            break;
                        }
                        if done {
                            break;
                        }
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
                    .map_err(|e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": e.to_string() })),
                        )
                    })?)
            } else {
                let response = manager
                    .infer(&formatted, &stops, temperature, max_tokens)
                    .map_err(|e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": e.to_string() })),
                        )
                    })?;

                let body = serde_json::to_string(&json!({
                    "model": req.model,
                    "created_at": Utc::now().to_rfc3339(),
                    "message": { "role": "assistant", "content": response },
                    "done": true
                }))
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": e.to_string() })),
                    )
                })?;

                Ok(Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .map_err(|e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": e.to_string() })),
                        )
                    })?)
            }
        }
    }
}

pub async fn generate(
    State(state): State<AppState>,
    Json(req): Json<GenerateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let config = MithrilConfig::load().unwrap_or_default();
    let model_dir = resolve_model_dir(&state);

    let target = resolve_chat_target(&req.model, &config, &model_dir).map_err(|err| match err {
        ResolveError::UnknownModel(m) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("model not found: {}", m) })),
        ),
        ResolveError::NoProviderAvailable(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": msg })),
        ),
    })?;

    match target {
        ResolvedChatTarget::Cloud {
            provider_name,
            model_override,
        } => {
            use crate::providers::{self, ChatMessage as ProviderMessage};
            let provider = providers::create_provider_with_model(
                provider_name,
                model_override.as_deref(),
                &config,
            )
            .map_err(|e| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "error": format!("provider not available: {e}") })),
                )
            })?;
            let mut messages = Vec::new();
            if let Some(sys) = &req.system {
                messages.push(ProviderMessage::system(sys));
            }
            messages.push(ProviderMessage::user(&req.prompt));
            let response = provider.chat(&messages).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
            Ok(Json(json!({
                "model": req.model,
                "created_at": Utc::now().to_rfc3339(),
                "response": response,
                "done": true,
                "done_reason": "stop"
            })))
        }
        ResolvedChatTarget::Fellowship(fellowship_config) => {
            use crate::flow::orchestrator::Orchestrator;
            use crate::flow::TraceMode;
            let mut orchestrator =
                Orchestrator::new(fellowship_config, config, TraceMode::Silent);
            let result = orchestrator.handle_request(&req.prompt).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
            Ok(Json(json!({
                "model": req.model,
                "created_at": Utc::now().to_rfc3339(),
                "response": result.response,
                "done": true,
                "done_reason": "stop"
            })))
        }
        ResolvedChatTarget::Local(model_info) => {
            let mut messages = vec![];
            if let Some(sys) = &req.system {
                messages.push(ChatMessage::new("system", sys));
            }
            messages.push(ChatMessage::new("user", &req.prompt));

            let formatted = format_chat(model_info.chat_template, &messages);
            let stops = get_stop_tokens(model_info.chat_template);
            let temperature = req
                .options
                .as_ref()
                .and_then(|o| o.temperature)
                .unwrap_or(0.7);
            let max_tokens = req
                .options
                .as_ref()
                .and_then(|o| o.num_predict)
                .unwrap_or(2048);

            let manager: Arc<LazyModelManager> = if model_info.id == MODELS[0].id
                && state
                    .model_manager
                    .model_path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    == Some(model_info.file_name)
            {
                Arc::clone(&state.model_manager)
            } else {
                Arc::new(LazyModelManager::new(
                    model_dir.join(model_info.file_name),
                    60,
                ))
            };

            let response = manager
                .infer(&formatted, &stops, temperature, max_tokens)
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": e.to_string() })),
                    )
                })?;

            Ok(Json(json!({
                "model": req.model,
                "created_at": Utc::now().to_rfc3339(),
                "response": response,
                "done": true,
                "done_reason": "stop"
            })))
        }
    }
}

/// Extract the `"model"` or `"name"` string field from a raw request payload.
/// Accepts JSON payloads regardless of `Content-Type` header (`curl -d` compatibility).
fn extract_model_or_name(raw_body: &Bytes) -> Result<String, (StatusCode, Json<Value>)> {
    let body: Value = serde_json::from_slice(raw_body).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("invalid JSON body: {e}") })),
        )
    })?;
    let model = body["model"]
        .as_str()
        .or_else(|| body["name"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "missing 'model' or 'name' field" })),
            )
        })?;
    Ok(model.to_string())
}

pub async fn show_model(raw_body: Bytes) -> (StatusCode, Json<Value>) {
    let model_name = match extract_model_or_name(&raw_body) {
        Ok(m) => m,
        Err(err_resp) => return err_resp,
    };

    if let Some(m) = find_model(&model_name) {
        return (
            StatusCode::OK,
            Json(json!({
                "modelfile": "",
                "parameters": "",
                "template": "",
                "details": {
                    "family": m.family,
                    "parameter_size": m.parameter_size,
                    "quantization_level": m.quantization,
                    "format": "gguf"
                }
            })),
        );
    }

    if let Some(provider_name) = detect_cloud_provider(&model_name) {
        let config = MithrilConfig::load().unwrap_or_default();
        if is_cloud_provider_ready(provider_name, &config) {
            return (
                StatusCode::OK,
                Json(json!({
                    "modelfile": "",
                    "parameters": "",
                    "template": "",
                    "details": {
                        "family": provider_name,
                        "parameter_size": "cloud",
                        "quantization_level": "none",
                        "format": "api"
                    }
                })),
            );
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": format!("model not found: {model_name}") })),
    )
}

/// Handle chat requests routed to a cloud provider (Gemini, OpenAI, Anthropic, Groq).
async fn chat_cloud(
    _state: AppState,
    req: ChatRequest,
    provider_name: &'static str,
    model_override: Option<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    use crate::providers::{self, ChatMessage as ProviderMessage, StreamChunk};

    let config = MithrilConfig::load().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("config load failed: {e}") })),
        )
    })?;

    let provider = providers::create_provider_with_model(
        provider_name,
        model_override.as_deref(),
        &config,
    )
    .map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": format!("provider not available: {}", e) })),
        )
    })?;

    let messages: Vec<ProviderMessage> = req
        .messages
        .iter()
        .map(|m| ProviderMessage {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();

    let model_name = req.model.clone();
    let use_stream = req.stream.unwrap_or(false);

    if use_stream {
        let (tx, mut rx) =
            tokio::sync::mpsc::unbounded_channel::<std::result::Result<StreamChunk, String>>();
        tokio::spawn(async move {
            let tx_chunk = tx.clone();
            let res = provider
                .chat_stream(
                    &messages,
                    Box::new(move |chunk| {
                        if !chunk.content.is_empty() {
                            let _ = tx_chunk.send(Ok(chunk));
                        }
                    }),
                )
                .await;
            if let Err(e) = res {
                let _ = tx.send(Err(e.to_string()));
            }
        });

        let body = Body::from_stream(async_stream::stream! {
            while let Some(item) = rx.recv().await {
                match item {
                    Ok(chunk) => {
                        let line = json!({
                            "model": model_name,
                            "created_at": Utc::now().to_rfc3339(),
                            "message": { "role": "assistant", "content": chunk.content },
                            "done": false
                        });
                        yield Ok::<axum::body::Bytes, std::convert::Infallible>(
                            format!("{}\n", serde_json::to_string(&line).unwrap_or_default()).into()
                        );
                    }
                    Err(err_msg) => {
                        let err_line = json!({
                            "model": model_name,
                            "created_at": Utc::now().to_rfc3339(),
                            "error": err_msg,
                            "done": true
                        });
                        yield Ok::<axum::body::Bytes, std::convert::Infallible>(
                            format!("{}\n", serde_json::to_string(&err_line).unwrap_or_default()).into()
                        );
                        return;
                    }
                }
            }
            let done_chunk = json!({
                "model": model_name,
                "created_at": Utc::now().to_rfc3339(),
                "message": { "role": "assistant", "content": "" },
                "done": true
            });
            yield Ok::<axum::body::Bytes, std::convert::Infallible>(
                format!("{}\n", serde_json::to_string(&done_chunk).unwrap_or_default()).into()
            );
        });

        Ok(Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(body)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?)
    } else {
        let response = provider.chat(&messages).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;

        let body = serde_json::to_string(&json!({
            "model": model_name,
            "created_at": Utc::now().to_rfc3339(),
            "message": { "role": "assistant", "content": response },
            "done": true
        }))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
        Ok(Response::builder()
            .header("content-type", "application/json")
            .body(Body::from(body))
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?)
    }
}

pub async fn pull_model(
    State(state): State<AppState>,
    raw_body: Bytes,
) -> (StatusCode, Json<Value>) {
    let requested = match extract_model_or_name(&raw_body) {
        Ok(m) => m,
        Err(err_resp) => return err_resp,
    };

    // Normalize to canonical catalog ID when recognized (e.g. "qwen2:1.5b" -> "qwen-1.5b")
    let canonical_model = find_model(&requested)
        .map(|m| m.id.to_string())
        .unwrap_or_else(|| requested.clone());

    // Check if already downloading
    {
        let downloads = state.active_downloads.lock();
        if downloads.contains_key(&canonical_model) {
            return (
                StatusCode::OK,
                Json(json!({
                    "status": "already pulling",
                    "model": canonical_model,
                    "requested": requested
                })),
            );
        }
    }

    // Register as in-progress
    state
        .active_downloads
        .lock()
        .insert(canonical_model.clone(), "pulling".to_string());

    let model_clone = canonical_model.clone();
    let downloads = state.active_downloads.clone();
    tokio::spawn(async move {
        // Use headless variant — run() uses indicatif/stdout, unsafe in background tasks
        let result = crate::cli::download::run_headless(&model_clone).await;
        let status = if result.is_ok() { "success" } else { "error" };
        downloads.lock().insert(model_clone, status.to_string());
    });

    (
        StatusCode::OK,
        Json(json!({
            "status": "pulling manifest",
            "model": canonical_model,
            "requested": requested
        })),
    )
}

pub async fn embed(
    State(_state): State<AppState>,
    Json(req): Json<EmbedRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Parse input: accept string or array of strings
    let texts: Vec<String> = match &req.input {
        Value::String(s) => vec![s.clone()],
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "input must be a string or array of strings"})),
            ))
        }
    };

    if texts.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "input must not be empty"})),
        ));
    }

    let config = MithrilConfig::load().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("config load failed: {e}")})),
        )
    })?;

    let (provider, effective_model) =
        resolve_embed_provider(&req.model, &config).map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": format!("provider not available: {e}")})),
            )
        })?;

    let embeddings = provider.embed(&texts).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    Ok(Json(json!({
        "model": effective_model,
        "embeddings": embeddings
    })))
}

#[derive(Deserialize)]
pub struct RerankRequest {
    pub model: String,
    pub query: String,
    pub documents: Vec<String>,
    #[serde(default = "default_top_n")]
    pub top_n: usize,
}

fn default_top_n() -> usize {
    3
}

pub async fn rerank(
    State(state): State<AppState>,
    Json(req): Json<RerankRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    use crate::providers::ChatMessage as ProviderMessage;

    if req.documents.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "documents must not be empty"})),
        ));
    }
    if req.query.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "query must not be empty"})),
        ));
    }

    let config = MithrilConfig::load().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("config load failed: {e}")})),
        )
    })?;

    let model_dir = resolve_model_dir(&state);
    let provider = resolve_rerank_provider(&req.model, &config, &model_dir).map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": format!("provider not available: {e}")})),
        )
    })?;

    // Build prompt for LLM-based reranking
    let docs_list: String = req
        .documents
        .iter()
        .enumerate()
        .map(|(i, d)| format!("[{}] {}", i, d.chars().take(500).collect::<String>()))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Rate the relevance of each document to the query on a scale of 0 to 10. \
        Return ONLY a JSON array of numbers, one score per document, in order. \
        Example: [8, 2, 5]\n\nQuery: {}\n\nDocuments:\n{}\n\nScores:",
        req.query, docs_list
    );

    let messages = vec![ProviderMessage::user(&prompt)];
    let response = provider.chat(&messages).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    // Parse scores from LLM response
    let scores: Vec<f64> = parse_scores(&response, req.documents.len());

    // Build results sorted by score descending
    let mut results: Vec<Value> = scores
        .iter()
        .enumerate()
        .map(|(i, &score)| {
            json!({
                "index": i,
                "document": req.documents.get(i).cloned().unwrap_or_default(),
                "relevance_score": score / 10.0 // normalize to 0-1
            })
        })
        .collect();
    results.sort_by(|a, b| {
        b["relevance_score"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&a["relevance_score"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(req.top_n);

    Ok(Json(json!({ "results": results })))
}

/// Parse LLM response into scores array, with fallback for unparseable responses.
fn parse_scores(response: &str, num_docs: usize) -> Vec<f64> {
    // Try to find JSON array in response
    let trimmed = response.trim();
    // Find array bounds
    if let (Some(start), Some(end)) = (trimmed.find('['), trimmed.rfind(']')) {
        let array_str = &trimmed[start..=end];
        if let Ok(scores) = serde_json::from_str::<Vec<f64>>(array_str) {
            if scores.len() == num_docs {
                return scores;
            }
        }
    }
    // Fallback: try to extract individual numbers
    let numbers: Vec<f64> = trimmed.split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter_map(|s| s.parse::<f64>().ok())
        .filter(|&n| n >= 0.0 && n <= 10.0)
        .collect();
    if numbers.len() >= num_docs {
        return numbers[..num_docs].to_vec();
    }
    // Last resort: equal scores
    vec![5.0; num_docs]
}
