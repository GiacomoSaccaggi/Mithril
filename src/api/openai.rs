#![allow(dead_code)]
use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::api::server::AppState;
use crate::engine::{
    chat_template::{format_chat, get_stop_tokens, ChatMessage},
    model_catalog::{find_model, MODELS},
};

#[derive(Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OAIMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: Option<bool>,
}

#[derive(Deserialize, Serialize)]
pub struct OAIMessage {
    pub role: String,
    pub content: String,
}

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let model_info = find_model(&req.model)
        // Graceful fallback to first model if unknown
        .unwrap_or(&MODELS[0]);

    let messages: Vec<ChatMessage> = req.messages.iter()
        .map(|m| ChatMessage::new(&m.role, &m.content))
        .collect();

    let formatted = format_chat(model_info.chat_template, &messages);
    let stops = get_stop_tokens(model_info.chat_template);
    let temperature = req.temperature.unwrap_or(0.7);
    let max_tokens = req.max_tokens.unwrap_or(2048);

    let response = state.model_manager
        .infer(&formatted, &stops, temperature, max_tokens)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": { "message": e.to_string(), "type": "server_error" } })),
            )
        })?;

    Ok(Json(json!({
        "id": format!("chatcmpl-{}", Uuid::new_v4()),
        "object": "chat.completion",
        "created": Utc::now().timestamp(),
        "model": req.model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": response
            },
            "finish_reason": "stop"
        }],
        "usage": {
            // L2: approximate token count (chars/4 heuristic — better than constant 0)
            "prompt_tokens": req.messages.iter().map(|m| m.content.len() / 4).sum::<usize>() as u64,
            "completion_tokens": (response.len() / 4) as u64,
            "total_tokens": (req.messages.iter().map(|m| m.content.len() / 4).sum::<usize>() + response.len() / 4) as u64
        }
    })))
}

pub async fn list_models() -> Json<Value> {
    let models: Vec<Value> = MODELS.iter().map(|m| {
        json!({
            "id": format!("{}:{}", m.family, m.parameter_size.to_lowercase()),
            "object": "model",
            "created": 1700000000,
            "owned_by": "mithril"
        })
    }).collect();

    Json(json!({
        "object": "list",
        "data": models
    }))
}
