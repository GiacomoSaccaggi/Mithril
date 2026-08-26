# Operators — Rangers of the North

Low-level I/O operations with security: sandboxed terminal, path traversal protection, shadow log for undo.

## Key Concept

Tools don't touch the filesystem directly — they go through operators that enforce security boundaries.

## Operators

- **FileOperator** — Path traversal blocked, base path enforcement
- **TerminalOperator** — Command blocklist, timeout, sandboxed execution
- **ShadowOperator** — Undo system (saves old file content before writes)
- **GitOperator** — Git operations via subprocess
- **WebOperator** — HTTP fetching with search API
- **ScanOperator** — Directory traversal respecting .gitignore

## Files

- `file.rs` — 
- `git.rs` — 
- `mod.rs` — 
- `scan.rs` — 
- `shadow.rs` — 
- `terminal.rs` — 
- `web.rs` — 
