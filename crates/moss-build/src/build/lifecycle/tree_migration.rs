//! Two jobs on the folder's `.moss/` tree, run at every build start.
//!
//! The build output an older moss kept inside the folder, `.moss/build.nosync/`
//! (or `.moss/build/` before that), moves to this computer's store for the
//! folder ([`MossPaths::build_dir`]): renamed when the two share a volume, so
//! the last build arrives warm, and deleted otherwise, a step per build in the
//! background. Neither is ever done while another live process serves the
//! folder: it may be an older moss serving that very tree.
//!
//! Copies of the shared store, `.moss/cache`, that a sync client makes when two
//! machines create it at once are folded into it and removed. Their blobs move
//! unread, so a placeholder is not downloaded to be moved, and are checked when
//! first read.

use std::path::{Path, PathBuf};

use crate::build::io_utils::{create_output_dir_all, remove_output_dir_all};
use crate::moss_paths::MossPaths;

/// Fold sync-client copies of the shared store into it. Idempotent; every
/// failure is logged and leaves the build to run with whatever is there.
pub(crate) fn migrate_build_tree(mp: &MossPaths) {
    for copy in renamed_aside_siblings(&mp.store_dir()) {
        match sightings(&copy) {
            1 => {}
            2 => {
                log::info!(
                    "[tree] {} came back after it was removed: a sync client is restoring it, so it is left until the next launch",
                    copy.display()
                );
                continue;
            }
            _ => continue,
        }
        for sub in ["objects", "transforms"] {
            merge(&copy.join(sub), &mp.store_dir().join(sub));
        }
        // allow:unlink a store copy the sync client left, after its entries joined the store
        match remove_output_dir_all(&copy) {
            Ok(()) => log::info!("[tree] folded and removed {}", copy.display()),
            Err(e) => log::info!("[tree] folded {} but could not remove it: {e}", copy.display()),
        }
    }
}

/// Move the build output an older moss left in the folder to this computer's
/// store: one rename when the store has no output yet and the rename works,
/// otherwise a background delete. Does nothing while another live process
/// serves the folder, and tries again at the next build.
pub(crate) fn move_output_out(mp: &MossPaths) {
    // allow:unlink moves the folder's old output tree whole, never a served staging entry
    let doomed = move_output_out_with(mp, |from, to| std::fs::rename(from, to));
    if !doomed.is_empty() {
        std::thread::spawn(move || doomed.iter().for_each(|root| delete_a_step(root)));
    }
}

/// [`move_output_out`] with the rename given, returning the trees left to delete.
fn move_output_out_with(mp: &MossPaths, rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>) -> Vec<PathBuf> {
    let leftovers = output_left_in_folder(mp);
    if leftovers.is_empty() {
        return leftovers;
    }
    let folder = mp.project_root();
    if crate::ops::serve::ownership::live_foreign_owner(folder) {
        log::info!("[tree] the folder's build output stays in it while another moss process serves it");
        return Vec::new();
    }
    let Ok(_held) = crate::infra::folder_lock::acquire(folder) else { return Vec::new() };
    let (old, new) = (mp.legacy_build_dir(), mp.build_dir());
    let mut doomed = leftovers;
    if doomed.first() == Some(&old) && new.symlink_metadata().is_err() {
        // allow:unlink the store has no output yet, so nothing this rename replaces is served
        let moved = new.parent().map_or(Ok(()), create_output_dir_all).and_then(|()| rename(&old, &new));
        match moved {
            Ok(()) => {
                log::info!("[tree] moved {} to {}", old.display(), new.display());
                relink_current(mp);
                doomed.remove(0);
            }
            Err(e) => log::info!("[tree] could not move {} ({e}); it is deleted instead", old.display()),
        }
    }
    doomed
}

/// The trees an older moss left: `.moss/build.nosync` first, then the root it
/// had before that, and every copy of either a sync client renamed aside.
fn output_left_in_folder(mp: &MossPaths) -> Vec<PathBuf> {
    let old = mp.legacy_build_dir();
    if old == mp.build_dir() {
        return Vec::new();
    }
    let older = mp.root().join("build");
    let mut found: Vec<PathBuf> = [&old, &older].into_iter().filter(|p| p.is_dir()).cloned().collect();
    found.extend(renamed_aside_siblings(&old));
    found.extend(renamed_aside_siblings(&older));
    found
}

/// An older moss wrote `current` as an absolute link into the folder, which
/// the move leaves dangling: point it at the same generation, relatively.
fn relink_current(mp: &MossPaths) {
    #[cfg(unix)]
    if let Ok(target) = std::fs::read_link(mp.current_ptr()) {
        if let (true, Some(id)) = (target.is_absolute(), target.file_name()) {
            let relative = Path::new("generations").join(id);
            if let Err(e) = crate::build::io_utils::replace_with_symlink(&relative, &mp.current_ptr()) {
                log::info!("[tree] could not relink {} ({e})", mp.current_ptr().display());
            }
        }
    }
    #[cfg(not(unix))]
    let _ = mp;
}

/// Remove one generation of `root`, or `root` itself once none is left, so a
/// tree of tens of thousands of files is never deleted in one go. Never
/// uploaded, so deleting it is not a write a sync client sees.
fn delete_a_step(root: &Path) {
    let next = std::fs::read_dir(root.join("generations"))
        .ok()
        .and_then(|entries| entries.flatten().find(|e| e.file_type().is_ok_and(|t| t.is_dir())))
        .map(|e| e.path());
    let target = next.unwrap_or_else(|| root.to_path_buf());
    // allow:unlink build output an older moss left in the folder; this computer's store holds the output now
    if let Err(e) = remove_output_dir_all(&target) {
        log::info!("[tree] could not remove {} ({e})", target.display());
    }
}

/// How many times this process has met `root` beside the live tree. A root
/// the provider restores after its removal (a placeholder whose delete it
/// never took) would otherwise be harvested and removed at every build start.
fn sightings(root: &Path) -> u32 {
    static SEEN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<PathBuf, u32>>> =
        std::sync::LazyLock::new(Default::default);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let n = seen.entry(root.to_path_buf()).or_insert(0);
    *n = n.saturating_add(1);
    *n
}

/// Directories beside `root` named as a sync client names its copy of it.
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
        .filter(|e| e.file_name().to_str().is_some_and(|c| crate::nested_roots::is_sync_copy_of(c, name)))
        .map(|e| e.path())
        .collect()
}

/// Move every entry of a two-level fan-out `from` lacks in `to`.
fn merge(from: &Path, to: &Path) {
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
    use crate::build::cache::ObjectStore;

    fn blob(dir: &Path, bytes: &[u8]) -> String {
        ObjectStore::new(dir.to_path_buf()).store_bytes(bytes).expect("stored")
    }

    /// An older moss's output tree with one generation `current` names, the
    /// link written absolute as some older versions wrote it.
    fn older_output(mp: &MossPaths) -> PathBuf {
        let old = mp.legacy_build_dir();
        let generation = old.join("generations").join("0123456789abcdef");
        std::fs::create_dir_all(&generation).unwrap();
        std::fs::write(generation.join("index.html"), "built").unwrap();
        std::fs::write(old.join("current.generation"), "0123456789abcdef").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&generation, old.join("current")).unwrap();
        old
    }

    #[cfg(unix)]
    fn inode(path: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).unwrap().ino()
    }

    /// On one volume the tree is renamed, so the last build arrives whole: the
    /// same directory, not a copy, and `current` still serves it.
    #[cfg(unix)]
    #[test]
    fn the_older_output_is_renamed_into_the_store_and_current_still_serves() {
        let tmp = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(tmp.path());
        let old = older_output(&mp);
        let before = inode(&old.join("generations").join("0123456789abcdef"));

        assert!(move_output_out_with(&mp, |from, to| std::fs::rename(from, to)).is_empty());

        assert!(!old.exists(), "nothing is left in the folder");
        assert_eq!(inode(&mp.generation_dir("0123456789abcdef")), before, "renamed, not copied");
        assert_eq!(std::fs::read_link(mp.current_ptr()).unwrap(), Path::new("generations/0123456789abcdef"));
        assert!(mp.current_ptr().join("index.html").exists(), "current resolves inside the store");
    }

    /// Across volumes the folder's tree is deleted, one generation a step, and
    /// nothing of it reaches the store.
    #[test]
    fn a_tree_that_cannot_be_renamed_is_deleted_a_step_at_a_time() {
        let tmp = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(tmp.path());
        let old = older_output(&mp);
        let aside = mp.root().join("build.nosync 2");
        std::fs::create_dir_all(&aside).unwrap();

        let doomed = move_output_out_with(&mp, |_, _| Err(std::io::Error::from_raw_os_error(libc::EXDEV)));

        assert_eq!(doomed, vec![old.clone(), aside.clone()]);
        assert!(!mp.generations_dir().exists(), "nothing moved into the store");
        delete_a_step(&old);
        assert!(old.exists() && !old.join("generations").join("0123456789abcdef").exists(), "one generation went first");
        delete_a_step(&old);
        assert!(!old.exists(), "then the rest");
    }

    /// Another live process may be an older moss serving that very tree.
    #[test]
    fn a_tree_another_live_process_serves_is_left_alone() {
        crate::infra::home::with_moss_home(|_home| {
            let tmp = tempfile::tempdir().unwrap();
            let mp = MossPaths::new(tmp.path());
            let old = older_output(&mp);
            let owner = crate::ops::serve::ownership::OwnerRecord {
                folder: tmp.path().to_path_buf(),
                pid: 1,
                kind: crate::ops::serve::ownership::HostKind::Desktop,
                version: "0.15.20".into(),
                url: "http://127.0.0.1:1".into(),
                ui_url: None,
                started_at: String::new(),
            };
            crate::ops::serve::ownership::write_record_for_tests(tmp.path(), &owner);

            assert!(move_output_out_with(&mp, |from, to| std::fs::rename(from, to)).is_empty());

            assert!(old.join("generations").join("0123456789abcdef").join("index.html").exists());
            assert!(!mp.build_dir().exists());
        });
    }

    fn put_record(dir: &Path, oid: &str, text: &str) -> PathBuf {
        let shard = dir.join(&oid[..2]).join(&oid[2..4]);
        std::fs::create_dir_all(&shard).unwrap();
        let path = shard.join(format!("{oid}.json"));
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Two machines that each created the store leave a sync client's copies of
    /// it beside the one every build reads. Their blobs and records join it,
    /// what it already holds is kept, and the copies go.
    #[test]
    fn copies_of_the_store_are_folded_into_it_and_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(tmp.path());
        std::fs::create_dir_all(mp.build_dir()).unwrap();
        let store = mp.store_dir();
        let kept = blob(&store.join("objects"), b"encoded here");
        let drive_copy = mp.root().join("cache 2");
        let dropbox_copy = mp.root().join("cache (Ana's conflicted copy 2026-09-29)");
        let only_in_drive_copy = blob(&drive_copy.join("objects"), b"encoded on the other machine");
        let only_in_dropbox_copy = blob(&dropbox_copy.join("objects"), b"encoded on a third machine");
        let both = "ab".repeat(32);
        let ours = put_record(&store.join("transforms"), &both, "ours");
        put_record(&drive_copy.join("transforms"), &both, "theirs");
        let theirs_only = "cd".repeat(32);
        put_record(&drive_copy.join("transforms"), &theirs_only, "theirs");

        migrate_build_tree(&mp);

        let objects = ObjectStore::new(mp.cache_objects());
        for oid in [&kept, &only_in_drive_copy, &only_in_dropbox_copy] {
            assert!(objects.blob_path(oid).exists(), "{oid} is in the store");
        }
        assert_eq!(std::fs::read_to_string(&ours).unwrap(), "ours", "a record the store has is kept");
        let moved = store.join("transforms").join(&theirs_only[..2]).join(&theirs_only[2..4]).join(format!("{theirs_only}.json"));
        assert_eq!(std::fs::read_to_string(moved).unwrap(), "theirs", "a record only the copy had joins the store");
        assert!(!drive_copy.exists() && !dropbox_copy.exists(), "the copies are gone");
        migrate_build_tree(&mp);
        assert!(objects.blob_path(&kept).exists(), "a second run changes nothing");
    }

    /// A sync client that has not taken the delete puts a store copy back
    /// before the next build; removing it again on every build only fights it.
    #[test]
    fn a_store_copy_that_comes_back_is_left_until_the_next_launch() {
        let tmp = tempfile::tempdir().unwrap();
        let mp = MossPaths::new(tmp.path());
        std::fs::create_dir_all(mp.build_dir()).unwrap();
        let aside = mp.root().join("cache 2");
        std::fs::create_dir_all(aside.join("objects")).unwrap();
        migrate_build_tree(&mp);
        assert!(!aside.exists(), "the first sighting is removed");

        std::fs::create_dir_all(aside.join("objects")).unwrap();
        migrate_build_tree(&mp);
        assert!(aside.exists(), "a root that came back is left alone this launch");
    }
}
