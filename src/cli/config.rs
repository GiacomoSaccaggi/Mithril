//! Config management CLI subcommand.

use anyhow::Result;
use colored::Colorize;
use crate::config::MithrilConfig;

/// Run config command
pub async fn run(action: &str, key: Option<&str>, value: Option<&str>) -> Result<()> {
    let mut config = MithrilConfig::load()?;

    match action {
        "list" => {
            println!("{}", "⚙️  Mithril Configuration".bold());
            println!();
            
            let path = MithrilConfig::config_path()?;
            println!("{}  {}", "Path:".dimmed(), path.display());
            println!();
            
            println!("{}", "Defaults".bold().blue());
            println!("  Provider: {}", config.default_provider.green());
            println!("  Model:    {}", config.default_model.green());
            println!();
            
            println!("{}", "Credentials".bold().blue());
            let creds = config.list_credentials();
            if creds.is_empty() {
                println!("  {}", "(none configured)".dimmed());
            } else {
                for name in creds {
                    println!("  {} {}", "●".green(), name);
                }
            }
            println!();
            
            println!("{}", "Provider Settings".bold().blue());
            println!("  Gemini:    model = {}", config.providers.gemini.model.cyan());
            println!("  OpenAI:    model = {}", config.providers.openai.model.cyan());
            if let Some(ref url) = config.providers.openai.base_url {
                println!("             base_url = {}", url.cyan());
            }
            println!("  Anthropic: model = {}", config.providers.anthropic.model.cyan());
            println!("  Groq:      model = {}", config.providers.groq.model.cyan());
        }

        "set" => {
            let key = key.ok_or_else(|| anyhow::anyhow!("Missing key. Usage: mithril config set <key> <value>"))?;
            let value = value.ok_or_else(|| anyhow::anyhow!("Missing value. Usage: mithril config set <key> <value>"))?;

            match key {
                "default-provider" | "provider" => {
                    config.set_default_provider(value)?;
                    println!("✅ Default provider set to: {}", value.green());
                }
                "default-model" | "model" => {
                    config.set_default_model(value)?;
                    println!("✅ Default model set to: {}", value.green());
                }
                "gemini" | "openai" | "anthropic" | "groq" => {
                    config.set_credential(key, value)?;
                    println!("✅ {} API key saved (encrypted)", key.green());
                }
                "gemini-model" => {
                    config.providers.gemini.model = value.to_string();
                    config.save()?;
                    println!("✅ Gemini model set to: {}", value.green());
                }
                "openai-model" => {
                    config.providers.openai.model = value.to_string();
                    config.save()?;
                    println!("✅ OpenAI model set to: {}", value.green());
                }
                "openai-base-url" => {
                    config.providers.openai.base_url = Some(value.to_string());
                    config.save()?;
                    println!("✅ OpenAI base URL set to: {}", value.green());
                }
                "anthropic-model" => {
                    config.providers.anthropic.model = value.to_string();
                    config.save()?;
                    println!("✅ Anthropic model set to: {}", value.green());
                }
                _ => {
                    // Treat as generic credential
                    config.set_credential(key, value)?;
                    println!("✅ Credential '{}' saved (encrypted)", key.green());
                }
            }
        }

        "unset" => {
            let key = key.ok_or_else(|| anyhow::anyhow!("Missing key. Usage: mithril config unset <key>"))?;

            match key {
                "openai-base-url" => {
                    config.providers.openai.base_url = None;
                    config.save()?;
                    println!("✅ OpenAI base URL removed");
                }
                _ => {
                    if config.unset_credential(key)? {
                        println!("✅ Credential '{}' removed", key.yellow());
                    } else {
                        println!("{} Credential '{}' not found", "⚠️".yellow(), key);
                    }
                }
            }
        }

        "get" => {
            let key = key.ok_or_else(|| anyhow::anyhow!("Missing key. Usage: mithril config get <key>"))?;

            match key {
                "default-provider" | "provider" => {
                    println!("{}", config.default_provider);
                }
                "default-model" | "model" => {
                    println!("{}", config.default_model);
                }
                "gemini-model" => {
                    println!("{}", config.providers.gemini.model);
                }
                "openai-model" => {
                    println!("{}", config.providers.openai.model);
                }
                "openai-base-url" => {
                    if let Some(url) = &config.providers.openai.base_url {
                        println!("{}", url);
                    }
                }
                "anthropic-model" => {
                    println!("{}", config.providers.anthropic.model);
                }
                _ => {
                    // Check credentials (won't print value for security)
                    if config.get_credential(key)?.is_some() {
                        println!("{} (configured)", "●".green());
                    } else {
                        println!("{}", "(not set)".dimmed());
                    }
                }
            }
        }

        "path" => {
            let path = MithrilConfig::config_path()?;
            println!("{}", path.display());
        }

        _ => {
            print_config_help();
        }
    }

    Ok(())
}

fn print_config_help() {
    println!("{}", "⚙️  Mithril Config".bold());
    println!();
    println!("{}", "Usage:".bold());
    println!("  mithril config list                    Show all configuration");
    println!("  mithril config set <key> <value>       Set a configuration value");
    println!("  mithril config unset <key>             Remove a configuration value");
    println!("  mithril config get <key>               Get a configuration value");
    println!("  mithril config path                    Show config file path");
    println!();
    println!("{}", "Keys:".bold());
    println!("  {}            Default provider (local, gemini, openai, anthropic)", "provider".cyan());
    println!("  {}               Default local model", "model".cyan());
    println!("  {}              Gemini API key", "gemini".cyan());
    println!("  {}              OpenAI API key", "openai".cyan());
    println!("  {}           Anthropic API key", "anthropic".cyan());
    println!("  {}        Model for Gemini provider", "gemini-model".cyan());
    println!("  {}        Model for OpenAI provider", "openai-model".cyan());
    println!("  {}     Custom OpenAI-compatible URL", "openai-base-url".cyan());
    println!("  {}     Model for Anthropic provider", "anthropic-model".cyan());
    println!();
    println!("{}", "Examples:".bold());
    println!("  mithril config set gemini AIza...");
    println!("  mithril config set provider gemini");
    println!("  mithril config set openai-model gpt-4o");
}
