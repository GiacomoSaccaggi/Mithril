#![allow(dead_code)]
use std::fs;
use std::path::{Path, PathBuf};

/// Port of HeadlessFileOperator.kt
#[derive(Clone)]
pub struct FileOperator {
    base_path: PathBuf,
}

impl FileOperator {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self { base_path: base_path.into() }
    }

    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);

        // Security: block absolute paths outside project
        if p.is_absolute() {
            let base_canonical = self.base_path.canonicalize()
                .unwrap_or_else(|_| self.base_path.clone());
            if let Ok(canonical) = p.canonicalize() {
                if canonical.starts_with(&base_canonical) {
                    return canonical;
                }
            }
            // Absolute path outside project or doesn't exist
            return self.base_path.join("__access_denied__");
        }

        // Security: block relative paths with .. components that escape base
        let normalized = path.replace("\\", "/");
        let components: Vec<&str> = normalized.split('/').collect();
        let mut depth: i32 = 0;
        for c in &components {
            if *c == ".." {
                depth -= 1;
            } else if !c.is_empty() && *c != "." {
                depth += 1;
            }
            if depth < 0 {
                return self.base_path.join("__access_denied__");
            }
        }

        // Safe relative path — join with base
        self.base_path.join(p)
    }

    pub fn read_file(&self, path: &str) -> String {
        let full = self.resolve(path);
        if full.ends_with("__access_denied__") {
            return format!("Error: Access denied (path outside project): {path}");
        }
        match fs::read_to_string(&full) {
            Ok(content) => content,
            Err(_) => format!("Error: File not found: {path}"),
        }
    }

    pub fn write_file(&self, path: &str, content: &str) -> bool {
        let full = self.resolve(path);
        // Security: reject if resolve() returned the access-denied sentinel
        if full.ends_with("__access_denied__") {
            tracing::warn!("write_file: path traversal blocked for: {path}");
            return false;
        }
        if let Some(parent) = full.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                tracing::warn!("write_file: failed to create dirs: {e}");
                return false;
            }
        }
        fs::write(full, content).is_ok()
    }

    pub fn delete_file(&self, path: &str) -> bool {
        let full = self.resolve(path);
        if full.ends_with("__access_denied__") {
            tracing::warn!("delete_file: path traversal blocked for: {path}");
            return false;
        }
        fs::remove_file(full).is_ok()
    }

    pub fn exists(&self, path: &str) -> bool {
        self.resolve(path).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_write_read_delete() {
        let dir = tempdir().unwrap();
        let op = FileOperator::new(dir.path());

        assert!(op.write_file("hello.txt", "world"));
        assert_eq!(op.read_file("hello.txt"), "world");
        assert!(op.exists("hello.txt"));
        assert!(op.delete_file("hello.txt"));
        assert!(!op.exists("hello.txt"));
    }

    #[test]
    fn test_missing_file() {
        let dir = tempdir().unwrap();
        let op = FileOperator::new(dir.path());
        let result = op.read_file("nonexistent.txt");
        assert!(result.starts_with("Error: File not found:"));
    }

    #[test]
    fn test_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let op = FileOperator::new(dir.path());
        assert!(op.write_file("a/b/c.txt", "nested"));
        assert_eq!(op.read_file("a/b/c.txt"), "nested");
    }
}
