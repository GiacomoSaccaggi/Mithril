//! Input redactor — masks credentials and secrets before they leave the machine.
//!
//! Scans text for known secret patterns (API keys, passwords, tokens, connection strings)
//! and replaces them with masked versions that preserve identifiability (prefix + suffix).

use lazy_static::lazy_static;
use regex::Regex;

pub struct RedactResult {
    pub text: String,
    pub redacted_count: usize,
}

lazy_static! {
    static ref PATTERNS: Vec<SecretPattern> = vec![
        // API keys with known prefixes
        SecretPattern::new("gemini_key",  r"AIza[A-Za-z0-9_-]{30,}"),
        SecretPattern::new("openai_key",  r"sk-[A-Za-z0-9_-]{20,}"),
        SecretPattern::new("anthropic_key", r"sk-ant-[A-Za-z0-9_-]{20,}"),
        SecretPattern::new("groq_key",    r"gsk_[A-Za-z0-9]{20,}"),
        SecretPattern::new("github_pat",  r"ghp_[A-Za-z0-9]{20,}"),
        SecretPattern::new("gitlab_pat",  r"glpat-[A-Za-z0-9_-]{20,}"),
        SecretPattern::new("aws_key",     r"AKIA[A-Z0-9]{16}"),
        // Bearer tokens
        SecretPattern::new("bearer",      r"Bearer\s+[A-Za-z0-9._-]{20,}"),
        // Connection strings with embedded credentials
        SecretPattern::new("conn_string", r"(postgres|mysql|mongodb|redis)://[^@\s]+:[^@\s]+@"),
        // JWT-like tokens
        SecretPattern::new("jwt",         r"eyJ[A-Za-z0-9_-]{50,}"),
    ];

    static ref PASSWORD_PATTERN: Regex = Regex::new(
        r#"(?i)(password|passwd|pwd|secret|token|api_key|apikey)\s*[:=]\s*["']?(\S+?)["']?(?:\s|$)|(?i)(password|passwd|pwd|secret|token)\s+is\s+["']?(\S+?)["']?(?:\s|$)"#
    ).unwrap();
}

struct SecretPattern {
    #[allow(dead_code)]
    name: &'static str,
    regex: Regex,
}

impl SecretPattern {
    fn new(name: &'static str, pattern: &str) -> Self {
        Self {
            name,
            regex: Regex::new(pattern).unwrap(),
        }
    }
}

/// Mask a secret value: show first 4 chars + **** + last 3 chars.
/// For short values (< 10 chars), just show first 2 + **** + last 1.
fn mask_value(value: &str) -> String {
    let len = value.len();
    if len <= 6 {
        return "****".to_string();
    }
    if len < 10 {
        format!("{}****{}", &value[..2], &value[len - 1..])
    } else {
        format!("{}****{}", &value[..4], &value[len - 3..])
    }
}

/// Scan text for known secret patterns and mask them.
pub fn redact_secrets(text: &str) -> RedactResult {
    let mut result = text.to_string();
    let mut count = 0usize;

    // Pass 1: known prefix patterns (API keys, bearer, JWT, conn strings)
    for pattern in PATTERNS.iter() {
        let mut new_result = String::new();
        let mut last_end = 0;
        for m in pattern.regex.find_iter(&result) {
            new_result.push_str(&result[last_end..m.start()]);
            new_result.push_str(&mask_value(m.as_str()));
            last_end = m.end();
            count += 1;
        }
        new_result.push_str(&result[last_end..]);
        result = new_result;
    }

    // Pass 2: password/secret/token in context (key=value, key: value, or "password is value")
    let mut new_result = String::new();
    let mut last_end = 0;
    for caps in PASSWORD_PATTERN.captures_iter(&result) {
        let full = caps.get(0).unwrap();
        // Group 2 = value from "key[:=]value", Group 4 = value from "key is value"
        let value = caps.get(2).or_else(|| caps.get(4));
        let value = match value {
            Some(v) => v,
            None => continue,
        };

        // Don't double-mask already masked values
        if value.as_str().contains("****") {
            continue;
        }

        new_result.push_str(&result[last_end..full.start()]);
        // Keep everything up to the value, then mask the value
        new_result.push_str(&result[full.start()..value.start()]);
        new_result.push_str("****");
        new_result.push_str(&result[value.end()..full.end()]);
        last_end = full.end();
        count += 1;
    }
    new_result.push_str(&result[last_end..]);
    result = new_result;

    RedactResult {
        text: result,
        redacted_count: count,
    }
}

/// Redact secrets using an LLM for fuzzy detection.
/// Returns the original text unchanged if the LLM doesn't respond or parse fails (fail-open).
pub async fn redact_with_llm(
    text: &str,
    provider: &dyn crate::providers::ChatProvider,
) -> RedactResult {
    use crate::providers::ChatMessage;

    const PROMPT: &str = r#"Analyze this text and identify any credentials, passwords, API keys, tokens, or secrets.
Reply ONLY with a JSON array of the exact secret values found. Example: ["password123", "sk-abc123"]
If no secrets found, reply: []
Do not explain. Only output the JSON array.

Text to analyze:
"#;

    let messages = vec![
        ChatMessage::user(&format!("{}{}", PROMPT, text)),
    ];

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        provider.chat(&messages),
    ).await;

    let response = match result {
        Ok(Ok(resp)) => resp,
        _ => return RedactResult { text: text.to_string(), redacted_count: 0 },
    };

    let secrets: Vec<String> = match parse_json_array(&response) {
        Some(arr) => arr,
        None => return RedactResult { text: text.to_string(), redacted_count: 0 },
    };

    if secrets.is_empty() {
        return RedactResult { text: text.to_string(), redacted_count: 0 };
    }

    let mut result_text = text.to_string();
    let mut count = 0;
    for secret in &secrets {
        if secret.len() >= 4 && result_text.contains(secret.as_str()) {
            result_text = result_text.replace(secret.as_str(), "****");
            count += 1;
            let prefix = &secret[..2.min(secret.len())];
            let suffix = &secret[secret.len().saturating_sub(2)..];
            tracing::warn!("🔒 LLM detected credential: {}...{}", prefix, suffix);
        }
    }

    RedactResult { text: result_text, redacted_count: count }
}

/// Try to parse a JSON string array from LLM response.
/// Handles cases where the LLM wraps the array in explanation text.
fn parse_json_array(response: &str) -> Option<Vec<String>> {
    if let Ok(arr) = serde_json::from_str::<Vec<String>>(response) {
        return Some(arr);
    }
    let start = response.find('[')?;
    let end = response.rfind(']')?;
    if start < end {
        serde_json::from_str(&response[start..=end]).ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_secrets_passthrough() {
        let r = redact_secrets("Hello, please review this code for bugs.");
        assert_eq!(r.text, "Hello, please review this code for bugs.");
        assert_eq!(r.redacted_count, 0);
    }

    #[test]
    fn test_gemini_api_key() {
        let r = redact_secrets("my key is AIzaSyBiCp5vH5l0RaPwEVkHy2EocH6D546511k");
        assert!(r.text.contains("AIza****11k"));
        assert!(!r.text.contains("AIzaSyBiCp5vH5l0"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_openai_key() {
        let r = redact_secrets("use sk-proj-abc123def456ghi789jkl012mno");
        assert!(r.text.contains("****"));
        assert!(!r.text.contains("abc123def456"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_anthropic_key() {
        let r = redact_secrets("key: sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456");
        assert!(r.text.contains("sk-a****"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_groq_key() {
        let r = redact_secrets("GROQ=gsk_abcdefghij1234567890klmnop");
        assert!(r.text.contains("gsk_****"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_github_pat() {
        let r = redact_secrets("git clone https://ghp_abcdefghij1234567890kl@github.com/repo");
        assert!(r.text.contains("ghp_****"));
        assert!(!r.text.contains("ghp_abcdefghij"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_gitlab_pat() {
        let r = redact_secrets("token glpat-abcdefghij1234567890-xy");
        assert!(r.text.contains("glpa****"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_aws_key() {
        let r = redact_secrets("aws_access_key_id = AKIAIOSFODNN7EXAMPLE");
        assert!(r.text.contains("AKIA****PLE"));
        assert!(!r.text.contains("AKIAIOSFODNN7"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_bearer_token() {
        let r = redact_secrets("Authorization: Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6.long.token.here");
        assert!(r.text.contains("Bear****"));
        assert!(!r.text.contains("eyJhbGciOiJ"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_connection_string() {
        let r = redact_secrets("DATABASE_URL=postgres://admin:s3cret_pass@db.example.com:5432/mydb");
        assert!(r.text.contains("****"));
        assert!(!r.text.contains("s3cret_pass"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_jwt_token() {
        let input = format!("token: eyJ{}", "a".repeat(60));
        let r = redact_secrets(&input);
        assert!(r.text.contains("****"));
        assert!(!r.text.contains(&"a".repeat(30)));
        assert!(r.redacted_count >= 1);
    }

    #[test]
    fn test_password_in_context() {
        let r = redact_secrets("the password is SuperSecret123 and it works");
        assert!(r.text.contains("password"));
        assert!(r.text.contains("****"));
        assert!(!r.text.contains("SuperSecret123"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_password_equals() {
        let r = redact_secrets("password=MyP@ssw0rd!");
        assert!(r.text.contains("password=****"));
        assert!(!r.text.contains("MyP@ssw0rd!"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_password_colon() {
        let r = redact_secrets("secret: hunter2");
        assert!(r.text.contains("secret: ****"));
        assert!(!r.text.contains("hunter2"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_password_quoted() {
        let r = redact_secrets(r#"token = "my_secret_token_value""#);
        assert!(r.text.contains("****"));
        assert!(!r.text.contains("my_secret_token_value"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_multiple_secrets() {
        let r = redact_secrets("key1=AIzaSyBiCp5vH5l0RaPwEVkHy2EocH6D546511k and password=hunter2 done");
        assert!(r.text.contains("AIza****"));
        assert!(r.text.contains("****"));
        assert!(!r.text.contains("hunter2"));
        assert!(r.redacted_count >= 2);
    }

    #[test]
    fn test_secret_at_start() {
        let r = redact_secrets("AIzaSyBiCp5vH5l0RaPwEVkHy2EocH6D546511k is the key");
        assert!(r.text.starts_with("AIza****"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_secret_at_end() {
        let r = redact_secrets("the key is AIzaSyBiCp5vH5l0RaPwEVkHy2EocH6D546511k");
        assert!(r.text.ends_with("AIza****11k"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_mask_short_value() {
        assert_eq!(mask_value("abc"), "****");
        assert_eq!(mask_value("abcdef"), "****");
    }

    #[test]
    fn test_mask_medium_value() {
        let m = mask_value("abcdefghi"); // 9 chars
        assert_eq!(m, "ab****i");
    }

    #[test]
    fn test_mask_long_value() {
        let m = mask_value("abcdefghijklmno"); // 15 chars
        assert_eq!(m, "abcd****mno");
    }

    #[test]
    fn test_mysql_connection_string() {
        let r = redact_secrets("mysql://root:password123@localhost:3306/db");
        assert!(r.text.contains("****"));
        assert!(!r.text.contains("password123"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_mongodb_connection_string() {
        let r = redact_secrets("mongodb://user:p4ss@cluster.mongodb.net/db");
        assert!(r.text.contains("****"));
        assert!(!r.text.contains("p4ss"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_normal_code_not_redacted() {
        let code = r#"fn main() {
    let x = 42;
    println!("hello world {}", x);
}"#;
        let r = redact_secrets(code);
        assert_eq!(r.text, code);
        assert_eq!(r.redacted_count, 0);
    }

    #[test]
    fn test_parse_json_array_direct() {
        let arr = parse_json_array(r#"["secret1", "secret2"]"#);
        assert_eq!(arr, Some(vec!["secret1".to_string(), "secret2".to_string()]));
    }

    #[test]
    fn test_parse_json_array_empty() {
        let arr = parse_json_array("[]");
        assert_eq!(arr, Some(vec![]));
    }

    #[test]
    fn test_parse_json_array_with_explanation() {
        let arr = parse_json_array(r#"Here are the secrets I found: ["password123", "mytoken"] in your text."#);
        assert_eq!(arr, Some(vec!["password123".to_string(), "mytoken".to_string()]));
    }

    #[test]
    fn test_parse_json_array_malformed() {
        let arr = parse_json_array("I didn't find any secrets in this text.");
        assert_eq!(arr, None);
    }

    #[test]
    fn test_parse_json_array_no_closing_bracket() {
        let arr = parse_json_array(r#"["incomplete"#);
        assert_eq!(arr, None);
    }
}
