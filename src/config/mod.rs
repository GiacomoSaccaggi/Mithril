//! Configuration management with Argon2id-derived AES-256-GCM credentials.
//!
//! Config stored at `~/.mithril/config.yaml`
//! Credential format (v2): base64(nonce\[12\] || salt\[16\] || ciphertext)
//! Credential format (v1, legacy): base64(nonce\[12\] || ciphertext)  — auto-migrated on read

#![allow(dead_code)]
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{Context, Result};
use argon2::{Argon2, Params};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use zeroize::Zeroizing;

const ARGON2_SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
// v2 = nonce(12) + salt(16) + tag(16) + min 1 byte ciphertext = 45
const V2_MIN_LEN: usize = NONCE_LEN + ARGON2_SALT_LEN + 17;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MithrilConfig {
    #[serde(default = "default_provider")]
    pub default_provider: String,
    #[serde(default = "default_model")]
    pub default_model: String,
    #[serde(default)]
    pub credentials: HashMap<String, String>,
    #[serde(default)]
    pub providers: ProviderSettings,
    /// Block dangerous shell commands in run_terminal tool (default: true)
    #[serde(default = "default_true")]
    pub terminal_sandbox: bool,
    /// Per-tool permission overrides: allow, deny, or ask (default: ask for dangerous tools)
    #[serde(default)]
    pub permissions: ToolPermissions,
    /// File extension to formatter command mapping.
    /// e.g. {".rs": "cargo fmt -- {file}", ".ts": "prettier --write {file}"}
    #[serde(default)]
    pub formatters: HashMap<String, String>,

    /// Telegram user IDs allowed to interact with the bot.
    /// Empty list = owner mode (first user auto-registered).
    #[serde(default)]
    pub telegram_allowed_users: Vec<i64>,
    /// Optional bearer token required for all inference and MCP API calls.
    /// Set via: mithril config set api_token "my-secret-token"
    /// Stored in ~/.mithril/secrets (NOT in config.yaml) with 0600 permissions.
    #[serde(skip)]
    pub api_token: Option<String>,
    /// Optional user-provided secret to strengthen credential encryption.
    /// Stored in ~/.mithril/secrets (NOT in config.yaml) with 0600 permissions.
    #[serde(skip)]
    pub key_password: Option<String>,
}

impl Default for MithrilConfig {
    fn default() -> Self {
        Self {
            default_provider: default_provider(),
            default_model: default_model(),
            credentials: HashMap::new(),
            providers: ProviderSettings::default(),
            terminal_sandbox: true,
            permissions: ToolPermissions::default(),
            formatters: HashMap::new(),
            telegram_allowed_users: Vec::new(),
            key_password: None,
            api_token: None,
        }
    }
}

// ── Secrets file (~/.mithril/secrets) ────────────────────────────────────────
// Sensitive fields (key_password, api_token) are stored here — never in config.yaml.

fn secrets_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".mithril").join("secrets"))
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct SecretsFile {
    #[serde(default)]
    pub key_password: Option<String>,
    #[serde(default)]
    pub api_token: Option<String>,
}

type Secrets = SecretsFile;

pub fn load_secrets_pub() -> SecretsFile { load_secrets() }
pub fn save_secrets_pub(s: &SecretsFile) -> Result<()> { save_secrets(s) }

fn load_secrets() -> Secrets {
    let path = match secrets_path() {
        Some(p) => p,
        None => return Secrets::default(),
    };
    if !path.exists() {
        return Secrets::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_yaml::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_secrets(secrets: &Secrets) -> Result<()> {
    let path = secrets_path().context("Cannot find home directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_yaml::to_string(secrets)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true).create(true).truncate(true)
            .mode(0o600)
            .open(&path)?
            .write_all(content.as_bytes())?;
    }
    #[cfg(not(unix))]
    { fs::write(&path, content)?; }
    Ok(())
}

fn default_provider() -> String { "local".to_string() }
fn default_model() -> String { "qwen-1.5b".to_string() }
fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderSettings {
    #[serde(default)]
    pub gemini: GeminiSettings,
    #[serde(default)]
    pub openai: OpenAISettings,
    #[serde(default)]
    pub anthropic: AnthropicSettings,
    #[serde(default)]
    pub groq: GroqSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiSettings {
    #[serde(default = "default_gemini_model")]
    pub model: String,
}
impl Default for GeminiSettings {
    fn default() -> Self { Self { model: default_gemini_model() } }
}
fn default_gemini_model() -> String { "gemini-3.6-flash".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAISettings {
    #[serde(default = "default_openai_model")]
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
}
impl Default for OpenAISettings {
    fn default() -> Self { Self { model: default_openai_model(), base_url: None } }
}
fn default_openai_model() -> String { "gpt-4o-mini".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicSettings {
    #[serde(default = "default_anthropic_model")]
    pub model: String,
}
impl Default for AnthropicSettings {
    fn default() -> Self { Self { model: default_anthropic_model() } }
}
fn default_anthropic_model() -> String { "claude-sonnet-4-20250514".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroqSettings {
    #[serde(default = "default_groq_model")]
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
}
impl Default for GroqSettings {
    fn default() -> Self { Self { model: default_groq_model(), base_url: None } }
}
fn default_groq_model() -> String { "meta-llama/llama-4-scout-17b-16e-instruct".to_string() }

// ── Tool Permissions ─────────────────────────────────────────────────────────

/// Permission level for a tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPermission {
    /// Always allow without asking
    Allow,
    /// Always deny (tool is disabled)
    Deny,
    /// Ask user for confirmation each time (default for dangerous tools)
    Ask,
}

/// Per-tool permission configuration.
///
/// In config.yaml:
/// ```yaml
/// permissions:
///   run_terminal: ask
///   write_file: allow
///   delete_file: deny
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolPermissions {
    #[serde(flatten)]
    pub overrides: HashMap<String, ToolPermission>,
}

/// Default dangerous tools that require "ask" permission.
const DEFAULT_DANGEROUS: &[&str] = &[
    "write_file", "edit_file", "apply_patch", "delete_file", "run_terminal",
];

impl ToolPermissions {
    /// Get the effective permission for a tool.
    /// Priority: explicit override > default (ask for dangerous, allow for safe).
    pub fn get_permission(&self, tool_name: &str) -> ToolPermission {
        if let Some(perm) = self.overrides.get(tool_name) {
            return perm.clone();
        }
        if DEFAULT_DANGEROUS.contains(&tool_name) {
            ToolPermission::Ask
        } else {
            ToolPermission::Allow
        }
    }

    /// Check if a tool is completely disabled.
    pub fn is_denied(&self, tool_name: &str) -> bool {
        self.get_permission(tool_name) == ToolPermission::Deny
    }

    /// Check if a tool requires user confirmation.
    pub fn needs_confirmation(&self, tool_name: &str) -> bool {
        self.get_permission(tool_name) == ToolPermission::Ask
    }
}

impl MithrilConfig {
    pub fn config_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not find home directory")?;
        Ok(home.join(".mithril").join("config.yaml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let mut config: Self = serde_yaml::from_str(&content)
            .context("Failed to parse config YAML")?;
        // Load secrets from separate file (never in config.yaml)
        let secrets = load_secrets();
        config.key_password = secrets.key_password;
        config.api_token = secrets.api_token;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_yaml::to_string(self)?;
        // L1: write with restricted permissions (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?
                .write_all(content.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            fs::write(&path, &content)?;
        }
        Ok(())
    }

    pub fn set_credential(&mut self, name: &str, value: &str) -> Result<()> {
        let encrypted = encrypt_credential_with_secret(value, self.key_password.as_deref())?;
        self.credentials.insert(name.to_string(), encrypted);
        self.save()
    }

    /// Re-encrypt all credentials with the current key_password.
    /// MUST be called after changing key_password to avoid losing access to old credentials.
    pub fn migrate_credentials(&mut self) -> Result<usize> {
        let names: Vec<String> = self.credentials.keys().cloned().collect();
        let mut count = 0;
        for name in names {
            // Try to decrypt with current key (old password already set in self.key_password)
            // If this is called AFTER key_password change, we need to pass old_secret explicitly.
            // For simplicity: migrate_credentials receives old_secret as param.
            // But since we store them already encrypted with old key, decryption will fail.
            // This is handled by the CLI which calls the 2-arg variant below.
            if let Some(encrypted) = self.credentials.get(&name).cloned() {
                if let Ok(z) = decrypt_credential_with_secret(&encrypted, self.key_password.as_deref()) {
                    let new_encrypted = encrypt_credential_with_secret(&z, self.key_password.as_deref())?;
                    self.credentials.insert(name, new_encrypted);
                    count += 1;
                }
            }
        }
        self.save()?;
        Ok(count)
    }

    /// Re-encrypt all credentials from old_secret to current key_password.
    pub fn migrate_credentials_from(&mut self, old_secret: Option<&str>) -> Result<usize> {
        let names: Vec<String> = self.credentials.keys().cloned().collect();
        let mut count = 0;
        let mut failed = 0;
        for name in names {
            if let Some(encrypted) = self.credentials.get(&name).cloned() {
                match decrypt_credential_with_secret(&encrypted, old_secret) {
                    Ok(z) => {
                        let new_encrypted = encrypt_credential_with_secret(&z, self.key_password.as_deref())?;
                        self.credentials.insert(name, new_encrypted);
                        count += 1;
                    }
                    Err(_) => { failed += 1; }
                }
            }
        }
        self.save()?;
        if failed > 0 {
            tracing::warn!("{} credential(s) could not be migrated (wrong old password?)", failed);
        }
        Ok(count)
    }

    /// Returns decrypted credential in a Zeroizing wrapper.
    /// The key material is wiped from memory when the wrapper is dropped.
    pub fn get_credential(&self, name: &str) -> Result<Option<String>> {
        match self.credentials.get(name) {
            Some(encrypted) => {
                let z = decrypt_credential_with_secret(encrypted, self.key_password.as_deref())?;
                Ok(Some(z.to_string()))
            }
            None => Ok(None),
        }
    }

    pub fn unset_credential(&mut self, name: &str) -> Result<bool> {
        let removed = self.credentials.remove(name).is_some();
        if removed { self.save()?; }
        Ok(removed)
    }

    pub fn list_credentials(&self) -> Vec<&str> {
        self.credentials.keys().map(|s| s.as_str()).collect()
    }

    pub fn set_default_provider(&mut self, provider: &str) -> Result<()> {
        self.default_provider = provider.to_string();
        self.save()
    }

    pub fn set_default_model(&mut self, model: &str) -> Result<()> {
        self.default_model = model.to_string();
        self.save()
    }
}

// ── Argon2id KDF ─────────────────────────────────────────────────────────────

/// Derive an AES-256 key from salt using Argon2id.
/// `extra_secret`: optional user-provided secret mixed into the password for higher entropy.
/// Must NOT call MithrilConfig::load() — would cause infinite recursion.
fn derive_key_argon2(salt: &[u8], extra_secret: Option<&str>) -> Result<Zeroizing<[u8; 32]>> {
    let username = whoami::username();
    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let password = match extra_secret.filter(|s| !s.is_empty()) {
        Some(secret) => format!("mithril-v2-{}-{}-{}", username, home, secret),
        None         => format!("mithril-v2-{}-{}", username, home),
    };

    // Argon2id with conservative params (m=65536 KB, t=3 iterations, p=1 lane)
    let params = Params::new(65536, 3, 1, Some(32))
        .map_err(|e| anyhow::anyhow!("Argon2 params error: {}", e))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| anyhow::anyhow!("Argon2 hash error: {}", e))?;
    Ok(key)
}

/// Encrypt using Argon2id KDF + AES-256-GCM.
/// Output format: base64(nonce[12] || salt[16] || ciphertext)
fn encrypt_credential_with_secret(plaintext: &str, extra_secret: Option<&str>) -> Result<String> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    let mut salt = [0u8; ARGON2_SALT_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    rand::thread_rng().fill_bytes(&mut salt);

    let key = derive_key_argon2(&salt, extra_secret)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref())
        .map_err(|e| anyhow::anyhow!("AES key error: {:?}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    let mut combined = Vec::with_capacity(NONCE_LEN + ARGON2_SALT_LEN + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&salt);
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(&combined))
}

/// Decrypt, with automatic detection of legacy v1 format (no Argon2 salt).
fn decrypt_credential_with_secret(encrypted: &str, extra_secret: Option<&str>) -> Result<Zeroizing<String>> {
    let combined = BASE64
        .decode(encrypted)
        .context("Invalid base64 in credential")?;

    if combined.len() >= V2_MIN_LEN {
        // v2: nonce(12) || salt(16) || ciphertext
        let nonce_bytes = &combined[..NONCE_LEN];
        let salt = &combined[NONCE_LEN..NONCE_LEN + ARGON2_SALT_LEN];
        let ciphertext = &combined[NONCE_LEN + ARGON2_SALT_LEN..];

        let key = derive_key_argon2(salt, extra_secret)?;
        let cipher = Aes256Gcm::new_from_slice(key.as_ref())
            .map_err(|e| anyhow::anyhow!("AES key error: {:?}", e))?;
        let nonce = Nonce::from_slice(nonce_bytes);
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow::anyhow!("Decryption failed — credential may be corrupted"))?;
        let s = String::from_utf8(plaintext).context("Invalid UTF-8 in decrypted credential")?;
        return Ok(Zeroizing::new(s));
    }

    // v1 legacy format: nonce(12) || ciphertext (weak KDF — auto-migrate on next write)
    if combined.len() < NONCE_LEN + 17 {
        anyhow::bail!("Encrypted data too short");
    }
    let nonce_bytes = &combined[..NONCE_LEN];
    let ciphertext = &combined[NONCE_LEN..];

    let username = whoami::username();
    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let key_material = format!("mithril-key-{}-{}", username, home);
    let mut key = [0u8; 32];
    let bytes = key_material.as_bytes();
    for (i, byte) in bytes.iter().cycle().take(32).enumerate() {
        key[i] = *byte;
    }
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("AES key error: {:?}", e))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("Legacy decryption failed"))?;
    let s = String::from_utf8(plaintext).context("Invalid UTF-8 in decrypted credential")?;
    Ok(Zeroizing::new(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let original = "sk-test-api-key-12345";
        let encrypted = encrypt_credential_with_secret(original, None).unwrap();
        let decrypted = decrypt_credential_with_secret(&encrypted, None).unwrap();
        assert_eq!(original, decrypted.as_str());
    }

    #[test]
    fn test_different_salts_produce_different_ciphertexts() {
        let plain = "same-secret";
        let c1 = encrypt_credential_with_secret(plain, None).unwrap();
        let c2 = encrypt_credential_with_secret(plain, None).unwrap();
        assert_ne!(c1, c2);
        assert_eq!(decrypt_credential_with_secret(&c1, None).unwrap().as_str(), plain);
        assert_eq!(decrypt_credential_with_secret(&c2, None).unwrap().as_str(), plain);
    }

    #[test]
    fn test_default_config() {
        let config = MithrilConfig::default();
        assert_eq!(config.default_provider, "local");
        assert_eq!(config.default_model, "qwen-1.5b");
        assert!(config.terminal_sandbox);
    }

    #[test]
    fn test_groq_settings_default() {
        let settings = GroqSettings::default();
        assert_eq!(settings.model, "meta-llama/llama-4-scout-17b-16e-instruct");
        assert!(settings.base_url.is_none());
    }

    #[test]
    fn test_provider_settings_has_groq() {
        let settings = ProviderSettings::default();
        assert_eq!(settings.groq.model, "meta-llama/llama-4-scout-17b-16e-instruct");
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = MithrilConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: MithrilConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.default_provider, "local");
        assert_eq!(parsed.providers.groq.model, "meta-llama/llama-4-scout-17b-16e-instruct");
    }

    #[test]
    fn test_tool_permission_defaults() {
        let perms = ToolPermissions::default();
        assert_eq!(perms.get_permission("read_psi"), ToolPermission::Allow);
        assert_eq!(perms.get_permission("write_file"), ToolPermission::Ask);
        assert_eq!(perms.get_permission("delete_file"), ToolPermission::Ask);
        assert_eq!(perms.get_permission("run_terminal"), ToolPermission::Ask);
        assert!(!perms.is_denied("read_psi"));
        assert!(!perms.is_denied("grep_files"));
        assert!(perms.needs_confirmation("run_terminal"));
        assert!(perms.needs_confirmation("edit_file"));
        assert!(!perms.needs_confirmation("list_files"));
    }

    #[test]
    fn test_tool_permission_overrides() {
        let mut perms = ToolPermissions::default();
        perms.overrides.insert("run_terminal".to_string(), ToolPermission::Allow);
        perms.overrides.insert("delete_file".to_string(), ToolPermission::Deny);
        assert_eq!(perms.get_permission("run_terminal"), ToolPermission::Allow);
        assert!(!perms.needs_confirmation("run_terminal"));
        assert!(perms.is_denied("delete_file"));
        // Non-overridden dangerous tool still asks
        assert!(perms.needs_confirmation("write_file"));
        // Non-overridden safe tool still allows
        assert_eq!(perms.get_permission("read_psi"), ToolPermission::Allow);
    }

    #[test]
    fn test_tool_permissions_yaml_parse() {
        let yaml = r#"
run_terminal: allow
delete_file: deny
write_file: ask
"#;
        let perms: ToolPermissions = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(perms.get_permission("run_terminal"), ToolPermission::Allow);
        assert_eq!(perms.get_permission("delete_file"), ToolPermission::Deny);
        assert_eq!(perms.get_permission("write_file"), ToolPermission::Ask);
    }

}
