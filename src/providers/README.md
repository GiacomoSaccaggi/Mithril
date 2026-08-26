# Providers — The Five Wizards

Abstraction layer over LLM APIs. Each provider translates the unified `ChatProvider` interface into provider-specific HTTP calls.

## Key Concept

The `ChatProvider` trait defines 5 methods that ALL providers implement. The orchestrator never knows which provider it's talking to — swap Gemini for GPT-4 by changing one YAML line.

## The 3 Core Functions

### chat(messages) → String
Simple request/response. Sends conversation history, gets full response back.

### chat_stream(messages, tx) → String  
Same as chat() but tokens arrive one-by-one via a channel. Used for real-time output.

### chat_with_tools(messages, tools) → ChatResponse
Function calling. The model can either respond with text OR request tool executions.

## Provider Differences

| Provider | Auth | Message Format | Endpoint |
|----------|------|---------------|----------|
| Gemini | ?key= in URL | contents[{role, parts}] | generativelanguage.googleapis.com |
| OpenAI | Bearer header | messages[{role, content}] | api.openai.com/v1 |
| Anthropic | x-api-key header | messages[] + system separate | api.anthropic.com/v1 |
| Groq | Bearer header | messages[] (OpenAI-compatible) | api.groq.com/openai/v1 |
| Local | N/A | Direct GGUF inference | In-process (no HTTP) |

## Adding a New Provider

1. Create `src/providers/your_provider.rs`
2. Implement `ChatProvider` trait (see template in README.html)
3. Add match arm in `create_provider()` in `mod.rs`
4. Done — orchestrator, API, CLI all work automatically

## Three Provider Types

### 1. Local GGUF (free, private)
Direct inference via llama.cpp. Used for the GGUF classifier/router. No network, no cost.

### 2. Cloud API (Gemini, OpenAI, Anthropic, Groq)
Standard HTTP calls to cloud endpoints. Each has its own request/response format but implements the same ChatProvider trait.

### 3. CLI Tools (Kiro, Junie, etc.)
Subprocess providers that invoke local CLI tools. Useful when you already have access to a CLI with its own authentication. The provider spawns the CLI process, sends the prompt, and parses the structured output.

```yaml
# Example: using Kiro CLI as a provider
- name: reviewer
  provider: kiro
  model: claude-opus-4.6
```

To add a new CLI provider, create a file in `src/providers/` that:
1. Spawns the CLI as a subprocess
2. Passes the prompt as an argument
3. Parses stdout for the response
4. Implements the ChatProvider trait

## Files

- `mod.rs` — ChatProvider trait, factory, retry_with_backoff
- `gemini.rs` — Google Gemini API adapter
- `openai.rs` — OpenAI / Azure adapter (configurable base_url)
- `anthropic.rs` — Anthropic Claude adapter
- `groq.rs` — Groq cloud adapter (OpenAI-compatible)
- `local.rs` — Local GGUF inference via llama.cpp
