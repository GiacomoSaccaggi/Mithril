//! `mithril fellowships` — list all available fellowship configurations.

use anyhow::Result;
use colored::Colorize;
use crate::flow::fellowship::{self, AgentType};

pub async fn run() -> Result<()> {
    let fellowships = fellowship::list_fellowships();

    if fellowships.is_empty() {
        println!("{} No fellowships found.", "⚠".yellow());
        println!();
        println!("Create one with: {}", "mithril fellowship init".cyan());
        println!("Or place YAML files in: {}", ".mithril/fellowships/".cyan());
        return Ok(());
    }

    println!("{}", "Available fellowships:".bold());
    println!();

    for (name, config) in &fellowships {
        let is_default = name == "default";
        let label = if is_default {
            format!("{} (default)", config.name).bold().to_string()
        } else {
            name.bold().to_string()
        };

        println!("  {} {}", "●".green(), label);

        if let Some(ref desc) = config.description {
            println!("    {}", desc.dimmed());
        }

        // Show agents summary
        let agents_str: Vec<String> = config.agents.iter().map(|a| {
            let provider = match a.agent_type {
                AgentType::Provider => a.provider.as_deref().unwrap_or("?").to_string(),
                AgentType::External => a.binary.as_deref().unwrap_or("?").to_string(),
            };
            format!("{} ({})", a.name, provider)
        }).collect();
        println!("    Agents: {}", agents_str.join(", ").dimmed());
        println!();
    }

    println!("{}", "Usage:".dimmed());
    println!("  {}        Start chat with default fellowship", "mithril chat".cyan());
    println!("  {} Start chat with named fellowship", "mithril chat <name>".cyan());

    Ok(())
}
