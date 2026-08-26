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
use crate::flow::TraceMode;
use crate::session::SharedSession;



/// Run the TUI chat interface with fellowship orchestration.
pub async fn run(
    fellowship_config: &FellowshipConfig,
    session: &SharedSession,
    config: &MithrilConfig,
) -> Result<()> {
    let _cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());

    // Track if this is a brand new session (for splash animation)
    let is_new_session = session.snapshot().is_empty();

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



    // Create centralized chat core
    let mut core = crate::cli::chat_core::ChatCore::new(
        fellowship_config.clone(), config.clone(), session.clone(), TraceMode::Silent
    );
    core.init_session();

    // Main loop
    let result = run_loop(
        &mut terminal, &mut app, &mut core, session,
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
    core: &mut crate::cli::chat_core::ChatCore,
    session: &SharedSession,
) -> Result<()> {
    use crate::cli::chat_core::ChatAction;

    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        let action = events::handle_events(app)?;

        match action {
            Action::None => {}
            Action::Exit => {
                app.should_exit = true;
                let _ = session.save();
                break;
            }
            Action::Submit(text) => {
                if app.thinking { continue; }

                app.push_message(Role::User, &text);
                app.thinking = true;
                terminal.draw(|frame| ui::render(frame, app))?;

                let result = core.process_message(&text).await;
                app.thinking = false;

                match result {
                    ChatAction::Response(orch_result) => {
                        app.current_round = orch_result.rounds;
                        app.max_rounds = core.orchestrator.max_rounds();

                        for trace in &orch_result.trace {
                            let (agent, detail) = match trace {
                                crate::flow::orchestrator::TraceEntry::Entry { agent } =>
                                    (agent.clone(), "entry".to_string()),
                                crate::flow::orchestrator::TraceEntry::AgentStart { agent, provider } =>
                                    (agent.clone(), format!("started ({})", provider)),
                                crate::flow::orchestrator::TraceEntry::ToolCall { name, success, preview } => {
                                    app.tool_call_count += 1;
                                    (name.clone(), format!("{} → {}", if *success { "✓" } else { "✗" }, preview))
                                },
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
                        }

                        let tokens_str = orch_result.tokens.total().display();
                        app.messages.push(app::ChatLine {
                            role: Role::Summary { rounds: orch_result.rounds, tokens: tokens_str },
                            content: String::new(),
                        });
                    }
                    ChatAction::Error(e) => {
                        app.push_message(Role::System, &format!("Error: {}", e));
                    }
                    _ => {}
                }

                let _ = session.save();
            }
            Action::Command(cmd) => {
                let result = core.execute_command(&cmd).await;
                match result {
                    ChatAction::Exit => {
                        app.should_exit = true;
                        let _ = session.save();
                        break;
                    }
                    ChatAction::Message(msg) => app.push_message(Role::System, &msg),
                    ChatAction::ModeChanged(is_plan) => {
                        app.mode = if is_plan { app::AgentMode::Plan } else { app::AgentMode::Build };
                        let msg = if is_plan { "🔒 Plan mode (read-only)" } else { "🔓 Build mode (all tools)" };
                        app.push_message(Role::System, msg);
                    }
                    ChatAction::Cleared => {
                        app.messages.clear();
                        app.scroll_offset = 0;
                    }
                    ChatAction::Undone(ok) => {
                        let msg = if ok { "↩️ Undone." } else { "⚠ Nothing to undo." };
                        app.push_message(Role::System, msg);
                    }
                    ChatAction::Redone(ok) => {
                        let msg = if ok { "↪️ Redone." } else { "⚠ Nothing to redo." };
                        app.push_message(Role::System, msg);
                    }
                    ChatAction::TelegramStarted => {
                        app.push_message(Role::System, "🤖 Telegram bot started (shared session)");
                    }
                    ChatAction::Error(e) => app.push_message(Role::System, &format!("Error: {}", e)),
                    ChatAction::Response(_) => {}
                    ChatAction::None => {}
                }
            }
        }

        if app.should_exit {
            break;
        }
    }

    Ok(())
}
