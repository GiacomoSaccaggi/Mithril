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

## Files

- `mod.rs` — ChatProvider trait, factory, retry_with_backoff
- `gemini.rs` — Google Gemini API adapter
- `openai.rs` — OpenAI / Azure adapter (configurable base_url)
- `anthropic.rs` — Anthropic Claude adapter
- `groq.rs` — Groq cloud adapter (OpenAI-compatible)
- `local.rs` — Local GGUF inference via llama.cpp
