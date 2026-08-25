#![allow(dead_code)]
/// Port of PalantirIndex.kt — BM25 semantic index for project files.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::operators::scan::ScanOperator;

const CURRENT_VERSION: u32 = 1;
const INDEX_DIR: &str = ".celebrimbot";
const INDEX_FILE: &str = "palantir_index.json";
const STALE_THRESHOLD: f64 = 0.20;
const BM25_K1: f64 = 1.5;
const BM25_B: f64 = 0.75;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PalantirEntry {
    pub path: String,
    pub symbols: Vec<String>,
    pub terms: HashMap<String, u32>,
    pub line_count: usize,
    pub last_modified: u64, // epoch millis
}

#[derive(Debug)]
pub struct ScoredEntry {
    pub entry: PalantirEntry,
    pub score: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PalantirIndex {
    pub version: u32,
    pub base_path: String,
    pub indexed_at: u64,
    pub entries: Vec<PalantirEntry>,
    pub idf: HashMap<String, f64>,
}

impl PalantirIndex {
    // ── Query ──────────────────────────────────────────────────────────────

    pub fn query(&self, prompt: &str, top_k: usize) -> Vec<ScoredEntry> {
        let query_terms = tokenize(prompt);
        if query_terms.is_empty() || self.entries.is_empty() {
            return vec![];
        }

        let avg_dl = {
            let total: u32 = self.entries.iter().map(|e| e.terms.values().sum::<u32>()).sum();
            if self.entries.is_empty() { 1.0 } else { total as f64 / self.entries.len() as f64 }
        };

        let mut scored: Vec<ScoredEntry> = self.entries.iter()
            .map(|entry| {
                let dl = entry.terms.values().sum::<u32>() as f64;
                let score: f64 = query_terms.iter().map(|term| {
                    let tf = *entry.terms.get(term.as_str()).unwrap_or(&0) as f64;
                    if tf == 0.0 { return 0.0; }
                    let idf = self.idf.get(term.as_str()).copied().unwrap_or(0.0);
                    let num = tf * (BM25_K1 + 1.0);
                    let den = tf + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / avg_dl);
                    idf * (num / den)
                }).sum();
                ScoredEntry { entry: entry.clone(), score }
            })
            .filter(|s| s.score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }

    // ── Staleness ──────────────────────────────────────────────────────────

    pub fn is_stale(&self, base_path: &str) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let changed = self.entries.iter().filter(|e| {
            let file = Path::new(base_path).join(&e.path);
            !file.exists() || {
                file.metadata()
                    .and_then(|m| m.modified())
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0)
                    })
                    .unwrap_or(0)
                    != e.last_modified
            }
        }).count();

        changed as f64 / self.entries.len() as f64 > STALE_THRESHOLD
    }

    // ── Persistence ────────────────────────────────────────────────────────

    pub fn save(&self, base_path: &str) {
        let dir = Path::new(base_path).join(INDEX_DIR);
        let _ = fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(dir.join(INDEX_FILE), json);
        }
    }

    pub fn load_or_null(base_path: &str) -> Option<PalantirIndex> {
        let path = Path::new(base_path).join(INDEX_DIR).join(INDEX_FILE);
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    // ── Builders ───────────────────────────────────────────────────────────

    pub fn build(base_path: &str, scan_op: &ScanOperator) -> PalantirIndex {
        let source_paths = scan_op.walk_source_files();
        // H4: walk_source_files() already returns relative paths from base_path (via pathdiff).
        // is_within_base() is available for use in external query/resource handlers.
        let entries: Vec<PalantirEntry> = source_paths
            .iter()
            .filter_map(|p| index_file(base_path, p))
            .collect();
        let idf = compute_idf(&entries);

        PalantirIndex {
            version: CURRENT_VERSION,
            base_path: base_path.to_string(),
            indexed_at: now_millis(),
            entries,
            idf,
        }
    }

    pub fn build_incremental(
        base_path: &str,
        scan_op: &ScanOperator,
        existing: Option<PalantirIndex>,
    ) -> PalantirIndex {
        let existing = match existing {
            Some(e) => e,
            None => return Self::build(base_path, scan_op),
        };

        let existing_by_path: HashMap<&str, &PalantirEntry> = existing
            .entries
            .iter()
            .map(|e| (e.path.as_str(), e))
            .collect();

        let source_paths = scan_op.walk_source_files();
        let entries: Vec<PalantirEntry> = source_paths
            .iter()
            .filter_map(|rel_path| {
                let file = Path::new(base_path).join(rel_path);
                let last_mod = file.metadata()
                    .and_then(|m| m.modified())
                    .map(|t| t.duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64).unwrap_or(0))
                    .unwrap_or(0);

                if let Some(cached) = existing_by_path.get(rel_path.as_str()) {
                    if cached.last_modified == last_mod {
                        return Some((*cached).clone()); // unchanged
                    }
                }
                index_file(base_path, rel_path)
            })
            .collect();

        let idf = compute_idf(&entries);
        PalantirIndex {
            version: CURRENT_VERSION,
            base_path: base_path.to_string(),
            indexed_at: now_millis(),
            entries,
            idf,
        }
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn index_file(base_path: &str, relative_path: &str) -> Option<PalantirEntry> {
    let path = Path::new(base_path).join(relative_path);
    let content = fs::read_to_string(&path).ok()?;
    let last_modified = path.metadata()
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64).unwrap_or(0))
        .unwrap_or(0);

    let symbols = extract_symbols(&content, relative_path);
    let terms = build_term_frequency(&content);
    let line_count = content.lines().count();

    Some(PalantirEntry {
        path: relative_path.to_string(),
        symbols,
        terms,
        line_count,
        last_modified,
    })
}

pub fn tokenize(text: &str) -> Vec<String> {
    let stopwords = stopwords();
    text.split(|c: char| !c.is_alphanumeric())
        .map(|s| s.to_lowercase())
        .filter(|s| s.len() >= 3 && !stopwords.contains(s.as_str()))
        .collect()

}

fn build_term_frequency(content: &str) -> HashMap<String, u32> {
    let mut freq: HashMap<String, u32> = HashMap::new();
    for term in tokenize(content) {
        *freq.entry(term).or_insert(0) += 1;
    }
    freq
}

fn extract_symbols(content: &str, path: &str) -> Vec<String> {
    let ext = path.rsplit('.').next().unwrap_or("");
    let pattern = match ext {
        "kt" | "kts" | "java" | "scala" | "cs" => {
            r"(?:class|interface|object|fun|val|var|enum)\s+(\w+)"
        }
        "py" => r"(?:class|def)\s+(\w+)",
        "js" | "ts" | "jsx" | "tsx" => r"(?:function|class|const|let|var)\s+(\w+)",
        "go" => r"(?:func|type|var|const)\s+(\w+)",
        "rs" => r"(?:fn|struct|enum|trait|impl|mod|const|let)\s+(\w+)",
        _ => r"(?:class|function|def|func|fn)\s+(\w+)",
    };

    let re = match Regex::new(pattern) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let stopwords = stopwords();
    re.captures_iter(content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .filter(|s| s.len() >= 2 && !stopwords.contains(s.to_lowercase().as_str()))
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .take(50)
        .collect()
}

fn compute_idf(entries: &[PalantirEntry]) -> HashMap<String, f64> {
    let n = entries.len() as f64;
    if n == 0.0 {
        return HashMap::new();
    }

    let mut doc_freq: HashMap<String, u32> = HashMap::new();
    for entry in entries {
        for term in entry.terms.keys() {
            *doc_freq.entry(term.clone()).or_insert(0) += 1;
        }
    }

    doc_freq
        .into_iter()
        .map(|(term, df)| {
            let idf = ((n - df as f64 + 0.5) / (df as f64 + 0.5) + 1.0).ln();
            (term, idf)
        })
        .collect()
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn stopwords() -> HashSet<&'static str> {
    // Port of PalantirIndex.kt STOPWORDS set
    [
        "val", "var", "fun", "class", "object", "interface", "enum", "data",
        "return", "import", "package", "public", "private", "protected",
        "override", "open", "final", "static", "abstract", "sealed",
        "this", "super", "null", "true", "false", "new", "void",
        "int", "long", "float", "double", "boolean", "string", "char",
        "if", "else", "when", "for", "while", "do", "try", "catch",
        "throw", "throws", "finally", "break", "continue",
        "def", "self", "none", "pass", "with", "from", "not", "and", "or",
        "lambda", "yield", "async", "await",
        "const", "let", "function", "typeof", "instanceof", "undefined",
        "prototype", "require", "module", "exports",
        "the", "and", "for", "are", "but", "not", "you", "all", "can",
        "get", "set", "add", "put", "has", "use", "new", "any", "map",
        "list", "type", "name", "size", "init", "run", "log", "err",
    ]
    .iter()
    .copied()
    .collect()
}

/// H4: Check that a candidate path is within the base directory (path traversal guard).
fn is_within_base(base: &Path, candidate: &str) -> bool {
    let joined = base.join(candidate);
    let canonical = match joined.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            joined.components()
                .fold(std::path::PathBuf::new(), |mut acc, c| {
                    match c {
                        std::path::Component::ParentDir => { acc.pop(); acc }
                        other => { acc.push(other); acc }
                    }
                })
        }
    };
    canonical.starts_with(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;
    use crate::operators::scan::ScanOperator;

    #[test]
    fn test_build_and_query() {
        let dir = tempdir().unwrap();

        fs::write(dir.path().join("main.rs"), "struct Foo { bar: i32 }\nfn main() {}").unwrap();
        fs::write(dir.path().join("lib.rs"), "pub struct Bar;\nimpl Bar { fn new() -> Self { Bar } }").unwrap();
        fs::write(dir.path().join("utils.rs"), "pub fn helper() -> String { String::new() }").unwrap();

        let scan_op = ScanOperator::new(dir.path());
        let index = PalantirIndex::build(dir.path().to_str().unwrap(), &scan_op);

        assert!(!index.entries.is_empty());

        let results = index.query("struct", 5);
        assert!(!results.is_empty());
        assert!(results[0].score > 0.0);
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("fn hello_world() { let x = 42; }");
        // Underscores split tokens: "hello_world" becomes ["hello", "world"]
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        // "let" is a stopword
        assert!(!tokens.contains(&"let".to_string()));
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let scan_op = ScanOperator::new(dir.path());
        fs::write(dir.path().join("test.rs"), "fn foo() {}").unwrap();

        let index = PalantirIndex::build(dir.path().to_str().unwrap(), &scan_op);
        index.save(dir.path().to_str().unwrap());

        let loaded = PalantirIndex::load_or_null(dir.path().to_str().unwrap());
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().entries.len(), index.entries.len());
    }
}
