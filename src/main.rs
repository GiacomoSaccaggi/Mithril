use clap::{Parser, Subcommand};
use anyhow::Result;
use std::io::IsTerminal;

mod cli;
mod engine;
mod operators;
mod tools;
mod api;
mod index;
mod config;
mod providers;
mod session;
mod flow;
mod tui;

#[derive(Parser)]
#[command(name = "mithril")]
#[command(about = "Lightweight local LLM inference engine", long_about = None)]
#[command(version = "0.3.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP server (Ollama-compatible API)
    Serve {
        #[arg(short, long, default_value = "16180")]
        port: u16,
    },
    /// Run inference on a prompt
    Forge {
        prompt: String,
    },
    /// Interactive chat using fellowship orchestration
    Chat {
        /// Fellowship name to use (default: loads .mithril/fellowship.yaml)
        fellowship: Option<String>,
        /// Resume an existing session by ID
        #[arg(long)]
        session: Option<String>,
        /// Use full-screen TUI with panels and popups
        #[arg(long)]
        tui: bool,
        /// Skip all tool confirmation prompts (auto-approve everything)
        #[arg(long)]
        no_confirm: bool,
        /// Also start HTTP server in background (like mithril start)
        #[arg(long)]
        serve: bool,
        /// Port for the background HTTP server (only used with --serve)
        #[arg(long, default_value = "16180")]
        port: u16,
    },
    /// Manage configuration and credentials
    Config {
        #[arg(default_value = "list")]
        action: String,
        key: Option<String>,
        value: Option<String>,
    },
    /// Start MCP server over stdio
    McpStdio,
    /// Build the Palantír semantic index for the current directory
    Scan,
    /// Undo the last shadow log session
    Undo,
    /// Download a GGUF model
    DownloadModel {
        #[arg(short, long, default_value = "qwen-1.5b")]
        model: String,
        #[arg(short, long)]
        list: bool,
    },
    /// Start the Telegram bot frontend (continues or starts a session)
    Telegram {
        /// Resume an existing session by ID (optional)
        #[arg(long)]
        session: Option<String>,
    },
    /// Manage saved chat sessions
    Sessions {
        /// Action: list (default), show, delete
        #[arg(default_value = "list")]
        action: String,
        /// Session ID (required for show/delete)
        id: Option<String>,
    },
    /// Run a multi-agent flow on a message
    Flow {
        /// The message / task to process
        message: String,
        /// Path to flow config file (default: .mithril-flow.yaml)
        #[arg(long, short)]
        config: Option<String>,
    },
    /// Manage multi-agent fellowship orchestration
    Fellowship {
        #[arg(default_value = "status")]
        action: String,
    },
    /// List available fellowship configurations
    Fellowships,
    /// Run agentic task non-interactively (headless mode for CI/CD)
    Exec(cli::exec::ExecArgs),

    /// Initialize project: analyze codebase and generate MITHRIL.md steering file
    Init,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();


    // Suppress llama.cpp verbose logging (Metal pipeline compilation, graph allocation, etc.)
    // These go directly to stderr via C FFI and corrupt TUI/REPL output.
    llama_cpp_2::send_logs_to_tracing(llama_cpp_2::LogOptions::default().with_logs_enabled(false));
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { port } => cli::serve::run(port).await,
        Commands::Forge { prompt } => cli::forge::run(&prompt).await,
        Commands::Chat { fellowship, session, tui, no_confirm, serve, port } => {
            if no_confirm {
                std::env::set_var("MITHRIL_NO_CONFIRM", "1");
            }

            // Optionally start HTTP server in background
            if serve {
                let cwd = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();
                let _ = std::process::Command::new("sh")
                    .args(["-c", &format!("lsof -ti :{port} | xargs kill -9 2>/dev/null")])
                    .status();
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                tokio::spawn(async move {
                    if let Err(e) = crate::api::server::MithrilServer::start(port, &cwd).await {
                        eprintln!("Server error: {}", e);
                    }
                });
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                eprintln!("  Server running on http://localhost:{}", port);
            }

            if tui && std::io::stdout().is_terminal() {
                // TUI mode (full-screen with panels)
                let config = config::MithrilConfig::load()?;
                let fellowship_config = match fellowship.as_deref() {
                    Some(name) => crate::flow::fellowship::load_by_name(name)?,
                    None => crate::flow::fellowship::load_by_name("default")?,
                };
                let sess = match session.as_deref() {
                    Some(id) => crate::session::SharedSession::load(id)?,
                    None => crate::session::SharedSession::new(&fellowship_config.name),
                };
                tui::run(&fellowship_config, &sess, &config).await
            } else {
                // Plain REPL (default)
                cli::chat::run(fellowship.as_deref(), session.as_deref()).await
            }
        }
        Commands::Config { action, key, value } => {
            cli::config::run(&action, key.as_deref(), value.as_deref()).await
        }
        Commands::McpStdio => cli::mcp_stdio::run().await,
        Commands::Scan => cli::scan::run().await,
        Commands::Undo => cli::undo::run().await,
        Commands::DownloadModel { model, list } => cli::download::run(&model, list).await,
        Commands::Telegram { session } => {
            cli::telegram::run(session.as_deref()).await
        }
        Commands::Sessions { action, id } => {
            cli::sessions::run(&action, id.as_deref()).await
        }
        Commands::Flow { message, config } => {
            cli::flow::run(&message, config.as_deref()).await
        }
        Commands::Fellowship { action } => cli::fellowship::run(&action).await,
        Commands::Fellowships => cli::fellowships::run().await,
        Commands::Exec(args) => cli::exec::run(args).await,

        Commands::Init => cli::init::run().await,
    }
}
