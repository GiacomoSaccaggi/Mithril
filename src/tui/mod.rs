//! Mithril TUI — full terminal user interface built with ratatui.
//!
//! Non-blocking architecture: agent loop runs in a background task,
//! communicates results via mpsc channel. The render loop stays responsive.

pub mod app;
pub mod events;
pub mod splash;
pub mod theme;
pub mod ui;

use std::io;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{App, Role};
use events::Action;

use crate::config::MithrilConfig;
use crate::flow::fellowship::FellowshipConfig;
use crate::flow::orchestrator::Orchestrator;
use crate::cli::agent_loop::TraceMode;
use crate::providers::ChatMessage;
use crate::session::SharedSession;



/// Run the TUI chat interface with fellowship orchestration.
pub async fn run(
    fellowship_config: &FellowshipConfig,
    session: &SharedSession,
    config: &MithrilConfig,
) -> Result<()> {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());

    // Track if this is a brand new session (for splash animation)
    let is_new_session = session.snapshot().is_empty();

    // Load steering + default system prompt
    if session.snapshot().is_empty() {
        let steering = crate::cli::steering::load_steering(&cwd);
        let default_system = "You are Mithril, an AI coding assistant running in the user's terminal. You have access to tools for reading files, editing code, running commands, searching the web, and navigating the codebase. Use these tools proactively to help the user. When asked about files or code, USE the read_psi tool to read them. When asked to modify code, USE the edit_file tool. When asked to run something, USE the run_terminal tool. Always act — don't just describe what you would do.";
        if steering.is_empty() {
            session.push(ChatMessage::system(default_system));
        } else {
            session.push(ChatMessage::system(&format!("{}

{}", default_system, steering)));
        }
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Play startup animation (only for new sessions)
    if is_new_session {
        splash::play_splash(&mut terminal);
        // Clear screen completely before entering the chat UI
        terminal.clear()?;
    }

    // Create app state with fellowship name
    let mut app = App::new(&fellowship_config.name, "fellowship", &session.id);

    // Load existing session messages into display
    for msg in session.snapshot() {
        match msg.role.as_str() {
            "user" => app.push_message(Role::User, &msg.content),
            "assistant" => app.push_message(Role::Assistant, &msg.content),
            _ => {} // don't display system/tool messages
        }
    }



    // Create orchestrator
    let mut orchestrator = Orchestrator::new(fellowship_config.clone(), config.clone(), TraceMode::Silent);

    // Main loop
    let result = run_loop(
        &mut terminal, &mut app, &mut orchestrator, session,
    ).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    orchestrator: &mut Orchestrator,
    session: &SharedSession,
) -> Result<()> {
    // Undo/Redo stacks for multi-step conversation restore
    let mut undo_stack: Vec<Vec<ChatMessage>> = Vec::new();
    let mut redo_stack: Vec<Vec<ChatMessage>> = Vec::new();

    loop {
        // Render
        terminal.draw(|frame| ui::render(frame, app))?;



        // Handle keyboard/mouse events (non-blocking, 50ms poll)
        let action = events::handle_events(app)?;

        match action {
            Action::None => {}
            Action::Exit => {
                app.should_exit = true;
                let _ = session.save();
                break;
            }
            Action::Submit(text) => {
                if app.thinking {
                    continue; // ignore input while agent is working
                }

                // Expand @file references (same as REPL)
                let text = crate::cli::chat::expand_file_references(&text);

                // Save checkpoint for undo
                undo_stack.push(session.snapshot());
                redo_stack.clear();

                // Add user message to display and session
                app.push_message(Role::User, &text);
                session.push(ChatMessage::user(&text));
                app.thinking = true;

                // Re-render NOW so user sees their message + thinking indicator
                terminal.draw(|frame| ui::render(frame, app))?;

                // Use orchestrator to handle the request
                let result = orchestrator.handle_request(&text).await;

                app.thinking = false;

                match result {
                    Ok(orch_result) => {
                        // Update round tracking
                        app.current_round = orch_result.rounds;
                        app.max_rounds = orchestrator.max_rounds();

                        // Show trace entries
                        for trace in &orch_result.trace {
                            let (agent, detail) = match trace {
                                crate::flow::orchestrator::TraceEntry::Entry { agent } => 
                                    (agent.clone(), "entry".to_string()),
                                crate::flow::orchestrator::TraceEntry::AgentStart { agent, provider } => 
                                    (agent.clone(), format!("started ({})", provider)),
                                crate::flow::orchestrator::TraceEntry::ToolCall { name, success, preview } => 
                                    (name.clone(), format!("{} → {}", if *success { "✓" } else { "✗" }, preview)),
                                crate::flow::orchestrator::TraceEntry::Delegation { from, to, task_preview } => 
                                    (from.clone(), format!("→ {} : {}", to, task_preview)),
                                crate::flow::orchestrator::TraceEntry::GgufCall { task_preview } => 
                                    ("gguf".to_string(), task_preview.clone()),
                                crate::flow::orchestrator::TraceEntry::Done { agent } => 
                                    (agent.clone(), "DONE".to_string()),
                                crate::flow::orchestrator::TraceEntry::BudgetWarning { used, limit } => 
                                    ("⚠".to_string(), format!("budget {}/{}", used, limit)),
                            };
                            app.messages.push(app::ChatLine {
                                role: Role::AgentTrace { agent, detail },
                                content: String::new(),
                            });
                        }
                        
                        if !orch_result.response.is_empty() {
                            app.push_message(Role::Assistant, &orch_result.response);
                            session.push(ChatMessage::assistant(&orch_result.response));
                        }

                        // Push summary
                        let tokens_str = orch_result.tokens.total().display();
                        app.messages.push(app::ChatLine {
                            role: Role::Summary { rounds: orch_result.rounds, tokens: tokens_str },
                            content: String::new(),
                        });
                    }
                    Err(e) => {
                        app.push_message(Role::System, &format!("Error: {}", e));
                    }
                }

                let _ = session.save();
            }
            Action::Command(cmd) => {
                // Handle /undo and /redo locally (need access to stacks)
                if cmd == "/undo" {
                    if let Some(snapshot) = undo_stack.pop() {
                        redo_stack.push(session.snapshot());
                        let mut msgs = session.messages.lock();
                        msgs.clear();
                        msgs.extend(snapshot);
                        drop(msgs);
                        let _ = session.save();
                        let shadow = crate::operators::shadow::ShadowOperator::new(".", 10);
                        let _ = shadow.undo_last_session();
                        app.push_message(Role::System, "↩️ Undone. Conversation and file changes reverted.");
                    } else {
                        app.push_message(Role::System, "⚠ Nothing to undo.");
                    }
                } else if cmd == "/redo" {
                    if let Some(snapshot) = redo_stack.pop() {
                        undo_stack.push(session.snapshot());
                        let mut msgs = session.messages.lock();
                        msgs.clear();
                        msgs.extend(snapshot);
                        drop(msgs);
                        let _ = session.save();
                        app.push_message(Role::System, "↪️ Redone. Conversation restored.");
                    } else {
                        app.push_message(Role::System, "⚠ Nothing to redo.");
                    }
                } else if cmd == "/compact" {
                    let snap = session.snapshot();
                    if snap.len() < 4 {
                        app.push_message(Role::System, "⚠ Not enough messages to compact (need at least 4).");
                    } else {
                        app.push_message(Role::System, &format!("📦 Compacting {} messages...", snap.len()));
                        let config = crate::config::MithrilConfig::load().unwrap_or_default();
                        let compact_result = match crate::providers::create_provider(&config.default_provider, &config) {
                            Ok(p) => crate::cli::compact::compact_history(p.as_ref(), &snap).await,
                            Err(e) => Err(e),
                        };
                        match compact_result {
                            Ok(summary) => {
                                let mut msgs = session.messages.lock();
                                crate::cli::compact::apply_compaction(&mut msgs, &summary);
                                drop(msgs);
                                let _ = session.save();
                                app.push_message(Role::System, "✓ History compacted. Context freed.");
                            }
                            Err(e) => {
                                app.push_message(Role::System, &format!("✗ Compaction failed: {}", e));
                            }
                        }
                    }
                } else {
                    handle_tui_command(app, &cmd, session);
                }
            }
        }

        if app.should_exit {
            break;
        }
    }

    Ok(())
}

fn handle_tui_command(app: &mut App, cmd: &str, session: &SharedSession) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    match parts.first().copied() {
        Some("/exit") | Some("/q") => {
            app.should_exit = true;
        }
        Some("/clear") => {
            app.messages.clear();
            app.scroll_offset = 0;
        }
        Some("/fellowship") => {
            // Show current fellowship or list available
            let fellowships = crate::flow::fellowship::list_fellowships();
            let mut info = format!("Current: {}\n\nAvailable:\n", app.fellowship_name);
            for (name, config) in &fellowships {
                let desc = config.description.as_deref().unwrap_or("");
                let marker = if *name == "default" && app.fellowship_name == config.name { "●" } else { "○" };
                info.push_str(&format!("  {} {} — {}\n", marker, config.name, desc));
            }
            info.push_str("\nUse: mithril chat <name> to switch");
            app.push_message(Role::System, &info);
        }
        Some("/help") | Some("/?") => {
            app.push_message(Role::System,
                "Commands:\n\
                 /exit        Exit chat\n\
                 /clear       Clear conversation\n\
                 /compact     Compress history\n\
                 /fellowship  Show/switch fellowship\n\
                 /undo        Undo last action\n\
                 /redo        Redo undone action\n\
                 /session     Show session info\n\
                 /history     Show messages\n\
                 /help        This help\n\n\
                 Keys: Shift+Enter=newline Tab=Plan/Build ctrl+s=sidebar ctrl+c=exit\n\
                 Tips: @file injects file content into prompt"
            );
        }
        Some("/session") => {
            let meta = session.meta();
            app.push_message(Role::System, &format!(
                "Session: {}\nFellowship: {}\nMessages: {}\nFrontend: {}",
                meta.id, app.fellowship_name, meta.message_count,
                session.active_frontend_name()
            ));
        }
        Some("/compact") => {
            // Handled in run_loop (async context). This shouldn't be reached.
            app.push_message(Role::System, "Compacting...");
        }
        Some("/history") => {
            let msgs = session.snapshot();
            if msgs.is_empty() {
                app.push_message(Role::System, "(no messages yet)");
            } else {
                let mut history = String::new();
                for (i, m) in msgs.iter().enumerate() {
                    let role = match m.role.as_str() {
                        "user" => "user",
                        "assistant" => "asst",
                        "system" => "sys",
                        _ => "?",
                    };
                    let preview = if m.content.len() > 60 {
                        format!("{}...", &m.content[..60])
                    } else {
                        m.content.clone()
                    };
                    history.push_str(&format!("{}. [{}] {}\n", i + 1, role, preview));
                }
                app.push_message(Role::System, &history);
            }
        }
        _ => {
            app.push_message(Role::System, &format!("Unknown command: {}", cmd));
        }
    }
}
