//! The record itself, and the ways one gets written: a landed publish
//! ([`snapshot`]), an author-named save ([`snapshot_manual`]), the automatic
//! save a restore takes of the present before overwriting it
//! ([`save_before_restore`]), all three through [`write_snapshot`]; and one
//! page another writer left, kept when the editor saved over it
//! ([`snapshot_displaced`]).
//!
//! # What happens at each record
//!
//! One read of a [`SourceTree`]: the sealed build's sources for a publish, so
//! the deploy baseline and the history record name the same bytes, or the
//! disk as it stands for every other record. For every entry, [`record_one`]
//! decides by `lstat`: a symlink stores its target string; a regular file already in the
//! store, over [`super::store::HISTORY_MEDIA_CEILING`], or cloud-evicted is
//! recorded by hash without a new blob. Never fails a publish — every I/O
//! error here is caught, logged at debug, and answered by recording what the
//! manifest already knows (design rule 3: "unreadable is not absent").

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::store;
use super::tree::SourceTree;
use crate::build::cache::ObjectStore;
use crate::types::content::{MODE_FILE, MODE_SYMLINK};

/// What caused a record to be written. `#[serde(default)]` on the field that
/// carries this: a v1 record written before this field existed has no
/// `trigger` key at all, and must still read as `Publish` forever.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    #[default]
    Publish,
    Manual,
    Restore,
    /// One page as another writer left it, kept when the editor saved the
    /// person's text over lines that writer had changed. A one-page record:
    /// it is never a site version, so the site timeline and a site restore
    /// skip it.
    Displaced,
}

/// One landed publish, author-named save, or pre-restore save: every source
/// path that was live, with enough to find or explain its bytes. Serialized
/// to `<site-dir>/publishes/<published_at with ':' -> '-'>-<generation_id>.json`
/// — that filename stem is the record's id everywhere else in this module.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublishRecord {
    pub version: u32,
    /// RFC3339, the same instant `deploy::landed` stamped on the deploy
    /// baseline for this publish (or, for a manual/restore save, the moment
    /// the save ran).
    pub published_at: String,
    /// `<method>:<id>` — the same string `published_record::path_for` derives,
    /// so a history record and the deploy baseline for one publish correlate
    /// by name.
    pub target: String,
    pub generation_id: String,
    /// The vault's own git commit at landing time, or `None` when the vault
    /// has no readable git repository. Captured once, here, and never
    /// re-derived: an old record shows the commit that was live *then* (see
    /// [`resolve_git_head`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    /// Why this record exists. Missing on disk reads as `Publish` — see
    /// [`Trigger`]'s own doc.
    #[serde(default)]
    pub trigger: Trigger,
    /// The name the author typed for a manual save, or the automatic "Before
    /// restoring …" label a restore-triggered save carries. `None` for an
    /// ordinary publish.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub entries: BTreeMap<String, Entry>,
}

impl PublishRecord {
    /// Where this record was published, or `None` for a kept outside edit,
    /// which was published nowhere.
    pub fn published_target(&self) -> Option<&str> {
        (self.trigger != Trigger::Displaced).then_some(self.target.as_str())
    }
}

/// One source file as it stood at a publish. `mode` follows the deploy
/// manifest's own tags ([`MODE_FILE`], [`MODE_SYMLINK`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// The source timestamp paired with the manifest hash, never the publish time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_mtime: Option<(u64, u32)>,
    pub mode: String,
    pub hash: String,
    /// Absent for a symlink. For a regular file, absent only means this
    /// build's manifest did not carry a size — never "not kept": whether the
    /// bytes were kept is `get_path(hash).is_some()`, derived, not stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// Snapshot one landed publish into the real, app-local history store.
///
/// Config-gated (`[history].enabled`, default on) and best-effort by
/// contract, matching every other write `record_landed` makes: the caller
/// logs a failure and moves on, never fails the publish over it.
pub(crate) fn snapshot(
    root: &Path,
    vault_root: &Path,
    tree: &SourceTree,
    target: &str,
    published_at: &str,
) -> Result<Option<String>, String> {
    write_snapshot(root, vault_root, tree, target, published_at, Trigger::Publish, None)
}

/// Save a version now, outside the publish flow, of `tree` (the disk as it
/// stands: [`SourceTree::on_disk`]).
pub(crate) fn snapshot_manual(
    root: &Path,
    vault_root: &Path,
    tree: &SourceTree,
    target: &str,
    label: Option<String>,
) -> Result<Option<String>, String> {
    let published_at = chrono::Utc::now().to_rfc3339();
    write_snapshot(root, vault_root, tree, target, &published_at, Trigger::Manual, label)
}

/// Save the present as a version before a restore overwrites it, so the
/// restore is undoable — but only when there is something to undo: if the
/// current tree already matches the newest record for `target`, a second,
/// identical save would cost a JSON file for nothing (the blobs dedup
/// regardless).
///
/// `restoring_id` is the version *being restored to* — a different record
/// from "the newest one", and the one whose date the automatic label names.
/// Answers the id of the record it wrote, or `None` when it wrote none.
pub(crate) fn save_before_restore(
    root: &Path,
    vault_root: &Path,
    current: &SourceTree,
    target: &str,
    restoring_id: &str,
) -> Result<Option<String>, String> {
    let restoring =
        store::load_record(root, restoring_id).ok_or_else(|| format!("no such version: {restoring_id}"))?;

    let newest_for_target = store::list_records(root).into_iter().rev().find(|(_, r)| r.target == target);
    let current_hashes = current.hashes();
    // No prior record for this target is not a real case a restore can reach
    // (you can only restore a record that already exists), but treat it as
    // "differs" rather than panic or skip: the save is still cheap and
    // correct with nothing to compare against.
    let differs = match &newest_for_target {
        None => true,
        Some((_, newest)) => current_hashes != flat_hashes(newest),
    };
    if !differs {
        return Ok(None);
    }

    let label = Some(format!("Before restoring {}", format_date(&restoring.published_at)));
    let published_at = chrono::Utc::now().to_rfc3339();
    write_snapshot(root, vault_root, current, target, &published_at, Trigger::Restore, label)
}

/// `PublishRecord.entries` as a plain path→hash map — the shape
/// [`crate::build::manifest::change_set::diff_hashes`] wants, and what a
/// flat record already is a `BTreeMap` of.
pub(crate) fn flat_hashes(record: &PublishRecord) -> HashMap<String, String> {
    record.entries.iter().map(|(path, entry)| (path.clone(), entry.hash.clone())).collect()
}

/// "Before restoring Sep 8" — a short human date for the automatic
/// pre-restore label. Falls back to the raw timestamp if `published_at`
/// somehow fails to parse, which never happens for a record this module
/// wrote itself but keeps the label honest about a hand-edited or foreign one.
pub(crate) fn format_date(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339).map(|dt| dt.format("%b %-d").to_string()).unwrap_or_else(|_| rfc3339.to_string())
}

/// The shared writer behind [`snapshot`], [`snapshot_manual`] and
/// [`save_before_restore`]: build entries from real bytes on disk and write
/// one record, answering its id (`None` with history off). The only
/// difference between the three callers is which `Trigger`/`label` the record
/// carries. `root` is the resolved site directory ([`super::HistoryStore`]'s
/// own field) — never re-derived from an app-data root here.
fn write_snapshot(
    root: &Path,
    vault_root: &Path,
    tree: &SourceTree,
    target: &str,
    published_at: &str,
    trigger: Trigger,
    label: Option<String>,
) -> Result<Option<String>, String> {
    if !store::history_enabled(vault_root) {
        return Ok(None);
    }

    let object_store = store::object_store(root);
    let entries = build_entries(vault_root, tree, &object_store, &crate::build::icloud::is_evicted);
    let record = PublishRecord {
        version: 1,
        published_at: published_at.to_string(),
        target: target.to_string(),
        generation_id: tree.generation_id().to_string(),
        git_head: resolve_git_head(vault_root),
        trigger,
        label,
        entries,
    };
    write_record(root, &record)?;
    Ok(Some(record_id(&record)))
}

/// Keep `bytes`, the whole of `path` as another writer left it, as a
/// one-page `Displaced` record, and answer its id. The caller is about to
/// write over those bytes, so this stores them from memory rather than
/// re-reading a file that may already be changing. Unlike the snapshots above
/// it is not best-effort: a caller that cannot keep the bytes must not write.
pub(crate) fn snapshot_displaced(root: &Path, path: &str, bytes: &[u8]) -> Result<String, String> {
    let hash = store::object_store(root).store_bytes(bytes, crate::build::cache::RecordMode::Wait)?;
    let mut entries = BTreeMap::new();
    entries.insert(
        path.to_string(),
        Entry { source_mtime: None, mode: MODE_FILE.to_string(), hash: hash.clone(), size: Some(bytes.len() as u64) },
    );
    let record = PublishRecord {
        version: 1,
        published_at: chrono::Utc::now().to_rfc3339(),
        // Published nowhere, so it matches no deploy target.
        target: String::new(),
        generation_id: format!("displaced-{}", &hash[..12.min(hash.len())]),
        git_head: None,
        trigger: Trigger::Displaced,
        label: None,
        entries,
    };
    write_record(root, &record)?;
    Ok(record_id(&record))
}

fn record_id(record: &PublishRecord) -> String {
    format!("{}-{}", record.published_at.replace(':', "-"), record.generation_id)
}

fn write_record(root: &Path, record: &PublishRecord) -> Result<(), String> {
    let filename = format!("{}.json", record_id(record));
    crate::infra::atomic_write::write_json_atomic(&root.join("publishes").join(filename), record)
}

/// Every entry of `tree`, each resolved against the real file now.
pub(crate) fn build_entries(
    vault_root: &Path,
    tree: &SourceTree,
    store: &ObjectStore,
    is_evicted: &dyn Fn(&Path) -> bool,
) -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    for (src, meta) in tree.sources() {
        record_one(vault_root, store, src, &meta.hash, Some(meta.size), is_evicted, &mut out);
    }
    for (src, entry) in &mut out {
        entry.source_mtime = tree.sources().get(src)
            .and_then(|m| m.mtime_nanos.map(|n| (m.mtime, n)));
    }
    out
}

/// Resolve and best-effort store one entry. Never propagates an error: a
/// file this cannot keep is still worth a record entry naming what the
/// manifest says it was (design rule 3, "unreadable is not absent").
///
/// `is_evicted` is a parameter rather than a direct call to
/// `build::icloud::is_evicted` so a test can simulate an evicted path
/// without a real cloud-managed file (macOS's `SF_DATALESS` flag is set by
/// the file provider extension, not by anything a test can `chflags`).
pub(crate) fn record_one(
    vault_root: &Path,
    store: &ObjectStore,
    src: &str,
    manifest_hash: &str,
    manifest_size: Option<u64>,
    is_evicted: &dyn Fn(&Path) -> bool,
    out: &mut BTreeMap<String, Entry>,
) {
    let full = vault_root.join(src);

    if let Ok(meta) = std::fs::symlink_metadata(&full) {
        if meta.file_type().is_symlink() {
            if let Ok(target) = std::fs::read_link(&full) {
                let target_str = target.to_string_lossy().into_owned();
                // The same hash `types::content::symlink_entry` embeds in the
                // deploy manifest's `120000:<hash>` tag — one convention for
                // "what a symlink's identity is", read here rather than
                // re-derived from `store_bytes`'s own oid, though the two are
                // definitionally the same sha256.
                let hash = format!("{:x}", Sha256::digest(target_str.as_bytes()));
                if let Err(e) = store.store_bytes(target_str.as_bytes(), crate::build::cache::RecordMode::Wait) {
                    log::debug!("history: could not store the symlink blob for {src}: {e}");
                }
                out.insert(src.to_string(), Entry { source_mtime: None, mode: MODE_SYMLINK.to_string(), hash, size: None });
                return;
            }
        }
    }

    // Regular file — or an lstat/read_link this could not resolve, for which
    // the manifest's own hash is still the honest answer to "what was
    // published".
    let oversize = manifest_size.is_some_and(|s| s > store::HISTORY_MEDIA_CEILING);
    let already_kept = store.holds(manifest_hash);
    if !already_kept && !oversize && !is_evicted(&full) {
        match store.store_file(&full, crate::build::cache::RecordMode::Wait) {
            Ok(oid) if oid != manifest_hash => {
                // A page the parse cache skipped carries its previous hash
                // forward, and an mtime-preserving sync client can deliver new
                // bytes under an unchanged size and mtime — either way the
                // blob just stored is real and kept under ITS hash; the
                // record keeps the manifest's, and the mismatch is not an
                // error to attribute (design, "What happens at each
                // publish").
                log::debug!(
                    "history: stored blob {oid} differs from the manifest hash {manifest_hash} for {src}"
                );
            }
            Ok(_) => {}
            Err(e) => log::debug!("history: could not store a blob for {src}: {e}"),
        }
    }
    out.insert(
        src.to_string(),
        Entry { source_mtime: None, mode: MODE_FILE.to_string(), hash: manifest_hash.to_string(), size: manifest_size },
    );
}

// ---------------------------------------------------------------------------
// git_head — plain file reads, never the git binary (moss reads a
// vault's .git and never writes it)
// ---------------------------------------------------------------------------

fn is_hex_oid(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The vault's git commit at landing time, or `None` on anything missing,
/// evicted or malformed. Resolution is plain file reads: a `.git` directory
/// or a `gitdir:` pointer file, a symbolic or detached `HEAD`, a loose ref or
/// `packed-refs`, `commondir` for a linked worktree.
pub(crate) fn resolve_git_head(vault_root: &Path) -> Option<String> {
    let gitdir = resolve_gitdir(&vault_root.join(".git"))?;
    let head = read_git_file(&gitdir.join("HEAD"))?;
    let head = head.trim();

    if let Some(refname) = head.strip_prefix("ref:") {
        let refname = refname.trim();
        // A branch ref is shared state: git keeps `refs/heads/*` in the
        // COMMON dir, never in a linked worktree's own private gitdir, so
        // `gitdir.join(refname)` is never where the everyday, active-branch
        // case lives. `commondir` is that shared directory (defaulting to
        // `gitdir` itself when there is no `commondir` file — the ordinary,
        // non-worktree repo, where the two are the same directory anyway).
        let commondir = resolve_commondir(&gitdir).unwrap_or_else(|| gitdir.clone());
        if let Some(oid) = read_git_file(&commondir.join(refname)) {
            let oid = oid.trim();
            if is_hex_oid(oid) {
                return Some(oid.to_string());
            }
        }
        let packed = read_git_file(&commondir.join("packed-refs"))?;
        return find_in_packed_refs(&packed, refname);
    }

    is_hex_oid(head).then(|| head.to_string())
}

/// `.git` as either a repo directory or a `gitdir: <path>` pointer file (a
/// linked worktree's own `.git`). A relative pointer path resolves against
/// `.git`'s own parent — the vault root — matching git's convention.
fn resolve_gitdir(dot_git: &Path) -> Option<PathBuf> {
    let meta = std::fs::symlink_metadata(dot_git).ok()?;
    if meta.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    let text = read_git_file(dot_git)?;
    let path_str = text.trim().strip_prefix("gitdir:")?.trim();
    let path = PathBuf::from(path_str);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(dot_git.parent()?.join(path))
    }
}

/// A linked worktree's `commondir` file, resolved relative to `gitdir` — the
/// path git itself writes it relative to.
fn resolve_commondir(gitdir: &Path) -> Option<PathBuf> {
    let text = read_git_file(&gitdir.join("commondir"))?;
    let path = PathBuf::from(text.trim());
    Some(if path.is_absolute() { path } else { gitdir.join(path) })
}

fn find_in_packed_refs(text: &str, refname: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
            return None;
        }
        let (oid, name) = line.split_once(' ')?;
        (name == refname && is_hex_oid(oid)).then(|| oid.to_string())
    })
}

/// `is_evicted` before every open; missing, evicted or unreadable all fold to
/// `None` alike — never waited on, never a download trigger. Not
/// `cloud_readiness::read_input_if_present`: it distinguishes absent from
/// offline by materializing first, and resolving a vault's git commit must
/// never wait on a download.
fn read_git_file(path: &Path) -> Option<String> {
    if crate::build::icloud::is_evicted(path) {
        return None;
    }
    // allow:raw_read is_evicted probed above; any failure here (missing, raced eviction, malformed) folds to None by design, never a wait
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
