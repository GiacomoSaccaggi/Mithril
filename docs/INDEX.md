# The Palantír — BM25 Semantic Index

> *"The Palantíri came from beyond Westernesse, from Eldamar. The Master-stone was under the Dome of Stars at Osgiliath."* — Gandalf

The Palantír provides far-seeing into your codebase through BM25 semantic search, enabling fast retrieval of relevant context during conversations.

---

## Overview

| Feature | Description |
|---------|-------------|
| **Algorithm** | BM25 (Best Match 25) |
| **Index Type** | Inverted index with term frequencies |
| **Storage** | Persistent cache in `.mithril/index/` |
| **Updates** | Incremental on file changes |
| **Query Time** | Sub-millisecond for typical projects |

---

## How It Works

### Indexing Pipeline

```
Project Files → Scanner → Tokenizer → BM25 Index → Cache
```

1. **Scanner** walks the project tree, respecting `.gitignore`
2. **Tokenizer** extracts terms from each file
3. **BM25 Index** computes term frequencies and document lengths
4. **Cache** persists to disk for fast startup

### Query Pipeline

```
Query → Tokenize → BM25 Score → Rank → Top K Results
```

1. **Tokenize** the query into search terms
2. **BM25 Score** each document against query terms
3. **Rank** documents by relevance score
4. **Return** top K most relevant chunks

---

## Building the Index

### Initial Scan

```bash
mithril scan
```

Output:
```
Scanning project...
  Found 1,234 files
  Indexed 856 files (378 ignored)
  Terms: 45,678
  Index size: 2.3 MB
  Time: 1.2s

Index saved to .mithril/index/
```

### Rebuild

Force full rebuild:

```bash
mithril scan --rebuild
```

### Selective Scan

Scan specific directory:

```bash
mithril scan --path src/
```

---

## Index Configuration

```yaml
# ~/.mithril/config.yaml
index:
  # Files to include (glob patterns)
  include:
    - "**/*.rs"
    - "**/*.py"
    - "**/*.ts"
    - "**/*.md"
    - "**/*.yaml"
    - "**/*.json"
  
  # Files to exclude
  exclude:
    - "**/node_modules/**"
    - "**/target/**"
    - "**/.git/**"
    - "**/dist/**"
    - "**/*.min.js"
  
  # Maximum file size to index (bytes)
  max_file_size: 1048576  # 1 MB
  
  # Chunk size for large files
  chunk_size: 1000  # lines
  
  # BM25 parameters
  bm25:
    k1: 1.2   # Term frequency saturation
    b: 0.75  # Document length normalization
```

---

## BM25 Algorithm

### Formula

The BM25 score for document `d` and query `q`:

```
score(d, q) = Σ IDF(qi) · (f(qi, d) · (k1 + 1)) / (f(qi, d) + k1 · (1 - b + b · |d|/avgdl))
```

Where:
- `IDF(qi)` = Inverse document frequency of term
- `f(qi, d)` = Term frequency in document
- `|d|` = Document length
- `avgdl` = Average document length
- `k1`, `b` = Tuning parameters

### Parameter Tuning

| Parameter | Default | Effect |
|-----------|---------|--------|
| `k1` | 1.2 | Higher = more weight to term frequency |
| `b` | 0.75 | Higher = more length normalization |

For code search, defaults work well. For documentation with varied lengths, consider `b: 0.5`.

---

## Index Structure

### Disk Layout

```
.mithril/index/
├── meta.json           # Index metadata
├── documents.bin       # Document store
├── terms.bin           # Term dictionary
├── postings.bin        # Inverted index
└── cache/
    └── query_cache.bin # Recent query results
```

### Meta Format

```json
{
  "version": 1,
  "created_at": "2024-01-15T10:30:00Z",
  "document_count": 856,
  "term_count": 45678,
  "total_tokens": 1234567,
  "avg_document_length": 1442.3,
  "bm25_k1": 1.2,
  "bm25_b": 0.75
}
```

---

## Automatic Context Injection

During chat, the Palantír automatically provides relevant context.

### How It Works

1. User sends message
2. Mithril extracts key terms
3. Palantír returns top relevant chunks
4. Chunks injected into system prompt
5. Model has project context

### Example

User: "How does the authentication work?"

Palantír returns:
- `src/auth/middleware.rs` (lines 1-50)
- `src/auth/jwt.rs` (lines 1-30)
- `docs/AUTH.md` (lines 1-100)

These are included in context before the model responds.

### Context Budget

```yaml
index:
  # Maximum tokens for injected context
  context_budget: 4000
  
  # Maximum chunks to inject
  max_chunks: 10
  
  # Minimum relevance score (0-1)
  min_score: 0.1
```

---

## Tokenization

### Text Processing

1. **Lowercase** — Convert to lowercase
2. **Split** — On whitespace and punctuation
3. **Filter** — Remove stopwords
4. **Stem** — Optional Porter stemming

### Code-Aware Tokenization

For source code:
- Split camelCase: `getUserName` → `get`, `user`, `name`
- Split snake_case: `get_user_name` → `get`, `user`, `name`
- Preserve identifiers: `HttpClient` indexed as-is AND split

### Stopwords

Default stopwords (configurable):

```yaml
index:
  stopwords:
    - the
    - a
    - an
    - is
    - are
    - was
    - were
    - be
    - been
    - being
    # ... etc
```

---

## Incremental Updates

The index updates incrementally on file changes:

### Change Detection

| Change | Action |
|--------|--------|
| New file | Add to index |
| Modified file | Re-index file |
| Deleted file | Remove from index |
| Renamed file | Remove old, add new |

### Update Triggers

- On `mithril scan` command
- On chat session start (if stale)
- On explicit `/reindex` command

### Staleness Check

```rust
fn is_stale(&self) -> bool {
    let last_scan = self.meta.created_at;
    let files_changed = self.workspace
        .walk()
        .any(|f| f.modified() > last_scan);
    
    files_changed
}
```

---

## Query Syntax

### Simple Query

```
authentication middleware
```

Returns documents containing both terms.

### Phrase Query

```
"jwt token"
```

Returns documents with exact phrase.

### Prefix Query

```
auth*
```

Returns documents with terms starting with "auth".

### Exclusion

```
authentication -test
```

Returns documents with "authentication" but not "test".

### Field-Specific

```
path:auth content:middleware
```

Searches specific fields.

---

## API Integration

### MCP Tool

The Palantír is available as an implicit part of context, but can also be queried directly:

```json
{
  "method": "tools/call",
  "params": {
    "name": "search_codebase",
    "arguments": {
      "query": "authentication middleware",
      "max_results": 5
    }
  }
}
```

### Response

```json
{
  "results": [
    {
      "path": "src/auth/middleware.rs",
      "score": 0.85,
      "snippet": "pub fn auth_middleware(req: Request) -> Result<Response> {...",
      "line_start": 15,
      "line_end": 45
    }
  ]
}
```

---

## Performance

### Benchmarks (1000 file project)

| Operation | Time |
|-----------|------|
| Full index build | 1.2s |
| Incremental update (10 files) | 50ms |
| Query (average) | 0.3ms |
| Query (complex) | 2ms |

### Memory Usage

| Project Size | Index Memory |
|--------------|--------------|
| 100 files | ~5 MB |
| 1,000 files | ~20 MB |
| 10,000 files | ~150 MB |

---

## Troubleshooting

### Index Not Finding Results

1. Check file is included:
   ```bash
   mithril scan --verbose | grep "your-file"
   ```

2. Verify patterns in config
3. Rebuild index: `mithril scan --rebuild`

### Index Too Large

1. Add exclusions for generated files
2. Reduce `chunk_size`
3. Increase `min_score` to filter weak matches

### Slow Queries

1. Check index isn't corrupted: `mithril scan --verify`
2. Reduce `max_chunks`
3. Rebuild: `mithril scan --rebuild`

---

> *"In place of a Dark Lord, you would have a queen! Not dark but beautiful and terrible as the dawn!"*
