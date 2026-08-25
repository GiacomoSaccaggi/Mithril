//! `mithril sessions` — list, show, and delete saved chat sessions.

use anyhow::Result;
use colored::Colorize;

use crate::session::{self, SharedSession};

pub async fn run(action: &str, session_id: Option<&str>) -> Result<()> {
    match action {
        "list" | "" => list_sessions(),
        "show" => show_session(session_id.ok_or_else(|| anyhow::anyhow!("Usage: mithril sessions show <id>"))?),
        "delete" => delete_session(session_id.ok_or_else(|| anyhow::anyhow!("Usage: mithril sessions delete <id>"))?),
        _ => {
            anyhow::bail!("Unknown action '{}'. Available: list, show, delete", action)
        }
    }
}

fn list_sessions() -> Result<()> {
    let sessions = session::list_sessions()?;

    if sessions.is_empty() {
        println!("{}", "(no sessions saved)".dimmed());
        println!("Start one with: {}", "mithril chat".cyan());
        return Ok(());
    }

    println!();
    println!("{}", "Saved sessions:".bold());
    println!();

    for s in &sessions {
        let id_short = &s.id[..8];
        println!(
            "  {} {}  {} msg  {}  {}",
            id_short.cyan(),
            s.provider_name.green(),
            format!("{:>3}", s.message_count).dimmed(),
            s.updated_at.format("%Y-%m-%d %H:%M").to_string().dimmed(),
            format!("mithril chat --session {}", s.id).dimmed()
        );
    }
    println!();

    Ok(())
}

fn show_session(id: &str) -> Result<()> {
    let session = SharedSession::load(id)?;
    let messages = session.snapshot();

    println!();
    println!("  {} {}", "Session:".bold(), session.id.cyan());
    println!("  Provider: {}", session.provider_name.green());
    println!("  Messages: {}", messages.len());
    println!("  Created:  {}", session.created_at.format("%Y-%m-%d %H:%M UTC"));
    println!();

    if messages.is_empty() {
        println!("{}", "  (no messages)".dimmed());
    } else {
        for (i, m) in messages.iter().enumerate() {
            let role = match m.role.as_str() {
                "user" => "you".green(),
                "assistant" => "bot".blue(),
                "system" => "sys".yellow(),
                _ => m.role.as_str().normal(),
            };
            let preview = if m.content.len() > 100 {
                format!("{}…", &m.content[..100])
            } else {
                m.content.clone()
            };
            println!("  {:>3}. [{}] {}", i + 1, role, preview);
        }
    }
    println!();
    Ok(())
}

fn delete_session(id: &str) -> Result<()> {
    session::delete_session(id)?;
    println!("  {} Deleted session {}", "✓".green(), id.dimmed());
    Ok(())
}
