//! Google Gemini provider with real SSE streaming.

use super::{ChatMessage, ChatProvider, StreamChunk};
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

pub struct GeminiProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: &str) -> Self {
        Self {
            api_key,
            model: model.to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: GeminiContentResponse,
}

#[derive(Deserialize)]
struct GeminiContentResponse {
    parts: Vec<GeminiPartResponse>,
}

#[derive(Deserialize)]
struct GeminiPartResponse {
    text: String,
}

fn build_contents(messages: &[ChatMessage]) -> Vec<GeminiContent> {
    // Gemini has no "system" role — prepend system content to the first user message
    let mut contents: Vec<GeminiContent> = Vec::new();
    let mut system_prefix = String::new();

    for m in messages {
        if m.role == "system" {
            system_prefix.push_str(&m.content);
            system_prefix.push('\n');
        }
    }

    for m in messages {
        if m.role == "system" {
            continue;
        }
        let text = if !system_prefix.is_empty() && m.role == "user" && contents.is_empty() {
            format!("{}{}", system_prefix, m.content)
        } else {
            m.content.clone()
        };
        contents.push(GeminiContent {
            role: if m.role == "assistant" { "model".to_string() } else { "user".to_string() },
            parts: vec![GeminiPart { text }],
        });
    }

    contents
}

#[async_trait]
impl ChatProvider for GeminiProvider {
    fn name(&self) -> &str { "gemini" }
    fn model(&self) -> &str { &self.model }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let request = GeminiRequest {
            contents: build_contents(messages),
            generation_config: Some(GeminiGenerationConfig { max_output_tokens: Some(8192) }),
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error ({}): {}", status, body);
        }

        let parsed: GeminiResponse = response.json().await?;
        Ok(parsed
            .candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .map(|p| p.text.clone())
            .unwrap_or_default())
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        let request = GeminiRequest {
            contents: build_contents(messages),
            generation_config: Some(GeminiGenerationConfig { max_output_tokens: Some(8192) }),
        };

        // :streamGenerateContent with alt=sse returns SSE events
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.model, self.api_key
        );

        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini streaming API error ({}): {}", status, body);
        }

        let mut full_text = String::new();
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            // SSE events are separated by "\n\n"
            while let Some(pos) = buffer.find("\n\n") {
                let event = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                for line in event.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            continue;
                        }
                        if let Ok(parsed) = serde_json::from_str::<GeminiResponse>(data) {
                            if let Some(text) = parsed
                                .candidates
                                .first()
                                .and_then(|c| c.content.parts.first())
                                .map(|p| p.text.clone())
                            {
                                full_text.push_str(&text);
                                on_chunk(StreamChunk { content: text, done: false });
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

        // Gemini function declarations format
        let function_declarations: Vec<Value> = tools.iter().map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters
            })
        }).collect();

        let contents = build_contents(messages);

        let mut request_body = json!({
            "contents": contents,
            "generationConfig": { "maxOutputTokens": 8192 }
        });

        if !function_declarations.is_empty() {
            request_body["tools"] = json!([{
                "functionDeclarations": function_declarations
            }]);
            // Auto mode: model decides when to call functions
            request_body["toolConfig"] = json!({
                "functionCallingConfig": { "mode": "AUTO" }
            });
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let response = self.client.post(&url).json(&request_body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini tool call API error ({}): {}", status, body);
        }

        let body: Value = response.json().await?;
        let candidate = &body["candidates"][0]["content"];
        let parts = candidate["parts"].as_array();

        // Check for function calls in the response parts
        if let Some(parts) = parts {
            let mut tool_calls = Vec::new();
            let mut text_parts = Vec::new();

            for part in parts {
                if let Some(fc) = part.get("functionCall") {
                    let name = fc["name"].as_str().unwrap_or("").to_string();
                    let args = fc.get("args").cloned().unwrap_or(Value::Object(Default::default()));
                    let arguments: std::collections::HashMap<String, String> = args
                        .as_object()
                        .map(|obj| {
                            obj.iter()
                                .map(|(k, v)| (k.clone(), v.as_str()
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| v.to_string())))
                                .collect()
                        })
                        .unwrap_or_default();
                    tool_calls.push(crate::providers::ToolCall {
                        id: uuid::Uuid::new_v4().to_string(),
                        name,
                        arguments,
                    });
                } else if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(text.to_string());
                }
            }

            if !tool_calls.is_empty() {
                return Ok(crate::providers::ToolCallResult::ToolCalls(tool_calls));
            }
            if !text_parts.is_empty() {
                return Ok(crate::providers::ToolCallResult::Text(text_parts.join("")));
            }
        }

        Ok(crate::providers::ToolCallResult::Text(String::new()))
    }
}
