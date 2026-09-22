use clap::{Parser, Subcommand};
use anyhow::Result;

mod cli;
mod engine;
mod operators;
mod tools;
mod api;
mod index;
mod config;
mod providers;
mod redact;
mod flow;

#[derive(Parser)]
#[command(name = "mithril")]
#[command(about = "Multi-model orchestration backend. Fellowship-based agent routing with Ollama/OpenAI/MCP API compatibility.", long_about = None)]
#[command(version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the HTTP server (Ollama + OpenAI + MCP compatible API)
    Serve {
        #[arg(short, long, default_value = "16180")]
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
    /// Download a GGUF model
    DownloadModel {
        #[arg(short, long, default_value = "qwen-1.5b")]
        model: String,
        #[arg(short, long)]
        list: bool,
    },
    /// Manage multi-agent fellowship orchestration
    Fellowship {
        #[arg(default_value = "status")]
        action: String,
    },
    /// List available fellowship configurations
    Fellowships,
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

    llama_cpp_2::send_logs_to_tracing(llama_cpp_2::LogOptions::default().with_logs_enabled(false));
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { port } => cli::serve::run(port).await,
        Commands::Config { action, key, value } => {
            cli::config::run(&action, key.as_deref(), value.as_deref()).await
        }
        Commands::McpStdio => cli::mcp_stdio::run().await,
        Commands::Scan => cli::scan::run().await,
        Commands::DownloadModel { model, list } => cli::download::run(&model, list).await,
        Commands::Fellowship { action } => cli::fellowship::run(&action).await,
        Commands::Fellowships => cli::fellowships::run().await,
        Commands::Init => cli::init::run().await,
    }
}
