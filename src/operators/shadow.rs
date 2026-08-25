#![allow(dead_code)]
use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};

const MAX_BACKUP_BYTES: u64 = 1_048_576; // 1 MB
const SHADOW_ROOT: &str = ".celebrimbot/shadow_log";
const GITIGNORE_ENTRY: &str = ".celebrimbot/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowOperation {
    pub op_type: String, // "WRITE" or "DELETE"
    pub path: String,
    pub backup_file: Option<String>,
    pub existed: bool,
    pub skipped: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShadowManifest {
    pub session_id: String,
    pub created_at: String,
    pub operations: Vec<ShadowOperation>,
}

#[derive(Debug)]
pub struct SessionSummary {
    pub session_id: String,
    pub created_at: String,
    pub operation_count: usize,
}

#[derive(Debug)]
pub struct UndoResult {
    pub session_id: String,
    pub restored: Vec<String>,
    pub deleted_new: Vec<String>,
    pub recreated: Vec<String>,
    pub errors: Vec<String>,
}

/// Write file with 0600 permissions on Unix.
fn write_restricted(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true).create(true).truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(data)?;
        Ok(())
    }
    #[cfg(not(unix))]
    { std::fs::write(path, data) }
}


/// Port of HeadlessShadowLogOperator.kt
pub struct ShadowOperator {
    base_path: PathBuf,
    max_sessions: usize,
    current_session_id: Option<String>,
    pending_ops: Vec<ShadowOperation>,
}

impl ShadowOperator {
    pub fn new(base_path: impl Into<PathBuf>, max_sessions: usize) -> Self {
        Self {
            base_path: base_path.into(),
            max_sessions,
            current_session_id: None,
            pending_ops: Vec::new(),
        }
    }

    fn shadow_root(&self) -> PathBuf {
        self.base_path.join(SHADOW_ROOT)
    }

    fn encode_path(path: &str) -> String {
        path.replace(['/', '\\'], "__")
    }

    fn ensure_gitignore(&self) {
        let gi = self.base_path.join(".gitignore");
        let entry = GITIGNORE_ENTRY;
        let current = fs::read_to_string(&gi).unwrap_or_default();
        if !current.contains(entry) {
            let _ = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&gi)
                .map(|mut f| {
                    use std::io::Write;
                    let _ = writeln!(f, "\n# Mithril shadow log\n{entry}");
                });
        }
    }

    pub fn start_session(&mut self) -> String {
        let ts = Utc::now().format("session_%Y-%m-%dT%H-%M-%S").to_string();
        let session_dir = self.shadow_root().join(&ts);
        let _ = fs::create_dir_all(&session_dir);
        self.ensure_gitignore();
        self.current_session_id = Some(ts.clone());
        self.pending_ops.clear();
        ts
    }

    pub fn end_session(&mut self) {
        let Some(session_id) = self.current_session_id.take() else { return };
        let manifest = ShadowManifest {
            session_id: session_id.clone(),
            created_at: Utc::now().to_rfc3339(),
            operations: std::mem::take(&mut self.pending_ops),
        };
        let session_dir = self.shadow_root().join(&session_id);
        let manifest_path = session_dir.join("manifest.json");
        if let Ok(json) = serde_json::to_string_pretty(&manifest) {
            // M2: restrict permissions on manifest containing file operation history
            let _ = write_restricted(&manifest_path, json.as_bytes());
        }
        self.prune_old_sessions();
    }

    pub fn backup_before_write(&mut self, relative_path: &str) {
        let Some(session_id) = &self.current_session_id else { return };
        let source = self.base_path.join(relative_path);

        if !source.exists() {
            self.pending_ops.push(ShadowOperation {
                op_type: "WRITE".into(),
                path: relative_path.to_string(),
                backup_file: None,
                existed: false,
                skipped: false,
            });
            return;
        }

        let size = source.metadata().map(|m| m.len()).unwrap_or(0);
        if size > MAX_BACKUP_BYTES {
            self.pending_ops.push(ShadowOperation {
                op_type: "WRITE".into(),
                path: relative_path.to_string(),
                backup_file: None,
                existed: true,
                skipped: true,
            });
            return;
        }

        let backup_name = Self::encode_path(relative_path);
        let dest = self.shadow_root().join(session_id).join(&backup_name);
        if let Ok(data) = fs::read(&source) {
            let _ = write_restricted(&dest, &data);
        }
        self.pending_ops.push(ShadowOperation {
            op_type: "WRITE".into(),
            path: relative_path.to_string(),
            backup_file: Some(backup_name),
            existed: true,
            skipped: false,
        });
    }

    pub fn backup_before_delete(&mut self, relative_path: &str) {
        let Some(session_id) = &self.current_session_id else { return };
        let source = self.base_path.join(relative_path);

        if !source.exists() {
            self.pending_ops.push(ShadowOperation {
                op_type: "DELETE".into(),
                path: relative_path.to_string(),
                backup_file: None,
                existed: false,
                skipped: false,
            });
            return;
        }

        let size = source.metadata().map(|m| m.len()).unwrap_or(0);
        if size > MAX_BACKUP_BYTES {
            self.pending_ops.push(ShadowOperation {
                op_type: "DELETE".into(),
                path: relative_path.to_string(),
                backup_file: None,
                existed: true,
                skipped: true,
            });
            return;
        }

        let backup_name = Self::encode_path(relative_path) + ".DELETED";
        let dest = self.shadow_root().join(session_id).join(&backup_name);
        if let Ok(data) = fs::read(&source) {
            let _ = write_restricted(&dest, &data);
        }
        self.pending_ops.push(ShadowOperation {
            op_type: "DELETE".into(),
            path: relative_path.to_string(),
            backup_file: Some(backup_name),
            existed: true,
            skipped: false,
        });
    }

    pub fn undo_last_session(&self) -> UndoResult {
        let sessions = self.list_sessions();
        if sessions.is_empty() {
            return UndoResult {
                session_id: "none".into(),
                restored: vec![],
                deleted_new: vec![],
                recreated: vec![],
                errors: vec!["No sessions found".into()],
            };
        }

        let latest = match sessions.into_iter().last() {
            Some(s) => s,
            None => return UndoResult {
                session_id: String::new(),
                restored: vec![],
                deleted_new: vec![],
                recreated: vec![],
                errors: vec!["No sessions found".into()],
            },
        };
        let session_dir = self.shadow_root().join(&latest.session_id);
        let manifest_path = session_dir.join("manifest.json");

        let manifest: ShadowManifest = match fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(m) => m,
            None => {
                return UndoResult {
                    session_id: latest.session_id,
                    restored: vec![],
                    deleted_new: vec![],
                    recreated: vec![],
                    errors: vec!["Could not read manifest".into()],
                }
            }
        };

        let mut restored = vec![];
        let mut deleted_new = vec![];
        let mut recreated = vec![];
        let mut errors = vec![];

        for op in manifest.operations.iter().rev() {
            if op.skipped {
                continue;
            }
            let target = self.base_path.join(&op.path);
            match op.op_type.as_str() {
                "WRITE" => {
                    if op.existed {
                        if let Some(ref bf) = op.backup_file {
                            let backup = session_dir.join(bf);
                            if let Some(parent) = target.parent() {
                                let _ = fs::create_dir_all(parent);
                            }
                            match fs::copy(&backup, &target) {
                                Ok(_) => restored.push(op.path.clone()),
                                Err(e) => errors.push(format!("{}: {e}", op.path)),
                            }
                        }
                    } else {
                        match fs::remove_file(&target) {
                            Ok(_) => deleted_new.push(op.path.clone()),
                            Err(e) => errors.push(format!("{}: {e}", op.path)),
                        }
                    }
                }
                "DELETE"
                    if op.existed => {
                        if let Some(ref bf) = op.backup_file {
                            let backup = session_dir.join(bf);
                            if let Some(parent) = target.parent() {
                                let _ = fs::create_dir_all(parent);
                            }
                            match fs::copy(&backup, &target) {
                                Ok(_) => recreated.push(op.path.clone()),
                                Err(e) => errors.push(format!("{}: {e}", op.path)),
                            }
                        }
                    }
                _ => {}
            }
        }

        if errors.is_empty() {
            let _ = fs::remove_dir_all(&session_dir);
        }

        UndoResult {
            session_id: latest.session_id,
            restored,
            deleted_new,
            recreated,
            errors,
        }
    }

    pub fn list_sessions(&self) -> Vec<SessionSummary> {
        let root = self.shadow_root();
        if !root.exists() {
            return vec![];
        }
        let mut sessions: Vec<SessionSummary> = fs::read_dir(&root)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && e.file_name().to_string_lossy().starts_with("session_")
            })
            .filter_map(|e| {
                let manifest_path = e.path().join("manifest.json");
                let manifest: ShadowManifest =
                    serde_json::from_str(&fs::read_to_string(manifest_path).ok()?).ok()?;
                Some(SessionSummary {
                    session_id: manifest.session_id,
                    created_at: manifest.created_at,
                    operation_count: manifest.operations.len(),
                })
            })
            .collect();
        sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        sessions
    }

    fn prune_old_sessions(&self) {
        let root = self.shadow_root();
        if !root.exists() {
            return;
        }
        let mut dirs: Vec<PathBuf> = fs::read_dir(&root)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && e.file_name().to_string_lossy().starts_with("session_")
            })
            .map(|e| e.path())
            .collect();
        dirs.sort();
        if dirs.len() > self.max_sessions {
            for old in dirs.iter().take(dirs.len() - self.max_sessions) {
                let _ = fs::remove_dir_all(old);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_shadow_operator_new() {
        let op = ShadowOperator::new("/tmp/test", 5);
        assert_eq!(op.max_sessions, 5);
        assert!(op.current_session_id.is_none());
        assert!(op.pending_ops.is_empty());
    }

    #[test]
    fn test_shadow_root_path() {
        let op = ShadowOperator::new("/my/project", 10);
        let root = op.shadow_root();
        assert!(root.ends_with(".celebrimbot/shadow_log"));
    }

    #[test]
    fn test_encode_path_slashes() {
        assert_eq!(ShadowOperator::encode_path("src/main.rs"), "src__main.rs");
        assert_eq!(ShadowOperator::encode_path("nested/deep/file.txt"), "nested__deep__file.txt");
    }

    #[test]
    fn test_encode_path_backslashes() {
        assert_eq!(ShadowOperator::encode_path("src\\main.rs"), "src__main.rs");
    }

    #[test]
    fn test_encode_path_no_slashes() {
        assert_eq!(ShadowOperator::encode_path("file.txt"), "file.txt");
    }

    #[test]
    fn test_start_session_creates_dir() {
        let dir = tempdir().unwrap();
        let mut op = ShadowOperator::new(dir.path(), 5);

        let session_id = op.start_session();
        assert!(session_id.starts_with("session_"));
        assert!(op.current_session_id.is_some());

        let session_dir = dir.path().join(SHADOW_ROOT).join(&session_id);
        assert!(session_dir.exists());
    }

    #[test]
    fn test_start_session_ensures_gitignore() {
        let dir = tempdir().unwrap();
        let mut op = ShadowOperator::new(dir.path(), 5);
        op.start_session();

        let gitignore = dir.path().join(".gitignore");
        assert!(gitignore.exists());
        let content = fs::read_to_string(&gitignore).unwrap();
        assert!(content.contains(GITIGNORE_ENTRY));
    }

    #[test]
    fn test_backup_before_write_nonexistent_file() {
        let dir = tempdir().unwrap();
        let mut op = ShadowOperator::new(dir.path(), 5);
        op.start_session();

        op.backup_before_write("nonexistent.txt");

        assert_eq!(op.pending_ops.len(), 1);
        let op_record = &op.pending_ops[0];
        assert_eq!(op_record.op_type, "WRITE");
        assert!(!op_record.existed);
        assert!(op_record.backup_file.is_none());
    }

    #[test]
    fn test_backup_before_write_existing_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("existing.txt"), "original content").unwrap();

        let mut op = ShadowOperator::new(dir.path(), 5);
        let session_id = op.start_session();

        op.backup_before_write("existing.txt");

        assert_eq!(op.pending_ops.len(), 1);
        let op_record = &op.pending_ops[0];
        assert_eq!(op_record.op_type, "WRITE");
        assert!(op_record.existed);
        assert!(op_record.backup_file.is_some());

        // Check backup was actually created
        let backup_path = dir.path()
            .join(SHADOW_ROOT)
            .join(&session_id)
            .join(op_record.backup_file.as_ref().unwrap());
        assert!(backup_path.exists());
        assert_eq!(fs::read_to_string(&backup_path).unwrap(), "original content");
    }

    #[test]
    fn test_backup_before_write_large_file_skipped() {
        let dir = tempdir().unwrap();
        // Create file larger than MAX_BACKUP_BYTES (1MB)
        let large_content = "x".repeat(1_100_000);
        fs::write(dir.path().join("large.bin"), &large_content).unwrap();

        let mut op = ShadowOperator::new(dir.path(), 5);
        op.start_session();

        op.backup_before_write("large.bin");

        assert_eq!(op.pending_ops.len(), 1);
        let op_record = &op.pending_ops[0];
        assert!(op_record.skipped);
        assert!(op_record.existed);
        assert!(op_record.backup_file.is_none());
    }

    #[test]
    fn test_backup_before_write_no_session() {
        let dir = tempdir().unwrap();
        let mut op = ShadowOperator::new(dir.path(), 5);
        // Don't start session
        op.backup_before_write("file.txt");
        // Should be a no-op
        assert!(op.pending_ops.is_empty());
    }

    #[test]
    fn test_backup_before_delete_nonexistent_file() {
        let dir = tempdir().unwrap();
        let mut op = ShadowOperator::new(dir.path(), 5);
        op.start_session();

        op.backup_before_delete("ghost.txt");

        assert_eq!(op.pending_ops.len(), 1);
        let op_record = &op.pending_ops[0];
        assert_eq!(op_record.op_type, "DELETE");
        assert!(!op_record.existed);
    }

    #[test]
    fn test_backup_before_delete_existing_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("to_delete.txt"), "delete me").unwrap();

        let mut op = ShadowOperator::new(dir.path(), 5);
        let session_id = op.start_session();

        op.backup_before_delete("to_delete.txt");

        assert_eq!(op.pending_ops.len(), 1);
        let op_record = &op.pending_ops[0];
        assert_eq!(op_record.op_type, "DELETE");
        assert!(op_record.existed);
        assert!(op_record.backup_file.is_some());
        assert!(op_record.backup_file.as_ref().unwrap().ends_with(".DELETED"));

        // Check backup exists
        let backup_path = dir.path()
            .join(SHADOW_ROOT)
            .join(&session_id)
            .join(op_record.backup_file.as_ref().unwrap());
        assert!(backup_path.exists());
    }

    #[test]
    fn test_end_session_creates_manifest() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.txt"), "content").unwrap();

        let mut op = ShadowOperator::new(dir.path(), 5);
        let session_id = op.start_session();
        op.backup_before_write("test.txt");
        op.end_session();

        let manifest_path = dir.path()
            .join(SHADOW_ROOT)
            .join(&session_id)
            .join("manifest.json");
        assert!(manifest_path.exists());

        let manifest: ShadowManifest = serde_json::from_str(
            &fs::read_to_string(&manifest_path).unwrap()
        ).unwrap();
        assert_eq!(manifest.session_id, session_id);
        assert_eq!(manifest.operations.len(), 1);
    }

    #[test]
    fn test_list_sessions_empty() {
        let dir = tempdir().unwrap();
        let op = ShadowOperator::new(dir.path(), 5);
        let sessions = op.list_sessions();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_list_sessions_with_sessions() {
        let dir = tempdir().unwrap();
        let mut op = ShadowOperator::new(dir.path(), 5);

        // Create a session
        let session_id = op.start_session();
        op.end_session();

        let sessions = op.list_sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, session_id);
    }

    #[test]
    fn test_undo_last_session_no_sessions() {
        let dir = tempdir().unwrap();
        let op = ShadowOperator::new(dir.path(), 5);

        let result = op.undo_last_session();
        assert_eq!(result.session_id, "none");
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_undo_last_session_restores_file() {
        let dir = tempdir().unwrap();
        let test_file = dir.path().join("file.txt");
        fs::write(&test_file, "original").unwrap();

        let mut op = ShadowOperator::new(dir.path(), 5);
        op.start_session();
        op.backup_before_write("file.txt");
        op.end_session();

        // Modify the file after backup
        fs::write(&test_file, "modified").unwrap();
        assert_eq!(fs::read_to_string(&test_file).unwrap(), "modified");

        // Undo should restore original
        let result = op.undo_last_session();
        assert!(result.errors.is_empty());
        assert!(result.restored.contains(&"file.txt".to_string()));
        assert_eq!(fs::read_to_string(&test_file).unwrap(), "original");
    }

    #[test]
    fn test_undo_last_session_deletes_new_file() {
        let dir = tempdir().unwrap();

        let mut op = ShadowOperator::new(dir.path(), 5);
        op.start_session();
        op.backup_before_write("new_file.txt"); // Doesn't exist yet
        op.end_session();

        // Create the file after recording the backup
        let test_file = dir.path().join("new_file.txt");
        fs::write(&test_file, "new content").unwrap();
        assert!(test_file.exists());

        // Undo should delete it
        let result = op.undo_last_session();
        assert!(result.deleted_new.contains(&"new_file.txt".to_string()));
        assert!(!test_file.exists());
    }

    #[test]
    fn test_undo_last_session_recreates_deleted_file() {
        let dir = tempdir().unwrap();
        let test_file = dir.path().join("deleted.txt");
        fs::write(&test_file, "will be deleted").unwrap();

        let mut op = ShadowOperator::new(dir.path(), 5);
        op.start_session();
        op.backup_before_delete("deleted.txt");
        op.end_session();

        // Actually delete the file
        fs::remove_file(&test_file).unwrap();
        assert!(!test_file.exists());

        // Undo should recreate it
        let result = op.undo_last_session();
        assert!(result.recreated.contains(&"deleted.txt".to_string()));
        assert!(test_file.exists());
        assert_eq!(fs::read_to_string(&test_file).unwrap(), "will be deleted");
    }

    #[test]
    fn test_prune_old_sessions() {
        let dir = tempdir().unwrap();
        let mut op = ShadowOperator::new(dir.path(), 5);

        // Create a session
        op.start_session();
        op.end_session();

        // Should have 1 session
        let sessions = op.list_sessions();
        assert_eq!(sessions.len(), 1);

        // Verify max_sessions is set correctly
        assert_eq!(op.max_sessions, 5);
    }

    #[test]
    fn test_session_summary_fields() {
        let summary = SessionSummary {
            session_id: "session_2025".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            operation_count: 5,
        };
        assert_eq!(summary.session_id, "session_2025");
        assert_eq!(summary.operation_count, 5);
    }

    #[test]
    fn test_shadow_operation_clone() {
        let op = ShadowOperation {
            op_type: "WRITE".into(),
            path: "test.txt".into(),
            backup_file: Some("backup".into()),
            existed: true,
            skipped: false,
        };
        let cloned = op.clone();
        assert_eq!(cloned.op_type, op.op_type);
        assert_eq!(cloned.path, op.path);
    }

    #[test]
    fn test_undo_result_structure() {
        let result = UndoResult {
            session_id: "sess_123".into(),
            restored: vec!["a.txt".into()],
            deleted_new: vec!["b.txt".into()],
            recreated: vec!["c.txt".into()],
            errors: vec![],
        };
        assert_eq!(result.restored.len(), 1);
        assert_eq!(result.deleted_new.len(), 1);
        assert_eq!(result.recreated.len(), 1);
        assert!(result.errors.is_empty());
    }
}
