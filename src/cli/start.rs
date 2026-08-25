//! `mithril start` — prints a beautiful ASCII banner, starts HTTP server
//! in background, then runs the full plain REPL (same features as `mithril chat --plain`).

use anyhow::Result;
use colored::Colorize;

const BANNER: &str = r#"
    ╔══════════════════════════════════════════════════════════════╗
    ║                                                              ║
    ║   ███╗   ███╗██╗████████╗██╗  ██╗██████╗ ██╗██╗             ║
    ║   ████╗ ████║██║╚══██╔══╝██║  ██║██╔══██╗██║██║             ║
    ║   ██╔████╔██║██║   ██║   ███████║██████╔╝██║██║             ║
    ║   ██║╚██╔╝██║██║   ██║   ██╔══██║██╔══██╗██║██║             ║
    ║   ██║ ╚═╝ ██║██║   ██║   ██║  ██║██║  ██║██║███████╗       ║
    ║   ╚═╝     ╚═╝╚═╝   ╚═╝   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝       ║
    ║                                                              ║
    ║   Lightweight LLM Inference Engine                           ║
    ║                                                              ║
    ╚══════════════════════════════════════════════════════════════╝
"#;

pub async fn run(port: u16) -> Result<()> {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // Print the banner
    println!("{}", BANNER.bright_yellow());

    // Kill any previous process on the same port
    let _ = std::process::Command::new("sh")
        .args(["-c", &format!("lsof -ti :{port} | xargs kill -9 2>/dev/null")])
        .status();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Start HTTP server in a background tokio task
    let cwd_for_server = cwd.clone();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    tokio::spawn(async move {
        match tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await {
            Ok(probe) => {
                drop(probe);
                let _ = ready_tx.send(Ok(()));
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e.to_string()));
                return;
            }
        }
        if let Err(e) = crate::api::server::MithrilServer::start(port, &cwd_for_server).await {
            eprintln!("  {} Server stopped: {}", "⚠".yellow(), e);
        }
    });

    // Wait for server readiness (max 5s)
    match tokio::time::timeout(std::time::Duration::from_secs(5), ready_rx).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => anyhow::bail!("Cannot start server on port {port}: {e}"),
        Ok(Err(_)) => anyhow::bail!("Server task died unexpectedly"),
        Err(_) => anyhow::bail!("Server did not start within 5 seconds"),
    }

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Load fellowship config
    let fellowship_config = match crate::flow::fellowship::load_by_name("default") {
        Ok(config) => config,
        Err(e) => {
            eprintln!("  {} No fellowship found: {}", "⚠".yellow(), e);
            eprintln!("  Run {} to create one.", "mithril fellowship init".cyan());
            return Err(e);
        }
    };

    // Print status info below the banner
    println!("  {} {}", "▸ Server".dimmed(), format!("http://localhost:{port}").green());
    println!("  {} {}", "▸ Fellowship".dimmed(), fellowship_config.name.cyan());
    if let Some(ref desc) = fellowship_config.description {
        println!("  {} {}", "▸".dimmed(), desc.dimmed());
    }
    let agents: Vec<_> = fellowship_config.agents.iter()
        .map(|a| format!("{} ({})", a.name, a.provider.as_deref().unwrap_or("local")))
        .collect();
    println!("  {} {}", "▸ Agents".dimmed(), agents.join(", ").dimmed());
    println!();
    println!("  {} Type {} to quit, {} for help", "›".dimmed(), "/exit".cyan(), "/help".cyan());
    println!("  {}", "─".repeat(60).dimmed());
    println!();

    // Run the same plain REPL as `mithril chat --plain`
    crate::cli::chat::run(None, None).await
}
