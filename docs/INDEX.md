# Index Module — Palantír

The Palantír index provides BM25-based semantic search over project source files.

**Location**: `src/index/`

## Files

| File | Purpose |
|------|---------|
| `palantir.rs` | BM25 index implementation |
| `mod.rs` | Module exports |

---

## palantir.rs — PalantirIndex

### Overview

Palantír is a BM25 (Best Matching 25) information retrieval index. It:
1. Scans source files in the project
2. Tokenizes content and extracts symbols
3. Computes term frequency (TF) and inverse document frequency (IDF)
4. Enables fast semantic search across the codebase

### Constants

```rust
const CURRENT_VERSION: u32 = 1;
const INDEX_DIR: &str = ".celebrimbot";
const INDEX_FILE: &str = "palantir_index.json";
const STALE_THRESHOLD: f64 = 0.20;  // 20% changed = stale
const BM25_K1: f64 = 1.5;           // Term saturation parameter
const BM25_B: f64 = 0.75;           // Length normalization parameter
```

---

## Data Structures

### Struct: `PalantirEntry`

Represents one indexed file.

```rust
pub struct PalantirEntry {
    pub path: String,              // Relative file path
    pub symbols: Vec<String>,      // Extracted function/class names
    pub terms: HashMap<String, u32>, // Term → frequency map
    pub line_count: usize,
    pub last_modified: u64,        // Epoch milliseconds
}
```

### Struct: `ScoredEntry`

Search result with relevance score.

```rust
pub struct ScoredEntry {
    pub entry: PalantirEntry,
    pub score: f64,  // BM25 score
}
```

### Struct: `PalantirIndex`

The complete index.

```rust
pub struct PalantirIndex {
    pub version: u32,
    pub base_path: String,
    pub indexed_at: u64,           // Epoch milliseconds
    pub entries: Vec<PalantirEntry>,
    pub idf: HashMap<String, f64>, // Term → IDF score
}
```

---

## Building the Index

### Method: `build`

```rust
pub fn build(base_path: &str, scan_op: &ScanOperator) -> PalantirIndex
```

Builds a complete index from scratch.

**Process:**
1. Get list of source files from `scan_op.walk_source_files()`
2. For each file:
   - Read content
   - Extract symbols (function/class names)
   - Build term frequency map
   - Record metadata (path, line count, modification time)
3. Compute IDF for all terms
4. Return complete index

---

### Method: `build_incremental`

```rust
pub fn build_incremental(
    base_path: &str,
    scan_op: &ScanOperator,
    existing: Option<PalantirIndex>,
) -> PalantirIndex
```

Builds index incrementally, reusing unchanged entries.

**Optimization:**
- For each file, compares `last_modified` timestamp
- If unchanged: reuses cached entry
- If changed: re-indexes the file
- Handles deleted files automatically

---

## Querying the Index

### Method: `query`

```rust
pub fn query(&self, prompt: &str, top_k: usize) -> Vec<ScoredEntry>
```

Searches the index using BM25 ranking.

**BM25 Formula:**
```
score(D, Q) = Σ IDF(qi) * (tf(qi, D) * (k1 + 1)) / (tf(qi, D) + k1 * (1 - b + b * |D|/avgdl))
```

Where:
- `D` = document (file)
- `Q` = query terms
- `qi` = individual query term
- `tf(qi, D)` = term frequency of qi in D
- `IDF(qi)` = inverse document frequency
- `|D|` = document length
- `avgdl` = average document length
- `k1` = 1.5 (saturation parameter)
- `b` = 0.75 (length normalization)

**Process:**
1. Tokenize query into terms
2. For each document, compute BM25 score
3. Filter documents with score > 0
4. Sort by score descending
5. Return top-k results

---

## Persistence

### Method: `save`

```rust
pub fn save(&self, base_path: &str)
```

Saves index to `.celebrimbot/palantir_index.json`.

---

### Method: `load_or_null`

```rust
pub fn load_or_null(base_path: &str) -> Option<PalantirIndex>
```

Loads index from disk, returns `None` if not found or invalid.

---

## Staleness Detection

### Method: `is_stale`

```rust
pub fn is_stale(&self, base_path: &str) -> bool
```

Checks if >20% of indexed files have changed.

**Criteria for "changed":**
- File no longer exists
- File's modification time differs from recorded time

---

## Tokenization

### Function: `tokenize`

```rust
pub fn tokenize(text: &str) -> Vec<String>
```

Splits text into searchable tokens.

**Process:**
1. Split on non-alphanumeric characters
2. Lowercase all tokens
3. Filter: length ≥ 3 characters
4. Remove stopwords

**Example:**
```
"fn hello_world() { let x = 42; }"
→ ["hello", "world"]  // "fn", "let" are stopwords
```

---

## Symbol Extraction

### Function: `extract_symbols`

```rust
fn extract_symbols(content: &str, path: &str) -> Vec<String>
```

Extracts function and class names from source code.

**Language-specific patterns:**

| Language | Extensions | Pattern |
|----------|------------|---------|
| Kotlin/Java/Scala | kt, kts, java, scala, cs | `class\|interface\|object\|fun\|val\|var\|enum` |
| Python | py | `class\|def` |
| JavaScript/TypeScript | js, ts, jsx, tsx | `function\|class\|const\|let\|var` |
| Go | go | `func\|type\|var\|const` |
| Rust | rs | `fn\|struct\|enum\|trait\|impl\|mod\|const\|let` |
| Other | * | `class\|function\|def\|func\|fn` |

**Limits:** Maximum 50 symbols per file.

---

## Stopwords

The index filters out common programming keywords:

```rust
// Language keywords
"val", "var", "fun", "class", "object", "interface", "enum", "data",
"return", "import", "package", "public", "private", "protected",
"override", "open", "final", "static", "abstract", "sealed",
"this", "super", "null", "true", "false", "new", "void",

// Types
"int", "long", "float", "double", "boolean", "string", "char",

// Control flow
"if", "else", "when", "for", "while", "do", "try", "catch",
"throw", "throws", "finally", "break", "continue",

// Python
"def", "self", "none", "pass", "with", "from", "not", "and", "or",
"lambda", "yield", "async", "await",

// JavaScript
"const", "let", "function", "typeof", "instanceof", "undefined",
"prototype", "require", "module", "exports",

// Common English
"the", "and", "for", "are", "but", "not", "you", "all", "can",

// Common code terms
"get", "set", "add", "put", "has", "use", "new", "any", "map",
"list", "type", "name", "size", "init", "run", "log", "err"
```

---

## IDF Calculation

### Function: `compute_idf`

```rust
fn compute_idf(entries: &[PalantirEntry]) -> HashMap<String, f64>
```

Computes inverse document frequency for all terms.

**Formula:**
```
IDF(term) = ln((N - df + 0.5) / (df + 0.5) + 1)
```

Where:
- `N` = total number of documents
- `df` = number of documents containing the term

Higher IDF = rarer term = more distinctive.
