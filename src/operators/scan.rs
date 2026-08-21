use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use walkdir::WalkDir;

const MAX_FILE_BYTES: u64 = 102_400; // 100 KB

const IGNORED_DIRS: &[&str] = &[
    "/.git/",
    "/build/",
    "/node_modules/",
    "/.gradle/",
    "/.idea/",
    "/dist/",
    "/out/",
    "/.celebrimbot/",
    "/target/", // Rust build dir
];

const SOURCE_EXTENSIONS: &[&str] = &[
    "kt", "java", "py", "js", "ts", "tsx", "jsx", "rs", "go",
    "c", "cpp", "cc", "h", "hpp", "cs", "rb", "scala", "swift", "php", "kts",
];

/// Port of HeadlessProjectScanOperator.kt
#[derive(Clone)]
pub struct ScanOperator {
    base_path: PathBuf,
}

impl ScanOperator {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self { base_path: base_path.into() }
    }

    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    fn is_ignored(path: &str) -> bool {
        // Normalize separators for cross-platform
        let normalized = path.replace('\\', "/");
        IGNORED_DIRS.iter().any(|d| normalized.contains(d))
    }

    pub fn list_files(&self, sub_path: Option<&str>, extension: Option<&str>) -> String {
        let root = if let Some(sp) = sub_path {
            self.base_path.join(sp)
        } else {
            self.base_path.clone()
        };

        if !root.exists() {
            return format!("Error: path not found: {}", root.display());
        }

        let mut files: Vec<String> = WalkDir::new(&root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                let path = e.path().to_string_lossy();
                !Self::is_ignored(&path)
            })
            .filter(|e| {
                if let Some(ext) = extension {
                    e.path()
                        .extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x == ext)
                        .unwrap_or(false)
                } else {
                    true
                }
            })
            .filter_map(|e| {
                pathdiff::diff_paths(e.path(), &self.base_path)
                    .map(|p| p.to_string_lossy().to_string())
            })
            .collect();

        files.sort();

        if files.is_empty() {
            "No files found.".to_string()
        } else {
            files.join("\n")
        }
    }

    pub fn grep_files(&self, pattern: &str, extension: Option<&str>) -> String {
        let regex = match Regex::new(&format!("(?i){pattern}")) {
            Ok(r) => r,
            Err(e) => return format!("Error: invalid regex pattern — {e}"),
        };

        let mut results: Vec<String> = Vec::new();

        for entry in WalkDir::new(&self.base_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            let path_str = path.to_string_lossy();
            if Self::is_ignored(&path_str) {
                continue;
            }
            if let Some(ext) = extension {
                let file_ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");
                if file_ext != ext {
                    continue;
                }
            }
            if let Ok(content) = fs::read_to_string(path) {
                let rel = pathdiff::diff_paths(path, &self.base_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                for (idx, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        results.push(format!("{rel}:{}: {line}", idx + 1));
                        if results.len() >= 50 {
                            let extra = " ... (more matches omitted)";
                            return results.join("\n") + extra;
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            format!("No matches found for pattern: {pattern}")
        } else {
            results.join("\n")
        }
    }

    pub fn find_by_name(&self, name: &str) -> String {
        let name_lower = name.to_lowercase();
        let mut results: Vec<String> = WalkDir::new(&self.base_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                let path_str = e.path().to_string_lossy();
                !Self::is_ignored(&path_str)
            })
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&name_lower)
            })
            .filter_map(|e| {
                pathdiff::diff_paths(e.path(), &self.base_path)
                    .map(|p| p.to_string_lossy().to_string())
            })
            .collect();

        if results.is_empty() {
            format!("No files matching '{name}' found.")
        } else {
            results.sort();
            results.join("\n")
        }
    }

    pub fn file_stats(&self, target: &str) -> String {
        let path = self.base_path.join(target);
        if !path.exists() {
            return format!("Error: file not found: {target}");
        }
        match fs::read_to_string(&path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                let blank = lines.iter().filter(|l| l.trim().is_empty()).count();
                format!(
                    "File: {target}\nSize: {size} bytes\nLines: {}\nBlank lines: {blank}\nCode lines: {}",
                    lines.len(),
                    lines.len().saturating_sub(blank)
                )
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    /// Returns relative paths of all indexable source files.
    /// Port of HeadlessProjectScanOperator.walkSourceFiles().
    pub fn walk_source_files(&self) -> Vec<String> {
        let exts: HashSet<&str> = SOURCE_EXTENSIONS.iter().copied().collect();

        WalkDir::new(&self.base_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                let path_str = e.path().to_string_lossy();
                !Self::is_ignored(&path_str)
            })
            .filter(|e| {
                let ext = e.path().extension().and_then(|x| x.to_str()).unwrap_or("");
                exts.contains(ext)
            })
            .filter(|e| {
                e.path().metadata().map(|m| m.len() <= MAX_FILE_BYTES).unwrap_or(false)
            })
            .filter(|e| !is_binary(e.path()))
            .filter_map(|e| {
                pathdiff::diff_paths(e.path(), &self.base_path)
                    .map(|p| p.to_string_lossy().to_string())
            })
            .collect()
    }
}

/// Detects binary files by checking for null bytes in first 512 bytes.
fn is_binary(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = fs::File::open(path) else { return false };
    let mut buf = [0u8; 512];
    let Ok(n) = file.read(&mut buf) else { return false };
    buf[..n].contains(&0u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_list_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("b.py"), "print('hi')").unwrap();

        let op = ScanOperator::new(dir.path());
        let result = op.list_files(None, None);
        assert!(result.contains("a.rs"));
        assert!(result.contains("b.py"));
    }

    #[test]
    fn test_list_files_with_extension() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "").unwrap();
        fs::write(dir.path().join("b.py"), "").unwrap();

        let op = ScanOperator::new(dir.path());
        let result = op.list_files(None, Some("rs"));
        assert!(result.contains("a.rs"));
        assert!(!result.contains("b.py"));
    }

    #[test]
    fn test_grep_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "fn hello() { println!(\"world\"); }").unwrap();

        let op = ScanOperator::new(dir.path());
        let result = op.grep_files("hello", None);
        assert!(result.contains("main.rs"));
    }

    #[test]
    fn test_file_stats() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.txt"), "line1\nline2\n\nline4\n").unwrap();

        let op = ScanOperator::new(dir.path());
        let result = op.file_stats("test.txt");
        assert!(result.contains("Lines: 4"));
        assert!(result.contains("Blank lines: 1"));
    }
}
