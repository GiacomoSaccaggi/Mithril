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
| qwen2:1.5b | ~1.2 GB | Request classifier (free routing) |
| qwen2:7b | ~4.5 GB | General coding |
| llama:8b | ~5 GB | Alternative general |

## Files

- `chat_template.rs` — 
- `lazy_model.rs` — Lazy model manager — loads GGUF on first use, auto-unloads after idle timeout.
- `mod.rs` — 
- `model_catalog.rs` — 
