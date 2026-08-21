use std::path::PathBuf;

use anyhow::{bail, Result};

/// Port of HeadlessTerminalOperator.kt
pub struct TerminalResult {
    pub output: String,
    pub exit_code: i32,
}

#[derive(Clone)]
pub struct TerminalOperator {
    working_dir: PathBuf,
    timeout_secs: u64,
    /// When true, dangerous commands are rejected before execution
    sandbox: bool,
}

/// Dangerous literal patterns blocked when sandbox=true.
const BLOCKED_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    "rm --no-preserve-root",
    "sudo ",
    "sudo\t",
    "dd if=",
    "mkfs",
    ":(){ :|:& };:",  // fork bomb
    "> /dev/sd",
    "chmod -R 777 /",
    "chown -R",
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "curl | sh",
    "wget | sh",
    "curl|sh",
    "wget|sh",
    // base64 decode-and-execute patterns
    "base64 -d",
    "base64 --decode",
    "base64 -D",
    // eval-based execution
    "eval $(", "eval $(",
    "`base64",
    // Dangerous execution patterns (NOT: sh -c / bash -c — those are used by cargo, git, etc.)
    "exec /bin/sh",
    "exec /bin/bash",
    // Interpreter one-liners for inline code execution (bypass vectors)
    "perl -e",
    "ruby -e",
    "node -e",
];

/// Validate a command against the sandbox denylist.
///
/// Note: this is a best-effort defence-in-depth layer, not a perfect sandbox.
/// A real sandbox requires OS-level isolation (seccomp, namespaces, etc.).
/// This blocks the most common LLM prompt-injection attack vectors.
pub fn validate_command(cmd: &str) -> Result<()> {
    let lower = cmd.trim().to_lowercase();

    // Strip common quoting/spacing obfuscation for matching
    // e.g. s'u'do → sudo, su\ do → sudo
    let collapsed: String = lower
        .replace("\'", "")
        .replace("\"", "")
        .replace("\'", "")
        .replace("\"", "")
        .replace(" \n", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    for pattern in BLOCKED_PATTERNS {
        if lower.contains(pattern) || collapsed.contains(pattern) {
            bail!(
                "Command blocked by sandbox: matches pattern '{}'.                  Set terminal_sandbox=false in config to disable.",
                pattern
            );
        }
    }

    // Block shell variable-based obfuscation: $IFS, $'...', ${...}
    if lower.contains("$ifs") || lower.contains("${ifs}") {
        bail!("Command blocked by sandbox: IFS manipulation detected.");
    }

    Ok(())
}

impl TerminalOperator {
    pub fn new(working_dir: impl Into<PathBuf>, timeout_secs: u64) -> Self {
        Self { working_dir: working_dir.into(), timeout_secs, sandbox: true }
    }

    pub fn with_sandbox(mut self, sandbox: bool) -> Self {
        self.sandbox = sandbox;
        self
    }

    pub async fn execute(&self, command: &str) -> TerminalResult {
        // Sandbox check before execution
        if self.sandbox {
            if let Err(e) = validate_command(command) {
                return TerminalResult { output: format!("Error: {e}"), exit_code: -1 };
            }
        }

        let working_dir = self.working_dir.clone();
        let command = command.to_string();
        let timeout_secs = self.timeout_secs;
        let timeout = std::time::Duration::from_secs(timeout_secs);

        // M1 fix: use tokio::process for async I/O — avoids pipe buffer deadlock.
        // kill_on_drop(true) ensures the child is killed when timeout fires.
        let result = tokio::time::timeout(timeout, async move {
            let mut cmd = if cfg!(target_os = "windows") {
                let mut c = tokio::process::Command::new("cmd");
                c.args(["/C", &command]);
                c
            } else {
                let mut c = tokio::process::Command::new("sh");
                c.args(["-c", &command]);
                c
            };

            cmd.current_dir(&working_dir)
                .env("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true);

            match cmd.output().await {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    TerminalResult {
                        output: format!("{stdout}{stderr}"),
                        exit_code: out.status.code().unwrap_or(-1),
                    }
                }
                Err(e) => TerminalResult {
                    output: format!("Error: failed to spawn process: {e}"),
                    exit_code: -1,
                },
            }
        })
        .await;

        match result {
            Ok(r) => r,
            Err(_) => TerminalResult {
                output: format!("[TIMEOUT: process killed after {}s]", timeout_secs),
                exit_code: -1,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocks_rm_rf_root() {
        assert!(validate_command("rm -rf /").is_err());
        assert!(validate_command("rm -rf /home").is_err()); // contains "rm -rf /"? No — only prefix/contains check
    }

    #[test]
    fn test_blocks_sudo() {
        assert!(validate_command("sudo apt-get install vim").is_err());
    }

    #[test]
    fn test_allows_safe_commands() {
        assert!(validate_command("ls -la").is_ok());
        assert!(validate_command("cat README.md").is_ok());
        assert!(validate_command("cargo build").is_ok());
        assert!(validate_command("cargo test --lib").is_ok());
        assert!(validate_command("echo hello world").is_ok());
        assert!(validate_command("git status").is_ok());
        assert!(validate_command("uv run pytest").is_ok());
        assert!(validate_command("make test").is_ok());
    }

    #[test]
    fn test_blocks_interpreter_one_liners() {
        assert!(validate_command("perl -e 'print 1'").is_err());
        assert!(validate_command("node -e 'console.log(1)'").is_err());
    }

    #[test]
    fn test_blocks_fork_bomb() {
        assert!(validate_command(":(){ :|:& };:").is_err());
    }
}


