//! JEV provider — connects to JEV classification and decision services (POST /v1/systemone)
//! or cloud TypeSafe JEV endpoints.

#![allow(dead_code)]
use super::{ChatMessage, ChatProvider, StreamChunk};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub struct JevProvider {
    model: String,
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl JevProvider {
    pub fn new(model: &str, base_url: String, api_key: Option<String>) -> Self {
        Self {
            model: model.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Serialize)]
struct SystemOneRequest {
    model: String,
    state: String,
    questions: HashMap<String, SystemOneQuestion>,
}

#[derive(Serialize)]
struct SystemOneQuestion {
    #[serde(rename = "type")]
    question_type: String,
    instructions: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    criteria: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
struct SystemOneResponse {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    answers: HashMap<String, SystemOneAnswer>,
}

#[derive(Deserialize)]
struct SystemOneAnswer {
    #[serde(rename = "type")]
    answer_type: Option<String>,
    choice: Option<String>,
    value: Option<serde_json::Value>,
    confidence: Option<f64>,
}

#[async_trait]
impl ChatProvider for JevProvider {
    fn name(&self) -> &str {
        "jev"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn is_available(&self) -> bool {
        let health_url = format!("{}/health", self.base_url);
        match self.client.get(&health_url).timeout(std::time::Duration::from_millis(500)).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(_) => false,
        }
    }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let mut evidence = String::new();
        let mut question = String::new();
        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    if !question.is_empty() {
                        question.push(' ');
                    }
                    question.push_str(&msg.content);
                }
                "user" => {
                    if evidence.is_empty() {
                        evidence.push_str(&msg.content);
                    } else {
                        evidence.push('\n');
                        evidence.push_str(&msg.content);
                    }
                }
                _ => {
                    evidence.push_str(&msg.content);
                }
            }
        }
        if question.is_empty() {
            question = "Provide the decision or classification.".to_string();
        }

        let mut questions = HashMap::new();
        questions.insert(
            "q".to_string(),
            SystemOneQuestion {
                question_type: "freeform".to_string(),
                instructions: question,
                criteria: None,
            },
        );

        let body = SystemOneRequest {
            model: self.model.clone(),
            state: evidence,
            questions,
        };

        let url = format!("{}/v1/systemone", self.base_url);
        let mut req = self.client.post(&url).json(&body);
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let err = response.text().await.unwrap_or_default();
            anyhow::bail!("JEV API error ({}): {}", status, err);
        }

        let resp: SystemOneResponse = response.json().await?;
        if let Some(ans) = resp.answers.get("q") {
            if let Some(ref choice) = ans.choice {
                return Ok(choice.clone());
            }
            if let Some(ref val) = ans.value {
                return Ok(val.to_string());
            }
        }

        Ok(String::new())
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        let text = self.chat(messages).await?;
        on_chunk(StreamChunk {
            content: text.clone(),
            done: true,
        });
        Ok(text)
    }

    async fn decide_choice(
        &self,
        instructions: &str,
        state: &str,
        candidates: &HashMap<String, String>,
    ) -> Result<String> {
        let mut questions = HashMap::new();
        questions.insert(
            "routing".to_string(),
            SystemOneQuestion {
                question_type: "choice".to_string(),
                instructions: instructions.to_string(),
                criteria: Some(candidates.clone()),
            },
        );

        let body = SystemOneRequest {
            model: self.model.clone(),
            state: state.to_string(),
            questions,
        };

        let url = format!("{}/v1/systemone", self.base_url);
        let mut req = self.client.post(&url).json(&body);
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let err = response.text().await.unwrap_or_default();
            anyhow::bail!("JEV routing error ({}): {}", status, err);
        }

        let resp: SystemOneResponse = response.json().await?;
        if let Some(ans) = resp.answers.get("routing") {
            if let Some(ref choice) = ans.choice {
                return Ok(choice.clone());
            }
        }

        anyhow::bail!("JEV did not return a choice answer for routing");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jev_provider_creation() {
        let provider = JevProvider::new("jevos-v4", "http://localhost:8017".to_string(), None);
        assert_eq!(provider.name(), "jev");
        assert_eq!(provider.model(), "jevos-v4");
    }
}
