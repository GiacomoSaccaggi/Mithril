//! Conversation compaction — summarize long histories to free context window.
//!
//! When triggered (via /compact or automatically), the current conversation
//! is summarized into a structured checkpoint preserving:
//! - Task status (what's done, what's next)
//! - Key decisions made
//! - File paths referenced
//! - Current working state

use anyhow::Result;
use crate::providers::{ChatMessage, ChatProvider};

const COMPACT_PROMPT: &str = r#"Summarize this conversation into a structured checkpoint. Preserve:
1. **Task status** — what was accomplished, what remains
2. **Key decisions** — architectural choices, approaches taken
3. **Files touched** — paths that were read, modified, or created
4. **Current state** — where things stand right now
5. **Next steps** — what should happen next

Be concise but complete. Use bullet points. This summary will replace the full history."#;

/// Compact a conversation history by asking the provider to summarize it.
/// Returns the summary text, or an error if the provider call fails.
pub async fn compact_history(
    provider: &dyn ChatProvider,
    messages: &[ChatMessage],
) -> Result<String> {
    if messages.len() < 4 {
        anyhow::bail!("Not enough messages to compact (need at least 4)");
    }

    // Build a request that includes the full history + compaction instruction
    let mut compact_messages = Vec::with_capacity(messages.len() + 1);

    // Include all existing messages as context
    compact_messages.extend_from_slice(messages);

    // Add the compaction request
    compact_messages.push(ChatMessage::user(COMPACT_PROMPT));

    // Ask the provider to generate the summary
    let summary = provider.chat(&compact_messages).await?;

    Ok(summary)
}

/// Replace the current history with a compacted version.
/// Keeps the original system message (if any) and replaces everything else
/// with a single system message containing the summary.
pub fn apply_compaction(messages: &mut Vec<ChatMessage>, summary: &str) {
    let original_count = messages.len();

    // Preserve the first system message (steering/system prompt)
    let system_msg = if messages.first().map(|m| m.role == "system").unwrap_or(false) {
        Some(messages[0].clone())
    } else {
        None
    };

    messages.clear();

    if let Some(sys) = system_msg {
        messages.push(sys);
    }

    messages.push(ChatMessage::system(format!(
        "[Conversation compacted — summary of previous {} messages]\n\n{}",
        original_count,
        summary
    )));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_compaction_preserves_system_message() {
        let mut messages = vec![
            ChatMessage::system("steering context"),
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi there"),
            ChatMessage::user("do something"),
            ChatMessage::assistant("done"),
        ];
        apply_compaction(&mut messages, "Summary: talked about stuff");
        // Should have system (original) + system (summary)
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[0].content, "steering context");
        assert!(messages[1].content.contains("Summary: talked about stuff"));
    }

    #[test]
    fn test_apply_compaction_without_system_message() {
        let mut messages = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi"),
            ChatMessage::user("bye"),
            ChatMessage::assistant("goodbye"),
        ];
        apply_compaction(&mut messages, "Convo summary");
        assert_eq!(messages.len(), 1);
        assert!(messages[0].content.contains("Convo summary"));
    }

    #[test]
    fn test_apply_compaction_includes_original_count() {
        let mut messages = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("1"),
            ChatMessage::assistant("2"),
            ChatMessage::user("3"),
            ChatMessage::assistant("4"),
            ChatMessage::user("5"),
        ];
        apply_compaction(&mut messages, "summary");
        assert!(messages[1].content.contains("6 messages"));
    }

    #[test]
    fn test_compact_prompt_contains_key_sections() {
        assert!(COMPACT_PROMPT.contains("Task status"));
        assert!(COMPACT_PROMPT.contains("Key decisions"));
        assert!(COMPACT_PROMPT.contains("Files touched"));
        assert!(COMPACT_PROMPT.contains("Current state"));
        assert!(COMPACT_PROMPT.contains("Next steps"));
    }

    #[test]
    fn test_compact_prompt_mentions_bullet_points() {
        assert!(COMPACT_PROMPT.contains("bullet points"));
    }

    #[test]
    fn test_compact_prompt_mentions_replacement() {
        assert!(COMPACT_PROMPT.contains("replace the full history"));
    }

    #[test]
    fn test_apply_compaction_clears_all_messages() {
        let mut messages = vec![
            ChatMessage::user("1"),
            ChatMessage::assistant("2"),
            ChatMessage::user("3"),
        ];
        let original_len = messages.len();
        apply_compaction(&mut messages, "summary");

        // Should have only 1 message (the summary)
        assert_eq!(messages.len(), 1);
        // Summary should mention original count
        assert!(messages[0].content.contains(&format!("{}", original_len)));
    }

    #[test]
    fn test_apply_compaction_summary_is_system_role() {
        let mut messages = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("world"),
        ];
        apply_compaction(&mut messages, "test summary");
        assert_eq!(messages[0].role, "system");
    }

    #[test]
    fn test_apply_compaction_empty_summary() {
        let mut messages = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("world"),
        ];
        apply_compaction(&mut messages, "");
        assert_eq!(messages.len(), 1);
        assert!(messages[0].content.contains("2 messages"));
    }

    #[test]
    fn test_apply_compaction_long_summary() {
        let mut messages = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("world"),
        ];
        let long_summary = "x".repeat(10000);
        apply_compaction(&mut messages, &long_summary);
        assert!(messages[0].content.len() > 10000);
    }

    #[test]
    fn test_apply_compaction_preserves_only_first_system() {
        let mut messages = vec![
            ChatMessage::system("first system"),
            ChatMessage::system("second system should be cleared"),
            ChatMessage::user("hello"),
            ChatMessage::assistant("world"),
        ];
        apply_compaction(&mut messages, "summary");

        // Should have original system + summary system
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "first system");
        assert!(messages[1].content.contains("summary"));
    }

    #[test]
    fn test_apply_compaction_multiline_summary() {
        let mut messages = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("world"),
        ];
        let summary = "Line 1\nLine 2\nLine 3";
        apply_compaction(&mut messages, summary);
        assert!(messages[0].content.contains("Line 1"));
        assert!(messages[0].content.contains("Line 2"));
        assert!(messages[0].content.contains("Line 3"));
    }

    #[test]
    fn test_apply_compaction_special_characters() {
        let mut messages = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("world"),
        ];
        let summary = "Summary with *special* **chars** `code` and <html>";
        apply_compaction(&mut messages, summary);
        assert!(messages[0].content.contains("*special*"));
        assert!(messages[0].content.contains("<html>"));
    }
}
