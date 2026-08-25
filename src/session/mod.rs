//! Shared chat session — the single source of truth for history, provider, and active frontend.
//!
//! A session can be handed off between Terminal, Telegram, and Junie (via MCP) without
//! losing conversation history. Only one frontend is active at a time.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::providers::ChatMessage;

// ── Frontend IDs ─────────────────────────────────────────────────────────────

pub const FRONTEND_TERMINAL: u8 = 0;
pub const FRONTEND_TELEGRAM: u8 = 1;
pub const FRONTEND_JUNIE: u8 = 2;
pub const FRONTEND_NONE: u8 = 255;

/// Human-readable frontend name for display.
pub fn frontend_name(id: u8) -> &'static str {
    match id {
        FRONTEND_TERMINAL => "terminal",
        FRONTEND_TELEGRAM => "telegram",
        FRONTEND_JUNIE => "junie",
        _ => "none",
    }
}

// ── Session metadata (lightweight, for listing) ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub provider_name: String,
    pub message_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ── Full session (stored on disk + held in memory) ───────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct SessionData {
    pub id: String,
    pub provider_name: String,
    pub messages: Vec<ChatMessage>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub title: Option<String>,
}

/// Shared session — clone-cheap because it's all behind Arc.
#[derive(Clone, Debug)]
pub struct SharedSession {
    pub id: String,
    pub provider_name: String,
    /// Short auto-generated title for this session.
    pub title: Arc<Mutex<Option<String>>>,
    /// Conversation history — shared between all frontends.
    pub messages: Arc<Mutex<Vec<ChatMessage>>>,
    /// Which frontend is currently active.
    /// FRONTEND_TERMINAL=0, FRONTEND_TELEGRAM=1, FRONTEND_JUNIE=2, FRONTEND_NONE=255
    pub active_frontend: Arc<AtomicU8>,
    pub created_at: DateTime<Utc>,
    updated_at: Arc<Mutex<DateTime<Utc>>>,
}

impl SharedSession {
    /// Create a brand-new session.
    pub fn new(provider_name: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            provider_name: provider_name.to_string(),
            title: Arc::new(Mutex::new(None)),
            messages: Arc::new(Mutex::new(Vec::new())),
            active_frontend: Arc::new(AtomicU8::new(FRONTEND_TERMINAL)),
            created_at: now,
            updated_at: Arc::new(Mutex::new(now)),
        }
    }

    /// Load an existing session from disk.
    pub fn load(session_id: &str) -> Result<Self> {
        let path = session_path(session_id);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Session '{}' not found at {}", session_id, path.display()))?;
        let data: SessionData =
            serde_json::from_str(&content).context("Failed to parse session file")?;
        Ok(Self {
            id: data.id,
            provider_name: data.provider_name,
            title: Arc::new(Mutex::new(data.title)),
            messages: Arc::new(Mutex::new(data.messages)),
            active_frontend: Arc::new(AtomicU8::new(FRONTEND_TERMINAL)),
            created_at: data.created_at,
            updated_at: Arc::new(Mutex::new(data.updated_at)),
        })
    }

    /// Save session to `~/.mithril/sessions/<id>.json`.
    pub fn save(&self) -> Result<()> {
        let now = Utc::now();
        // Take a snapshot while holding messages lock, then release before touching updated_at
        let snapshot = self.messages.lock().clone();
        let dir = sessions_dir()?;
        fs::create_dir_all(&dir)?;
        let data = SessionData {
            id: self.id.clone(),
            provider_name: self.provider_name.clone(),
            messages: snapshot,
            created_at: self.created_at,
            updated_at: now,
            title: self.title.lock().clone(),
        };
        let content = serde_json::to_string_pretty(&data)?;
        write_restricted(&session_path(&self.id), content.as_bytes())?;
        // Update timestamp only after successful write, outside messages lock
        *self.updated_at.lock() = now;
        Ok(())
    }

    /// Add a message to history and auto-save.
    /// Logs a warning if save fails (disk full, permissions error, etc.).
    /// Set a title for this session (auto-generated from first message).
    pub fn set_title(&self, title: &str) {
        *self.title.lock() = Some(title.to_string());
    }

    /// Get the current title.
    pub fn get_title(&self) -> Option<String> {
        self.title.lock().clone()
    }

    pub fn push(&self, msg: ChatMessage) {
        self.messages.lock().push(msg);
        if let Err(e) = self.save() {
            tracing::warn!("Session save failed (message still in memory): {}", e);
        }
    }

    /// Add a message to history and propagate save errors to the caller.
    /// Holds the messages lock across push+save to make rollback atomic.
    pub fn push_with_result(&self, msg: ChatMessage) -> Result<()> {
        // Compute timestamp BEFORE acquiring messages lock to avoid lock-ordering inversion.
        // updated_at is updated only after save succeeds, outside any other lock.
        let now = chrono::Utc::now();
        let mut msgs = self.messages.lock();
        msgs.push(msg);
        match self.save_inner(&msgs, now) {
            Ok(()) => {
                drop(msgs); // release messages lock before acquiring updated_at lock
                *self.updated_at.lock() = now;
                Ok(())
            }
            Err(e) => {
                msgs.pop(); // rollback under messages lock
                Err(e)
            }
        }
    }

    /// Internal save that accepts an already-locked messages slice.
    /// Does NOT acquire any other locks — caller must ensure lock ordering is safe.
    /// `now` is passed by the caller to avoid acquiring updated_at.lock() here.
    fn save_inner(&self, msgs: &[ChatMessage], now: chrono::DateTime<chrono::Utc>) -> Result<()> {
        let dir = sessions_dir()?;
        std::fs::create_dir_all(&dir)?;
        let data = SessionData {
            id: self.id.clone(),
            provider_name: self.provider_name.clone(),
            messages: msgs.to_vec(),
            created_at: self.created_at,
            updated_at: now,
            title: self.title.lock().clone(),
        };
        let content = serde_json::to_string_pretty(&data)?;
        write_restricted(&session_path(&self.id), content.as_bytes())?;
        Ok(())
    }

    /// Snapshot of the current history (cheap clone).
    pub fn snapshot(&self) -> Vec<ChatMessage> {
        self.messages.lock().clone()
    }

    /// Number of messages after the given offset (used by terminal to detect new Telegram messages).
    #[allow(dead_code)]
    pub fn messages_since(&self, offset: usize) -> Vec<ChatMessage> {
        let msgs = self.messages.lock();
        if offset >= msgs.len() {
            vec![]
        } else {
            msgs[offset..].to_vec()
        }
    }

    /// Try to claim a frontend atomically using compare_exchange.
    /// Returns Ok if claimed, Err if already taken by someone else.
    pub fn claim_frontend(&self, frontend: u8) -> Result<()> {
        // Try to swap NONE -> frontend atomically
        if let Ok(_) = self.active_frontend.compare_exchange(
            FRONTEND_NONE,
            frontend,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) { return Ok(()) }
        // Already held by the same frontend (idempotent re-claim)
        let current = self.active_frontend.load(Ordering::SeqCst);
        if current == frontend {
            return Ok(());
        }
        anyhow::bail!(
            "Session is currently active on '{}' frontend",
            frontend_name(current)
        )
    }

    /// Release a frontend (set to NONE).
    pub fn release_frontend(&self, frontend: u8) {
        let _ = self.active_frontend.compare_exchange(
            frontend,
            FRONTEND_NONE,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    pub fn active_frontend_name(&self) -> &'static str {
        frontend_name(self.active_frontend.load(Ordering::SeqCst))
    }

    pub fn meta(&self) -> SessionMeta {
        SessionMeta {
            id: self.id.clone(),
            provider_name: self.provider_name.clone(),
            message_count: self.messages.lock().len(),
            created_at: self.created_at,
            updated_at: *self.updated_at.lock(),
        }
    }
}

// ── Session directory helpers ─────────────────────────────────────────────────

/// Write file with 0600 permissions (owner read/write only).
/// Falls back to fs::write on non-Unix platforms.
fn write_restricted(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true).create(true).truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(data)?;
        Ok(())
    }
    #[cfg(not(unix))]
    { std::fs::write(path, data) }
}


fn sessions_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot find home directory")?;
    Ok(home.join(".mithril").join("sessions"))
}

fn session_path(id: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".mithril")
        .join("sessions")
        .join(format!("{id}.json"))
}

/// List all saved sessions, sorted by updated_at descending.
pub fn list_sessions() -> Result<Vec<SessionMeta>> {
    let dir = sessions_dir()?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut metas = Vec::new();
    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(data) = serde_json::from_str::<SessionData>(&content) {
                metas.push(SessionMeta {
                    id: data.id,
                    provider_name: data.provider_name,
                    message_count: data.messages.len(),
                    created_at: data.created_at,
                    updated_at: data.updated_at,
                });
            }
        }
    }
    metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(metas)
}

/// Delete a session from disk.
pub fn delete_session(id: &str) -> Result<()> {
    let path = session_path(id);
    if path.exists() {
        fs::remove_file(&path)?;
        Ok(())
    } else {
        anyhow::bail!("Session '{}' not found", id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_session_has_terminal_frontend() {
        let s = SharedSession::new("local");
        assert_eq!(s.active_frontend.load(Ordering::SeqCst), FRONTEND_TERMINAL);
    }

    #[test]
    fn test_claim_release_frontend() {
        let s = SharedSession::new("local");
        // claim telegram from terminal state
        s.active_frontend.store(FRONTEND_NONE, Ordering::SeqCst);
        assert!(s.claim_frontend(FRONTEND_TELEGRAM).is_ok());
        // can't claim terminal while telegram is active
        assert!(s.claim_frontend(FRONTEND_TERMINAL).is_err());
        // release telegram
        s.release_frontend(FRONTEND_TELEGRAM);
        assert_eq!(s.active_frontend.load(Ordering::SeqCst), FRONTEND_NONE);
    }

    #[test]
    fn test_push_and_snapshot_basic() {
        let s = SharedSession::new("local");
        s.messages.lock().push(ChatMessage::user("hello"));
        let snap = s.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].content, "hello");
    }

    #[test]
    fn test_messages_since_offset() {
        let s = SharedSession::new("local");
        s.messages.lock().push(ChatMessage::user("msg1"));
        s.messages.lock().push(ChatMessage::assistant("reply1"));
        s.messages.lock().push(ChatMessage::user("msg2"));
        let new_msgs = s.messages_since(2);
        assert_eq!(new_msgs.len(), 1);
        assert_eq!(new_msgs[0].content, "msg2");
    }

    #[test]
    fn test_new_session_generates_uuid() {
        let s1 = SharedSession::new("test");
        let s2 = SharedSession::new("test");
        assert_ne!(s1.id, s2.id);
        assert_eq!(s1.id.len(), 36); // UUID format
    }

    #[test]
    fn test_new_session_provider_name() {
        let s = SharedSession::new("gemini");
        assert_eq!(s.provider_name, "gemini");
    }

    #[test]
    fn test_new_session_starts_empty() {
        let s = SharedSession::new("local");
        assert!(s.snapshot().is_empty());
    }

    #[test]
    fn test_snapshot_is_clone() {
        let s = SharedSession::new("local");
        s.messages.lock().push(ChatMessage::user("test"));
        let snap1 = s.snapshot();
        s.messages.lock().push(ChatMessage::assistant("reply"));
        let snap2 = s.snapshot();

        assert_eq!(snap1.len(), 1);
        assert_eq!(snap2.len(), 2);
    }

    #[test]
    fn test_messages_since_empty() {
        let s = SharedSession::new("local");
        let msgs = s.messages_since(0);
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_messages_since_offset_beyond_length() {
        let s = SharedSession::new("local");
        s.messages.lock().push(ChatMessage::user("msg"));
        let msgs = s.messages_since(10);
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_meta_returns_correct_info() {
        let s = SharedSession::new("openai");
        s.messages.lock().push(ChatMessage::user("hello"));
        s.messages.lock().push(ChatMessage::assistant("hi"));

        let meta = s.meta();
        assert_eq!(meta.provider_name, "openai");
        assert_eq!(meta.message_count, 2);
        assert_eq!(meta.id, s.id);
    }

    #[test]
    fn test_active_frontend_name() {
        let s = SharedSession::new("local");
        assert_eq!(s.active_frontend_name(), "terminal");

        s.active_frontend.store(FRONTEND_TELEGRAM, Ordering::SeqCst);
        assert_eq!(s.active_frontend_name(), "telegram");

        s.active_frontend.store(FRONTEND_JUNIE, Ordering::SeqCst);
        assert_eq!(s.active_frontend_name(), "junie");

        s.active_frontend.store(FRONTEND_NONE, Ordering::SeqCst);
        assert_eq!(s.active_frontend_name(), "none");
    }

    #[test]
    fn test_frontend_name_helper() {
        assert_eq!(frontend_name(FRONTEND_TERMINAL), "terminal");
        assert_eq!(frontend_name(FRONTEND_TELEGRAM), "telegram");
        assert_eq!(frontend_name(FRONTEND_JUNIE), "junie");
        assert_eq!(frontend_name(FRONTEND_NONE), "none");
        assert_eq!(frontend_name(99), "none"); // unknown
    }

    #[test]
    fn test_claim_frontend_idempotent() {
        let s = SharedSession::new("local");
        s.active_frontend.store(FRONTEND_NONE, Ordering::SeqCst);

        // First claim
        assert!(s.claim_frontend(FRONTEND_TELEGRAM).is_ok());
        // Re-claim same frontend should succeed
        assert!(s.claim_frontend(FRONTEND_TELEGRAM).is_ok());
    }

    #[test]
    fn test_release_frontend_wrong_frontend() {
        let s = SharedSession::new("local");
        s.active_frontend.store(FRONTEND_TELEGRAM, Ordering::SeqCst);

        // Try to release different frontend
        s.release_frontend(FRONTEND_TERMINAL);
        // Should still be telegram
        assert_eq!(s.active_frontend.load(Ordering::SeqCst), FRONTEND_TELEGRAM);
    }

    #[test]
    fn test_session_clone_shares_state() {
        let s = SharedSession::new("local");
        let s_clone = s.clone();

        s.messages.lock().push(ChatMessage::user("from original"));

        // Clone should see the same message
        assert_eq!(s_clone.snapshot().len(), 1);
    }

    #[test]
    fn test_session_meta_timestamps() {
        let before = Utc::now();
        let s = SharedSession::new("local");
        let after = Utc::now();

        let meta = s.meta();
        assert!(meta.created_at >= before);
        assert!(meta.created_at <= after);
    }

    #[test]
    fn test_session_meta_serialization() {
        let meta = SessionMeta {
            id: "test-id".to_string(),
            provider_name: "local".to_string(),
            message_count: 5,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: SessionMeta = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, meta.id);
        assert_eq!(deserialized.provider_name, meta.provider_name);
        assert_eq!(deserialized.message_count, meta.message_count);
    }

    #[test]
    fn test_frontend_constants() {
        assert_eq!(FRONTEND_TERMINAL, 0);
        assert_eq!(FRONTEND_TELEGRAM, 1);
        assert_eq!(FRONTEND_JUNIE, 2);
        assert_eq!(FRONTEND_NONE, 255);
    }

    #[test]
    fn test_multiple_frontends_sequence() {
        let s = SharedSession::new("local");
        s.active_frontend.store(FRONTEND_NONE, Ordering::SeqCst);

        // Terminal claims
        assert!(s.claim_frontend(FRONTEND_TERMINAL).is_ok());
        s.release_frontend(FRONTEND_TERMINAL);

        // Telegram claims
        assert!(s.claim_frontend(FRONTEND_TELEGRAM).is_ok());
        s.release_frontend(FRONTEND_TELEGRAM);

        // Junie claims
        assert!(s.claim_frontend(FRONTEND_JUNIE).is_ok());
        s.release_frontend(FRONTEND_JUNIE);

        assert_eq!(s.active_frontend.load(Ordering::SeqCst), FRONTEND_NONE);
    }

    #[test]
    fn test_session_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let s = Arc::new(SharedSession::new("local"));
        let mut handles = vec![];

        for i in 0..10 {
            let session = Arc::clone(&s);
            handles.push(thread::spawn(move || {
                session.messages.lock().push(ChatMessage::user(&format!("msg {}", i)));
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(s.snapshot().len(), 10);
    }

    #[test]
    fn test_messages_since_boundary() {
        let s = SharedSession::new("local");
        for i in 0..5 {
            s.messages.lock().push(ChatMessage::user(&format!("msg {}", i)));
        }

        // Offset at exact length
        let msgs = s.messages_since(5);
        assert!(msgs.is_empty());

        // Offset at length - 1
        let msgs = s.messages_since(4);
        assert_eq!(msgs.len(), 1);
    }
}
