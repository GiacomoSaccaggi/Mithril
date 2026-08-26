//! Interactive chat REPL — terminal frontend using fellowship orchestration.
//! Delegates all logic to `chat_core::ChatCore`.

use anyhow::Result;
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Context, Helper};

use crate::config::MithrilConfig;
use crate::flow::fellowship::{self, FellowshipConfig};
use crate::flow::TraceMode;
use crate::session::{SharedSession, FRONTEND_TERMINAL};

use super::chat_core::{self, ChatCore, ChatAction, COMMANDS};

// ── Rustyline Helper (Tab-completion) ────────────────────────────────────────

struct MithrilHelper {
    agent_names: Vec<String>,
}

impl Helper for MithrilHelper {}
impl Highlighter for MithrilHelper {}
impl Validator for MithrilHelper {}
impl Hinter for MithrilHelper {
    type Hint = String;
}

impl Completer for MithrilHelper {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        let text = &line[..pos];

        // /command completion
        if text.starts_with('/') && !text.contains(' ') {
            let matches: Vec<Pair> = COMMANDS.iter()
                .filter(|(cmd, _)| cmd.starts_with(text))
                .map(|(cmd, desc)| Pair {
                    display: format!("{:<14} {}", cmd, desc),
                    replacement: cmd.to_string(),
                })
                .collect();
            return Ok((0, matches));
        }

        // #agent completion
        if text.starts_with('#') && !text.contains(' ') {
            let prefix = &text[1..];
            let matches: Vec<Pair> = self.agent_names.iter()
                .filter(|name| name.to_lowercase().starts_with(&prefix.to_lowercase()))
                .map(|name| Pair {
                    display: format!("#{}", name),
                    replacement: format!("#{} ", name),
                })
                .collect();
            return Ok((0, matches));
        }

        // @file completion
        if let Some(at_pos) = text.rfind('@') {
            if at_pos == 0 || text.as_bytes()[at_pos - 1].is_ascii_whitespace() {
                let prefix = &text[at_pos + 1..];
                let (dir, file_prefix) = if let Some(slash) = prefix.rfind('/') {
                    (&prefix[..slash + 1], &prefix[slash + 1..])
                } else {
                    ("", prefix)
                };
                let search_dir = if dir.is_empty() { "." } else { dir };
                if let Ok(entries) = std::fs::read_dir(search_dir) {
                    let matches: Vec<Pair> = entries.flatten()
                        .filter_map(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            if name.starts_with('.') { return None; }
                            if !name.to_lowercase().starts_with(&file_prefix.to_lowercase()) {
                                return None;
                            }
                            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                            let (display, replacement) = if is_dir {
                                (format!("{}/", name), format!("@{}{}/", dir, name))
                            } else {
                                (name.clone(), format!("@{}{} ", dir, name))
                            };
                            Some(Pair { display, replacement })
                        })
                        .take(15)
                        .collect();
                    return Ok((at_pos, matches));
                }
            }
        }

        Ok((0, vec![]))
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Run interactive REPL using fellowship orchestration.
pub async fn run(fellowship_name: Option<&str>, session_id: Option<&str>) -> Result<()> {
    let config = MithrilConfig::load()?;
    let fellowship_config = fellowship::load_by_name(fellowship_name.unwrap_or("default"))?;

    let session = match session_id {
        Some(id) => {
            let s = SharedSession::load(id)?;
            println!("  Resumed session {}", id.cyan());
            s
        }
        None => SharedSession::new(&fellowship_config.name),
    };

    session.claim_frontend(FRONTEND_TERMINAL)?;

    // Create centralized chat core
    let mut core = ChatCore::new(fellowship_config.clone(), config, session.clone(), TraceMode::Inline);
    core.init_session();

    print_banner(&fellowship_config);

    // Setup readline with completion
    let rl_config = Config::builder()
        .completion_type(CompletionType::List)
        .build();
    let mut rl = Editor::with_config(rl_config)?;
    rl.set_helper(Some(MithrilHelper { agent_names: core.agent_names() }));

    let history_path = dirs::home_dir().map(|h| h.join(".mithril").join("chat_history.txt"));
    if let Some(ref path) = history_path {
        let _ = rl.load_history(path);
    }

    loop {
        let prompt = if core.plan_mode {
            format!("{} {} ", "⚔ plan".yellow().bold(), "›".dimmed())
        } else {
            format!("{} {} ", "⚔ mithril".cyan().bold(), "›".dimmed())
        };
        let continuation_prompt = format!("  {} ", "·".dimmed());

        match rl.readline(&prompt) {
            Ok(line) => {
                // Multiline: if line ends with \ keep reading
                let mut full_input = line.clone();
                while full_input.trim_end().ends_with('\\') {
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
                    match core.execute_command(input).await {
                        ChatAction::Exit => break,
                        ChatAction::Message(msg) => println!("\n  {}\n", msg),
                        ChatAction::ModeChanged(is_plan) => {
                            if is_plan {
                                println!("  {} Mode: {} (read-only tools only)", "🔒".bold(), "PLAN".yellow().bold());
                            } else {
                                println!("  {} Mode: {} (all tools enabled)", "🔓".bold(), "BUILD".green().bold());
                            }
                        }
                        ChatAction::Cleared => println!("  {}", "Conversation cleared.".dimmed()),
                        ChatAction::Undone(ok) => {
                            if ok { println!("  {} Undone.", "↩️".bold()); }
                            else { println!("  {} Nothing to undo.", "⚠".yellow()); }
                        }
                        ChatAction::Redone(ok) => {
                            if ok { println!("  {} Redone.", "↪️".bold()); }
                            else { println!("  {} Nothing to redo.", "⚠".yellow()); }
                        }
                        ChatAction::TelegramStarted => {
                            println!("  {} Telegram bot started (shared session)", "🤖".bold());
                        }
                        ChatAction::Error(e) => eprintln!("  {} {}", "Error:".red(), e),
                        ChatAction::Response(_) => {} // shouldn't happen from commands
                        ChatAction::None => {}
                    }
                    continue;
                }

                // Process user message
                println!();
                match core.process_message(input).await {
                    ChatAction::Response(result) => {
                        // Print traces dimmed
                        for trace in &result.trace {
                            eprintln!("\x1b[2m  {}\x1b[0m", chat_core::format_trace(trace));
                        }
                        // Print response
                        println!("{}", chat_core::strip_markdown(&result.response));
                        // Print summary dimmed
                        let tokens_str = result.tokens.total().display();
                        eprintln!("\x1b[2m  ✓ {} rounds | {}\x1b[0m", result.rounds, tokens_str);
                        println!();
                    }
                    ChatAction::Error(e) => {
                        eprintln!("  \x1b[31mError:\x1b[0m {}", e);
                    }
                    _ => {}
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
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

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

// ── Display ──────────────────────────────────────────────────────────────────

fn print_banner(fellowship_config: &FellowshipConfig) {
    let banner = r#"
    ███╗   ███╗██╗████████╗██╗  ██╗██████╗ ██╗██╗
    ████╗ ████║██║╚══██╔══╝██║  ██║██╔══██╗██║██║
    ██╔████╔██║██║   ██║   ███████║██████╔╝██║██║
    ██║╚██╔╝██║██║   ██║   ██╔══██║██╔══██╗██║██║
    ██║ ╚═╝ ██║██║   ██║   ██║  ██║██║  ██║██║███████╗
    ╚═╝     ╚═╝╚═╝   ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝
"#;
    println!("{}", banner.bright_yellow());

    let agents: Vec<_> = fellowship_config.agents.iter()
        .map(|a| format!("{} ({})", a.name, a.provider.as_deref().unwrap_or("local")))
        .collect();

    println!("  {} Fellowship: {}", "🤖".bold(), fellowship_config.name.green());
    println!("     {}", agents.join(" • ").dimmed());
    if let Some(ref desc) = fellowship_config.description {
        println!("  {} {}", "📋".bold(), desc.dimmed());
    }
    println!("  {} Mode: {}", "🛡️".bold(), "BUILD (all tools)".green());
    println!();
    println!("  Type {} for commands, {} to quit", "/help".cyan(), "/exit".cyan());
    println!("  {}", "━".repeat(50).dimmed());
    println!();
}
