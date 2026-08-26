//! Chat providers — unified interface for local and cloud LLMs.
//!
//! All providers implement [`ChatProvider`] which exposes:
//! - `chat()` — single response
//! - `chat_stream()` — token-by-token streaming
//! - `chat_with_tools()` — tool calling with structured responses
//!
//! ```mermaid
//! graph LR
//!     CP[ChatProvider trait]
//!     CP --> L[LocalProvider]
//!     CP --> G[GeminiProvider]
//!     CP --> O[OpenAIProvider]
//!     CP --> A[AnthropicProvider]
//!     CP --> Q[GroqProvider]
//!     L --> E[LazyModelManager]
//!     G --> API1[Gemini API]
//!     O --> API2[OpenAI API]
//!     A --> API3[Anthropic API]
//!     Q --> API4[Groq API]
//! ```

#![allow(dead_code)]
mod local;
pub mod kiro;
mod gemini;
mod openai;
mod anthropic;
mod groq;

pub use local::LocalProvider;
pub use gemini::GeminiProvider;
pub use openai::OpenAIProvider;
pub use anthropic::AnthropicProvider;
pub use groq::GroqProvider;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
}

/// Streaming chunk from provider
#[derive(Debug, Clone)]
pub struct StreamChunk {
    pub content: String,
    pub done: bool,
}

/// A tool call requested by the LLM
#[derive(Debug, Clone)]
pub struct ToolCall {
    /// Unique call ID (used to match result back to call)
    pub id: String,
    pub name: String,
    pub arguments: std::collections::HashMap<String, String>,
}

/// Result of chat_with_tools: either a final text reply or a tool invocation request
#[derive(Debug)]
pub enum ToolCallResult {
    /// LLM finished with a text response
    Text(String),
    /// LLM is requesting one or more tool calls
    ToolCalls(Vec<ToolCall>),
}

/// Definition of a tool exposed to the LLM
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema object for the parameters
    pub parameters: serde_json::Value,
}

impl ToolDefinition {
    pub fn from_registry_tool(tool: &dyn crate::tools::registry::Tool) -> Self {
        use serde_json::json;
        // M3: call parameters() once, reuse the Vec for both properties and required
        let params = tool.parameters();
        let properties: serde_json::Value = params.iter().fold(
            serde_json::Map::new(),
            |mut map, p| {
                map.insert(p.name.clone(), json!({
                    "type": p.param_type,
                    "description": p.description
                }));
                map
            },
        ).into();
        let required: Vec<&str> = params.iter()
            .filter(|p| p.required)
            .map(|p| p.name.as_str())
            .collect();
        Self {
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            parameters: json!({
                "type": "object",
                "properties": properties,
                "required": required
            }),
        }
    }
}

/// Unified interface for chat providers
#[async_trait]
pub trait ChatProvider: Send + Sync {
    /// Provider name (e.g., "local", "gemini", "openai")
    fn name(&self) -> &str;

    /// Model being used
    fn model(&self) -> &str;

    /// Generate a complete response
    async fn chat(&self, messages: &[ChatMessage]) -> Result<String>;

    /// Generate response with streaming (returns chunks via callback)
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String>;

    /// Generate a response with tool calling support.
    /// Returns either a final text or a list of tool calls to execute.
    /// Default implementation falls back to plain chat (no tool calling).
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<ToolCallResult> {
        let text = self.chat(messages).await?;
        Ok(ToolCallResult::Text(text))
    }

    /// Check if provider is available (has credentials, model exists, etc.)
    async fn is_available(&self) -> bool;
}

/// Create provider from name (uses model from global config).
pub fn create_provider(
    name: &str,
    config: &crate::config::MithrilConfig,
) -> Result<Box<dyn ChatProvider>> {
    create_provider_with_model(name, None, config)
}

/// Create provider with an optional model override.
/// If `model_override` is Some, uses that model instead of the global config default.
/// This is used by the fellowship orchestrator to respect per-agent model settings.
pub fn create_provider_with_model(
    name: &str,
    model_override: Option<&str>,
    config: &crate::config::MithrilConfig,
) -> Result<Box<dyn ChatProvider>> {
    match name {
        "kiro" => {
            // Kiro CLI provider — no API key needed (uses kiro-cli auth)
            let model = model_override.unwrap_or("claude-sonnet-4");
            Ok(Box::new(kiro::KiroProvider::new(model)))
        }
        "local" => {
            let model = model_override.unwrap_or(&config.default_model);
            Ok(Box::new(LocalProvider::new(model)?))
        }
        "gemini" => {
            let api_key = config
                .get_credential("gemini")?
                .ok_or_else(|| anyhow::anyhow!("Gemini API key not configured. Run: mithril config set gemini <your-api-key>"))?;
            let model = model_override.unwrap_or(&config.providers.gemini.model);
            Ok(Box::new(GeminiProvider::new(api_key, model)))
        }
        "openai" => {
            let api_key = config
                .get_credential("openai")?
                .ok_or_else(|| anyhow::anyhow!("OpenAI API key not configured. Run: mithril config set openai <your-api-key>"))?;
            let model = model_override.unwrap_or(&config.providers.openai.model);
            Ok(Box::new(OpenAIProvider::new(
                api_key, model, config.providers.openai.base_url.clone(),
            )))
        }
        "anthropic" => {
            let api_key = config
                .get_credential("anthropic")?
                .ok_or_else(|| anyhow::anyhow!("Anthropic API key not configured. Run: mithril config set anthropic <your-api-key>"))?;
            let model = model_override.unwrap_or(&config.providers.anthropic.model);
            Ok(Box::new(AnthropicProvider::new(api_key, model)))
        }
        "groq" => {
            let api_key = config
                .get_credential("groq")?
                .ok_or_else(|| anyhow::anyhow!("Groq API key not configured. Run: mithril config set groq <your-api-key>"))?;
            let model = model_override.unwrap_or(&config.providers.groq.model);
            Ok(Box::new(GroqProvider::new(
                api_key, model, config.providers.groq.base_url.clone(),
            )))
        }
        _ => anyhow::bail!("Unknown provider: {}. Available: local, gemini, openai, anthropic, groq", name),
    }
}

/// List all available providers
pub fn available_providers() -> Vec<&'static str> {
    vec!["local", "gemini", "openai", "anthropic", "groq"]
}


/// Retry an async operation with exponential backoff.
/// Retries on transient errors (429, 503, 500, connection errors).
pub(crate) async fn retry_with_backoff<F, Fut, T>(max_retries: u32, mut f: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut attempt = 0;
    loop {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                attempt += 1;
                let err_str = e.to_string();
                let is_retryable = err_str.contains("429")
                    || err_str.contains("503")
                    || err_str.contains("500")
                    || err_str.contains("rate limit")
                    || err_str.contains("connection")
                    || err_str.contains("timeout");

                if !is_retryable || attempt >= max_retries {
                    return Err(e);
                }

                let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1));
                tracing::warn!("Provider error (attempt {}/{}): {}. Retrying in {:?}", attempt, max_retries, err_str, delay);
                tokio::time::sleep(delay).await;
            }
        }
    }
}


/// Shared HTTP provider base — reduces boilerplate in cloud provider implementations.
pub struct HttpProviderBase {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub client: reqwest::Client,
}

impl HttpProviderBase {
    pub fn new(api_key: String, model: &str, base_url: &str) -> Self {
        Self {
            api_key,
            model: model.to_string(),
            base_url: base_url.to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_user() {
        let msg = ChatMessage::user("hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn test_chat_message_assistant() {
        let msg = ChatMessage::assistant("response");
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "response");
    }

    #[test]
    fn test_chat_message_system() {
        let msg = ChatMessage::system("you are helpful");
        assert_eq!(msg.role, "system");
    }

    #[test]
    fn test_available_providers_includes_all() {
        let providers = available_providers();
        assert!(providers.contains(&"local"));
        assert!(providers.contains(&"gemini"));
        assert!(providers.contains(&"openai"));
        assert!(providers.contains(&"anthropic"));
        assert!(providers.contains(&"groq"));
        assert_eq!(providers.len(), 5);
    }

    #[test]
    fn test_create_provider_unknown() {
        let config = crate::config::MithrilConfig::default();
        let result = create_provider("nonexistent", &config);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("Unknown provider"));
    }

    #[test]
    fn test_tool_definition_from_registry() {
        let registry = crate::tools::create_default_registry(".");
        let tool = registry.get("read_psi").unwrap();
        let def = ToolDefinition::from_registry_tool(tool);
        assert_eq!(def.name, "read_psi");
        assert!(!def.description.is_empty());
        assert!(def.parameters["properties"]["target"].is_object());
    }
}
