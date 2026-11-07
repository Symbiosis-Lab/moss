//! Tests for the Versions command bodies: the pure row mappings, the rules
//! the bodies add on top of the store (a caller-supplied version id is
//! checked against the records the store lists; saving and restoring read the
//! disk, never a build), and the trash wiring a site restore hands down.
//!
//! What is deliberately NOT here: a second copy of the store's own coverage.
//! `record_tests`, `restore_tests`, `timeline_tests` and `store_tests` already
//! prove snapshotting, restoring and folding; these tests only exercise what
//! this file decides.

use std::path::{Path, PathBuf};

use sha2::Digest;

use super::*;
use crate::build::manifest::{HashBucket, PendingManifest, SealedManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::SourceMetadata;
use crate::types::content::SiteHashes;

fn tmp_vault() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    (dir, vault)
}

/// A sealed manifest describing exactly the given source files, as though a
/// build had just rendered each one to `<stem>/index.html`.
fn manifest_of(pages: &[(&str, &[u8])]) -> SealedManifest {
    let mut pending = PendingManifest::new(SiteHashes::default());
    for (src, bytes) in pages {
        let out = format!("{}/index.html", src.trim_end_matches(".md"));
        let sp = ServedPath::from_source(&out).unwrap();
        pending.register(&sp, bytes, HashBucket::Files);
        pending.register_source_mapping(src.to_string(), &sp);
        pending.register_page_source_hash(
            src.to_string(),
            SourceMetadata {
                hash: format!("{:x}", sha2::Sha256::digest(bytes)),
                size: bytes.len() as u64,
                mtime: 1,
                mtime_nanos: None,
                ctime: None,
                inode: None,
            },
        );
    }
    pending.seal()
}

fn empty_record(target: &str, trigger: Trigger, label: Option<&str>) -> PublishRecord {
    PublishRecord {
        version: 1,
        published_at: "2026-09-10T08:12:33Z".to_string(),
        target: target.to_string(),
        generation_id: "gen1".to_string(),
        git_head: None,
        trigger,
        label: label.map(str::to_string),
        entries: Default::default(),
    }
}

// ── The row mappings ──────────────────────────────────────────────────────

#[test]
fn trigger_str_covers_every_variant_with_the_designs_own_words() {
    assert_eq!(trigger_str(Trigger::Publish), "publish");
    assert_eq!(trigger_str(Trigger::Manual), "manual");
    assert_eq!(trigger_str(Trigger::Restore), "restore");
    assert_eq!(trigger_str(Trigger::Displaced), "displaced");
}

#[test]
fn to_version_row_carries_every_field_through_unchanged() {
    let row = super::super::Row {
        id: "abc".to_string(),
        published_at: "2026-09-10T08:12:33Z".to_string(),
        target: "moss:test".to_string(),
        trigger: Trigger::Manual,
        label: Some("Before rewriting the intro".to_string()),
        changed: 3,
        added: 1,
        removed: 0,
    };
    let out = to_version_row(row);
    assert_eq!(out.id, "abc");
    assert_eq!(out.published_at, "2026-09-10T08:12:33Z");
    assert_eq!(out.target, "moss:test");
    assert_eq!(out.trigger, "manual");
    assert_eq!(out.label.as_deref(), Some("Before rewriting the intro"));
    assert_eq!((out.changed, out.added, out.removed), (3, 1, 0));
}

#[test]
fn version_row_from_record_has_no_prior_record_to_diff_so_every_count_is_zero() {
    let record = empty_record("moss:test", Trigger::Manual, Some("A checkpoint"));
    let out = version_row_from_record("2026-09-10T08-12-33Z-gen1".to_string(), &record);
    assert_eq!(out.id, "2026-09-10T08-12-33Z-gen1");
    assert_eq!(out.trigger, "manual");
    assert_eq!(out.label.as_deref(), Some("A checkpoint"));
    assert_eq!((out.changed, out.added, out.removed), (0, 0, 0));
}

#[test]
fn version_row_from_record_reads_a_publish_trigger_as_publish() {
    let record = empty_record("moss:test", Trigger::Publish, None);
    let out = version_row_from_record("id".to_string(), &record);
    assert_eq!(out.trigger, "publish");
    assert_eq!(out.label, None);
}

// ── What the bodies decide ────────────────────────────────────────────────

/// Saving, comparing and restoring work with no build at all: the folder has
/// never been built, so there is no manifest, and each answers from the disk.
#[test]
fn save_compare_and_restore_need_no_build() {
    let (_dir, vault) = tmp_vault();
    std::fs::write(vault.join("a.md"), b"first").unwrap();

    let saved = save_version(&vault, Some("First draft".to_string())).unwrap();
    std::fs::write(vault.join("a.md"), b"second").unwrap();

    let drill = list_versions(&vault, "site", Some(saved.id.clone())).unwrap();
    assert_eq!(drill.changed_since.len(), 1, "the edit since the save is seen without a build");
    assert_eq!(drill.changed_since[0].source_path, "a.md");

    let report = restore_version(&vault, &saved.id, Some("a.md".to_string()), "in_place").unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(std::fs::read(vault.join("a.md")).unwrap(), b"first");
    let undo = report.undo_id.expect("the edited page was saved before the restore");
    assert_eq!(read_version(&vault, &undo, "a.md").unwrap().text.as_deref(), Some("second"));
}

/// A version id is joined into a filename by the store, so an id the store
/// does not list must be refused before it reaches one. Over Tauri IPC the id
/// is always a row moss listed; over the HTTP carrier it is whatever the
/// request body said, and `../` would otherwise escape `publishes/` and
/// restore a file the caller planted.
#[test]
fn a_version_id_the_store_does_not_list_never_reaches_the_filesystem() {
    let (_dir, vault) = tmp_vault();
    std::fs::write(vault.join("a.md"), b"present").unwrap();
    let store = HistoryStore::in_vault(&vault);
    store
        .snapshot(&vault, &SourceTree::from_sealed(&manifest_of(&[("a.md", b"present")])), "moss", "2026-01-01T00:00:00Z")
        .unwrap();

    // The escape target: a real, loadable record one directory above
    // `publishes/`, whose stored bytes differ from what is on disk now.
    let (real_id, mut planted) = store.list_records().pop().unwrap();
    planted.published_at = "2020-01-01T00:00:00Z".to_string();
    let outside = store.store_dir().join("planted.json");
    std::fs::write(&outside, serde_json::to_vec(&planted).unwrap()).unwrap();
    assert!(outside.exists(), "the escape target must exist for this to mean anything");

    for escape in ["../planted", &format!("../publishes/{real_id}")] {
        let err = restore_version(&vault, escape, None, "in_place")
            .expect_err("an unlisted id must be refused");
        assert!(err.contains("no such version"), "got: {err}");
    }
    let err = read_version(&vault, "../planted", "a.md").expect_err("unlisted id");
    assert!(err.contains("no such version"), "got: {err}");

    let err = list_versions(&vault, "site", Some("../planted".to_string()))
        .expect_err("unlisted id");
    assert!(err.contains("no such version"), "got: {err}");

    // Nothing was written back and no pre-restore record was left behind.
    assert_eq!(std::fs::read(vault.join("a.md")).unwrap(), b"present");
    assert_eq!(store.list_records().len(), 1, "a refused restore writes no backup record");
}

/// A site restore trashes what the site has gained since the version, and the
/// paths it hands the trash closure are vault-relative, so the closure has to
/// join them onto the root before the delete core's containment check sees
/// them. `restore::real_trash` does; a closure that forwarded the relative path
/// unchanged (which the app's command used to pass) failed every such page and
/// reported it under `failed` instead of `trashed`.
///
/// The restore runs with a recording trash, so no real file reaches the
/// developer's OS Trash; the join itself is checked on an already-gone path,
/// which the delete core treats as the goal state.
#[test]
fn a_site_restore_trashes_pages_added_since_the_version() {
    let (_dir, vault) = tmp_vault();
    std::fs::write(vault.join("a.md"), b"hello").unwrap();
    let store = HistoryStore::in_vault(&vault);
    store
        .snapshot(&vault, &SourceTree::from_sealed(&manifest_of(&[("a.md", b"hello")])), "moss", "2026-01-01T00:00:00Z")
        .unwrap();
    let (id, _) = store.list_records().pop().unwrap();
    std::fs::write(vault.join("since.md"), b"new page").unwrap();

    let trashed = std::sync::Mutex::new(Vec::new());
    let trash = |_: &Path, rel: &str| -> Result<(), String> {
        trashed.lock().unwrap().push(rel.to_string());
        Ok(())
    };
    let report = restore_version_with(&vault, &id, None, "in_place", &trash).unwrap();

    assert_eq!(report.trashed, vec!["since.md".to_string()], "failed: {:?}", report.failed);
    assert_eq!(*trashed.lock().unwrap(), vec!["since.md".to_string()]);
    assert_eq!(restore::real_trash(&vault, "gone.md"), Ok(()), "the default trash joins the path onto the root");
}

/// A whole-site restore to a kept outside edit is refused before anything is
/// saved: no backup record, no Undo for a restore that did nothing.
#[test]
fn a_site_restore_to_a_kept_outside_edit_is_refused_before_any_backup() {
    let (_dir, vault) = tmp_vault();
    std::fs::write(vault.join("a.md"), b"mine").unwrap();
    let store = HistoryStore::in_vault(&vault);
    let id = store.keep_displaced(&vault, "a.md", b"theirs").unwrap().unwrap();

    let err = restore_version(&vault, &id, None, "in_place").unwrap_err();

    assert!(err.contains("kept outside edit"), "{err}");
    assert_eq!(store.list_records().len(), 1, "nothing but the kept edit itself");
}

/// A page restore answers the backup it wrote as `undo_id`, read from the
/// writer itself: a newer record of another kind is never mistaken for it.
#[test]
fn a_page_restore_names_its_own_backup_even_when_another_record_is_newer() {
    let (_dir, vault) = tmp_vault();
    std::fs::write(vault.join("a.md"), b"v2").unwrap();
    let store = HistoryStore::in_vault(&vault);
    store.snapshot(&vault, &SourceTree::from_sealed(&manifest_of(&[("a.md", b"v1")])), "moss", "2026-01-01T00:00:00Z").unwrap();
    let (id, _) = store.list_records().pop().unwrap();
    store.keep_displaced(&vault, "b.md", b"an agent's page").unwrap();

    let report = restore_version(&vault, &id, Some("a.md".to_string()), "in_place").unwrap();

    let undo = store.load_record(report.undo_id.as_deref().expect("a backup was written")).unwrap();
    assert_eq!(undo.trigger, Trigger::Restore);
}

/// `reveal_history_store` resolves the vault's own store directory, with no
/// caller-supplied path anywhere in it. Asserted on the path rather than by
/// spawning a file manager, which a test may not do.
#[test]
fn reveal_targets_the_vaults_own_history_store() {
    let (_dir, vault) = tmp_vault();
    assert_eq!(
        HistoryStore::in_vault(&vault).store_dir(),
        Path::new(&vault).join(".moss").join("history")
    );
}
