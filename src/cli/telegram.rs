//! Telegram bot frontend for Mithril.
//!
//! Security: messages are only processed from users whose Telegram user_id is in
//! the `telegram_allowed_users` config list. If the list is empty, only the first
//! user to message the bot is whitelisted (owner mode).

#![allow(dead_code)]
use std::sync::Arc;

use anyhow::Result;
use colored::Colorize;
use parking_lot::Mutex;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use tokio_util::sync::CancellationToken;

use crate::config::MithrilConfig;
use crate::providers::{self, ChatMessage};
use crate::session::{SharedSession, FRONTEND_TELEGRAM};

/// Maximum message length accepted from Telegram (chars). Longer messages are rejected.
const MAX_MESSAGE_LEN: usize = 4000;

/// Start the Telegram bot attached to an existing SharedSession.
pub async fn run_with_session(
    token: String,
    session: SharedSession,
    config: Arc<MithrilConfig>,
    cancel: CancellationToken,
) -> Result<()> {
    session.claim_frontend(FRONTEND_TELEGRAM)
        .map_err(|e| anyhow::anyhow!("Cannot start Telegram: {}", e))?;

    println!(
        "  {} Telegram bot active. Session: {}",
        "📱".bold(),
        session.id.dimmed()
    );
    println!("  Send {} from Telegram to return to terminal.\n", "/stop".cyan());

    let session = Arc::new(session);
    let bot = Bot::new(token);
    let stop_flag = Arc::new(tokio::sync::Notify::new());

    // Allowed user IDs: loaded from config. If empty, first user auto-registers (owner mode).
    let allowed_users: Arc<Mutex<Vec<i64>>> = Arc::new(Mutex::new(
        config.telegram_allowed_users.clone()
    ));
    // H3: per-chat-id mutex to serialize messages from the same user.
    // M1: bounded to MAX_CHAT_LOCK_ENTRIES — oldest entries evicted when limit hit.
    const MAX_CHAT_LOCK_ENTRIES: usize = 256;
    let chat_locks: Arc<Mutex<std::collections::HashMap<i64, Arc<tokio::sync::Mutex<()>>>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));

    let handler = Update::filter_message().endpoint({
        let session = Arc::clone(&session);
        let stop = Arc::clone(&stop_flag);
        let config = Arc::clone(&config);
        let allowed = Arc::clone(&allowed_users);
        let locks = Arc::clone(&chat_locks);
        move |bot: Bot, msg: Message| {
            let session = Arc::clone(&session);
            let stop = Arc::clone(&stop);
            let config = Arc::clone(&config);
            let allowed = Arc::clone(&allowed);
            let locks = Arc::clone(&locks);
            async move {
                // Acquire or create per-chat lock
                let chat_id_raw = msg.chat.id.0;
                let chat_lock = {
                    let mut map = locks.lock();
                    map.entry(chat_id_raw)
                        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                        .clone()
                };
                // Serialize messages from the same chat
                let _guard = chat_lock.lock().await;
                handle_telegram_message(bot, msg, session, stop, config, allowed).await;
                respond(())
            }
        }
    });

    let mut dispatcher = Dispatcher::builder(bot, handler)
        .enable_ctrlc_handler()
        .build();

    tokio::select! {
        _ = dispatcher.dispatch() => {}
        _ = stop_flag.notified() => {
            println!("\n  {} /stop received from Telegram.", "📱".dimmed());
        }
        _ = cancel.cancelled() => {}
    }

    session.release_frontend(FRONTEND_TELEGRAM);
    Ok(())
}

/// Standalone entry point: `mithril telegram [--session <id>]`
pub async fn run(session_id: Option<&str>) -> Result<()> {
    let config = MithrilConfig::load()?;

    let token = config
        .get_credential("telegram")?
        .ok_or_else(|| anyhow::anyhow!(
            "Telegram bot token not configured.\n\
             Run: mithril config set telegram <your-bot-token>\n\
             Get a token from @BotFather on Telegram."
        ))?;

    let provider_name = pick_best_provider(&config);

    let session = match session_id {
        Some(id) => {
            println!("  Loading session {}...", id.dimmed());
            SharedSession::load(id)?
        }
        None => {
            let s = SharedSession::new(&provider_name);
            println!(
                "  {} New session: {}  (provider: {})",
                "🗡️".bold(),
                s.id.cyan(),
                provider_name.green()
            );
            s
        }
    };

    if config.telegram_allowed_users.is_empty() {
        println!("  {} No allowed_users configured — first user to message will be auto-registered.", "⚠️".yellow());
        println!("  {} Run: {} to restrict access.", "ℹ️".blue(), "mithril config set telegram_allowed_users <user_id>".cyan());
    }

    println!(
        "  {} Start chatting at your Telegram bot. Press {} to stop.\n",
        "📱".bold(),
        "Ctrl+C".cyan()
    );

    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        cancel_clone.cancel();
    });

    run_with_session(token, session, Arc::new(config), cancel).await
}

// ── Message handler ───────────────────────────────────────────────────────────

async fn handle_telegram_message(
    bot: Bot,
    msg: Message,
    session: Arc<SharedSession>,
    stop: Arc<tokio::sync::Notify>,
    config: Arc<MithrilConfig>,
    allowed_users: Arc<Mutex<Vec<i64>>>,
) {
    let text = match msg.text() {
        Some(t) => t.to_string(),
        None => return,
    };
    let chat_id = msg.chat.id;
    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);

    // ── Authentication ────────────────────────────────────────────────────────
    // Check auth and compute response message BEFORE any await point
    let (is_authorized, new_owner_msg) = {
        let mut allowed = allowed_users.lock();
        if allowed.is_empty() {
            if user_id != 0 {
                allowed.push(user_id);
                // Persist to config.yaml so restart doesn't lose the owner
                if let Ok(mut cfg) = crate::config::MithrilConfig::load() {
                    cfg.telegram_allowed_users = allowed.clone();
                    let _ = cfg.save();
                }
                (true, Some(format!(
                    "✅ You are now registered as the bot owner (user_id: {user_id}).
Owner ID saved to config."
                )))
            } else {
                (false, None)
            }
        } else if allowed.contains(&user_id) {
            (true, None)
        } else {
            (false, None)
        }
    }; // lock dropped here — safe to await below

    if let Some(msg) = new_owner_msg {
        let _ = bot.send_message(chat_id, msg).await;
    }
    if !is_authorized {
        let _ = bot.send_message(chat_id, "🚫 Unauthorized.").await;
        return;
    }

    // ── Message length guard ──────────────────────────────────────────────────
    if text.len() > MAX_MESSAGE_LEN {
        let _ = bot.send_message(
            chat_id,
            format!("⚠️ Message too long ({} chars, max {MAX_MESSAGE_LEN}). Please shorten it.", text.len()),
        ).await;
        return;
    }

    // ── Built-in commands ─────────────────────────────────────────────────────
    if text.trim() == "/stop" {
        session.release_frontend(FRONTEND_TELEGRAM);
        stop.notify_one();
        let _ = bot.send_message(chat_id, "✅ Session handed back to terminal.").await;
        return;
    }

    if text.trim() == "/session" {
        let meta = session.meta();
        let info = format!(
            "📋 *Session:* `{}`\nProvider: {}\nMessages: {}\nCreated: {}",
            meta.id,
            meta.provider_name,
            meta.message_count,
            meta.created_at.format("%Y-%m-%d %H:%M UTC")
        );
        let _ = bot.send_message(chat_id, info)
            .parse_mode(ParseMode::MarkdownV2)
            .await;
        return;
    }

    // ── Frontend ownership check ──────────────────────────────────────────────
    let current = session.active_frontend.load(std::sync::atomic::Ordering::SeqCst);
    if current != FRONTEND_TELEGRAM {
        let _ = bot.send_message(
            chat_id,
            "⚠️ Session is currently active on another frontend. Send /stop to reclaim it.",
        ).await;
        return;
    }

    // ── Inference ─────────────────────────────────────────────────────────────
    let user_msg = ChatMessage::user(&text);
    if let Err(e) = session.push_with_result(user_msg) {
        let _ = bot.send_message(chat_id, format!("❌ Failed to save message: {e}")).await;
        return;
    }

    // Use pre-loaded config (not reload on every message)
    let provider = match providers::create_provider(&session.provider_name, &config) {
        Ok(p) => p,
        Err(e) => {
            let _ = bot.send_message(chat_id, format!("❌ Provider error: {e}")).await;
            session.messages.lock().pop();
            return;
        }
    };

    let _ = bot.send_chat_action(chat_id, teloxide::types::ChatAction::Typing).await;

    let messages_snap = session.snapshot();
    match provider.chat(&messages_snap).await {
        Ok(response) => {
            if let Err(e) = session.push_with_result(ChatMessage::assistant(&response)) {
                eprintln!("Warning: failed to persist assistant message: {e}");
            }
            let _ = bot.send_message(chat_id, &response).await;
        }
        Err(e) => {
            let _ = bot.send_message(chat_id, format!("❌ Inference error: {e}")).await;
            session.messages.lock().pop();
        }
    }
}

// ── Provider selection ────────────────────────────────────────────────────────

fn pick_best_provider(config: &MithrilConfig) -> String {
    for name in &["gemini", "openai", "anthropic"] {
        if config.get_credential(name).ok().flatten().is_some() {
            return name.to_string();
        }
    }
    "local".to_string()
}
