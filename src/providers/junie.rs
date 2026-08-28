//! Junie CLI provider — uses JetBrains Junie CLI as a subprocess.
//!
//! Supports non-interactive mode with JSON output for clean parsing.
//! Install: Junie CLI comes with JetBrains AI subscription.

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use super::{ChatMessage, ChatProvider, StreamChunk, ToolCallResult, ToolDefinition};

pub struct JunieProvider {
    model: String,
}

impl JunieProvider {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
        }
    }

    /// Build prompt from message history.
    fn build_prompt(messages: &[ChatMessage]) -> String {
        let mut parts: Vec<String> = Vec::new();
        for msg in messages {
            match msg.role.as_str() {
                "system" => parts.push(format!("[System: {}]", msg.content)),
                "user" => parts.push(msg.content.clone()),
                "assistant" => parts.push(format!("[Previous response: {}]", msg.content)),
                _ => parts.push(msg.content.clone()),
            }
        }
        parts.join("\n\n")
    }
}

#[async_trait]
impl ChatProvider for JunieProvider {
    fn name(&self) -> &str { "junie" }
    fn model(&self) -> &str { &self.model }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let prompt = Self::build_prompt(messages);

        let output = tokio::process::Command::new("junie")
            .args([&prompt, "--output-format", "json", "--model", &self.model])
            .output()
            .await
            .map_err(|e| anyhow!("Failed to run junie CLI: {}. Is it installed?", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse JSON output — find the result field
        for line in stdout.lines() {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(result) = parsed["result"].as_str() {
                    return Ok(result.to_string());
                }
            }
        }

        // Fallback: return raw stdout cleaned of ANSI codes
        let cleaned: String = stdout.chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect();

        if cleaned.trim().is_empty() {
            Err(anyhow!("No response from junie CLI"))
        } else {
            Ok(cleaned.trim().to_string())
        }
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        let response = self.chat(messages).await?;
        on_chunk(StreamChunk { content: response.clone(), done: false });
        on_chunk(StreamChunk { content: String::new(), done: true });
        Ok(response)
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<ToolCallResult> {
        // Junie has its own tools — we just get text back
        let response = self.chat(messages).await?;
        Ok(ToolCallResult::Text(response))
    }

    async fn is_available(&self) -> bool {
        tokio::process::Command::new("junie")
            .arg("--help")
            .output()
            .await
            .is_ok()
    }
}
