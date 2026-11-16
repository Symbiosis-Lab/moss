//! A save asks the folder's rebuild worker for its build.

use super::rename_undo::RenameUndoStore;
use super::tests::{ok, pages};
use super::{dispatch, Change, CommandHost};
use crate::editor::filesystem::PublishDateMemo;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A host that hands a save to the folder's worker, as the serve host does,
/// and keeps each path it heard was written with its text at that moment.
struct SavingHost {
    root: PathBuf,
    undo: RenameUndoStore,
    dates: PublishDateMemo,
    heard: std::sync::Mutex<Vec<(PathBuf, String)>>,
}

impl CommandHost for SavingHost {
    fn root(&self) -> &Path {
        &self.root
    }
    fn rename_undo(&self) -> &RenameUndoStore {
        &self.undo
    }
    fn publish_dates(&self) -> &PublishDateMemo {
        &self.dates
    }
    fn served_dir(&self) -> PathBuf {
        self.root.join(".moss")
    }
    fn publish(&self, _: &crate::types::events::MossEvent) {}
    fn changed(&self, change: Change<'_>) {
        if let Change::Wrote(path) = change {
            self.heard.lock().unwrap().push((path.to_path_buf(), std::fs::read_to_string(path).unwrap()));
        }
        crate::ops::watch::hand_off(&self.root, change);
    }
}

/// A save asks the folder's rebuild worker for its own build once its bytes
/// are on disk, through the watcher's content-hash gate: the slot holds a
/// gated content-only request for the page. The host's folder is a symlink,
/// so the worker is found under the canonical folder it is registered by.
#[cfg(unix)]
#[tokio::test]
async fn a_save_asks_the_folders_worker_for_a_gated_rebuild_of_the_page() {
    let (_d, root) = pages();
    let link = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(&root, link.path().join("site")).unwrap();
    let host = SavingHost { root: link.path().join("site"), undo: Default::default(), dates: Default::default(), heard: Default::default() };
    let folder = root.to_string_lossy().into_owned();
    let worker = crate::ops::watch::worker::register(&folder);
    let parsed: Value = ok(dispatch(&host, "parse_frontmatter", json!({ "filePath": "a.md" })).await).await;
    let args = json!({ "filePath": "a.md", "frontmatter": { "title": "B" }, "body": "new body", "expectedHash": parsed["hash"] });
    let saved: Value = ok(dispatch(&host, "save_editor_content", args).await).await;
    let held = worker.take();
    crate::ops::watch::worker::deregister(&folder, &worker);
    assert_eq!(saved["status"], json!("saved"), "{saved}");
    let page = vec![root.join("a.md")];
    assert_eq!(held.map(|r| (r.trigger, r.gate_paths)), Some((crate::build::BuildTrigger::ContentOnly(page.clone()), Some(page.clone()))));
    let heard = host.heard.lock().unwrap().clone();
    assert!(heard.len() == 1 && heard[0].0 == page[0], "the host hears of the save once, by its canonical path: {heard:?}");
    assert!(heard[0].1.contains("new body"), "the host hears of the save after the write: {heard:?}");
}
