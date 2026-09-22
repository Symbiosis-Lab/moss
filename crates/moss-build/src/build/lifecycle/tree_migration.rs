//! One-time move from the single `.moss/build` root to the split tree: the
//! content-addressed store at `.moss/cache/{objects,transforms}`, shared with
//! the folder, and everything else per machine under `.moss/build.nosync`.
//!
//! Runs at every build start and does nothing once the tree is in shape. The
//! per-machine root is renamed, never copied, so the move is free; the two
//! store directories are renamed out of it, or merged into a store another
//! machine already synced in. A root a cloud sync client renamed aside —
//! `build 2`, `build 31` — is harvested for blobs the store lacks, each hashed
//! once before it is trusted, and then removed. On a provider that honours the
//! `.nosync` suffix the rename is also what clears the cloud's copy of the old
//! root: it sees a delete, and a create it does not upload.

use std::path::{Path, PathBuf};

use crate::build::cache::ObjectStore;
use crate::build::io_utils::{create_output_dir_all, remove_output_dir_all};
use crate::moss_paths::MossPaths;

/// The name the tree had before the split.
const LEGACY_ROOT: &str = "build";

/// Bring `mp`'s tree into the split layout. Idempotent; every failure is
/// logged and leaves the build to run with whatever is there.
pub(crate) fn migrate_build_tree(mp: &MossPaths) {
    let moss = mp.root();
    let legacy = moss.join(LEGACY_ROOT);
    let per_machine = moss.join("build.nosync");
    if legacy.is_dir() && !per_machine.exists() {
        // allow:unlink the legacy root is renamed whole, contents untouched
        match std::fs::rename(&legacy, &per_machine) {
            Ok(()) => log::info!("[tree] moved {} to {}", legacy.display(), per_machine.display()),
            Err(e) => {
                log::warn!("[tree] could not move {} aside ({e}); building beside it", legacy.display());
                return;
            }
        }
    }
    for (sub, verify) in [("objects", true), ("transforms", false)] {
        move_or_merge(&per_machine.join("cache").join(sub), &mp.store_dir().join(sub), verify);
    }

    // Roots left beside the live one: the legacy root, when `build.nosync`
    // was already there (the authoring docs once told iCloud users to make
    // it by hand), and every root a sync client renamed aside.
    let mut leftovers = renamed_aside_siblings(&legacy);
    if legacy.is_dir() {
        leftovers.push(legacy);
    }
    for root in leftovers {
        for (sub, verify) in [("objects", true), ("transforms", false)] {
            merge(&root.join("cache").join(sub), &mp.store_dir().join(sub), verify);
        }
        // allow:unlink a root the sync client renamed aside, or the legacy root, after harvest
        match remove_output_dir_all(&root) {
            Ok(()) => log::info!("[tree] harvested and removed {}", root.display()),
            Err(e) => log::warn!("[tree] harvested {} but could not remove it: {e}", root.display()),
        }
    }
}

/// `<name> <digits>` directories beside `root`, the shape a sync client's
/// rename-aside leaves.
fn renamed_aside_siblings(root: &Path) -> Vec<PathBuf> {
    let (Some(name), Some(parent)) = (root.file_name().and_then(|n| n.to_str()), root.parent()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| e.file_name().to_str().is_some_and(|c| super::root_identity::is_renamed_aside(c, name)))
        .map(|e| e.path())
        .collect()
}

/// Rename `from` into place as `to`; when `to` already exists — another
/// machine's store synced in first — merge into it instead.
fn move_or_merge(from: &Path, to: &Path, verify: bool) {
    if !from.is_dir() {
        return;
    }
    if !to.exists() {
        let parent_ok = to.parent().is_none_or(|p| create_output_dir_all(p).is_ok());
        // allow:unlink a store directory is renamed whole into the store
        if parent_ok && std::fs::rename(from, to).is_ok() {
            log::info!("[tree] moved {} to {}", from.display(), to.display());
            return;
        }
    }
    merge(from, to, verify);
    // allow:unlink the emptied store directory under the per-machine root, not staging
    let _ = remove_output_dir_all(from);
}

/// Move every entry of a two-level fan-out `from` lacks in `to`. With
/// `verify`, an entry is trusted only if its bytes hash to its name — a blob
/// out of a root the sync client shunted may be anything.
fn merge(from: &Path, to: &Path, verify: bool) {
    if !from.is_dir() {
        return;
    }
    let mut taken = 0usize;
    for p1 in read_dirs(from) {
        for p2 in read_dirs(&p1) {
            let Ok(files) = std::fs::read_dir(&p2) else { continue };
            for file in files.flatten() {
                let name = file.file_name();
                let Some(name_str) = name.to_str() else { continue };
                if name_str.contains(".pending.") || !file.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                let dest = to
                    .join(p1.file_name().unwrap_or_default())
                    .join(p2.file_name().unwrap_or_default())
                    .join(&name);
                if dest.exists() {
                    continue;
                }
                if verify && ObjectStore::hash_file(&file.path()).ok().as_deref() != Some(name_str) {
                    log::warn!("[tree] {} does not hash to its name — left behind", file.path().display());
                    continue;
                }
                if dest.parent().is_none_or(|p| create_output_dir_all(p).is_err()) {
                    continue;
                }
                // allow:unlink a harvested entry moves into the store by rename
                if std::fs::rename(file.path(), &dest).is_ok() {
                    taken += 1;
                }
            }
        }
    }
    if taken > 0 {
        log::info!("[tree] took {taken} entries from {} into {}", from.display(), to.display());
    }
}

fn read_dirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|it| it.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(dir: &Path, bytes: &[u8]) -> String {
        ObjectStore::new(dir.to_path_buf()).store_bytes(bytes).expect("stored")
    }

    fn put_forged(dir: &Path, oid: &str) {
        let shard = dir.join(&oid[..2]).join(&oid[2..4]);
        std::fs::create_dir_all(&shard).unwrap();
        std::fs::write(shard.join(oid), b"not what the name says").unwrap();
    }

    #[test]
    fn the_legacy_root_becomes_the_per_machine_root_and_the_store_moves_out_of_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(tmp.path());
        let legacy = mp.root().join(LEGACY_ROOT);
        std::fs::create_dir_all(legacy.join("staging")).unwrap();
        std::fs::write(legacy.join("staging").join("index.html"), "built").unwrap();
        let kept = blob(&legacy.join("cache").join("objects"), b"encoded once");

        migrate_build_tree(&mp);

        assert!(!legacy.exists(), "nothing is left at the legacy root");
        assert!(mp.staging_dir().join("index.html").exists(), "staging rode along under the per-machine root");
        assert!(ObjectStore::new(mp.cache_objects()).blob_path(&kept).exists(), "the blob is in the store");
        assert!(!mp.build_dir().join("cache").join("objects").exists(), "and no longer under the per-machine root");
        migrate_build_tree(&mp);
        assert!(mp.staging_dir().join("index.html").exists(), "a second run changes nothing");
    }

    #[test]
    fn renamed_aside_roots_are_harvested_for_verifying_blobs_and_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(tmp.path());
        let legacy = mp.root().join(LEGACY_ROOT);
        std::fs::create_dir_all(legacy.join("staging")).unwrap();
        let aside = mp.root().join("build 31");
        let only_there = blob(&aside.join("cache").join("objects"), b"a video encoded before the swap");
        let forged = "f".repeat(64);
        put_forged(&aside.join("cache").join("objects"), &forged);
        // A hand-made per-machine root already holds a store of its own.
        let hand_made = mp.root().join("build.nosync");
        let already = blob(&hand_made.join("cache").join("objects"), b"stored by hand");

        migrate_build_tree(&mp);

        let store = ObjectStore::new(mp.cache_objects());
        assert!(store.blob_path(&only_there).exists(), "the sibling's blob was harvested");
        assert!(store.blob_path(&already).exists(), "so was the hand-made root's");
        assert!(!store.blob_path(&forged).exists(), "a blob that does not hash to its name was not");
        assert!(!aside.exists(), "the sibling is gone");
        assert!(!legacy.exists(), "and so is the legacy root");
    }
}
