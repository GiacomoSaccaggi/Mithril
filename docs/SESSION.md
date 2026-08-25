# Session Persistence and Handoff

> *"I will not say: do not weep; for not all tears are an evil."* — Gandalf

Mithril sessions persist automatically, allowing seamless handoff between interfaces and recovery of your work.

---

## Session Overview

A session captures:

| Component | Description |
|-----------|-------------|
| **Messages** | Full conversation history |
| **Context** | Files read, tools used |
| **Mode** | Plan or Build state |
| **Model** | Which model was active |
| **Tokens** | Usage statistics |
| **Timestamp** | Creation and last activity |

---

## Auto-Titling

> *"The tale grew in the telling."*

Sessions are automatically titled based on content using the active model.

### How It Works

1. After 3-5 messages, Mithril sends conversation to model
2. Model generates concise, descriptive title
3. Title is stored with session metadata

### Title Prompt

```
Summarize this conversation in 3-6 words for a session title.
Focus on the main task or topic.
Respond with ONLY the title, no quotes or explanation.
```

### Examples

| Conversation Start | Auto-Title |
|-------------------|------------|
| "help me fix this rust error" | "Debugging Rust Compilation" |
| "explain how auth works" | "Authentication Architecture Review" |
| "add pagination to API" | "API Pagination Implementation" |

### Manual Override

```
/session rename "My Better Title"
```

---

## Session Storage

Sessions are stored in `~/.mithril/sessions/`:

```
~/.mithril/sessions/
├── debugging-rust-compilation/
│   ├── session.json
│   └── context/
│       └── cached_files.json
├── api-pagination/
│   ├── session.json
│   └── context/
└── index.json
```

### Session JSON Structure

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "title": "Debugging Rust Compilation",
  "created_at": "2024-01-15T10:30:00Z",
  "updated_at": "2024-01-15T11:45:00Z",
  "model": "gemini-2.5-flash",
  "mode": "build",
  "messages": [
    {
      "role": "user",
      "content": "help me fix this rust error",
      "timestamp": "2024-01-15T10:30:00Z"
    },
    {
      "role": "assistant", 
      "content": "I'll help you debug...",
      "timestamp": "2024-01-15T10:30:05Z",
      "tool_calls": [...],
      "tokens": {"input": 150, "output": 420}
    }
  ],
  "tokens": {
    "total_input": 3500,
    "total_output": 8200
  },
  "working_directory": "/home/user/myproject"
}
```

---

## Session Commands

### List Sessions

```bash
mithril sessions
```

Output:
```
Sessions (5 total):
  📖 debugging-rust-compilation    2024-01-15 10:30  (1.2h ago)
  🔨 api-pagination                2024-01-15 09:00  (2.7h ago)
  📖 architecture-review           2024-01-14 16:00  (1d ago)
  🔨 test-coverage-improvement     2024-01-13 14:00  (2d ago)
  📖 onboarding-docs               2024-01-10 11:00  (5d ago)
```

### Resume Session

```bash
mithril chat --session "debugging-rust-compilation"
```

Or in chat:
```
/load debugging-rust-compilation
```

### Delete Session

```bash
mithril sessions --delete "old-session-name"
```

### Export Session

```bash
mithril sessions --export "session-name" > session.json
```

### Import Session

```bash
mithril sessions --import session.json
```

---

## Handoff Between Interfaces

> *"The road goes ever on and on, down from the door where it began."*

Sessions can be handed off between different Mithril interfaces.

### Supported Interfaces

| Interface | Description |
|-----------|-------------|
| **Terminal TUI** | `mithril chat` |
| **Plain REPL** | `mithril chat --plain` |
| **Telegram** | `mithril telegram` |
| **Junie/IDE** | Via Ollama API |

### Sharing to Telegram

From terminal:
```
/share telegram
```

Output:
```
Session shared! Token: abc123xyz
In Telegram, send: /load abc123xyz
Token expires in 10 minutes.
```

### Sharing from Telegram

In Telegram bot:
```
/share terminal
```

Then in terminal:
```bash
mithril chat --token "abc123xyz"
```

### Share Mechanism

1. Session is serialized to temporary storage
2. Short-lived token is generated
3. Token maps to session data
4. Target interface loads and continues

```rust
pub struct ShareToken {
    token: String,
    session_id: Uuid,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    source: Interface,
    target: Interface,
}
```

---

## Automatic Persistence

### When Sessions Save

| Event | Auto-Save |
|-------|-----------|
| Every message | ✅ Incremental |
| Mode change | ✅ |
| Model change | ✅ |
| `/save` command | ✅ Full |
| Clean exit | ✅ Full |
| Crash | ✅ Last incremental |

### Crash Recovery

If Mithril exits unexpectedly:

1. Last incremental save is preserved
2. On next start, recovery prompt appears:
   ```
   Recovered session "api-pagination" from crash.
   Resume? [Y/n]
   ```

---

## Session Context

Sessions include cached context for efficiency:

### Cached Items

| Item | Purpose |
|------|---------|
| **File contents** | Avoid re-reading unchanged files |
| **Git state** | Branch, status at session start |
| **Palantír hits** | Previous search results |
| **Tool outputs** | Memoized expensive operations |

### Context Invalidation

Context is invalidated when:

- File modification time changes
- Working directory changes
- Explicit `/clear` command
- Session age exceeds threshold (default: 24h)

---

## Multi-Session Workflows

### Parallel Sessions

Run multiple sessions in different terminals:

```bash
# Terminal 1
mithril chat --session "feature-auth"

# Terminal 2  
mithril chat --session "bugfix-api"
```

### Session Branching

Create a branch from existing session:

```
/session branch "experiment-v2"
```

This creates a new session with current history but independent future.

### Session Comparison

Compare what changed between sessions:

```bash
mithril sessions --diff "before" "after"
```

---

## Privacy and Cleanup

### Session Retention

By default, sessions are kept for 30 days. Configure in `~/.mithril/config.yaml`:

```yaml
sessions:
  retention_days: 30
  max_sessions: 100
  auto_cleanup: true
```

### Manual Cleanup

```bash
mithril sessions --cleanup         # Remove expired
mithril sessions --cleanup --all   # Remove all
```

### Sensitive Data

Sessions may contain:
- Code snippets from your project
- File paths
- Error messages
- Conversation content

**Never share session exports publicly** without review.

---

## Configuration

```yaml
# ~/.mithril/config.yaml
sessions:
  # Storage location (default: ~/.mithril/sessions)
  path: ~/.mithril/sessions
  
  # Auto-title after N messages
  auto_title_after: 3
  
  # Days before auto-cleanup
  retention_days: 30
  
  # Maximum stored sessions
  max_sessions: 100
  
  # Share token expiry (minutes)
  share_token_expiry: 10
  
  # Enable crash recovery
  crash_recovery: true
```

---

> *"All we have to decide is what to do with the time that is given us."*
