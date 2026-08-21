//! Interactive chat CLI — terminal frontend using fellowship orchestration.

use anyhow::Result;
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use crate::config::MithrilConfig;
use crate::flow::fellowship::{self, FellowshipConfig};
use crate::flow::orchestrator::Orchestrator;
use crate::cli::agent_loop::TraceMode;
use crate::providers::ChatMessage;
use crate::session::{SharedSession, FRONTEND_TERMINAL};

/// Run interactive chat using fellowship orchestration.
/// `fellowship_name`: which fellowship to use (default if None)
/// `session_id`: load an existing session
#[allow(unused_assignments)]
pub async fn run(fellowship_name: Option<&str>, session_id: Option<&str>) -> Result<()> {
    let config = MithrilConfig::load()?;
    
    // Load fellowship config
    let fellowship_config = fellowship::load_by_name(fellowship_name.unwrap_or("default"))?;

    // Load or create session
    let session = match session_id {
        Some(id) => {
            let s = SharedSession::load(id)?;
            println!("  Resumed session {}", id.cyan());
            s
        }
        None => SharedSession::new(&fellowship_config.name),
    };

    // Claim terminal frontend
    session.claim_frontend(FRONTEND_TERMINAL)?;

    // Create orchestrator
    let mut orchestrator = Orchestrator::new(fellowship_config.clone(), config.clone(), TraceMode::Inline);

    // Plan/Build mode — Build is default (all tools). Plan = read-only tools only.
    let mut plan_mode = false;

    // Set up tool registry for @file expansion
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());

    // Undo/Redo stack: each entry is a snapshot of messages before an agent run
    let mut undo_stack: Vec<Vec<ChatMessage>> = Vec::new();
    let mut redo_stack: Vec<Vec<ChatMessage>> = Vec::new();

    // Load project steering (MITHRIL.md + .mithril/steering/*.md)
    if session.snapshot().is_empty() {
        let steering = super::steering::load_steering(&cwd);
        let default_system = "You are Mithril, an AI coding assistant running in the user's terminal. You have access to tools for reading files, editing code, running commands, searching the web, and navigating the codebase. Use these tools proactively to help the user. When asked about files or code, USE the read_psi tool to read them. When asked to modify code, USE the edit_file tool. When asked to run something, USE the run_terminal tool. Always act — don't just describe what you would do.";
        if steering.is_empty() {
            session.push(ChatMessage::system(default_system));
        } else {
            session.push(ChatMessage::system(&format!("{}

{}", default_system, steering)));
        }
    }

    print_banner(&fellowship_config, &session.id);

    let mut rl = DefaultEditor::new()?;
    let history_path = dirs::home_dir()
        .map(|h| h.join(".mithril").join("chat_history.txt"));
    if let Some(ref path) = history_path {
        let _ = rl.load_history(path);
    }

    // Track how many messages were in the session when we started

    loop {
        let mode_label = if plan_mode { "PLAN".yellow() } else { "BUILD".green() };
        let prompt = format!("[{}] {} ", mode_label, ">".cyan().bold());
        let continuation_prompt = format!("{} ", "…".dimmed());

        match rl.readline(&prompt) {
            Ok(line) => {
                // Multiline: if line ends with \ keep reading
                let mut full_input = line.clone();
                while full_input.trim_end().ends_with('\\') {
                    // Remove trailing backslash and add newline
                    let trimmed = full_input.trim_end().strip_suffix('\\').unwrap_or(&full_input);
                    full_input = format!("{}\n", trimmed);
                    match rl.readline(&continuation_prompt) {
                        Ok(next_line) => full_input.push_str(&next_line),
                        Err(_) => break,
                    }
                }

                let input = full_input.trim();
                if input.is_empty() { continue; }
                let _ = rl.add_history_entry(input);

                if input.starts_with('/') {
                    // Handle /undo and /redo locally (they need access to the stacks)
                    if input == "/undo" {
                        if let Some(snapshot) = undo_stack.pop() {
                            // Save current state to redo stack
                            redo_stack.push(session.snapshot());
                            // Restore conversation to checkpoint
                            let mut msgs = session.messages.lock();
                            msgs.clear();
                            msgs.extend(snapshot);
                            drop(msgs);
                            let _ = session.save();
                            // Also undo file changes via shadow log
                            let shadow = crate::operators::shadow::ShadowOperator::new(".", 10);
                            let _ = shadow.undo_last_session();
                            println!("  {} Undone. Conversation and file changes reverted.", "↩️".bold());
                        } else {
                            println!("  {} Nothing to undo.", "⚠".yellow());
                        }
                        continue;
                    }
                    if input == "/redo" {
                        if let Some(snapshot) = redo_stack.pop() {
                            // Save current state to undo stack
                            undo_stack.push(session.snapshot());
                            // Restore conversation to redo point
                            let mut msgs = session.messages.lock();
                            msgs.clear();
                            msgs.extend(snapshot);
                            drop(msgs);
                            let _ = session.save();
                            println!("  {} Redone. Conversation restored.", "↪️".bold());
                        } else {
                            println!("  {} Nothing to redo.", "⚠".yellow());
                        }
                        continue;
                    }

                    if input == "/plan" {
                        plan_mode = true;
                        println!("  {} Mode: {} (read-only tools only)", "🔒".bold(), "PLAN".yellow().bold());
                        continue;
                    }
                    if input == "/build" {
                        plan_mode = false;
                        println!("  {} Mode: {} (all tools enabled)", "🔓".bold(), "BUILD".green().bold());
                        continue;
                    }

                    match handle_command(input, &config, &session).await {
                        CommandResult::Continue => continue,
                        CommandResult::Exit => break,

                    }
                }

                // Expand @file references: inject file content into the message
                let input = expand_file_references(input);
                let input = input.as_str();

                // Save checkpoint for undo (before the agent modifies anything)
                undo_stack.push(session.snapshot());
                redo_stack.clear(); // new action invalidates redo history

                // Send message
                let user_msg = ChatMessage::user(input);
                session.push(user_msg);

                println!();

                // Use orchestrator to handle the request
                orchestrator.plan_mode = plan_mode;
                match orchestrator.handle_request(input).await {
                    Ok(result) => {
                        // Print trace entries dimmed
                        for trace in &result.trace {
                            let trace_str = match trace {
                                crate::flow::orchestrator::TraceEntry::Entry { agent } => 
                                    format!("⚡ gguf → {}", agent),
                                crate::flow::orchestrator::TraceEntry::AgentStart { agent, provider } => 
                                    format!("▶ {} ({})", agent, provider),
                                crate::flow::orchestrator::TraceEntry::ToolCall { name, success, preview } => 
                                    format!("  {} {} → {}", if *success { "⚙" } else { "✗" }, name, preview),
                                crate::flow::orchestrator::TraceEntry::Delegation { from, to, task_preview } => 
                                    format!("🔀 {} → {}: {}", from, to, task_preview),
                                crate::flow::orchestrator::TraceEntry::GgufCall { task_preview } => 
                                    format!("⚙ → gguf: {}", task_preview),
                                crate::flow::orchestrator::TraceEntry::Done { agent } => 
                                    format!("✓ {} → DONE", agent),
                                crate::flow::orchestrator::TraceEntry::BudgetWarning { used, limit } => 
                                    format!("⚠ budget exhausted ({}/{})", used, limit),
                            };
                            eprintln!("\x1b[2m  {}\x1b[0m", trace_str);
                        }

                        // Print response normal
                        println!("{}", result.response);
                        
                        // Print summary dimmed
                        let tokens_str = result.tokens.total().display();
                        eprintln!("\x1b[2m  ✓ {} rounds | {}\x1b[0m", result.rounds, tokens_str);
                        println!();
                        
                        session.push(ChatMessage::assistant(&result.response));
                    }
                    Err(e) => {
                        eprintln!("  {} {}", "\x1b[31mError:\x1b[0m", e);
                        session.messages.lock().pop();
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("{}", "^C".dimmed());
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("{}", "Goodbye!".dimmed());
                break;
            }
            Err(err) => {
                eprintln!("Error reading input: {:?}", err);
                break;
            }
        }
    }

    // Always release frontend — even on Ctrl+D, panic unwind, or unexpected break
    session.release_frontend(FRONTEND_TERMINAL);
    let _ = session.save();

    if let Some(ref path) = history_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = rl.save_history(path);
    }

    Ok(())
}

// ── Command handling ─────────────────────────────────────────────────────────

enum CommandResult {
    Continue,
    Exit,
}

async fn handle_command(
    input: &str,
    config: &MithrilConfig,
    session: &SharedSession,
) -> CommandResult {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let command = parts.first().copied().unwrap_or("");
    let messages = &mut *session.messages.lock();

    match command {
        "/exit" | "/quit" | "/q" => {
            println!("{}", "Goodbye!".dimmed());
            CommandResult::Exit
        }

        "/clear" | "/c" => {
            messages.clear();
            let _ = session.save();
            println!("{}", "Conversation cleared.".dimmed());
            CommandResult::Continue
        }

        "/help" | "/h" | "/?" => {
            let _ = messages; // release lock before printing
            print_help();
            CommandResult::Continue
        }

        "/fellowship" | "/f" => {
            let _ = messages;
            let fellowships = crate::flow::fellowship::list_fellowships();
            println!("\n  {} Current: {}", "⚔️".bold(), config.default_provider.cyan());
            println!("\n  Available:");
            for (_key, cfg) in &fellowships {
                let desc = cfg.description.as_deref().unwrap_or("");
                println!("    {} {} — {}", "●".green(), cfg.name.cyan(), desc.dimmed());
            }
            println!("\n  {} Use: mithril chat <name> to switch\n", "💡".dimmed());
            CommandResult::Continue
        }

        "/session" => {
            let meta = session.meta();
            println!(
                "\n  {} Session ID:  {}\n  Messages:    {}\n  Frontend:    {}\n",
                "📋".bold(),
                meta.id.cyan(),
                meta.message_count,
                session.active_frontend_name().yellow()
            );
            CommandResult::Continue
        }



        "/history" => {
            if messages.is_empty() {
                println!("{}", "(no messages yet)".dimmed());
            } else {
                for (i, m) in messages.iter().enumerate() {
                    let role = match m.role.as_str() {
                        "user" => m.role.green(),
                        "assistant" => m.role.blue(),
                        "system" => m.role.yellow(),
                        _ => m.role.normal(),
                    };
                    let preview = if m.content.len() > 80 {
                        format!("{}…", &m.content[..80])
                    } else {
                        m.content.clone()
                    };
                    println!("{:2}. [{}] {}", i + 1, role, preview);
                }
            }
            CommandResult::Continue
        }













        "/compact" => {
            let msg_count = messages.len();
            let _ = messages; // release lock before async work
            if msg_count < 4 {
                println!("  {} Not enough messages to compact (need at least 4)", "⚠".yellow());
            } else {
                println!("  {} Compacting {} messages...", "📦".bold(), msg_count);
                let snap = session.snapshot();
                // Use the default provider from config for compaction
                let compact_provider = crate::providers::create_provider(
                    &config.default_provider, config
                );
                let compact_result = match compact_provider {
                    Ok(p) => super::compact::compact_history(p.as_ref(), &snap).await,
                    Err(e) => Err(e),
                };
                match compact_result {
                    Ok(summary) => {
                        let mut msgs = session.messages.lock();
                        super::compact::apply_compaction(&mut msgs, &summary);
                        let _ = session.save();
                        println!("  {} Compacted to {} messages. Context freed.", "✓".green(), msgs.len());
                    }
                    Err(e) => eprintln!("  {} Compaction failed: {}", "Error:".red(), e),
                }
            }
            CommandResult::Continue
        }

        _ => {
            println!("{} Unknown command: {}", "⚠️".yellow(), command);
            println!("Type {} for available commands", "/help".cyan());
            CommandResult::Continue
        }
    }
}


// ── Display helpers ──────────────────────────────────────────────────────────

fn print_banner(fellowship_config: &FellowshipConfig, session_id: &str) {
    println!();
    println!(
        "  {} {} │ Fellowship: {} │ Session: {}",
        "🗡️".bold(),
        "Mithril Chat".bold().cyan(),
        fellowship_config.name.green(),
        session_id[..8].dimmed()
    );
    if let Some(ref desc) = fellowship_config.description {
        println!("  {}", desc.dimmed());
    }
    let agents: Vec<_> = fellowship_config.agents.iter().map(|a| a.name.as_str()).collect();
    println!("  Agents: {}", agents.join(", ").dimmed());
    println!(
        "  Type {} for commands, {} for Telegram, {} to quit",
        "/help".cyan(),
        "/start-telegram".cyan(),
        "/exit".cyan()
    );
    println!();
}

fn print_help() {
    println!();
    println!("{}", "Commands:".bold());
    println!("  {}         Exit chat", "/exit, /q".cyan());
    println!("  {}        Clear conversation", "/clear, /c".cyan());
    println!("  {}       Compact conversation", "/compact".cyan());
    println!("  {}    Show/switch fellowship", "/fellowship".cyan());
    println!("  {}          Undo last action", "/undo".cyan());
    println!("  {}          Redo undone action", "/redo".cyan());
    println!("  {}          Plan mode (read-only)", "/plan".cyan());
    println!("  {}         Build mode (all tools)", "/build".cyan());
    println!("  {}       Show session info", "/session".cyan());
    println!("  {}       Show message history", "/history".cyan());
    println!("  {}          Show this help", "/help".cyan());
    println!();
    println!("{}", "Tips:".bold());
    println!("  {}  Attach file content to your message", "@path/to/file".cyan());
    println!("  {} Attach file with spaces in name", "@\"path/to/my file.rs\"".cyan());
    println!("  {} End line with \\ for multiline input", "\\".cyan());
    println!("  {} List available fellowships", "mithril fellowships".cyan());
    println!();
}

// ── @file reference expansion ────────────────────────────────────────────────

/// Expand `@path/to/file` references in the user's input.
/// Each @reference is replaced with a block containing the file's content.
/// Supports fuzzy matching: if the exact path doesn't exist, tries find_file.
pub(crate) fn expand_file_references(input: &str) -> String {
    use regex::Regex;

    // Supports @path/to/file and @"path with spaces/file.rs"
    let re = match Regex::new(r#"@"([^"]+)"|@([\w./_\-]+)"#) {
        Ok(r) => r,
        Err(_) => return input.to_string(),
    };

    if !re.is_match(input) {
        return input.to_string();
    }

    let mut expanded = input.to_string();
    let cwd = std::env::current_dir().unwrap_or_default();

    for cap in re.captures_iter(input) {
        let full_match = cap.get(0).unwrap().as_str();
        // Group 1 = quoted path, Group 2 = unquoted path
        let file_ref = cap.get(1)
            .or_else(|| cap.get(2))
            .unwrap()
            .as_str();

        // Try exact path first
        let file_path = cwd.join(file_ref);
        let content = if file_path.is_file() {
            std::fs::read_to_string(&file_path).ok()
        } else {
            // Fuzzy: walk project and find best match
            fuzzy_find_file(&cwd, file_ref)
        };

        if let Some(content) = content {
            let truncated = if content.len() > 8000 {
                format!("{}…\n[truncated, {} total chars]", &content[..8000], content.len())
            } else {
                content
            };

            let replacement = format!(
                "[File: {}]\n```\n{}\n```",
                file_ref, truncated
            );
            expanded = expanded.replacen(full_match, &replacement, 1);
            eprintln!("  {} Attached: {}", "📎".dimmed(), file_ref);
        } else {
            eprintln!("  {} File not found: {}", "⚠".yellow(), file_ref);
        }
    }

    expanded
}

/// Fuzzy find a file by name fragment in the project.
fn fuzzy_find_file(root: &std::path::Path, query: &str) -> Option<String> {
    use walkdir::WalkDir;

    let query_lower = query.to_lowercase();
    let ignore = ["target", "node_modules", ".git", "dist", "build", ".cache", "__pycache__", ".venv", ".idea", "tmp"];

    let mut best_match: Option<std::path::PathBuf> = None;
    let mut best_score = 0usize;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !ignore.contains(&e.file_name().to_str().unwrap_or("")))
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
        let rel_str = rel.to_string_lossy().to_lowercase();

        // Score: exact filename match > path contains > partial
        let score = if rel_str == query_lower {
            100
        } else if rel_str.ends_with(&query_lower) {
            80
        } else if rel.file_name().map(|n| n.to_string_lossy().to_lowercase().contains(&query_lower)).unwrap_or(false) {
            60
        } else if rel_str.contains(&query_lower) {
            40
        } else {
            0
        };

        if score > best_score {
            best_score = score;
            best_match = Some(entry.path().to_path_buf());
        }
    }

    best_match.and_then(|p| std::fs::read_to_string(p).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    // Test expand_file_references with inputs that don't require filesystem access
    #[test]
    fn test_expand_file_references_no_refs() {
        let input = "Hello, how are you?";
        let result = expand_file_references(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_expand_file_references_nonexistent_file() {
        // Non-existent files remain as-is
        let input = "Look at @nonexistent_xyz_12345.txt please";
        let result = expand_file_references(input);
        assert!(result.contains("@nonexistent_xyz_12345.txt"));
    }

    #[test]
    fn test_expand_file_references_preserves_surrounding_text() {
        // Even for non-existent files, surrounding text is preserved
        let input = "Before @nonexistent_abc.txt after";
        let result = expand_file_references(input);
        assert!(result.contains("Before"));
        assert!(result.contains("after"));
    }

    // Test fuzzy_find_file directly (doesn't depend on cwd)
    #[test]
    fn test_fuzzy_find_file_exact_match() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("exact.txt"), "found").unwrap();

        let result = fuzzy_find_file(dir.path(), "exact.txt");
        assert_eq!(result, Some("found".to_string()));
    }

    #[test]
    fn test_fuzzy_find_file_partial_match() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("my_module.rs"), "rust code").unwrap();

        let result = fuzzy_find_file(dir.path(), "module");
        assert_eq!(result, Some("rust code".to_string()));
    }

    #[test]
    fn test_fuzzy_find_file_case_insensitive() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("README.md"), "readme content").unwrap();

        let result = fuzzy_find_file(dir.path(), "readme");
        assert_eq!(result, Some("readme content".to_string()));
    }

    #[test]
    fn test_fuzzy_find_file_in_subdirectory() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "lib content").unwrap();

        let result = fuzzy_find_file(dir.path(), "lib.rs");
        assert_eq!(result, Some("lib content".to_string()));
    }

    #[test]
    fn test_fuzzy_find_file_deep_nested() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/deep/nested")).unwrap();
        fs::write(dir.path().join("src/deep/nested/file.rs"), "nested content").unwrap();

        let result = fuzzy_find_file(dir.path(), "file.rs");
        assert_eq!(result, Some("nested content".to_string()));
    }

    #[test]
    fn test_fuzzy_find_file_ignores_target() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("target/ignored.rs"), "should be ignored").unwrap();

        let result = fuzzy_find_file(dir.path(), "ignored.rs");
        assert!(result.is_none());
    }

    #[test]
    fn test_fuzzy_find_file_ignores_node_modules() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        fs::write(dir.path().join("node_modules/pkg.js"), "should be ignored").unwrap();

        let result = fuzzy_find_file(dir.path(), "pkg.js");
        assert!(result.is_none());
    }

    #[test]
    fn test_fuzzy_find_file_not_found() {
        let dir = tempdir().unwrap();
        let result = fuzzy_find_file(dir.path(), "nonexistent.xyz");
        assert!(result.is_none());
    }

    #[test]
    fn test_fuzzy_find_file_empty_query() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.txt"), "content").unwrap();

        // Empty query should match (contains empty string)
        let result = fuzzy_find_file(dir.path(), "");
        assert!(result.is_some());
    }

    #[test]
    fn test_fuzzy_find_file_special_chars() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("special.txt"), "content with <html> and `code`").unwrap();

        let result = fuzzy_find_file(dir.path(), "special");
        assert!(result.is_some());
        let content = result.unwrap();
        assert!(content.contains("<html>"));
        assert!(content.contains("`code`"));
    }

    #[test]
    fn test_fuzzy_find_file_large_content() {
        let dir = tempdir().unwrap();
        let large_content = "x".repeat(10000);
        fs::write(dir.path().join("large.txt"), &large_content).unwrap();

        let result = fuzzy_find_file(dir.path(), "large.txt");
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 10000);
    }

    #[test]
    fn test_fuzzy_find_file_best_match() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.txt"), "wrong").unwrap();
        fs::write(dir.path().join("test_module.txt"), "better").unwrap();

        // Should find files containing "module"
        let result = fuzzy_find_file(dir.path(), "module");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "better");
    }
}
