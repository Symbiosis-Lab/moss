use std::path::{Path, PathBuf};

use futures::future::BoxFuture;
use serde_json::{json, Value};

use super::rename_undo::{RenameUndoStore, RENAME_UNDO_CAPACITY};
use super::*;
use crate::editor::rename_plan::RenameApplyResult;

/// A representative body for each command, in the shape moss's UI sends.
/// Each is parsed with the command's own argument type, so a renamed or
/// retyped argument fails here before it fails a caller.
fn samples() -> Vec<(&'static str, Value)> {
    vec![
        ("scan_shortcodes", json!({ "text": "# hi" })),
        ("parse_frontmatter", json!({ "filePath": "a.md" })),
        ("editor_resolve_asset", json!({ "target": "i.png", "fromFile": "a.md" })),
        (
            "editor_resolve_references",
            json!({ "targets": [], "fromFile": "a.md" }),
        ),
        ("editor_bootstrap", json!({ "sourcePath": null })),
        (
            "list_directory",
            json!({ "path": "posts", "projectPath": "/ignored", "showInternal": false }),
        ),
        ("list_tree", json!({ "path": "/ignored", "showInternal": true })),
        ("get_file_info", json!({ "filePath": "a.md" })),
        (
            "compute_heading_state",
            json!({ "filePath": "a.md", "frontmatter": {}, "body": "" }),
        ),
        ("describe_source_role", json!({ "filePath": "a.md" })),
        ("resolve_url_for_file", json!({ "filePath": "a.md" })),
        ("validate_content", json!({ "filePath": "a.md" })),
        ("get_publish_preflight", json!({ "folder": "/ignored" })),
        ("list_vault_terms", json!({})),
        ("get_cloud_unavailable_files", json!({})),
        (
            "list_versions",
            json!({ "projectPath": "/ignored", "scope": "site", "path": null }),
        ),
        ("read_version", json!({ "id": "v1", "path": "a.md" })),
        ("reveal_history_store", json!({ "projectPath": "/ignored" })),
        ("resolve_attachment_dir", json!({ "pageRelativePath": "posts/a.md" })),
        ("resolve_page_source", json!({ "urlPath": "/posts/a/" })),
        (
            "create_files",
            json!({ "files": [{ "dir": "posts", "name": "a.md", "frontmatter": {} }] }),
        ),
        ("create_folder", json!({ "parentDir": "posts", "name": "new" })),
        ("delete_entry", json!({ "path": "a.md" })),
        ("delete_entries", json!({ "paths": ["a.md"] })),
        ("rename_entry_with_refs", json!({ "oldPath": "a.md", "newPath": "b.md" })),
        ("scan_references_for_delete", json!({ "path": "a.md" })),
        ("clean_references_and_delete", json!({ "paths": ["a.md"] })),
        ("undo_rename", json!({ "undoId": "id" })),
        (
            "save_editor_content",
            json!({ "filePath": "a.md", "frontmatter": {}, "body": "", "expectedHash": "h", "keepDisplaced": false }),
        ),
        (
            "restore_version",
            json!({ "projectPath": "/ignored", "id": "v1", "path": null, "mode": "page" }),
        ),
        ("save_version", json!({ "projectPath": "/ignored", "label": null })),
    ]
}

#[test]
fn every_command_parses_the_json_its_callers_send() {
    let samples = samples();
    for (cmd, body) in &samples {
        let parsed = parse_public_read(cmd, body.clone())
            .or_else(|| parse_session_read(cmd, body.clone()))
            .or_else(|| parse_write(cmd, body.clone()))
            .unwrap_or_else(|| panic!("{cmd} is in no tier"));
        parsed.unwrap_or_else(|e| panic!("{cmd} refused {body}: {e}"));
    }
    let sampled: Vec<&str> = samples.iter().map(|(cmd, _)| *cmd).collect();
    for cmd in PUBLIC_READS.iter().chain(SESSION_READS).chain(WRITES) {
        assert!(sampled.contains(cmd), "{cmd} has no wire sample here");
    }
}

/// A command is in exactly one tier, so a transport that maps tiers to trust
/// classes cannot expose a write as a read.
#[test]
fn the_tiers_are_disjoint() {
    let all: Vec<&str> = PUBLIC_READS.iter().chain(SESSION_READS).chain(WRITES).copied().collect();
    for (i, cmd) in all.iter().enumerate() {
        assert!(!all[i + 1..].contains(cmd), "{cmd} is in two tiers");
    }
}

struct TestHost {
    root: PathBuf,
    undo: RenameUndoStore,
}

impl CommandHost for TestHost {
    fn root(&self) -> &Path {
        &self.root
    }
    fn resolve(&self, path: &VaultPath) -> Result<PathBuf, String> {
        Ok(self.root.join(path.as_written()))
    }
    fn rename_undo(&self) -> &RenameUndoStore {
        &self.undo
    }
    fn served_dir(&self) -> PathBuf {
        self.root.join(".moss")
    }
    fn publish(&self, _: &MossEvent) {}
    fn sealed_now(&self) -> BoxFuture<'_, Result<SealedManifest, String>> {
        Box::pin(async { Err("no build in tests".to_string()) })
    }
}

#[test]
fn the_undo_store_keeps_only_the_newest_renames() {
    let store = RenameUndoStore::default();
    let ids: Vec<String> = (0..=RENAME_UNDO_CAPACITY)
        .map(|_| store.summarize_and_store(Path::new("/f"), RenameApplyResult::default()).undo_id)
        .collect();
    assert!(store.take(&ids[0]).is_none(), "the oldest is dropped past capacity");
    assert!(store.take(&ids[RENAME_UNDO_CAPACITY]).is_some());
}

/// An id minted in one folder does not undo anything once the host is in another.
#[tokio::test]
async fn undo_refuses_a_rename_from_another_folder() {
    let host = TestHost { root: PathBuf::from("/now/open"), undo: RenameUndoStore::default() };
    let id = host.undo.summarize_and_store(Path::new("/was/open"), RenameApplyResult::default()).undo_id;
    match dispatch_write(&host, "undo_rename", json!({ "undoId": id })).await {
        Some(Err(CommandError::Failed(msg))) => assert!(msg.contains("folder has changed"), "{msg}"),
        other => panic!("expected the folder check to refuse, got {other:?}"),
    }
}
