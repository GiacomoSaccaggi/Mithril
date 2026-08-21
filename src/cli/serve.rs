use anyhow::Result;
use crate::api::server::MithrilServer;
use crate::engine::model_catalog::MODELS;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_banner(port: u16, cwd: &str) {
    const RESET: &str = "\x1b[0m";
    const BOLD: &str = "\x1b[1m";
    const DIM: &str = "\x1b[2m";
    const W: &str = "\x1b[38;5;255m";
    const S1: &str = "\x1b[38;5;189m";
    const S2: &str = "\x1b[38;5;153m";
    const B1: &str = "\x1b[38;5;111m";
    const B2: &str = "\x1b[38;5;75m";
    const P: &str = "\x1b[38;5;183m";
    const CYAN: &str = "\x1b[38;5;80m";
    const GREEN: &str = "\x1b[38;5;114m";
    const GRAY: &str = "\x1b[38;5;243m";

    // Clear screen
    print!("\x1b[2J\x1b[H");

    // Logo — all lines are exactly 71 chars between the ║ borders
    println!("{BOLD}{S1}        ╔═══════════════════════════════════════════════════════════════════╗{RESET}");
    println!("{BOLD}{S1}        ║{RESET}                                                                   {BOLD}{S1}║{RESET}");
    println!("{BOLD}{S1}        ║{RESET}   {BOLD}{W}███╗   ███╗{S1}██╗{S2}████████╗{B1}██╗  ██╗{B2}██████╗ {P}██╗{S1}██╗      {RESET}              {BOLD}{S1}║{RESET}");
    println!("{BOLD}{S1}        ║{RESET}   {BOLD}{W}████╗ ████║{S1}██║{S2}╚══██╔══╝{B1}██║  ██║{B2}██╔══██╗{P}██║{S1}██║      {RESET}              {BOLD}{S1}║{RESET}");
    println!("{BOLD}{S1}        ║{RESET}   {BOLD}{W}██╔████╔██║{S1}██║{S2}   ██║   {B1}███████║{B2}██████╔╝{P}██║{S1}██║      {RESET}              {BOLD}{S1}║{RESET}");
    println!("{BOLD}{S1}        ║{RESET}   {BOLD}{S1}██║╚██╔╝██║{S2}██║{S2}   ██║   {B1}██╔══██║{B2}██╔══██╗{P}██║{S1}██║      {RESET}              {BOLD}{S1}║{RESET}");
    println!("{BOLD}{S1}        ║{RESET}   {BOLD}{S2}██║ ╚═╝ ██║{S2}██║{B1}   ██║   {B1}██║  ██║{B2}██║  ██║{P}██║{S1}███████╗ {RESET}              {BOLD}{S1}║{RESET}");
    println!("{BOLD}{S1}        ║{RESET}   {BOLD}{GRAY}╚═╝     ╚═╝{GRAY}╚═╝{GRAY}   ╚═╝   {GRAY}╚═╝  ╚═╝{GRAY}╚═╝  ╚═╝{GRAY}╚═╝{GRAY}╚══════╝ {RESET}              {BOLD}{S1}║{RESET}");
    println!("{BOLD}{S1}        ║{RESET}                                                                   {BOLD}{S1}║{RESET}");
    println!("{BOLD}{S1}        ║{RESET}   {P}✦{RESET} {DIM}\"Light as a feather, and as hard as dragon-scales\"{RESET}          {BOLD}{S1}║{RESET}");
    println!("{BOLD}{S1}        ║{RESET}                                                                   {BOLD}{S1}║{RESET}");
    println!("{BOLD}{S1}        ╚═══════════════════════════════════════════════════════════════════╝{RESET}");
    println!();
    println!("        {GREEN}●{RESET} {BOLD}v{VERSION}{RESET}  {DIM}│{RESET}  {DIM}Lightweight LLM Inference Engine{RESET}");
    println!("        {DIM}📁 {cwd}{RESET}");
    println!();
    println!("        {DIM}┌─────────────────────────────────────────────────────────┐{RESET}");
    println!("        {DIM}│{RESET}  {BOLD}Endpoints{RESET}                                                {DIM}│{RESET}");
    println!("        {DIM}│{RESET}                                                          {DIM}│{RESET}");
    println!("        {DIM}│{RESET}    Ollama   {CYAN}http://localhost:{port}/api/chat{RESET}               {DIM}│{RESET}");
    println!("        {DIM}│{RESET}    OpenAI   {CYAN}http://localhost:{port}/v1/chat/completions{RESET}    {DIM}│{RESET}");
    println!("        {DIM}│{RESET}    MCP      {CYAN}http://localhost:{port}/mcp{RESET}                    {DIM}│{RESET}");
    println!("        {DIM}│{RESET}    Health   {CYAN}http://localhost:{port}/health{RESET}                 {DIM}│{RESET}");
    println!("        {DIM}└─────────────────────────────────────────────────────────┘{RESET}");
    println!();
}

pub async fn run(port: u16) -> Result<()> {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let model_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".mithril/models");

    let has_model = MODELS.iter().any(|m| model_dir.join(m.file_name).exists());

    print_banner(port, &cwd);

    if !has_model {
        println!("        \x1b[38;5;214m⚠  No local model found.\x1b[0m");
        println!("        \x1b[2mCloud providers (Gemini, OpenAI, Anthropic) still work.\x1b[0m");
        println!("        \x1b[2mTo add a local model: \x1b[38;5;80mmithril download-model --model qwen-1.5b\x1b[0m");
        println!();
    }

    // Start server — auto-kill if port busy
    if let Err(e) = MithrilServer::start(port, &cwd).await {
        if e.to_string().contains("Address already in use") {
            eprintln!("  \x1b[38;5;214m⚠  Port {} busy — killing existing process...\x1b[0m", port);
            let _ = std::process::Command::new("sh")
                .args(["-c", &format!("lsof -ti :{port} | xargs kill -9 2>/dev/null")])
                .status();
            std::thread::sleep(std::time::Duration::from_millis(500));
            eprintln!("  \x1b[38;5;114m●  Retrying...\x1b[0m\n");
            MithrilServer::start(port, &cwd).await?;
        } else {
            return Err(e);
        }
    }
    Ok(())
}
