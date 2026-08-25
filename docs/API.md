# The Beacons — API Reference

> *"The beacons of Minas Tirith! The beacons are lit! Gondor calls for aid!"* — Pippin

Mithril exposes three API dialects: Ollama-compatible, OpenAI-compatible, and MCP JSON-RPC. This document details each endpoint.

---

## API Overview

| Dialect | Base Path | Purpose |
|---------|-----------|---------|
| Ollama | `/api/*` | Ollama client compatibility |
| OpenAI | `/v1/*` | OpenAI SDK compatibility |
| MCP | `/mcp` | Model Context Protocol tools |

Default port: `16180` (the golden ratio × 10,000)

---

## Health Check

```
GET /health
```

**Response:**
```json
{
  "status": "healthy",
  "version": "0.1.0",
  "model_loaded": true,
  "uptime_seconds": 3600
}
```

---

## Ollama API

> *"Short cuts make long delays."*

Full compatibility with Ollama clients.

### List Models

```
GET /api/tags
```

**Response:**
```json
{
  "models": [
    {
      "name": "qwen2.5-7b-instruct",
      "modified_at": "2024-01-15T10:30:00Z",
      "size": 4500000000,
      "digest": "sha256:abc123...",
      "details": {
        "format": "gguf",
        "family": "qwen2",
        "parameter_size": "7B",
        "quantization_level": "Q4_K_M"
      }
    }
  ]
}
```

### Chat Completion

```
POST /api/chat
```

**Request:**
```json
{
  "model": "qwen2.5-7b-instruct",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello!"}
  ],
  "stream": true,
  "options": {
    "temperature": 0.7,
    "top_p": 0.9,
    "num_predict": 1024
  }
}
```

**Streaming Response:**
```json
{"message": {"role": "assistant", "content": "Hello"}, "done": false}
{"message": {"role": "assistant", "content": "!"}, "done": false}
{"message": {"role": "assistant", "content": " How"}, "done": false}
{"done": true, "total_duration": 1234567890, "eval_count": 42}
```

**Non-Streaming Response:**
```json
{
  "message": {
    "role": "assistant",
    "content": "Hello! How can I help you today?"
  },
  "done": true,
  "total_duration": 1234567890,
  "load_duration": 100000000,
  "prompt_eval_count": 15,
  "prompt_eval_duration": 500000000,
  "eval_count": 42,
  "eval_duration": 600000000
}
```

### Generate (Legacy)

```
POST /api/generate
```

**Request:**
```json
{
  "model": "qwen2.5-7b-instruct",
  "prompt": "Why is the sky blue?",
  "stream": false,
  "options": {
    "temperature": 0.7,
    "num_predict": 256
  }
}
```

**Response:**
```json
{
  "response": "The sky appears blue due to Rayleigh scattering...",
  "done": true,
  "context": [1, 2, 3, ...],
  "total_duration": 1234567890,
  "eval_count": 128
}
```

### Model Operations

```
POST /api/pull
```

**Request:**
```json
{
  "name": "qwen2.5:7b"
}
```

**Response:** Streaming progress updates.

---

## OpenAI API

> *"Even the very wise cannot see all ends."*

OpenAI SDK compatibility for broad client support.

### List Models

```
GET /v1/models
```

**Response:**
```json
{
  "object": "list",
  "data": [
    {
      "id": "qwen2.5-7b-instruct",
      "object": "model",
      "created": 1705315800,
      "owned_by": "local"
    },
    {
      "id": "gemini-2.5-flash",
      "object": "model",
      "created": 1705315800,
      "owned_by": "google"
    }
  ]
}
```

### Chat Completion

```
POST /v1/chat/completions
```

**Request:**
```json
{
  "model": "qwen2.5-7b-instruct",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello!"}
  ],
  "stream": true,
  "temperature": 0.7,
  "max_tokens": 1024,
  "top_p": 0.9
}
```

**Streaming Response (SSE):**
```
data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1705315800,"model":"qwen2.5-7b-instruct","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1705315800,"model":"qwen2.5-7b-instruct","choices":[{"index":0,"delta":{"content":"!"},"finish_reason":null}]}

data: {"id":"chatcmpl-123","object":"chat.completion.chunk","created":1705315800,"model":"qwen2.5-7b-instruct","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

**Non-Streaming Response:**
```json
{
  "id": "chatcmpl-123",
  "object": "chat.completion",
  "created": 1705315800,
  "model": "qwen2.5-7b-instruct",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "Hello! How can I help you today?"
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 15,
    "completion_tokens": 42,
    "total_tokens": 57
  }
}
```

### Tool Calls

```json
{
  "model": "qwen2.5-7b-instruct",
  "messages": [...],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "read_file",
        "description": "Read contents of a file",
        "parameters": {
          "type": "object",
          "properties": {
            "path": {"type": "string"}
          },
          "required": ["path"]
        }
      }
    }
  ],
  "tool_choice": "auto"
}
```

---

## MCP API

> *"I am a servant of the Secret Fire, wielder of the flame of Anor."*

Model Context Protocol for tool-using agents.

### Endpoint

```
POST /mcp
```

All requests use JSON-RPC 2.0 format.

### List Tools

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/list"
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [
      {
        "name": "read_file",
        "description": "Read the contents of a file",
        "inputSchema": {
          "type": "object",
          "properties": {
            "path": {
              "type": "string",
              "description": "Path to the file"
            }
          },
          "required": ["path"]
        }
      }
    ]
  }
}
```

### Call Tool

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "read_file",
    "arguments": {
      "path": "src/main.rs"
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "fn main() {\n    println!(\"Hello, world!\");\n}"
      }
    ]
  }
}
```

---

## The 24 Tools

### File Operations

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents |
| `write_file` | Write content to file |
| `edit_file` | Apply search/replace edits |
| `delete_file` | Remove a file |
| `apply_patch` | Apply unified diff |

### Discovery

| Tool | Description |
|------|-------------|
| `list_files` | List directory contents |
| `grep_files` | Search for patterns |
| `find_file` | Find files by name |
| `file_stats` | Get file metadata |

### Git

| Tool | Description |
|------|-------------|
| `git_status` | Repository status |
| `git_log` | Commit history |
| `git_diff` | Show changes |
| `git_blame` | Line-by-line authorship |
| `git_branch` | List/manage branches |
| `git_commit` | Create commit |

### Terminal

| Tool | Description |
|------|-------------|
| `run_terminal` | Execute shell command |

### Web

| Tool | Description |
|------|-------------|
| `web_search` | Search the web |
| `fetch_page` | Retrieve web page |

### Code

| Tool | Description |
|------|-------------|
| `search_symbols` | Find code symbols |
| `document_outline` | Extract file structure |

### Lore

| Tool | Description |
|------|-------------|
| `lore_write` | Store project knowledge |
| `lore_read` | Retrieve project knowledge |

### Session

| Tool | Description |
|------|-------------|
| `share_session` | Share to another interface |

---

## Tool Schema Reference

### read_file

```json
{
  "name": "read_file",
  "inputSchema": {
    "type": "object",
    "properties": {
      "path": {"type": "string", "description": "File path"},
      "encoding": {"type": "string", "default": "utf-8"},
      "line_start": {"type": "integer"},
      "line_end": {"type": "integer"}
    },
    "required": ["path"]
  }
}
```

### write_file

```json
{
  "name": "write_file",
  "inputSchema": {
    "type": "object",
    "properties": {
      "path": {"type": "string"},
      "content": {"type": "string"}
    },
    "required": ["path", "content"]
  }
}
```

### run_terminal

```json
{
  "name": "run_terminal",
  "inputSchema": {
    "type": "object",
    "properties": {
      "command": {"type": "string"},
      "working_dir": {"type": "string"},
      "timeout": {"type": "integer", "default": 30}
    },
    "required": ["command"]
  }
}
```

### git_diff

```json
{
  "name": "git_diff",
  "inputSchema": {
    "type": "object",
    "properties": {
      "path": {"type": "string"},
      "staged": {"type": "boolean"},
      "commit": {"type": "string"},
      "file": {"type": "string"}
    }
  }
}
```

---

## Error Responses

### Ollama Format

```json
{
  "error": "model not found"
}
```

### OpenAI Format

```json
{
  "error": {
    "message": "Invalid API key provided",
    "type": "invalid_request_error",
    "code": "invalid_api_key"
  }
}
```

### MCP Format

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32602,
    "message": "Invalid params",
    "data": {"path": "required field missing"}
  }
}
```

### Error Codes

| Code | Meaning |
|------|---------|
| 400 | Bad request |
| 401 | Unauthorized |
| 404 | Not found |
| 429 | Rate limited |
| 500 | Internal error |

---

## CORS Configuration

Enable CORS for web clients:

```bash
mithril serve --cors
```

Or in config:

```yaml
server:
  cors:
    enabled: true
    origins:
      - "http://localhost:3000"
      - "https://your-app.com"
```

---

## Rate Limiting

Default limits:

| Endpoint | Limit |
|----------|-------|
| `/api/chat` | 60/min |
| `/v1/chat/completions` | 60/min |
| `/mcp` | 120/min |

Configure:

```yaml
server:
  rate_limit:
    requests_per_minute: 120
    burst: 10
```

---

## Authentication

Optional API key authentication:

```yaml
server:
  auth:
    enabled: true
    api_keys:
      - name: "my-app"
        key: "mithril_sk_..."
```

Usage:
```bash
curl -H "Authorization: Bearer mithril_sk_..." http://localhost:16180/api/chat
```

---

> *"All we have to decide is what to do with the time that is given us."*
