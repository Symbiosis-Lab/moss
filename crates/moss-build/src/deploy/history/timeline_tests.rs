use std::fs;
use std::path::Path;

use sha2::Digest;

use super::*;
use crate::build::manifest::change_set::PageVerb;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::build::types::SourceMetadata;
use crate::types::content::SiteHashes;

fn hash_of(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

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

fn publish(history_root: &Path, vault: &Path, pages: &[(&str, &str, &[u8])], published_at: &str) {
    let mut pending = PendingManifest::new(SiteHashes::default());
    for (src, out, bytes) in pages {
        seed_page(vault, &mut pending, src, out, bytes);
    }
    let sealed = pending.seal();
    record::snapshot(history_root, vault, &sealed, "moss:test", published_at).unwrap();
}

// ── page_timeline ──────────────────────────────────────────────────────────

#[test]
fn page_timeline_includes_a_publish_only_when_the_page_s_hash_moves() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    // R1: a.md appears (differs from "nothing before").
    publish(&history_root, &vault, &[("a.md", "a/index.html", b"v1")], "2026-01-01T00:00:00Z");
    // R2: a.md unchanged, b.md appears — a.md must not show up here.
    publish(
        &history_root,
        &vault,
        &[("a.md", "a/index.html", b"v1"), ("b.md", "b/index.html", b"v1")],
        "2026-01-02T00:00:00Z",
    );
    // R3: a.md edited.
    publish(
        &history_root,
        &vault,
        &[("a.md", "a/index.html", b"v2"), ("b.md", "b/index.html", b"v1")],
        "2026-01-03T00:00:00Z",
    );
    // R4: a.md deleted (disappears).
    publish(&history_root, &vault, &[("b.md", "b/index.html", b"v1")], "2026-01-04T00:00:00Z");

    let rows = page_timeline(&history_root, "a.md");

    assert_eq!(rows.len(), 3, "R2 must be absent — a.md did not change there");
    assert_eq!(rows[0].published_at, "2026-01-01T00:00:00Z");
    assert_eq!(rows[1].published_at, "2026-01-03T00:00:00Z");
    assert_eq!(rows[2].published_at, "2026-01-04T00:00:00Z", "disappearing counts as differing");
}

#[test]
fn page_timeline_always_includes_manual_and_restore_records() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    publish(&history_root, &vault, &[("a.md", "a/index.html", b"v1")], "2026-01-01T00:00:00Z");

    let mut pending = PendingManifest::new(SiteHashes::default());
    seed_page(&vault, &mut pending, "a.md", "a/index.html", b"v1"); // unchanged
    let sealed = pending.seal();
    record::snapshot_manual(&history_root, &vault, &sealed, "moss:test", Some("A checkpoint".to_string())).unwrap();

    // A manual save of an UNRELATED page — a.md did not change, and this
    // save's subject (if it had one) is not a.md, yet it must still appear.
    let rows = page_timeline(&history_root, "a.md");
    assert_eq!(rows.len(), 2, "the manual save must appear even though a.md is unchanged there");
    assert!(rows[1].label.as_deref() == Some("A checkpoint"));
}

// ── site_timeline ──────────────────────────────────────────────────────────

#[test]
fn site_timeline_counts_changed_added_and_removed_against_the_previous_record() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    publish(
        &history_root,
        &vault,
        &[("a.md", "a/index.html", b"v1"), ("b.md", "b/index.html", b"v1")],
        "2026-01-01T00:00:00Z",
    );
    // a.md edited, b.md removed, c.md added.
    publish(&history_root, &vault, &[("a.md", "a/index.html", b"v2"), ("c.md", "c/index.html", b"v1")], "2026-01-02T00:00:00Z");

    let rows = site_timeline(&history_root);

    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].changed, rows[0].added, rows[0].removed), (0, 2, 0), "the first record has nothing to diff against");
    assert_eq!((rows[1].changed, rows[1].added, rows[1].removed), (1, 1, 1));
}

// ── site_version_pages ───────────────────────────────────────────────────

#[test]
fn site_version_pages_reports_edited_added_and_deleted_since_and_the_unchanged_rest() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");

    publish(
        &history_root,
        &vault,
        &[
            ("keep.md", "keep/index.html", b"keep"),
            ("edit.md", "edit/index.html", b"before"),
            ("gone.md", "gone/index.html", b"gone"),
        ],
        "2026-01-01T00:00:00Z",
    );
    let (id, _) = store::list_records(&history_root)[0].clone();

    let mut pending = PendingManifest::new(SiteHashes::default());
    seed_page(&vault, &mut pending, "keep.md", "keep/index.html", b"keep");
    seed_page(&vault, &mut pending, "edit.md", "edit/index.html", b"after");
    seed_page(&vault, &mut pending, "new.md", "new/index.html", b"new");
    let current = pending.seal();

    let result = site_version_pages(&history_root, &id, &current).unwrap();

    let mut changed = result.changed_since.clone();
    changed.sort_by(|a, b| a.source_path.cmp(&b.source_path));
    assert_eq!(
        changed,
        vec![
            crate::build::manifest::change_set::ChangedPage { source_path: "edit.md".to_string(), verb: PageVerb::Edited },
            crate::build::manifest::change_set::ChangedPage { source_path: "gone.md".to_string(), verb: PageVerb::Deleted },
            crate::build::manifest::change_set::ChangedPage { source_path: "new.md".to_string(), verb: PageVerb::Added },
        ]
    );
    assert_eq!(result.unchanged, vec!["keep.md".to_string()]);
}

#[test]
fn site_version_pages_errors_for_an_unknown_id() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir_all(&vault).unwrap();
    let history_root = dir.path().join("history");
    let current = PendingManifest::new(SiteHashes::default()).seal();

    let err = site_version_pages(&history_root, "no-such-id", &current).unwrap_err();
    assert!(err.contains("no such version"), "{err}");
}
