# Security Guide

This page documents all security mechanisms in Mithril, how to configure them, and what protection they provide.

---

## Threat Model

Mithril is designed for **single-user local deployment**. The primary threats are:

1. **Local file access** — other users on the same machine reading config/session files
2. **Network access** — unauthorized clients hitting the HTTP API on the local network
3. **Prompt injection** — LLM-controlled tools executing dangerous commands
4. **Credential theft** — API keys extracted from config files

---

## Credential Encryption

All API keys (Gemini, OpenAI, Anthropic, Telegram) are encrypted before storage.

### Format

```
base64( nonce[12] || salt[16] || AES-256-GCM(plaintext) )
```

### Key derivation

Argon2id (m=65536 KB, t=3, p=1) with password = `"mithril-v2-{username}-{homedir}"`.

If `key_password` is configured, it is mixed into the password for much higher entropy:

```bash
mithril config set key_password "my-strong-secret"
```

### Secrets file

`key_password` and `api_token` are stored in `~/.mithril/secrets` — a **separate file** from `config.yaml`, written with `0600` permissions (owner read/write only). They are never serialized into `config.yaml`.

```
~/.mithril/
├── config.yaml        (0600) — settings + encrypted credentials
├── secrets            (0600) — key_password, api_token (never in config.yaml)
└── sessions/          (dir)  — conversation history, each file 0600
```

### Key rotation

When you change `key_password`, all existing credentials are automatically re-encrypted:

```bash
mithril config set key_password "new-secret"
# Output: ✓ Migrated 3 credential(s) to new key.
```

### Legacy credentials (v1)

Credentials encrypted with the old weak KDF (no Argon2) are auto-detected and decrypted correctly. They are re-encrypted with the new format on the next `mithril config set`.

---

## API Token Authentication

The HTTP server can require a bearer token for all inference and MCP routes.

```bash
# Configure
mithril config set api_token "my-secret-token"

# Clients must send:
# Authorization: Bearer my-secret-token
```

Routes that require auth when `api_token` is set:

| Route | Auth required? |
|-------|---------------|
| `POST /api/chat` | ✅ |
| `POST /api/generate` | ✅ |
| `POST /api/pull` | ✅ |
| `POST /v1/chat/completions` | ✅ |
| `POST /mcp` | ✅ |
| `GET /health` | ❌ (always public) |
| `GET /api/tags` | ❌ (always public) |
| `GET /api/version` | ❌ (always public) |

Token comparison uses `subtle::ConstantTimeEq` to prevent timing attacks.

---

## MCP Stdio Authentication

The `mcp-stdio` transport is used by Claude Desktop and Junie. Auth options:

### Option 1 — Environment variable

```bash
export MITHRIL_API_TOKEN="my-secret-token"
mithril mcp-stdio
```

### Option 2 — JSON-RPC auth message

Send as first message before any tool calls:

```json
{"jsonrpc":"2.0","method":"mithril/auth","params":{"token":"my-secret-token"},"id":0}
```

Response on success:
```json
{"jsonrpc":"2.0","id":0,"result":{"authenticated":true}}
```

### Claude Desktop config with auth

```json
{
  "mcpServers": {
    "mithril": {
      "command": "/path/to/mithril",
      "args": ["mcp-stdio"],
      "env": {
        "MITHRIL_API_TOKEN": "my-secret-token"
      }
    }
  }
}
```

---

## Terminal Sandbox

The `run_terminal` MCP tool runs shell commands. The sandbox blocks dangerous patterns before execution.

### Blocked patterns

| Pattern | Category |
|---------|----------|
| `rm -rf /`, `rm -rf ~` | Destructive deletion |
| `sudo `, `sudo\t` | Privilege escalation |
| `dd if=`, `mkfs` | Disk operations |
| `:(){ :|:& };:` | Fork bomb |
| `> /dev/sd` | Direct disk write |
| `base64 -d`, `base64 --decode` | Decode-and-execute bypass |
| `eval $(`, `` `base64 `` | Eval injection |
| `perl -e`, `ruby -e`, `node -e` | Interpreter one-liners |
| `exec /bin/sh`, `exec /bin/bash` | Shell replacement |
| `$IFS`, `${IFS}` | IFS manipulation |

The sandbox also strips common quoting obfuscation (`s'u'do` → `sudo`) before matching.

### Explicit PATH

Commands run with an explicit safe `PATH`:
```
/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin
```

This prevents binary hijacking via `PATH` manipulation.

### Disable sandbox

```bash
mithril config set terminal_sandbox false
```

⚠️ Disabling the sandbox allows the LLM to execute arbitrary commands.

---

## File Permissions

| File | Permissions | Contents |
|------|-------------|----------|
| `~/.mithril/config.yaml` | 0600 | Settings + encrypted credentials |
| `~/.mithril/secrets` | 0600 | `key_password`, `api_token` |
| `~/.mithril/sessions/*.json` | 0600 | Conversation history |
| `.celebrimbot/shadow_log/*/manifest.json` | 0600 | File operation log |
| `.celebrimbot/shadow_log/*/backup_files` | 0600 | File backups |

---

## Rate Limiting

The HTTP server limits concurrent inference requests to prevent resource exhaustion:

- **`/api/chat`, `/api/generate`, `/v1/chat/completions`, `/mcp`**: max 10 concurrent requests (configurable via `ConcurrencyLimitLayer`)
- **`/health`, `/api/tags`, `/api/version`**: unlimited (no rate limit — required for health checks)

The MCP stdio transport does not have a concurrency limit (it processes one request at a time by design, since stdin is sequential).

---

## Session Security

Sessions are stored at `~/.mithril/sessions/<uuid>.json`. Each file:
- Written with `0600` permissions
- Contains the full conversation history
- UUID-named (unpredictable)

```bash
# List sessions
mithril sessions list

# Delete a session
mithril sessions delete <id>
```

---

## Reporting Security Issues

If you discover a security vulnerability, please open a private issue or contact the maintainer directly. Do not open public issues for security vulnerabilities.
