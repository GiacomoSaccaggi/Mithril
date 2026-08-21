# CLI Reference

All commands exposed by the `mithril` binary.

---

## Command Overview

```
mithril <COMMAND>

Commands:
  start           Start server + interactive chat in one command (recommended)
  serve           Start the HTTP server (Ollama + OpenAI + MCP APIs)
  chat            Interactive chat (TUI by default, --plain for REPL)
  flow            Run agentic Planner→Tools flow on a message
  fellowship      Multi-agent fellowship orchestration
  exec            Run agentic task non-interactively (CI/CD)
  forge           Single inference and print result
  config          Manage credentials and settings
  scan            Build the Palantír BM25 semantic index
  undo            Undo the last shadow log session
  download-model  Download a GGUF model from HuggingFace
  mcp-stdio       Start MCP server over stdin/stdout
  telegram        Start the Telegram bot frontend
  sessions        Manage saved chat sessions

Options:
  -h, --help     Print help
  -V, --version  Print version
```

---

## `mithril start`

**Recommended way to use Mithril.** Starts the HTTP server in a background tokio task and opens the interactive TUI chat in the same terminal.

```bash
mithril start [--port 16180]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--port`, `-p` | `16180` | TCP port for the server |

**Behavior:**
- Kills any existing process on the port
- Spawns the HTTP server in background
- Waits for port to be available (max 5s)
- Opens TUI chat using the default fellowship

---

## `mithril init`

Analyze the project and generate a `MITHRIL.md` steering file that gives the LLM persistent project context.

```bash
mithril init
```

**What it does:**
- Scans project structure (languages, file counts, line counts)
- Detects build system (Cargo, npm, pyproject, CMake, etc.)
- Identifies frameworks and entry points
- Lists key directories and configuration files
- Generates `MITHRIL.md` with all findings + placeholder sections for conventions/rules

**The generated file is automatically injected into every LLM conversation** via the steering system. Edit it to add project-specific rules.

```
  🔍 Analyzing project...
  ✅ Generated MITHRIL.md (45 lines)
  💡 Tip: Commit MITHRIL.md to version control so your team shares the same project context.
```

---

## `mithril serve`

Start the HTTP server exposing Ollama, OpenAI, and MCP APIs.

```bash
mithril serve [--port 16180]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--port`, `-p` | `16180` | TCP port to listen on |

**Endpoints started:**

| API | URL |
|-----|-----|
| Ollama | `http://localhost:16180/api/chat` |
| OpenAI | `http://localhost:16180/v1/chat/completions` |
| MCP | `http://localhost:16180/mcp` |
| Health | `http://localhost:16180/health` |

The server intercepts `Ctrl+C` (SIGINT) and unloads the model from GPU memory before exiting.

---

## `mithril chat`

Interactive chat. Opens a **full TUI** (ratatui-based) by default. Use `--plain` for the classic readline REPL.

```bash
mithril chat [FELLOWSHIP] [--session <id>] [--plain] [--no-confirm]
```

| Argument/Option | Description |
|-----------------|-------------|
| `FELLOWSHIP` | Optional fellowship name (uses `.mithril/fellowships/<name>.yaml`). Omit to use default `.mithril/fellowship.yaml` |
| `--session` | Resume an existing session by UUID |
| `--plain` | Use readline REPL instead of TUI |
| `--no-confirm` | Skip all tool confirmation prompts (auto-approve everything) |

### TUI Mode (default)

- Full-screen ratatui terminal UI
- Dwarf mining splash animation on startup
- Split-pane: scrollable output + input area
- Non-blocking: agent loop runs in background task, UI stays responsive
- **Status bar** — Shows `Mithril | fellowship_name | BUILD/PLAN | session_id`
- **Tab key** (when input is empty): toggle between **BUILD** mode (full tool access) and **PLAN** mode (read-only analysis only)
- **Multiline input** — **Shift+Enter** inserts a newline; **Enter** sends
- **Suggestion accept** — Press **Enter** on a suggestion to accept and send immediately
- **@file injection** — Type `@path/to/file` to inject file content into context
- **Agent traces** — Agent work shown dimmed (`┄┄` headers, `⚙` tools, `→` delegations); final response shown normal
- Themed colors via `src/tui/theme.rs`

### Plain REPL Multiline

In `--plain` mode, end a line with `\` to continue on the next line:

```
> write a function that \
… parses config from YAML \
… and validates all fields
```

### @file References

Attach file content directly in your prompt using `@path/to/file`:

```
> explain @src/main.rs
> compare @src/old.rs with @src/new.rs
> look at @"path/with spaces/file.rs"
```

The file content is automatically expanded and injected before sending to the LLM.

### In-chat commands

| Command | Description |
|---------|-------------|
| `/exit`, `/quit`, `/q` | Exit and save session |
| `/clear`, `/c` | Clear conversation history |
| `/compact` | Summarize conversation to free context window |
| `/fellowship` | Show current fellowship agents and their roles |
| `/undo` | Undo last action (reverts conversation + file changes) |
| `/redo` | Redo undone action |
| `/session` | Show session ID, fellowship, message count, active frontend |
| `/history` | Show all messages with content preview |
| `/help`, `/h` | Show command list |

---

## `mithril fellowships`

List all available fellowship configurations.

```bash
mithril fellowships
```

**Discovery order:**
1. `.mithril/fellowship.yaml` — default fellowship (always listed first)
2. `.mithril/fellowships/*.yaml` — named fellowships (listed alphabetically)

**Output example:**
```
Available fellowships:
  (default)         Architect + Coder for code review
  fast-groq         Fast Groq-only for quick tasks
  research          Multi-provider with evaluation
```

Use any listed name with `mithril chat`:
```bash
mithril chat              # uses default
mithril chat fast-groq    # uses fast-groq
```

---

## `mithril flow`

Run the **agentic Planner→Tools loop** on a message. The planner reasons, calls tools, feeds results back, and repeats until the task is complete or `max_iterations` is reached.

```bash
mithril flow "refactor auth.rs to use traits" [--config path]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--config`, `-c` | `.mithril-flow.yaml` | Path to flow configuration file |

### Configuration (`.mithril-flow.yaml`)

```yaml
version: "1.0"
name: "Gemini + MCP Tools"
max_iterations: 10

planner:
  name: "Gemini"
  provider: gemini
  system_prompt: |
    You are a senior software engineer assistant...
  tools:
    - read_psi
    - write_file
    - grep_files
    - find_file
    - list_files
    - run_terminal
    - git_status
    - git_diff
```

**Config resolution order:**
1. Explicit `--config` path
2. `.mithril-flow.yaml` in current directory
3. `~/.mithril/flows/default.yaml`
4. Built-in default (Gemini planner, all tools)

**Algorithm:**
```
Planner → chat_with_tools() → ToolCalls? → Execute → Feed Results → Repeat
                             → Text?     → Done (print final response)
```

---

## `mithril fellowship`

Manage and inspect the multi-agent Fellowship system. Fellowship is **always active** in chat mode — every `mithril chat` session uses a fellowship configuration.

```bash
mithril fellowship [action]
```

| Action | Description |
|--------|-------------|
| `status` (default) | Show all agents with their roles, tools, and delegation permissions |
| `init` | Create a template `.mithril/fellowship.yaml` |
| `test` | Test connectivity to each agent (API call or PATH check) |

### Fellowship Architecture

The Fellowship is Mithril's multi-agent orchestration — **the core of chat mode**:

1. **GGUF Controller** — A fast, free local model (e.g. `qwen-1.5b`) classifies each user message and picks the first agent based on agent `when` descriptions
2. **Agent Free-Flow** — Agents communicate via the `NEXT:/TASK:` protocol, delegating to each other as needed
3. **GGUF as Worker** — Any agent can call `"gguf"` for trivial tasks (free local inference with tools)
4. **Rust Enforcement** — `can_call` permissions, `max_rounds`, and `token_budget` are enforced by the runtime

### NEXT:/TASK: Protocol

Agents end their responses with a protocol line that controls the flow:

| Protocol | Meaning |
|----------|---------|
| `NEXT: DONE` | Task complete — return final response to user |
| `NEXT: agent_name` | Delegate to another agent |
| `NEXT: gguf` | Delegate to free local GGUF model |
| `TASK: description` | (follows NEXT:) Task description for the next agent |

**Example agent response:**
```
I've implemented the feature. The code compiles and tests pass.

NEXT: reviewer
TASK: Review the changes in src/auth.rs for security best practices
```

### Fellowship Configuration

```yaml
# .mithril/fellowship.yaml
name: "mithril-dev"
description: "Development fellowship"

controller:
  provider: local
  model: qwen-1.5b
  context_window: 2            # Messages shown to controller for classification

max_rounds: 15                 # Maximum agent-to-agent delegations
token_budget: 50000            # Total token budget across all agents

agents:
  - name: "worker"
    provider: gemini
    model: gemini-3.6-flash
    role: "General worker"
    when: "any task"
    can_call: ["reviewer", "gguf"]
    tools: ["*"]

  - name: "reviewer"
    provider: gemini
    model: gemini-2.5-pro
    role: "Senior reviewer. EXPENSIVE."
    when: "explicit review request"
    can_call: ["worker"]
    tools: ["read_psi", "grep_files", "git_diff"]
```

### Configuration Fields

| Field | Description |
|-------|-------------|
| `name` | Fellowship identifier |
| `description` | Human-readable description |
| `controller.provider` | Provider for the entry classifier (usually `local`) |
| `controller.model` | Model for classification (e.g. `qwen-1.5b`) |
| `controller.context_window` | Number of recent messages shown to controller |
| `max_rounds` | Maximum agent-to-agent delegations before forced DONE |
| `token_budget` | Total token budget across all agents |
| `agents[].name` | Unique agent identifier |
| `agents[].provider` | Provider for this agent (`gemini`, `openai`, `groq`, `local`) |
| `agents[].model` | Model for this agent |
| `agents[].role` | Agent role description (shown in prompts) |
| `agents[].when` | Hint for controller on when to pick this agent |
| `agents[].can_call` | List of agents this agent can delegate to |
| `agents[].tools` | Tool access (`["*"]` for all, or explicit list) |

### Status Output

`mithril fellowship status` shows all agents with their roles, tools, and delegation permissions.

---

## `mithril exec`

Run the agentic loop **non-interactively**. Designed for CI/CD, scripts, and git hooks.

```bash
mithril exec "add tests for auth.rs" [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--model` | from config | Model override |
| `--json` | — | Output result as structured JSON |
| `--quiet`, `-q` | — | Suppress traces, print only final response |
| `--max-iterations` | `10` | Maximum tool-calling iterations |
| `--system` | built-in | Custom system prompt |

### Output Modes

**Normal** (default): Prints trace + final response
```bash
mithril exec "add error handling to parse_config"
```

**JSON** (`--json`): Structured output for programmatic use
```json
{
  "response": "Added error handling...",
  "iterations": 3,
  "tools_called": [
    {"name": "read_psi", "success": true},
    {"name": "edit_file", "success": true}
  ]
}
```

**Quiet** (`--quiet`): Only the final response text (no trace, no formatting)
```bash
result=$(mithril exec --quiet "what is the default port?")
```

**Exit codes:**
- `0` — task completed successfully
- `2` — max iterations reached without completion

---

## `mithril forge`

Run a single inference and print the result to stdout. Useful for scripting.

```bash
mithril forge "Explain Rust lifetimes in one paragraph"
```

Uses the default model (`qwen-1.5b`) and ChatML template. Exits after printing.

---

## `mithril config`

Manage credentials and settings stored at `~/.mithril/config.yaml`.
All API keys are encrypted with **Argon2id + AES-256-GCM** before storage.

```bash
mithril config [list|set|unset|get|path] [key] [value]
```

### Actions

| Action | Usage | Description |
|--------|-------|-------------|
| `list` (default) | `mithril config` | Show all settings and credential status |
| `set` | `mithril config set gemini "AIza..."` | Set a value or credential |
| `unset` | `mithril config unset gemini` | Remove a credential |
| `get` | `mithril config get provider` | Get a single value |
| `path` | `mithril config path` | Show config file path |

### Configuration keys

| Key | Description | Example |
|-----|-------------|---------|
| `provider` | Default provider | `gemini` |
| `model` | Default local model | `qwen-7b` |
| `gemini` | Gemini API key (encrypted) | `mithril config set gemini "AIza..."` |
| `openai` | OpenAI API key (encrypted) | `mithril config set openai "sk-..."` |
| `anthropic` | Anthropic API key (encrypted) | `mithril config set anthropic "sk-ant-..."` |
| `groq` | Groq API key (encrypted) | `mithril config set groq "gsk_..."` |
| `telegram` | Telegram bot token (encrypted) | `mithril config set telegram "123:abc..."` |
| `gemini-model` | Gemini model override | `gemini-1.5-pro` |
| `openai-model` | OpenAI model override | `gpt-4o` |
| `openai-base-url` | Custom OpenAI-compatible URL | `https://api.groq.com/openai/v1` |
| `anthropic-model` | Anthropic model override | `claude-opus-4-20250514` |
| `groq-model` | Groq model override | `llama-3.3-70b-versatile` |
| `terminal_sandbox` | Block dangerous shell commands | `false` to disable |
| `key_password` | Strengthen credential encryption (stored in `~/.mithril/secrets`) | any string |
| `api_token` | Bearer token for HTTP routes (stored in `~/.mithril/secrets`) | any string |

### Permissions (per-tool access control)

Configure per-tool permissions in `~/.mithril/config.yaml`:

```yaml
permissions:
  run_terminal: allow    # Never ask for confirmation
  delete_file: deny      # Completely disable this tool
  write_file: ask        # Ask before each use (default for dangerous tools)
```

**Permission levels:**
- `allow` — execute without asking (default for safe tools like `read_psi`, `list_files`)
- `deny` — completely disable; the LLM gets an error if it tries to use it
- `ask` — prompt user for confirmation (default for `write_file`, `edit_file`, `apply_patch`, `delete_file`, `run_terminal`)

Use `--no-confirm` flag with `mithril chat` to auto-approve everything for a session.

---

## `mithril scan`

Build or update the Palantír BM25 semantic index for the current directory.

```bash
mithril scan
```

The index is saved to `.celebrimbot/palantir_index.json`. Subsequent runs only re-index changed files (incremental). Used by Junie and other MCP clients to inject relevant files into context instead of the full codebase.

---

## `mithril undo`

Revert all file changes made in the last shadow log session.

```bash
mithril undo
```

The shadow log tracks every `write_file` and `delete_file` tool call. Each `mithril chat` or `mithril telegram` session is a separate shadow log group.

---

## `mithril download-model`

Download a GGUF model from HuggingFace to `~/.mithril/models/`.

```bash
mithril download-model --model qwen-1.5b
mithril download-model --list
```

| Option | Default | Description |
|--------|---------|-------------|
| `--model`, `-m` | `qwen-1.5b` | Model ID to download |
| `--list`, `-l` | — | List all available models |

**Available models:**

| ID | Description | Size |
|----|-------------|------|
| `qwen-1.5b` | Qwen 2.5 Coder 1.5B — fast | ~1.2 GB |
| `qwen-7b` | Qwen 2.5 Coder 7B — powerful | ~4.5 GB |
| `llama-8b` | Llama 3.1 8B Instruct | ~5 GB |
| `deepseek-6.7b` | DeepSeek Coder 6.7B | ~4.5 GB |
| `phi-3.5` | Phi-3.5 Mini 3.8B | ~2.5 GB |

Uses atomic rename (`.gguf.tmp` → `.gguf`) to prevent partial downloads.

---

## `mithril mcp-stdio`

Start the MCP server over stdin/stdout for Claude Desktop, Cursor, and other MCP clients.

```bash
mithril mcp-stdio
```

Reads JSON-RPC 2.0 requests from stdin (one per line), dispatches via the same handler as `/mcp` HTTP, writes responses to stdout.

**Claude Desktop config** (`~/Library/Application Support/Claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "mithril": {
      "command": "/path/to/mithril",
      "args": ["mcp-stdio"]
    }
  }
}
```

**Junie / Kiro config** (`.kiro/mcp.json` in project root):

```json
{
  "mcpServers": {
    "mithril": {
      "command": "/path/to/mithril",
      "args": ["mcp-stdio"]
    },
    "scomp-link": {
      "command": "scomp-link",
      "args": ["mcp"]
    }
  }
}
```

---

## `mithril telegram`

Start the Telegram bot frontend, attached to a new or existing session.

```bash
mithril telegram [--session <id>]
```

| Option | Description |
|--------|-------------|
| `--session` | Resume an existing session by UUID |

**Requires** a bot token configured:

```bash
mithril config set telegram "<token-from-BotFather>"
```

**Provider selection**: automatically picks the best available cloud provider (`gemini` > `openai` > `anthropic` > `local`) for fast mobile responses.

**Telegram bot commands:**

| Command | Description |
|---------|-------------|
| Any message | Sent to LLM, response returned |
| `/session` | Show session ID, provider, message count |
| `/stop` | Release Telegram frontend, return to terminal |

---

## `mithril sessions`

Manage saved chat sessions stored at `~/.mithril/sessions/`.

```bash
mithril sessions <action> [id]
```

| Action | Usage | Description |
|--------|-------|-------------|
| `list` (default) | `mithril sessions list` | List all sessions, sorted by last update |
| `show` | `mithril sessions show <id>` | Show full history of a session |
| `delete` | `mithril sessions delete <id>` | Delete a session from disk |
