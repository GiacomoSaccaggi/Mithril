# Mithril Technical Documentation

> *"Mithril! All folk desired it."* — Gandalf

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Module Reference](#module-reference)
4. [Flow System](#flow-system)
5. [Fellowship (Multi-Agent)](#fellowship-multi-agent)
6. [TUI](#tui)
7. [Key Design Decisions](#key-design-decisions)

---

## Overview

Mithril is a lightweight, standalone LLM inference engine written in Rust. It serves GGUF models via multiple API interfaces, provides built-in tools for file manipulation, terminal execution, git operations and web search, and supports persistent shared sessions handed off between Terminal, Telegram, and Junie.

### Key Features

- **Single Binary** — No JVM, Python, or external runtime required
- **Multiple APIs** — Ollama-compatible, OpenAI-compatible, MCP (JSON-RPC 2.0)
- **TUI** — Full ratatui-based terminal UI with splash animation (default for `mithril chat`)
- **Lazy Loading** — Models load on first inference, auto-unload after idle timeout
- **GPU Acceleration** — Automatic Metal support on Apple Silicon
- **Real Streaming** — Token-by-token via `std::sync::mpsc` → `tokio::sync::mpsc` bridge
- **21 Built-in Tools** — File, terminal (sandboxed), git, web search, code intelligence, lore, patch
- **36 Tools with scomp-link** — 15 additional ML tools via external MCP subprocess
- **Multi-Provider** — Local GGUF + Gemini, OpenAI, Anthropic, Groq
- **Flow System** — Configurable agentic Planner→Tools loop via `.mithril-flow.yaml`
- **Fellowship** — Multi-agent orchestration with GGUF controller + agent free-flow via NEXT:/TASK: protocol
- **SharedSession** — Persistent chat history handed off between Terminal, Telegram, Junie
- **Telegram Bot** — Continue any session from your phone
- **Headless Exec** — Non-interactive agentic mode for CI/CD
- **BM25 Semantic Index** — Palantír index for fast project context retrieval
- **Shadow Log** — Backup/undo for all file operations
- **Argon2id Credentials** — AES-256-GCM encryption with proper KDF

---

## Architecture

### Source Tree

```
src/
├── main.rs              # CLI entry point (clap Subcommands)
├── lib.rs               # Module exports
│
├── cli/                 # 19 CLI command modules
│   ├── mod.rs           # Exports all submodules
│   ├── start.rs         # `mithril start` — server + TUI in one
│   ├── serve.rs         # `mithril serve` — HTTP server only
│   ├── chat.rs          # `mithril chat` — interactive (TUI or plain REPL)
│   ├── fellowships.rs   # `mithril fellowships` — list available fellowships
│   ├── flow.rs          # `mithril flow` — agentic flow runner
│   ├── fellowship.rs    # `mithril fellowship` — multi-agent management
│   ├── exec.rs          # `mithril exec` — headless CI/CD mode
│   ├── agent_loop.rs    # Shared agentic loop (chat + exec)
│   ├── compact.rs       # /compact — conversation compaction
│   ├── steering.rs      # Steering file loader (.mithril/steering/)
│   ├── config.rs        # `mithril config`
│   ├── forge.rs         # `mithril forge`
│   ├── scan.rs          # `mithril scan`
│   ├── undo.rs          # `mithril undo`
│   ├── download.rs      # `mithril download-model`
│   ├── mcp_stdio.rs     # `mithril mcp-stdio`
│   ├── telegram.rs      # `mithril telegram`
│   └── sessions.rs      # `mithril sessions`
│
├── tui/                 # Full terminal user interface (ratatui)
│   ├── mod.rs           # TUI entry point, agent message channel
│   ├── app.rs           # App state, message routing
│   ├── ui.rs            # Layout rendering
│   ├── events.rs        # Keyboard/event handling
│   ├── splash.rs        # Startup animation (9 frames)
│   └── theme.rs         # Color theming
│
├── flow/                # Multi-agent flow system
│   ├── mod.rs           # Exports
│   ├── config.rs        # FlowConfig — .mithril-flow.yaml parser
│   ├── runner.rs        # FlowRunner — Planner→Tools loop
│   ├── fellowship.rs    # FellowshipConfig — multi-agent definitions
│   ├── orchestrator.rs  # Orchestrator — Controller→Agents coordination
│   └── tokens.rs        # Token tracking per-agent
│
├── api/                 # HTTP server layer
│   ├── mod.rs           # Exports
│   ├── server.rs        # Axum server setup, CORS, routing
│   ├── ollama.rs        # Ollama-compatible endpoints
│   ├── openai.rs        # OpenAI-compatible endpoints
│   └── mcp.rs           # MCP JSON-RPC 2.0 handler
│
├── engine/              # Model inference engine
│   ├── mod.rs           # Exports
│   ├── lazy_model.rs    # LazyModelManager (load/unload/infer)
│   ├── chat_template.rs # ChatML, Llama3, Phi3 templates
│   └── model_catalog.rs # ModelInfo + HuggingFace URLs
│
├── providers/           # Chat provider backends
│   ├── mod.rs           # ChatProvider trait, create_provider factory
│   ├── local.rs         # LocalProvider (llama-cpp-2)
│   ├── gemini.rs        # GeminiProvider (SSE streaming)
│   ├── openai.rs        # OpenAIProvider (SSE + tool calling)
│   ├── anthropic.rs     # AnthropicProvider (SSE + tool calling)
│   └── groq.rs          # GroqProvider (SSE + tool calling + compound)
│
├── tools/               # MCP tool registry + implementations
│   ├── mod.rs           # create_default_registry() — registers 21 tools
│   ├── registry.rs      # Tool trait, ToolRegistry, JSON schema export
│   └── implementations.rs # 23 tool structs (21 registered)
│
├── operators/           # Low-level I/O operators
│   ├── mod.rs           # Exports
│   ├── file.rs          # FileOperator (read/write/delete)
│   ├── terminal.rs      # TerminalOperator (shell + sandbox)
│   ├── git.rs           # GitOperator (status/log/diff/blame/branch)
│   ├── web.rs           # WebOperator (search + fetch)
│   ├── scan.rs          # ScanOperator (list/grep/find/stats/symbols)
│   └── shadow.rs        # ShadowOperator (backup/restore)
│
├── config/              # Configuration + credential encryption
│   └── mod.rs           # MithrilConfig, SecretsFile, Argon2id+AES-256-GCM
│
├── session/             # Persistent shared sessions
│   └── mod.rs           # SharedSession, frontend handoff, save/load
│
├── index/               # Search indexing
│   ├── mod.rs           # Exports
│   └── palantir.rs      # Palantír BM25 full-text index
│
└── bin/
    └── debug.rs         # Debug utility binary
```

### Architecture Diagram

```
┌──────────────────────────────────────────────────────────────────────┐
│                        CLI (main.rs + clap)                           │
│  start  serve  chat  flow  fellowship  exec  forge  scan  undo       │
│  download  mcp-stdio  telegram  sessions  config                     │
└──────────────────────────────────────────────────────────────────────┘
         │           │           │            │              │
         ▼           ▼           ▼            ▼              ▼
┌─────────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐
│  TUI        │ │ API      │ │ Flow     │ │ Providers│ │ Session  │
│ (ratatui)   │ │ server   │ │ system   │ │          │ │          │
│             │ │ ollama   │ │ runner   │ │ local    │ │ Shared   │
│ app.rs      │ │ openai   │ │ orchestr │ │ gemini   │ │ Session  │
│ ui.rs       │ │ mcp      │ │ fellowsh │ │ openai   │ │ (Arc     │
│ events.rs   │ │          │ │ tokens   │ │ anthropic│ │  Mutex)  │
│ splash.rs   │ │          │ │          │ │ groq     │ │          │
│ theme.rs    │ │          │ │          │ │          │ │          │
└─────────────┘ └──────────┘ └──────────┘ └──────────┘ └──────────┘
         │           │           │            │              │
         └───────────┴─────┬─────┴────────────┘              │
                           ▼                                  │
              ┌───────────────────────┐                       │
              │   Tools (21)          │◀──────────────────────┘
              │   ToolRegistry        │
              │   agent_loop.rs       │
              └───────────────────────┘
                           │
              ┌────────────┴────────────┐
              ▼                         ▼
┌───────────────────────┐  ┌───────────────────────┐
│   Operators           │  │   Index                │
│   file, terminal(sb)  │  │   palantir BM25        │
│   git, web, scan      │  └───────────────────────┘
│   shadow              │
└───────────────────────┘  ┌───────────────────────┐
                           │   External MCP         │
              ┌────────┐   │   scomp-link (15 ML)   │
              │ Engine │   └───────────────────────┘
              │ lazy   │
              │ tmpl   │   ┌───────────────────────┐
              │ catalog│   │   Config               │
              └────────┘   │   Argon2id+AES-256-GCM │
                           │   SecretsFile          │
                           └───────────────────────┘
```

### Data Flow

1. **HTTP Request** → `server.rs` routes to handler
2. **Handler** (ollama/openai/mcp) → calls `LazyModelManager` or provider
3. **LazyModelManager** → loads model if needed, runs inference on dedicated thread
4. **Streaming** → `std::mpsc` → bridge thread → `tokio::mpsc` → HTTP response
5. **Tool Calls** → `ToolRegistry` dispatches to tool implementations
6. **Chat Mode** → Fellowship Orchestrator → GGUF Controller → Agent → tool calls → execute → NEXT:/TASK: protocol → repeat or DONE
7. **TUI** → receives `AgentMessage` via mpsc channel, renders non-blocking
8. **Session** → `SharedSession` auto-saved to `~/.mithril/sessions/`
9. **Telegram** → bot polls Telegram API, reads/writes `SharedSession`
10. **scomp-link** → launched as separate process via `mcp.json`, proxied by MCP client

---

## Flow System

The Flow system (`src/flow/`) provides configurable agentic loops.

### Components

| File | Struct | Purpose |
|------|--------|---------|
| `config.rs` | `FlowConfig` | Parse `.mithril-flow.yaml` |
| `runner.rs` | `FlowRunner` | Execute Planner→Tools loop |
| `fellowship.rs` | `FellowshipConfig` | Multi-agent configuration |
| `orchestrator.rs` | `Orchestrator` | Controller→Agents coordination |
| `tokens.rs` | `SessionTokens` | Per-agent token tracking |

### FlowConfig

```rust
pub struct FlowConfig {
    pub name: String,           // Human-readable flow name
    pub version: String,        // Informational
    pub planner: AgentConfig,   // The reasoning agent
    pub worker: Option<AgentConfig>, // Reserved for Phase 2
    pub max_iterations: u32,    // Safety cap
}

pub struct AgentConfig {
    pub name: String,           // Display name
    pub provider: String,       // "gemini", "openai", etc.
    pub model: Option<String>,  // Model override
    pub system_prompt: String,  // Injected at start
    pub tools: Vec<String>,     // Allowed tool names (["*"] = all)
}
```

### FlowRunner Algorithm

```
1. Build tool definitions (filtered by agent's tools list)
2. Send system prompt + user message to planner via chat_with_tools()
3. If response = ToolCalls → execute each, add results to history, goto 2
4. If response = Text → return final response
5. If max_iterations reached → return whatever we have
```

---

## Fellowship (Multi-Agent)

The Fellowship system is **the core of chat mode** — every `mithril chat` session uses a fellowship configuration with free-flow agent communication.

### FellowshipConfig

```rust
pub struct FellowshipConfig {
    pub name: String,
    pub description: Option<String>,
    pub controller: ControllerConfig,           // GGUF entry classifier
    pub agents: Vec<FellowshipAgent>,           // Specialist agents
    pub max_rounds: u32,                        // Max agent-to-agent delegations
    pub token_budget: u64,                      // Total token budget
    pub token_tracking: TokenTrackingConfig,
}

pub struct ControllerConfig {
    pub provider: String,               // Usually "local" for free GGUF
    pub model: Option<String>,          // e.g. "qwen-1.5b"
    pub context_window: usize,          // Messages shown to controller
}

pub struct FellowshipAgent {
    pub name: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub role: String,                   // Agent role description
    pub when: Option<String>,           // Hint for controller classification
    pub can_call: Option<Vec<String>>,  // Allowed delegation targets
    pub tools: Option<Vec<String>>,     // Tool access
}
```

### Orchestrator Algorithm (Entry Classify → Agent Free-Flow)

Every chat message goes through the fellowship orchestrator:

```
1. Load fellowship config (default or named from CLI arg)
2. GGUF Controller classifies the entry message:
   - Shows last N messages (context_window) + user message
   - Agent `when` descriptions are included in the prompt
   - Controller picks the best-matching agent
3. Selected agent runs with its tools and system prompt
4. Agent ends response with protocol line:
   - NEXT: DONE → return final response to user
   - NEXT: agent_name → delegate to another agent (if in can_call)
   - NEXT: gguf → delegate to free local GGUF model
5. If NEXT: agent_name, goto 3 with the delegated agent
6. Track tokens per-agent throughout the flow
7. Enforce max_rounds and token_budget limits
```

### NEXT:/TASK: Protocol

Agents communicate via a simple protocol appended to their responses:

| Protocol | Meaning |
|----------|---------|
| `NEXT: DONE` | Task complete — return final response to user |
| `NEXT: agent_name` | Delegate to another agent (must be in `can_call`) |
| `NEXT: gguf` | Delegate to free local GGUF model for trivial tasks |
| `TASK: description` | (follows NEXT:) Task description for the next agent |

**Example agent response:**
```
I've implemented the authentication module.

NEXT: reviewer
TASK: Review the changes in src/auth.rs for security issues
```

### GGUF as Cheap Worker

Any agent with `"gguf"` in their `can_call` list can delegate trivial tasks to the free local GGUF model. This is useful for:
- Simple file reads
- Basic text formatting
- Quick lookups
- Offloading routine work from expensive cloud models

### Rust Enforcement

The Rust runtime enforces safety constraints:
- **`can_call` permissions** — Agents can only delegate to allowed targets
- **`max_rounds`** — Prevents infinite delegation loops
- **`token_budget`** — Hard limit on total token usage across all agents

### Token Tracking

```rust
pub struct TokenUsage { pub input: u64, pub output: u64 }
pub struct SessionTokens { pub trackers: HashMap<String, AgentTokenTracker> }
```

Tracks input/output tokens per agent, supports display formatting (`1.5k in / 800 out`) and total aggregation.

---

## TUI

The TUI (`src/tui/`) is a full ratatui-based terminal interface, default for `mithril chat`.

### Architecture

```
┌─ Terminal ─────────────────────────────────────┐
│  ┌─ Splash (9 frames) ─┐                      │
│  └──────────────────────┘                      │
│  ┌─ App ───────────────────────────────────┐   │
│  │  UI (split pane: output + input)        │   │
│  │  Events (keyboard handler)              │   │
│  │  Theme (color palette)                  │   │
│  └─────────────────────────────────────────┘   │
│              ↕ mpsc channel                    │
│  ┌─ Background Task ──────────────────────┐   │
│  │  Agent Loop → Provider → Tools          │   │
│  │  Sends AgentMessage { ToolCall | Done } │   │
│  └─────────────────────────────────────────┘   │
└────────────────────────────────────────────────┘
```

### AgentMessage enum

```rust
enum AgentMessage {
    ToolCall { name: String, success: bool, preview: String, target: Option<String> },
    Done { response: String, iterations: u32, messages_to_persist: Vec<ChatMessage> },
    Error(String),
}
```

The TUI render loop polls for `AgentMessage` events without blocking — the agent can run multi-second tool loops while the UI remains interactive.

---

## Module Reference

See linked files for full documentation:

| Module | File | Description |
|--------|------|-------------|
| Engine | [ENGINE.md](./ENGINE.md) | LazyModelManager, streaming, chat templates |
| API | [API.md](./API.md) | HTTP server, Ollama/OpenAI/MCP endpoints |
| Providers | [PROVIDERS.md](./PROVIDERS.md) | Local + cloud providers, streaming, tool calling |
| Tools | [TOOLS.md](./TOOLS.md) | Tool registry, 21 built-in tools |
| Operators | [OPERATORS.md](./OPERATORS.md) | File, terminal (sandbox), git, web, scan, shadow |
| Index | [INDEX.md](./INDEX.md) | Palantír BM25 semantic search |
| CLI | [CLI.md](./CLI.md) | All commands (start, flow, fellowship, exec, etc.) |
| Session | [SESSION.md](./SESSION.md) | SharedSession, Telegram handoff, Junie MCP tools |
| Compatibility | [COMPATIBILITY.md](./COMPATIBILITY.md) | All client integrations + scomp-link |
| Token Efficiency | [TOKEN_EFFICIENCY.md](./TOKEN_EFFICIENCY.md) | BM25, shadow diff, MCP on-demand |
| Security | [SECURITY.md](./SECURITY.md) | Argon2id, sandbox, API token, secrets |

---

## Module Dependency Graph

```
main.rs
├── cli/
│   ├── start.rs       → api/server.rs, tui/, flow/orchestrator
│   ├── serve.rs       → api/server.rs
│   ├── chat.rs        → flow/orchestrator, session/, tools/, tui/
│   ├── fellowships.rs → flow/fellowship.rs (config loading)
│   ├── flow.rs        → flow/runner.rs
│   ├── fellowship.rs  → flow/orchestrator.rs
│   ├── exec.rs        → providers/, tools/, agent_loop.rs
│   ├── agent_loop.rs  → providers/, tools/ (shared by chat+exec)
│   ├── compact.rs     → providers/ (summarization)
│   ├── steering.rs    → (filesystem)
│   ├── forge.rs       → engine/
│   ├── scan.rs        → index/palantir.rs
│   ├── undo.rs        → operators/shadow.rs
│   ├── download.rs    → engine/model_catalog.rs
│   ├── mcp_stdio.rs   → api/mcp.rs, tools/
│   ├── telegram.rs    → session/, providers/
│   └── sessions.rs    → session/
│
├── tui/
│   ├── mod.rs         → providers/, tools/, session/, cli/agent_loop
│   ├── app.rs         → (state management)
│   ├── ui.rs          → ratatui widgets
│   ├── events.rs      → crossterm events
│   ├── splash.rs      → ratatui animation
│   └── theme.rs       → color constants
│
├── flow/
│   ├── config.rs      → serde_yaml
│   ├── runner.rs      → providers/, tools/
│   ├── fellowship.rs  → serde_yaml
│   ├── orchestrator.rs → providers/, tools/, cli/agent_loop
│   └── tokens.rs      → (standalone)
│
├── api/
│   ├── server.rs      → engine/, tools/, session/
│   ├── ollama.rs      → engine/ (streaming bridge)
│   ├── openai.rs      → engine/
│   └── mcp.rs         → tools/registry.rs
│
├── session/
│   └── mod.rs         → providers/ChatMessage (Arc Mutex history)
│
├── engine/
│   ├── lazy_model.rs  → llama-cpp-2 (mpsc streaming)
│   ├── chat_template  → (standalone)
│   └── model_catalog  → chat_template
│
├── providers/
│   ├── mod.rs         → ChatProvider trait + ToolDefinition + factory
│   ├── local.rs       → engine/
│   ├── gemini.rs      → reqwest (SSE + tool calling)
│   ├── openai.rs      → reqwest (SSE + tool calling)
│   ├── anthropic.rs   → reqwest (SSE + tool calling)
│   └── groq.rs        → reqwest (SSE + tool calling + compound)
│
├── tools/
│   ├── registry.rs    → (standalone)
│   ├── implementations.rs → operators/
│   └── mod.rs         → registry + implementations
│
├── operators/
│   ├── file.rs, terminal.rs (sandbox), git.rs
│   ├── web.rs, scan.rs, shadow.rs
│
├── config/
│   └── mod.rs         → Argon2id KDF, AES-256-GCM, MithrilConfig, SecretsFile
│
└── index/
    └── palantir.rs    → operators/scan.rs
```

---

## Key Design Decisions

### `!Send` Inference Thread

`LlamaModel` and `LlamaBackend` are `!Send + !Sync` — they cannot be moved across threads. Every inference call uses `std::thread::spawn` with `Arc<Mutex<ModelState>>`. Streaming bridges the gap with `std::sync::mpsc::SyncSender → spawn_blocking → tokio::sync::mpsc`.

### TUI Non-Blocking Agent

The TUI must never block on the agent loop. The agent runs in a `tokio::spawn` background task and sends `AgentMessage` variants via `tokio::sync::mpsc`. The render loop calls `try_recv()` on each tick — if there's a message, it updates state; otherwise it redraws and processes keyboard input.

### Single Mutex for Shared State

`SharedSession` uses `Arc<Mutex<Vec<ChatMessage>>>` (via `parking_lot::Mutex`) for history and `Arc<AtomicU8>` for the frontend flag. The atomic ensures frontend exclusivity without locking the full history on every check.

### Agent Loop Reuse

`cli/agent_loop.rs` contains the core loop logic (provider → tool calls → execute → feed back → repeat). It's used by:
- `chat.rs` (interactive mode, `TraceMode::Inline`)
- `exec.rs` (headless mode, `TraceMode::Full` or `Silent`)
- `tui/mod.rs` (background task, `TraceMode::Silent` with `AgentMessage` channel)

### Dangerous Tool Permission

In interactive chat, tools classified as "dangerous" (`write_file`, `edit_file`, `apply_patch`, `delete_file`, `run_terminal`) require user confirmation before execution. In headless `exec` mode, all tools execute without confirmation.

### Argon2id KDF

Credentials use `nonce(12) || salt(16) || ciphertext` format. The 16-byte random salt means every encryption produces a unique output even for the same input. Legacy v1 credentials (no salt) are auto-detected and read correctly.

### Terminal Sandbox

`validate_command()` in `terminal.rs` blocks patterns like `rm -rf /`, `sudo`, fork bombs, and disk erasure before the shell spawns. Configurable via `mithril config set terminal_sandbox false`.

### Steering Files

The `cli/steering.rs` module loads project context from:
1. `.mithril/steering/*.md` files in project root
2. `MITHRIL.md` in project root

These are injected into the system prompt for every provider call, giving the LLM persistent project knowledge without manual setup.
