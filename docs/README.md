# Mithril Documentation

> *Complete technical documentation for the Mithril LLM inference engine.*

## Document Index

| Document | Description |
|----------|-------------|
| [ARCHITECTURE.md](./ARCHITECTURE.md) | System overview, source tree, module structure, Flow & Fellowship, TUI, design decisions |
| [ENGINE.md](./ENGINE.md) | LazyModelManager, streaming bridge, chat templates, model catalog |
| [API.md](./API.md) | HTTP server (axum), Ollama/OpenAI/MCP endpoints, API token auth |
| [PROVIDERS.md](./PROVIDERS.md) | Multi-provider backends (local + 4 cloud), SSE streaming, tool calling |
| [TOOLS.md](./TOOLS.md) | Tool registry, 21 built-in tools (file, terminal, git, web, code intel, lore, patch) |
| [OPERATORS.md](./OPERATORS.md) | File, terminal (sandbox), git, web, scan, shadow operators |
| [INDEX.md](./INDEX.md) | Palantír BM25 semantic search index |
| [CLI.md](./CLI.md) | Full CLI reference: start, serve, chat, flow, fellowship, exec, forge, etc. |
| [SESSION.md](./SESSION.md) | SharedSession, Terminal↔Telegram↔Junie handoff |
| [COMPATIBILITY.md](./COMPATIBILITY.md) | Junie, Open WebUI, LangChain, Claude Desktop, Cursor |
| [TOKEN_EFFICIENCY.md](./TOKEN_EFFICIENCY.md) | How Mithril reduces token usage (BM25, shadow diff, MCP on-demand) |
| [SECURITY.md](./SECURITY.md) | Argon2id + AES-256-GCM, terminal sandbox, API token, secrets file |

## Quick Start

```bash
mithril start                    # Server + TUI chat in one command
mithril chat                     # TUI chat with default fellowship
mithril chat fast-groq           # TUI chat with named fellowship
mithril fellowships              # List available fellowship configs
mithril flow "add tests"         # Agentic loop (Planner→Tools)
mithril exec "fix the bug" --json  # Headless for CI/CD
```

## Key Architecture Concepts

- **12 source modules**: cli, tui, flow, api, engine, providers, tools, operators, config, session, index
- **21 built-in tools** + 15 scomp-link external = 36 total MCP tools
- **5 providers**: local (llama.cpp), Gemini, OpenAI, Anthropic, Groq
- **5 local models**: qwen-1.5b, qwen-7b, llama-8b, deepseek-6.7b, phi-3.5
- **TUI default**: `mithril chat` opens ratatui UI; `--plain` for readline REPL
- **Shared sessions**: JSON on disk, handed off between Terminal/Telegram/Junie
- **Fellowship**: core of chat mode — GGUF controller classifies entry → agent free-flow via NEXT:/TASK: protocol
- **Agentic loop**: provider → tool_calls → execute → feed_back → repeat (up to N iterations)
- **Flow system**: `.mithril-flow.yaml` configures standalone agentic loops
