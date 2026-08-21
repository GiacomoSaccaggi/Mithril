//! `mithril flow` — run a multi-agent flow on a user message.

use anyhow::Result;
use colored::Colorize;

use crate::flow::{load_flow_config, FlowRunner};

pub async fn run(message: &str, config_path: Option<&str>) -> Result<()> {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let config = load_flow_config(config_path)?;

    println!();
    println!("  {} {}  {}",
        "🗡️".bold(),
        "Mithril Flow".bold().cyan(),
        format!("({})", config.name).dimmed()
    );
    println!("  {} Planner: {} [{}]",
        "◆".cyan(),
        config.planner.name.bold(),
        config.planner.provider.dimmed()
    );
    if let Some(ref w) = config.worker {
        println!("  {} Worker:  {} [{}]",
            "◇".dimmed(),
            w.name.bold(),
            w.provider.dimmed()
        );
    }
    println!();

    let runner = FlowRunner::new(config, &cwd);
    runner.run(message).await?;

    Ok(())
}
