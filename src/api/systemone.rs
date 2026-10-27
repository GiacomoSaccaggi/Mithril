//! JEV SystemOne decision & classification endpoint (/v1/systemone)
#![allow(dead_code)]
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::api::server::AppState;
use crate::engine::{
    chat_template::{get_stop_tokens, ChatTemplate},
    model_catalog::{find_model, MODELS},
};

#[derive(Deserialize)]
pub struct SystemOneApiRequest {
    pub model: Option<String>,
    pub state: String,
    pub questions: HashMap<String, SystemOneApiQuestion>,
}

#[derive(Deserialize)]
pub struct SystemOneApiQuestion {
    #[serde(rename = "type")]
    pub question_type: String,
    pub instructions: String,
    pub criteria: Option<HashMap<String, String>>,
}

#[derive(Serialize)]
pub struct SystemOneApiResponse {
    pub model: String,
    pub answers: HashMap<String, SystemOneApiAnswer>,
    pub usage: SystemOneApiUsage,
}

#[derive(Serialize)]
pub struct SystemOneApiAnswer {
    #[serde(rename = "type")]
    pub answer_type: String,
    pub choice: Option<String>,
    pub value: Option<Value>,
    pub confidence: f64,
}

#[derive(Serialize)]
pub struct SystemOneApiUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

pub async fn systemone_handler(
    State(state): State<AppState>,
    Json(req): Json<SystemOneApiRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let model_name = req.model.as_deref().unwrap_or("jevos-v4");
    let model_info = find_model(model_name).unwrap_or(&MODELS[0]);

    let mut answers = HashMap::new();
    let mut total_output_tokens = 0;

    for (q_id, q) in &req.questions {
        let (prompt, stops) = match q.question_type.as_str() {
            "choice" => {
                let mut options_text = String::new();
                if let Some(ref crit) = q.criteria {
                    for (k, v) in crit {
                        options_text.push_str(&format!("- {}: {}\n", k, v));
                    }
                }
                let prompt = format!(
                    "<evidence>\n{}\n\nCandidates:\n{}\n</evidence>\n\nQuestion: {}\nAnswer:",
                    req.state.trim(),
                    options_text.trim(),
                    q.instructions.trim()
                );
                (prompt, get_stop_tokens(ChatTemplate::Jev))
            }
            _ => {
                let prompt = format!(
                    "<evidence>\n{}\n</evidence>\n\nQuestion: {}\nAnswer:",
                    req.state.trim(),
                    q.instructions.trim()
                );
                (prompt, get_stop_tokens(model_info.chat_template))
            }
        };

        let response = state.model_manager
            .infer(&prompt, &stops, 0.1, 64)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": { "message": e.to_string(), "type": "server_error" } })),
                )
            })?;

        total_output_tokens += response.len() / 4 + 1;
        let trimmed_reply = response.trim();

        if q.question_type == "choice" {
            let reply_lower = trimmed_reply.to_lowercase();
            let mut matched_choice = trimmed_reply.to_string();
            if let Some(ref crit) = q.criteria {
                for k in crit.keys() {
                    if reply_lower.contains(&k.to_lowercase()) {
                        matched_choice = k.clone();
                        break;
                    }
                }
            }
            answers.insert(
                q_id.clone(),
                json!({
                    "type": "choice",
                    "choice": matched_choice,
                    "confidence": 0.95
                }),
            );
        } else {
            answers.insert(
                q_id.clone(),
                json!({
                    "type": q.question_type,
                    "value": trimmed_reply,
                    "confidence": 0.90
                }),
            );
        }
    }

    let input_tokens = req.state.len() / 4;

    Ok(Json(json!({
        "model": model_info.id,
        "answers": answers,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": total_output_tokens,
        }
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_systemone_request_deserialization() {
        let raw = r#"{
            "model": "jevos-v4",
            "state": "The server crashed due to out of memory error.",
            "questions": {
                "route": {
                    "type": "choice",
                    "instructions": "Which agent should fix this?",
                    "criteria": {
                        "coder": "Fixes code issues and bugs",
                        "architect": "Designs high level systems"
                    }
                }
            }
        }"#;

        let req: SystemOneApiRequest = serde_json::from_str(raw).expect("valid json");
        assert_eq!(req.model.as_deref(), Some("jevos-v4"));
        assert_eq!(req.questions.len(), 1);
        let q = &req.questions["route"];
        assert_eq!(q.question_type, "choice");
        assert!(q.criteria.as_ref().unwrap().contains_key("coder"));
    }
}
