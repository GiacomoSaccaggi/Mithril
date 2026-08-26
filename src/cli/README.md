# CLI — The Fellowship's Voice

Terminal interface: REPL with Tab completion, exec mode for CI, session management.

## Key Concept

chat_core.rs centralizes ALL command logic. REPL and TUI are thin rendering layers that call ChatCore.

## Commands

`/exit` `/clear` `/compact` `/fellowship` `/undo` `/redo` `/plan` `/build` `/session` `/history` `/share` `/telegram` `/help`

## Special Syntax

- `@file.rs` — injects file content into message
- `#agent` — routes directly to named agent
- `\` at end of line — multiline input

## Files

- `agent_loop.rs` — Re-exports from flow::agent_loop for backward compatibility.
- `chat.rs` — Interactive chat REPL — terminal frontend using fellowship orchestration.
- `chat_core.rs` — Centralized chat logic shared between REPL and TUI frontends.
- `compact.rs` — Conversation compaction — summarize long histories to free context window.
- `config.rs` — Config management CLI subcommand.
- `download.rs` — 
- `exec.rs` — Headless exec mode — run the agentic loop non-interactively.
- `fellowship.rs` — Fellowship CLI — manage multi-agent orchestration.
- `fellowships.rs` — `mithril fellowships` — list all available fellowship configurations.
- `flow.rs` — `mithril flow` — run a multi-agent flow on a user message.
- `forge.rs` — 
- `init.rs` — `mithril init` — auto-analyze project and generate MITHRIL.md.
- `mcp_stdio.rs` — MCP server over stdin/stdout.
- `mod.rs` — 
- `scan.rs` — 
- `serve.rs` — 
- `sessions.rs` — `mithril sessions` — list, show, and delete saved chat sessions.
- `steering.rs` — Steering — load project context from .mithril/steering/ and MITHRIL.md.
- `telegram.rs` — Telegram bot frontend for Mithril.
- `undo.rs` — 
