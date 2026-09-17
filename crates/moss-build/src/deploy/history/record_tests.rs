//! `deploy::landed`'s one integration test
//! (`deploy/landed_tests.rs::a_landed_publish_snapshots_into_publish_history`)
//! drives this through `record_landed` end to end; everything here calls
//! this module's own functions directly, under a tempdir root, so nothing
//! ever touches the real app-data directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::*;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::SourceMetadata;
use crate::deploy::history::store::count_blob_files_for_tests;
use crate::types::content::{parse_entry, symlink_entry, SiteHashes, MODE_FILE};

const OID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn never_evicted(_: &Path) -> bool {
    false
}

// ── record_one: dedup, ceiling, eviction, oid mismatch, symlinks ─────────

#[test]
fn dedup_skips_a_hash_already_in_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(vault.join("post.md"), b"fresh bytes this path must never hash").unwrap();

    let store = ObjectStore::new(dir.path().join("objects"));
    let pre_existing_hash = store.store_bytes(b"already kept").unwrap();

    let mut out = BTreeMap::new();
    record_one(&vault, &store, "post.md", &pre_existing_hash, Some(4), &never_evicted, &mut out);

    assert_eq!(out["post.md"].hash, pre_existing_hash);
    assert_eq!(
        count_blob_files_for_tests(store.root()),
        1,
        "a hash already in the store must not trigger a second store_file"
    );
}

#[test]
fn oversize_files_are_recorded_by_hash_only() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(vault.join("video.mp4"), b"a small fixture standing in for a huge file").unwrap();
    let store = ObjectStore::new(dir.path().join("objects"));

    let mut out = BTreeMap::new();
    let big = store::HISTORY_MEDIA_CEILING + 1;
    record_one(&vault, &store, "video.mp4", "deadbeef", Some(big), &never_evicted, &mut out);

    assert_eq!(
        out["video.mp4"],
        Entry { mode: MODE_FILE.to_string(), hash: "deadbeef".to_string(), size: Some(big) }
    );
    assert_eq!(
        count_blob_files_for_tests(store.root()),
        0,
        "an entry over the ceiling must not be copied into the store"
    );
}

#[test]
fn evicted_files_are_skipped_without_storing() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    // Readable on disk — the point is that the injected predicate, not a real
    // iCloud eviction, is what the skip goes through.
    fs::write(vault.join("evicted.bin"), b"pretend this is cloud-only").unwrap();
    let store = ObjectStore::new(dir.path().join("objects"));
    let looks_evicted = |p: &Path| p.file_name().is_some_and(|n| n == "evicted.bin");

    let mut out = BTreeMap::new();
    record_one(&vault, &store, "evicted.bin", "abc123", Some(9), &looks_evicted, &mut out);

    assert_eq!(out["evicted.bin"].hash, "abc123");
    assert_eq!(
        count_blob_files_for_tests(store.root()),
        0,
        "an evicted source must never be waited on or copied"
    );
}

#[test]
fn a_mismatched_oid_keeps_the_real_blob_and_the_manifest_hash() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let real_bytes = b"the bytes actually on disk now";
    fs::write(vault.join("stale.md"), real_bytes).unwrap();
    let store = ObjectStore::new(dir.path().join("objects"));

    // A parse-cache-skipped page (or an mtime-preserving sync client) can hand
    // the manifest a hash from a previous build that the current bytes no
    // longer match.
    let mut out = BTreeMap::new();
    record_one(&vault, &store, "stale.md", "0ld-hash-carried-forward", Some(4), &never_evicted, &mut out);

    let entry = &out["stale.md"];
    assert_eq!(
        entry.hash, "0ld-hash-carried-forward",
        "the record keeps the manifest's hash, never the oid store_file actually found"
    );
    let real_hash = format!("{:x}", sha2::Sha256::digest(real_bytes));
    assert!(store.get_path(&real_hash).is_some(), "the real bytes must still be kept, under their own hash");
}

#[cfg(unix)]
#[test]
fn a_symlink_round_trips_through_its_target_string() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let target = "resources/cities-heat-map-app";
    std::os::unix::fs::symlink(target, vault.join("cities-heat-map")).unwrap();
    let store = ObjectStore::new(dir.path().join("objects"));

    let mut out = BTreeMap::new();
    record_one(&vault, &store, "cities-heat-map", "irrelevant-manifest-hash", None, &never_evicted, &mut out);

    let entry = &out["cities-heat-map"];
    let expected_entry = symlink_entry(target);
    let (expected_mode, expected_hash) = parse_entry(&expected_entry);
    assert_eq!(entry.mode, expected_mode, "should be the deploy manifest's own symlink mode tag");
    assert_eq!(entry.hash, expected_hash, "should follow types::content::symlink_entry's own hash convention");
    assert_eq!(entry.size, None);

    let blob = store.get_path(&entry.hash).expect("the target string must be kept as a blob");
    assert_eq!(fs::read(blob).unwrap(), target.as_bytes());
}

// ── git_head ──────────────────────────────────────────────────────────────

#[test]
fn git_head_resolves_a_directory_repo_via_a_loose_ref() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path();
    let git = vault.join(".git");
    fs::create_dir_all(git.join("refs/heads")).unwrap();
    fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(git.join("refs/heads/main"), format!("{OID_A}\n")).unwrap();

    assert_eq!(resolve_git_head(vault), Some(OID_A.to_string()));
}

#[test]
fn git_head_resolves_through_a_gitdir_pointer_file() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let real_git = dir.path().join("real.git");
    fs::create_dir_all(&vault).unwrap();
    fs::create_dir_all(real_git.join("refs/heads")).unwrap();
    fs::write(vault.join(".git"), "gitdir: ../real.git\n").unwrap();
    fs::write(real_git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(real_git.join("refs/heads/main"), format!("{OID_A}\n")).unwrap();

    assert_eq!(resolve_git_head(&vault), Some(OID_A.to_string()));
}

#[test]
fn git_head_falls_back_to_packed_refs_when_no_loose_ref_exists() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path();
    let git = vault.join(".git");
    fs::create_dir_all(&git).unwrap();
    fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(
        git.join("packed-refs"),
        format!("# pack-refs with: peeled fully-peeled sorted\n{OID_B} refs/heads/main\n"),
    )
    .unwrap();

    assert_eq!(resolve_git_head(vault), Some(OID_B.to_string()));
}

#[test]
fn git_head_reads_a_detached_head_directly() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path();
    let git = vault.join(".git");
    fs::create_dir_all(&git).unwrap();
    fs::write(git.join("HEAD"), format!("{OID_A}\n")).unwrap();

    assert_eq!(resolve_git_head(vault), Some(OID_A.to_string()));
}

#[test]
fn git_head_resolves_a_linked_worktree_through_a_loose_ref_in_commondir() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let main_git = dir.path().join("main.git");
    let wt_gitdir = main_git.join("worktrees").join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::create_dir_all(&wt_gitdir).unwrap();
    fs::create_dir_all(main_git.join("refs/heads")).unwrap();
    fs::write(vault.join(".git"), format!("gitdir: {}\n", wt_gitdir.display())).unwrap();
    fs::write(wt_gitdir.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
    fs::write(wt_gitdir.join("commondir"), "../..\n").unwrap();
    // The everyday state of an active branch: a loose ref in the MAIN
    // repo's refs/heads, never in the worktree's own private gitdir.
    fs::write(main_git.join("refs/heads/feature"), format!("{OID_A}\n")).unwrap();

    assert_eq!(resolve_git_head(&vault), Some(OID_A.to_string()));
}

#[test]
fn git_head_resolves_a_linked_worktree_through_packed_refs_in_commondir() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    let main_git = dir.path().join("main.git");
    let wt_gitdir = main_git.join("worktrees").join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::create_dir_all(&wt_gitdir).unwrap();
    fs::write(vault.join(".git"), format!("gitdir: {}\n", wt_gitdir.display())).unwrap();
    fs::write(wt_gitdir.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
    // No loose ref anywhere — the branch was packed, so the common dir's
    // packed-refs is the only place left to look.
    fs::write(wt_gitdir.join("commondir"), "../..\n").unwrap();
    fs::write(main_git.join("packed-refs"), format!("{OID_B} refs/heads/feature\n")).unwrap();

    assert_eq!(resolve_git_head(&vault), Some(OID_B.to_string()));
}

#[test]
fn git_head_is_none_when_there_is_no_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(resolve_git_head(dir.path()), None);
}

// ── the shared page-set helper ───────────────────────────────────────────

fn manifest_with_a_page_and_an_asset() -> SealedManifest {
    let mut pending = PendingManifest::new(SiteHashes::default());

    let sp = ServedPath::from_source("hello/index.html").unwrap();
    pending.register(&sp, b"<p>hi</p>", HashBucket::Files);
    pending.register_source_mapping("hello.md".to_string(), &sp);
    pending.register_page_source_hash(
        "hello.md".to_string(),
        SourceMetadata { hash: "page-hash".to_string(), size: 5, mtime: 1, mtime_nanos: None, ctime: None, inode: None },
    );

    // An asset: present in `sources()` with no `source_to_output()` mapping,
    // the same shape the deferred media walk produces.
    pending.register_page_source_hash(
        "assets/logo.png".to_string(),
        SourceMetadata { hash: "asset-hash".to_string(), size: 9, mtime: 1, mtime_nanos: None, ctime: None, inode: None },
    );

    pending.seal()
}

#[test]
fn page_source_hashes_agrees_with_from_sealed() {
    let sealed = manifest_with_a_page_and_an_asset();

    let via_helper = crate::build::manifest::change_set::page_source_hashes(&sealed);
    let via_from_sealed = crate::build::manifest::change_set::PublishedSnapshot::from_sealed(
        &sealed,
        "moss:test",
        "2026-01-01T00:00:00Z".to_string(),
    );

    assert_eq!(via_helper, via_from_sealed.sources);
    assert_eq!(via_helper.get("hello.md"), Some(&"page-hash".to_string()));
    assert!(!via_helper.contains_key("assets/logo.png"), "the asset must not be read as a page");
}

#[test]
fn current_entry_hashes_includes_pages_and_assets() {
    let sealed = manifest_with_a_page_and_an_asset();
    let hashes = current_entry_hashes(&sealed);

    assert_eq!(hashes.get("hello.md"), Some(&"page-hash".to_string()));
    assert_eq!(hashes.get("assets/logo.png"), Some(&"asset-hash".to_string()), "the asset must still be comparable");
}

#[test]
fn build_entries_includes_the_asset_half_of_sources() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(vault.join("assets")).unwrap();
    fs::write(vault.join("hello.md"), b"hello").unwrap();
    fs::write(vault.join("assets/logo.png"), b"logo bytes").unwrap();
    let store = ObjectStore::new(dir.path().join("objects"));

    let sealed = manifest_with_a_page_and_an_asset();
    let entries = build_entries(&vault, &sealed, &store, &never_evicted);

    assert!(entries.contains_key("hello.md"), "the page must be recorded");
    assert!(entries.contains_key("assets/logo.png"), "the asset must be recorded alongside the page");
}

// ── config gate, record shape, and the new fields ─────────────────────────

#[test]
fn history_disabled_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(vault.join(".moss")).unwrap();
    fs::write(vault.join(".moss/config.toml"), "[history]\nenabled = false\n").unwrap();
    let history_root = dir.path().join("history");

    let sealed = manifest_with_a_page_and_an_asset();
    snapshot(&history_root, &vault, &sealed, "moss:test", "2026-01-01T00:00:00Z").unwrap();

    assert!(!history_root.exists(), "a disabled site must leave no trace under the history root");
}

#[test]
fn snapshot_writes_a_well_formed_record() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(vault.join("hello.md"), b"hello").unwrap();
    let history_root = dir.path().join("history");

    let sealed = manifest_with_a_page_and_an_asset();
    snapshot(&history_root, &vault, &sealed, "moss:test", "2026-01-01T08:12:33Z").unwrap();

    // `snapshot` takes `history_root` as the site's own resolved directory
    // directly — a fixed vault-relative path, with no key to derive.
    let record_path = history_root
        .join("publishes")
        .join(format!("2026-01-01T08-12-33Z-{}.json", sealed.generation_id()));
    let record: PublishRecord = serde_json::from_str(&fs::read_to_string(&record_path).unwrap()).unwrap();
    assert_eq!(record.version, 1);
    assert_eq!(record.target, "moss:test");
    assert_eq!(record.trigger, Trigger::Publish, "an ordinary landed publish defaults to Publish");
    assert_eq!(record.label, None);
    assert_eq!(record.entries["hello.md"].mode, MODE_FILE);
}

/// A v1 record on disk predates `trigger`/`label` and must still parse, with
/// `trigger` reading as `Publish` — the whole point of `#[serde(default)]`
/// on both fields.
#[test]
fn a_v1_record_without_the_new_fields_still_parses() {
    let json = r#"{
        "version": 1,
        "published_at": "2026-01-01T00:00:00Z",
        "target": "moss:test",
        "generation_id": "gen-1",
        "entries": { "a.md": { "mode": "100644", "hash": "h-a" } }
    }"#;

    let record: PublishRecord = serde_json::from_str(json).unwrap();
    assert_eq!(record.trigger, Trigger::Publish);
    assert_eq!(record.label, None);
    assert_eq!(record.git_head, None);
    assert_eq!(record.entries["a.md"].size, None);
}

#[test]
fn a_record_with_the_new_fields_round_trips() {
    let mut entries = BTreeMap::new();
    entries.insert("a.md".to_string(), Entry { mode: MODE_FILE.to_string(), hash: "h-a".to_string(), size: Some(3) });
    let record = PublishRecord {
        version: 1,
        published_at: "2026-01-01T00:00:00Z".to_string(),
        target: "moss:test".to_string(),
        generation_id: "gen-1".to_string(),
        git_head: Some(OID_A.to_string()),
        trigger: Trigger::Manual,
        label: Some("Before rewriting the intro".to_string()),
        entries,
    };

    let json = serde_json::to_string(&record).unwrap();
    let round_tripped: PublishRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(round_tripped, record);
}

#[test]
fn format_date_reads_as_a_short_human_date() {
    assert_eq!(format_date("2026-09-08T11:20:00Z"), "Sep 8");
    assert_eq!(format_date("2026-01-01T00:00:00Z"), "Jan 1");
}

#[test]
fn format_date_falls_back_to_the_raw_string_when_it_cannot_parse() {
    assert_eq!(format_date("not-a-date"), "not-a-date");
}

#[test]
fn snapshot_manual_writes_a_manual_record_with_its_label() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(vault.join("hello.md"), b"hello").unwrap();
    let history_root = dir.path().join("history");
    let sealed = manifest_with_a_page_and_an_asset();

    snapshot_manual(&history_root, &vault, &sealed, "moss:test", Some("Before rewriting the intro".to_string()))
        .unwrap();

    let records = store::list_records(&history_root);
    assert_eq!(records.len(), 1);
    let (_, record) = &records[0];
    assert_eq!(record.trigger, Trigger::Manual);
    assert_eq!(record.label.as_deref(), Some("Before rewriting the intro"));
}

// ── save_before_restore ────────────────────────────────────────────────────

#[test]
fn save_before_restore_writes_nothing_when_current_matches_the_newest_record() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(vault.join("hello.md"), b"hello").unwrap();
    let history_root = dir.path().join("history");
    let sealed = manifest_with_a_page_and_an_asset();

    snapshot(&history_root, &vault, &sealed, "moss:test", "2026-01-01T00:00:00Z").unwrap();
    let (restoring_id, _) = &store::list_records(&history_root)[0];

    // The tree has not changed since that publish: restoring it is a no-op
    // for undo purposes, so no pre-restore record should appear.
    save_before_restore(&history_root, &vault, &sealed, "moss:test", restoring_id).unwrap();

    assert_eq!(store::list_records(&history_root).len(), 1, "no redundant pre-restore record");
}

#[test]
fn save_before_restore_saves_when_the_current_tree_differs() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    fs::write(vault.join("hello.md"), b"hello").unwrap();
    let history_root = dir.path().join("history");
    let sealed = manifest_with_a_page_and_an_asset();
    snapshot(&history_root, &vault, &sealed, "moss:test", "2026-01-01T00:00:00Z").unwrap();
    let (restoring_id, restoring_record) = store::list_records(&history_root)[0].clone();

    // The author has since edited the page: current entries no longer match
    // the newest record.
    fs::write(vault.join("hello.md"), b"hello, edited").unwrap();
    let mut pending = PendingManifest::new(SiteHashes::default());
    let sp = ServedPath::from_source("hello/index.html").unwrap();
    pending.register(&sp, b"<p>hi edited</p>", HashBucket::Files);
    pending.register_source_mapping("hello.md".to_string(), &sp);
    pending.register_page_source_hash(
        "hello.md".to_string(),
        SourceMetadata { hash: "page-hash-2".to_string(), size: 13, mtime: 2, mtime_nanos: None, ctime: None, inode: None },
    );
    let current = pending.seal();

    save_before_restore(&history_root, &vault, &current, "moss:test", &restoring_id).unwrap();

    let records = store::list_records(&history_root);
    assert_eq!(records.len(), 2, "the pre-restore save must be written");
    let pre_restore = records.iter().find(|(id, _)| id != &restoring_id).unwrap();
    assert_eq!(pre_restore.1.trigger, Trigger::Restore);
    let expected_label = format!("Before restoring {}", format_date(&restoring_record.published_at));
    assert_eq!(pre_restore.1.label.as_deref(), Some(expected_label.as_str()));
}
