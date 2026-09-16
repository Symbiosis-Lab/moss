//! What the last publish put on the live site, reconstructed from the
//! generation tree it left behind — and the one entry point a seal calls to
//! decide what the publish surfaces should say.
//!
//! # Why this exists
//!
//! Before the first publish to a target under the record format there is no
//! record for it under `.moss/deploy/records/`, so `change_set::classify(None,
//! …)` is blank: no verbs, and — until this module — no counts either. The
//! publish button then said *nothing at all*, which reads as "nothing to publish"
//! rather than "moss cannot tell you" (moss#993 defect 2).
//!
//! moss is not actually ignorant there. `.moss/state.toml` has carried
//! `last_deployed_generation_id` since 2026-06-15, generation directories are
//! immutable and normally survive GC (the last-deployed id is one of
//! `store_gc::gc_roots`' roots), so re-hashing the named tree reconstructs the
//! `files` map that publish sent. That is a true FILE-LEVEL upload/remove
//! count, computed locally, with no network.
//!
//! # What it deliberately does not reconstruct
//!
//! Verbs. `sources` and `source_to_output` are page-level state that a
//! generation directory does not contain and no per-generation snapshot
//! retains, so the result stays `classified: false`. Inventing a verb from a
//! partial record is the one thing the change-set surfaces must never do — a
//! confident wrong "413 edited" is not correctable by the person reading it.
//!
//! # Why the tree round-trips to manifest entries
//!
//! A manifest entry is `<octal-mode>:<hash>` and both halves are derivable from
//! the tree alone:
//!
//! - **Regular file** → `100644:` + xxh3_64 of the bytes, exactly what
//!   `register_with_hash` writes via `types::file_entry`.
//! - **Symlink** → `120000:` + sha256 of the *target string*, read with
//!   `read_link` and never dereferenced — the same value `types::symlink_entry`
//!   produces at build time and `deploy.rs` recomputes to verify an upload.
//!
//! The hash *domain* also matches, which is the non-obvious half. A
//! generation's HTML is the annotation-stripped copy of staging's, and so is
//! its manifest entry: `emit::slots` re-registers every page it injects with
//! `ship::apply_transform`-ed bytes. Deploy depends on that already — it
//! byte-verifies each upload read out of the generation directory against the
//! manifest hash, and would refuse every page otherwise.
//!
//! Two known imprecisions, both bounded and both in the direction of
//! over-reporting rather than under-reporting a publish:
//!
//! - `ship_phase` walks the filesystem rather than the manifest, so a
//!   generation can hold a file the manifest never listed (see
//!   `docs/reference/deploy-generations.md`). Such a file was never uploaded,
//!   but reads here as one removal.
//! - A page that slot injection did not touch keeps its pre-strip hash in the
//!   manifest and so reads as one upload.
//!
//! # Failure is silence, never a stopped build
//!
//! Every read here is advisory: the answer decorates a button. A pruned
//! generation, an unreadable file, a `state.toml` with no pointer — all degrade
//! to `None` and the caller falls back to today's honest blank. That is why
//! this module returns `Option` instead of `build::outcome`'s `BuildStopped`:
//! there is no reading of "would waiting help?" that should ever hold up a
//! seal for the sake of a count.
//!
//! Failure is also *remembered* — see [`MEMO`]. A walk that fails costs a full
//! tree traversal to produce nothing, and seals recur on every rebuild, so
//! retrying the same evicted generation on each one is a per-keystroke tax with
//! no upside.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use super::change_set::{self, ChangeSet};
use super::{published_record, SealedManifest};
use crate::build::ports::host::ServerDiff;
use crate::build::assets::paths::compute_binary_hash_file;
use crate::moss_paths::MossPaths;
use crate::types::content::{file_entry, symlink_entry};

/// Output path → mode-tagged manifest entry: the shape of
/// [`SealedManifest::files`], which is what makes a reconstruction directly
/// comparable to one.
type FileSet = HashMap<String, String>;

/// Which sealed manifest this seal tail may describe to the publish surfaces.
///
/// Two independent refusals, and neither of them means "there is nothing to
/// publish" — both mean "not from here":
///
/// - `!mat_ok`: the generation never reached disk, so it is never advertised to
///   deploy. What deploy would publish is the manifest advertised *before* it,
///   which the previously stashed change set already describes correctly.
/// - `!owns_shared`: a NEWER build already promoted, and already published its
///   own correct change set. An older tail writing over it leaves the two
///   disagreeing — the same rule `ship::tail_owns_shared_state` states for
///   `hashes.json` and the stale sweep.
///
/// The second term is redundant today (`Promotion::Promoted` is the only
/// `mat_ok`, and it is never `Superseded`) and is written out anyway: this is
/// the site where an older tail could silently overwrite shared state, and the
/// rule should be visible here rather than inferred from `Promotion`'s variants.
pub fn tail_speaks(
    sealed: &SealedManifest,
    mat_ok: bool,
    owns_shared: bool,
) -> Option<&SealedManifest> {
    (mat_ok && owns_shared).then_some(sealed)
}

/// What the local arms of [`for_seal`] decided.
///
/// `AskServer` means both local sources came up empty — no publish record, no
/// generation on disk — and the caller may still ask the host's
/// [`ServerDiff`] port. Split out of `for_seal` deliberately: the network ask
/// must run AFTER the sealed manifest has been advertised to deploy, so a
/// slow server can delay only the button decoration, never a publish. The
/// caller with no port turns `AskServer` into `ChangeSet::default()` — the
/// same honest blank this state produced before the port existed.
pub enum SealVerdict {
    Ready(ChangeSet),
    AskServer,
}

/// The change set a completed seal advertises, or `None` when this tail has
/// nothing to say — see [`tail_speaks`], which is how callers decide.
///
/// `None` never means "clear the stash": the standing answer describes the
/// manifest deploy would still publish, and the two are a pair
/// (`AppState::current_change_set`).
///
/// The record is the preferred input — it is the only one that can carry
/// verbs. The backfill is the fallback; the host's [`ServerDiff`] port
/// ([`from_server`], deferred — see [`SealVerdict`]) the fallback's fallback;
/// and the blank default last, so a site with no record, no generation on
/// disk, and no server to ask lands exactly where it landed before this
/// module existed.
///
/// The tree walk goes to the blocking pool: it hashes every file of a whole
/// generation, which on a large vault is thousands of reads.
pub async fn for_seal(
    mp: &MossPaths,
    describable: Option<&SealedManifest>,
) -> Option<SealVerdict> {
    let sealed = describable?;
    let target = crate::build::site_config::get_domain_config(
        &mp.project_root().to_string_lossy(),
    )
    .ok()
    .and_then(|c| c.publish_target());
    if let Some(record) = published_record::load_for(mp, target.as_deref()) {
        return Some(SealVerdict::Ready(change_set::classify(Some(&record), sealed)));
    }
    let project_root = mp.project_root().to_path_buf();
    let previous = tokio::task::spawn_blocking(move || {
        published_files(&MossPaths::new(&project_root))
    })
    .await
    .unwrap_or(None);
    Some(match previous {
        Some(files) => SealVerdict::Ready(change_set::flat_against(&files, sealed)),
        None => SealVerdict::AskServer,
    })
}

/// The server's own `(need, remove)` for a sealed manifest — the arm for a
/// vault with NO local trace of its last publish: a re-cloned vault whose
/// `state.toml` was gitignored and whose generations are gone, but whose site
/// is live (the CPHS vault, moss#993's worst case). The counts pass through
/// [`change_set::flat`] and stay verbless.
///
/// Advisory like everything here: no answer (host refusal, network failure,
/// timeout) degrades to the blank default. And because the caller runs this
/// only AFTER advertising the manifest to deploy, a slow server delays the
/// stash refresh, never a publish.
///
/// The counts are the sync endpoint's, with its caveats: a partial upload of
/// this exact manifest staged on the server shrinks `need` to the remainder
/// (resume), so the badge can briefly under-report until that deploy
/// finishes or is swept.
///
/// One ask per sealed generation, remembered in [`SERVER_MEMO`] — which
/// bounds re-seals of UNCHANGED content only. Every content edit mints a new
/// generation id and so a new ask; the host's port is expected to keep each
/// ask cheap and bounded rather than rely on this memo for rate limiting.
pub async fn from_server(
    port: &ServerDiff,
    files: FileSet,
    gen_id: String,
) -> ChangeSet {
    let counts = match SERVER_MEMO.get(&gen_id) {
        Some(hit) => hit,
        None => {
            let counts = port(files, gen_id.clone()).await;
            SERVER_MEMO.put(&gen_id, &counts);
            counts
        }
    };
    match counts {
        Some((need, remove)) => change_set::flat(need, remove),
        None => ChangeSet::default(),
    }
}

/// Output-path → manifest entry for the generation `state.toml` names as the
/// last deployed one, or `None` when there is nothing trustworthy to rebuild
/// from (no pointer, pruned directory, unreadable file).
///
/// **Blocking.** Walks and hashes an entire generation tree — at most once per
/// generation, in either direction; see [`MEMO`].
pub fn published_files(mp: &MossPaths) -> Option<FileSet> {
    published_files_memoized(&MEMO, mp)
}

/// Testable core of [`published_files`] with the memo passed in, so the
/// remember-the-failure behaviour can be asserted without reaching into a
/// process-global that other tests share (and evict from).
fn published_files_memoized(memo: &Memo<PathBuf, FileSet>, mp: &MossPaths) -> Option<FileSet> {
    let project = mp.project_root().to_string_lossy().to_string();
    let gen_id = crate::build::site_config::get_domain_config(&project)
        .ok()?
        .last_deployed_generation_id?;
    let dir = mp.generation_dir(&gen_id);
    if let Some(hit) = memo.get(&dir) {
        return hit;
    }
    let files = hash_tree(&dir);
    memo.put(&dir, &files);
    files
}

/// The last reconstruction attempt, keyed by its generation directory.
///
/// The negative entry is scoped to the directory that failed, so it is never
/// a process-wide surrender: a later publish moves
/// `last_deployed_generation_id`, which names a different directory, which is
/// a different key — and by then a publish record exists and this path is not
/// taken at all. And the failures here are evictions, which end — usually
/// within seconds, because the provider is already downloading — so the
/// [`FAILURE_RETRY_AFTER`] expiry means the answer comes back on its own once
/// the files land, instead of waiting for a folder switch or an app restart.
///
/// What a hit guarantees is bounded, not absolute. A generation id hashes the
/// sealed manifest's `files` map, while `ship_phase` walks the *filesystem*, so
/// two builds with identical manifests share an id — and therefore one
/// directory — while differing in leftovers the manifest never listed. A hit
/// can be one walk behind on exactly those leftovers, and every leftover reads
/// as a removal, so the whole error is a `flat_remove` count off by the
/// leftover delta. That is the same over-reporting direction the module docs
/// already accept for the same reason, and it is not worth re-walking a few
/// thousand files on every seal to shave.
static MEMO: LazyLock<Memo<PathBuf, FileSet>> = LazyLock::new(Memo::default);

/// The last server-diff answer, keyed by generation id — the same
/// outcome-not-just-success shape as [`MEMO`], for the same per-keystroke
/// reason: seals recur on every rebuild, and this arm costs a network round
/// trip where the other costs a tree walk. A generation id hashes the sealed
/// manifest, so a hit is exact (the server's answer for these bytes), and a
/// remembered failure expires after [`FAILURE_RETRY_AFTER`] like any other.
/// Note what this does NOT bound: a content edit mints a new generation id,
/// so the editing flow asks once per distinct content state — the per-ask
/// cost is bounded at the port (single attempt, short timeout), not here.
static SERVER_MEMO: LazyLock<Memo<String, (usize, usize)>> = LazyLock::new(Memo::default);

/// The one-slot cache behind [`MEMO`] and [`SERVER_MEMO`]. A type rather than
/// a bare static so its hit and eviction behaviour is testable without a
/// process-global; generic over key and value because the two memoized
/// questions differ only in what they ask, not in how the answer is kept.
///
/// One slot, not a map: only the folder in the foreground asks, so a second
/// key evicts the first and pays one recomputation — cheaper than pinning
/// answers per folder for the whole session.
///
/// The doubled `Option` is the point: the outer one is "this key has been
/// tried", the inner one is "and it worked". A failure is remembered too
/// (with its time, so it expires — see [`FAILURE_RETRY_AFTER`]), and that is
/// the load-bearing half: without a negative entry every seal re-pays the
/// full cost of re-failing.
struct Memo<K, V> {
    slot: Mutex<Option<(K, Option<V>, Option<std::time::Instant>)>>,
}

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self { slot: Mutex::new(None) }
    }
}

/// How long a failed answer is trusted before it is worth trying again.
///
/// What makes a walk fail is an eviction, and what makes a server ask fail is
/// a network blip — both TRANSIENT. Remembering the failure forever would
/// mean the button stays blank after the cause clears, for the rest of the
/// session, which is the very symptom this module exists to remove.
/// Remembering it for a minute is enough: seals fire on every
/// keystroke-driven rebuild, so the cost this bounds is per-keystroke, not
/// per-minute.
const FAILURE_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

impl<K: PartialEq + Clone, V: Clone> Memo<K, V> {
    fn get(&self, key: &K) -> Option<Option<V>> {
        let guard = self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_, value, failed_at) = guard.as_ref().filter(|(k, _, _)| k == key)?;
        if failed_at.is_some_and(|t| t.elapsed() >= FAILURE_RETRY_AFTER) {
            return None; // stale failure — recompute and let the source answer
        }
        Some(value.clone())
    }

    fn put(&self, key: &K, value: &Option<V>) {
        let mut guard = self.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some((
            key.clone(),
            value.clone(),
            value.is_none().then(std::time::Instant::now),
        ));
    }
}

/// Re-derive manifest entries for every file under `dir`.
///
/// All-or-nothing on purpose. A partially-read tree reports every file it could
/// not hash as an upload, which is a wrong number presented with the same
/// confidence as a right one; `None` at least routes to a surface that says it
/// does not know.
fn hash_tree(dir: &Path) -> Option<FileSet> {
    if !dir.is_dir() {
        return None;
    }
    let mut files = HashMap::new();
    for entry in walkdir::WalkDir::new(dir) {
        let entry = give_up_on_err(entry, dir)?;
        let ft = entry.file_type();
        if ft.is_dir() {
            continue;
        }
        // Manifest keys are served paths: root-relative, forward slashes.
        let rel = entry.path().strip_prefix(dir).ok()?.to_str()?;
        let value = if ft.is_symlink() {
            // `read_link`, not a deref: the entry hashes the target STRING, and
            // the target may legitimately not resolve.
            let target = give_up_on_err(std::fs::read_link(entry.path()), dir)?;
            symlink_entry(&target.to_string_lossy())
        } else {
            file_entry(&give_up_on_err(compute_binary_hash_file(entry.path()), dir)?)
        };
        files.insert(rel.replace('\\', "/"), value);
    }
    Some(files)
}

/// Log once and abandon the reconstruction. Kept as a helper so every failure
/// mode in `hash_tree` says which generation went unreconstructed — the whole
/// point of the log line is that the count silently reverting to zero is
/// otherwise indistinguishable from a site with nothing to publish.
fn give_up_on_err<T, E: std::fmt::Display>(result: Result<T, E>, dir: &Path) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            log::info!(
                "publish change set: cannot re-hash the last deployed generation at {} ({e}); \
                 the resting count stays unknown",
                dir.display()
            );
            None
        }
    }
}

#[cfg(test)]
#[path = "backfill_tests.rs"]
mod tests;
