/// Chat template types matching different model families.
/// Port of ChatTemplateFormatter.kt TemplateType enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatTemplate {
    /// ChatML format: <|im_start|>role\ncontent<|im_end|>
    /// Used by: Qwen, Yi, DeepSeek
    ChatML,
    /// Llama 3 instruct format: <|start_header_id|>role<|end_header_id|>\ncontent<|eot_id|>
    Llama3,
    /// Phi-3 format: <|role|>\ncontent<|end|>
    Phi3,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

/// Format a list of messages into a prompt string using the given template.
/// Appends the assistant turn start so the model continues from there.
/// Port of ChatTemplateFormatter.format().
pub fn format_chat(template: ChatTemplate, messages: &[ChatMessage]) -> String {
    match template {
        ChatTemplate::ChatML => format_chatml(messages),
        ChatTemplate::Llama3 => format_llama3(messages),
        ChatTemplate::Phi3 => format_phi3(messages),
    }
}

/// Returns stop strings for the given template type.
/// Port of ChatTemplateFormatter.stopStrings().
pub fn get_stop_tokens(template: ChatTemplate) -> Vec<String> {
    match template {
        ChatTemplate::ChatML => vec!["<|im_end|>".into(), "<|im_start|>".into()],
        ChatTemplate::Llama3 => vec!["<|eot_id|>".into(), "<|start_header_id|>".into()],
        ChatTemplate::Phi3 => vec!["<|end|>".into(), "<|user|>".into()],
    }
}

fn format_chatml(messages: &[ChatMessage]) -> String {
    let mut s = String::new();
    for msg in messages {
        s.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", msg.role, msg.content));
    }
    s.push_str("<|im_start|>assistant\n");
    s
}

fn format_llama3(messages: &[ChatMessage]) -> String {
    let mut s = String::from("<|begin_of_text|>");
    for msg in messages {
        s.push_str(&format!(
            "<|start_header_id|>{}<|end_header_id|>\n\n{}<|eot_id|>",
            msg.role, msg.content
        ));
    }
    s.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
    s
}

fn format_phi3(messages: &[ChatMessage]) -> String {
    let mut s = String::new();
    for msg in messages {
        s.push_str(&format!("<|{}|>\n{}<|end|>\n", msg.role, msg.content));
    }
    s.push_str("<|assistant|>\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chatml_format() {
        let messages = vec![
            ChatMessage::new("system", "You are helpful."),
            ChatMessage::new("user", "Hello!"),
        ];
        let result = format_chat(ChatTemplate::ChatML, &messages);
        assert!(result.starts_with("<|im_start|>system\nYou are helpful.<|im_end|>\n"));
        assert!(result.contains("<|im_start|>user\nHello!<|im_end|>\n"));
        assert!(result.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn test_llama3_format() {
        let messages = vec![ChatMessage::new("user", "Hi")];
        let result = format_chat(ChatTemplate::Llama3, &messages);
        assert!(result.starts_with("<|begin_of_text|>"));
        assert!(result.contains("<|start_header_id|>user<|end_header_id|>\n\nHi<|eot_id|>"));
        assert!(result.ends_with("<|start_header_id|>assistant<|end_header_id|>\n\n"));
    }

    #[test]
    fn test_phi3_format() {
        let messages = vec![ChatMessage::new("user", "Hi")];
        let result = format_chat(ChatTemplate::Phi3, &messages);
        assert!(result.contains("<|user|>\nHi<|end|>\n"));
        assert!(result.ends_with("<|assistant|>\n"));
    }

    #[test]
    fn test_stop_tokens() {
        let stops = get_stop_tokens(ChatTemplate::ChatML);
        assert!(stops.contains(&"<|im_end|>".to_string()));
    }
}
