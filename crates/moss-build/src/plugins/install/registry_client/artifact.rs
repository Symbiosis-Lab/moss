//! Installing a plugin the registry publishes, from bytes that came over the
//! network.
//!
//! The bundled installer copies from the binary, so the bytes are already
//! trusted and already here. This one has neither property, and the whole
//! module is the difference: what arrives is verified against the hash review
//! pinned, unpacked somewhere it cannot damage anything, checked against the
//! same admission verdict the loader uses, and only then allowed to become the
//! plugin the folder has.
//!
//! The archive is hashed TWICE and that is not redundant.
//! [`fetch_verified`](crate::system::large_download::fetch_verified) hashes the
//! file it wrote; this module then reads that file, hashes the bytes it
//! actually read, and unpacks *those bytes* from memory. Between the two there
//! is a real window — the artifact is a file on disk, and a file on disk can
//! change — so the second hash is what makes "what was verified" and "what was
//! unpacked" the same bytes rather than two reads that usually agree.
//!
//! Nothing is unpacked inside `.moss/plugins/`. Three separate scanners read
//! that directory and load every subdirectory holding a `manifest.json`
//! without filtering names, so an archive staged there is loadable in the
//! window between extracting it and judging it — and after a crash, forever.
//! Staging lives in `.moss/.plugin-staging/` instead, which nothing scans.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::super::zip_extract;
use super::index::IndexEntry;
use crate::system::large_download::fetch_verified;

/// Install `entry` into `project_path`'s `.moss/plugins/<id>`.
///
/// Returns the installed version on success.
pub fn install_from_registry(entry: &IndexEntry, project_path: &str) -> Result<String, String> {
    install_into(entry, project_path, &download_dir()?)
}

/// [`install_from_registry`] with the archive cache named explicitly.
///
/// Exists so a test can point it at a temporary directory: `download_dir` is
/// the real app-data path on the machine running the test, and a test that
/// writes there is a test that installs plugins into the developer's own moss.
pub fn install_into(
    entry: &IndexEntry,
    project_path: &str,
    archive_dir: &Path,
) -> Result<String, String> {
    let archive = archive_dir.join(format!("{}-{}.zip", entry.id, entry.version));
    let label = format!("[plugin {} {}]", entry.id, entry.version);
    fetch_verified(&entry.download_url, &archive, &entry.sha256, &label, None)?;

    let bytes = read_verified(&archive, &entry.sha256).map_err(|e| {
        // Whatever is in the cache is not what was asked for, and a file that
        // stays there is a file the next attempt may read again.
        let _ = fs::remove_file(&archive);
        format!("{} {}: {e}", entry.id, entry.version)
    })?;

    // `.moss/plugins/` need not exist yet — staging is elsewhere, so nothing
    // has created it on the way here, and the swap below is a rename into it.
    let plugins_dir = Path::new(project_path).join(".moss").join("plugins");
    fs::create_dir_all(&plugins_dir)
        .map_err(|e| format!("create {}: {e}", plugins_dir.display()))?;
    let target = plugins_dir.join(&entry.id);
    let staging = staging_dir(project_path, &entry.id)?;

    // Every step from here owes the same cleanup — a staging directory left
    // behind is a half-unpacked plugin sitting in the vault — so it is written
    // once, around all of them, rather than once per step where the next step
    // added can forget it.
    let outcome = (|| {
        unpack_and_admit(&bytes, &staging, entry)?;

        // What the archive laid down, read off disk rather than out of the
        // extractor: the receipt records it so the *next* update can tell this
        // version's code from the plugin's own files. An incomplete answer
        // would make that update resurrect whatever the walk missed, so a
        // directory that cannot be read fails the install instead.
        let delivered: Vec<String> = relative_paths(&staging)
            .map_err(|e| format!("read the unpacked {}: {e}", entry.id))?
            .iter()
            .map(|p| receipt_name(p))
            .collect();

        // Into staging, before the swap, so the receipt and the code it
        // describes become the installed directory in the same rename.
        // Written afterwards, a process killed in between left a version live
        // with no receipt — and an unreceipted directory is the
        // keep-everything branch, so the *next* update would carry this
        // version's deleted code forward and then record a receipt that does
        // not name it, protecting it for good. `delivered` is computed above,
        // so the receipt does not list itself — unless the archive shipped a
        // file by that name, which `carry`'s unconditional skip absorbs.
        super::receipt::write(&staging, entry, delivered)?;

        promote(&staging, &target, &entry.id)
    })();
    if outcome.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    outcome?;

    // The user asked for these bytes, which is the consent the loader will
    // ask about. After the swap, on the live directory, so the record names
    // the path the loader is handed. A record that cannot be written is not a
    // failed install: the plugin is in place, refused until the user allows
    // it from the tile, and the log says why.
    if let Err(e) = super::enforce::approve(&target) {
        log::warn!(target: "registry", "{} {} installed but its approval was not recorded: {e}", entry.id, entry.version);
    }
    Ok(entry.version.clone())
}

/// Read `archive` and return its bytes only if they hash to `sha256`.
///
/// The returned buffer is what gets unpacked, which is the point: this hashes
/// the bytes it read rather than the file it read them from, so nothing can
/// change between the check and the use.
fn read_verified(archive: &Path, sha256: &str) -> Result<Vec<u8>, String> {
    let bytes = fs::read(archive).map_err(|e| format!("read the downloaded archive: {e}"))?;
    if hex_sha256(&bytes) != sha256.to_lowercase() {
        return Err("does not match the hash the registry published, and nothing was installed"
            .to_string());
    }
    Ok(bytes)
}

/// A directory to unpack into that nothing loads plugins from.
///
/// Cleared first: a previous attempt that was killed between extracting and
/// judging leaves one behind, and reusing it would mix two archives.
fn staging_dir(project_path: &str, id: &str) -> Result<PathBuf, String> {
    let dir = Path::new(project_path)
        .join(".moss")
        .join(".plugin-staging")
        .join(id);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Unpack into `staging` and decide whether what came out may run.
///
/// The manifest inside the archive is checked against the entry that pointed
/// at it. Registry CI reads the manifest out of the published zip, so the two
/// agree by construction — a disagreement means the index and the artifact
/// have come apart, and installing the artifact anyway would install something
/// nobody reviewed under a name somebody did.
fn unpack_and_admit(bytes: &[u8], staging: &Path, entry: &IndexEntry) -> Result<(), String> {
    zip_extract::extract_zip_safe(bytes, staging)
        .map_err(|e| format!("unpack {}: {e:?}", entry.id))?;

    let manifest = crate::plugins::bundled::read_installed_manifest(staging)
        .ok_or_else(|| format!("{} has no readable manifest.json", entry.id))?;
    if manifest.name != entry.id || manifest.version != entry.version {
        return Err(format!(
            "the registry lists {} {} but the archive contains {} {}",
            entry.id, entry.version, manifest.name, manifest.version
        ));
    }
    // Withdrawn or too new, and not consent: these bytes were not asked for
    // by anyone yet, and the install that succeeds is what records that.
    if let Some(refusal) = super::enforce::withheld(&manifest, false) {
        return Err(format!("{} cannot be installed: {}", entry.id, refusal.sentence(&manifest)));
    }
    Ok(())
}

/// Swap `staging` in as `target`, keeping what the plugin wrote for itself.
///
/// The invariant every step holds: a file the plugin wrote is always inside a
/// directory that is either `target` or renameable back to it. That is why the
/// plugin's files move only *after* the swap has succeeded, into the live
/// directory — the earlier version moved them first, and a failed swap then
/// left them in a staging directory the error path deleted.
///
/// So a set-aside that fails changes nothing, a swap that fails restores the
/// old directory whole, and a process killed anywhere in between leaves the
/// state split between exactly two directories that the next run reconciles.
/// The cost is a moment where the new code is live without the plugin's state,
/// which is recoverable; deleted credentials are not.
///
/// The files move rather than copy. A rename relocates a symlink instead of
/// following it, so a link planted in a plugin directory cannot make this walk
/// somewhere else — which a recursive copy would.
fn promote(staging: &Path, target: &Path, id: &str) -> Result<(), String> {
    // A symlinked plugin directory is a developer's checkout, which `bundled`
    // supports on purpose. Setting one aside would rename the link and then
    // refuse to walk it, wedging every later update on the leftover; and the
    // alternative — walking it — would move their source tree. Neither is
    // something to do quietly.
    if target.is_symlink() {
        return Err(format!(
            "{} is a symlink, so it is somebody's working copy rather than an install",
            target.display()
        ));
    }

    // A set-aside copy lives beside staging, for the same reason staging is
    // not in `.moss/plugins/`: it is a second directory with the same manifest
    // name, and the scanners would load whichever readdir handed them first.
    // Named from the id rather than from `target`'s file name, so two plugins
    // can never land on one name and adopt each other's files.
    let displaced = staging.with_file_name(format!(".replaced-{id}"));
    let discard = staging.with_file_name(format!(".discard-{id}"));

    // A leftover from a run that was killed mid-swap, and the only copy of
    // whatever the plugin had written. It is finished here rather than
    // deleted: deleting it unread is how an interrupted update becomes a
    // silent logout on the next attempt.
    reconcile(&displaced, &discard, target);
    let leftover = match relative_paths(&displaced) {
        Ok(left) => !left.is_empty(),
        // Unreadable is not empty. `carry` has just refused to act on a
        // partial answer here for the same reason, and the two must agree:
        // the other branch of this `match` leads into `remove_dir_all`, which
        // deletes as it walks, so treating "could not tell" as "nothing to
        // lose" would unlink whatever it reached before hitting the same
        // obstacle. `symlink_metadata` rather than `exists` so that the other
        // way `relative_paths` errs — a symlinked `displaced` — is refused
        // too, and refused whether or not the link still resolves: `exists`
        // follows it, so a dangling one would read as absent and fall into the
        // very `remove_dir_all` this branch exists to avoid. Refusing is the
        // right answer for a link — `reconcile` has already declined to follow
        // it, and unlinking it is not this function's call to make.
        Err(_) => fs::symlink_metadata(&displaced).is_ok(),
    };
    if leftover {
        // Files still there means the handover could not finish, and it will
        // not finish on its own — the same obstacle is in the way. Said out
        // loud, before `target` is touched: the alternative is that the rename
        // below fails with ENOTEMPTY on every future update, naming neither
        // the leftover nor what to do about it. A husk holding only the empty
        // directories a completed carry left behind is not that, and is swept.
        return Err(format!(
            "an earlier update of {id} left files at {} that could not be moved into place; \
             move them aside by hand and try again",
            displaced.display()
        ));
    }
    let _ = fs::remove_dir_all(&displaced);

    if target.exists() {
        fs::rename(target, &displaced)
            .map_err(|e| format!("set the old {} aside: {e}", target.display()))?;
    }
    if let Err(e) = fs::rename(staging, target) {
        // Restored whole, not merged: putting the new archive's files into the
        // old directory would leave a version that never existed, and the old
        // receipt would then describe it wrongly forever.
        if displaced.exists() {
            let _ = fs::rename(&displaced, target);
        }
        return Err(format!("install {}: {e}", target.display()));
    }

    reconcile(&displaced, &discard, target);
    Ok(())
}

/// Finish with `displaced`: hand its contents to `target`, or become it.
///
/// Both ends of the swap use this, and so does a run that arrives to find a
/// `displaced` an earlier attempt never cleared — which is the point. The
/// cases are the ways a swap can be interrupted, and none of them needs to
/// know which one it is.
///
/// `discard` is what makes the last step safe to interrupt. [`carry`] is
/// best-effort by nature, so deleting `displaced` straight after it would
/// delete whatever the carry could not move; and deleting it *in place* would
/// destroy the receipt that says which of the remaining files matter, in an
/// order nobody chose. Instead a complete carry renames the husk to `discard`
/// — one atomic step that means "nothing in here is wanted" — and only then
/// removes it. A `discard` found on arrival is deleted unread, because that
/// rename already decided.
fn reconcile(displaced: &Path, discard: &Path, target: &Path) {
    let _ = fs::remove_dir_all(discard);
    if !displaced.exists() {
        return;
    }
    if !target.exists() {
        let _ = fs::rename(displaced, target);
        return;
    }
    if carry(displaced, target) && fs::rename(displaced, discard).is_ok() {
        let _ = fs::remove_dir_all(discard);
    }
}

/// Move everything `from` holds that the plugin wrote for itself into `to`,
/// reporting whether it managed all of it.
///
/// Three kinds of file live in an installed plugin directory: what the new
/// archive brings, what the last archive brought, and what the plugin wrote
/// while it ran. Only the third may cross. The second is code a new version
/// deleted on purpose — carrying it forward would restore a helper that was
/// removed because it was broken — and the last install's receipt is what
/// names it. (A file the last archive shipped and the plugin then edited in
/// place counts as the second kind and is replaced: an update is allowed to
/// change what it ships, and the plugin's own writes belong under names the
/// archive does not use.)
///
/// Without a receipt there is no way to tell the second kind from the third,
/// and the two mistakes are not symmetric: leaving stale code beside its
/// replacement is a bug, deleting the credentials a plugin stored is the
/// user's loss. So an unreceipted directory — a bundled install, or one from
/// before receipts — keeps everything the new archive did not bring.
///
/// Returns false if any file that should have crossed did not. The caller
/// deletes `from` only on true, because "skipped" and "deleted" must never be
/// the same outcome for a file only this directory holds. The two deliberate
/// skips — the new archive brings it, or the last one did — are not failures.
fn carry(from: &Path, to: &Path) -> bool {
    let Ok(present) = relative_paths(from) else {
        // A directory that cannot be read cannot be emptied safely. Leaving it
        // where it is loses nothing; walking a partial answer would delete
        // whatever the walk failed to see.
        return false;
    };
    let last_delivered: Option<HashSet<String>> = super::receipt::read(from)
        .and_then(|r| r.files)
        .map(|files| files.into_iter().collect());

    let mut complete = true;
    for path in present {
        let name = receipt_name(&path);
        if name == super::receipt::RECEIPT {
            continue;
        }
        let landing = to.join(&path);
        if landing.exists() {
            continue;
        }
        if last_delivered.as_ref().is_some_and(|files| files.contains(&name)) {
            continue;
        }
        let made_room = landing
            .parent()
            .is_none_or(|parent| fs::create_dir_all(parent).is_ok());
        if !made_room || fs::rename(from.join(&path), &landing).is_err() {
            complete = false;
        }
    }
    complete
}

/// Every file in `root`, relative to it.
///
/// Symlinks are listed but never followed — a link is a path to move, not a
/// directory to walk into — and a `root` that is itself a link is refused
/// outright, because `.moss/plugins/<id>` is allowed to be a symlink into a
/// developer's checkout and walking one would move their source tree.
///
/// A directory it cannot read is an error rather than an omission. Both
/// callers act on the answer being complete: the receipt records it as what an
/// archive laid down, and [`carry`] deletes what it does not mention.
fn relative_paths(root: &Path) -> Result<Vec<PathBuf>, String> {
    if root.is_symlink() {
        return Err(format!("{} is a symlink", root.display()));
    }
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
            let path = entry.map_err(|e| format!("read {}: {e}", dir.display()))?.path();
            let meta = fs::symlink_metadata(&path)
                .map_err(|e| format!("stat {}: {e}", path.display()))?;
            if meta.is_dir() {
                walk(root, &path, out)?;
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

/// How a path is spelled in a receipt: slash-separated, so a list written on
/// one platform still matches on another.
///
/// Only the receipt uses this spelling. Everything that touches the filesystem
/// works from the `PathBuf` the walk produced, because a name that does not
/// survive the round trip — a non-UTF-8 one, or a Unix file with a backslash
/// in it — would otherwise be looked up under a name it does not have and
/// silently left behind to be deleted.
fn receipt_name(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Where downloaded archives are kept between attempts.
///
/// App data, not the vault: a half-finished download is moss's business and
/// resuming it should work whichever folder is open. It is also not something
/// to sync — the vault's `.moss` is a synced folder for at least one real user.
fn download_dir() -> Result<PathBuf, String> {
    let dir = crate::infra::app_data::app_data_dir_early()
        .ok_or_else(|| "no app-data directory to download into".to_string())?
        .join("plugin-downloads");
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// The digest of `bytes`, spelled the way the registry spells it.
///
/// `hex::encode` rather than a hand-rolled loop: `large_download` — called one
/// statement earlier in `install_into` — already spells it that way, and two
/// spellings of one encoding inside a single call chain is one too many.
fn hex_sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::{hex_sha256, install_into, promote, read_verified, staging_dir};
    use crate::plugins::install::registry_client::index::IndexEntry;
    use crate::plugins::install::zip_extract::tests::zip_with_files;
    use std::fs;

    fn entry(id: &str, version: &str) -> IndexEntry {
        entry_with_sha256(id, version, &"0".repeat(64))
    }

    fn entry_with_sha256(id: &str, version: &str, sha256: &str) -> IndexEntry {
        serde_json::from_value(serde_json::json!({
            "type": "plugin",
            "id": id,
            "display_name": id,
            "version": version,
            "download_url": "https://example.invalid/x.zip",
            "sha256": sha256,
        }))
        .unwrap()
    }

    /// The receipt is what tells an update apart from a plugin's own writes.
    /// `a.js` is code the v1 archive shipped and v2 no longer does — dropped
    /// on purpose, and the old receipt is what names it as droppable.
    /// `data.json` is not in that receipt at all, so it is the plugin's own
    /// state and survives regardless of what either archive shipped.
    #[test]
    fn an_update_drops_code_the_last_receipt_named_and_keeps_what_the_plugin_wrote() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("plugins").join("stranger");
        let staging_root = root.path().join("staging");
        let staging = staging_root.join("stranger");

        // The live v1 install: its own code, the plugin's runtime data, and
        // the receipt v1's install wrote naming only the code as delivered.
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("a.js"), "v1 code").unwrap();
        fs::write(target.join("data.json"), "the plugin's own state").unwrap();
        super::super::receipt::write(&target, &entry("stranger", "1.0.0"), vec!["a.js".to_string()])
            .unwrap();

        // v2 ships different code and no longer includes a.js.
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("b.js"), "v2 code").unwrap();

        promote(&staging, &target, "stranger").expect("the update installs");

        assert!(target.join("b.js").exists(), "the new archive's code is live");
        assert!(
            !target.join("a.js").exists(),
            "code the last receipt named and this archive dropped is not carried forward"
        );
        assert_eq!(
            fs::read_to_string(target.join("data.json")).unwrap(),
            "the plugin's own state",
            "a file the receipt never named is the plugin's own write, and survives"
        );
    }

    /// A swap that cannot even begin must change nothing.
    ///
    /// The order is the whole point: the plugin's own files move into the live
    /// directory *after* the swap succeeds. Move them first — as an earlier
    /// version did — and a set-aside that fails returns with the user's
    /// credentials sitting in a staging directory the caller then deletes.
    #[cfg(unix)]
    #[test]
    fn a_swap_that_cannot_start_leaves_the_installed_plugin_whole() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        let target = plugins.join("stranger");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("manifest.json"), "{}").unwrap();
        fs::write(target.join("auth-state.json"), "the user's session").unwrap();

        let staging_root = root.path().join("staging");
        let staging = staging_root.join("stranger");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("manifest.json"), "{}").unwrap();

        // The set-aside renames `target` out of `plugins/`, which needs write
        // permission on both parents. Taking it from staging's parent is what
        // makes the rename fail without making the old directory unreadable.
        let locked = fs::metadata(&staging_root).unwrap().permissions();
        let mut readonly = locked.clone();
        readonly.set_mode(0o555);
        fs::set_permissions(&staging_root, readonly).unwrap();
        let outcome = promote(&staging, &target, "stranger");
        fs::set_permissions(&staging_root, locked).unwrap();

        assert!(outcome.is_err(), "the swap could not happen and must say so");
        assert_eq!(
            fs::read_to_string(target.join("auth-state.json")).unwrap(),
            "the user's session",
            "and the plugin's own files never left the installed directory"
        );
        assert!(target.join("manifest.json").exists(), "nor did its code");
    }

    /// What an update killed mid-swap leaves behind, and what the next one
    /// owes it.
    ///
    /// The set-aside copy is the only place the plugin's files exist during
    /// the swap. A run that finds one has to finish it — deleting it unread,
    /// which an earlier version did unconditionally, turns one interrupted
    /// update into a silent logout on the next attempt.
    #[test]
    fn an_update_killed_mid_swap_is_finished_by_the_next_one() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        let target = plugins.join("stranger");
        let staging_root = root.path().join("staging");
        let staging = staging_root.join("stranger");

        // Killed after the set-aside: no installed directory at all, and one
        // `.replaced-stranger` holding everything the user had.
        let orphan = staging_root.join(".replaced-stranger");
        fs::create_dir_all(&orphan).unwrap();
        fs::create_dir_all(&plugins).unwrap();
        fs::write(orphan.join("manifest.json"), "{}").unwrap();
        fs::write(orphan.join("auth-state.json"), "the user's session").unwrap();

        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("manifest.json"), "{}").unwrap();

        promote(&staging, &target, "stranger").expect("the next update installs");

        assert_eq!(
            fs::read_to_string(target.join("auth-state.json")).unwrap(),
            "the user's session",
            "the interrupted run's only copy was adopted, not deleted"
        );
        assert!(!orphan.exists(), "and nothing is left to adopt twice");
    }

    /// The case that needs the arrival check, and the one the swap cannot get
    /// past without it.
    ///
    /// Killed during the last step — the new code is already installed, and
    /// the set-aside copy still holds what the plugin wrote. Without finishing
    /// it on arrival, the next update's set-aside rename hits a non-empty
    /// `.replaced-<id>` and fails with ENOTEMPTY, which leaves the plugin
    /// permanently un-updatable as well as missing its state.
    #[test]
    fn an_update_killed_while_handing_files_over_is_finished_by_the_next_one() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("plugins").join("stranger");
        let staging_root = root.path().join("staging");
        let staging = staging_root.join("stranger");

        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("manifest.json"), "{}").unwrap();

        let orphan = staging_root.join(".replaced-stranger");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(orphan.join("auth-state.json"), "the user's session").unwrap();

        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("manifest.json"), "{}").unwrap();

        promote(&staging, &target, "stranger").expect("the next update installs");

        assert_eq!(
            fs::read_to_string(target.join("auth-state.json")).unwrap(),
            "the user's session",
            "the handover the killed run did not finish is finished here"
        );
        assert!(!orphan.exists(), "and the husk is gone once it is empty of anything wanted");
    }

    /// A carry that could not move everything must not be followed by a
    /// delete. The two outcomes for a file only the old directory holds are
    /// "moved" and "still there"; "skipped, then deleted" is neither.
    #[test]
    fn a_handover_that_could_not_finish_does_not_delete_what_it_left() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("plugins").join("stranger");
        let staging_root = root.path().join("staging");
        let staging = staging_root.join("stranger");

        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("manifest.json"), "{}").unwrap();

        // The old version kept its state under `data/`; the new one ships a
        // regular file by that name. Nothing hostile — a plugin changing how
        // it stores things between versions — and `create_dir_all` cannot make
        // room for the old path underneath it.
        let orphan = staging_root.join(".replaced-stranger");
        fs::create_dir_all(orphan.join("data")).unwrap();
        fs::write(orphan.join("data/auth-state.json"), "the user's session").unwrap();

        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("manifest.json"), "{}").unwrap();
        fs::write(staging.join("data"), "now a file").unwrap();

        promote(&staging, &target, "stranger").expect("the install itself succeeds");

        assert_eq!(
            fs::read_to_string(orphan.join("data/auth-state.json")).unwrap(),
            "the user's session",
            "what could not be handed over stays where it is, not deleted"
        );

        // And the next attempt says so. The obstacle has not moved, so the
        // handover will fail again — without this the set-aside rename below
        // it fails with ENOTEMPTY on every future update, naming neither the
        // leftover nor anything a person could act on.
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("manifest.json"), "{}").unwrap();
        let wedged = promote(&staging, &target, "stranger")
            .expect_err("an update that cannot finish must not look like one that did");
        assert!(
            wedged.contains(".replaced-stranger") && wedged.contains("by hand"),
            "the error names the leftover and what to do with it: {wedged}"
        );
    }

    /// A set-aside copy that cannot be read is not an empty one.
    ///
    /// [`carry`] already refuses to act on a directory it cannot walk, on the
    /// grounds that a partial answer deletes whatever the walk did not see.
    /// The leftover check asks the same question one line later and has to
    /// reach the same answer: "no files left" and "could not tell" are the
    /// same value out of `relative_paths` and must not be the same decision,
    /// because the second one leads straight into `remove_dir_all`.
    #[test]
    #[cfg(unix)]
    fn a_set_aside_copy_that_cannot_be_read_is_not_deleted_as_if_it_were_empty() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        let target = plugins.join("stranger");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("manifest.json"), "{}").unwrap();

        let staging_root = root.path().join("staging");
        let staging = staging_root.join("stranger");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("manifest.json"), "{}").unwrap();

        let displaced = staging_root.join(".replaced-stranger");
        fs::create_dir_all(&displaced).unwrap();
        fs::write(displaced.join("auth-state.json"), "the user's session").unwrap();
        let readable = fs::metadata(&displaced).unwrap().permissions();
        let mut sealed = readable.clone();
        sealed.set_mode(0o000);
        fs::set_permissions(&displaced, sealed).unwrap();

        let outcome = promote(&staging, &target, "stranger");
        fs::set_permissions(&displaced, readable).unwrap();

        assert_eq!(
            fs::read_to_string(displaced.join("auth-state.json")).unwrap(),
            "the user's session",
            "an unreadable leftover is left alone, not swept"
        );
        let refused = outcome.expect_err("and the install says why rather than proceeding");
        assert!(
            refused.contains(".replaced-stranger") && refused.contains("by hand"),
            "naming the leftover and what to do with it: {refused}"
        );
    }

    /// The one property that matters about where an archive is unpacked: it is
    /// not somewhere a plugin scanner looks. Three of them read
    /// `.moss/plugins/` and load any directory holding a `manifest.json`,
    /// whatever it is called, so an archive staged there is loadable before
    /// anything has judged it.
    #[test]
    fn staging_is_not_anywhere_plugins_are_loaded_from() {
        let vault = tempfile::tempdir().unwrap();
        let staging = staging_dir(vault.path().to_str().unwrap(), "stranger").unwrap();
        assert!(
            !staging.starts_with(vault.path().join(".moss").join("plugins")),
            "{} is under the directory plugins are loaded from",
            staging.display()
        );
        assert!(staging.is_dir(), "and it exists, ready to unpack into");
    }

    /// The read-back check cannot be reached through `install_into` — by the
    /// time it runs, `fetch_verified` has already guaranteed the file hashed
    /// correctly, and the window it closes is one no test can open on purpose.
    /// It is tested here instead, where the file can simply be wrong.
    #[test]
    fn bytes_are_returned_only_when_they_hash_to_what_was_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("plugin.zip");
        std::fs::write(&archive, b"the bytes that actually arrived").unwrap();

        let honest = hex_sha256(b"the bytes that actually arrived");
        assert_eq!(
            read_verified(&archive, &honest).unwrap(),
            b"the bytes that actually arrived",
            "the buffer that gets unpacked is the buffer that was hashed"
        );
        assert_eq!(
            read_verified(&archive, &honest.to_uppercase()).unwrap().len(),
            31,
            "the published hash's case is not a mismatch"
        );

        let wrong = hex_sha256(b"what the registry actually pinned");
        assert!(
            read_verified(&archive, &wrong).is_err(),
            "bytes that are not what review pinned are never returned"
        );
    }

    /// `install_into` end to end, with no network: `fetch_verified` returns
    /// immediately when `archive_dir` already holds a file matching the
    /// entry's pinned hash ([`cached_file_is_valid`], checked before any
    /// request), so pre-seeding the cache is what makes this reachable
    /// without a server. What matters is the line nothing else exercises —
    /// `super::receipt::write` inside `install_into`, between the unpack and
    /// the swap — so the assertion is the receipt on disk, read back through
    /// the same [`super::receipt::read`] a later update would use.
    ///
    /// [`cached_file_is_valid`]: crate::system::large_download::cached_file_is_valid
    #[test]
    fn install_into_writes_a_receipt_a_later_update_can_read() {
        let id = "stranger";
        let version = "1.0.0";
        let zip = zip_with_files(&[
            (
                "manifest.json",
                format!(r#"{{"name":"{id}","version":"{version}","entry":"main.js"}}"#)
                    .as_bytes(),
            ),
            ("main.js", b"module.exports = {};"),
        ]);
        let sha256 = hex_sha256(&zip);
        let entry = entry_with_sha256(id, version, &sha256);

        let archive_dir = tempfile::tempdir().unwrap();
        fs::write(archive_dir.path().join(format!("{id}-{version}.zip")), &zip).unwrap();
        let project = tempfile::tempdir().unwrap();

        let installed =
            install_into(&entry, project.path().to_str().unwrap(), archive_dir.path())
                .expect("a pre-cached, correctly-hashed archive installs without any network");
        assert_eq!(installed, version);

        let plugin_dir = project.path().join(".moss").join("plugins").join(id);
        assert!(plugin_dir.join("main.js").exists(), "the archive's own files land");

        let receipt = super::super::receipt::read(&plugin_dir)
            .expect("install_into wrote a receipt before the swap, so one is there to read");
        assert_eq!(receipt.id.as_deref(), Some(id));
        assert_eq!(receipt.version, version);
        assert_eq!(receipt.sha256, sha256);
        assert_eq!(
            receipt.receipt_on,
            super::super::receipt::ReceiptOn::Extract,
            "a plugin install is receipted at extract time, not bring-up"
        );
        assert_eq!(
            receipt.files.as_deref(),
            Some(&["main.js".to_string(), "manifest.json".to_string()][..]),
            "every path the archive laid down is what the next update tells apart from the plugin's own writes"
        );
    }
}
