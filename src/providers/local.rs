//! Local GGUF model provider — uses a global LazyModelManager singleton per model path.
//! Having a singleton prevents loading the same model twice when LocalProvider is
//! instantiated multiple times (e.g. on provider switch in mithril chat).

#![allow(dead_code)]
use super::{ChatMessage, ChatProvider, StreamChunk};
use crate::engine::{self, find_model, get_stop_tokens, ChatTemplate, LazyModelManager};
use anyhow::{Context, Result};
use async_trait::async_trait;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// Global registry: model_path → Weak<LazyModelManager>
// - Weak references allow the manager to be freed when no LocalProvider holds it.
// - If all LocalProviders for a model are dropped, the registry entry is cleaned up.
lazy_static! {
    static ref MODEL_REGISTRY: Mutex<HashMap<PathBuf, std::sync::Weak<LazyModelManager>>> =
        Mutex::new(HashMap::new());
}

fn get_or_create_manager(model_path: PathBuf) -> Arc<LazyModelManager> {
    let mut registry = MODEL_REGISTRY.lock();
    // Evict stale entries while we hold the lock
    registry.retain(|_, weak| weak.strong_count() > 0);
    // Try to upgrade existing Weak, or create a new manager
    if let Some(weak) = registry.get(&model_path) {
        if let Some(arc) = weak.upgrade() {
            return arc;
        }
    }
    let arc = Arc::new(LazyModelManager::new(model_path.clone(), 60));
    registry.insert(model_path, Arc::downgrade(&arc));
    arc
}

pub struct LocalProvider {
    model_id: String,
    model_path: PathBuf,
    template: ChatTemplate,
    manager: Arc<LazyModelManager>,
}

impl LocalProvider {
    pub fn new(model_id: &str) -> Result<Self> {
        let model_info = find_model(model_id)
            .with_context(|| format!("Unknown model: {}", model_id))?;

        let model_path = dirs::home_dir()
            .context("Could not find home directory")?
            .join(".mithril")
            .join("models")
            .join(model_info.file_name);

        if !model_path.exists() {
            anyhow::bail!(
                "Model {} not downloaded. Run: mithril download-model --model {}",
                model_id, model_id
            );
        }

        let manager = get_or_create_manager(model_path.clone());
        Ok(Self {
            model_id: model_id.to_string(),
            model_path,
            template: model_info.chat_template,
            manager,
        })
    }

    fn format(&self, messages: &[ChatMessage]) -> (String, Vec<String>) {
        let engine_msgs: Vec<engine::ChatMessage> = messages
            .iter()
            .map(|m| engine::ChatMessage::new(&m.role, &m.content))
            .collect();
        let formatted = engine::format_chat(self.template, &engine_msgs);
        let stops = get_stop_tokens(self.template);
        (formatted, stops)
    }
}

#[async_trait]
impl ChatProvider for LocalProvider {
    fn name(&self) -> &str { "local" }
    fn model(&self) -> &str { &self.model_id }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let (formatted, stops) = self.format(messages);
        self.manager.infer(&formatted, &stops, 0.7, 2048)
    }

    /// Real token-by-token streaming via std::sync::mpsc → tokio::sync::mpsc bridge.
    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        let (formatted, stops) = self.format(messages);

        let (std_tx, std_rx) = std::sync::mpsc::sync_channel::<Option<String>>(64);
        let (tok_tx, mut tok_rx) = tokio::sync::mpsc::channel::<Option<String>>(64);

        // Start inference — sends tokens into std_tx on a detached thread
        self.manager.infer_streaming(&formatted, &stops, 0.7, 2048, std_tx);

        // Bridge: std::mpsc → tokio::mpsc (runs on blocking thread pool)
        tokio::task::spawn_blocking(move || {
            while let Ok(msg) = std_rx.recv() {
                let done = msg.is_none();
                // If receiver dropped (caller cancelled), stop the bridge
                if tok_tx.blocking_send(msg).is_err() { break; }
                if done { break; }
            }
        });

        let mut full_response = String::new();
        loop {
            match tok_rx.recv().await {
                Some(Some(piece)) => {
                    full_response.push_str(&piece);
                    on_chunk(StreamChunk { content: piece, done: false });
                }
                Some(None) | None => {
                    on_chunk(StreamChunk { content: String::new(), done: true });
                    break;
                }
            }
        }

        Ok(full_response)
    }

    /// Tool calling for local GGUF models via prompt injection + response parsing.
    /// Injects tool definitions into the system prompt and parses <tool_call> blocks from output.
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[crate::providers::ToolDefinition],
    ) -> anyhow::Result<crate::providers::ToolCallResult> {
        use crate::providers::ToolCallResult;

        if tools.is_empty() {
            let text = self.chat(messages).await?;
            return Ok(ToolCallResult::Text(text));
        }

        // Build tool definitions as compact JSON for the system prompt
        let tool_json: Vec<serde_json::Value> = tools.iter().map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters
            })
        }).collect();

        let tool_prompt = format!(
            r#"You have access to these tools:
{}

To call a tool, respond with EXACTLY this format (you can call multiple tools):
<tool_call>
{{"name": "tool_name", "arguments": {{"param": "value"}}}}
</tool_call>

If you don't need any tool, just respond with plain text (no <tool_call> tags).
IMPORTANT: Always use <tool_call> tags when you want to use a tool. Never describe the tool call in prose."#,
            serde_json::to_string_pretty(&tool_json).unwrap_or_default()
        );

        // Inject tool instructions into messages
        let mut augmented = messages.to_vec();
        if let Some(first) = augmented.first_mut() {
            if first.role == "system" {
                first.content = format!("{}

{}", first.content, tool_prompt);
            } else {
                augmented.insert(0, ChatMessage::system(&tool_prompt));
            }
        } else {
            augmented.insert(0, ChatMessage::system(&tool_prompt));
        }

        let response = self.chat(&augmented).await?;

        // Parse tool calls from response
        let tool_calls = parse_local_tool_calls(&response);

        if tool_calls.is_empty() {
            // No tool calls found — treat as plain text
            // Strip any accidental partial tags
            let clean = response
                .replace("<tool_call>", "")
                .replace("</tool_call>", "")
                .trim()
                .to_string();
            Ok(ToolCallResult::Text(clean))
        } else {
            Ok(ToolCallResult::ToolCalls(tool_calls))
        }
    }

    async fn is_available(&self) -> bool {
        self.model_path.exists()
    }
}

/// Parse <tool_call>...</tool_call> blocks from local model output.
fn parse_local_tool_calls(response: &str) -> Vec<crate::providers::ToolCall> {
    let mut calls = Vec::new();
    let mut remaining = response;

    while let Some(start) = remaining.find("<tool_call>") {
        let after_tag = &remaining[start + "<tool_call>".len()..];
        if let Some(end) = after_tag.find("</tool_call>") {
            let json_str = after_tag[..end].trim();
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_str) {
                let name = value["name"].as_str().unwrap_or("").to_string();
                let args = value.get("arguments")
                    .and_then(|a| a.as_object())
                    .map(|obj| {
                        obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| v.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();

                if !name.is_empty() {
                    calls.push(crate::providers::ToolCall {
                        id: uuid::Uuid::new_v4().to_string(),
                        name,
                        arguments: args,
                    });
                }
            }
            remaining = &after_tag[end + "</tool_call>".len()..];
        } else {
            break;
        }
    }

    calls
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_local_tool_calls_single() {
        let response = r#"I'll read the file for you.
<tool_call>
{"name": "read_psi", "arguments": {"target": "src/main.rs"}}
</tool_call>"#;
        let calls = parse_local_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_psi");
        assert_eq!(calls[0].arguments["target"], "src/main.rs");
    }

    #[test]
    fn test_parse_local_tool_calls_multiple() {
        let response = r#"Let me check both files.
<tool_call>
{"name": "read_psi", "arguments": {"target": "src/main.rs"}}
</tool_call>
And also:
<tool_call>
{"name": "read_psi", "arguments": {"target": "src/lib.rs"}}
</tool_call>"#;
        let calls = parse_local_tool_calls(response);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].arguments["target"], "src/main.rs");
        assert_eq!(calls[1].arguments["target"], "src/lib.rs");
    }

    #[test]
    fn test_parse_local_tool_calls_none() {
        let response = "I don't need any tools for this. The answer is 42.";
        let calls = parse_local_tool_calls(response);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_local_tool_calls_malformed_json() {
        let response = r#"<tool_call>
not valid json at all
</tool_call>"#;
        let calls = parse_local_tool_calls(response);
        assert!(calls.is_empty()); // gracefully handles bad JSON
    }

    #[test]
    fn test_parse_local_tool_calls_missing_name() {
        let response = r#"<tool_call>
{"arguments": {"target": "file.rs"}}
</tool_call>"#;
        let calls = parse_local_tool_calls(response);
        assert!(calls.is_empty()); // no name = skip
    }

    #[test]
    fn test_parse_local_tool_calls_unclosed_tag() {
        let response = r#"<tool_call>
{"name": "read_psi", "arguments": {"target": "file.rs"}}
"#; // no closing tag
        let calls = parse_local_tool_calls(response);
        assert!(calls.is_empty()); // gracefully handles missing end tag
    }

    #[test]
    fn test_parse_local_tool_calls_with_extra_whitespace() {
        let response = r#"
<tool_call>
  {"name": "list_files", "arguments": {}}
</tool_call>
"#;
        let calls = parse_local_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_files");
    }
}
