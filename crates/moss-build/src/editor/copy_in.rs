//! Landing OS files inside the vault, and saying what did not land.
//!
//! The loop is the whole story: validating a destination is a few lines, and
//! every interesting decision — what counts as a skip, what a skip means, how
//! long to wait for a cloud file — is here. Two carriers share this core: a
//! native file picker hands it absolute OS paths, and an HTTP upload route
//! hands it paths to freshly written temp files; either way the result is the
//! same [`CopyReport`].

use std::path::{Path, PathBuf};

use super::filesystem::{CopiedFile, CopyReport, ImportSkipReason, SkippedImport};

/// What the caller can do about a failed copy.
///
/// The only distinction worth drawing is "waiting would help". A cloud
/// placeholder that failed to materialize is not a broken file — the bytes
/// exist, moss has asked for them, and the same drop works once they land. The
/// classifier is `build::icloud`'s, not a second copy of it: `EDEADLK` from the
/// dataless fail-fast policy on Sonoma+, and an `ENOENT` with a `.name.icloud`
/// sibling on macOS 12-13. Everywhere else there is no fail-fast policy to
/// recover from, so every error is what it says it is.
///
/// A host uses this to classify its own copy failure the same way the shared loop does (cloud placeholder versus real error).
pub fn skip_reason(src: &Path, err: &std::io::Error) -> ImportSkipReason {
    if crate::build::icloud::is_offline_not_absent(src, err) {
        ImportSkipReason::StillInCloud
    } else {
        ImportSkipReason::Failed
    }
}

/// Validate the destination directory of an into-project copy. One door: the
/// shared first-party guard ([`crate::vault::fs::validate_entry_path`]), then —
/// because the target must already exist — the canonical-form recheck its doc
/// prescribes, so a symlinked component inside the vault cannot point the copy
/// outside it. Returns the validated (non-canonical) path so callers'
/// relative-path math keeps the root's original spelling.
///
/// A host uses this to check a destination folder before handing it to [`copy_files_into`].
pub fn validate_copy_target(
    project_root: &Path,
    target_dir: &str,
) -> Result<PathBuf, String> {
    let target = crate::vault::fs::validate_entry_path(project_root, target_dir)?;
    if !target.is_dir() {
        return Err(format!("Target '{}' is not a directory", target.display()));
    }
    crate::vault::fs::recheck_canonical(project_root, &target)?;
    Ok(target)
}

/// Directory names to skip when recursively copying a folder into the vault.
/// These are excluded to avoid copying large caches / metadata:
/// - `.moss`        — moss's own generated build artefacts
/// - `node_modules` — NPM dependency trees (potentially gigabytes)
/// - `.git`         — version-control history
///
/// Comparison is by exact `file_name()` match — a user-named folder like
/// `notes/.git-history/` still copies because its name is `.git-history`, not `.git`.
///
/// A host uses this to apply the same skip list when it walks a folder of its own.
pub const IMPORT_SKIP_DIRS: &[&str] = &[".moss", "node_modules", ".git"];

/// Recursively copy `src` (a directory) into `dst`, skipping entries whose
/// `file_name()` appears in [`IMPORT_SKIP_DIRS`]. Returns the number of files
/// (not directories) copied — callers use it only to tell "nothing landed"
/// from "something did", which is what decides whether the folder is reported
/// as copied at all.
///
/// # iCloud safety
/// Uses `std::fs::copy` exclusively — NEVER `fs::hard_link`: on APFS this
/// becomes a COW reflink via `fclonefileat(2)`; on every other filesystem it
/// pays the byte copy as the cost of staying safe against cloud eviction,
/// which a hard link is not (an evicted source zeroes every link sharing its
/// inode).
///
/// # Symlink policy
/// A symlink entry is copied as a plain file — `fs::copy` writes the bytes it
/// points to, not the link. The directory test is `DirEntry::file_type`, which
/// does NOT traverse: `src_child.is_dir()` follows the link, so a folder
/// containing `link -> ..` would recurse without bound (nothing consults the
/// deadline between directories) and a link to `/` would copy the reachable
/// filesystem into the vault.
///
/// # Failures do not abandon the folder
/// Every error is pushed onto `skipped` and the walk continues: one evicted
/// photo in a folder of two hundred must not cost the other 199, and a folder
/// reported as a single failure tells the caller nothing about what did land.
/// Returns the number of FILES copied.
///
/// A host uses this to copy a dropped folder with the same skip, symlink and deadline rules as [`copy_files_into`].
/// `dst` must come from [`validate_copy_target`]; this function does no containment check of its own.
pub fn copy_dir_for_import(
    src: &Path,
    dst: &Path,
    deadline: std::time::Instant,
    skipped: &mut Vec<SkippedImport>,
) -> u32 {
    // A free fn, not a closure: a closure capturing `skipped` would hold the
    // unique borrow across the recursive call below.
    fn note(out: &mut Vec<SkippedImport>, path: &Path, msg: String, reason: ImportSkipReason) {
        log::warn!("import: {msg}");
        out.push(SkippedImport {
            original_name: path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string()),
            reason,
        });
    }

    if let Err(e) = std::fs::create_dir_all(dst) {
        note(skipped, src, format!("cannot create '{}': {e}", dst.display()), ImportSkipReason::Failed);
        return 0;
    }

    let entries = match std::fs::read_dir(src) {
        Ok(entries) => entries,
        Err(e) => {
            note(skipped, src, format!("cannot read '{}': {e}", src.display()), ImportSkipReason::Failed);
            return 0;
        }
    };

    let mut file_count = 0u32;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                note(skipped, src, format!("cannot read an entry of '{}': {e}", src.display()), ImportSkipReason::Failed);
                continue;
            }
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let src_child = entry.path();
        let dst_child = dst.join(&name);

        // NOT `src_child.is_dir()` — see the symlink policy above.
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if IMPORT_SKIP_DIRS.iter().any(|s| *s == name_str.as_ref()) {
                log::debug!("import: skipping excluded directory '{}'", src_child.display());
                continue;
            }
            file_count += copy_dir_for_import(&src_child, &dst_child, deadline, skipped);
        } else {
            // Through `copy_file_into_vault`, not a bare `fs::copy`: a folder of
            // evicted iCloud photos is the same drop as a selection of them, and
            // only the flat selection getting the materialization wait would be
            // a drop that reported success for some of the same folder and
            // failure for the rest, depending on which path it arrived by. Same
            // shared deadline, so a big folder cannot multiply it.
            let budget = deadline.saturating_duration_since(std::time::Instant::now());
            match copy_file_into_vault(&src_child, &dst_child, budget) {
                Ok(_) => file_count += 1,
                Err(e) => {
                    let reason = skip_reason(&src_child, &e);
                    note(skipped, &src_child, format!("cannot copy '{}': {e}", src_child.display()), reason);
                }
            }
        }
    }

    file_count
}

/// Copy ONE OS file into the vault, giving a cloud-evicted source a bounded
/// chance to arrive first.
///
/// Every drop path lands its files through here, because the failure this
/// exists for is not specific to one of them: under a dataless fail-fast
/// policy a plain `fs::copy` of an evicted iCloud source returns `EDEADLK`
/// immediately, and the bytes are in the cloud rather than missing.
/// `retry_after_materialize` is the recovery half of that policy — it asks the
/// provider for the file and copies again — so the usual outcome is that the
/// drop simply works, a few seconds late.
///
/// `budget` is the caller's remaining wait for the WHOLE gesture, not per
/// file: twenty evicted photos must not park the caller for twenty deadlines.
/// A zero budget still attempts the copy and still asks for the download, so a
/// second drop finds the file local; it just does not wait.
///
/// Any other error passes straight through unchanged — waiting cannot fix a
/// missing file or a permissions problem, and delaying that report only makes
/// it worse.
///
/// A host uses this to copy a single file with the same cloud-materialization wait as [`copy_files_into`].
/// `dst` must come from [`validate_copy_target`]; this function does no containment check of its own.
pub fn copy_file_into_vault(
    src: &Path,
    dest: &Path,
    budget: std::time::Duration,
) -> std::io::Result<u64> {
    if budget.is_zero() {
        // allow:raw_write copies a user's file into the vault, not build output; an evicted source is handled by the retry below
        return std::fs::copy(src, dest).inspect_err(|e| {
            if crate::build::icloud::is_offline_not_absent(src, e) {
                crate::build::cloud_readiness::request_download(src);
            }
        });
    }
    crate::build::cloud_readiness::retry_after_materialize(src, budget, || {
        // allow:raw_write copies a user's file into the vault, not build output; an evicted source is retried by the wrapper
        std::fs::copy(src, dest)
    })
}

/// The copy loop itself, without any host-shell state. `target` is already
/// validated as a directory inside `project_root` (see
/// [`validate_copy_target`]); this only decides what lands and what is
/// reported back.
///
/// A host uses this to land a list of OS paths in a validated target and get back the [`CopyReport`].
pub fn copy_files_into(
    project_root: &Path,
    source_paths: &[String],
    target: &Path,
) -> CopyReport {
    let mut report = CopyReport { copied: Vec::new(), skipped: Vec::new() };
    // ONE materialization budget for the whole gesture, folders included. Files
    // past it are still attempted and still requested from the provider — they
    // just do not wait.
    let deadline = std::time::Instant::now() + crate::build::cloud_readiness::INTERACTIVE_DEADLINE;

    for src_path_str in source_paths {
        let src = Path::new(src_path_str);
        let named = src.file_name().map(|n| n.to_string_lossy().to_string());
        // Something to name in the message even when the path has no basename.
        let label = named.clone().unwrap_or_else(|| src_path_str.clone());

        let Some(original_name) = named else {
            log::warn!("Cannot determine filename for '{}', skipping", src.display());
            report.skipped.push(SkippedImport { original_name: label, reason: ImportSkipReason::Failed });
            continue;
        };
        // `exists()` is false for a pre-Sonoma iCloud placeholder: the real path
        // is genuinely absent and only a hidden `.name.icloud` sibling is there.
        // Asking the classifier first is what keeps an evicted file from being
        // reported as one the caller deleted.
        if !src.exists() && !crate::build::icloud::is_still_in_the_cloud(src) {
            log::warn!("Skipping non-existent source: {}", src.display());
            report.skipped.push(SkippedImport { original_name, reason: ImportSkipReason::Failed });
            continue;
        }

        let dest = super::filesystem::resolve_collision(&target.join(&original_name));
        let final_name = dest.file_name().unwrap().to_string_lossy().to_string();
        if src.is_dir() {
            // The single skip-list-aware recursive copy (excludes .moss /
            // node_modules / .git). Never a generic recursive-copy helper that
            // has no exclusions.
            //
            // It reports per-file failures into `report.skipped` rather than
            // returning on the first one, so a folder of 200 evicted photos
            // yields 199 copies and one named skip instead of one skip named
            // after the folder and a half-copied directory in the vault.
            let before = report.skipped.len();
            let files = copy_dir_for_import(src, &dest, deadline, &mut report.skipped);
            if files == 0 && report.skipped.len() > before {
                // Nothing landed, so nothing should remain: the walk creates
                // each destination directory before it knows whether anything
                // will go in it, and a drop that copied no file at all would
                // otherwise leave the caller an empty folder skeleton to
                // delete by hand. `remove_dir_all` is safe here for the same
                // reason — it can only contain directories this call made.
                let _ = std::fs::remove_dir_all(&dest);
                continue;
            }
            let relative_path = dest.strip_prefix(project_root).unwrap_or(&dest).to_string_lossy().to_string();
            report.copied.push(CopiedFile { original_name, final_name, final_path: dest.to_string_lossy().to_string(), relative_path });
            continue;
        }
        let copy_result = {
            let budget = deadline.saturating_duration_since(std::time::Instant::now());
            copy_file_into_vault(src, &dest, budget)
                .map(|_| ())
                .map_err(|e| {
                    let reason = skip_reason(src, &e);
                    (format!("Failed to copy '{}' to '{}': {e}", src.display(), dest.display()), reason)
                })
        };
        if let Err((msg, reason)) = copy_result {
            log::warn!("Failed to copy '{}': {msg}", src.display());
            report.skipped.push(SkippedImport { original_name, reason });
            continue;
        }
        let relative_path = dest.strip_prefix(project_root).unwrap_or(&dest).to_string_lossy().to_string();
        report.copied.push(CopiedFile { original_name, final_name, final_path: dest.to_string_lossy().to_string(), relative_path });
    }
    report
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A drop whose files all fail must not come back as an empty success.
    /// That shape — `Ok` with nothing in it — is what let a real caller drop a
    /// photo in and see no image and no error.
    #[test]
    fn a_copy_that_lands_nothing_reports_every_skip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("posts");
        std::fs::create_dir_all(&target).unwrap();

        let report = copy_files_into(
            root,
            &[root.join("gone.jpg").to_string_lossy().to_string()],
            &target,
        );

        assert!(report.copied.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].original_name, "gone.jpg");
        assert_eq!(report.skipped[0].reason, ImportSkipReason::Failed);
    }

    /// Partial failure is the case that hides best: the good file lands, the
    /// caller sees a non-empty list, and the missing one is never mentioned.
    #[test]
    fn a_partial_copy_reports_both_halves() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("posts");
        std::fs::create_dir_all(&target).unwrap();
        let good = root.join("photo.jpg");
        std::fs::write(&good, b"jpegbytes").unwrap();

        let report = copy_files_into(
            root,
            &[
                good.to_string_lossy().to_string(),
                root.join("gone.jpg").to_string_lossy().to_string(),
            ],
            &target,
        );

        assert_eq!(report.copied.len(), 1);
        assert_eq!(report.copied[0].final_name, "photo.jpg");
        assert!(target.join("photo.jpg").exists());
        assert!(good.exists(), "landing a file in the vault is a copy, never a move — the source stays put");
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].original_name, "gone.jpg");
    }

    /// A rejected path is a skip like any other — it used to be a `continue`.
    #[test]
    fn a_rejected_path_is_reported_too() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("posts");
        std::fs::create_dir_all(&target).unwrap();

        let report = copy_files_into(root, &["../../etc/passwd".to_string()], &target);

        assert!(report.copied.is_empty());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, ImportSkipReason::Failed);
    }

    /// Containment is the target's job ([`validate_copy_target`]), never a
    /// substring test on the source: a `..` scan protects nothing — an
    /// absolute `/etc/passwd` has no `..` and always passes — while rejecting a
    /// filename people really have.
    #[test]
    fn a_dotdot_in_a_filename_is_not_a_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("posts");
        std::fs::create_dir_all(&target).unwrap();
        let odd = root.join("photo..jpg");
        std::fs::write(&odd, b"jpegbytes").unwrap();

        let report = copy_files_into(root, &[odd.to_string_lossy().to_string()], &target);

        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        assert_eq!(report.copied.len(), 1);
        assert!(target.join("photo..jpg").exists());
    }

    /// The folder drop, through the real entry point. One unreadable file must
    /// cost that file and nothing else: a recursion that returns on the first
    /// error would make a folder of two hundred evicted photos land two of
    /// them and report a single skip named after the folder.
    #[cfg(unix)]
    #[test]
    fn a_folder_drop_keeps_going_past_a_file_it_cannot_copy() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("posts");
        std::fs::create_dir_all(&target).unwrap();
        let folder = root.join("album");
        std::fs::create_dir_all(&folder).unwrap();
        // Alphabetically first, so the walk meets the failure before the files
        // that must survive it.
        std::os::unix::fs::symlink(root.join("nowhere.jpg"), folder.join("a-broken.jpg")).unwrap();
        std::fs::write(folder.join("b.jpg"), b"one").unwrap();
        std::fs::write(folder.join("c.jpg"), b"two").unwrap();

        let report = copy_files_into(root, &[folder.to_string_lossy().to_string()], &target);

        assert_eq!(report.copied.len(), 1, "the folder itself landed");
        assert_eq!(report.copied[0].final_name, "album");
        assert!(target.join("album/b.jpg").exists());
        assert!(target.join("album/c.jpg").exists());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].original_name, "a-broken.jpg");
    }

    /// The other end of the same walk: when NOTHING copies, the caller is told
    /// the drop failed, so an empty copy of the folder tree must not be sitting
    /// in the vault contradicting that.
    #[cfg(unix)]
    #[test]
    fn a_folder_drop_that_copies_nothing_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("posts");
        std::fs::create_dir_all(&target).unwrap();
        let folder = root.join("album");
        std::fs::create_dir_all(folder.join("nested")).unwrap();
        std::os::unix::fs::symlink(root.join("nowhere.jpg"), folder.join("broken.jpg")).unwrap();

        let report = copy_files_into(root, &[folder.to_string_lossy().to_string()], &target);

        assert!(report.copied.is_empty(), "nothing copied");
        assert_eq!(report.skipped.len(), 1);
        assert!(!target.join("album").exists(), "no empty skeleton left in the vault");
    }

    /// `Path::is_dir()` follows symlinks and `DirEntry::file_type()` does not.
    /// With the former, a dropped folder containing `link -> ..` would recurse
    /// without bound (nothing consults the deadline between directories), and a
    /// link to `/` would copy the reachable filesystem into the vault.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_inside_a_drop_is_not_descended() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let target = root.join("posts");
        std::fs::create_dir_all(&target).unwrap();
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"not yours").unwrap();
        let folder = root.join("album");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("photo.jpg"), b"jpegbytes").unwrap();
        std::os::unix::fs::symlink(&outside, folder.join("escape")).unwrap();

        let report = copy_files_into(root, &[folder.to_string_lossy().to_string()], &target);

        assert_eq!(report.copied.len(), 1);
        assert!(target.join("album/photo.jpg").exists());
        assert!(
            !target.join("album/escape/secret.txt").exists(),
            "the link was descended and pulled a file from outside the drop"
        );
        assert!(
            !target.join("album/escape").is_dir(),
            "the link landed as a directory rather than being copied as a file"
        );
    }

    /// Anything a download cannot fix is a plain failure, on every platform.
    #[test]
    fn a_permissions_error_is_not_a_cloud_stall() {
        let err = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            skip_reason(Path::new("/nowhere/x.jpg"), &err),
            ImportSkipReason::Failed
        );
    }

    /// The reported bug's error, classified. `EDEADLK` is the macOS dataless
    /// fail-fast policy refusing to materialize an evicted iCloud file — the
    /// bytes are in the cloud, so this must never read as a broken file.
    ///
    /// macOS-only by construction: no other platform arms that policy, so
    /// `is_offline_not_absent` (and this test) can only be meaningful there.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_evicted_icloud_source_is_reported_as_still_in_the_cloud() {
        let err = std::io::Error::from_raw_os_error(libc::EDEADLK);
        assert_eq!(
            skip_reason(Path::new("/Users/x/Desktop/photo.jpg"), &err),
            ImportSkipReason::StillInCloud
        );
    }

    /// [`validate_copy_target`] rejects a destination outside the project root
    /// — the same guard the HTTP upload route leans on for its `dir` field.
    #[test]
    fn validate_copy_target_rejects_outside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();

        let err = validate_copy_target(&root, outside.to_str().unwrap()).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn validate_copy_target_accepts_a_directory_inside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("posts")).unwrap();

        let target = validate_copy_target(root, &root.join("posts").to_string_lossy()).unwrap();
        assert_eq!(target, root.join("posts"));
    }
}
