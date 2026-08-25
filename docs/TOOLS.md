# The Armory — Twenty-Four Tools

> *"It's a dangerous business, Frodo, going out your door."* — Bilbo Baggins

The Armory contains twenty-four tools forged for the tasks that lie ahead. Each tool is available via MCP protocol and can be invoked by agents during chat sessions.

---

## Tool Overview

| Category | Count | Tools |
|----------|-------|-------|
| File Operations | 5 | `read_file`, `write_file`, `edit_file`, `delete_file`, `apply_patch` |
| Discovery | 4 | `list_files`, `grep_files`, `find_file`, `file_stats` |
| Git | 6 | `git_status`, `git_log`, `git_diff`, `git_blame`, `git_branch`, `git_commit` |
| Terminal | 1 | `run_terminal` |
| Web | 2 | `web_search`, `fetch_page` |
| Code Intelligence | 2 | `search_symbols`, `document_outline` |
| Project Lore | 2 | `lore_write`, `lore_read` |
| Session | 2 | `share_session` |

---

## File Operations

> *"I wish it need not have happened in my time," said Frodo. "So do I," said Gandalf, "and so do all who live to see such times."*

### `read_file`

Read the contents of a file from the filesystem.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | yes | Path to the file |
| `encoding` | string | no | Character encoding (default: utf-8) |
| `line_start` | integer | no | Start reading from this line |
| `line_end` | integer | no | Stop reading at this line |

**Returns:** File contents as string, or error if file not found.

---

### `write_file`

Write content to a file, creating directories as needed.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | yes | Path to the file |
| `content` | string | yes | Content to write |
| `encoding` | string | no | Character encoding (default: utf-8) |

**Returns:** Confirmation with bytes written.

**Shadow Log:** Original file is backed up before overwrite.

---

### `edit_file`

Apply targeted edits to a file using search/replace pairs.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | yes | Path to the file |
| `edits` | array | yes | Array of `{search, replace}` pairs |

**Returns:** Number of edits applied.

**Shadow Log:** Original file is backed up before modification.

---

### `delete_file`

Remove a file from the filesystem.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | yes | Path to the file |

**Returns:** Confirmation of deletion.

**Shadow Log:** File is backed up before deletion.

---

### `apply_patch`

Apply a unified diff patch to files.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `patch` | string | yes | Unified diff content |
| `base_path` | string | no | Base directory for relative paths |

**Returns:** List of files modified.

**Shadow Log:** All affected files are backed up.

---

## Discovery

> *"Not all those who wander are lost."* — Bilbo Baggins

### `list_files`

List files and directories in a path.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | yes | Directory path |
| `recursive` | boolean | no | Include subdirectories (default: false) |
| `pattern` | string | no | Glob pattern to filter files |
| `max_depth` | integer | no | Maximum recursion depth |

**Returns:** Array of file entries with metadata.

---

### `grep_files`

Search for patterns across files.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `pattern` | string | yes | Regex pattern to search |
| `path` | string | no | Directory to search (default: .) |
| `include` | string | no | Glob pattern for files to include |
| `exclude` | string | no | Glob pattern for files to exclude |
| `max_results` | integer | no | Limit number of matches |

**Returns:** Array of matches with file, line number, and context.

---

### `find_file`

Find files by name pattern.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | string | yes | File name or glob pattern |
| `path` | string | no | Starting directory (default: .) |
| `type` | string | no | Filter: "file", "directory", or "any" |

**Returns:** Array of matching paths.

---

### `file_stats`

Get metadata about a file or directory.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | yes | Path to examine |

**Returns:** Object with size, modified time, permissions, type.

---

## Git Mastery

> *"All we have to decide is what to do with the time that is given us."* — Gandalf

### `git_status`

Get the current git repository status.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | no | Repository path (default: .) |

**Returns:** Object with branch, staged files, modified files, untracked files.

---

### `git_log`

View commit history.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | no | Repository path (default: .) |
| `count` | integer | no | Number of commits (default: 10) |
| `branch` | string | no | Branch to query |
| `file` | string | no | Filter to specific file |

**Returns:** Array of commit objects with hash, author, date, message.

---

### `git_diff`

Show changes between commits, working tree, or staged files.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | no | Repository path (default: .) |
| `staged` | boolean | no | Show staged changes |
| `commit` | string | no | Compare against specific commit |
| `file` | string | no | Filter to specific file |

**Returns:** Unified diff output.

---

### `git_blame`

Show line-by-line authorship of a file.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | yes | File path |
| `line_start` | integer | no | Start line |
| `line_end` | integer | no | End line |

**Returns:** Array of blame entries with commit, author, date per line.

---

### `git_branch`

List or manage branches.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | no | Repository path (default: .) |
| `list` | boolean | no | List all branches |
| `create` | string | no | Create new branch with this name |
| `delete` | string | no | Delete branch with this name |

**Returns:** Branch list or confirmation of operation.

---

### `git_commit`

Create a commit with staged changes.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | no | Repository path (default: .) |
| `message` | string | yes | Commit message |
| `add` | array | no | Files to stage before commit |

**Returns:** Commit hash and summary.

---

## Terminal

> *"There is only one Lord of the Ring, only one who can bend it to his will."*

### `run_terminal`

Execute a shell command in a sandboxed environment.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `command` | string | yes | Command to execute |
| `working_dir` | string | no | Working directory |
| `timeout` | integer | no | Timeout in seconds (default: 30) |
| `env` | object | no | Environment variables |

**Returns:** Object with stdout, stderr, exit code.

**Security:** The Terminal Sanctuary blocks dangerous commands. See [SECURITY.md](SECURITY.md).

---

## Web Scouting

> *"Even the very wise cannot see all ends."* — Gandalf

### `web_search`

Search the web for information.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | yes | Search query |
| `max_results` | integer | no | Number of results (default: 5) |

**Returns:** Array of results with title, url, snippet.

---

### `fetch_page`

Retrieve and extract content from a web page.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `url` | string | yes | URL to fetch |
| `selector` | string | no | CSS selector to extract specific content |
| `format` | string | no | Output format: "text", "markdown", "html" |

**Returns:** Page content in requested format.

---

## Code Intelligence

> *"I am a servant of the Secret Fire, wielder of the flame of Anor."* — Gandalf

### `search_symbols`

Search for code symbols across the project.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | yes | Symbol name or pattern |
| `path` | string | no | Search path (default: .) |
| `type` | string | no | Symbol type: "function", "class", "variable", "any" |

**Returns:** Array of symbols with name, type, file, line.

---

### `document_outline`

Extract the structure of a source file.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | yes | Path to source file |

**Returns:** Hierarchical structure of classes, functions, and symbols.

---

## Project Lore

> *"I sit beside the fire and think of people long ago."* — Bilbo Baggins

### `lore_write`

Write project knowledge to the lore store.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `key` | string | yes | Lore key/identifier |
| `content` | string | yes | Knowledge to store |
| `tags` | array | no | Tags for categorization |

**Returns:** Confirmation with lore entry ID.

---

### `lore_read`

Retrieve project knowledge from the lore store.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `key` | string | no | Specific key to retrieve |
| `query` | string | no | Search query across lore |
| `tags` | array | no | Filter by tags |

**Returns:** Matching lore entries.

---

## Session Control

> *"I will not say: do not weep; for not all tears are an evil."* — Gandalf

### `share_session`

Share the current session for access from another interface.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `target` | string | yes | Target interface: "telegram", "web", "cli" |
| `expiry` | integer | no | Minutes until share expires |

**Returns:** Share token and instructions.

---

## Tool Permissions by Mode

| Mode | Read Tools | Write Tools | Terminal |
|------|------------|-------------|----------|
| **Plan** | All | None | Read-only commands |
| **Build** | All | All | All (with sanctuary) |

---

## Error Handling

All tools return errors in a consistent format:

```json
{
  "error": {
    "code": "FILE_NOT_FOUND",
    "message": "The path '/foo/bar' does not exist",
    "details": {}
  }
}
```

Common error codes:
- `FILE_NOT_FOUND` — Path does not exist
- `PERMISSION_DENIED` — Operation not allowed
- `PATH_TRAVERSAL` — Attempted escape from allowed paths
- `SANCTUARY_BLOCKED` — Command blocked by terminal sanctuary
- `TIMEOUT` — Operation exceeded time limit

---

> *"May the wind under your wings bear you where the sun sails and the moon walks."*
