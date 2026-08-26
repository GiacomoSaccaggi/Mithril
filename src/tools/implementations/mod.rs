//! Tool implementations split by category.

pub mod file_tools;
pub mod terminal_tools;
pub mod web_tools;
pub mod scan_tools;
pub mod git_tools;
pub mod lore_tools;
pub mod utility_tools;

pub use file_tools::*;
pub use terminal_tools::*;
pub use web_tools::*;
pub use scan_tools::*;
pub use git_tools::*;
pub use lore_tools::*;
pub use utility_tools::*;

#[cfg(test)]
mod tool_execution_tests {
    use std::collections::HashMap;
    use crate::tools::registry::Tool;
    use crate::operators::file::FileOperator;
    use super::file_tools::{
        ReadPsiTool, WriteFileTool, DeleteFileTool, EditFileTool, PatchTool,
        parse_edit_blocks, apply_unified_patch,
    };
    use super::scan_tools::{
        ListFilesTool, GrepFilesTool, FileStatsTool, DocumentOutlineTool,
    };
    use super::lore_tools::{LoreWriteTool, LoreReadTool};
    use tempfile::tempdir;

    fn make_file_op(dir: &std::path::Path) -> FileOperator {
        FileOperator::new(dir)
    }

    #[test]
    fn test_read_psi_success() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();
        let tool = ReadPsiTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "test.txt".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert_eq!(result.output, "hello world");
    }

    #[test]
    fn test_read_psi_not_found() {
        let dir = tempdir().unwrap();
        let tool = ReadPsiTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "missing.txt".into());
        let result = tool.execute(&args);
        assert!(!result.success);
    }

    #[test]
    fn test_read_psi_missing_arg() {
        let dir = tempdir().unwrap();
        let tool = ReadPsiTool::new(make_file_op(dir.path()));
        let result = tool.execute(&HashMap::new());
        assert!(!result.success);
        assert!(result.output.contains("Missing"));
    }

    #[test]
    fn test_write_file_creates() {
        let dir = tempdir().unwrap();
        let tool = WriteFileTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "new.txt".into());
        args.insert("content".into(), "created content".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert_eq!(std::fs::read_to_string(dir.path().join("new.txt")).unwrap(), "created content");
    }

    #[test]
    fn test_delete_file_success() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("del.txt"), "x").unwrap();
        let tool = DeleteFileTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "del.txt".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert!(!dir.path().join("del.txt").exists());
    }

    #[test]
    fn test_edit_file_tool_success() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("edit.txt"), "foo\nbar\nbaz").unwrap();
        let tool = EditFileTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "edit.txt".into());
        args.insert("edits".into(), "<<<<<<< SEARCH\nbar\n=======\nBAR\n>>>>>>> REPLACE".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert!(result.output.contains("Applied 1 edit"));
        let content = std::fs::read_to_string(dir.path().join("edit.txt")).unwrap();
        assert_eq!(content, "foo\nBAR\nbaz");
    }

    #[test]
    fn test_edit_file_tool_not_found_search() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("e.txt"), "hello").unwrap();
        let tool = EditFileTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "e.txt".into());
        args.insert("edits".into(), "<<<<<<< SEARCH\nNOPE\n=======\nYES\n>>>>>>> REPLACE".into());
        let result = tool.execute(&args);
        assert!(!result.success);
        assert!(result.output.contains("not found"));
    }

    #[test]
    fn test_edit_file_multiple_edits() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("m.txt"), "aaa\nbbb\nccc").unwrap();
        let tool = EditFileTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "m.txt".into());
        args.insert("edits".into(), "<<<<<<< SEARCH\naaa\n=======\nAAA\n>>>>>>> REPLACE\n<<<<<<< SEARCH\nccc\n=======\nCCC\n>>>>>>> REPLACE".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert!(result.output.contains("Applied 2 edit"));
    }

    #[test]
    fn test_patch_tool_add_line() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("p.txt"), "line 1\nline 2\nline 3").unwrap();
        let tool = PatchTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "p.txt".into());
        args.insert("patch".into(), "@@ -1,3 +1,4 @@\n line 1\n+inserted\n line 2\n line 3".into());
        let result = tool.execute(&args);
        assert!(result.success);
        let content = std::fs::read_to_string(dir.path().join("p.txt")).unwrap();
        assert!(content.contains("inserted"));
    }

    #[test]
    fn test_lore_write_creates_file() {
        let dir = tempdir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let tool = LoreWriteTool::new();
        let mut args = HashMap::new();
        args.insert("content".into(), "remember this".into());
        args.insert("category".into(), "todo".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert!(dir.path().join(".mithril/lore.md").exists());
        let lore = std::fs::read_to_string(dir.path().join(".mithril/lore.md")).unwrap();
        assert!(lore.contains("remember this"));
        assert!(lore.contains("[todo]"));
    }

    #[test]
    fn test_lore_read_empty() {
        let dir = tempdir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let tool = LoreReadTool::new();
        let result = tool.execute(&HashMap::new());
        assert!(result.success);
        assert!(result.output.contains("empty"));
    }

    #[test]
    fn test_document_outline_rust_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn hello() {}\nstruct Foo {}\nimpl Foo {\n    fn bar() {}\n}").unwrap();
        let tool = DocumentOutlineTool::new(make_file_op(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "lib.rs".into());
        let result = tool.execute(&args);
        assert!(result.success);
        assert!(result.output.contains("hello"));
        assert!(result.output.contains("Foo"));
    }

    #[test]
    fn test_list_files_tool() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();
        let tool = ListFilesTool::new(crate::operators::scan::ScanOperator::new(dir.path()));
        let result = tool.execute(&HashMap::new());
        assert!(result.success);
    }

    #[test]
    fn test_grep_files_tool() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("x.rs"), "fn hello_world() {}").unwrap();
        let tool = GrepFilesTool::new(crate::operators::scan::ScanOperator::new(dir.path()));
        let mut args = HashMap::new();
        args.insert("pattern".into(), "hello".into());
        let result = tool.execute(&args);
        assert!(result.success);
    }

    #[test]
    fn test_file_stats_tool() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("s.txt"), "line1\nline2\nline3\n").unwrap();
        let tool = FileStatsTool::new(crate::operators::scan::ScanOperator::new(dir.path()));
        let mut args = HashMap::new();
        args.insert("target".into(), "s.txt".into());
        let result = tool.execute(&args);
        assert!(result.success);
    }
}

