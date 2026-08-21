# Tools Module

The tools module provides a registry of 21 built-in tools that can be called by LLMs via MCP and the agentic loop.

**Location**: `src/tools/`

## Files

| File | Purpose |
|------|---------|
| `registry.rs` | Tool trait, ToolRegistry, JSON schema export |
| `implementations.rs` | All 23 tool structs (21 registered in default registry) |
| `mod.rs` | Factory function `create_default_registry()` |

---

## registry.rs — Tool Registry

### Struct: `ToolParam`

```rust
pub struct ToolParam {
    pub name: String,        // Parameter name
    pub param_type: String,  // JSON Schema type (always "string")
    pub description: String, // Human-readable description
    pub required: bool,      // Is this parameter required?
}
```

### Struct: `ToolResult`

```rust
pub struct ToolResult {
    pub success: bool,   // Did the operation succeed?
    pub output: String,  // Output text (or error message)
}

impl ToolResult {
    pub fn ok(output: impl Into<String>) -> Self
    pub fn err(output: impl Into<String>) -> Self
}
```

### Trait: `Tool`

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn parameters(&self) -> Vec<ToolParam>;
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult;
}
```

### Struct: `ToolRegistry`

```rust
impl ToolRegistry {
    pub fn new() -> Self
    pub fn register<T: Tool + 'static>(&mut self, tool: T)
    pub fn get(&self, name: &str) -> Option<&dyn Tool>
    pub fn all(&self) -> Vec<&dyn Tool>
    pub fn to_json_schema(&self) -> String       // For planner prompts
    pub fn to_mcp_tool_list(&self) -> Vec<Value> // For MCP tools/list
    pub fn len(&self) -> usize
}
```

---

## mod.rs — Factory

### Function: `create_default_registry`

```rust
pub fn create_default_registry(base_path: &str) -> ToolRegistry
```

Creates a registry with all **21 default tools** configured for the given base path.

**Tools registered (in order):**

| # | Struct | Tool Name | Operator |
|---|--------|-----------|----------|
| 1 | `ReadPsiTool` | `read_psi` | FileOperator |
| 2 | `DeleteFileTool` | `delete_file` | FileOperator |
| 3 | `WriteFileTool` | `write_file` | FileOperator |
| 4 | `EditFileTool` | `edit_file` | FileOperator |
| 5 | `RunTerminalTool` | `run_terminal` | TerminalOperator |
| 6 | `WebSearchTool` | `web_search` | WebOperator |
| 7 | `FetchPageTool` | `fetch_page` | WebOperator |
| 8 | `ListFilesTool` | `list_files` | ScanOperator |
| 9 | `GrepFilesTool` | `grep_files` | ScanOperator |
| 10 | `FindFileTool` | `find_file` | ScanOperator |
| 11 | `FileStatsTool` | `file_stats` | ScanOperator |
| 12 | `GitStatusTool` | `git_status` | GitOperator |
| 13 | `GitLogTool` | `git_log` | GitOperator |
| 14 | `GitDiffTool` | `git_diff` | GitOperator |
| 15 | `GitBlameTool` | `git_blame` | GitOperator |
| 16 | `GitBranchTool` | `git_branch` | GitOperator |
| 17 | `SearchSymbolsTool` | `search_symbols` | ScanOperator |
| 18 | `DocumentOutlineTool` | `document_outline` | FileOperator |
| 19 | `LoreWriteTool` | `lore_write` | (standalone) |
| 20 | `LoreReadTool` | `lore_read` | (standalone) |
| 21 | `PatchTool` | `apply_patch` | FileOperator |

**Not registered** (exist as structs but not in default registry):
- `SessionReadTool` / `SessionWriteTool` — used only in MCP server context for Junie handoff

---

## File Tools

### Tool: `read_psi`

**Description:** Read the content of a file

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | Yes | Relative file path |

**Returns:** File content or error message

---

### Tool: `write_file`

**Description:** Write raw content to a file (creates or overwrites)

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | Yes | Relative file path |
| `content` | string | Yes | Full file content |

**Behavior:**
- Creates parent directories if needed
- Overwrites existing files
- Tracked by shadow log for undo

---

### Tool: `edit_file`

**Description:** Apply targeted edits to a file using search/replace blocks. Preferred over `write_file` for modifying existing files.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | Yes | Relative file path |
| `edits` | string | Yes | One or more search/replace blocks |

**Edit block format:**
```
<<<<<<< SEARCH
exact text to find
=======
replacement text
>>>>>>> REPLACE
```

**Behavior:**
- Reads current file content
- Parses all edit blocks
- Verifies all search texts exist before applying any
- Applies edits sequentially (atomic — all succeed or all fail)
- Returns error with preview if search text not found

---

### Tool: `delete_file`

**Description:** Delete a file

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | Yes | Relative file path |

**Returns:** Success message or error. Tracked by shadow log for undo.

---

### Tool: `apply_patch`

**Description:** Apply a unified diff patch to a file

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | Yes | File path to patch |
| `patch` | string | Yes | Unified diff format content |

**Behavior:**
- Parses unified diff format (`@@ -start,count +start,count @@`)
- Applies hunks to the target file
- Returns error if hunks don't match

---

## Terminal Tools

### Tool: `run_terminal`

**Description:** Execute a shell command

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `command` | string | Yes | Shell command to execute |

**Behavior:**
- Runs in the project's working directory
- 30-second timeout
- Returns exit code in output if non-zero
- Uses `sh -c` on Unix, `cmd /C` on Windows
- **Sandbox:** Blocks dangerous commands (`rm -rf /`, `sudo`, fork bombs, `dd if=`) unless disabled

---

## Web Tools

### Tool: `web_search`

**Description:** Search the web via DuckDuckGo

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | Yes | Search query |

**Returns:** Search results with summaries and URLs (DuckDuckGo Instant Answer API)

---

### Tool: `fetch_page`

**Description:** Fetch and read a URL

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | Yes | URL to fetch |

**Returns:** Page content (HTML stripped, truncated to 4000 chars)

---

## Scan Tools

### Tool: `list_files`

**Description:** List project files, optionally filtered

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | No | Sub-path to list |
| `extension` | string | No | File extension filter |

**Returns:** Newline-separated list of relative paths

---

### Tool: `grep_files`

**Description:** Regex search across project files

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `pattern` | string | Yes | Regex pattern |
| `extension` | string | No | File extension filter |

**Returns:** Matching lines with `file:line` format (max 50 matches)

---

### Tool: `find_file`

**Description:** Find files by name fragment

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | Yes | File name fragment |

**Returns:** Matching file paths

---

### Tool: `file_stats`

**Description:** Show line count and size of a file

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | Yes | Relative file path |

**Returns:** File statistics (size, lines, blank lines, code lines)

---

## Git Tools

### Tool: `git_status`

**Description:** Show working tree status

**Parameters:** None

**Returns:** Output of `git status --short`

---

### Tool: `git_log`

**Description:** Show recent commit history

**Parameters:** None

**Returns:** Last 10 commits (`git log --oneline --decorate -10`)

---

### Tool: `git_diff`

**Description:** Show uncommitted changes

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | No | Specific file (omit for full diff) |

**Returns:** Diff output (truncated to 6000 chars)

---

### Tool: `git_blame`

**Description:** Show per-line authorship of a file

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | Yes | Relative file path |

**Returns:** Blame output (truncated to 4000 chars)

---

### Tool: `git_branch`

**Description:** Show current branch name

**Parameters:** None

**Returns:** Current branch name

---

## Code Intelligence Tools

### Tool: `search_symbols`

**Description:** Search for symbol definitions (functions, classes, structs, traits, etc.) across the project. Returns file paths and line numbers where matching symbols are defined.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | Yes | Symbol name or pattern to search for |
| `extension` | string | No | File extension filter (e.g. `rs`, `py`) |

**Returns:** Matching definitions in `file:line → definition` format (max 50 results)

**Supported patterns:** `fn`, `struct`, `enum`, `trait`, `impl`, `mod`, `class`, `def`, `function`, `interface`, `type`, `const`, `let`, `var`, `object`, `fun`, `val`

---

### Tool: `document_outline`

**Description:** Get the structural outline of a file — all function, class, struct, trait, and method definitions with line numbers. Useful for understanding file organization without reading the full content.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | Yes | File path to outline |

**Returns:** Indented outline with line numbers

**Language support:**
- Rust: `fn`, `struct`, `enum`, `trait`, `impl`, `mod`
- Python: `class`, `def`
- JavaScript/TypeScript: `function`, `class`, `const`, `interface`, `type`
- Go: `func`, `type`
- Java/Kotlin/Scala: `class`, `interface`, `fun`, `val`, `var`, `object`, `enum`

---

## Lore Tools (Persistent Memory)

The Lore is Mithril's long-term project memory — notes, TODOs, and context that survive across sessions. Stored at `.mithril/lore.md` in the project root.

### Tool: `lore_write`

**Description:** Write a note to the project's persistent memory. Use this to record TODOs, decisions, known issues, or anything that should survive across sessions. Notes are timestamped and appended to `.mithril/lore.md`.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `content` | string | Yes | The note/TODO/decision to record |
| `category` | string | No | Category tag: `todo`, `decision`, `issue`, `note` (default: `note`) |

**Storage format:**
```markdown
## [category] 2026-08-19 17:00

content goes here
```

---

### Tool: `lore_read`

**Description:** Read the project's persistent memory (Lore). Optionally filter by category.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `category` | string | No | Filter by category tag (omit to read all) |

**Returns:** Full lore content or filtered entries

---

## Tool Safety

In the **agentic loop** (interactive chat, TUI, exec), tools are classified as safe or dangerous:

**Dangerous tools** (require user confirmation in interactive mode):
- `write_file`
- `edit_file`
- `apply_patch`
- `delete_file`
- `run_terminal`

In headless `mithril exec` mode, all tools execute without confirmation.

**Terminal sandbox** blocks before execution:
- `rm -rf /`, `rm -rf ~`
- `sudo` commands
- Fork bombs (`:(){ :|:& };:`)
- Disk erasure (`dd if=/dev/zero of=/dev/sda`)
- System shutdown/reboot

Disable with: `mithril config set terminal_sandbox false`

### Configurable Permissions

Override per-tool behavior in `~/.mithril/config.yaml`:

```yaml
permissions:
  run_terminal: allow    # Never ask, always execute
  delete_file: deny      # Completely disable this tool
  write_file: ask        # Prompt for confirmation (default for dangerous tools)
```

**Permission levels:**
- `allow` — Execute without confirmation (default for read-only tools)
- `deny` — Tool is disabled; LLM receives an error if it tries to use it
- `ask` — Prompt user for [y/N] confirmation (default for dangerous tools)

Use `--no-confirm` flag with `mithril chat` to auto-approve all tools for that session.
