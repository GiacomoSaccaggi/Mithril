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

### MCP (Model Context Protocol)
- `POST /mcp` — JSON-RPC 2.0 for tool discovery and execution

## How Clients Connect

Point any Ollama client to `http://localhost:16180`. Select fellowship name as the "model".

## Files

- `mcp.rs` — 
- `mod.rs` — 
- `ollama.rs` — 
- `openai.rs` — 
- `server.rs` — 
