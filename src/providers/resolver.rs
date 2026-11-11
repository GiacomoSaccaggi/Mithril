//! Centralized model resolution, readiness filtering, and multi-provider fallback.
//!
//! Used by the Ollama-compatible HTTP API (`/api/tags`, `/api/chat`, `/api/generate`,
//! `/api/rerank`, `/api/embed`) to ensure seamless compatibility with clients like
//! ProjectCompass regardless of which local GGUF or cloud provider is configured.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{json, Value};

use crate::config::MithrilConfig;
use crate::engine::model_catalog::{find_model, ModelInfo, MODELS};
use crate::flow::fellowship::{try_list_fellowships, FellowshipConfig};
use crate::providers::{create_provider_with_model, ChatProvider};

/// Target resolved for a chat or generation request.
#[derive(Debug, Clone)]
pub enum ResolvedChatTarget {
    Local(&'static ModelInfo),
    Cloud {
        provider_name: &'static str,
        model_override: Option<String>,
    },
    Fellowship(FellowshipConfig),
}

/// Resolution error distinguishing unknown model names (404) from unavailable providers (500/503).
#[derive(Debug, Clone)]
pub enum ResolveError {
    /// Model name is completely unknown (maps to 404 Not Found).
    UnknownModel(String),
    /// Model is recognized, but neither it nor any fallback provider is ready (maps to 500/503).
    NoProviderAvailable(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::UnknownModel(m) => write!(
                f,
                "model not found: {}. Use a local model, cloud model, or fellowship name.",
                m
            ),
            ResolveError::NoProviderAvailable(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Default directory where local GGUF models are stored (`~/.mithril/models`).
pub fn default_model_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".mithril")
        .join("models")
}

/// Strip optional `:latest` tag suffix from a model identifier.
pub fn strip_tag_suffix(model: &str) -> &str {
    let trimmed = model.trim();
    if trimmed.len() > 7 && trimmed[trimmed.len() - 7..].eq_ignore_ascii_case(":latest") {
        &trimmed[..trimmed.len() - 7]
    } else {
        trimmed
    }
}

/// Check whether a local catalog model's GGUF file exists on disk and is non-empty.
pub fn is_local_model_downloaded(model_info: &ModelInfo, model_dir: &Path) -> bool {
    let file = model_dir.join(model_info.file_name);
    file.metadata().map(|md| md.len() > 0).unwrap_or(false)
}

/// Check whether a cloud provider has a non-empty API key configured.
pub fn is_cloud_provider_ready(provider_name: &str, config: &MithrilConfig) -> bool {
    config
        .get_credential(provider_name)
        .ok()
        .flatten()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
}

/// Detect if a model name refers to a cloud provider.
/// Supported families:
/// - Gemini: `gemini`, `gemini-*`, `models/gemini-*`
/// - OpenAI: `openai`, `gpt-*`, `o1*`, `o3*`, `o4*`, `chatgpt-*`
/// - Anthropic: `anthropic`, `claude-*`
/// - Groq: `groq`, `groq:*`, `meta-llama/*`, `mixtral-*`
pub fn detect_cloud_provider(model: &str) -> Option<&'static str> {
    let stripped = strip_tag_suffix(model);
    let m = stripped.to_lowercase();
    if m == "gemini" || m.starts_with("gemini-") || m.starts_with("models/gemini-") {
        return Some("gemini");
    }
    if m == "openai"
        || m.starts_with("gpt-")
        || m.starts_with("chatgpt-")
        || m == "o1"
        || m.starts_with("o1-")
        || m == "o3"
        || m.starts_with("o3-")
        || m == "o4"
        || m.starts_with("o4-")
    {
        return Some("openai");
    }
    if m == "anthropic" || m.starts_with("claude-") {
        return Some("anthropic");
    }
    if m == "groq"
        || m.starts_with("groq:")
        || m.starts_with("meta-llama/")
        || m.starts_with("mixtral-")
    {
        return Some("groq");
    }
    None
}

/// Extract the clean cloud model override (with `:latest` or provider prefix stripped),
/// or `None` if the caller passed a generic provider name like `"gemini"` or `"openai"`.
pub fn cloud_model_override(requested: &str, provider_name: &str) -> Option<String> {
    let stripped = strip_tag_suffix(requested);
    if stripped.eq_ignore_ascii_case(provider_name) {
        return None;
    }
    if provider_name == "groq" {
        if let Some(rest) = stripped.strip_prefix("groq:") {
            let clean = strip_tag_suffix(rest);
            return if clean.is_empty() {
                None
            } else {
                Some(clean.to_string())
            };
        }
    }
    if provider_name == "gemini" {
        if let Some(rest) = stripped.strip_prefix("models/") {
            return Some(rest.to_string());
        }
    }
    Some(stripped.to_string())
}

fn format_local_model_entry(m: &ModelInfo, size: u64) -> Value {
    json!({
        "name": m.ollama_tag(),
        "model": m.id,
        "size": size,
        "details": {
            "family": m.family,
            "parameter_size": m.parameter_size,
            "quantization_level": m.quantization,
            "format": "gguf"
        }
    })
}

/// Build the `/api/tags` model list.
///
/// Returns only ready models (downloaded local GGUF files with size > 0, cloud providers
/// with configured credentials, and fellowships). When zero models are ready (e.g. fresh
/// install before setup or unit tests), falls back to the static catalog list so `/api/tags`
/// is never empty.
pub fn list_ready_models(config: &MithrilConfig, model_dir: &Path) -> Vec<Value> {
    let mut ready_models: Vec<Value> = Vec::new();

    // 1. Downloaded local GGUF models
    for m in MODELS {
        let file = model_dir.join(m.file_name);
        let size = file.metadata().map(|md| md.len()).unwrap_or(0);
        if size > 0 {
            ready_models.push(format_local_model_entry(m, size));
        }
    }

    // 2. Cloud models with configured API keys
    let cloud = [
        ("gemini", &config.providers.gemini.model, "gemini"),
        ("openai", &config.providers.openai.model, "openai"),
        ("anthropic", &config.providers.anthropic.model, "anthropic"),
        ("groq", &config.providers.groq.model, "groq"),
    ];
    for (key, model_name, family) in cloud {
        if is_cloud_provider_ready(key, config) {
            let tag_name = if model_name.ends_with(":latest") {
                model_name.clone()
            } else {
                format!("{}:latest", model_name)
            };
            ready_models.push(json!({
                "name": tag_name,
                "model": model_name,
                "size": 0,
                "details": {
                    "family": family,
                    "parameter_size": "cloud",
                    "quantization_level": "none",
                    "format": "api"
                }
            }));
        }
    }

    // 3. Fellowship virtual models
    if let Ok(fellowships) = try_list_fellowships() {
        for (_name, f_cfg) in fellowships {
            let agent_count = f_cfg.agents.len();
            let desc = f_cfg.description.as_deref().unwrap_or("fellowship");
            ready_models.push(json!({
                "name": format!("{}:latest", f_cfg.name),
                "model": f_cfg.name,
                "size": 0,
                "details": {
                    "family": "mithril-fellowship",
                    "parameter_size": format!("{} agents", agent_count),
                    "quantization_level": "orchestrated",
                    "format": "fellowship"
                },
                "description": desc,
            }));
        }
    }

    // 4. Fallback: if nothing is ready yet, return catalog models with size 0
    if ready_models.is_empty() {
        return MODELS
            .iter()
            .map(|m| format_local_model_entry(m, 0))
            .collect();
    }

    ready_models
}

/// Find the best available fallback target when a known model is requested but not ready.
///
/// Priority:
/// 1. Configured `default_provider` if ready (cloud or downloaded local `default_model`)
/// 2. Any configured cloud provider (`gemini`, `openai`, `anthropic`, `groq`)
/// 3. Any downloaded local GGUF model in `MODELS`
pub fn find_fallback_chat_target(
    config: &MithrilConfig,
    model_dir: &Path,
) -> Option<ResolvedChatTarget> {
    // 1. Check default_provider
    match config.default_provider.as_str() {
        "gemini" if is_cloud_provider_ready("gemini", config) => {
            return Some(ResolvedChatTarget::Cloud {
                provider_name: "gemini",
                model_override: None,
            });
        }
        "openai" if is_cloud_provider_ready("openai", config) => {
            return Some(ResolvedChatTarget::Cloud {
                provider_name: "openai",
                model_override: None,
            });
        }
        "anthropic" if is_cloud_provider_ready("anthropic", config) => {
            return Some(ResolvedChatTarget::Cloud {
                provider_name: "anthropic",
                model_override: None,
            });
        }
        "groq" if is_cloud_provider_ready("groq", config) => {
            return Some(ResolvedChatTarget::Cloud {
                provider_name: "groq",
                model_override: None,
            });
        }
        "local" => {
            if let Some(m) = find_model(&config.default_model) {
                if is_local_model_downloaded(m, model_dir) {
                    return Some(ResolvedChatTarget::Local(m));
                }
            }
        }
        _ => {}
    }

    // 2. Any ready cloud provider
    for provider_name in ["gemini", "openai", "anthropic", "groq"] {
        if is_cloud_provider_ready(provider_name, config) {
            return Some(ResolvedChatTarget::Cloud {
                provider_name,
                model_override: None,
            });
        }
    }

    // 3. Any downloaded local GGUF model
    for m in MODELS {
        if is_local_model_downloaded(m, model_dir) {
            return Some(ResolvedChatTarget::Local(m));
        }
    }

    None
}

/// Resolve a chat/generate model request into a ready target or appropriate error.
///
/// - Completely unrecognized model names return `Err(ResolveError::UnknownModel)` (HTTP 404).
/// - Known local or cloud models that are ready are used directly (with `:latest` stripped).
/// - Known local or cloud models that are not ready automatically fall back via
///   `find_fallback_chat_target`, or return `Err(ResolveError::NoProviderAvailable)` if no
///   provider is ready at all.
pub fn resolve_chat_target(
    requested: &str,
    config: &MithrilConfig,
    model_dir: &Path,
) -> std::result::Result<ResolvedChatTarget, ResolveError> {
    let stripped = strip_tag_suffix(requested);

    // 1. Check fellowship virtual models
    if let Ok(fellowships) = try_list_fellowships() {
        let model_lower = stripped.to_lowercase();
        for (_key, fellowship_config) in &fellowships {
            if fellowship_config.name.to_lowercase() == model_lower {
                return Ok(ResolvedChatTarget::Fellowship(fellowship_config.clone()));
            }
        }
    }

    // 2. Check cloud provider models
    if let Some(provider_name) = detect_cloud_provider(stripped) {
        if is_cloud_provider_ready(provider_name, config) {
            return Ok(ResolvedChatTarget::Cloud {
                provider_name,
                model_override: cloud_model_override(stripped, provider_name),
            });
        }

        if let Some(fallback) = find_fallback_chat_target(config, model_dir) {
            tracing::info!(
                "Requested cloud model '{}' ({}) is not configured; falling back to {:?}",
                requested,
                provider_name,
                fallback
            );
            return Ok(fallback);
        }

        return Err(ResolveError::NoProviderAvailable(format!(
            "provider '{}' is not configured for model '{}' and no fallback provider is available",
            provider_name, requested
        )));
    }

    // 3. Check local catalog models
    if let Some(model_info) = find_model(stripped) {
        if is_local_model_downloaded(model_info, model_dir) {
            return Ok(ResolvedChatTarget::Local(model_info));
        }

        if let Some(fallback) = find_fallback_chat_target(config, model_dir) {
            tracing::info!(
                "Requested local model '{}' ({}) is not downloaded; falling back to {:?}",
                requested,
                model_info.id,
                fallback
            );
            return Ok(fallback);
        }

        return Err(ResolveError::NoProviderAvailable(format!(
            "Local model '{}' ({}) is not downloaded and no fallback provider is configured",
            requested, model_info.file_name
        )));
    }

    // 4. Truly unknown model name -> 404
    Err(ResolveError::UnknownModel(requested.to_string()))
}

/// Resolve a `ChatProvider` for `/api/rerank`, honoring `requested` when available and
/// falling back across configured cloud or downloaded local providers otherwise.
pub fn resolve_rerank_provider(
    requested: &str,
    config: &MithrilConfig,
    model_dir: &Path,
) -> Result<Box<dyn ChatProvider>> {
    let target = match resolve_chat_target(requested, config, model_dir) {
        Ok(t) => t,
        Err(ResolveError::UnknownModel(_)) => {
            find_fallback_chat_target(config, model_dir).ok_or_else(|| {
                anyhow::anyhow!(
                    "No provider available for reranking (requested model '{}')",
                    requested
                )
            })?
        }
        Err(ResolveError::NoProviderAvailable(msg)) => {
            anyhow::bail!("{}", msg);
        }
    };

    match target {
        ResolvedChatTarget::Cloud {
            provider_name,
            model_override,
        } => create_provider_with_model(provider_name, model_override.as_deref(), config),
        ResolvedChatTarget::Local(model_info) => {
            create_provider_with_model("local", Some(model_info.id), config)
        }
        ResolvedChatTarget::Fellowship(_) => {
            if let Some(fb) = find_fallback_chat_target(config, model_dir) {
                match fb {
                    ResolvedChatTarget::Cloud {
                        provider_name,
                        model_override,
                    } => create_provider_with_model(provider_name, model_override.as_deref(), config),
                    ResolvedChatTarget::Local(model_info) => {
                        create_provider_with_model("local", Some(model_info.id), config)
                    }
                    ResolvedChatTarget::Fellowship(_) => {
                        anyhow::bail!("Fellowship cannot be used directly as a rerank provider")
                    }
                }
            } else {
                anyhow::bail!("No provider available for reranking")
            }
        }
    }
}

/// Resolve an embedding provider for `/api/embed`, honoring the requested provider (`gemini`
/// or `openai`) if configured and falling back to the other configured embedding provider if not.
pub fn resolve_embed_provider(
    requested: &str,
    config: &MithrilConfig,
) -> Result<(Box<dyn ChatProvider>, String)> {
    let stripped = strip_tag_suffix(requested);
    let lower = stripped.to_lowercase();

    let (primary, secondary) =
        if lower.starts_with("text-embedding") || lower.starts_with("openai") {
            ("openai", "gemini")
        } else {
            ("gemini", "openai")
        };

    let chosen_provider = if is_cloud_provider_ready(primary, config) {
        primary
    } else if is_cloud_provider_ready(secondary, config) {
        tracing::info!(
            "Requested embedding provider '{}' for model '{}' is not configured; falling back to '{}'",
            primary,
            requested,
            secondary
        );
        secondary
    } else {
        anyhow::bail!(
            "No embedding provider configured (tried '{}' and '{}'). Set gemini or openai credentials.",
            primary,
            secondary
        );
    };

    let model_override = if chosen_provider == primary && !stripped.is_empty() {
        Some(stripped)
    } else {
        None
    };

    let provider = create_provider_with_model(chosen_provider, model_override, config)?;
    let effective_model = model_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| provider.model().to_string());

    Ok((provider, effective_model))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn empty_config() -> MithrilConfig {
        MithrilConfig::default()
    }

    #[test]
    fn test_strip_tag_suffix() {
        assert_eq!(strip_tag_suffix("qwen2:1.5b:latest"), "qwen2:1.5b");
        assert_eq!(strip_tag_suffix("gemini-2.5-flash:latest"), "gemini-2.5-flash");
        assert_eq!(strip_tag_suffix("gemini-2.5-flash:LATEST"), "gemini-2.5-flash");
        assert_eq!(strip_tag_suffix("qwen2:1.5b"), "qwen2:1.5b");
    }

    #[test]
    fn test_detect_cloud_provider_and_override() {
        assert_eq!(detect_cloud_provider("gemini-2.5-flash"), Some("gemini"));
        assert_eq!(detect_cloud_provider("gemini-2.5-flash:latest"), Some("gemini"));
        assert_eq!(detect_cloud_provider("gpt-4o-mini:latest"), Some("openai"));
        assert_eq!(detect_cloud_provider("claude-sonnet-4-20250514"), Some("anthropic"));
        assert_eq!(detect_cloud_provider("qwen2:1.5b"), None);

        assert_eq!(
            cloud_model_override("gemini-2.5-flash:latest", "gemini"),
            Some("gemini-2.5-flash".to_string())
        );
        assert_eq!(cloud_model_override("gemini", "gemini"), None);
    }

    #[test]
    fn test_list_ready_models_falls_back_when_none_ready() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = empty_config();
        // When no local models exist in temp_dir and no cloud credentials exist (assuming clean env)
        let models = list_ready_models(&config, temp_dir.path());
        assert!(!models.is_empty());
    }

    #[test]
    fn test_list_ready_models_filters_to_downloaded_gguf() {
        let temp_dir = tempfile::tempdir().unwrap();
        let qwen7b = find_model("qwen2:7b").unwrap();
        let fake_gguf = temp_dir.path().join(qwen7b.file_name);
        fs::write(&fake_gguf, b"fake-gguf-content").unwrap();

        let config = empty_config();
        let models = list_ready_models(&config, temp_dir.path());
        let local_models: Vec<&Value> = models
            .iter()
            .filter(|m| m["details"]["format"] == "gguf")
            .collect();
        assert_eq!(local_models.len(), 1);
        assert_eq!(local_models[0]["name"], "qwen2:7b");
        assert_eq!(local_models[0]["model"], "qwen-7b");
    }

    #[test]
    fn test_resolve_unknown_model_returns_unknown_error() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = empty_config();
        let res = resolve_chat_target("nonexistent-model-xyz", &config, temp_dir.path());
        assert!(matches!(res, Err(ResolveError::UnknownModel(_))));
    }

    #[test]
    fn test_resolve_known_local_model_falls_back_to_downloaded_gguf() {
        let temp_dir = tempfile::tempdir().unwrap();
        // Download qwen-7b, but request qwen2:1.5b
        let qwen7b = find_model("qwen2:7b").unwrap();
        fs::write(temp_dir.path().join(qwen7b.file_name), b"gguf-bytes").unwrap();

        let config = empty_config();
        let resolved = resolve_chat_target("qwen2:1.5b", &config, temp_dir.path()).unwrap();
        match resolved {
            ResolvedChatTarget::Local(m) => assert_eq!(m.id, "qwen-7b"),
            other => panic!("Expected Local(qwen-7b) or ready cloud fallback, got {:?}", other),
        }
    }
}
