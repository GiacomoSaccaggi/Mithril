//! Anthropic Claude provider with real SSE streaming.

#![allow(dead_code)]
use super::{ChatMessage, ChatProvider, StreamChunk};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: &str) -> Self {
        Self {
            api_key,
            model: model.to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize)]
struct AnthropicContent {
    text: String,
}

/// SSE event types from Anthropic streaming API
#[derive(Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    delta: Option<AnthropicDelta>,
}

#[derive(Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
}

fn split_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<AnthropicMessage>) {
    let system = messages.iter().find(|m| m.role == "system").map(|m| m.content.clone());
    let msgs = messages
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| AnthropicMessage { role: m.role.clone(), content: m.content.clone() })
        .collect();
    (system, msgs)
}

#[async_trait]
impl ChatProvider for AnthropicProvider {
    fn name(&self) -> &str { "anthropic" }
    fn model(&self) -> &str { &self.model }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let (system, msgs) = split_messages(messages);
        let request = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            messages: msgs,
            system,
            stream: None,
        };

        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error ({}): {}", status, body);
        }

        let parsed: AnthropicResponse = response.json().await?;
        Ok(parsed.content.first().map(|c| c.text.clone()).unwrap_or_default())
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        let (system, msgs) = split_messages(messages);
        let request = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            messages: msgs,
            system,
            stream: Some(true),
        };

        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic streaming API error ({}): {}", status, body);
        }

        let mut full_text = String::new();
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            // SSE events separated by "\n\n"
            while let Some(pos) = buffer.find("\n\n") {
                let event = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                let mut event_type_line = String::new();
                let mut data_line = String::new();

                for line in event.lines() {
                    if let Some(t) = line.strip_prefix("event: ") {
                        event_type_line = t.to_string();
                    } else if let Some(d) = line.strip_prefix("data: ") {
                        data_line = d.to_string();
                    }
                }

                // We care about content_block_delta events
                if event_type_line == "content_block_delta" || !event_type_line.is_empty() {
                    if let Ok(parsed) = serde_json::from_str::<AnthropicStreamEvent>(&data_line) {
                        if parsed.event_type == "content_block_delta" {
                            if let Some(delta) = &parsed.delta {
                                if delta.delta_type.as_deref() == Some("text_delta") {
                                    if let Some(text) = &delta.text {
                                        full_text.push_str(text);
                                        on_chunk(StreamChunk { content: text.clone(), done: false });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        on_chunk(StreamChunk { content: String::new(), done: true });
        Ok(full_text)
    }

    async fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[crate::providers::ToolDefinition],
    ) -> anyhow::Result<crate::providers::ToolCallResult> {
        use serde_json::{json, Value};

        let (system, msgs) = split_messages(messages);

        let anthropic_tools: Vec<Value> = tools.iter().map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters
            })
        }).collect();

        let mut request = json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": msgs.iter().map(|m| json!({ "role": m.role, "content": m.content })).collect::<Vec<_>>(),
            "tools": anthropic_tools
        });
        if let Some(sys) = system {
            request["system"] = json!(sys);
        }

        let response = self.client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic tool call API error ({}): {}", status, body);
        }

        let body: Value = response.json().await?;

        // Anthropic returns content as an array of blocks
        if let Some(content_arr) = body["content"].as_array() {
            let tool_calls: Vec<crate::providers::ToolCall> = content_arr
                .iter()
                .filter(|block| block["type"].as_str() == Some("tool_use"))
                .map(|block| {
                    let id = block["id"].as_str().unwrap_or("").to_string();
                    let name = block["name"].as_str().unwrap_or("").to_string();
                    let arguments = block["input"]
                        .as_object()
                        .map(|obj| {
                            obj.iter()
                                .map(|(k, v)| {
                                    (k.clone(), v.as_str().map(|s| s.to_string())
                                        .unwrap_or_else(|| v.to_string()))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    crate::providers::ToolCall { id, name, arguments }
                })
                .collect();

            if !tool_calls.is_empty() {
                return Ok(crate::providers::ToolCallResult::ToolCalls(tool_calls));
            }

            // Collect text blocks
            let text: String = content_arr
                .iter()
                .filter(|b| b["type"].as_str() == Some("text"))
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            return Ok(crate::providers::ToolCallResult::Text(text));
        }

        Ok(crate::providers::ToolCallResult::Text(String::new()))
    }
}
