//! Writing a version back: one page ([`restore_page`]) or the whole site
//! ([`restore_site`]), both through [`ObjectStore::link_to`] — the same
//! atomic temp-then-rename `link_to` already uses for the output tree, now
//! writing into the vault instead.
//!
//! Neither function saves the present as a version first — that is
//! [`super::record::save_before_restore`], a separate call the caller makes
//! (with a freshly sealed manifest) before either of these, exactly as a
//! manual save is. Composing them into one user-facing "Restore" action is
//! for the slice that has a CLI verb and a panel to drive it from.

use std::path::{Path, PathBuf};

use super::record::{self, Entry};
use super::store;
use crate::build::cache::ObjectStore;
use crate::build::manifest::SealedManifest;
use crate::types::content::MODE_SYMLINK;

/// Where a restored page's bytes go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreMode {
    /// Overwrite the file at its current vault path.
    InPlace,
    /// Write beside the current file as `<stem> (<date>).<ext>`, leaving the
    /// present content untouched.
    AsCopy,
}

/// The outcome of a best-effort site restore: what came back, what could
/// not, and what got trashed to reach the restored state. Never a hard
/// failure — see [`restore_site`]'s doc.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RestoreReport {
    pub restored: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub trashed: Vec<String>,
}

/// Restore one page from one version, in place or as a sibling copy.
///
/// Refuses when the content was not kept (over the ceiling, or otherwise
/// never stored), naming the reason rather than leaving the caller to infer
/// it from a missing file. `root` is the resolved site directory
/// ([`super::HistoryStore`]'s own field).
pub(crate) fn restore_page(root: &Path, vault_root: &Path, id: &str, path: &str, mode: RestoreMode) -> Result<(), String> {
    let record = store::load_record(root, id).ok_or_else(|| format!("no such version: {id}"))?;
    let entry = record.entries.get(path).ok_or_else(|| format!("{path} is not part of this version"))?;
    let object_store = store::object_store(root);
    let target = match mode {
        RestoreMode::InPlace => vault_root.join(path),
        RestoreMode::AsCopy => copy_sibling_path(vault_root, path, &record.published_at),
    };
    restore_entry(&object_store, entry, &target).map_err(|e| format!("{path}: {e}"))
}

/// Restore the whole site to one version: every entry whose hash differs
/// from `current` is written back (this covers both an edit since then and a
/// deletion since then — a deleted-since path simply is not in `current`'s
/// hashes); every path `current` has that this version does not goes to the
/// OS Trash.
///
/// Best-effort like the rest of publish history: one page's restore or trash
/// failing does not stop the rest, and the report says exactly what
/// happened rather than a single pass/fail bit. `trash` takes `(vault_root,
/// vault-relative path)`; production wires it to
/// [`crate::vault::fs::delete_entry_inner`] ([`real_trash`]), and every test
/// in this module passes a fake that records the call instead of moving a
/// temp file into the developer's real OS Trash — the same
/// dependency-injection shape [`super::record::record_one`] already uses for
/// `is_evicted`, for the same reason: the real thing is not something a test
/// may trigger.
pub(crate) fn restore_site(
    root: &Path,
    vault_root: &Path,
    id: &str,
    current: &SealedManifest,
    trash: &dyn Fn(&Path, &str) -> Result<(), String>,
) -> RestoreReport {
    let mut report = RestoreReport::default();
    let Some(record) = store::load_record(root, id) else {
        report.failed.push(("*".to_string(), format!("no such version: {id}")));
        return report;
    };
    let object_store = store::object_store(root);
    let current_hashes = record::current_entry_hashes(current);

    for (path, entry) in &record.entries {
        if current_hashes.get(path) == Some(&entry.hash) {
            continue; // already matches; nothing to restore
        }
        let target = vault_root.join(path);
        match restore_entry(&object_store, entry, &target) {
            Ok(()) => report.restored.push(path.clone()),
            Err(e) => report.failed.push((path.clone(), e)),
        }
    }

    for path in current_hashes.keys() {
        if record.entries.contains_key(path) {
            continue;
        }
        match trash(vault_root, path) {
            Ok(()) => report.trashed.push(path.clone()),
            Err(e) => report.failed.push((path.clone(), e)),
        }
    }

    report.restored.sort();
    report.trashed.sort();
    report.failed.sort_by(|a, b| a.0.cmp(&b.0));
    report
}

/// Move a vault-relative path to the OS Trash through the existing delete
/// core, the same one the desktop `delete_entry` command and the HTTP
/// mutation arm use — restoring an older version never gets a second,
/// bespoke way to remove a file. The default a real `restore_site` caller
/// passes (`cli::run_restore_site`, since slice 2); a test passes its own
/// fake instead, since this one is not something a test may trigger (it
/// moves a real file to the real OS Trash).
pub(crate) fn real_trash(vault_root: &Path, path: &str) -> Result<(), String> {
    let full = vault_root.join(path);
    crate::vault::fs::delete_entry_inner(vault_root, &full.to_string_lossy())
}

fn restore_entry(store: &ObjectStore, entry: &Entry, target: &Path) -> Result<(), String> {
    if entry.mode == MODE_SYMLINK {
        let blob = store.get_path(&entry.hash).ok_or_else(|| not_kept_reason(entry))?;
        // allow:raw_read .moss/history/ — cloud-synced now, but content-addressed and write-once: a missing/evicted blob already reads as "not kept," never silently as fresh
        let target_str =
            std::fs::read_to_string(&blob).map_err(|e| format!("could not read the kept symlink target: {e}"))?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        // `link_to` handles its own atomic replace for a regular file; a
        // symlink has no such helper, so remove-then-create is the whole
        // operation. Best-effort remove: "already gone" is not a failure.
        let _ = std::fs::remove_file(target);
        symlink_to(&target_str, target)
    } else {
        store.get_path(&entry.hash).ok_or_else(|| not_kept_reason(entry))?;
        store.link_to(&entry.hash, target)
    }
}

fn not_kept_reason(entry: &Entry) -> String {
    if entry.size.is_some_and(|s| s > store::HISTORY_MEDIA_CEILING) {
        "the content at this version was not kept (over the size ceiling)".to_string()
    } else {
        "the content at this version was not kept".to_string()
    }
}

#[cfg(unix)]
fn symlink_to(target_str: &str, dest: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target_str, dest).map_err(|e| format!("could not recreate the symlink: {e}"))
}

#[cfg(windows)]
fn symlink_to(target_str: &str, dest: &Path) -> Result<(), String> {
    std::os::windows::fs::symlink_file(target_str, dest).map_err(|e| format!("could not recreate the symlink: {e}"))
}

/// `<stem> (<date>).<ext>` beside the original — "Save a copy" leaves the
/// present file alone and writes the version next to it.
fn copy_sibling_path(vault_root: &Path, path: &str, published_at: &str) -> PathBuf {
    let original = vault_root.join(path);
    let stem = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let date = record::format_date(published_at);
    let filename = match original.extension() {
        Some(ext) => format!("{stem} ({date}).{}", ext.to_string_lossy()),
        None => format!("{stem} ({date})"),
    };
    original.with_file_name(filename)
}

#[cfg(test)]
#[path = "restore_tests.rs"]
mod tests;
