//! Steering — load project context from .mithril/steering/ and MITHRIL.md.
//!
//! Steering files are Markdown documents with optional front-matter that tell
//! the agent about your project's conventions, stack, and rules.
//!
//! Front-matter format:
//! ```text
//! ---
//! inclusion: always | manual
//! ---
//! ```
//! - `always` (default): always included in system prompt
//! - `manual`: only included when explicitly referenced

use std::fs;
use std::path::{Path, PathBuf};

/// Collected steering content ready to inject into system prompt.
pub fn load_steering(project_root: &str) -> String {
    let root = Path::new(project_root);
    let mut sections: Vec<String> = Vec::new();

    // 1. MITHRIL.md at project root
    let mithril_md = root.join("MITHRIL.md");
    if mithril_md.exists() {
        if let Ok(content) = fs::read_to_string(&mithril_md) {
            sections.push(content);
        }
    }

    // 2. .mithril/steering/*.md files
    let steering_dir = root.join(".mithril").join("steering");
    if steering_dir.is_dir() {
        let mut files: Vec<PathBuf> = fs::read_dir(&steering_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
            .collect();
        files.sort(); // deterministic order

        for file in files {
            if let Ok(content) = fs::read_to_string(&file) {
                let (front_matter, body) = parse_front_matter(&content);
                // Only include "always" (default) files
                let inclusion = front_matter
                    .get("inclusion")
                    .map(|s| s.as_str())
                    .unwrap_or("always");
                if inclusion == "always" {
                    sections.push(body.to_string());
                }
            }
        }
    }

    if sections.is_empty() {
        return String::new();
    }

    format!(
        "[Project Context — from steering files]\n\n{}",
        sections.join("\n\n---\n\n")
    )
}

/// Parse YAML front-matter from a markdown document.
/// Returns (key-value map, body without front-matter).
fn parse_front_matter(content: &str) -> (std::collections::HashMap<String, String>, &str) {
    let mut map = std::collections::HashMap::new();

    if !content.starts_with("---") {
        return (map, content);
    }

    // Find closing ---
    if let Some(end) = content[3..].find("\n---") {
        let fm_block = &content[3..3 + end];
        let body_start = 3 + end + 4; // skip past "\n---"
        let body = if body_start < content.len() {
            content[body_start..].trim_start_matches('\n')
        } else {
            ""
        };

        // Simple key: value parsing
        for line in fm_block.lines() {
            let line = line.trim();
            if let Some((key, value)) = line.split_once(':') {
                map.insert(
                    key.trim().to_string(),
                    value.trim().to_string(),
                );
            }
        }

        (map, body)
    } else {
        (map, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_front_matter_with_inclusion() {
        let content = "---\ninclusion: always\n---\n# Hello\nBody text";
        let (fm, body) = parse_front_matter(content);
        assert_eq!(fm.get("inclusion").map(|s| s.as_str()), Some("always"));
        assert!(body.contains("# Hello"));
        assert!(body.contains("Body text"));
    }

    #[test]
    fn test_parse_front_matter_manual() {
        let content = "---\ninclusion: manual\n---\n# Manual Only";
        let (fm, body) = parse_front_matter(content);
        assert_eq!(fm.get("inclusion").map(|s| s.as_str()), Some("manual"));
        assert!(body.contains("# Manual Only"));
    }

    #[test]
    fn test_parse_front_matter_no_front_matter() {
        let content = "# Just a regular file\nNo front matter here";
        let (fm, body) = parse_front_matter(content);
        assert!(fm.is_empty());
        assert_eq!(body, content);
    }

    #[test]
    fn test_parse_front_matter_empty_front_matter() {
        let content = "---\n---\n# Body";
        let (fm, body) = parse_front_matter(content);
        assert!(fm.is_empty());
        assert!(body.contains("# Body"));
    }

    #[test]
    fn test_load_steering_empty_dir() {
        let dir = tempdir().unwrap();
        let result = load_steering(dir.path().to_str().unwrap());
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_steering_with_mithril_md() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("MITHRIL.md"), "# Project Rules\nUse Rust.").unwrap();
        let result = load_steering(dir.path().to_str().unwrap());
        assert!(result.contains("Project Rules"));
        assert!(result.contains("Use Rust"));
    }

    #[test]
    fn test_load_steering_with_steering_dir() {
        let dir = tempdir().unwrap();
        let steering_dir = dir.path().join(".mithril").join("steering");
        std::fs::create_dir_all(&steering_dir).unwrap();
        std::fs::write(
            steering_dir.join("rules.md"),
            "---\ninclusion: always\n---\n# Always included",
        ).unwrap();
        std::fs::write(
            steering_dir.join("manual.md"),
            "---\ninclusion: manual\n---\n# Should be excluded",
        ).unwrap();
        let result = load_steering(dir.path().to_str().unwrap());
        assert!(result.contains("Always included"));
        assert!(!result.contains("Should be excluded"));
    }

    #[test]
    fn test_load_steering_deterministic_order() {
        let dir = tempdir().unwrap();
        let steering_dir = dir.path().join(".mithril").join("steering");
        std::fs::create_dir_all(&steering_dir).unwrap();
        std::fs::write(steering_dir.join("b_second.md"), "# B").unwrap();
        std::fs::write(steering_dir.join("a_first.md"), "# A").unwrap();
        let result = load_steering(dir.path().to_str().unwrap());
        let a_pos = result.find("# A").unwrap();
        let b_pos = result.find("# B").unwrap();
        assert!(a_pos < b_pos, "Files should be sorted alphabetically");
    }
}
