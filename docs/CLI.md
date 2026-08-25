# The Sixteen Commands

> *"The board is set, the pieces are moving."* — Gandalf

Mithril offers sixteen commands to begin your journey, plus interactive chat commands and agent mentions.

---

## Command Overview

| Command | Purpose |
|---------|---------|
| `start` | Server + chat in one command |
| `serve` | HTTP server only |
| `chat` | Interactive TUI or plain REPL |
| `exec` | Non-interactive execution |
| `flow` | Agentic Planner→Tools loop |
| `fellowship` | Multi-agent orchestration |
| `fellowships` | List available fellowships |
| `forge` | Single inference and print |
| `init` | Generate MITHRIL.md steering file |
| `scan` | Build Palantír BM25 index |
| `config` | Manage credentials and settings |
| `download-model` | Download GGUF models |
| `mcp-stdio` | MCP server over stdin/stdout |
| `telegram` | Start Telegram bot frontend |
| `sessions` | Manage saved sessions |
| `undo` | Undo last shadow log session |

---

## The Commands in Detail

### `mithril start`

> *"The road goes ever on."*

Launch both server and chat interface in a single command — the recommended way to begin.

```bash
mithril start                     # Server + TUI
mithril start --plain             # Server + plain REPL
mithril start --port 8080         # Custom port
```

| Flag | Description |
|------|-------------|
| `--plain` | Use readline REPL instead of TUI |
| `--port` | HTTP server port (default: 16180) |
| `--model` | Default model to load |

---

### `mithril serve`

Run the HTTP server without the chat interface.

```bash
mithril serve                     # Default port 16180
mithril serve --port 8080         # Custom port
mithril serve --host 0.0.0.0      # Expose to network
```

| Flag | Description |
|------|-------------|
| `--port` | Server port (default: 16180) |
| `--host` | Bind address (default: 127.0.0.1) |
| `--cors` | Enable CORS for web clients |

---

### `mithril chat`

Interactive chat interface with full tool access.

```bash
mithril chat                      # TUI mode
mithril chat --plain              # Plain REPL mode
mithril chat --session "mywork"   # Resume named session
mithril chat --model qwen-7b      # Specify model
```

| Flag | Description |
|------|-------------|
| `--plain` | Use readline REPL instead of TUI |
| `--session` | Session name to create or resume |
| `--model` | Model to use |
| `--fellowship` | Fellowship configuration to load |
| `--mode` | Initial mode: "plan" or "build" |

---

### `mithril exec`

Non-interactive execution for scripts and automation.

```bash
mithril exec "explain this code" --file src/main.rs
mithril exec "$(cat prompt.txt)" --json
echo "hello" | mithril exec -
```

| Flag | Description |
|------|-------------|
| `--file` | Include file contents in context |
| `--json` | Output JSON response |
| `--model` | Model to use |

---

### `mithril flow`

Run an agentic loop with planning and tool execution.

```bash
mithril flow "refactor the auth module"
mithril flow "add tests for utils.rs" --max-steps 10
```

| Flag | Description |
|------|-------------|
| `--max-steps` | Maximum planning iterations |
| `--model` | Model to use |
| `--dry-run` | Show plan without executing |

---

### `mithril fellowship`

Execute a multi-agent orchestration.

```bash
mithril fellowship "review and improve error handling"
mithril fellowship "implement feature X" --config custom.yaml
```

| Flag | Description |
|------|-------------|
| `--config` | Fellowship YAML configuration |
| `--max-rounds` | Maximum orchestration rounds |

---

### `mithril fellowships`

List available fellowship configurations.

```bash
mithril fellowships               # List all
mithril fellowships --verbose     # Show details
```

---

### `mithril forge`

Single inference with immediate output — no chat loop.

```bash
mithril forge "translate to Spanish: hello world"
mithril forge "summarize:" --file long_document.md
```

| Flag | Description |
|------|-------------|
| `--file` | Include file in prompt |
| `--model` | Model to use |
| `--max-tokens` | Maximum output tokens |

---

### `mithril init`

Generate a `MITHRIL.md` steering file for the current project.

```bash
mithril init                      # Interactive setup
mithril init --template rust      # Use language template
```

| Flag | Description |
|------|-------------|
| `--template` | Language/framework template |
| `--force` | Overwrite existing file |

---

### `mithril scan`

Build the Palantír BM25 index for semantic search.

```bash
mithril scan                      # Scan current directory
mithril scan --path /project      # Scan specific path
mithril scan --rebuild            # Force full rebuild
```

| Flag | Description |
|------|-------------|
| `--path` | Directory to index |
| `--rebuild` | Ignore cache, rebuild from scratch |
| `--exclude` | Patterns to exclude |

---

### `mithril config`

Manage credentials and settings.

```bash
mithril config set gemini "AIza..."    # Store API key
mithril config set openai "sk-..."     # Store API key
mithril config get gemini              # Retrieve (masked)
mithril config list                    # Show all keys
mithril config unset gemini            # Remove key
```

Subcommands:

| Subcommand | Description |
|------------|-------------|
| `set <key> <value>` | Store encrypted credential |
| `get <key>` | Retrieve credential (masked) |
| `list` | List all configured keys |
| `unset <key>` | Remove credential |

---

### `mithril download-model`

Download GGUF models from Hugging Face or URLs.

```bash
mithril download-model TheBloke/Mistral-7B-v0.1-GGUF
mithril download-model https://example.com/model.gguf
mithril download-model --list     # Show downloaded models
```

| Flag | Description |
|------|-------------|
| `--list` | List downloaded models |
| `--output` | Custom output directory |

---

### `mithril mcp-stdio`

Run as an MCP server over stdin/stdout for integration with MCP clients.

```bash
mithril mcp-stdio                 # Start MCP server
```

Used by Claude Desktop and other MCP-compatible clients.

---

### `mithril telegram`

Start the Telegram bot frontend.

```bash
mithril telegram                  # Use configured token
mithril telegram --token "..."    # Override token
```

| Flag | Description |
|------|-------------|
| `--token` | Telegram bot token |
| `--allowed-users` | Comma-separated user IDs |

---

### `mithril sessions`

Manage saved chat sessions.

```bash
mithril sessions                  # List all sessions
mithril sessions --delete "name"  # Delete a session
mithril sessions --export "name"  # Export to JSON
```

| Flag | Description |
|------|-------------|
| `--delete` | Delete named session |
| `--export` | Export session to file |
| `--import` | Import session from file |

---

### `mithril undo`

Restore files from the shadow log.

```bash
mithril undo                      # Undo last session
mithril undo --list               # Show restore points
mithril undo --session "2024-..."  # Undo specific session
```

| Flag | Description |
|------|-------------|
| `--list` | List available restore points |
| `--session` | Restore specific session |
| `--dry-run` | Show what would be restored |

---

## Chat Commands

> *"Short cuts make long delays."* — Pippin

While in chat mode, these `/commands` are available:

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/mode [plan\|build]` | Switch mode or show current |
| `/model <name>` | Switch model |
| `/session <name>` | Switch or create session |
| `/share [telegram]` | Share session to another interface |
| `/clear` | Clear conversation history |
| `/save` | Save current session |
| `/load <name>` | Load a saved session |
| `/export <file>` | Export conversation to file |
| `/tokens` | Show token usage |
| `/quit` or `/exit` | Exit chat |

---

## @Agent Mentions

> *"You shall not pass!"* — Gandalf

Address specific agents directly in your messages:

```
@reviewer please check my changes to auth.rs
@worker implement the fix that reviewer suggested
@gguf use the local model for this task
```

Available agents depend on your fellowship configuration. Default agents:

| Agent | Role |
|-------|------|
| `@worker` | Primary task executor |
| `@reviewer` | Code review and analysis |
| `@gguf` | Local model inference |

Markdown agents from `.mithril/agents/*.md` are also addressable:

```
@loremaster explain the architecture of this project
```

---

## Custom Commands

> *"The wise speak only of what they know."* — Gandalf

Define custom commands in your fellowship configuration:

```yaml
# .mithril/fellowship.yaml
commands:
  /review:
    description: "Review current changes"
    prompt: "Review the current git diff and suggest improvements"
    agent: reviewer
    
  /test:
    description: "Run and analyze tests"
    prompt: "Run the test suite and analyze any failures"
    agent: worker
    
  /docs:
    description: "Update documentation"
    prompt: "Update documentation to reflect recent code changes"
    agent: worker
```

Usage:
```
/review
/test
/docs
```

---

## Mode Switching

Press `Tab` in TUI mode to toggle between Plan and Build:

| Mode | File Writes | Terminal | Use Case |
|------|-------------|----------|----------|
| **Plan** | ❌ Blocked | Read-only | Analysis, exploration |
| **Build** | ✅ Allowed | Full access | Implementation |

The status bar shows current mode.

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `MITHRIL_MODEL` | Default model |
| `MITHRIL_PORT` | Server port |
| `MITHRIL_LOG` | Log level (debug, info, warn, error) |
| `MITHRIL_HOME` | Configuration directory |

---

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | General error |
| `2` | Configuration error |
| `3` | Model not found |
| `4` | Provider error |

---

> *"Home is behind, the world ahead."*
