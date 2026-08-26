# Session — The Chronicles

Persistent conversation state with multi-frontend handoff.

## Key Concept

A SharedSession can be claimed by one frontend at a time. Messages persist as JSON. Auto-title from first message.

## Session Contains

- ID (UUID)
- Messages (full conversation)
- Title (auto from first message)
- Version (for migration)
- Timestamps

## Multi-Frontend

Terminal claims session → Telegram can't write. Terminal releases → Telegram can claim. Prevents corruption.

## Files

- `mod.rs` — Shared chat session — the single source of truth for history, provider, and active frontend.
