//! Fellowship CLI — manage multi-agent orchestration.

use anyhow::Result;
use colored::Colorize;

use crate::flow::fellowship::{self, FellowshipConfig, AgentType};

pub async fn run(action: &str) -> Result<()> {
    match action {
        "status" => status().await,
        "init" => init().await,
        "test" => test_agents().await,
        _ => {
            println!("{}", "Fellowship — Multi-Agent Orchestration".bold());
            println!();
            println!("Usage: mithril fellowship <action>");
            println!();
            println!("Actions:");
            println!("  {}    Show configured agents and status", "status".cyan());
            println!("  {}      Create fellowship.yaml template", "init".cyan());
            println!("  {}      Test connectivity to each agent", "test".cyan());
            println!();
            Ok(())
        }
    }
}

async fn status() -> Result<()> {
    match FellowshipConfig::try_load() {
        Some(config) => {
            println!("{} {}", "⚔️  Fellowship:".bold(), config.name.cyan());
            if let Some(ref desc) = config.description {
                println!("  {}", desc.dimmed());
            }
            println!();

            // Controller (GGUF entry classifier + worker)
            println!("{}", "Controller (GGUF):".bold());
            println!("  Provider: {}", config.controller.provider.green());
            println!("  Model:    {}", config.controller.model.as_deref().unwrap_or("(default)").green());
            println!("  Context:  {} rounds", config.controller.context_window);
            println!();

            // Safety limits
            println!("{}", "Safety Limits:".bold());
            println!("  Max rounds:    {}", config.max_rounds);
            if let Some(budget) = config.token_budget {
                println!("  Token budget:  {}", budget);
            } else {
                println!("  Token budget:  (unlimited)");
            }
            println!();

            // Per-agent breakdown
            println!("{}", "Agents:".bold());
            println!("{}", "─".repeat(60).dimmed());

            for (i, agent) in config.agents.iter().enumerate() {
                let type_badge = match agent.agent_type {
                    AgentType::Provider => "API".blue(),
                    AgentType::External => "CLI".yellow(),
                };

                // Agent header
                println!();
                println!("  {} {} [{}]", 
                    format!("{}.", i + 1).dimmed(),
                    agent.name.bold(),
                    type_badge
                );

                // Role
                println!("     {}", agent.role.dimmed());
                println!();

                // Model info
                match &agent.agent_type {
                    AgentType::Provider => {
                        let prov = agent.provider.as_deref().unwrap_or("?");
                        let model = agent.model.as_deref().unwrap_or("(default)");
                        println!("     {} {} / {}", "Model:".dimmed(), prov.green(), model.cyan());
                    }
                    AgentType::External => {
                        let bin = agent.binary.as_deref().unwrap_or("?");
                        let args = agent.args.as_ref()
                            .map(|a| a.join(" "))
                            .unwrap_or_default();
                        println!("     {} {} {}", "Binary:".dimmed(), bin.green(), args.dimmed());
                    }
                }

                // When (classifier hint)
                if let Some(ref when) = agent.when {
                    println!("     {} {}", "When:".dimmed(), when.dimmed());
                }

                // Can call
                if !agent.can_call.is_empty() {
                    println!("     {} {}", "Can call:".dimmed(), agent.can_call.join(", ").cyan());
                } else {
                    println!("     {} (terminal agent)", "Can call:".dimmed());
                }

                // Tools
                if let Some(ref tools) = agent.tools {
                    if tools.len() == 1 && tools[0] == "*" {
                        println!("     {} all (full access)", "Tools:".dimmed());
                    } else if tools.is_empty() {
                        println!("     {} none (no tools)", "Tools:".dimmed());
                    } else {
                        let tool_str = if tools.len() > 5 {
                            format!("{}, ... ({} total)", tools[..5].join(", "), tools.len())
                        } else {
                            tools.join(", ")
                        };
                        println!("     {} {}", "Tools:".dimmed(), tool_str);
                    }
                }
            }

            println!();
            println!("{}", "─".repeat(60).dimmed());

            // Token tracking
            println!();
            println!("{}", "Token Tracking:".bold());
            println!("  Enabled: {}  Per-agent: {}", 
                if config.token_tracking.enabled { "yes".green() } else { "no".red() },
                if config.token_tracking.show_per_agent { "yes" } else { "no" }
            );
        }
        None => {
            println!("{} No fellowship.yaml found.", "⚠".yellow());
            println!();
            println!("Run {} to create one.", "mithril fellowship init".cyan());
        }
    }
    Ok(())
}

async fn init() -> Result<()> {
    let path = std::path::Path::new(".mithril/fellowship.yaml");

    if path.exists() {
        println!("{} {} already exists.", "⚠".yellow(), path.display());
        return Ok(());
    }

    // Create .mithril/ if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let template = fellowship::default_template();
    std::fs::write(path, &template)?;

    println!("{} Created {}", "✅".green(), path.display());
    println!();
    println!("Edit it to configure your agents, then use:");
    println!("  {} — to verify agents", "mithril fellowship status".cyan());
    println!("  {} — to test connectivity", "mithril fellowship test".cyan());
    println!();
    println!("The fellowship will activate automatically on next {}.", "mithril chat".cyan());

    Ok(())
}

async fn test_agents() -> Result<()> {
    let config = match FellowshipConfig::try_load() {
        Some(c) => c,
        None => {
            println!("{} No fellowship.yaml found. Run {} first.", "⚠".yellow(), "mithril fellowship init".cyan());
            return Ok(());
        }
    };

    println!("{}", "Testing fellowship agents...".bold());
    println!();

    // Test controller
    print!("  Controller ({})... ", config.controller.provider);
    let mithril_config = crate::config::MithrilConfig::load()?;
    match crate::providers::create_provider(&config.controller.provider, &mithril_config) {
        Ok(p) => {
            match p.chat(&[crate::providers::ChatMessage::user("hello")]).await {
                Ok(_) => println!("{}", "✓".green()),
                Err(e) => println!("{} {}", "✗".red(), e),
            }
        }
        Err(e) => println!("{} {}", "✗".red(), e),
    }

    // Test each agent
    for agent in &config.agents {
        print!("  {} ({})... ", agent.name, match &agent.agent_type {
            AgentType::Provider => agent.provider.as_deref().unwrap_or("?"),
            AgentType::External => agent.binary.as_deref().unwrap_or("?"),
        });

        match &agent.agent_type {
            AgentType::Provider => {
                let prov = agent.provider.as_deref().unwrap_or("unknown");
                match crate::providers::create_provider(prov, &mithril_config) {
                    Ok(p) => {
                        match p.chat(&[crate::providers::ChatMessage::user("hello")]).await {
                            Ok(_) => println!("{}", "✓".green()),
                            Err(e) => println!("{} {}", "✗".red(), e),
                        }
                    }
                    Err(e) => println!("{} {}", "✗".red(), e),
                }
            }
            AgentType::External => {
                let binary = agent.binary.as_deref().unwrap_or("unknown");
                match std::process::Command::new("which").arg(binary).output() {
                    Ok(o) if o.status.success() => println!("{} (found in PATH)", "✓".green()),
                    _ => println!("{} not found in PATH", "✗".red()),
                }
            }
        }
    }

    println!();
    Ok(())
}
