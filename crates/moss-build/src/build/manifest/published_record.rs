//! Persistence for [`PublishedSnapshot`] — `.moss/deploy/records/<key>.json`.
//!
//! # One record per target
//!
//! A project can publish to more than one place — moss hosting and an onion are
//! the shipped pair — and each target has its own live tree, so each needs its
//! own baseline. They were one file until 2026-08-17, which meant publishing to
//! the second target destroyed the first's record: switch back, and moss said
//! "no machine record" about a publish it remembered perfectly well. The record
//! is keyed by the whole target, not by its method, so two moss-hosted sites
//! from one folder are as separate as two hosts.
//!
//! **The file name is an index; the `target` inside the file is the authority.**
//! [`load_for`] re-checks it, so a key collision or a future change to
//! [`key_for`] degrades the change set instead of diffing against another
//! target's tree.
//!
//! # Why not `build::io_utils`
//!
//! `io_utils` exists for `.moss/build.nosync/**`, where the "regenerable output:
//! dataless is absent" rule applies — moss always holds the replacement bytes,
//! so it may truncate freely. This record is the opposite: it is durable and
//! cannot be regenerated from the project. Losing it costs a
//! degraded change set until the next publish lands. So it goes through
//! `infra::atomic_write`, the one writer in the tree that replaces a file:
//! a uniquely named sibling temp, fsync, then rename.
//!
//! # Why a read swallows errors
//!
//! Every failure mode — absent file, truncated JSON from a crash mid-write, a
//! schema change — means the same thing: there is nothing trustworthy to diff
//! against. That is degraded mode, which is a designed state with its own
//! honest surface, not an error. Propagating would push a decision the caller
//! can only re-make the same way.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::change_set::PublishedSnapshot;
use crate::moss_paths::MossPaths;

/// The record file for `target` — `.moss/deploy/records/<method>-<digest>.json`.
///
/// A target is a slot from `crate::config::deployment::slot_for` —
/// `moss:<site_id>` or a bare plugin method. The `:` (and whatever a legacy
/// `<method>:<url>` target carried) is why the name is a legible method plus
/// a digest of the whole thing rather than the target itself.
pub fn path_for(paths: &MossPaths, target: &str) -> PathBuf {
    paths.deploy_records_dir().join(format!("{}.json", key_for(target)))
}

/// The last publish this machine confirmed **to the target the site publishes
/// to now**, or `None` if there isn't a trustworthy one. `None` is degraded
/// mode, never an error.
///
/// `target` is `<method>:<id>` — see [`PublishedSnapshot::target`]. `None`
/// means the project has no configured target to contradict a record (a fresh
/// folder, or one whose config names a method but no id yet), so the sole
/// record on disk answers; with two there is no sole record and picking one
/// would be a guess about where the next publish goes.
pub fn load_for(paths: &MossPaths, target: Option<&str>) -> Option<PublishedSnapshot> {
    let Some(target) = target else { return sole_record(paths) };
    read(&path_for(paths, target))
        .or_else(|| same_method_record(paths, target))
        .or_else(|| read(&legacy_path(paths)))
        .filter(|snap| snap.describes_target(target))
}

/// A record written under the pre-slot key for this target's method.
///
/// Plugin records used to be keyed `<method>:<site_url>`, so their filename
/// digest differs from today's bare-method slot and a direct lookup misses.
/// The method slug rides in front of every filename, so scan the directory
/// for that prefix and let `describes_target` decide — one small dir, read
/// only after a primary miss. Retires with `legacy_path`.
fn same_method_record(paths: &MossPaths, target: &str) -> Option<PublishedSnapshot> {
    if target.contains(':') {
        return None; // moss slots kept their key shape; only plugin slots moved
    }
    let prefix = format!("{}-", method_slug(target));
    std::fs::read_dir(paths.deploy_records_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".json"))
        })
        .filter_map(|p| read(&p))
        .filter(|snap| snap.describes_target(target))
        // Several pre-slot records can share a method (a content-addressed
        // target left one per publish); the newest is the live baseline.
        .max_by(|a, b| a.published_at.cmp(&b.published_at))
}

/// `.moss/deploy/last-published.json` — the single record moss kept before the
/// keyed layout (2026-08-05 to 2026-08-17).
///
/// Every project published in that window has one, and refusing to read it
/// would hand each an unclassified change set on upgrade. It lives here rather
/// than in `MossPaths` because it is not part of the layout any more: when the
/// migration retires, this function goes with it.
fn legacy_path(paths: &MossPaths) -> PathBuf {
    paths.deploy_dir().join("last-published.json")
}

/// Record what went live. Call only after the target confirmed the commit — a
/// record written for a publish that did not land makes the next change set
/// under-report, telling the author nothing will change when something will.
///
/// Keyed by `snap.target`, so the writer and every reader derive the same file
/// name from the same string by construction.
///
/// Atomic: a crash mid-write leaves either the old record or none, never a
/// truncated one that would deserialize into a wrong diff.
pub fn save(paths: &MossPaths, snap: &PublishedSnapshot) -> Result<(), String> {
    let dest = path_for(paths, &snap.target);
    let json = serde_json::to_string(snap)
        .map_err(|e| format!("failed to serialize {}: {e}", dest.display()))?;
    crate::infra::atomic_write::write_atomic(&dest, &json)?;
    // The single slot has been migrated off — see `legacy_path`.
    // Leaving it would let a baseline from before the keyed layout outlive the
    // record that replaced it, and answer for a target it no longer describes.
    // allow:unlink the legacy publish-record path under .moss, not staging
    let _ = std::fs::remove_file(legacy_path(paths));
    Ok(())
}

/// `<method>-<64-bit digest of the whole target>`.
///
/// The method rides in front so the directory is readable — a person looking at
/// `.moss/deploy/records/` can see which hosts this folder has published to.
/// The digest is what makes it a key: distinct per target, stable across runs,
/// and free of separators, `..`, and case-folding surprises.
fn key_for(target: &str) -> String {
    // A bare target IS the method (a plugin slot); `moss:<id>` splits.
    let method = target.split_once(':').map(|(m, _)| m).unwrap_or(target);
    let digest = Sha256::digest(target.as_bytes());
    let mut key = method_slug(method);
    key.push('-');
    for b in &digest[..8] {
        key.push_str(&format!("{b:02x}"));
    }
    key
}

/// The legible half of a record filename, shared with the legacy prefix scan.
fn method_slug(method: &str) -> String {
    let slug: String = method
        .chars()
        .take(24)
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    if slug.is_empty() { "target".to_string() } else { slug }
}

/// The one record on disk, when the caller has no target to select by.
///
/// Falls back to the pre-keyed single slot only when there are no keyed records
/// at all: once one exists the legacy file is stale by definition, and `save`
/// has already deleted it.
fn sole_record(paths: &MossPaths) -> Option<PublishedSnapshot> {
    let mut records: Vec<PathBuf> = std::fs::read_dir(paths.deploy_records_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    match records.len() {
        0 => read(&legacy_path(paths)),
        1 => read(&records.pop()?),
        _ => None,
    }
}

/// Deserialize one record, or `None` for every reason a record can be missing
/// or untrustworthy — see the module docs.
fn read(path: &Path) -> Option<PublishedSnapshot> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(snap) => Some(snap),
        Err(e) => {
            log::warn!(
                "publish record at {} is unreadable ({e}); the next change set will be unclassified",
                path.display()
            );
            None
        }
    }
}


#[cfg(test)]
#[path = "published_record_tests.rs"]
mod tests;
