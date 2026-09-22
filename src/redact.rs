//! Secret Shield — credential masking before input leaves the machine.
//!
//! Detection strategies (in scan order):
//! 1. Known provider API key prefixes (16 providers)
//! 2. Key=value assignments (password=, token:, secret=)
//! 3. Natural language ("the password is X")
//! 4. JSON sensitive fields ("password": "value")
//! 5. HTTP headers (Authorization:, Cookie:, X-Api-Key:)
//! 6. Auth schemes (Bearer, Basic, OAuth)
//! 7. Connection string URIs (postgres://user:pass@host)
//! 8. JWT-shaped tokens (heuristic: base64 header contains "alg" or "enc")

use lazy_static::lazy_static;
use regex::Regex;

const REDACTED: &str = "[REDACTED]";

pub struct RedactResult {
    pub text: String,
    pub redacted_count: usize,
}

// ── 1. Known provider prefixes ──────────────────────────────────────────────

struct PrefixPattern {
    #[allow(dead_code)]
    name: &'static str,
    regex: Regex,
}

impl PrefixPattern {
    fn new(name: &'static str, pattern: &str) -> Self {
        Self { name, regex: Regex::new(pattern).unwrap() }
    }
}

lazy_static! {
    static ref PREFIX_PATTERNS: Vec<PrefixPattern> = vec![
        PrefixPattern::new("google_api",    r"AIza[A-Za-z0-9_-]{20,}"),
        PrefixPattern::new("google_oauth",  r"GOCSPX-[A-Za-z0-9_-]{10,}"),
        PrefixPattern::new("google_access", r"ya29\.[A-Za-z0-9._~-]{10,}"),
        PrefixPattern::new("sk_family",     r"sk-[A-Za-z0-9_-]{12,}"),
        PrefixPattern::new("groq",          r"gsk_[A-Za-z0-9_-]{12,}"),
        PrefixPattern::new("github",        r"(?:gh[pousr]_[A-Za-z0-9_]{12,}|github_pat_[A-Za-z0-9_]{12,})"),
        PrefixPattern::new("gitlab",        r"(?:glpat|glrt|gldt|glsoat|glcbt)-[A-Za-z0-9_-]{10,}"),
        PrefixPattern::new("aws_id",        r"(?:AKIA|ASIA)[A-Z0-9]{16}"),
        PrefixPattern::new("junie",         r"perm-[A-Za-z0-9_-]{12,}"),
        PrefixPattern::new("stripe",        r"[sr]k_(?:live|test)_[A-Za-z0-9]{8,}"),
        PrefixPattern::new("slack",         r"(?:xox[a-z]|xapp)-[A-Za-z0-9_-]{8,}"),
        PrefixPattern::new("sendgrid",      r"SG\.[A-Za-z0-9_-]{12,}\.[A-Za-z0-9_-]{12,}"),
        PrefixPattern::new("huggingface",   r"hf_[A-Za-z0-9]{12,}"),
        PrefixPattern::new("npm",           r"npm_[A-Za-z0-9]{12,}"),
        PrefixPattern::new("pypi",          r"pypi-[A-Za-z0-9_-]{12,}"),
        PrefixPattern::new("vault",         r"hv[sbr]\.[A-Za-z0-9_-]{12,}"),
    ];

    // 2. Key=value assignments
    static ref ASSIGN_PATTERN: Regex = Regex::new(
        r#"(?i)(password|passwd|pwd|passphrase|secret|token|credential|api_key|apikey|auth|private_key|access_key|secret_key|client_secret|connection_string|database_url|refresh_token|access_token)\s*[:=]\s*("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|\S+)"#
    ).unwrap();

    // 3. Natural language
    static ref NATURAL_PATTERN: Regex = Regex::new(
        r#"(?i)(password|passwd|pwd|secret|token)\s+(?:is|equals|è|est)\s+("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|\S+)"#
    ).unwrap();

    // 4. JSON sensitive fields
    static ref JSON_PATTERN: Regex = Regex::new(
        r#""(password|passwd|pwd|secret|token|api_key|apikey|client_secret|access_token|refresh_token|private_key|authorization)"\s*:\s*("(?:[^"\\]|\\.)*")"#
    ).unwrap();

    // 5. HTTP headers
    static ref HEADER_PATTERN: Regex = Regex::new(
        r"(?im)^[ \t]*(?:authorization|proxy-authorization|cookie|set-cookie|x-api-key)[ \t]*:[ \t]*([^\r\n]+)"
    ).unwrap();

    // 6. Auth schemes
    static ref AUTH_PATTERN: Regex = Regex::new(
        r"(?i)\b(?:Bearer|Basic|OAuth)\s+([A-Za-z0-9._+/=-]{8,})"
    ).unwrap();

    // 7. Connection string URIs
    static ref URI_PATTERN: Regex = Regex::new(
        r#"\b(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis(?:s)?|amqp(?:s)?)://([^/\s?#<>"']+)@"#
    ).unwrap();

    // 8. JWT-shaped tokens (boundary-checked in code since regex crate has no lookbehind)
    static ref JWT_PATTERN: Regex = Regex::new(
        r"([A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*)"
    ).unwrap();
}

/// Scan text for known secret patterns and mask them with [REDACTED].
pub fn redact_secrets(text: &str) -> RedactResult {
    let mut result = text.to_string();
    let mut count = 0usize;

    // 1. Known provider prefixes
    for pattern in PREFIX_PATTERNS.iter() {
        let new = pattern.regex.replace_all(&result, |_: &regex::Captures| {
            count += 1;
            REDACTED.to_string()
        });
        result = new.into_owned();
    }

    // 2. Key=value assignments
    result = replace_named_group(&ASSIGN_PATTERN, &result, 2, &mut count);

    // 3. Natural language
    result = replace_named_group(&NATURAL_PATTERN, &result, 2, &mut count);

    // 4. JSON sensitive fields — replace value with "[REDACTED]"
    {
        let mut new_result = String::new();
        let mut last_end = 0;
        for caps in JSON_PATTERN.captures_iter(&result) {
            let full = caps.get(0).unwrap();
            let value = match caps.get(2) {
                Some(v) => v,
                None => continue,
            };
            if value.as_str() == &format!("\"{}\"", REDACTED) {
                continue;
            }
            new_result.push_str(&result[last_end..value.start()]);
            new_result.push_str(&format!("\"{}\"", REDACTED));
            new_result.push_str(&result[value.end()..full.end()]);
            last_end = full.end();
            count += 1;
        }
        new_result.push_str(&result[last_end..]);
        result = new_result;
    }

    // 5. HTTP headers
    result = replace_named_group(&HEADER_PATTERN, &result, 1, &mut count);

    // 6. Auth schemes
    result = replace_named_group(&AUTH_PATTERN, &result, 1, &mut count);

    // 7. Connection string URIs
    result = replace_named_group(&URI_PATTERN, &result, 1, &mut count);

    // 8. JWT heuristic
    {
        let mut new_result = String::new();
        let mut last_end = 0;
        let bytes = result.as_bytes();
        for caps in JWT_PATTERN.captures_iter(&result) {
            let token_match = caps.get(1).unwrap();
            // Boundary check: char before must not be [A-Za-z0-9_-]
            if token_match.start() > 0 {
                let prev = bytes[token_match.start() - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'-' {
                    continue;
                }
            }
            // Boundary check: char after must not be [A-Za-z0-9_.-]
            if token_match.end() < bytes.len() {
                let next = bytes[token_match.end()];
                if next.is_ascii_alphanumeric() || next == b'_' || next == b'-' || next == b'.' {
                    continue;
                }
            }
            let token = token_match.as_str();
            let parts: Vec<&str> = token.split('.').collect();
            if parts.len() != 3 {
                continue;
            }
            if let Some(true) = is_jwt_header(parts[0]) {
                new_result.push_str(&result[last_end..token_match.start()]);
                new_result.push_str(REDACTED);
                last_end = token_match.end();
                count += 1;
            }
        }
        new_result.push_str(&result[last_end..]);
        result = new_result;
    }

    RedactResult { text: result, redacted_count: count }
}

/// Replace capture group `group_idx` with [REDACTED], skipping already-redacted values.
fn replace_named_group(pattern: &Regex, text: &str, group_idx: usize, count: &mut usize) -> String {
    let mut new_result = String::new();
    let mut last_end = 0;
    for caps in pattern.captures_iter(text) {
        let full = caps.get(0).unwrap();
        let value = match caps.get(group_idx) {
            Some(v) => v,
            None => continue,
        };
        if value.as_str().is_empty() || value.as_str() == REDACTED {
            continue;
        }
        new_result.push_str(&text[last_end..value.start()]);
        new_result.push_str(REDACTED);
        new_result.push_str(&text[value.end()..full.end()]);
        last_end = full.end();
        *count += 1;
    }
    new_result.push_str(&text[last_end..]);
    new_result
}

/// Check if a base64url-encoded string decodes to a JSON object with "alg" or "enc".
fn is_jwt_header(header_b64: &str) -> Option<bool> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let padded = match header_b64.len() % 4 {
        2 => format!("{}==", header_b64),
        3 => format!("{}=", header_b64),
        _ => header_b64.to_string(),
    };
    let replaced = padded.replace('-', "+").replace('_', "/");
    let raw = STANDARD.decode(replaced.as_bytes()).ok()?;
    let text = std::str::from_utf8(&raw).ok()?;
    let obj: serde_json::Value = serde_json::from_str(text).ok()?;
    if let Some(map) = obj.as_object() {
        Some(map.contains_key("alg") || map.contains_key("enc"))
    } else {
        Some(false)
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
            result_text = result_text.replace(secret.as_str(), REDACTED);
            count += 1;
        }
    }

    RedactResult { text: result_text, redacted_count: count }
}

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

    // ── 1. Provider prefixes ────────────────────────────────────────────

    #[test]
    fn test_google_api_key() {
        let r = redact_secrets("key is AIzaSyBiCp5vH5l0RaPwEVkHy2EocH6D546511k");
        assert_eq!(r.text, format!("key is {REDACTED}"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_google_oauth_key() {
        let r = redact_secrets("GOCSPX-abcdefghij1234567890");
        assert_eq!(r.text, REDACTED);
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_openai_key() {
        let r = redact_secrets("use sk-proj-abc123def456ghi789jkl012mno");
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("abc123def456"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_anthropic_key() {
        let r = redact_secrets("key: sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456");
        assert!(r.text.contains(REDACTED));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_groq_key() {
        let r = redact_secrets("GROQ=gsk_abcdefghij1234567890klmnop");
        assert!(r.text.contains(REDACTED));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_github_pat() {
        let r = redact_secrets("git clone https://ghp_abcdefghij1234567890kl@github.com/repo");
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("ghp_abcdefghij"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_gitlab_pat() {
        let r = redact_secrets("token glpat-abcdefghij1234567890-xy");
        assert!(r.text.contains(REDACTED));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_aws_key() {
        let r = redact_secrets("aws_access_key_id = AKIAIOSFODNN7EXAMPLE");
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("AKIAIOSFODNN7"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_stripe_key() {
        let r = redact_secrets("sk_live_abcdefgh12345678");
        assert_eq!(r.text, REDACTED);
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_slack_token() {
        let r = redact_secrets("xoxb-abcdefgh12345678");
        assert_eq!(r.text, REDACTED);
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_huggingface_token() {
        let r = redact_secrets("hf_abcdefghij123456");
        assert_eq!(r.text, REDACTED);
        assert_eq!(r.redacted_count, 1);
    }

    // ── 2. Key=value assignments ────────────────────────────────────────

    #[test]
    fn test_password_equals() {
        let r = redact_secrets("password=MyP@ssw0rd!");
        assert_eq!(r.text, format!("password={REDACTED}"));
        assert!(!r.text.contains("MyP@ssw0rd!"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_secret_colon() {
        let r = redact_secrets("secret: hunter2");
        assert_eq!(r.text, format!("secret: {REDACTED}"));
        assert!(!r.text.contains("hunter2"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_token_quoted() {
        let r = redact_secrets(r#"token = "my_secret_token_value""#);
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("my_secret_token_value"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_database_url() {
        let r = redact_secrets("database_url=postgres://admin:secret@db:5432/mydb");
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("secret"));
    }

    // ── 3. Natural language ─────────────────────────────────────────────

    #[test]
    fn test_password_is() {
        let r = redact_secrets("the password is SuperSecret123 and it works");
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("SuperSecret123"));
        assert_eq!(r.redacted_count, 1);
    }

    // ── 4. JSON sensitive fields ────────────────────────────────────────

    #[test]
    fn test_json_password_field() {
        let r = redact_secrets(r#"{"password": "s3cret", "name": "admin"}"#);
        assert!(r.text.contains(&format!("\"{}\"", REDACTED)));
        assert!(!r.text.contains("s3cret"));
        assert!(r.text.contains("admin"));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_json_api_key_field() {
        let r = redact_secrets(r#"{"api_key": "sk-abc123def456"}"#);
        // api_key value matches both JSON pattern and potentially prefix pattern
        assert!(r.text.contains(REDACTED));
        assert!(r.redacted_count >= 1);
    }

    // ── 5–6. Headers and auth schemes ───────────────────────────────────

    #[test]
    fn test_authorization_header() {
        let r = redact_secrets("Authorization: Bearer eyJtoken123456789.long.value");
        assert!(r.text.contains(REDACTED));
        assert!(r.redacted_count >= 1);
    }

    #[test]
    fn test_bearer_token() {
        let r = redact_secrets("use Bearer abc123def456.token.here");
        assert!(r.text.contains(REDACTED));
        assert!(r.redacted_count >= 1);
    }

    // ── 7. Connection strings ───────────────────────────────────────────

    #[test]
    fn test_postgres_connection_string() {
        let r = redact_secrets("DATABASE_URL=postgres://admin:s3cret_pass@db.example.com:5432/mydb");
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("s3cret_pass"));
        assert!(r.redacted_count >= 1);
    }

    #[test]
    fn test_mysql_connection_string() {
        let r = redact_secrets("mysql://root:password123@localhost:3306/db");
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("password123"));
    }

    #[test]
    fn test_mongodb_connection_string() {
        let r = redact_secrets("mongodb://user:p4ss@cluster.mongodb.net/db");
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("p4ss"));
    }

    #[test]
    fn test_redis_connection_string() {
        let r = redact_secrets("rediss://default:mytoken@redis.example.com:6380");
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("mytoken"));
    }

    // ── 8. JWT heuristic ────────────────────────────────────────────────

    #[test]
    fn test_jwt_token_detected() {
        // Real JWT header: {"alg":"HS256","typ":"JWT"} base64url-encoded
        let header = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
        let token = format!("{}.eyJzdWIiOiIxMjM0NTY3ODkwIn0.signature_here", header);
        let r = redact_secrets(&format!("token: {}", token));
        assert!(r.text.contains(REDACTED));
        assert_eq!(r.redacted_count, 1);
    }

    #[test]
    fn test_non_jwt_dotted_string_not_redacted() {
        // version-like strings should NOT be detected as JWT
        let r = redact_secrets("use package com.example.app version 1.2.3");
        assert_eq!(r.redacted_count, 0);
    }

    // ── Multiple secrets ────────────────────────────────────────────────

    #[test]
    fn test_multiple_secrets() {
        let r = redact_secrets("key1=AIzaSyBiCp5vH5l0RaPwEVkHy2EocH6D546511k and password=hunter2 done");
        assert!(r.text.contains(REDACTED));
        assert!(!r.text.contains("hunter2"));
        assert!(r.redacted_count >= 2);
    }

    // ── Safe passthrough ────────────────────────────────────────────────

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

    // ── LLM helpers ─────────────────────────────────────────────────────

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
        let arr = parse_json_array(r#"Here are the secrets: ["password123", "mytoken"] in your text."#);
        assert_eq!(arr, Some(vec!["password123".to_string(), "mytoken".to_string()]));
    }

    #[test]
    fn test_parse_json_array_malformed() {
        let arr = parse_json_array("I didn't find any secrets.");
        assert_eq!(arr, None);
    }
}
