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
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base_path.join(p)
        }
    }

    pub fn read_file(&self, path: &str) -> String {
        let full = self.resolve(path);
        match fs::read_to_string(&full) {
            Ok(content) => content,
            Err(_) => format!("Error: File not found: {path}"),
        }
    }

    pub fn write_file(&self, path: &str, content: &str) -> bool {
        let full = self.resolve(path);
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
