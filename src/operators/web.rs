use regex::Regex;

/// Port of DuckDuckGoSearchOperator.kt
#[derive(Clone)]
pub struct WebOperator {
    client: reqwest::Client,
}

impl WebOperator {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mithril/0.1")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub async fn search(&self, query: &str) -> String {
        let encoded = urlencoding::encode(query);
        let url = format!(
            "https://api.duckduckgo.com/?q={encoded}&format=json&no_html=1&skip_disambig=1"
        );

        let resp = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => return format!("Error: web search failed — {e}"),
        };

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => return format!("Error: failed to read response — {e}"),
        };

        self.parse_ddg_response(&body)
    }

    pub async fn fetch_page(&self, url: &str) -> String {
        let resp = match self.client.get(url).send().await {
            Ok(r) => r,
            Err(e) => return format!("Error: fetch failed — {e}"),
        };

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => return format!("Error: failed to read page — {e}"),
        };

        let stripped = strip_html(&body);
        if stripped.len() > 4000 {
            stripped[..4000].to_string()
        } else {
            stripped
        }
    }

    fn parse_ddg_response(&self, json: &str) -> String {
        let Ok(obj) = serde_json::from_str::<serde_json::Value>(json) else {
            return "Error: could not parse search response".to_string();
        };

        let mut result = String::new();

        if let Some(abstract_text) = obj["AbstractText"].as_str() {
            if !abstract_text.is_empty() {
                result.push_str(&format!("Summary: {abstract_text}\n"));
            }
        }

        if let Some(abstract_url) = obj["AbstractURL"].as_str() {
            if !abstract_url.is_empty() {
                result.push_str(&format!("Source: {abstract_url}\n"));
            }
        }

        if let Some(topics) = obj["RelatedTopics"].as_array() {
            let mut count = 0;
            for topic in topics {
                if count >= 5 {
                    break;
                }
                let text = topic["Text"].as_str().unwrap_or("");
                let url = topic["FirstURL"].as_str().unwrap_or("");
                if text.is_empty() {
                    continue;
                }
                if url.is_empty() {
                    result.push_str(&format!("- {text}\n"));
                } else {
                    result.push_str(&format!("- {text} ({url})\n"));
                }
                count += 1;
            }
        }

        if result.is_empty() {
            "No results found for the query.".to_string()
        } else {
            result.trim().to_string()
        }
    }
}

impl Default for WebOperator {
    fn default() -> Self {
        Self::new()
    }
}

fn strip_html(html: &str) -> String {
    // Remove <style> and <script> blocks
    let style_re = Regex::new(r"(?i)<style[^>]*>[\s\S]*?</style>").unwrap();
    let script_re = Regex::new(r"(?i)<script[^>]*>[\s\S]*?</script>").unwrap();
    let tag_re = Regex::new(r"<[^>]+>").unwrap();
    let ws_re = Regex::new(r"\s{2,}").unwrap();

    let s = style_re.replace_all(html, "");
    let s = script_re.replace_all(&s, "");
    let s = tag_re.replace_all(&s, " ");
    ws_re.replace_all(&s, " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_operator_new() {
        let _op = WebOperator::new();
        // Should not panic - client is created successfully
    }

    #[test]
    fn test_web_operator_default() {
        let _op = WebOperator::default();
        // Should not panic - default implementation works
    }

    #[test]
    fn test_strip_html_basic() {
        let html = "<p>Hello</p>";
        let result = strip_html(html);
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_strip_html_removes_tags() {
        let html = "<div><h1>Title</h1><p>Paragraph</p></div>";
        let result = strip_html(html);
        assert!(result.contains("Title"));
        assert!(result.contains("Paragraph"));
        assert!(!result.contains("<"));
        assert!(!result.contains(">"));
    }

    #[test]
    fn test_strip_html_removes_script_tags() {
        let html = r#"<p>Before</p><script>alert('xss');</script><p>After</p>"#;
        let result = strip_html(html);
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
        assert!(!result.contains("alert"));
        assert!(!result.contains("script"));
    }

    #[test]
    fn test_strip_html_removes_style_tags() {
        let html = r#"<style>.foo { color: red; }</style><p>Content</p>"#;
        let result = strip_html(html);
        assert!(result.contains("Content"));
        assert!(!result.contains("color"));
        assert!(!result.contains("style"));
    }

    #[test]
    fn test_strip_html_removes_style_tags_case_insensitive() {
        let html = r#"<STYLE>.bar{}</STYLE><p>Text</p>"#;
        let result = strip_html(html);
        assert!(result.contains("Text"));
        assert!(!result.contains(".bar"));
    }

    #[test]
    fn test_strip_html_removes_script_tags_case_insensitive() {
        let html = r#"<SCRIPT>code</SCRIPT><div>Content</div>"#;
        let result = strip_html(html);
        assert!(result.contains("Content"));
        assert!(!result.contains("code"));
    }

    #[test]
    fn test_strip_html_collapses_whitespace() {
        let html = "<p>Hello    World</p>";
        let result = strip_html(html);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_strip_html_multiline() {
        let html = r#"
            <div>
                <p>Line 1</p>
                <p>Line 2</p>
            </div>
        "#;
        let result = strip_html(html);
        assert!(result.contains("Line 1"));
        assert!(result.contains("Line 2"));
        // Multiple whitespaces should be collapsed
        assert!(!result.contains("  "));
    }

    #[test]
    fn test_strip_html_empty_input() {
        let result = strip_html("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_strip_html_plain_text() {
        let text = "Just plain text without any tags";
        let result = strip_html(text);
        assert_eq!(result, text);
    }

    #[test]
    fn test_strip_html_nested_tags() {
        let html = "<div><span><b>Bold Text</b></span></div>";
        let result = strip_html(html);
        assert!(result.contains("Bold Text"));
        assert!(!result.contains("<"));
    }

    #[test]
    fn test_strip_html_with_attributes() {
        let html = r#"<a href="http://example.com" class="link">Click here</a>"#;
        let result = strip_html(html);
        assert_eq!(result, "Click here");
    }

    #[test]
    fn test_strip_html_preserves_text_between_tags() {
        let html = "<li>Item 1</li><li>Item 2</li><li>Item 3</li>";
        let result = strip_html(html);
        assert!(result.contains("Item 1"));
        assert!(result.contains("Item 2"));
        assert!(result.contains("Item 3"));
    }

    #[test]
    fn test_strip_html_complex_page() {
        let html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>Test Page</title>
            <style>body { margin: 0; }</style>
            <script>console.log('test');</script>
        </head>
        <body>
            <header><h1>Welcome</h1></header>
            <main>
                <p>Main content here.</p>
            </main>
            <footer>Copyright</footer>
        </body>
        </html>
        "#;
        let result = strip_html(html);
        assert!(result.contains("Welcome"));
        assert!(result.contains("Main content here"));
        assert!(result.contains("Copyright"));
        assert!(!result.contains("console.log"));
        assert!(!result.contains("margin"));
    }

    #[test]
    fn test_strip_html_inline_script() {
        let html = r#"<p>Before</p><script type="text/javascript">var x=1;</script><p>After</p>"#;
        let result = strip_html(html);
        assert!(!result.contains("var x"));
    }

    #[test]
    fn test_strip_html_multiline_script() {
        let html = r#"<script>
            function foo() {
                return 42;
            }
        </script><p>Visible</p>"#;
        let result = strip_html(html);
        assert!(result.contains("Visible"));
        assert!(!result.contains("function"));
    }

    #[test]
    fn test_parse_ddg_response_empty_json() {
        let op = WebOperator::new();
        let result = op.parse_ddg_response("{}");
        assert!(result.contains("No results"));
    }

    #[test]
    fn test_parse_ddg_response_with_abstract() {
        let op = WebOperator::new();
        let json = r#"{"AbstractText": "Rust is a programming language.", "AbstractURL": "https://rust-lang.org"}"#;
        let result = op.parse_ddg_response(json);
        assert!(result.contains("Rust is a programming language"));
        assert!(result.contains("rust-lang.org"));
    }

    #[test]
    fn test_parse_ddg_response_with_related_topics() {
        let op = WebOperator::new();
        let json = r#"{
            "RelatedTopics": [
                {"Text": "Topic 1", "FirstURL": "http://example.com/1"},
                {"Text": "Topic 2", "FirstURL": "http://example.com/2"}
            ]
        }"#;
        let result = op.parse_ddg_response(json);
        assert!(result.contains("Topic 1"));
        assert!(result.contains("Topic 2"));
        assert!(result.contains("example.com/1"));
    }

    #[test]
    fn test_parse_ddg_response_limits_topics() {
        let op = WebOperator::new();
        let topics: Vec<String> = (0..10)
            .map(|i| format!(r#"{{"Text": "Topic {}", "FirstURL": "http://ex.com/{}"}}"#, i, i))
            .collect();
        let json = format!(r#"{{"RelatedTopics": [{}]}}"#, topics.join(","));
        let result = op.parse_ddg_response(&json);
        // Should only show 5 topics max
        assert!(result.contains("Topic 0"));
        assert!(result.contains("Topic 4"));
        assert!(!result.contains("Topic 5"));
    }

    #[test]
    fn test_parse_ddg_response_invalid_json() {
        let op = WebOperator::new();
        let result = op.parse_ddg_response("not valid json");
        assert!(result.contains("Error"));
    }

    #[test]
    fn test_parse_ddg_response_topic_without_url() {
        let op = WebOperator::new();
        let json = r#"{"RelatedTopics": [{"Text": "Just text, no URL"}]}"#;
        let result = op.parse_ddg_response(json);
        assert!(result.contains("Just text"));
    }

    #[test]
    fn test_parse_ddg_response_empty_abstract() {
        let op = WebOperator::new();
        let json = r#"{"AbstractText": "", "AbstractURL": ""}"#;
        let result = op.parse_ddg_response(json);
        // Empty abstract should not appear in output
        assert!(!result.contains("Summary:"));
    }

    #[test]
    fn test_web_operator_clone() {
        let op = WebOperator::new();
        let _cloned = op.clone();
        // Should compile and not panic
    }
}
