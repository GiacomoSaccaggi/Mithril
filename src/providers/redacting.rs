//! Redacting provider wrapper — masks secrets in messages before forwarding to cloud providers.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

use super::{ChatMessage, ChatProvider, StreamChunk, ToolCallResult, ToolDefinition};
use crate::redact;

pub struct RedactingProvider {
    inner: Box<dyn ChatProvider>,
    llm_checker: Option<Arc<dyn ChatProvider>>,
}

impl RedactingProvider {
    pub fn new(inner: Box<dyn ChatProvider>, llm_checker: Option<Arc<dyn ChatProvider>>) -> Box<dyn ChatProvider> {
        Box::new(Self { inner, llm_checker })
    }

    async fn llm_redact_user_messages(&self, messages: &mut Vec<ChatMessage>) -> usize {
        let checker = match &self.llm_checker {
            Some(c) => c,
            None => return 0,
        };
        let mut total = 0;
        for msg in messages.iter_mut() {
            if msg.role == "user" {
                let r = redact::redact_with_llm(&msg.content, checker.as_ref()).await;
                total += r.redacted_count;
                msg.content = r.text;
            }
        }
        total
    }
}

fn redact_messages(messages: &[ChatMessage]) -> (Vec<ChatMessage>, usize) {
    let mut total = 0;
    let redacted = messages.iter().map(|m| {
        let r = redact::redact_secrets(&m.content);
        total += r.redacted_count;
        ChatMessage {
            role: m.role.clone(),
            content: r.text,
        }
    }).collect();
    (redacted, total)
}

fn log_redaction(count: usize) {
    if count > 0 {
        tracing::warn!(
            "⚠ {} credential{} redacted from input before sending to cloud provider",
            count,
            if count == 1 { "" } else { "s" }
        );
    }
}

fn log_redaction_llm(count: usize) {
    if count > 0 {
        tracing::warn!(
            "🔒 LLM detected {} additional credential{} missed by regex",
            count,
            if count == 1 { "" } else { "s" }
        );
    }
}

#[async_trait]
impl ChatProvider for RedactingProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn model(&self) -> &str {
        self.inner.model()
    }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let (mut redacted, count) = redact_messages(messages);
        log_redaction(count);
        let llm_count = self.llm_redact_user_messages(&mut redacted).await;
        log_redaction_llm(llm_count);
        self.inner.chat(&redacted).await
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        let (mut redacted, count) = redact_messages(messages);
        log_redaction(count);
        let llm_count = self.llm_redact_user_messages(&mut redacted).await;
        log_redaction_llm(llm_count);
        self.inner.chat_stream(&redacted, on_chunk).await
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ToolCallResult> {
        let (mut redacted, count) = redact_messages(messages);
        log_redaction(count);
        let llm_count = self.llm_redact_user_messages(&mut redacted).await;
        log_redaction_llm(llm_count);
        self.inner.chat_with_tools(&redacted, tools).await
    }

    async fn is_available(&self) -> bool {
        self.inner.is_available().await
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut total = 0;
        let redacted: Vec<String> = texts.iter().map(|t| {
            let r = redact::redact_secrets(t);
            total += r.redacted_count;
            r.text
        }).collect();
        log_redaction(total);
        self.inner.embed(&redacted).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Mock provider that records messages it receives.
    struct RecordingProvider {
        received: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingProvider {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let received = Arc::new(Mutex::new(Vec::new()));
            (Self { received: received.clone() }, received)
        }
    }

    #[async_trait]
    impl ChatProvider for RecordingProvider {
        fn name(&self) -> &str { "mock" }
        fn model(&self) -> &str { "mock-model" }

        async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
            for m in messages {
                self.received.lock().unwrap().push(m.content.clone());
            }
            Ok("ok".to_string())
        }

        async fn chat_stream(
            &self,
            messages: &[ChatMessage],
            _on_chunk: Box<dyn Fn(StreamChunk) + Send>,
        ) -> Result<String> {
            self.chat(messages).await
        }

        async fn is_available(&self) -> bool { true }
    }

    #[tokio::test]
    async fn test_redacting_provider_masks_api_key() {
        let (mock, received) = RecordingProvider::new();
        let provider = RedactingProvider::new(Box::new(mock), None);

        provider.chat(&[
            ChatMessage::user("my key is AIzaSyBiCp5vH5l0RaPwEVkHy2EocH6D546511k"),
        ]).await.unwrap();

        let msgs = received.lock().unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].contains("[REDACTED]"));
        assert!(!msgs[0].contains("AIzaSyBiCp5vH5l0"));
    }

    #[tokio::test]
    async fn test_redacting_provider_passthrough_clean_text() {
        let (mock, received) = RecordingProvider::new();
        let provider = RedactingProvider::new(Box::new(mock), None);

        provider.chat(&[
            ChatMessage::user("please review this code"),
        ]).await.unwrap();

        let msgs = received.lock().unwrap();
        assert_eq!(msgs[0], "please review this code");
    }

    #[tokio::test]
    async fn test_redacting_provider_name_passthrough() {
        let (mock, _) = RecordingProvider::new();
        let provider = RedactingProvider::new(Box::new(mock), None);
        assert_eq!(provider.name(), "mock");
        assert_eq!(provider.model(), "mock-model");
    }
}
