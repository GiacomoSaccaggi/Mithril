//! GitHub Copilot CLI provider — uses `copilot` CLI as a subprocess.
//!
//! Supports all models available in Copilot (GPT-4o, GPT-5.4, Claude, etc.)
//! Uses -p (non-interactive prompt mode) for structured output.
//! Install: npm install -g @github/copilot

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use super::{ChatMessage, ChatProvider, StreamChunk, ToolCallResult, ToolDefinition};

pub struct CopilotProvider {
    model: String,
}

impl CopilotProvider {
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

    /// Parse copilot output — remove the stats footer.
    fn parse_output(output: &str) -> String {
        let lines: Vec<&str> = output.lines().collect();
        // Find where stats start (lines like "Changes", "AI Credits", "Tokens", "Resume")
        let mut end = lines.len();
        for (i, line) in lines.iter().enumerate().rev() {
            if line.starts_with("Changes") || line.starts_with("AI Credits")
                || line.starts_with("Tokens") || line.starts_with("Resume")
            {
                end = i;
            }
        }
        // Trim trailing empty lines before stats
        while end > 0 && lines[end - 1].is_empty() {
            end -= 1;
        }
        lines[..end].join("\n")
    }
}

#[async_trait]
impl ChatProvider for CopilotProvider {
    fn name(&self) -> &str { "copilot" }
    fn model(&self) -> &str { &self.model }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let prompt = Self::build_prompt(messages);

        let mut cmd = tokio::process::Command::new("copilot");
        cmd.args(["-p", &prompt, "--model", &self.model]);

        let output = cmd.output().await
            .map_err(|e| anyhow!("Failed to run copilot CLI: {}. Install with: npm install -g @github/copilot", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Copilot CLI error: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let response = Self::parse_output(&stdout);

        if response.is_empty() {
            Err(anyhow!("No response from copilot CLI"))
        } else {
            Ok(response)
        }
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        // Copilot doesn't support streaming in non-interactive mode
        // Run synchronously and emit the full response as one chunk
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
        // Copilot has its own tools — we just get text back
        let response = self.chat(messages).await?;
        Ok(ToolCallResult::Text(response))
    }

    async fn is_available(&self) -> bool {
        tokio::process::Command::new("copilot")
            .arg("--version")
            .output()
            .await
            .is_ok()
    }
}
