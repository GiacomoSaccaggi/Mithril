use std::path::PathBuf;
use std::process::Command;

/// Port of HeadlessGitOperator.kt
#[derive(Clone)]
pub struct GitOperator {
    base_path: PathBuf,
}

impl GitOperator {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self { base_path: base_path.into() }
    }

    fn run(&self, args: &[&str]) -> String {
        let child = Command::new("git")
            .args(args)
            .current_dir(&self.base_path)
            .output();

        match child {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let combined = format!("{stdout}{stderr}");
                let trimmed = combined.trim();
                if trimmed.is_empty() { "(no output)".to_string() } else { trimmed.to_string() }
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    pub fn status(&self) -> String {
        self.run(&["status", "--short"])
    }

    pub fn log(&self, max_entries: usize) -> String {
        let n = format!("-{max_entries}");
        self.run(&["log", "--oneline", "--decorate", &n])
    }

    pub fn diff(&self, target: Option<&str>) -> String {
        let result = if let Some(t) = target {
            self.run(&["diff", "HEAD", "--", t])
        } else {
            self.run(&["diff", "HEAD"])
        };
        let truncated = if result.len() > 6000 { &result[..6000] } else { &result };
        if truncated.trim().is_empty() || truncated == "(no output)" { "No changes.".to_string() } else { truncated.to_string() }
    }

    pub fn blame(&self, target: &str) -> String {
        let result = self.run(&["blame", "--line-porcelain", target]);
        if result.len() > 4000 { result[..4000].to_string() } else { result }
    }

    pub fn branch(&self) -> String {
        self.run(&["branch", "--show-current"])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    /// Helper to create a git repo with an initial commit.
    fn setup_git_repo() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // git init
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();

        // Configure git user (required for commit)
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .output()
            .unwrap();

        // Create and commit a file
        std::fs::write(path.join("README.md"), "# Test Project\n").unwrap();
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(path)
            .output()
            .unwrap();

        dir
    }

    #[test]
    fn test_git_operator_new() {
        let op = GitOperator::new("/tmp/test");
        assert_eq!(op.base_path, PathBuf::from("/tmp/test"));
    }

    #[test]
    fn test_git_operator_clone() {
        let op = GitOperator::new("/tmp/test");
        let cloned = op.clone();
        assert_eq!(cloned.base_path, op.base_path);
    }

    #[test]
    fn test_status_in_git_repo() {
        let dir = setup_git_repo();
        let op = GitOperator::new(dir.path());

        // Initially clean
        let status = op.status();
        assert!(status.contains("(no output)") || status.is_empty() || !status.contains("??"));

        // Add an untracked file
        std::fs::write(dir.path().join("new_file.txt"), "untracked").unwrap();
        let status = op.status();
        assert!(status.contains("??") || status.contains("new_file"));
    }

    #[test]
    fn test_status_in_non_git_dir() {
        let dir = tempdir().unwrap();
        let op = GitOperator::new(dir.path());
        let status = op.status();
        // Should contain error about not being a git repo
        assert!(status.contains("not a git repository") || status.contains("fatal"));
    }

    #[test]
    fn test_log_with_commits() {
        let dir = setup_git_repo();
        let op = GitOperator::new(dir.path());

        let log = op.log(5);
        assert!(log.contains("Initial commit"));
    }

    #[test]
    fn test_log_limits_entries() {
        let dir = setup_git_repo();
        let path = dir.path();

        // Create a few more commits
        for i in 1..=5 {
            std::fs::write(path.join(format!("file{}.txt", i)), format!("content {}", i)).unwrap();
            Command::new("git")
                .args(["add", "."])
                .current_dir(path)
                .output()
                .unwrap();
            Command::new("git")
                .args(["commit", "-m", &format!("Commit {}", i)])
                .current_dir(path)
                .output()
                .unwrap();
        }

        let op = GitOperator::new(path);
        let log = op.log(2);
        // Should only show 2 entries
        let lines: Vec<&str> = log.lines().collect();
        assert!(lines.len() <= 3); // 2 commits max + possible newline
    }

    #[test]
    fn test_diff_no_changes() {
        let dir = setup_git_repo();
        let op = GitOperator::new(dir.path());

        let diff = op.diff(None);
        assert!(diff.contains("No changes."));
    }

    #[test]
    fn test_diff_with_changes() {
        let dir = setup_git_repo();
        let op = GitOperator::new(dir.path());

        // Modify committed file
        std::fs::write(dir.path().join("README.md"), "# Modified\nNew content").unwrap();
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let diff = op.diff(None);
        assert!(diff.contains("Modified") || diff.contains("New content") || diff.contains("+++"));
    }

    #[test]
    fn test_diff_specific_file() {
        let dir = setup_git_repo();
        let op = GitOperator::new(dir.path());

        std::fs::write(dir.path().join("README.md"), "# Changed").unwrap();
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let diff = op.diff(Some("README.md"));
        assert!(diff.contains("README") || diff.contains("Changed") || diff.contains("+++"));
    }

    #[test]
    fn test_branch_name() {
        let dir = setup_git_repo();
        let op = GitOperator::new(dir.path());

        let branch = op.branch();
        // Default branch is usually "main" or "master"
        assert!(branch.contains("main") || branch.contains("master"));
    }

    #[test]
    fn test_branch_on_new_branch() {
        let dir = setup_git_repo();
        let path = dir.path();

        // Create and checkout a new branch
        Command::new("git")
            .args(["checkout", "-b", "feature-test"])
            .current_dir(path)
            .output()
            .unwrap();

        let op = GitOperator::new(path);
        let branch = op.branch();
        assert!(branch.contains("feature-test"));
    }

    #[test]
    fn test_blame_on_file() {
        let dir = setup_git_repo();
        let op = GitOperator::new(dir.path());

        let blame = op.blame("README.md");
        // Should contain author info and line content
        assert!(blame.contains("Test User") || blame.contains("author") || blame.contains("README"));
    }

    #[test]
    fn test_blame_nonexistent_file() {
        let dir = setup_git_repo();
        let op = GitOperator::new(dir.path());

        let blame = op.blame("nonexistent.txt");
        assert!(blame.contains("fatal") || blame.contains("error") || blame.contains("no such"));
    }

    #[test]
    fn test_blame_truncation() {
        let dir = setup_git_repo();
        let path = dir.path();

        // Create a file with many lines
        let content: String = (0..500).map(|i| format!("Line {}\n", i)).collect();
        std::fs::write(path.join("big.txt"), &content).unwrap();
        Command::new("git")
            .args(["add", "big.txt"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Add big file"])
            .current_dir(path)
            .output()
            .unwrap();

        let op = GitOperator::new(path);
        let blame = op.blame("big.txt");
        // Should be truncated to 4000 chars
        assert!(blame.len() <= 4000);
    }

    #[test]
    fn test_diff_truncation() {
        let dir = setup_git_repo();
        let path = dir.path();

        // Create a file with lots of content
        let content: String = (0..1000).map(|i| format!("Line {} with some padding content\n", i)).collect();
        std::fs::write(path.join("huge.txt"), &content).unwrap();
        Command::new("git")
            .args(["add", "huge.txt"])
            .current_dir(path)
            .output()
            .unwrap();

        let op = GitOperator::new(path);
        let diff = op.diff(None);
        // Should be truncated to 6000 chars
        assert!(diff.len() <= 6000);
    }

    #[test]
    fn test_run_returns_combined_output() {
        let dir = tempdir().unwrap();
        let op = GitOperator::new(dir.path());
        // Running in non-git dir should return stderr message
        let result = op.status();
        assert!(!result.is_empty());
    }
}
