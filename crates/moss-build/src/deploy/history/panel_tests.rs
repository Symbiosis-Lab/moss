//! Tests for the Versions command bodies: the pure row mappings, the two
//! rules the bodies add on top of the store (a caller-supplied version id is
//! checked against the records the store lists; the read path never asks for a
//! sealed manifest), and the trash wiring a site restore hands down.
//!
//! What is deliberately NOT here: a second copy of the store's own coverage.
//! `record_tests`, `restore_tests`, `timeline_tests` and `store_tests` already
//! prove snapshotting, restoring and folding; these tests only exercise what
//! this file decides.

use std::path::{Path, PathBuf};

use sha2::Digest;

use super::*;
use crate::build::manifest::{HashBucket, PendingManifest};
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

fn ready(manifest: SealedManifest) -> impl std::future::Future<Output = Result<SealedManifest, String>> {
    async move { Ok(manifest) }
}

/// A sealed-manifest provider that fails if it is ever awaited — the shape of
/// "this path must not build".
fn never() -> impl std::future::Future<Output = Result<SealedManifest, String>> {
    async { Err::<SealedManifest, String>("this path must not ask for a sealed manifest".to_string()) }
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

/// Opening the Versions panel reads records off disk and nothing else. This is
/// what lets the HTTP carrier answer a list without building the site, so it
/// is asserted rather than assumed: the provider here fails if awaited, and
/// both list shapes still return.
#[tokio::test]
async fn neither_timeline_asks_for_a_sealed_manifest() {
    let (_dir, vault) = tmp_vault();
    std::fs::write(vault.join("a.md"), b"hello").unwrap();
    HistoryStore::in_vault(&vault)
        .snapshot(&vault, &manifest_of(&[("a.md", b"hello")]), "moss", "2026-01-01T00:00:00Z")
        .unwrap();

    let site = list_versions(&vault, "site", None, never()).await.unwrap();
    assert_eq!(site.rows.len(), 1, "the site timeline must answer from records alone");

    let page = list_versions(&vault, "page", Some("a.md".to_string()), never()).await.unwrap();
    assert_eq!(page.rows.len(), 1, "a page timeline must answer from records alone");
}

/// A version id is joined into a filename by the store, so an id the store
/// does not list must be refused before it reaches one. Over Tauri IPC the id
/// is always a row moss listed; over the HTTP carrier it is whatever the
/// request body said, and `../` would otherwise escape `publishes/` and
/// restore a file the caller planted.
#[tokio::test]
async fn a_version_id_the_store_does_not_list_never_reaches_the_filesystem() {
    let (_dir, vault) = tmp_vault();
    std::fs::write(vault.join("a.md"), b"present").unwrap();
    let store = HistoryStore::in_vault(&vault);
    store
        .snapshot(&vault, &manifest_of(&[("a.md", b"present")]), "moss", "2026-01-01T00:00:00Z")
        .unwrap();

    // The escape target: a real, loadable record one directory above
    // `publishes/`, whose stored bytes differ from what is on disk now.
    let (real_id, mut planted) = store.list_records().pop().unwrap();
    planted.published_at = "2020-01-01T00:00:00Z".to_string();
    let outside = store.store_dir().join("planted.json");
    std::fs::write(&outside, serde_json::to_vec(&planted).unwrap()).unwrap();
    assert!(outside.exists(), "the escape target must exist for this to mean anything");

    for escape in ["../planted", &format!("../publishes/{real_id}")] {
        let err = restore_version(&vault, escape, None, "in_place", ready(manifest_of(&[])))
            .await
            .expect_err("an unlisted id must be refused");
        assert!(err.contains("no such version"), "got: {err}");
    }
    let err = read_version(&vault, "../planted", "a.md").expect_err("unlisted id");
    assert!(err.contains("no such version"), "got: {err}");

    let err = list_versions(&vault, "site", Some("../planted".to_string()), never())
        .await
        .expect_err("unlisted id");
    assert!(err.contains("no such version"), "got: {err}");

    // Nothing was written back and no pre-restore record was left behind.
    assert_eq!(std::fs::read(vault.join("a.md")).unwrap(), b"present");
    assert_eq!(store.list_records().len(), 1, "a refused restore writes no backup record");
}

/// A site restore trashes what the site has gained since the version, and the
/// paths it hands the trash closure are vault-relative — so the closure has to
/// join them onto the root before the delete core's containment check sees
/// them. `restore::real_trash` does; a closure that forwarded the relative path
/// unchanged (which the app's command used to pass) failed every such page and
/// reported it under `failed` instead of `trashed`.
///
/// `since.md` is absent from disk on purpose: the delete core treats an
/// already-gone path as the goal state, so the join is exercised without this
/// test moving a real file into the developer's OS Trash.
#[tokio::test]
async fn a_site_restore_trashes_pages_added_since_the_version() {
    let (_dir, vault) = tmp_vault();
    std::fs::write(vault.join("a.md"), b"hello").unwrap();
    let store = HistoryStore::in_vault(&vault);
    store
        .snapshot(&vault, &manifest_of(&[("a.md", b"hello")]), "moss", "2026-01-01T00:00:00Z")
        .unwrap();
    let (id, _) = store.list_records().pop().unwrap();

    let now = manifest_of(&[("a.md", b"hello"), ("since.md", b"new page")]);
    let report = restore_version(&vault, &id, None, "in_place", ready(now)).await.unwrap();

    assert_eq!(report.trashed, vec!["since.md".to_string()], "failed: {:?}", report.failed);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
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
