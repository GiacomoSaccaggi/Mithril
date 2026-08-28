use crate::engine::chat_template::ChatTemplate;

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub file_name: &'static str,
    pub download_url: &'static str,
    pub family: &'static str,
    pub parameter_size: &'static str,
    pub quantization: &'static str,
    pub chat_template: ChatTemplate,
}

pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "qwen-1.5b",
        display_name: "Qwen 2.5 Coder 1.5B (Fast, ~1.2GB)",
        file_name: "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf",
        download_url: "https://huggingface.co/Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF/resolve/main/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf",
        family: "qwen2",
        parameter_size: "1.5B",
        quantization: "Q4_K_M",
        chat_template: ChatTemplate::ChatML,
    },
    ModelInfo {
        id: "qwen-7b",
        display_name: "Qwen 2.5 Coder 7B (Powerful, ~4.5GB)",
        file_name: "qwen2.5-coder-7b-instruct-q4_k_m.gguf",
        download_url: "https://huggingface.co/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF/resolve/main/qwen2.5-coder-7b-instruct-q4_k_m.gguf",
        family: "qwen2",
        parameter_size: "7B",
        quantization: "Q4_K_M",
        chat_template: ChatTemplate::ChatML,
    },
    ModelInfo {
        id: "qwen-14b",
        display_name: "Qwen 2.5 Coder 14B (Best local coder, ~9GB)",
        file_name: "qwen2.5-coder-14b-instruct-q4_k_m.gguf",
        download_url: "https://huggingface.co/Qwen/Qwen2.5-Coder-14B-Instruct-GGUF/resolve/main/qwen2.5-coder-14b-instruct-q4_k_m.gguf",
        family: "qwen2",
        parameter_size: "14B",
        quantization: "Q4_K_M",
        chat_template: ChatTemplate::ChatML,
    },
    ModelInfo {
        id: "llama-8b",
        display_name: "Llama 3.1 8B Instruct (All-rounder, ~5GB)",
        file_name: "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
        download_url: "https://huggingface.co/bartowski/Meta-Llama-3.1-8B-Instruct-GGUF/resolve/main/Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
        family: "llama",
        parameter_size: "8B",
        quantization: "Q4_K_M",
        chat_template: ChatTemplate::Llama3,
    },
    ModelInfo {
        id: "deepseek-6.7b",
        display_name: "DeepSeek Coder 6.7B (Expert Coder, ~4.5GB)",
        file_name: "deepseek-coder-6.7b-instruct.Q4_K_M.gguf",
        download_url: "https://huggingface.co/TheBloke/deepseek-coder-6.7B-instruct-GGUF/resolve/main/deepseek-coder-6.7b-instruct.Q4_K_M.gguf",
        family: "deepseek",
        parameter_size: "6.7B",
        quantization: "Q4_K_M",
        chat_template: ChatTemplate::ChatML, // DeepSeek uses ChatML per ChatTemplateFormatter.kt
    },
    ModelInfo {
        id: "phi-3.5",
        display_name: "Phi-3.5 Mini 3.8B (Lightweight, ~2.5GB)",
        file_name: "Phi-3.5-mini-instruct-Q4_K_M.gguf",
        download_url: "https://huggingface.co/bartowski/Phi-3.5-mini-instruct-GGUF/resolve/main/Phi-3.5-mini-instruct-Q4_K_M.gguf",
        family: "phi3",
        parameter_size: "3.8B",
        quantization: "Q4_K_M",
        chat_template: ChatTemplate::Phi3,
    },
];

/// Find a model by ID, with normalization for Ollama-style names.
pub fn find_model(id: &str) -> Option<&'static ModelInfo> {
    // Try exact match first
    if let Some(m) = MODELS.iter().find(|m| m.id == id) {
        return Some(m);
    }

    // Normalize Ollama-style names
    let lower = id.to_lowercase();
    let normalized = normalize_ollama_name(&lower);
    MODELS.iter().find(|m| m.id == normalized)
}

fn normalize_ollama_name(name: &str) -> &'static str {
    match name {
        "qwen2.5-coder:1.5b" | "qwen2.5-coder-1.5b" | "qwen:1.5b" => "qwen-1.5b",
        "qwen2.5-coder:7b" | "qwen2.5-coder-7b" | "qwen:7b" => "qwen-7b",
        "llama3.1:8b" | "llama3.1-8b" | "llama:8b" | "llama3:8b" => "llama-8b",
        "deepseek-coder:6.7b" | "deepseek-coder-6.7b" => "deepseek-6.7b",
        "phi3.5" | "phi3.5:3.8b" | "phi-3.5:3.8b" | "phi3:3.8b" => "phi-3.5",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_by_exact_id() {
        assert!(find_model("qwen-1.5b").is_some());
        assert!(find_model("qwen-7b").is_some());
        assert!(find_model("llama-8b").is_some());
        assert!(find_model("deepseek-6.7b").is_some());
        assert!(find_model("phi-3.5").is_some());
    }

    #[test]
    fn test_find_by_ollama_name() {
        let m1 = find_model("qwen2.5-coder:1.5b");
        let m2 = find_model("qwen-1.5b");
        assert!(m1.is_some());
        assert_eq!(m1.unwrap().id, m2.unwrap().id);
    }

    #[test]
    fn test_unknown_model() {
        assert!(find_model("nonexistent-model").is_none());
    }
}
