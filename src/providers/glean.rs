//! Glean AI provider with cookie-based authentication and enterprise search.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use super::{ChatMessage, ChatProvider, StreamChunk, ToolCallResult, ToolDefinition};

pub struct GleanProvider {
    instance_url: String,
    cookies: String,
    client: reqwest::Client,
}

impl GleanProvider {
    pub fn new(instance_url: &str, cookies: &str) -> Self {
        Self {
            instance_url: instance_url.trim_end_matches('/').to_string(),
            cookies: cookies.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Search Glean's enterprise knowledge base.
    pub async fn search(&self, query: &str, page_size: usize) -> Result<Vec<GleanSearchResult>> {
        let url = format!("{}/api/v1/search", self.instance_url);

        let body = serde_json::json!({
            "query": query,
            "pageSize": page_size
        });

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Origin", "https://app.glean.com")
            .header("Referer", "https://app.glean.com/")
            .header("Cookie", &self.cookies)
            .json(&body)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(anyhow!(
                "Glean session expired. Run: mithril config set glean --browser"
            ));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Glean search API error ({}): {}", status, body));
        }

        let parsed: GleanSearchResponse = response.json().await?;
        Ok(parse_search_results(&parsed))
    }
}

/// Resolve the Glean instance URL from environment or desktop app config.
/// Priority: MITHRIL_GLEAN_INSTANCE env var → Glean desktop app config → None.
pub fn resolve_instance() -> Option<String> {
    std::env::var("MITHRIL_GLEAN_INSTANCE")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(detect_instance_url)
}

/// Auto-detect Glean instance URL from the desktop app configuration.
pub fn detect_instance_url() -> Option<String> {
    let config_path = get_glean_config_path()?;

    let content = std::fs::read_to_string(&config_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    let qe_base = json
        .get("state")?
        .get("app")?
        .get("auth")?
        .get("qeBase")?
        .as_str()?;

    Some(qe_base.trim_end_matches('/').to_string())
}

fn get_glean_config_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| h.join("Library/Application Support/Glean/glean.json"))
    }

    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .ok()
            .map(|appdata| std::path::PathBuf::from(appdata).join("Glean/glean.json"))
    }

    #[cfg(target_os = "linux")]
    {
        dirs::home_dir().map(|h| h.join(".config/Glean/glean.json"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

/// A search result from Glean.
#[derive(Debug, Clone)]
pub struct GleanSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub datasource: String,
}

// --- Request/Response types ---

#[derive(Serialize)]
struct GleanChatRequest {
    messages: Vec<GleanMessage>,
    stream: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct GleanMessage {
    author: String,
    fragments: Vec<GleanFragment>,
    #[serde(rename = "messageType", skip_serializing_if = "Option::is_none")]
    message_type: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct GleanFragment {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    citation: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct GleanChatResponse {
    messages: Vec<GleanMessage>,
}

#[derive(Deserialize)]
struct GleanStreamChunk {
    #[serde(rename = "messageType")]
    message_type: Option<String>,
    fragments: Option<Vec<GleanFragment>>,
}

#[derive(Deserialize)]
struct GleanSearchResponse {
    results: Option<Vec<GleanSearchResultRaw>>,
}

#[derive(Deserialize)]
struct GleanSearchResultRaw {
    title: Option<String>,
    url: Option<String>,
    snippets: Option<Vec<GleanSnippet>>,
    metadata: Option<GleanMetadata>,
}

#[derive(Deserialize)]
struct GleanSnippet {
    snippet: Option<String>,
}

#[derive(Deserialize)]
struct GleanMetadata {
    datasource: Option<String>,
}

// --- Conversion helpers ---

fn convert_messages(messages: &[ChatMessage]) -> Vec<GleanMessage> {
    messages
        .iter()
        .map(|m| {
            let author = match m.role.as_str() {
                "user" => "USER",
                "assistant" => "GLEAN_AI",
                "system" => "SYSTEM",
                _ => "USER",
            };
            GleanMessage {
                author: author.to_string(),
                fragments: vec![GleanFragment {
                    text: Some(m.content.clone()),
                    citation: None,
                }],
                message_type: None,
            }
        })
        .collect()
}

fn extract_text_from_messages(messages: &[GleanMessage]) -> String {
    let mut result = String::new();
    for msg in messages {
        for frag in &msg.fragments {
            if let Some(text) = &frag.text {
                result.push_str(text);
            }
        }
    }
    result
}

fn extract_text_from_fragments(fragments: &[GleanFragment]) -> String {
    let mut result = String::new();
    for frag in fragments {
        if let Some(text) = &frag.text {
            result.push_str(text);
        }
    }
    result
}

fn parse_search_results(response: &GleanSearchResponse) -> Vec<GleanSearchResult> {
    response
        .results
        .as_ref()
        .map(|results| {
            results
                .iter()
                .map(|r| GleanSearchResult {
                    title: r.title.clone().unwrap_or_default(),
                    url: r.url.clone().unwrap_or_default(),
                    snippet: r
                        .snippets
                        .as_ref()
                        .and_then(|s| s.first())
                        .and_then(|s| s.snippet.clone())
                        .unwrap_or_default(),
                    datasource: r
                        .metadata
                        .as_ref()
                        .and_then(|m| m.datasource.clone())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl ChatProvider for GleanProvider {
    fn name(&self) -> &str {
        "glean"
    }

    fn model(&self) -> &str {
        "default"
    }

    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        let url = format!("{}/api/v1/chat", self.instance_url);

        let request = GleanChatRequest {
            messages: convert_messages(messages),
            stream: false,
        };

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Origin", "https://app.glean.com")
            .header("Referer", "https://app.glean.com/")
            .header("Cookie", &self.cookies)
            .json(&request)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(anyhow!(
                "Glean session expired. Run: mithril config set glean --browser"
            ));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Glean chat API error ({}): {}", status, body));
        }

        let parsed: GleanChatResponse = response.json().await?;
        Ok(extract_text_from_messages(&parsed.messages))
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_chunk: Box<dyn Fn(StreamChunk) + Send>,
    ) -> Result<String> {
        let url = format!("{}/api/v1/chat", self.instance_url);

        let request = GleanChatRequest {
            messages: convert_messages(messages),
            stream: true,
        };

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Origin", "https://app.glean.com")
            .header("Referer", "https://app.glean.com/")
            .header("Cookie", &self.cookies)
            .json(&request)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(anyhow!(
                "Glean session expired. Run: mithril config set glean --browser"
            ));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Glean streaming chat API error ({}): {}",
                status,
                body
            ));
        }

        let mut full_text = String::new();
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            // Glean sends newline-delimited JSON (not SSE)
            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if let Ok(chunk_data) = serde_json::from_str::<GleanStreamChunk>(&line) {
                    // Only process CONTENT messages
                    if chunk_data.message_type.as_deref() == Some("CONTENT") {
                        if let Some(fragments) = &chunk_data.fragments {
                            let text = extract_text_from_fragments(fragments);
                            if !text.is_empty() {
                                full_text.push_str(&text);
                                on_chunk(StreamChunk {
                                    content: text,
                                    done: false,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Process any remaining content in buffer
        let line = buffer.trim();
        if !line.is_empty() {
            if let Ok(chunk_data) = serde_json::from_str::<GleanStreamChunk>(line) {
                if chunk_data.message_type.as_deref() == Some("CONTENT") {
                    if let Some(fragments) = &chunk_data.fragments {
                        let text = extract_text_from_fragments(fragments);
                        if !text.is_empty() {
                            full_text.push_str(&text);
                            on_chunk(StreamChunk {
                                content: text,
                                done: false,
                            });
                        }
                    }
                }
            }
        }

        on_chunk(StreamChunk {
            content: String::new(),
            done: true,
        });
        Ok(full_text)
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<ToolCallResult> {
        // Glean handles tools internally, just return plain text
        let text = self.chat(messages).await?;
        Ok(ToolCallResult::Text(text))
    }

    async fn is_available(&self) -> bool {
        !self.cookies.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_convert_messages() {
        let messages = vec![
            ChatMessage::system("You are helpful"),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there"),
        ];

        let converted = convert_messages(&messages);

        assert_eq!(converted.len(), 3);

        assert_eq!(converted[0].author, "SYSTEM");
        assert_eq!(converted[0].fragments[0].text, Some("You are helpful".to_string()));

        assert_eq!(converted[1].author, "USER");
        assert_eq!(converted[1].fragments[0].text, Some("Hello".to_string()));

        assert_eq!(converted[2].author, "GLEAN_AI");
        assert_eq!(converted[2].fragments[0].text, Some("Hi there".to_string()));
    }

    #[test]
    fn test_parse_chat_response() {
        let response_json = json!({
            "messages": [
                {
                    "author": "GLEAN_AI",
                    "fragments": [
                        {"text": "Hello, "},
                        {"text": "how can I help?"},
                        {"citation": {"url": "https://example.com"}}
                    ]
                }
            ]
        });

        let response: GleanChatResponse = serde_json::from_value(response_json).unwrap();
        let text = extract_text_from_messages(&response.messages);

        assert_eq!(text, "Hello, how can I help?");
    }

    #[test]
    fn test_parse_chat_response_multiple_messages() {
        let response_json = json!({
            "messages": [
                {
                    "author": "USER",
                    "fragments": [{"text": "Question"}]
                },
                {
                    "author": "GLEAN_AI",
                    "fragments": [
                        {"text": "Answer part 1"},
                        {"citation": {"id": "123"}},
                        {"text": " and part 2"}
                    ]
                }
            ]
        });

        let response: GleanChatResponse = serde_json::from_value(response_json).unwrap();
        let text = extract_text_from_messages(&response.messages);

        assert_eq!(text, "QuestionAnswer part 1 and part 2");
    }

    #[test]
    fn test_parse_streaming_response() {
        let lines = vec![
            r#"{"messageType": "SEARCH", "fragments": []}"#,
            r#"{"messageType": "CONTENT", "fragments": [{"text": "1"}]}"#,
            r#"{"messageType": "CONTENT", "fragments": [{"text": "\n2\n3"}]}"#,
            r#"{"messageType": "CITATION", "fragments": [{"citation": {"url": "test"}}]}"#,
            r#"{"messageType": "CONTENT", "fragments": [{"text": "\n4\n5"}]}"#,
        ];

        let mut collected_text = String::new();

        for line in lines {
            if let Ok(chunk) = serde_json::from_str::<GleanStreamChunk>(line) {
                if chunk.message_type.as_deref() == Some("CONTENT") {
                    if let Some(fragments) = &chunk.fragments {
                        let text = extract_text_from_fragments(fragments);
                        collected_text.push_str(&text);
                    }
                }
            }
        }

        assert_eq!(collected_text, "1\n2\n3\n4\n5");
    }

    #[test]
    fn test_parse_streaming_response_with_citations() {
        let line = r#"{"messageType": "CONTENT", "fragments": [{"text": "See "}, {"citation": {"title": "Doc"}}, {"text": "this doc"}]}"#;

        let chunk: GleanStreamChunk = serde_json::from_str(line).unwrap();
        assert_eq!(chunk.message_type, Some("CONTENT".to_string()));

        let text = extract_text_from_fragments(chunk.fragments.as_ref().unwrap());
        assert_eq!(text, "See this doc");
    }

    #[test]
    fn test_detect_instance_url() {
        use std::io::Write;

        let temp_dir = std::env::temp_dir().join("glean_test");
        let _ = std::fs::create_dir_all(&temp_dir);
        let config_path = temp_dir.join("glean.json");

        let config_content = json!({
            "state": {
                "app": {
                    "auth": {
                        "qeBase": "https://test-company.glean.com/"
                    }
                }
            }
        });

        let mut file = std::fs::File::create(&config_path).unwrap();
        write!(file, "{}", config_content).unwrap();

        // Read back and parse manually to verify the test data
        let content = std::fs::read_to_string(&config_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        let qe_base = json["state"]["app"]["auth"]["qeBase"]
            .as_str()
            .unwrap()
            .trim_end_matches('/');

        assert_eq!(qe_base, "https://test-company.glean.com");

        // Cleanup
        let _ = std::fs::remove_file(config_path);
        let _ = std::fs::remove_dir(temp_dir);
    }

    #[test]
    fn test_detect_instance_url_strips_trailing_slash() {
        let json: serde_json::Value = json!({
            "state": {
                "app": {
                    "auth": {
                        "qeBase": "https://company.glean.com///"
                    }
                }
            }
        });

        let qe_base = json["state"]["app"]["auth"]["qeBase"]
            .as_str()
            .unwrap()
            .trim_end_matches('/');

        assert_eq!(qe_base, "https://company.glean.com");
    }

    #[test]
    fn test_parse_search_response() {
        let response_json = json!({
            "results": [
                {
                    "title": "Document Title",
                    "url": "https://example.com/doc",
                    "snippets": [
                        {"snippet": "This is a snippet..."}
                    ],
                    "metadata": {
                        "datasource": "confluence"
                    }
                },
                {
                    "title": "Another Doc",
                    "url": "https://example.com/doc2",
                    "snippets": [
                        {"snippet": "Another snippet"}
                    ],
                    "metadata": {
                        "datasource": "gdrive"
                    }
                }
            ]
        });

        let response: GleanSearchResponse = serde_json::from_value(response_json).unwrap();
        let results = parse_search_results(&response);

        assert_eq!(results.len(), 2);

        assert_eq!(results[0].title, "Document Title");
        assert_eq!(results[0].url, "https://example.com/doc");
        assert_eq!(results[0].snippet, "This is a snippet...");
        assert_eq!(results[0].datasource, "confluence");

        assert_eq!(results[1].title, "Another Doc");
        assert_eq!(results[1].datasource, "gdrive");
    }

    #[test]
    fn test_parse_search_response_empty() {
        let response_json = json!({
            "results": []
        });

        let response: GleanSearchResponse = serde_json::from_value(response_json).unwrap();
        let results = parse_search_results(&response);

        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_search_response_missing_fields() {
        let response_json = json!({
            "results": [
                {
                    "title": "Partial Doc"
                }
            ]
        });

        let response: GleanSearchResponse = serde_json::from_value(response_json).unwrap();
        let results = parse_search_results(&response);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Partial Doc");
        assert_eq!(results[0].url, "");
        assert_eq!(results[0].snippet, "");
        assert_eq!(results[0].datasource, "");
    }

    #[test]
    fn test_glean_provider_new_strips_trailing_slash() {
        let provider = GleanProvider::new("https://test.glean.com/", "cookie=value");
        assert_eq!(provider.instance_url, "https://test.glean.com");
    }

    #[test]
    fn test_glean_provider_name_and_model() {
        let provider = GleanProvider::new("https://test.glean.com", "cookie=value");
        assert_eq!(provider.name(), "glean");
        assert_eq!(provider.model(), "default");
    }
}
