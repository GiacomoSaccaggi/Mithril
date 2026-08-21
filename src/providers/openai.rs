//! OpenAI provider (also works with compatible APIs like Azure, Groq, etc.)

#![allow(dead_code)]
use super::{ChatMessage, ChatProvider, StreamChunk};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub struct OpenAIProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAIProvider {
    pub fn new(api_key: String, model: &str, base_url: Option<String>) -> Self {
        Self {
            api_key,
            model: model.to_string(),
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
}

#[derive(Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Deserialize)]
struct OpenAIStreamResponse {
    choices: Vec<OpenAIStreamChoice>,
}

#[derive(Deserialize)]
struct OpenAIStreamChoice {
    delta: OpenAIDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIDelta {
    content: Option<String>,
}

#[async_trait]
impl ChatProvider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let openai_messages: Vec<OpenAIMessage> = messages
            .iter()
            .map(|m| OpenAIMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let request = OpenAIRequest {
            model: self.model.clone(),
            messages: openai_messages,
            max_tokens: Some(4096),
            stream: false,
        };

        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error ({}): {}", status, error_text);
        }

        let openai_response: OpenAIResponse = response.json().await?;

        let text = openai_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(text)
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        let openai_messages: Vec<OpenAIMessage> = messages
            .iter()
            .map(|m| OpenAIMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let request = OpenAIRequest {
            model: self.model.clone(),
            messages: openai_messages,
            max_tokens: Some(4096),
            stream: true,
        };

        let url = format!("{}/chat/completions", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error ({}): {}", status, error_text);
        }

        let mut full_response = String::new();
        let mut stream = response.bytes_stream();

        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);

            for line in text.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        on_chunk(StreamChunk {
                            content: String::new(),
                            done: true,
                        });
                        break;
                    }

                    if let Ok(parsed) = serde_json::from_str::<OpenAIStreamResponse>(data) {
                        if let Some(choice) = parsed.choices.first() {
                            if let Some(content) = &choice.delta.content {
                                full_response.push_str(content);
                                on_chunk(StreamChunk {
                                    content: content.clone(),
                                    done: choice.finish_reason.is_some(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(full_response)
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

        let openai_messages: Vec<Value> = messages.iter().map(|m| {
            json!({ "role": m.role, "content": m.content })
        }).collect();

        let openai_tools: Vec<Value> = tools.iter().map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters
                }
            })
        }).collect();

        let url = format!("{}/chat/completions", self.base_url);
        let request = json!({
            "model": self.model,
            "messages": openai_messages,
            "tools": openai_tools,
            "tool_choice": "auto"
        });

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI tool call API error ({}): {}", status, body);
        }

        let body: Value = response.json().await?;
        let choice = &body["choices"][0]["message"];

        // Check for tool_calls
        if let Some(tool_calls) = choice["tool_calls"].as_array() {
            let calls: Vec<crate::providers::ToolCall> = tool_calls
                .iter()
                .filter_map(|tc| {
                    let id = tc["id"].as_str()?.to_string();
                    let name = tc["function"]["name"].as_str()?.to_string();
                    let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                    let args_value: Value = serde_json::from_str(args_str).unwrap_or_default();
                    let arguments = args_value
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
                    Some(crate::providers::ToolCall { id, name, arguments })
                })
                .collect();

            if !calls.is_empty() {
                return Ok(crate::providers::ToolCallResult::ToolCalls(calls));
            }
        }

        // Plain text response
        let text = choice["content"].as_str().unwrap_or("").to_string();
        Ok(crate::providers::ToolCallResult::Text(text))
    }
}
