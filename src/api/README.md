# API — The Beacon Towers

HTTP server exposing Mithril as Ollama-compatible and OpenAI-compatible endpoint, plus MCP.

## Key Concept

External tools (Junie, OpenCode) connect here. They see fellowships as "models" and talk standard protocols.

## Supported Protocols

### Ollama API
- `GET /api/tags` — list models (fellowships appear as models)
- `POST /api/chat` — chat completion with streaming
- `POST /api/generate` — text generation

### OpenAI API
- `GET /v1/models` — list models
- `POST /v1/chat/completions` — chat completion

### JEV System 1 API
- `POST /v1/systemone` — zero-shot decision & classification routing

### MCP (Model Context Protocol)
- `POST /mcp` — JSON-RPC 2.0 for tool discovery and execution

## How Clients Connect

Point any Ollama client to `http://localhost:16180`. Select fellowship name as the "model".
To perform fast zero-shot decisions, post to `http://localhost:16180/v1/systemone`.

## Files

- `mcp.rs` — Model Context Protocol JSON-RPC 2.0 tool execution.
- `mod.rs` — API routes and state definition.
- `ollama.rs` — Ollama-compatible API handlers (/api/tags, /api/chat, /api/generate).
- `openai.rs` — OpenAI-compatible API handlers (/v1/models, /v1/chat/completions).
- `server.rs` — Axum HTTP server setup, middleware, and route mounting.
- `systemone.rs` — JEV System 1 zero-shot classification & decision routing handler (/v1/systemone).
