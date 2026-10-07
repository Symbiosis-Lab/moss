//! Retention and garbage collection for the local build store (`.moss/build.nosync/`),
//! and the generation-directory bookkeeping both share: the write lock, and
//! what a directory holds against its manifest.
//!
//! **Pure I/O, no `tauri`, no app-side singletons** — every entry point takes
//! plain paths and plain data, so this module moves into `crates/moss-build`
//! (which owns cache/CAS and lifecycle by charter, and is CI-asserted
//! tauri-free) unchanged at M6a. Same discipline as `io_utils`, and the reason
//! the config knob is read by `build::site_config` and passed *in* rather than
//! looked up here: `config.toml` has one reader by charter.
//!
//! Two collectors live here, with different shapes and different payoffs:
//!
//! - **Generations** — [`gc_old_generations`] keeps the newest `n` generation
//!   directories plus an explicit root set. On a copy-on-write filesystem the
//!   retained ones are nearly free (`ship` materializes via `fs::copy`, which
//!   reflinks); on ext4 they cost their full byte size, which is why
//!   [`effective_keep_generations`] clamps `n` when the probe says there is no
//!   reflink. See [`supports_cow`].
//!
//! - **The content-addressed cache** — `cache::gc` is a correct mark-and-sweep
//!   that used to have no automatic caller at all: the only entry point
//!   was the `run_cache_gc` Tauri command, which nothing in the app's UI ever
//!   invoked. Measured on a real site: 10,143 objects on disk against 923
//!   referenced — ~0.5 GB of genuinely unreachable blobs, growing forever.
//!   [`maybe_gc_cache`] supplies the missing trigger, modelled on git's
//!   `gc.auto`.
//!
//! ## Why the cache trigger needs a watermark
//!
//! A bare "more than N objects" threshold either never fires on a small site or
//! fires on *every* build of a large one, because the reachable set of a big
//! site legitimately exceeds any fixed N. git solves this by recording how many
//! loose objects survived the last collection and re-collecting only when the
//! store has grown substantially past that. [`GcWatermark`] is that record, so
//! the store settles: after a sweep the watermark IS the reachable set, and the
//! next sweep waits until the store has roughly doubled.
//!
//! ## Invariants
//!
//! - **GC must never run concurrently with a build.** `cache::gc` assumes a
//!   quiescent cache; sweeping while a build is about to reference a blob would
//!   delete it. Callers hold the per-folder `stage_write_lock`.
//! - **The retention floor is 2, not 1 or 0.** `build/pipeline.rs` serves the
//!   previous generation for zero-flicker preview while `staging/` is rewritten,
//!   and `current` must never dangle during materialize.
//! - **Roots are enumerated, never inferred.** Every generation some other
//!   subsystem may open must appear in the set passed to [`gc_old_generations`].
//!   The non-obvious one is `last_deployed_generation_id` — see
//!   [`gc_roots`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::cache;

/// Generation directories retained by default.
///
/// **2 is the floor, not the default.** One generation is impossible: the
/// preview serves the previous generation while `staging/` is rewritten, so
/// live-plus-one is structurally required. The third buys the case where you
/// roll back and discover the previous generation was also bad — cheap on a COW
/// filesystem, and the only reason to keep more than the floor.
///
/// It is deliberately NOT larger. moss is a client that owns the source and has
/// git; deep history of a *derived* tree is not moss's job, and only the newest
/// generation is ever read (a full reader inventory found this resolves
/// `current_ptr()`, with one exception — see [`gc_roots`]). The prior value was
/// 5, a hardcoded literal with no knob, matching Capistrano's `keep_releases`
/// default — but Capistrano retains on the *server*, where the source is absent
/// and rollback is the only recovery. Borrow the knob, reject the default.
pub const KEEP_GENERATIONS_DEFAULT: usize = 3;

/// Hard floor for generation retention — see [`KEEP_GENERATIONS_DEFAULT`].
///
/// A user who writes `keep_generations = 0` still gets 2. Below this, `current`
/// dangles mid-materialize and the preview flickers.
pub const KEEP_GENERATIONS_FLOOR: usize = 2;

/// Object count below which the cache is never swept automatically.
///
/// A first sweep on a store this small would free a trivial amount and cost a
/// full directory walk. Roughly git's `gc.auto` order of magnitude.
const CACHE_GC_MIN_OBJECTS: usize = 2_048;

/// Growth headroom over the post-sweep watermark before sweeping again:
/// `trigger = watermark * FACTOR + SLACK`. The additive slack keeps a site whose
/// reachable set is tiny from re-sweeping on every build.
const CACHE_GC_GROWTH_FACTOR: usize = 2;
const CACHE_GC_GROWTH_SLACK: usize = 512;

// ---------------------------------------------------------------------------
// Generation retention
// ---------------------------------------------------------------------------

/// Resolve how many generations to keep, honouring the config knob, the floor,
/// and the copy-on-write probe.
///
/// `configured` is `[build].keep_generations` as read by
/// `build::site_config::get_build_keep_generations`, or `None` when unset.
///
/// The COW clamp is the whole point of the probe: retaining generations is
/// cheap only when `fs::copy` reflinks. On ext4 — no reflink, silent full byte
/// copy — a 340 MB site pays ~1 GB for three generations where an APFS user
/// pays ~340 MB plus deltas, and `du` reports the same number on both, so the
/// difference is indistinguishable from normal operation until the disk fills.
pub fn effective_keep_generations(configured: Option<usize>, build_dir: &Path) -> usize {
    let n = configured.unwrap_or(KEEP_GENERATIONS_DEFAULT).max(KEEP_GENERATIONS_FLOOR);
    if n > KEEP_GENERATIONS_FLOOR && !supports_cow(build_dir) {
        log::info!(
            "generation retention: {} → {} — {} has no copy-on-write support, \
             so each retained generation costs its full size on disk",
            n,
            KEEP_GENERATIONS_FLOOR,
            build_dir.display()
        );
        return KEEP_GENERATIONS_FLOOR;
    }
    n
}

/// Assemble the GC root set.
///
/// Enumerated, never inferred. Three roots today:
///
/// 1. `current_gen_id` — the generation just materialized and pointed at.
/// 2. `pinned` — any generation an in-flight deploy is reading, so a GC storm
///    from several fast rebuilds cannot `remove_dir_all` the directory an upload
///    is streaming from.
/// 3. `last_deployed` — `last_deployed_generation_id` from `.moss/state.toml`.
///    This is the non-obvious one, and the reason this is a
///    correctness fix rather than a cleanup: `email/commands.rs`'s
///    publish-before-send gate resolves that id and checks the math PNGs exist
///    inside it. Once that generation aged past the retention window it was
///    deleted and the gate silently failed closed with "Publish the site first"
///    — for a site that *had* been published. It is the only reader in the
///    codebase that opens a non-current generation, and it was unprotected.
///    It also gates lowering retention at all: dropping 5 → 3 without this makes
///    the failure strictly more likely.
pub fn gc_roots(
    current_gen_id: &str,
    pinned: &HashSet<String>,
    last_deployed: Option<&str>,
) -> HashSet<String> {
    let mut roots = HashSet::new();
    roots.insert(current_gen_id.to_string());
    roots.extend(pinned.iter().cloned());
    if let Some(d) = last_deployed {
        roots.insert(d.to_string());
    }
    roots
}

/// Lock file for `generations_dir.join(gen_id)`, kept beside the directory
/// rather than inside it: the directory is what deploy walks and serves.
fn write_lock_path(generations_dir: &Path, gen_id: &str) -> PathBuf {
    generations_dir.join(format!(".{gen_id}.writing"))
}

/// An exclusive flock on a generation's `.<id>.writing` file. Moss has no
/// other cross-process lock on the build store, and a CLI build may run
/// beside the app on the same folder, so every writer and every removal of a
/// generation directory goes through this lock.
///
/// A writer takes it before creating the directory and calls
/// [`Self::finish`] only after its copy is complete and promoted. Dropping
/// it without finishing (a failed copy), or the process exiting mid-copy,
/// releases the lock but leaves the file, which is how GC recognises the
/// directory as abandoned. GC takes the same lock before removing any
/// directory, so a writer that re-derives an old id waits for the removal
/// to finish instead of copying into a directory being deleted.
pub struct GenerationWriteLock {
    /// The open lock file; `same_file::Handle` so identity can be checked
    /// against the path on every platform.
    handle: same_file::Handle,
    path: PathBuf,
    /// This lock created its file, so no earlier copy of this id was left
    /// unfinished. False when a file was found left behind, or when the
    /// filesystem cannot lock and nothing can be concluded.
    fresh: bool,
}

/// The outcome of [`GenerationWriteLock::take`].
enum Take {
    /// Locked, on the file currently at the path.
    Locked(GenerationWriteLock),
    /// Another handle holds the lock.
    Busy,
    /// The file exists but this filesystem cannot lock at all; `true` when
    /// this call created it.
    Unlockable(GenerationWriteLock, bool),
}

impl GenerationWriteLock {
    pub(crate) fn try_acquire(generations_dir: &Path, gen_id: &str) -> std::io::Result<Option<Self>> {
        match Self::take(generations_dir, gen_id, false)? {
            Take::Locked(lock) => Ok(Some(lock)),
            Take::Busy => Ok(None),
            Take::Unlockable(_, _) => Err(std::io::Error::other("generation identities require a lock")),
        }
    }

    /// Take `gen_id`'s write lock for a copy, waiting while anyone else holds
    /// it. If the filesystem cannot lock at all, the file is still created
    /// and the copy proceeds: GC cannot lock there either, so it keeps the
    /// directory. Any other lock failure is returned, failing the copy
    /// before it starts and leaving `current` where it was.
    pub fn acquire(generations_dir: &Path, gen_id: &str) -> std::io::Result<Self> {
        crate::build::io_utils::create_output_dir_all(generations_dir)?;
        match Self::take(generations_dir, gen_id, true)? {
            Take::Locked(lock) | Take::Unlockable(lock, _) => Ok(lock),
            Take::Busy => Err(std::io::Error::other("generation lock reported busy on a blocking lock")),
        }
    }

    /// Open (creating if needed) and lock `gen_id`'s lock file, then check
    /// that the file locked is still the one at the path: a holder that
    /// finished may have unlinked it between our open and our lock, and a
    /// lock on an unlinked file protects nothing, so retry on a fresh one.
    fn take(generations_dir: &Path, gen_id: &str, wait: bool) -> std::io::Result<Take> {
        use fs2::FileExt;
        let path = write_lock_path(generations_dir, gen_id);
        loop {
            // allow:raw_write an empty lock file, never truncated or written
            let mut open = std::fs::OpenOptions::new();
            open.read(true).write(true);
            let (file, created) = match open.clone().create_new(true).open(&path) {
                Ok(file) => (file, true),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => match open.open(&path) {
                    Ok(file) => (file, false),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                },
                Err(e) => return Err(e),
            };
            let handle = same_file::Handle::from_file(file)?;
            let attempt = || if wait { handle.as_file().lock_exclusive() } else { handle.as_file().try_lock_exclusive() };
            #[cfg(test)]
            let attempt = || INJECTED_LOCK_ERRORS.with(|q| q.borrow_mut().pop_front()).map_or_else(attempt, Err);
            let locked = loop {
                match attempt() {
                    // A signal cut a blocking lock short: nothing is held yet.
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    result => break result,
                }
            };
            match locked {
                Ok(()) => {}
                // fs2's contended error, not a fixed ErrorKind: Windows reports
                // ERROR_LOCK_VIOLATION, which std does not map to WouldBlock.
                Err(e) if e.kind() == fs2::lock_contended_error().kind() => return Ok(Take::Busy),
                Err(e) if wait && !cannot_lock_here(&e) => return Err(e),
                Err(e) => {
                    log::warn!("generation {gen_id}: cannot lock {}: {e}", path.display());
                    return Ok(Take::Unlockable(Self { handle, path, fresh: false }, created));
                }
            }
            if same_file::Handle::from_path(&path).is_ok_and(|at_path| at_path == handle) {
                return Ok(Take::Locked(Self { handle, path, fresh: created }));
            }
        }
    }

    /// Whether no copy of this generation was cut off before this lock was
    /// taken: an existing directory is then one a copy finished (or one
    /// that predates the lock), never one a copy abandoned.
    pub fn fresh(&self) -> bool {
        self.fresh
    }

    /// Whether `gen_dir` already holds the generation whose manifest entries
    /// are `files`, so that promoting it needs no copy. A save that changes
    /// nothing, or an edit and its undo, re-derives a generation that is
    /// already on disk — often the one `current` serves — and copying it
    /// again rewrites every file in it for nothing.
    ///
    /// The id is a hash of every entry's path and content hash, so a directory
    /// named for it was copied from the same entries. It holds them all when
    /// no copy of it was cut off or shipped bytes that drifted from the
    /// manifest (either leaves the lock file behind, so [`Self::fresh`] is
    /// false) and every entry is present: each file lands by rename, so a
    /// present file is whole. The presence check also lets a generation that
    /// shipped without an evicted `_moss/math/` PNG pick it up on a later seal.
    /// Asked under this lock, so no GC can remove the directory between this
    /// answer and the promotion.
    pub fn holds(&self, gen_dir: &Path, files: &std::collections::HashMap<String, String>) -> bool {
        self.fresh && dir_holds(gen_dir, files, cfg!(windows))
    }

    /// Seed this lock's generation directory `gen_dir`, still absent, with a
    /// copy-on-write clone of `base`, a finished generation beside it, so the
    /// copy that follows writes only the entries the two do not share. One
    /// `clonefile(2)` replaces one copy per file, which on a site of a few
    /// thousand outputs is most of what sealing a one-page edit costs.
    ///
    /// The base's lock must be free and its lock file one this call made, so
    /// no copy of it is running or was cut off or drifted, and it must still be the directory `base` recorded, not one
    /// another process removed and copied again under the same id. Its lock is
    /// taken without waiting and held across the check and the clone, so no GC
    /// removes it meanwhile. Returns whether `gen_dir` now holds the clone; a
    /// failed clone may leave part of one, which [`prune_to_manifest`] and a
    /// full copy then put right.
    pub fn seed_from(&self, gen_dir: &Path, base: &WholeGeneration) -> bool {
        let Some(generations_dir) = gen_dir.parent() else { return false };
        let base_dir = generations_dir.join(&base.id);
        if base_dir == gen_dir || std::fs::symlink_metadata(gen_dir).is_ok() {
            return false;
        }
        let lock = match Self::take(generations_dir, &base.id, false) {
            Ok(Take::Locked(lock)) if lock.fresh() => lock,
            // Left behind, a file this call made would mark the base abandoned.
            Ok(Take::Unlockable(lock, true)) => {
                lock.finish();
                return false;
            }
            _ => return false,
        };
        let cloned = dir_identity(&base_dir).is_some_and(|dir| dir == base.dir)
            && crate::build::io_utils::clone_output_dir(&base_dir, gen_dir)
                .inspect_err(|e| log::debug!("generation {}: not cloned into {}: {e}", base.id, gen_dir.display()))
                .is_ok();
        lock.finish();
        cloned
    }

    /// Done with the generation: remove the lock file while still holding
    /// it, then release. For a writer this marks the copy complete.
    pub fn finish(self) {
        let Self { handle, path, .. } = self;
        // allow:unlink this generation's own lock file, while its lock is held
        if let Err(e) = std::fs::remove_file(&path) {
            log::warn!("failed to remove {}: {e}", path.display());
        }
        drop(handle);
    }
}

/// Every entry of `files` is present in `gen_dir`. Never, when
/// `copies_link_targets` and any entry is a symlink: Windows ships a symlink's
/// target bytes, but its manifest entry hashes only the target path, so an
/// edit inside a linked folder leaves the id unchanged.
fn dir_holds(gen_dir: &Path, files: &std::collections::HashMap<String, String>, copies_link_targets: bool) -> bool {
    gen_dir.is_dir()
        && files.iter().all(|(rel_path, entry)| {
            let (mode, _) = crate::types::content::parse_entry(entry);
            !(copies_link_targets && mode == crate::types::content::MODE_SYMLINK)
                && crate::build::io_utils::entry_output_present(&gen_dir.join(rel_path), mode)
        })
}

/// A generation as it was when a copy of it finished: what a later copy may be
/// seeded from. Nothing stops another process, or a person, from changing a
/// generation's files after that, so its directory's identity and each file's
/// size and mtime are kept, and a seed that no longer matches is not trusted.
pub struct WholeGeneration {
    id: String,
    dir: DirIdentity,
    files: std::collections::HashMap<String, Finished>,
}

/// One file of a [`WholeGeneration`]: its manifest entry, and its file's stat.
struct Finished {
    entry: String,
    size: u64,
    mtime: Option<std::time::SystemTime>,
}

impl WholeGeneration {
    /// Stat every file of `gen_id`, a whole generation at `gen_dir` whose
    /// manifest entries are `files`. An entry with no file — the `_moss/math/`
    /// exemption — is left out, so it is never taken as held. `None` where the
    /// directory has no identity to check, so no copy is seeded from it.
    pub fn record(gen_dir: &Path, gen_id: &str, files: &std::collections::HashMap<String, String>) -> Option<Self> {
        let files = files
            .iter()
            .filter_map(|(rel, entry)| {
                let meta = std::fs::symlink_metadata(gen_dir.join(rel)).ok()?;
                Some((rel.clone(), Finished { entry: entry.clone(), size: meta.len(), mtime: meta.modified().ok() }))
            })
            .collect();
        Some(Self { id: gen_id.to_string(), dir: dir_identity(gen_dir)?, files })
    }

    /// Whether `path`, the seeded copy of `rel_path`, still holds `entry`'s
    /// bytes: this generation finished `rel_path` as `entry`, and the file is
    /// present with the size and mtime it had then (a clone keeps both). An
    /// edit since, or a dataless or evicted file, is shipped again. Presence is
    /// the one predicate every generation check asks, given the stat taken
    /// here; every ancestor of `path` is a real directory, because
    /// [`prune_to_manifest`] removes any link that is not itself an entry.
    pub fn still_holds(&self, rel_path: &str, entry: &str, path: &Path) -> bool {
        let Some(finished) = self.files.get(rel_path) else { return false };
        let stat = std::fs::symlink_metadata(path);
        finished.entry == entry
            && stat.as_ref().is_ok_and(|meta| meta.len() == finished.size && meta.modified().ok() == finished.mtime)
            && crate::build::io_utils::probe_output_stat(path, crate::types::content::parse_entry(entry).0, stat)
                .is_present()
    }
}

/// A directory's device and inode: a directory removed and made again under
/// the same name has a different one.
#[derive(PartialEq)]
struct DirIdentity {
    device: u64,
    inode: u64,
}

/// `None` off unix, where nothing is seeded.
#[cfg(unix)]
fn dir_identity(dir: &Path) -> Option<DirIdentity> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(dir).ok().map(|meta| DirIdentity { device: meta.dev(), inode: meta.ino() })
}

#[cfg(not(unix))]
fn dir_identity(_dir: &Path) -> Option<DirIdentity> {
    None
}

/// Remove from `gen_dir` every file and link `files` does not name, and every
/// directory that leaves empty, so a promoted generation holds exactly its
/// manifest. A seeded directory holds its base's outputs, and a copy cut off
/// after seeding leaves them for the next copy of the same id.
pub fn prune_to_manifest(gen_dir: &Path, files: &std::collections::HashMap<String, String>) -> std::io::Result<()> {
    prune_dir(gen_dir, "", files).map(|_| ())
}

/// [`prune_to_manifest`] below `dir`, whose manifest path is `rel`. Returns
/// whether `dir` is empty afterwards.
fn prune_dir(dir: &Path, rel: &str, files: &std::collections::HashMap<String, String>) -> std::io::Result<bool> {
    let mut empty = true;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let child = match rel {
            "" => name.to_string_lossy().into_owned(),
            _ => format!("{rel}/{}", name.to_string_lossy()),
        };
        if entry.file_type()?.is_dir() {
            if !prune_dir(&entry.path(), &child, files)? {
                empty = false;
            } else {
                // allow:unlink a generation directory emptied of outputs its manifest does not name
                std::fs::remove_dir(entry.path())?;
            }
        } else if files.contains_key(&child) {
            empty = false;
        } else {
            // allow:unlink an output this generation's manifest does not name, before promote
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(empty)
}

/// Whether `e` means this filesystem has no working locks at all (a network
/// share without a lock manager), as opposed to one lock attempt failing.
/// Only then may a copy run unlocked, because every GC there fails the same
/// way and keeps the directory. Raw errnos as well as the kind: macOS's
/// `ENOTSUP` and `ENOLCK` map to no specific `ErrorKind`.
fn cannot_lock_here(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    if matches!(e.raw_os_error(), Some(libc::ENOTSUP | libc::EOPNOTSUPP | libc::ENOLCK)) {
        return true;
    }
    e.kind() == std::io::ErrorKind::Unsupported
}

#[cfg(test)]
thread_local! {
    /// Test-only: errors the next lock attempts on this thread return
    /// instead of locking, in order.
    pub(crate) static INJECTED_LOCK_ERRORS: std::cell::RefCell<std::collections::VecDeque<std::io::Error>> =
        const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
    /// Test-only: called by [`gc_old_generations`] just before it removes a
    /// directory, with that directory's lock held.
    pub(crate) static BEFORE_GENERATION_REMOVAL: std::cell::RefCell<Option<Box<dyn FnMut(&Path)>>> =
        const { std::cell::RefCell::new(None) };
}

/// List the generation-directory names present under `generations_dir`.
pub fn list_generations(generations_dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(generations_dir) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .collect()
}

/// Remove old generation directories, keeping the `n` most-recently-modified
/// plus everything in `roots`, and plus whatever `current_marker` names on
/// disk when the directory is about to go (another process may have promoted
/// it after `roots` was computed).
///
/// Every removal happens under the directory's [`GenerationWriteLock`]. A
/// directory whose lock is held, or cannot be checked, is kept. A directory
/// whose lock file was left behind by a copy that failed or was cut off is
/// removed even inside the `n`, unless it is a root: it would otherwise take
/// a retention slot from a real generation. A directory with no lock file is
/// finished or predates the lock, and gets ordinary retention; treating it
/// as abandoned could delete a copy an older moss binary is still running.
///
/// Errors are logged but non-fatal: the caller logs them as warnings so a GC
/// failure never invalidates a build that already succeeded.
///
/// *Known imprecision:* "the `n` most recent" means recency of the last copy,
/// not of the last promotion. A rebuild that re-derives an old id promotes the
/// directory already on disk without copying it (`ship::materialize_and_promote`),
/// so its mtime stays that of its first copy. Harmless: it is a root while
/// current, and past that its retention only buys a rollback.
pub fn gc_old_generations(
    generations_dir: &Path,
    current_marker: &Path,
    roots: &HashSet<String>,
    n: usize,
) -> std::io::Result<()> {
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(generations_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((mtime, e.path()))
        })
        .collect();

    // Sort newest first.
    entries.sort_by(|a, b| b.0.cmp(&a.0));

    let mut removed: Vec<String> = Vec::new();
    for (idx, (_, path)) in entries.iter().enumerate() {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if roots.contains(name) {
            continue; // pinned: current, in-flight deploy, or last-deployed
        }
        let retained = idx < n;
        if retained && std::fs::symlink_metadata(write_lock_path(generations_dir, name)).is_err() {
            continue; // finished and within retention: nothing to lock
        }
        let lock = match GenerationWriteLock::take(generations_dir, name, false) {
            // Its copy finished between the check above and the lock.
            Ok(Take::Locked(lock)) if retained && lock.fresh() => {
                lock.finish();
                continue;
            }
            Ok(Take::Locked(lock)) => lock,
            Ok(Take::Busy | Take::Unlockable(..)) | Err(_) => continue, // copying, or cannot tell
        };
        // Under the lock no copy of this id is running and none can start.
        match std::fs::read_to_string(current_marker) {
            Ok(current) if current.trim() == name => {
                lock.finish(); // promoted, so complete: drop any stale lock file
                continue;
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => continue, // cannot tell what is current: keep
        }
        #[cfg(test)]
        BEFORE_GENERATION_REMOVAL.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook(path);
            }
        });
        // allow:unlink an old or abandoned generation that is neither current,
        // pinned by a deploy, nor being indexed, removed under its write lock
        match crate::build::io_utils::remove_output_dir_all(path) {
            Ok(()) => {
                // allow:unlink the original identities of this removed generation
                let _ = std::fs::remove_file(crate::build::manifest::preview_originals::receipt_path(generations_dir, name));
                lock.finish();
                removed.push(name.to_string());
            }
            // Dropped without finishing: the lock file stays, so the next GC
            // retries this half-removed directory as abandoned.
            Err(e) => log::warn!("generation GC: failed to remove {:?}: {}", path, e),
        }
    }
    if !removed.is_empty() {
        let mut kept: Vec<&str> = roots.iter().map(String::as_str).collect();
        kept.sort_unstable();
        log::info!("generation GC: removed [{}], kept roots [{}]", removed.join(", "), kept.join(", "));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Cache GC trigger (git's `gc.auto`)
// ---------------------------------------------------------------------------

/// How many objects survived the last automatic sweep.
///
/// Persisted next to the cache so the trigger is stable across app restarts. A
/// missing or corrupt file reads as "never swept", which is the safe direction —
/// the worst case is one extra sweep.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
struct GcWatermark {
    objects_after_gc: usize,
}

fn watermark_path(build_dir: &Path) -> PathBuf {
    build_dir.join("cache").join("gc-watermark.json")
}

fn load_watermark(build_dir: &Path) -> Option<usize> {
    let raw = std::fs::read_to_string(watermark_path(build_dir)).ok()?;
    serde_json::from_str::<GcWatermark>(&raw)
        .ok()
        .map(|w| w.objects_after_gc)
}

fn save_watermark(build_dir: &Path, objects_after_gc: usize) {
    let Ok(json) = serde_json::to_string(&GcWatermark { objects_after_gc }) else {
        return;
    };
    // `.moss/build.nosync/**` is regenerable output, so this goes through io_utils:
    // a cloud-evicted destination is *absent*, not something to materialize.
    // A raw `fs::write` here would `EDEADLK` the build.
    if let Err(e) = super::io_utils::write_output(&watermark_path(build_dir), json.as_bytes()) {
        log::warn!("cache GC: failed to record watermark: {}", e);
    }
}

/// Count blobs in one sharded object store root.
///
/// The store is one fanout level deep (`objects/ab/cdef…`), so this walks the
/// fanout directories rather than recursing arbitrarily.
fn count_objects(objects_dir: &Path) -> usize {
    let Ok(fanout) = std::fs::read_dir(objects_dir) else {
        return 0;
    };
    fanout
        .filter_map(|e| e.ok())
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                std::fs::read_dir(&p).map(|d| d.count()).unwrap_or(0)
            } else {
                1
            }
        })
        .sum()
}

/// Decide whether the store has grown enough to be worth sweeping.
///
/// Split out as a pure function so the policy is testable without a filesystem —
/// the settling behaviour described in the module header is the thing worth
/// pinning, and it is entirely arithmetic.
fn should_gc_cache(objects_on_disk: usize, watermark: Option<usize>) -> bool {
    if objects_on_disk < CACHE_GC_MIN_OBJECTS {
        return false;
    }
    match watermark {
        None => true,
        Some(w) => objects_on_disk > w * CACHE_GC_GROWTH_FACTOR + CACHE_GC_GROWTH_SLACK,
    }
}

/// Run `cache::gc` if the object store has grown past its watermark and no
/// build or detached encode of the folder is writing it.
///
/// Returns the result when a sweep ran, `None` when the threshold was not met
/// or a writer holds a lease. A skipped sweep is never waited for: the next
/// seal tries again.
///
/// Blocking — walks and unlinks. Async callers wrap it in `spawn_blocking`.
pub fn maybe_gc_cache(mp: &crate::moss_paths::MossPaths) -> Option<cache::GcResult> {
    let build_dir = &mp.build_dir();
    let before = count_objects(&mp.cache_objects()) + count_objects(&mp.cache_local_objects());
    if !should_gc_cache(before, load_watermark(build_dir)) {
        return None;
    }
    let token = match crate::build::lifecycle::try_begin_cache_gc(mp) {
        Ok(token) => token,
        Err((builds, encodes)) => {
            log::info!("cache GC skipped: {} build and {} encode lease(s) open", builds, encodes);
            return None;
        }
    };

    let result = match cache::gc(mp, &token) {
        Ok(result) => result,
        Err(input) => {
            log::warn!("cache GC aborted: {} — nothing deleted", input);
            return None;
        }
    };
    let after = before.saturating_sub(result.objects_removed);
    save_watermark(build_dir, after);
    log::info!(
        "cache GC: {} → {} objects ({} removed, {} transforms, {} bytes freed)",
        before,
        after,
        result.objects_removed,
        result.transforms_removed,
        result.bytes_freed
    );
    Some(result)
}

// ---------------------------------------------------------------------------
// Copy-on-write probe
// ---------------------------------------------------------------------------

/// Does `dir`'s filesystem support reflink copies?
///
/// `ship::materialize_and_promote` copies a whole generation with `fs::copy`,
/// which the kernel turns into a reflink on APFS and on Btrfs/XFS — an
/// independent inode at near-zero disk cost. **ext4 has no reflink and silently
/// does a full byte copy.** Nothing in moss detected, asserted, logged, or fell
/// back, and `du` reports the same number either way, so the difference was
/// invisible until the disk filled.
///
/// Hardlinking is not the fallback — it is banned in the output tree because
/// iCloud "optimize storage" zeroes by inode, turning every hardlinked copy into
/// a 0-byte stub at once. The only lever is retaining fewer generations, which is
/// what [`effective_keep_generations`] does with this answer.
///
/// Probes by attempting the clone rather than matching a filesystem name, so a
/// filesystem moss has never heard of still gets the right answer. A probe that
/// cannot run at all reports "supported": that preserves the behaviour moss
/// shipped with, and a false positive costs disk rather than correctness.
pub fn supports_cow(dir: &Path) -> bool {
    probe_cow(dir).unwrap_or(true)
}

#[cfg(target_os = "linux")]
fn probe_cow(dir: &Path) -> Option<bool> {
    use std::os::unix::io::AsRawFd;

    // FICLONE — _IOW(0x94, 9, int). Clones src's extents into dst; fails with
    // EOPNOTSUPP (EINVAL on some stacks) when the filesystem has no reflink.
    const FICLONE: libc::c_ulong = 0x4009_4409;

    crate::build::io_utils::create_output_dir_all(dir).ok()?;
    let src = dir.join(".moss-cow-probe-src");
    let dst = dir.join(".moss-cow-probe-dst");
    // allow:raw_write probe scratch, not an output artifact — deleted below
    std::fs::write(&src, b"moss copy-on-write probe").ok()?;
    let src_f = std::fs::File::open(&src).ok()?;
    // allow:raw_write probe scratch, not an output artifact — deleted below
    let dst_f = std::fs::File::create(&dst).ok()?;
    let rc = unsafe { libc::ioctl(dst_f.as_raw_fd(), FICLONE, src_f.as_raw_fd()) };
    drop(src_f);
    drop(dst_f);
    // allow:unlink probe files this call created
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
    Some(rc == 0)
}

#[cfg(target_os = "macos")]
fn probe_cow(dir: &Path) -> Option<bool> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    crate::build::io_utils::create_output_dir_all(dir).ok()?;
    let c = CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let name: Vec<u8> = st
        .f_fstypename
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as u8)
        .collect();
    // APFS is the only Apple filesystem `fclonefileat` works on; an HFS+
    // external drive pays the full byte copy exactly like ext4.
    Some(name.eq_ignore_ascii_case(b"apfs"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn probe_cow(_dir: &Path) -> Option<bool> {
    // Windows: `std::fs::copy` uses `CopyFileEx`, which does not block-clone
    // even on ReFS. Anything else: unknown, and an unknown filesystem is far
    // more likely to lack reflink than to have it.
    Some(false)
}

#[cfg(test)]
#[path = "store_gc_tests.rs"]
mod tests;
