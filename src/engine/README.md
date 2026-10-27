# Engine — Khazad-dûm

Local GGUF model inference via llama.cpp FFI. Lazy-loads models, streams tokens, uses Metal GPU.

## Key Concept

The LazyModelManager loads the model only on first use, keeps it in memory, and provides a streaming bridge between C++ and async Rust.

## How It Works

- **Lazy loading:** Model loaded on first inference (~2-5 seconds)
- **Stay resident:** Stays in memory for fast subsequent calls
- **Metal GPU:** Uses Apple Silicon GPU automatically
- **Streaming:** Tokens flow from llama.cpp via mpsc channel to async Rust

## Model Catalog

| Model | Size | Use |
|-------|------|-----|
| jevos-v4 | ~620 MB | Zero-shot agent router & System 1 decision engine (~20–50ms) |
| qwen-1.5b | ~1.2 GB | Request classifier (free routing) |
| qwen-7b | ~4.5 GB | General coding |
| qwen-14b | ~9.0 GB | Strong offline coding |
| llama-8b | ~5.0 GB | Alternative general |
| deepseek-6.7b | ~4.5 GB | Code completion & review |
| phi-3.5 | ~2.5 GB | Lightweight instruction model |

## Files

- `chat_template.rs` — Chat prompt formatters and stop tokens (ChatML, Llama3, Phi3, Jev).
- `lazy_model.rs` — Lazy model manager — loads GGUF on first use, auto-unloads after idle timeout.
- `mod.rs` — Engine module exports and lazy manager instance.
- `model_catalog.rs` — Pre-registered GGUF models catalog (including `jevos-v4`) and name normalizer.
