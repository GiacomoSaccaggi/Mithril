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
            println!();
            println!("{}", "Security".bold().blue());
            println!("  Input redaction (regex): {}", if config.redact_input { "enabled".green() } else { "disabled".yellow() });
            println!("  LLM credential check:   {}", if config.redact_llm { "enabled".green() } else { "disabled".yellow() });
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
                "glean" => {
                    config.set_credential(key, value)?;
                    println!("✅ {} session cookies saved (encrypted)", key.green());
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
                "redact_input" | "redact-input" => {
                    let val = value.eq_ignore_ascii_case("true") || value == "1";
                    config.redact_input = val;
                    config.save()?;
                    println!("✅ Input redaction: {}", if val { "enabled".green() } else { "disabled".yellow() });
                }
                "redact_llm" | "redact-llm" => {
                    let val = value.eq_ignore_ascii_case("true") || value == "1";
                    config.redact_llm = val;
                    config.save()?;
                    println!("✅ LLM credential detection: {}", if val { "enabled".green() } else { "disabled".yellow() });
                }
                _ => {
                    // Treat as generic credential
                    config.set_credential(key, value)?;
                    println!("✅ Credential '{}' saved (encrypted)", key.green());
                }
            }
        }

        "login" => {
            let key = key.ok_or_else(|| anyhow::anyhow!("Missing provider. Usage: mithril config login glean"))?;
            match key {
                "glean" => {
                    login_glean(&mut config).await?;
                }
                _ => {
                    anyhow::bail!("Login not supported for provider: {}. Only 'glean' supports browser login.", key);
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
                "redact_input" | "redact-input" => {
                    println!("{}", config.redact_input);
                }
                "redact_llm" | "redact-llm" => {
                    println!("{}", config.redact_llm);
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
    println!("  mithril config login <provider>        Browser login (glean)");
    println!("  mithril config path                    Show config file path");
    println!();
    println!("{}", "Keys:".bold());
    println!("  {}            Default provider (local, gemini, openai, anthropic, glean)", "provider".cyan());
    println!("  {}               Default local model", "model".cyan());
    println!("  {}              Gemini API key", "gemini".cyan());
    println!("  {}              OpenAI API key", "openai".cyan());
    println!("  {}           Anthropic API key", "anthropic".cyan());
    println!("  {}              Glean session cookies", "glean".cyan());
    println!("  {}        Model for Gemini provider", "gemini-model".cyan());
    println!("  {}        Model for OpenAI provider", "openai-model".cyan());
    println!("  {}     Custom OpenAI-compatible URL", "openai-base-url".cyan());
    println!("  {}     Model for Anthropic provider", "anthropic-model".cyan());
    println!();
    println!("{}", "Examples:".bold());
    println!("  mithril config set gemini AIza...");
    println!("  mithril config set provider gemini");
    println!("  mithril config set openai-model gpt-4o");
    println!("  mithril config login glean");
}

async fn login_glean(config: &mut MithrilConfig) -> Result<()> {
    // Auto-detect instance
    let instance = crate::providers::glean::resolve_instance()
        .ok_or_else(|| anyhow::anyhow!("Glean instance not configured. Set MITHRIL_GLEAN_INSTANCE env var or install Glean desktop app"))?;

    println!("{}", "🔐 Glean Login".bold());
    println!();
    println!("Instance: {}", instance.cyan());
    println!();

    // Open browser
    let app_url = "https://app.glean.com";
    println!("Opening {} in your browser...", app_url.cyan());
    let _ = open_browser(app_url);

    println!();
    println!("{}", "After logging in, extract the cookies:".bold());
    println!("  1. Open DevTools (F12) → Application tab → Cookies");
    println!("  2. Find cookies for your Glean domain");
    println!("  3. Copy the values of:");
    println!("     • {}", "glean-session-store".yellow());
    println!("     • {}", "okta-saml-hosted-login-session-store".yellow());
    println!("  4. Paste them below in this format:");
    println!("     {}", "glean-session-store=<value>; okta-saml-hosted-login-session-store=<value>".dimmed());
    println!();

    // Read cookie string
    print!("{} ", "Paste cookies:".bold());
    use std::io::Write;
    std::io::stdout().flush()?;

    let mut cookie_str = String::new();
    std::io::stdin().read_line(&mut cookie_str)?;
    let cookie_str = cookie_str.trim().to_string();

    if cookie_str.is_empty() {
        anyhow::bail!("No cookies provided");
    }

    // Validate
    println!("Validating session...");
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/listchats", instance))
        .header("Content-Type", "application/json")
        .header("Origin", "https://app.glean.com")
        .header("Cookie", &cookie_str)
        .body("{}")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "Cookie validation failed (HTTP {}). Please check your cookies and try again.",
            resp.status()
        );
    }

    // Save
    config.set_credential("glean", &cookie_str)?;
    println!();
    println!(
        "✅ Glean session saved! You can now use {} as a provider.",
        "glean".green()
    );

    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", url])
        .spawn();
}
