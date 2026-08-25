//! Groq provider — OpenAI-compatible API with Compound (server-side tool) support.
//!
//! Standard models (llama-4-scout, qwen3-32b, etc.): same as OpenAI — chat, streaming, tool calling.
//! Compound models (groq/compound, groq/compound-mini): tools executed server-side by Groq.

#![allow(dead_code)]
use super::{ChatMessage, ChatProvider, StreamChunk, ToolCall, ToolCallResult, ToolDefinition};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "https://api.groq.com/openai/v1";

pub struct GroqProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl GroqProvider {
    pub fn new(api_key: String, model: &str, base_url: Option<String>) -> Self {
        Self {
            api_key,
            model: model.to_string(),
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            client: reqwest::Client::new(),
        }
    }

    /// Returns true if the current model is a Compound model (server-side tools).
    fn is_compound(&self) -> bool {
        self.model.starts_with("groq/compound")
    }
}

// ── Request/Response types (OpenAI-compatible) ──────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<RequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    compound_custom: Option<CompoundCustom>,
}

#[derive(Serialize, Deserialize)]
struct RequestMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct CompoundCustom {
    tools: CompoundTools,
}

#[derive(Serialize)]
struct CompoundTools {
    enabled_tools: Vec<String>,
}

impl Default for CompoundCustom {
    fn default() -> Self {
        Self {
            tools: CompoundTools {
                enabled_tools: vec![
                    "web_search".to_string(),
                    "code_interpreter".to_string(),
                    "visit_website".to_string(),
                ],
            },
        }
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ResponseToolCall>>,
    #[serde(default)]
    executed_tools: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ResponseToolCall {
    id: String,
    function: FunctionCall,
}

#[derive(Deserialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct StreamResponse {
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

// ── ChatProvider implementation ─────────────────────────────────────────────

#[async_trait]
impl ChatProvider for GroqProvider {
    fn name(&self) -> &str {
        "groq"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let request_messages: Vec<RequestMessage> = messages
            .iter()
            .map(|m| RequestMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let request = ChatRequest {
            model: self.model.clone(),
            messages: request_messages,
            max_tokens: Some(4096),
            stream: false,
            compound_custom: if self.is_compound() {
                Some(CompoundCustom::default())
            } else {
                None
            },
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
            anyhow::bail!("Groq API error ({}): {}", status, error_text);
        }

        let chat_response: ChatResponse = response.json().await?;

        // Log executed_tools if present (compound mode transparency)
        if let Some(choice) = chat_response.choices.first() {
            if let Some(ref tools) = choice.message.executed_tools {
                tracing::debug!("Groq compound executed_tools: {}", tools);
            }
        }

        let text = chat_response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(text)
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        let request_messages: Vec<RequestMessage> = messages
            .iter()
            .map(|m| RequestMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let request = ChatRequest {
            model: self.model.clone(),
            messages: request_messages,
            max_tokens: Some(4096),
            stream: true,
            compound_custom: if self.is_compound() {
                Some(CompoundCustom::default())
            } else {
                None
            },
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
            anyhow::bail!("Groq API error ({}): {}", status, error_text);
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

                    if let Ok(parsed) = serde_json::from_str::<StreamResponse>(data) {
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
        tools: &[ToolDefinition],
    ) -> Result<ToolCallResult> {
        // Compound models handle tools server-side — ignore Mithril's tools,
        // just do a plain chat and return the final text.
        if self.is_compound() {
            let text = self.chat(messages).await?;
            return Ok(ToolCallResult::Text(text));
        }

        // Standard models: OpenAI-compatible tool calling
        use serde_json::{json, Value};

        let openai_messages: Vec<Value> = messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect();

        let openai_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect();

        let url = format!("{}/chat/completions", self.base_url);
        let request = json!({
            "model": self.model,
            "messages": openai_messages,
            "tools": openai_tools,
            "tool_choice": "auto"
        });

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
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Groq tool call API error ({}): {}", status, body);
        }

        let body: Value = response.json().await?;
        let choice = &body["choices"][0]["message"];

        // Check for tool_calls
        if let Some(tool_calls) = choice["tool_calls"].as_array() {
            let calls: Vec<ToolCall> = tool_calls
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
                                    (
                                        k.clone(),
                                        v.as_str()
                                            .map(|s| s.to_string())
                                            .unwrap_or_else(|| v.to_string()),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(ToolCall { id, name, arguments })
                })
                .collect();

            if !calls.is_empty() {
                return Ok(ToolCallResult::ToolCalls(calls));
            }
        }

        // Plain text response
        let text = choice["content"].as_str().unwrap_or("").to_string();
        Ok(ToolCallResult::Text(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_compound_true() {
        let provider = GroqProvider::new("key".into(), "groq/compound", None);
        assert!(provider.is_compound());
    }

    #[test]
    fn test_is_compound_mini_true() {
        let provider = GroqProvider::new("key".into(), "groq/compound-mini", None);
        assert!(provider.is_compound());
    }

    #[test]
    fn test_is_compound_false_standard_model() {
        let provider = GroqProvider::new("key".into(), "meta-llama/llama-4-scout-17b-16e-instruct", None);
        assert!(!provider.is_compound());
    }

    #[test]
    fn test_is_compound_false_other_model() {
        let provider = GroqProvider::new("key".into(), "llama-3.3-70b-versatile", None);
        assert!(!provider.is_compound());
    }

    #[test]
    fn test_default_base_url() {
        let provider = GroqProvider::new("key".into(), "model", None);
        assert_eq!(provider.base_url, "https://api.groq.com/openai/v1");
    }

    #[test]
    fn test_custom_base_url() {
        let provider = GroqProvider::new("key".into(), "model", Some("http://proxy:8080".into()));
        assert_eq!(provider.base_url, "http://proxy:8080");
    }

    #[test]
    fn test_provider_name() {
        let provider = GroqProvider::new("key".into(), "model", None);
        assert_eq!(provider.name(), "groq");
    }

    #[test]
    fn test_provider_model() {
        let provider = GroqProvider::new("key".into(), "llama-4-scout", None);
        assert_eq!(provider.model(), "llama-4-scout");
    }

    #[test]
    fn test_compound_custom_default() {
        let custom = CompoundCustom::default();
        assert!(custom.tools.enabled_tools.contains(&"web_search".to_string()));
        assert!(custom.tools.enabled_tools.contains(&"code_interpreter".to_string()));
        assert!(custom.tools.enabled_tools.contains(&"visit_website".to_string()));
    }
}
