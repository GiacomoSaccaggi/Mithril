//! Kiro CLI provider — uses kiro-cli as a subprocess for inference.
//!
//! Supports all models available in Kiro (Claude Opus, Sonnet, Haiku, DeepSeek, etc.)
//! Uses --output-format stream-json for structured output parsing.

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use super::{ChatMessage, ChatProvider, StreamChunk, ToolCallResult, ToolDefinition};

pub struct KiroProvider {
    model: String,
}

impl KiroProvider {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
        }
    }

    /// Build the user prompt from message history.
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
impl ChatProvider for KiroProvider {
    fn name(&self) -> &str { "kiro" }
    fn model(&self) -> &str { &self.model }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let prompt = Self::build_prompt(messages);

        let output = tokio::process::Command::new("kiro-cli")
            .args([
                "chat",
                &prompt,
                "--model", &self.model,
                "--no-interactive",
                "--output-format", "stream-json",
                "--agent-engine", "v2",
            ])
            .output()
            .await
            .map_err(|e| anyhow!("Failed to run kiro-cli: {}. Is it installed?", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse JSON lines to find the final text
        for line in stdout.lines() {
            if line.contains("\"runFinished\"") {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(text) = parsed["data"]["finalText"].as_str() {
                        return Ok(text.to_string());
                    }
                }
            }
        }

        // Fallback: collect text chunks
        let mut text = String::new();
        for line in stdout.lines() {
            if line.contains("agent_message_chunk") {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(chunk) = parsed["data"]["update"]["content"]["text"].as_str() {
                        text.push_str(chunk);
                    }
                }
            }
        }

        if text.is_empty() {
            // Check for errors
            for line in stdout.lines() {
                if line.contains("\"runError\"") {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) {
                        if let Some(msg) = parsed["data"]["message"].as_str() {
                            return Err(anyhow!("Kiro error: {}", msg));
                        }
                    }
                }
            }
            Err(anyhow!("No response from kiro-cli"))
        } else {
            Ok(text)
        }
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        let prompt = Self::build_prompt(messages);

        let mut child = tokio::process::Command::new("kiro-cli")
            .args([
                "chat",
                &prompt,
                "--model", &self.model,
                "--no-interactive",
                "--output-format", "stream-json",
                "--agent-engine", "v2",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn kiro-cli: {}", e))?;

        let stdout = child.stdout.take().unwrap();
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut full_text = String::new();

        use tokio::io::AsyncBufReadExt;
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = reader.read_line(&mut line).await?;
            if bytes == 0 { break; }

            if line.contains("agent_message_chunk") {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(chunk) = parsed["data"]["update"]["content"]["text"].as_str() {
                        full_text.push_str(chunk);
                        on_chunk(StreamChunk { content: chunk.to_string(), done: false });
                    }
                }
            }
        }

        on_chunk(StreamChunk { content: String::new(), done: true });
        let _ = child.wait().await;
        Ok(full_text)
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<ToolCallResult> {
        // Kiro handles tools internally — we just get text back
        let response = self.chat(messages).await?;
        Ok(ToolCallResult::Text(response))
    }

    async fn is_available(&self) -> bool {
        tokio::process::Command::new("kiro-cli")
            .arg("--version")
            .output()
            .await
            .is_ok()
    }
}
