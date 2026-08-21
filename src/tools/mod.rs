//! MCP Tools — 21 built-in tools the LLM can invoke.
//!
//! Each tool implements the [`registry::Tool`] trait and is registered
//! in [`create_default_registry`].
//!
//! ```mermaid
//! graph TD
//!     R[ToolRegistry] --> F[File: read_psi write edit delete patch]
//!     R --> T[Terminal: run_terminal]
//!     R --> W[Web: web_search fetch_page]
//!     R --> S[Scan: list grep find stats]
//!     R --> G[Git: status log diff blame branch]
//!     R --> C[Code: search_symbols document_outline]
//!     R --> K[Knowledge: lore_write lore_read]
//!     F --> FO[FileOperator]
//!     T --> TO[TerminalOperator]
//!     W --> WO[WebOperator]
//!     S --> SO[ScanOperator]
//!     G --> GO[GitOperator]
//! ```

pub mod registry;
pub mod implementations;

use crate::operators::{
    file::FileOperator, git::GitOperator, scan::ScanOperator,
    terminal::TerminalOperator, web::WebOperator,
};
use registry::ToolRegistry;

pub fn create_default_registry(base_path: &str) -> ToolRegistry {
    use implementations::*;

    // Read sandbox setting from config (default: true)
    let sandbox = crate::config::MithrilConfig::load()
        .map(|c| c.terminal_sandbox)
        .unwrap_or(true);

    let file_op = FileOperator::new(base_path);
    let term_op = TerminalOperator::new(base_path, 30).with_sandbox(sandbox);
    let git_op = GitOperator::new(base_path);
    let web_op = WebOperator::new();
    let scan_op = ScanOperator::new(base_path);

    let mut registry = ToolRegistry::new();
    registry.register(ReadPsiTool::new(file_op.clone()));
    registry.register(DeleteFileTool::new(file_op.clone()));
    registry.register(WriteFileTool::new(file_op.clone()));
    registry.register(EditFileTool::new(file_op.clone()));
    registry.register(RunTerminalTool::new(term_op));
    registry.register(WebSearchTool::new(web_op.clone()));
    registry.register(FetchPageTool::new(web_op));
    registry.register(ListFilesTool::new(scan_op.clone()));
    registry.register(GrepFilesTool::new(scan_op.clone()));
    registry.register(FindFileTool::new(scan_op.clone()));
    registry.register(FileStatsTool::new(scan_op.clone()));
    registry.register(GitStatusTool::new(git_op.clone()));
    registry.register(GitLogTool::new(git_op.clone()));
    registry.register(GitDiffTool::new(git_op.clone()));
    registry.register(GitBlameTool::new(git_op.clone()));
    registry.register(GitBranchTool::new(git_op));
    registry.register(SearchSymbolsTool::new(scan_op.clone()));
    registry.register(DocumentOutlineTool::new(file_op.clone()));
    registry.register(LoreWriteTool::new());
    registry.register(LoreReadTool::new());
    registry.register(PatchTool::new(file_op));
    registry
}
