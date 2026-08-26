# Tools — The Armory

24 executable tools that agents can invoke: file I/O, git, web search, terminal, code analysis.

## Key Concept

Every tool implements the Tool trait (name, description, params, execute). The registry makes them available to agents as function-calling definitions.

## Categories (24 Total)

- **File (5):** read_psi, write_file, edit_file, delete_file, apply_patch
- **Discovery (5):** list_files, grep_files, find_file, file_stats, glob_files
- **Git (5):** git_status, git_log, git_diff, git_blame, git_branch
- **Web (2):** web_search, fetch_page
- **Code (2):** search_symbols, document_outline
- **Terminal (1):** run_terminal (sandboxed)
- **Knowledge (2):** lore_write, lore_read
- **Interaction (2):** todo_write, question

## Adding a New Tool

1. Create struct implementing `Tool` trait in the appropriate category file
2. Register in `src/tools/mod.rs` `create_default_registry()`
3. Done — all agents can use it immediately

## Files

- `implementations/file_tools.rs` — 
- `implementations/git_tools.rs` — 
- `implementations/lore_tools.rs` — 
- `implementations/mod.rs` — Tool implementations split by category.
- `implementations/scan_tools.rs` — 
- `implementations/terminal_tools.rs` — 
- `implementations/utility_tools.rs` — 
- `implementations/web_tools.rs` — 
- `mod.rs` — MCP Tools — 21 built-in tools the LLM can invoke.
- `registry.rs` — 
