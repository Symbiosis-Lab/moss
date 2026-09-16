use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

use sha2::Digest;

use super::*;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::SourceMetadata;
use crate::types::content::{MODE_FILE, SiteHashes};

fn hash_of(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// Write `bytes` to `vault/src`, and register it as a page in `pending` under
/// its own real hash — so the blob `record::snapshot_into` stores is
/// reachable under the exact hash the record keeps, the way a real build's
/// manifest and its on-disk bytes always agree.
fn seed_page(vault: &Path, pending: &mut PendingManifest, src: &str, out: &str, bytes: &[u8]) {
    fs::write(vault.join(src), bytes).unwrap();
    let sp = ServedPath::from_source(out).unwrap();
    pending.register(&sp, bytes, HashBucket::Files);
    pending.register_source_mapping(src.to_string(), &sp);
    pending.register_page_source_hash(
        src.to_string(),
        SourceMetadata { hash: hash_of(bytes), size: bytes.len() as u64, mtime: 1, mtime_nanos: None, ctime: None, inode: None },
    );
}

/// A record built by hand rather than through `record::snapshot_into`, for
/// the one case that needs an entry whose blob was never stored (an
/// oversize file) without actually writing 20MB to a fixture.
fn write_record_directly(root: &Path, published_at: &str, generation_id: &str, entries: BTreeMap<String, Entry>) -> String {
    let record = record::PublishRecord {
        version: 1,
        published_at: published_at.to_string(),
        target: "moss:test".to_string(),
        generation_id: generation_id.to_string(),
        git_head: None,
        trigger: record::Trigger::Publish,
        label: None,
        entries,
    };
    let filename = format!("{}-{generation_id}.json", published_at.replace(':', "-"));
    crate::infra::atomic_write::write_json_atomic(&root.join("publishes").join(filename), &record).unwrap();
    format!("{}-{generation_id}", published_at.replace(':', "-"))
}

// ── restore_page ───────────────────────────────────────────────────────────

#[test]
fn restore_page_writes_the_old_bytes_back_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    let mut pending = PendingManifest::new(SiteHashes::default());
    seed_page(&vault, &mut pending, "a.md", "a/index.html", b"original");
    let sealed = pending.seal();
    record::snapshot(&history_root, &vault, &sealed, "moss:test", "2026-01-01T00:00:00Z").unwrap();
    let (id, _) = store::list_records(&history_root)[0].clone();

    fs::write(vault.join("a.md"), b"edited since").unwrap();

    restore_page(&history_root, &vault, &id, "a.md", RestoreMode::InPlace).unwrap();

    assert_eq!(fs::read(vault.join("a.md")).unwrap(), b"original");
}

#[test]
fn restore_page_as_copy_leaves_the_present_file_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    let mut pending = PendingManifest::new(SiteHashes::default());
    seed_page(&vault, &mut pending, "a.md", "a/index.html", b"original");
    let sealed = pending.seal();
    record::snapshot(&history_root, &vault, &sealed, "moss:test", "2026-01-01T00:00:00Z").unwrap();
    let (id, rec) = store::list_records(&history_root)[0].clone();

    fs::write(vault.join("a.md"), b"edited since").unwrap();

    restore_page(&history_root, &vault, &id, "a.md", RestoreMode::AsCopy).unwrap();

    assert_eq!(fs::read(vault.join("a.md")).unwrap(), b"edited since", "the present file must be untouched");
    let sibling_path = vault.join(format!("a ({}).md", record::format_date(&rec.published_at)));
    assert_eq!(fs::read(&sibling_path).unwrap(), b"original");
}

#[test]
fn restore_page_names_the_reason_when_the_content_was_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    let mut entries = BTreeMap::new();
    entries.insert(
        "huge.mp4".to_string(),
        Entry { mode: MODE_FILE.to_string(), hash: "deadbeef".to_string(), size: Some(store::HISTORY_MEDIA_CEILING + 1) },
    );
    let id = write_record_directly(&history_root, "2026-01-01T00:00:00Z", "gen-1", entries);

    let err = restore_page(&history_root, &vault, &id, "huge.mp4", RestoreMode::InPlace).unwrap_err();
    assert!(err.contains("not kept"), "{err}");
    assert!(err.contains("ceiling"), "{err}");
}

#[test]
fn restore_page_errors_for_a_path_absent_from_the_version() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");
    let id = write_record_directly(&history_root, "2026-01-01T00:00:00Z", "gen-1", BTreeMap::new());

    let err = restore_page(&history_root, &vault, &id, "nope.md", RestoreMode::InPlace).unwrap_err();
    assert!(err.contains("not part of this version"), "{err}");
}

#[cfg(unix)]
#[test]
fn restore_page_recreates_a_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    let target = "resources/app";
    std::os::unix::fs::symlink(target, vault.join("link")).unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());
    // Not a page mapping — just enough for `build_entries`'s asset half to
    // see the path; `record_one`'s lstat check does the rest.
    pending.register_page_source_hash(
        "link".to_string(),
        SourceMetadata { hash: "unused".to_string(), size: 0, mtime: 1, mtime_nanos: None, ctime: None, inode: None },
    );
    let sealed = pending.seal();
    record::snapshot(&history_root, &vault, &sealed, "moss:test", "2026-01-01T00:00:00Z").unwrap();
    let (id, _) = store::list_records(&history_root)[0].clone();

    fs::remove_file(vault.join("link")).unwrap();
    assert!(std::fs::symlink_metadata(vault.join("link")).is_err());

    restore_page(&history_root, &vault, &id, "link", RestoreMode::InPlace).unwrap();

    let restored = std::fs::read_link(vault.join("link")).unwrap();
    assert_eq!(restored, std::path::PathBuf::from(target));
}

// ── restore_site ───────────────────────────────────────────────────────────

#[test]
fn restore_site_restores_edited_and_deleted_and_trashes_added() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    let mut pending = PendingManifest::new(SiteHashes::default());
    seed_page(&vault, &mut pending, "keep.md", "keep/index.html", b"keep body");
    seed_page(&vault, &mut pending, "edit.md", "edit/index.html", b"original body");
    seed_page(&vault, &mut pending, "gone.md", "gone/index.html", b"about to vanish");
    let old = pending.seal();
    record::snapshot(&history_root, &vault, &old, "moss:test", "2026-01-01T00:00:00Z").unwrap();
    let (id, _) = store::list_records(&history_root)[0].clone();

    fs::write(vault.join("edit.md"), b"edited body").unwrap();
    fs::remove_file(vault.join("gone.md")).unwrap();
    fs::write(vault.join("new.md"), b"brand new").unwrap();

    let mut cur_pending = PendingManifest::new(SiteHashes::default());
    seed_page(&vault, &mut cur_pending, "keep.md", "keep/index.html", b"keep body");
    seed_page(&vault, &mut cur_pending, "edit.md", "edit/index.html", b"edited body");
    seed_page(&vault, &mut cur_pending, "new.md", "new/index.html", b"brand new");
    let current = cur_pending.seal();

    let trashed_calls: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let fake_trash = |_vault_root: &Path, path: &str| -> Result<(), String> {
        trashed_calls.lock().unwrap().push(path.to_string());
        Ok(())
    };

    let report = restore_site(&history_root, &vault, &id, &current, &fake_trash);

    assert_eq!(report.restored, vec!["edit.md".to_string(), "gone.md".to_string()]);
    assert_eq!(report.trashed, vec!["new.md".to_string()]);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(fs::read(vault.join("edit.md")).unwrap(), b"original body");
    assert_eq!(fs::read(vault.join("gone.md")).unwrap(), b"about to vanish", "the deleted-since page must come back");
    assert_eq!(trashed_calls.into_inner().unwrap(), vec!["new.md".to_string()]);
}

#[test]
fn restore_site_leaves_an_unchanged_page_alone() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    let mut pending = PendingManifest::new(SiteHashes::default());
    seed_page(&vault, &mut pending, "keep.md", "keep/index.html", b"keep body");
    let old = pending.seal();
    record::snapshot(&history_root, &vault, &old, "moss:test", "2026-01-01T00:00:00Z").unwrap();
    let (id, _) = store::list_records(&history_root)[0].clone();

    let no_trash = |_: &Path, path: &str| -> Result<(), String> {
        panic!("nothing should be trashed, got {path}");
    };
    let report = restore_site(&history_root, &vault, &id, &old, &no_trash);

    assert!(report.restored.is_empty());
    assert!(report.trashed.is_empty());
    assert!(report.failed.is_empty());
}

#[test]
fn restore_site_reports_a_failure_without_stopping_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    let edit_hash = hash_of(b"original body");
    let mut entries = BTreeMap::new();
    entries.insert("edit.md".to_string(), Entry { mode: MODE_FILE.to_string(), hash: edit_hash.clone(), size: Some(13) });
    entries.insert(
        "huge.mp4".to_string(),
        Entry { mode: MODE_FILE.to_string(), hash: "deadbeef".to_string(), size: Some(store::HISTORY_MEDIA_CEILING + 1) },
    );
    // Store the one blob that IS kept, matching edit.md's hash exactly.
    let object_store = store::object_store(&history_root);
    fs::write(vault.join("source-for-blob"), b"original body").unwrap();
    let oid = object_store.store_file(&vault.join("source-for-blob")).unwrap();
    assert_eq!(oid, edit_hash, "the fixture bytes must hash to the entry's own hash");
    let id = write_record_directly(&history_root, "2026-01-01T00:00:00Z", "gen-1", entries);

    fs::write(vault.join("edit.md"), b"edited body").unwrap();
    let mut cur_pending = PendingManifest::new(SiteHashes::default());
    seed_page(&vault, &mut cur_pending, "edit.md", "edit/index.html", b"edited body");
    let current = cur_pending.seal();

    let no_trash = |_: &Path, _: &str| -> Result<(), String> { Ok(()) };
    let report = restore_site(&history_root, &vault, &id, &current, &no_trash);

    assert_eq!(report.restored, vec!["edit.md".to_string()]);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, "huge.mp4");
    assert!(report.failed[0].1.contains("not kept"), "{}", report.failed[0].1);
    assert_eq!(fs::read(vault.join("edit.md")).unwrap(), b"original body");
}
