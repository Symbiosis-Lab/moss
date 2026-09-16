//! Publish history: a content-addressed copy of every source file a vault
//! published, kept per computer, outside the vault
//! ([ADR-083](../../../../../docs/decisions/ADR-083-publish-history-lives-outside-the-vault.md)),
//! under `<app-data>/history/<site-key>/` — never `.moss/`, so a synced vault
//! never uploads a version store a sync client can tear mid-write. `<site-key>`
//! is the vault's identity public key, or a path-hash fallback (see
//! [`store::site_key`]).
//!
//! Slice 1 — the design's ["How it lands"](../../../../../docs/archive/2026-09-11-publish-history-design.md)
//! item 1: the store ([`store`]), the record and its three writers
//! ([`record`]), page and site restore ([`restore`]), and the page/site
//! timeline folds ([`timeline`]). Slice 2 adds the `moss history` CLI verb
//! ([`cli`]) over the same facade; the UI (slice 3) has not landed —
//! everything here is reached by `deploy::landed::record_landed`, by
//! [`cli::run`], and by this module's own tests.
//!
//! [`HistoryStore::in_app_data`] resolves the real per-computer directory for
//! a vault (`None` only with no app-data directory — the one fallible step,
//! done once); [`HistoryStore::at`] is the test seam, an explicit directory
//! with no site-key derivation. Every other method then takes only its own
//! domain parameters — the same shape as `SecretStore::in_app_data`
//! (`identity/secrets.rs`): resolve the fallible root once, and nothing
//! downstream repeats that resolution or needs a parallel test-rooted twin.

pub mod cli;
pub(crate) mod record;
pub(crate) mod restore;
pub(crate) mod store;
pub(crate) mod timeline;

use std::path::{Path, PathBuf};

use crate::build::manifest::SealedManifest;

pub use record::{Entry, PublishRecord, Trigger};
pub use restore::{RestoreMode, RestoreReport};
pub use store::is_live;
pub use timeline::{Row, SiteVersionPages};

/// One site's publish history: the resolved directory (see the module doc)
/// plus the methods that read and write under it.
#[derive(Debug, Clone)]
pub struct HistoryStore {
    root: PathBuf,
}

impl HistoryStore {
    /// Test seam: an explicit site directory, no site-key derivation.
    pub(crate) fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// The real, per-computer store for `vault_root`'s site. `None` only
    /// when this platform has no application data directory — the same
    /// absence `record_landed`'s `history: Option<&HistoryStore>` models.
    ///
    /// `MOSS_HISTORY_DIR` replaces that root: it is the test suites' isolation
    /// seam (CLI e2e and the Linux app e2e), read here and nowhere else, so it
    /// reaches the history store and no other subsystem.
    pub fn in_app_data(vault_root: &Path) -> Option<Self> {
        let history_root = match std::env::var_os("MOSS_HISTORY_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => crate::infra::app_data::app_data_dir_early()?.join("history"),
        };
        Some(Self::at(store::site_dir(&history_root, vault_root)))
    }

    /// "Show in Finder" target. No UI here; this only resolves the path.
    pub fn store_dir(&self) -> &Path {
        &self.root
    }

    /// See [`store::list_records`].
    pub fn list_records(&self) -> Vec<(String, PublishRecord)> {
        store::list_records(&self.root)
    }

    /// See [`store::read_version`].
    pub fn read_version(&self, id: &str, path: &str) -> Result<Vec<u8>, String> {
        store::read_version(&self.root, id, path)
    }

    /// See [`record::snapshot`].
    pub fn snapshot(&self, vault_root: &Path, sealed: &SealedManifest, target: &str, published_at: &str) -> Result<(), String> {
        record::snapshot(&self.root, vault_root, sealed, target, published_at)
    }

    /// See [`record::snapshot_manual`].
    pub fn snapshot_manual(&self, vault_root: &Path, sealed: &SealedManifest, target: &str, label: Option<String>) -> Result<(), String> {
        record::snapshot_manual(&self.root, vault_root, sealed, target, label)
    }

    /// See [`record::save_before_restore`].
    pub fn save_before_restore(&self, vault_root: &Path, current: &SealedManifest, target: &str, restoring_id: &str) -> Result<(), String> {
        record::save_before_restore(&self.root, vault_root, current, target, restoring_id)
    }

    /// See [`restore::restore_page`].
    pub fn restore_page(&self, vault_root: &Path, id: &str, path: &str, mode: RestoreMode) -> Result<(), String> {
        restore::restore_page(&self.root, vault_root, id, path, mode)
    }

    /// See [`restore::restore_site`]. `trash` has no default: pass
    /// [`restore::real_trash`] in production, a fake in a test.
    pub fn restore_site(
        &self,
        vault_root: &Path,
        id: &str,
        current: &SealedManifest,
        trash: &dyn Fn(&Path, &str) -> Result<(), String>,
    ) -> RestoreReport {
        restore::restore_site(&self.root, vault_root, id, current, trash)
    }

    /// See [`timeline::page_timeline`].
    pub fn page_timeline(&self, path: &str) -> Vec<Row> {
        timeline::page_timeline(&self.root, path)
    }

    /// See [`timeline::site_timeline`].
    pub fn site_timeline(&self) -> Vec<Row> {
        timeline::site_timeline(&self.root)
    }

    /// See [`timeline::site_version_pages`].
    pub fn site_version_pages(&self, id: &str, current: &SealedManifest) -> Result<SiteVersionPages, String> {
        timeline::site_version_pages(&self.root, id, current)
    }
}

#[cfg(test)]
pub(crate) use store::count_blob_files_for_tests;

/// `HistoryStore`'s own methods, proven end to end — every module test below
/// this one exercises the free functions directly, which leaves the
/// delegation in this file (which `root`/`vault_root` argument goes where)
/// unchecked by anything but the type checker. A swapped argument here would
/// still compile.
#[cfg(test)]
mod tests {
    use sha2::Digest;

    use super::*;
    use crate::build::manifest::{HashBucket, PendingManifest};
    use crate::build::served_path::ServedPath;
    use crate::build::types::SourceMetadata;
    use crate::types::content::SiteHashes;

    fn one_page_manifest(src: &str, out: &str, bytes: &[u8]) -> SealedManifest {
        let mut pending = PendingManifest::new(SiteHashes::default());
        let sp = ServedPath::from_source(out).unwrap();
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
        pending.seal()
    }

    #[test]
    fn snapshot_list_records_and_read_version_round_trip_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::write(vault.join("a.md"), b"hello").unwrap();
        let store = HistoryStore::at(dir.path().join("history"));
        assert_eq!(store.store_dir(), dir.path().join("history"));

        let sealed = one_page_manifest("a.md", "a/index.html", b"hello");
        store.snapshot(&vault, &sealed, "moss:test", "2026-01-01T00:00:00Z").unwrap();

        let records = store.list_records();
        assert_eq!(records.len(), 1);
        let (id, record) = &records[0];
        assert_eq!(record.target, "moss:test");

        assert_eq!(store.read_version(id, "a.md").unwrap(), b"hello");
    }

    #[test]
    fn snapshot_manual_and_page_timeline_and_site_timeline_agree_with_the_written_record() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::write(vault.join("a.md"), b"hello").unwrap();
        let store = HistoryStore::at(dir.path().join("history"));
        let sealed = one_page_manifest("a.md", "a/index.html", b"hello");

        store.snapshot_manual(&vault, &sealed, "moss:test", Some("A checkpoint".to_string())).unwrap();

        let page_rows = store.page_timeline("a.md");
        assert_eq!(page_rows.len(), 1);
        assert_eq!(page_rows[0].label.as_deref(), Some("A checkpoint"));

        let site_rows = store.site_timeline();
        assert_eq!(site_rows.len(), 1);
        assert_eq!(site_rows[0].trigger, Trigger::Manual);
    }

    #[test]
    fn restore_page_and_restore_site_and_site_version_pages_delegate_to_the_same_root() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::write(vault.join("a.md"), b"original").unwrap();
        let store = HistoryStore::at(dir.path().join("history"));
        let sealed = one_page_manifest("a.md", "a/index.html", b"original");
        store.snapshot(&vault, &sealed, "moss:test", "2026-01-01T00:00:00Z").unwrap();
        let (id, _) = store.list_records().into_iter().next().unwrap();

        let pages = store.site_version_pages(&id, &sealed).unwrap();
        assert!(pages.changed_since.is_empty(), "nothing changed since the version just taken");
        assert_eq!(pages.unchanged, vec!["a.md".to_string()]);

        std::fs::write(vault.join("a.md"), b"edited").unwrap();
        store.restore_page(&vault, &id, "a.md", RestoreMode::InPlace).unwrap();
        assert_eq!(std::fs::read(vault.join("a.md")).unwrap(), b"original");

        std::fs::write(vault.join("a.md"), b"edited again").unwrap();
        let current = one_page_manifest("a.md", "a/index.html", b"edited again");
        let no_trash = |_: &Path, _: &str| -> Result<(), String> { Ok(()) };
        let report = store.restore_site(&vault, &id, &current, &no_trash);
        assert_eq!(report.restored, vec!["a.md".to_string()]);
        assert_eq!(std::fs::read(vault.join("a.md")).unwrap(), b"original");
    }
}
