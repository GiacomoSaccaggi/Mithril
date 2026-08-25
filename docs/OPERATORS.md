# Operators Module

Operators are low-level implementations for file system, terminal, git, and web operations. They are used by tools and other parts of the system.

**Location**: `src/operators/`

## Files

| File | Purpose |
|------|---------|
| `file.rs` | File read/write/delete operations |
| `terminal.rs` | Shell command execution |
| `git.rs` | Git repository operations |
| `web.rs` | HTTP requests and web search |
| `scan.rs` | Project file scanning and search |
| `shadow.rs` | Backup/undo system for file changes |
| `mod.rs` | Module exports |

---

## file.rs — FileOperator

Basic file system operations with path resolution.

### Struct: `FileOperator`

```rust
#[derive(Clone)]
pub struct FileOperator {
    base_path: PathBuf,
}
```

### Constructor

```rust
pub fn new(base_path: impl Into<PathBuf>) -> Self
```

Creates operator rooted at the given directory.

---

### Method: `read_file`

```rust
pub fn read_file(&self, path: &str) -> String
```

Reads file content as UTF-8 string.

**Parameters:**
- `path`: Relative or absolute path

**Returns:** File content or `"Error: File not found: {path}"`

**Path Resolution:**
- Absolute paths used as-is
- Relative paths joined with `base_path`

---

### Method: `write_file`

```rust
pub fn write_file(&self, path: &str, content: &str) -> bool
```

Writes content to file.

**Behavior:**
- Creates parent directories if they don't exist
- Overwrites existing files
- Returns `true` on success, `false` on failure

---

### Method: `delete_file`

```rust
pub fn delete_file(&self, path: &str) -> bool
```

Deletes a file. Returns `true` on success.

---

### Method: `exists`

```rust
pub fn exists(&self, path: &str) -> bool
```

Checks if a file exists.

---

## terminal.rs — TerminalOperator

Executes shell commands with timeout.

### Struct: `TerminalResult`

```rust
pub struct TerminalResult {
    pub output: String,   // Combined stdout + stderr
    pub exit_code: i32,   // Process exit code (-1 on error)
}
```

### Struct: `TerminalOperator`

```rust
#[derive(Clone)]
pub struct TerminalOperator {
    working_dir: PathBuf,
    timeout_secs: u64,
}
```

### Constructor

```rust
pub fn new(working_dir: impl Into<PathBuf>, timeout_secs: u64) -> Self
```

---

### Method: `execute`

```rust
pub async fn execute(&self, command: &str) -> TerminalResult
```

Executes a shell command.

**Behavior:**
1. Spawns shell process:
   - Unix: `sh -c "{command}"`
   - Windows: `cmd /C "{command}"`
2. Sets working directory
3. Waits for completion with timeout
4. Returns combined stdout + stderr

**Timeout Handling:**
- Checks completion every 50ms
- Returns `"[TIMEOUT WARNING: Process killed after 30 seconds]"` on timeout

**Error Conditions:**
- Spawn failure: `"Error: failed to spawn process: {e}"`
- Timeout: exit_code = -1
- Task panic: `"Error: task panicked"`

---

## git.rs — GitOperator

Git repository operations via CLI.

### Struct: `GitOperator`

```rust
#[derive(Clone)]
pub struct GitOperator {
    base_path: PathBuf,
}
```

### Private Method: `run`

```rust
fn run(&self, args: &[&str]) -> String
```

Executes `git {args}` in the base path, returns combined output.

---

### Method: `status`

```rust
pub fn status(&self) -> String
```

Returns `git status --short` output.

---

### Method: `log`

```rust
pub fn log(&self, max_entries: usize) -> String
```

Returns `git log --oneline --decorate -{n}` output.

---

### Method: `diff`

```rust
pub fn diff(&self, target: Option<&str>) -> String
```

Returns `git diff HEAD` (full) or `git diff HEAD -- {target}` (file).

**Truncation:** Output limited to 6000 characters.

---

### Method: `blame`

```rust
pub fn blame(&self, target: &str) -> String
```

Returns `git blame --line-porcelain {target}` output.

**Truncation:** Output limited to 4000 characters.

---

### Method: `branch`

```rust
pub fn branch(&self) -> String
```

Returns `git branch --show-current` output.

---

## web.rs — WebOperator

HTTP client for web search and page fetching.

### Struct: `WebOperator`

```rust
#[derive(Clone)]
pub struct WebOperator {
    client: reqwest::Client,
}
```

### Constructor

```rust
pub fn new() -> Self
```

Creates client with:
- User-Agent: `"Mithril/0.1"`
- Timeout: 15 seconds

---

### Method: `search`

```rust
pub async fn search(&self, query: &str) -> String
```

Searches DuckDuckGo Instant Answer API.

**API URL:** `https://api.duckduckgo.com/?q={query}&format=json&no_html=1&skip_disambig=1`

**Response Parsing:**
- Extracts `AbstractText` and `AbstractURL`
- Extracts up to 5 `RelatedTopics` with text and URL

**Returns:** Formatted search results or error message

---

### Method: `fetch_page`

```rust
pub async fn fetch_page(&self, url: &str) -> String
```

Fetches URL and strips HTML.

**HTML Stripping:**
1. Removes `<style>` blocks
2. Removes `<script>` blocks
3. Removes all HTML tags
4. Collapses whitespace

**Truncation:** Output limited to 4000 characters.

---

## scan.rs — ScanOperator

Project file scanning, search, and analysis.

### Constants

```rust
const MAX_FILE_BYTES: u64 = 102_400;  // 100 KB max for indexing

const IGNORED_DIRS: &[&str] = &[
    "/.git/", "/build/", "/node_modules/", "/.gradle/",
    "/.idea/", "/dist/", "/out/", "/.celebrimbot/", "/target/"
];

const SOURCE_EXTENSIONS: &[&str] = &[
    "kt", "java", "py", "js", "ts", "tsx", "jsx", "rs", "go",
    "c", "cpp", "cc", "h", "hpp", "cs", "rb", "scala", "swift", "php", "kts"
];
```

### Struct: `ScanOperator`

```rust
#[derive(Clone)]
pub struct ScanOperator {
    base_path: PathBuf,
}
```

---

### Method: `list_files`

```rust
pub fn list_files(&self, sub_path: Option<&str>, extension: Option<&str>) -> String
```

Lists files in the project.

**Parameters:**
- `sub_path`: Optional subdirectory to start from
- `extension`: Optional extension filter (e.g., "rs")

**Returns:** Newline-separated relative paths (sorted)

**Filtering:**
- Excludes ignored directories
- Applies extension filter if provided

---

### Method: `grep_files`

```rust
pub fn grep_files(&self, pattern: &str, extension: Option<&str>) -> String
```

Searches files for regex pattern.

**Parameters:**
- `pattern`: Regex pattern (case-insensitive)
- `extension`: Optional extension filter

**Returns:** Matches in `file:line: content` format

**Limits:** Maximum 50 matches

---

### Method: `find_by_name`

```rust
pub fn find_by_name(&self, name: &str) -> String
```

Finds files whose name contains the given fragment (case-insensitive).

---

### Method: `file_stats`

```rust
pub fn file_stats(&self, target: &str) -> String
```

Returns file statistics.

**Output Format:**
```
File: {path}
Size: {bytes} bytes
Lines: {total}
Blank lines: {blank}
Code lines: {code}
```

---

### Method: `walk_source_files`

```rust
pub fn walk_source_files(&self) -> Vec<String>
```

Returns relative paths of all indexable source files.

**Filtering Criteria:**
- Has source extension (see `SOURCE_EXTENSIONS`)
- Not in ignored directory
- Size ≤ 100KB
- Not a binary file (no null bytes in first 512 bytes)

**Used by:** Palantír index builder

---

## shadow.rs — ShadowOperator

Backup and undo system for file modifications.

### Constants

```rust
const MAX_BACKUP_BYTES: u64 = 1_048_576;  // 1 MB max backup
const SHADOW_ROOT: &str = ".celebrimbot/shadow_log";
```

### Structs

```rust
pub struct ShadowOperation {
    pub op_type: String,         // "WRITE" or "DELETE"
    pub path: String,            // Relative file path
    pub backup_file: Option<String>,  // Backup filename
    pub existed: bool,           // Did file exist before?
    pub skipped: bool,           // Was backup skipped (too large)?
}

pub struct ShadowManifest {
    pub session_id: String,
    pub created_at: String,
    pub operations: Vec<ShadowOperation>,
}

pub struct UndoResult {
    pub session_id: String,
    pub restored: Vec<String>,    // Files restored to previous state
    pub deleted_new: Vec<String>, // New files deleted
    pub recreated: Vec<String>,   // Deleted files recreated
    pub errors: Vec<String>,
}
```

---

### Method: `start_session`

```rust
pub fn start_session(&mut self) -> String
```

Starts a new shadow log session.

**Behavior:**
1. Creates session directory: `.celebrimbot/shadow_log/session_2024-01-01T12-00-00/`
2. Adds `.celebrimbot/` to `.gitignore` if not present
3. Returns session ID

---

### Method: `end_session`

```rust
pub fn end_session(&mut self)
```

Ends current session and saves manifest.

**Behavior:**
1. Writes `manifest.json` with all operations
2. Prunes old sessions (keeps last `max_sessions`)

---

### Method: `backup_before_write`

```rust
pub fn backup_before_write(&mut self, relative_path: &str)
```

Called before writing a file.

**Behavior:**
- If file doesn't exist: records operation (no backup needed)
- If file > 1MB: skips backup, records as skipped
- Otherwise: copies file to session directory

---

### Method: `backup_before_delete`

```rust
pub fn backup_before_delete(&mut self, relative_path: &str)
```

Called before deleting a file.

**Behavior:** Same as `backup_before_write` but marks as DELETE operation.

---

### Method: `undo_last_session`

```rust
pub fn undo_last_session(&self) -> UndoResult
```

Undoes the most recent session.

**For WRITE operations:**
- If file existed: restore from backup
- If file didn't exist: delete the new file

**For DELETE operations:**
- If file existed: restore from backup

**Cleanup:** Removes session directory if no errors.

---

### Method: `list_sessions`

```rust
pub fn list_sessions(&self) -> Vec<SessionSummary>
```

Returns all sessions sorted by ID (oldest first).
