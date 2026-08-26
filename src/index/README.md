# Index — The Palantír

BM25 full-text search over the project codebase for context injection.

## Key Concept

Scans the project, builds an inverted index, agents query it to find relevant code without reading every file.

## How It Works

1. `mithril scan` walks the project tree
2. Each file tokenized into terms
3. BM25 inverted index stored in `.mithril/palantir.idx`
4. Agents query with natural language → ranked file paths

## Files

- `mod.rs` — 
- `palantir.rs` — 
