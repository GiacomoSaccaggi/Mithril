//! Token tracking — count tokens per provider call, accumulate per-agent.
//!
//! Uses real API token counts when available (Gemini usageMetadata,
//! Groq/OpenAI usage field), falls back to text-length estimation.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Token usage for a single API call.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
}

impl TokenUsage {
    pub fn new(input: u64, output: u64) -> Self {
        Self { input, output }
    }

    pub fn total(&self) -> u64 {
        self.input + self.output
    }

    /// Estimate tokens from text lengths (1 token ≈ 4 chars).
    pub fn estimate(input_text: &str, output_text: &str) -> Self {
        Self {
            input: (input_text.len() as u64).div_ceil(4),
            output: (output_text.len() as u64).div_ceil(4),
        }
    }

    /// Format for display: "1.2k in / 800 out"
    pub fn display(&self) -> String {
        format!("{}in / {}out", format_count(self.input), format_count(self.output))
    }

    pub fn add(&mut self, other: &TokenUsage) {
        self.input += other.input;
        self.output += other.output;
    }
}

/// Per-agent token accumulator.
#[derive(Debug, Clone, Default)]
pub struct AgentTokenTracker {
    pub agent_name: String,
    pub usage: TokenUsage,
    pub call_count: u32,
}

impl AgentTokenTracker {
    pub fn new(name: &str) -> Self {
        Self {
            agent_name: name.to_string(),
            usage: TokenUsage::default(),
            call_count: 0,
        }
    }

    pub fn record(&mut self, usage: &TokenUsage) {
        self.usage.add(usage);
        self.call_count += 1;
    }
}

/// Session-level token tracking across all agents.
#[derive(Debug, Clone, Default)]
pub struct SessionTokens {
    pub trackers: HashMap<String, AgentTokenTracker>,
}

impl SessionTokens {
    pub fn new() -> Self {
        Self { trackers: HashMap::new() }
    }

    /// Record token usage for an agent.
    pub fn record(&mut self, agent_name: &str, usage: &TokenUsage) {
        let tracker = self.trackers
            .entry(agent_name.to_string())
            .or_insert_with(|| AgentTokenTracker::new(agent_name));
        tracker.record(usage);
    }

    /// Get total tokens across all agents.
    pub fn total(&self) -> TokenUsage {
        let mut total = TokenUsage::default();
        for tracker in self.trackers.values() {
            total.add(&tracker.usage);
        }
        total
    }

    /// Format all agents for display.
    pub fn display_all(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let mut sorted: Vec<_> = self.trackers.values().collect();
        sorted.sort_by_key(|t| std::cmp::Reverse(t.usage.total()));

        for tracker in &sorted {
            lines.push(format!(
                "  {}: {} ({}x)",
                tracker.agent_name,
                tracker.usage.display(),
                tracker.call_count
            ));
        }

        let total = self.total();
        if sorted.len() > 1 {
            lines.push("  ─────────".to_string());
            lines.push(format!("  TOTAL: {}", total.display()));
        }

        lines
    }
}

/// Thread-safe shared token tracker.
pub type SharedTokens = Arc<Mutex<SessionTokens>>;

pub fn new_shared_tokens() -> SharedTokens {
    Arc::new(Mutex::new(SessionTokens::new()))
}

/// Format a token count for display.
fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M ", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k ", n as f64 / 1_000.0)
    } else {
        format!("{} ", n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_estimate() {
        let usage = TokenUsage::estimate("hello world", "response");
        assert_eq!(usage.input, 3); // 11 chars / 4 ≈ 3
        assert_eq!(usage.output, 2); // 8 chars / 4 = 2
    }

    #[test]
    fn test_token_usage_display() {
        let usage = TokenUsage::new(1500, 800);
        assert!(usage.display().contains("1.5k"));
        assert!(usage.display().contains("800"));
    }

    #[test]
    fn test_session_tokens_record() {
        let mut session = SessionTokens::new();
        session.record("gemini", &TokenUsage::new(100, 50));
        session.record("gemini", &TokenUsage::new(200, 100));
        session.record("kiro", &TokenUsage::new(0, 0));

        let total = session.total();
        assert_eq!(total.input, 300);
        assert_eq!(total.output, 150);
        assert_eq!(session.trackers["gemini"].call_count, 2);
    }

    #[test]
    fn test_format_count() {
        assert_eq!(format_count(500), "500 ");
        assert_eq!(format_count(1500), "1.5k ");
        assert_eq!(format_count(1_500_000), "1.5M ");
    }

    #[test]
    fn test_token_usage_new() {
        let usage = TokenUsage::new(100, 200);
        assert_eq!(usage.input, 100);
        assert_eq!(usage.output, 200);
    }

    #[test]
    fn test_token_usage_total() {
        let usage = TokenUsage::new(100, 200);
        assert_eq!(usage.total(), 300);
    }

    #[test]
    fn test_token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input, 0);
        assert_eq!(usage.output, 0);
        assert_eq!(usage.total(), 0);
    }

    #[test]
    fn test_token_usage_add() {
        let mut usage = TokenUsage::new(100, 50);
        usage.add(&TokenUsage::new(200, 150));
        assert_eq!(usage.input, 300);
        assert_eq!(usage.output, 200);
    }

    #[test]
    fn test_token_usage_estimate_empty() {
        let usage = TokenUsage::estimate("", "");
        assert_eq!(usage.input, 0);
        assert_eq!(usage.output, 0);
    }

    #[test]
    fn test_token_usage_estimate_short() {
        let usage = TokenUsage::estimate("hi", "ok");
        assert_eq!(usage.input, 1); // 2 chars → (2+3)/4 = 1
        assert_eq!(usage.output, 1); // 2 chars → (2+3)/4 = 1
    }

    #[test]
    fn test_token_usage_estimate_long() {
        let long_input = "x".repeat(1000);
        let long_output = "y".repeat(2000);
        let usage = TokenUsage::estimate(&long_input, &long_output);
        assert_eq!(usage.input, 250); // 1000/4
        assert_eq!(usage.output, 500); // 2000/4
    }

    #[test]
    fn test_format_count_zero() {
        assert_eq!(format_count(0), "0 ");
    }

    #[test]
    fn test_format_count_small() {
        assert_eq!(format_count(1), "1 ");
        assert_eq!(format_count(999), "999 ");
    }

    #[test]
    fn test_format_count_thousands() {
        assert_eq!(format_count(1000), "1.0k ");
        assert_eq!(format_count(10_000), "10.0k ");
        assert_eq!(format_count(999_999), "1000.0k ");
    }

    #[test]
    fn test_format_count_millions() {
        assert_eq!(format_count(1_000_000), "1.0M ");
        assert_eq!(format_count(10_000_000), "10.0M ");
    }

    #[test]
    fn test_agent_token_tracker_new() {
        let tracker = AgentTokenTracker::new("test-agent");
        assert_eq!(tracker.agent_name, "test-agent");
        assert_eq!(tracker.call_count, 0);
        assert_eq!(tracker.usage.total(), 0);
    }

    #[test]
    fn test_agent_token_tracker_record() {
        let mut tracker = AgentTokenTracker::new("gemini");
        tracker.record(&TokenUsage::new(100, 50));
        assert_eq!(tracker.call_count, 1);
        assert_eq!(tracker.usage.input, 100);
        assert_eq!(tracker.usage.output, 50);

        tracker.record(&TokenUsage::new(200, 100));
        assert_eq!(tracker.call_count, 2);
        assert_eq!(tracker.usage.input, 300);
        assert_eq!(tracker.usage.output, 150);
    }

    #[test]
    fn test_session_tokens_new() {
        let session = SessionTokens::new();
        assert!(session.trackers.is_empty());
    }

    #[test]
    fn test_session_tokens_default() {
        let session = SessionTokens::default();
        assert!(session.trackers.is_empty());
    }

    #[test]
    fn test_session_tokens_total_empty() {
        let session = SessionTokens::new();
        let total = session.total();
        assert_eq!(total.input, 0);
        assert_eq!(total.output, 0);
    }

    #[test]
    fn test_session_tokens_multiple_agents() {
        let mut session = SessionTokens::new();
        session.record("agent1", &TokenUsage::new(100, 50));
        session.record("agent2", &TokenUsage::new(200, 100));
        session.record("agent3", &TokenUsage::new(300, 150));

        let total = session.total();
        assert_eq!(total.input, 600);
        assert_eq!(total.output, 300);
        assert_eq!(session.trackers.len(), 3);
    }

    #[test]
    fn test_session_tokens_display_all_empty() {
        let session = SessionTokens::new();
        let lines = session.display_all();
        assert!(lines.is_empty());
    }

    #[test]
    fn test_session_tokens_display_all_single_agent() {
        let mut session = SessionTokens::new();
        session.record("gemini", &TokenUsage::new(1000, 500));

        let lines = session.display_all();
        assert_eq!(lines.len(), 1); // Only one agent, no total line
        assert!(lines[0].contains("gemini"));
    }

    #[test]
    fn test_session_tokens_display_all_multiple_agents() {
        let mut session = SessionTokens::new();
        session.record("agent1", &TokenUsage::new(100, 50));
        session.record("agent2", &TokenUsage::new(200, 100));

        let lines = session.display_all();
        // Should have agent1, agent2, separator, and TOTAL
        assert!(lines.len() >= 3);
        assert!(lines.iter().any(|l| l.contains("TOTAL")));
    }

    #[test]
    fn test_session_tokens_display_sorted_by_total() {
        let mut session = SessionTokens::new();
        session.record("small", &TokenUsage::new(10, 5));
        session.record("large", &TokenUsage::new(1000, 500));
        session.record("medium", &TokenUsage::new(100, 50));

        let lines = session.display_all();
        // Large should come first (sorted by total descending)
        let large_pos = lines.iter().position(|l| l.contains("large")).unwrap();
        let medium_pos = lines.iter().position(|l| l.contains("medium")).unwrap();
        let small_pos = lines.iter().position(|l| l.contains("small")).unwrap();

        assert!(large_pos < medium_pos);
        assert!(medium_pos < small_pos);
    }

    #[test]
    fn test_new_shared_tokens() {
        let tokens = new_shared_tokens();
        let locked = tokens.lock().unwrap();
        assert!(locked.trackers.is_empty());
    }

    #[test]
    fn test_shared_tokens_thread_safe() {
        use std::thread;

        let tokens = new_shared_tokens();
        let tokens_clone = tokens.clone();

        let handle = thread::spawn(move || {
            let mut locked = tokens_clone.lock().unwrap();
            locked.record("thread_agent", &TokenUsage::new(100, 50));
        });

        handle.join().unwrap();

        let locked = tokens.lock().unwrap();
        assert_eq!(locked.trackers.len(), 1);
    }

    #[test]
    fn test_token_usage_display_zero() {
        let usage = TokenUsage::new(0, 0);
        let display = usage.display();
        assert!(display.contains("0"));
    }

    #[test]
    fn test_token_usage_display_large() {
        let usage = TokenUsage::new(5_000_000, 2_500_000);
        let display = usage.display();
        assert!(display.contains("M"));
    }

    #[test]
    fn test_token_usage_clone() {
        let usage = TokenUsage::new(100, 50);
        let cloned = usage.clone();
        assert_eq!(cloned.input, usage.input);
        assert_eq!(cloned.output, usage.output);
    }

    #[test]
    fn test_agent_token_tracker_clone() {
        let mut tracker = AgentTokenTracker::new("test");
        tracker.record(&TokenUsage::new(100, 50));

        let cloned = tracker.clone();
        assert_eq!(cloned.agent_name, tracker.agent_name);
        assert_eq!(cloned.call_count, tracker.call_count);
    }

    #[test]
    fn test_session_tokens_clone() {
        let mut session = SessionTokens::new();
        session.record("agent", &TokenUsage::new(100, 50));

        let cloned = session.clone();
        assert_eq!(cloned.trackers.len(), session.trackers.len());
    }
}
