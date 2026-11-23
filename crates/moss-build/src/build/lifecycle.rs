//! The per-folder build lifecycle: which tree the preview serves, when a build
//! may unlink from staging, and which render `current` holds.
//!
//! One record per `.moss` directory, and the only writer of the served pointer.
//! A rebuild normally parks on `current` only after that generation catches up
//! with the render shown on staging; otherwise it leaves staging in place and
//! grants no sweep permit. When the selected route has a verified copy in
//! `current`, [`protect_current_before_rebuild`] may briefly show that older
//! complete copy while staging is rewritten. A ready new render returns the
//! preview to staging. [`Render::withdraw`] also returns to the older copy when
//! the visible render itself proves unreadable.
//!
//! Known gap: a process killed between a render and its seal, on a folder
//! that has never promoted, leaves pages in staging that no manifest names.
//! If such a page's source is then deleted while the folder is closed, the
//! next open has no `current` to park on and no recorded source to prove
//! gone, so the page is served until a later build gets a sweep permit.
//!
//! Nothing here is persisted. Every render number was minted by this process,
//! and the two that get compared are `Option`s that start `None`, so a number
//! from a dead process can never be compared with one from this one.

pub(crate) mod cas_heal;
pub(crate) mod tree_migration;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError, RwLock};

use crate::moss_paths::{MossPaths, Served};
use crate::types::runtime::SiteDirectoryState;

pub(crate) use crate::moss_paths::ServedCell;

#[derive(Default)]
struct FolderLifecycle {
    /// The cell [`adopt_server`] registered.
    served: Option<ServedCell>,
    /// Last render number minted by [`show_render`].
    next_render: u64,
    /// The newest render whose background work has sealed ([`Render::note_sealed`]).
    newest_sealed: Option<u64>,
    /// The render on the cell while it names staging; `None` when this record
    /// did not put it there.
    shown_render: Option<u64>,
    /// The render of the generation this process last promoted.
    current_render: Option<u64>,
    /// Highest promotion epoch that reached `current`.
    promoted_epoch: u64,
    /// The generation this process last left whole on disk, or the one read
    /// from the record kept beside it: what the next copy may be seeded from
    /// ([`whole_generation`]).
    whole: Option<Arc<crate::build::store_gc::WholeGeneration>>,
    /// Open [`CacheWriteLease`]s: builds between `run_pipeline` entry and the
    /// end of the seal tail's `materialize_and_promote`.
    build_writers: usize,
    /// Open [`DetachedCacheLease`]s: cache publishers and encodes that outlive their build.
    detached_writers: usize,
    /// A [`CacheGcToken`] is out.
    gc_running: bool,
}

/// One folder's record. Leases, permits and seal tails hold it, which is what
/// keeps [`lock_for`] from evicting it under them.
#[derive(Default)]
pub(crate) struct LifecycleCell {
    state: Mutex<FolderLifecycle>,
    /// Signalled when a cache GC ends, for writers waiting to start.
    changed: Condvar,
    /// Serializes the `current` swap, which on Windows copies a whole
    /// generation — so it is never taken under `state`.
    promote_lock: Mutex<()>,
}

impl LifecycleCell {
    fn state(&self) -> MutexGuard<'_, FolderLifecycle> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

static RECORDS: LazyLock<Mutex<HashMap<PathBuf, Arc<LifecycleCell>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The record for `mp`'s folder.
///
/// Drain on switch, like `FolderSessionRegistry`: looking up a folder with no
/// record first evicts every other record nothing else holds. A record a lease,
/// permit or seal tail still holds is kept until a later switch finds it idle —
/// evicting it would hand the next lookup a fresh record with no leases, and a
/// build would then read "nobody else is writing" while someone is.
pub(crate) fn lock_for(mp: &MossPaths) -> Arc<LifecycleCell> {
    {
        let records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(record) = records.get(mp.root()) {
            return record.clone();
        }
    }
    // Unlocked from here to the re-lock below, so the scratch clear's I/O
    // never holds every folder's lookup; the second `records.get` closes the
    // race two first lookups of one folder open.
    //
    // The first lookup of a folder in this process comes before anything here
    // writes its scratch, so what `cache/tmp` holds is a dead process's.
    static SCRATCH_CLEARED: LazyLock<Mutex<std::collections::HashSet<PathBuf>>> =
        LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));
    if SCRATCH_CLEARED.lock().unwrap_or_else(PoisonError::into_inner).insert(mp.root().to_path_buf()) {
        crate::build::io_utils::clear_scratch(&mp.cache_tmp());
    }
    let mut records = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(record) = records.get(mp.root()) {
        return record.clone();
    }
    records.retain(|_, record| Arc::strong_count(record) > 1);
    let record = Arc::new(LifecycleCell::default());
    records.insert(mp.root().to_path_buf(), record.clone());
    record
}

fn point(cell: &ServedCell, mp: &MossPaths, to: &Path) -> bool {
    SiteDirectoryState { current_dir: cell.clone() }.switch_to(Served::of(mp, to.to_path_buf()))
}

fn read(cell: &ServedCell) -> Served {
    cell.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Register the preview server's cell for this folder, pointing it here only
/// when it names nothing or another folder's tree.
///
/// That is a folder switch or a cold start. A cell already on this folder's
/// build tree is left alone, so the second of two builds started a second
/// apart does not pull the preview back off the first one's render.
///
/// Returns the record holding the cell. A caller that will show a render
/// keeps it until [`show_render`] returns: otherwise another folder's first
/// lookup can evict the record, and the render finds no cell to point.
#[must_use = "keep the record until the render is shown, or another folder's first lookup can evict it in between"]
pub(crate) fn adopt_server(mp: &MossPaths, cell: &ServedCell) -> Arc<LifecycleCell> {
    let target = mp.initial_serve_dir();
    let record = lock_for(mp);
    let mut st = record.state();
    st.served = Some(cell.clone());
    if read(cell).folder != Served::folder_of(mp) {
        point(cell, mp, &target);
        st.shown_render = None;
        log::info!("serve: adopted the preview for {} on {}", mp.project_root().display(), target.display());
    }
    drop(st);
    record
}

/// Proof that nothing still needs what this sweep unlinks from staging: no
/// reader of the staged tree, or only the render on screen, whose manifest the
/// sweep keeps; and no earlier build's workers still writing it.
///
/// A detached encode outlives its build, so the permit also carries what such
/// encodes own in staging: the outputs of every video still running, and what
/// a finished run put there that no build has registered yet.
pub(crate) struct SweepPermit {
    record: Arc<LifecycleCell>,
    encode_outputs: std::collections::HashSet<String>,
    /// A serving tail's hold on the folder build lock, against another
    /// process's build writing staging under the sweep.
    _folder_lock: Option<crate::infra::folder_lock::Held>,
}

/// Staging's sweep stamp, shared by every process that builds the folder: the
/// lock stamp of the last build to enter, then `swept` once that build's seal
/// tail swept staging against the manifest it sealed. Read and written only
/// under the folder build lock. Anything unreadable reads as not swept.
fn sweep_stamp(mp: &MossPaths) -> PathBuf {
    mp.build_dir().join("staging-swept")
}

fn read_sweep_stamp(mp: &MossPaths) -> (Option<u64>, bool) {
    let text = std::fs::read_to_string(sweep_stamp(mp)).unwrap_or_default();
    let mut words = text.split_whitespace();
    (words.next().and_then(|w| w.parse().ok()), words.next() == Some("swept"))
}

/// `permit`, unless the seal tail of the last build to enter swept staging
/// against the manifest this build inherits, so sweeping again removes
/// nothing. Records this build's entry first, so a build that writes staging
/// and then fails, in any process, leaves the next one sweeping.
pub(crate) fn unless_tail_swept<'a>(mp: &MossPaths, locked_at: u64, permit: Option<&'a SweepPermit>) -> Option<&'a SweepPermit> {
    let (_, swept) = read_sweep_stamp(mp);
    if let Err(e) = crate::infra::atomic_write::write_atomic(&sweep_stamp(mp), locked_at.to_string()) {
        log::warn!("staging sweep stamp: {e}");
        let _ = std::fs::remove_file(sweep_stamp(mp));
        return permit;
    }
    permit.filter(|_| !swept)
}

/// Record that the seal tail of the build that entered at `locked_at` swept
/// staging, unless another build has entered since.
pub(crate) fn note_tail_swept(mp: &MossPaths, locked_at: u64) {
    if read_sweep_stamp(mp).0 == Some(locked_at) {
        if let Err(e) = crate::infra::atomic_write::write_atomic(&sweep_stamp(mp), format!("{locked_at} swept")) {
            log::warn!("staging sweep stamp: {e}");
        }
    }
}

/// The seal tail's staging sweep, by call site.
#[derive(Default)]
pub(crate) enum SealSweep {
    #[default]
    None,
    /// The `exits_after_build` call site (CLI / `build_sync` / the
    /// snapshot-test harness), whose caller drops the runtime as soon as this
    /// returns: no later build will sweep what this one orphaned (a second
    /// pass in the same process starts from the sealed manifest, so it
    /// inherits the sweep), so the tail reclaims it now.
    Final(SweepPermit),
    /// The serving call site: the encodes whose outputs the sweep keeps, read
    /// when it sweeps (`Render::seal_sweep_permit`). Only a shipping seal
    /// sweeps: a withheld one leaves the previous manifest as the next build's
    /// baseline, which may name what this one dropped.
    Serving(std::sync::Arc<crate::types::runtime::VideoConversionState>),
}

impl SealSweep {
    pub(crate) fn final_permit(&self) -> Option<&SweepPermit> {
        match self { Self::Final(permit) => Some(permit), _ => None }
    }

    /// The serving tail's permit for `render`, passed only when its seal ships
    /// under the stage write lock.
    pub(crate) fn serving_permit(&self, render: Option<&Render>, folder: &Path, holds_lease: bool) -> Option<SweepPermit> {
        let (Self::Serving(encodes), Some(render)) = (self, render) else { return None };
        render.seal_sweep_permit(folder, holds_lease, encodes)
    }
}


impl SweepPermit {

    /// Whether the sweep must keep `key` for a running or finished encode:
    /// the output itself, or a `.tmp.`/`.pending.` sibling it is being written
    /// through.
    pub(crate) fn keeps(&self, key: &str) -> bool {
        if self.encode_outputs.contains(key) {
            return true;
        }
        [".tmp.", ".pending."]
            .iter()
            .filter_map(|marker| key.rfind(marker).map(|i| &key[..i]))
            .any(|base| self.encode_outputs.contains(base))
    }

    /// Whether `dir` (relative to staging) holds an output the sweep keeps.
    pub(crate) fn keeps_dir(&self, dir: &Path) -> bool {
        self.encode_outputs.iter().any(|key| Path::new(key).starts_with(dir))
    }
}

/// The permit for a one-shot build's reclaim of its own staging, after its
/// seal: that caller drops the runtime as soon as the tail returns, so no
/// server and no other build reads staging afterwards. The one reader left is
/// the same process's own second pass (`build::drive_passes`), which starts
/// from the manifest this seal just wrote; the sweep keeps every file that
/// manifest names, so that pass finds exactly what it would after a restart.
/// Minted at exactly that call site (`staging_unlink_invariant_test` pins it).
pub(crate) fn final_build_permit(mp: &MossPaths) -> SweepPermit {
    SweepPermit { record: lock_for(mp), encode_outputs: Default::default(), _folder_lock: None }
}

/// A GC token for tests that drive `cache::gc` directly, on a record of their
/// own so no folder's leases are consulted.
#[cfg(test)]
pub(crate) fn gc_token_for_test() -> CacheGcToken {
    CacheGcToken { record: Arc::new(LifecycleCell::default()) }
}

/// Every key of `keys` licensed, as under a permit, for tests that drive
/// `remove_stale_html` directly.
#[cfg(test)]
pub(crate) fn stale_html_for_test(keys: &[String]) -> StaleHtml {
    StaleHtml::licence(Some(&permit_for_test()), false, Path::new(""), keys.iter().map(|key| (key.clone(), None)).collect())
}

/// A permit for tests that drive a sweep directly.
#[cfg(test)]
pub(crate) fn permit_for_test() -> SweepPermit {
    SweepPermit { record: Arc::new(LifecycleCell::default()), encode_outputs: Default::default(), _folder_lock: None }
}

/// The stale page HTML a build may unlink from staging, spent only by
/// `media::pipeline::remove_stale_html`.
///
/// Under a [`SweepPermit`] that is every stale key. Without one the preview
/// is still reading staging, and a page whose source is gone would go on
/// being served from it, so a render about to be shown may still remove the
/// HTML of a page whose recorded source is no longer on disk. A source that
/// is offline, unreadable or only unlisted keeps its page.
pub(crate) struct StaleHtml {
    keys: Vec<String>,
}

impl StaleHtml {
    /// `stale` pairs each stale HTML key with the source the last manifest
    /// recorded for it, relative to `root`; `shown` is whether this render
    /// will be shown.
    pub(crate) fn licence(permit: Option<&SweepPermit>, shown: bool, root: &Path, stale: Vec<(String, Option<String>)>) -> Self {
        let source_gone = |source: &Option<String>| {
            source.as_ref().is_some_and(|source| {
                let path = root.join(source);
                std::fs::symlink_metadata(&path).is_err_and(|e| crate::build::icloud::is_definitely_absent(&path, &e))
            })
        };
        let keys = stale
            .into_iter()
            .filter(|(_, source)| permit.is_some() || (shown && source_gone(source)))
            .map(|(key, _)| key)
            .collect();
        Self { keys }
    }

    pub(crate) fn keys(&self) -> &[String] {
        &self.keys
    }
}

/// Move the preview off staging for a rebuild when that is no step back, and
/// say whether this build may unlink from staging.
///
/// A permit needs both: the cell is off staging — on `current` or on another
/// tree — or it shows a render `current` already holds, in which case it moves
/// to `current` first; and no build other than the caller holds a
/// [`CacheWriteLease`], because a withheld or slow build's workers outlive its
/// render and are still writing staging. Otherwise the pointer stays where it
/// is and this build unlinks nothing. Never waits.
///
/// `encode_outputs` is `VideoConversionState::protected_outputs`, snapshotted
/// by the caller.
pub(crate) fn park_for_rebuild(
    mp: &MossPaths,
    holds_lease: bool,
    encode_outputs: std::collections::HashSet<String>,
) -> Option<SweepPermit> {
    let staging = mp.staging_dir();
    let current = mp.current_ptr();
    let current_exists = current.exists();
    let current_gen = mp.current_generation_id().unwrap_or_else(|_| "none".to_string());
    let record = lock_for(mp);
    let st = record.state();
    let others_writing = others_writing(&st, holds_lease);
    let on_staging = st.served.as_ref().is_some_and(|cell| read(cell).dir == staging);
    let caught_up = matches!((st.shown_render, st.current_render), (Some(s), Some(c)) if c >= s);
    let render = |r: Option<u64>| r.map_or("none".to_string(), |r| r.to_string());
    if others_writing == 0 && (!on_staging || (caught_up && current_exists)) {
        if on_staging {
            if let Some(cell) = &st.served {
                point(cell, mp, &current);
            }
        }
        log::info!(
            "serve: rebuild parked on current (gen {}, render #{}) — staging sweep permitted",
            current_gen,
            render(st.current_render)
        );
        return Some(SweepPermit { record: record.clone(), encode_outputs, _folder_lock: None });
    }
    log::info!(
        "serve: rebuild left preview on {} (shows render #{}, current gen {} is render #{}; {} earlier build(s) still writing) — no unlinks this build",
        if on_staging { "staging" } else { "its tree" },
        render(st.shown_render),
        current_gen,
        render(st.current_render),
        others_writing
    );
    None
}

/// Builds other than the caller still between `run_pipeline` entry and their
/// seal tail's promotion: a withheld or slow build's workers outlive its
/// render and are still writing staging.
fn others_writing(st: &FolderLifecycle, holds_lease: bool) -> usize {
    st.build_writers.saturating_sub(usize::from(holds_lease))
}

/// Keep an exact prior route on the immutable current generation while a
/// new render rewrites staging. This moves only the served pointer;
/// it grants no sweep permit and changes no render or promotion epoch.
pub(crate) fn protect_current_before_rebuild(mp: &MossPaths) {
    let current = mp.current_ptr();
    if !current.exists() { return }
    let record = lock_for(mp);
    if let Some(cell) = &record.state().served {
        point(cell, mp, &current);
    };
}

/// Mint this render's number and, when it may be shown, point the preview at
/// staging. Called under the stage-write lock, once per render.
///
/// Returns the render and the directory to announce as served: staging for a
/// shown render; for a withheld one the cell and `shown_render` are left alone
/// and the answer is `current`, or nothing on a vault that has never promoted.
pub(crate) fn show_render(mp: &MossPaths, publishable: bool) -> (Render, Option<PathBuf>) {
    let staging = mp.staging_dir();
    let current = mp.current_ptr();
    let current_exists = current.exists();
    let record = lock_for(mp);
    let mut st = record.state();
    st.next_render += 1;
    let seq = st.next_render;
    let announce = if publishable {
        if let Some(cell) = &st.served {
            point(cell, mp, &staging);
        }
        st.shown_render = Some(seq);
        Some(staging)
    } else {
        current_exists.then_some(current)
    };
    drop(st);
    (Render { record, mp: mp.clone(), seq }, announce)
}

/// One render `show_render` minted. It holds its folder's record, because a
/// record nobody holds is dropped at another folder's first lookup and its
/// successor starts counting renders again: whoever asks about this render
/// later (the asset walk, the derive job, the seal tail) must reach the record
/// that counted it, and the paths of that folder, never another's.
#[derive(Clone)]
pub(crate) struct Render {
    record: Arc<LifecycleCell>,
    mp: MossPaths,
    seq: u64,
}

impl Render {
    /// Render `seq` of `mp`'s folder without minting it, for a test that
    /// orders renders itself.
    #[cfg(test)]
    pub(crate) fn numbered(mp: &MossPaths, seq: u64) -> Self {
        Self { record: lock_for(mp), mp: mp.clone(), seq }
    }

    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }

    /// Whether no newer render has been admitted for this folder: the test
    /// detached work makes between files, so a build that superseded it is
    /// the one that does the rest.
    pub(crate) fn is_latest(&self) -> bool {
        self.record.state().next_render == self.seq
    }

    /// Record that this render's background work has sealed, so its tail is
    /// about to run.
    pub(crate) fn note_sealed(&self) {
        let mut state = self.record.state();
        state.newest_sealed = state.newest_sealed.max(Some(self.seq));
    }

    /// Whether a newer render has already sealed. Its tail is then running,
    /// and writes everything this one's would unless it withholds or the
    /// process ends first; this one's writes would only be replaced by it, or
    /// land after it and roll it back.
    pub(crate) fn newer_sealed(&self) -> bool {
        self.record.state().newest_sealed.is_some_and(|newest| newest > self.seq)
    }

    /// The licence for this render's seal tail to unlink from staging what its
    /// sealed manifest does not name: while the preview shows this render, no
    /// newer one has been minted and no other build is writing.
    ///
    /// What can still ask for a condemned file is a page older than the one on
    /// screen: a second tab, or a lazy image of a page the preview has since
    /// morphed. Every tab was told to reload when this render was shown, so
    /// each such request is from a page about to be replaced, and a 404 there
    /// is what the reload would show. Encode outputs are kept as under
    /// [`park_for_rebuild`].
    ///
    /// The caller holds the stage write lock. Renders are minted under it, and
    /// the one exception, an unchanged entry revealing staging, needs a park
    /// permit, which this tail's open lease refuses; so nothing newer starts
    /// writing between this check and the unlink. The folder build lock is only
    /// tried: `build_inner` takes it before the stage write lock, so waiting
    /// here could deadlock, and whoever holds it is building, and sweeps for
    /// itself. The permit holds it until the sweep is done.
    pub(crate) fn seal_sweep_permit(&self, folder: &Path, holds_lease: bool, encodes: &crate::types::runtime::VideoConversionState) -> Option<SweepPermit> {
        let folder_lock = crate::infra::folder_lock::try_acquire_named(folder, "build").ok().flatten()?;
        let encode_outputs = encodes.protected_outputs();
        let st = self.record.state();
        let newest_on_screen = st.next_render == self.seq && st.shown_render == Some(self.seq);
        if !newest_on_screen || others_writing(&st, holds_lease) > 0 {
            return None;
        }
        Some(SweepPermit { record: self.record.clone(), encode_outputs, _folder_lock: Some(folder_lock) })
    }

    /// Run `f` only while the preview shows this render, shown at once or
    /// revealed by its tail. The record stays held across the check and `f`,
    /// so no newer render can be shown in between.
    pub(crate) fn while_shown(&self, f: impl FnOnce()) {
        let state = self.record.state();
        if state.shown_render == Some(self.seq) {
            f();
        }
    }

    /// Reveal this render once its required outputs became ready after its
    /// initial preview decision. A detached tail may do this only while its
    /// render is still the newest admitted one.
    pub(crate) fn reveal_if_latest(&self) -> bool {
        let staging = self.mp.staging_dir();
        let mut state = self.record.state();
        if state.next_render != self.seq { return false; }
        if state.shown_render == Some(self.seq) { return false; }
        let Some(cell) = state.served.clone() else { return false; };
        // The preview server's cell is process-global and can be re-adopted by a
        // different folder while this detached tail is waiting. Prove ownership
        // and switch the pointer under the same write lock so a folder switch
        // cannot land between the check and the write.
        let mut served = cell.write().unwrap_or_else(PoisonError::into_inner);
        if served.folder != Served::folder_of(&self.mp) { return false; }
        if served.dir != staging {
            served.dir = staging;
        }
        drop(served);
        state.shown_render = Some(self.seq);
        true
    }

    /// Return the preview to `current` when this render is on screen and its
    /// generation was withheld as unreadable.
    ///
    /// The one move to an older generation this module makes. A newer render
    /// already on screen is left alone, and a vault with nothing promoted stays on
    /// staging because there is nothing better to show.
    pub(crate) fn withdraw(&self) {
        let current = self.mp.current_ptr();
        if !current.exists() {
            return;
        }
        let current_gen = self.mp.current_generation_id().unwrap_or_else(|_| "unknown".to_string());
        let mut st = self.record.state();
        if st.shown_render != Some(self.seq) {
            return;
        }
        if let Some(cell) = &st.served {
            point(cell, &self.mp, &current);
        }
        st.shown_render = None;
        log::info!("serve: render #{} withdrawn — preview back on gen {}", self.seq, current_gen);
    }
}

/// Repoint `current` at `gen_id` unless a newer build already has, and record
/// that `current` now holds `render` (none for a build that minted no render).
/// `Ok(false)` is refused-as-stale.
///
/// A `generation_id` is a content hash with no order, and seal
/// tails run detached, so build N's tail can land after build N+1's. The epoch
/// is what orders them. The swap runs under its own lock and the render is
/// recorded only after it, so a reader between the two sees the older render
/// and withholds a permit rather than granting one early.
///
/// `copied` says the generation directory was just (re)written; see
/// [`repoint_needed`].
pub(crate) fn promote(
    mp: &MossPaths,
    epoch: u64,
    render: Option<&Render>,
    gen_id: &str,
    copied: bool,
) -> std::io::Result<bool> {
    let record = render.map_or_else(|| lock_for(mp), |render| render.record.clone());
    let _swap = record.promote_lock.lock().unwrap_or_else(PoisonError::into_inner);
    let promoted = record.state().promoted_epoch;
    if promoted >= epoch {
        log::info!("promotion refused: gen {} epoch {} < promoted {}", gen_id, epoch, promoted);
        return Ok(false);
    }
    if repoint_needed(copied, || serves(mp, gen_id)) {
        mp.set_current_ptr(gen_id)?;
    }
    let mut st = record.state();
    st.promoted_epoch = epoch;
    st.current_render = render.map(Render::seq);
    Ok(true)
}

/// Record `sealed`'s generation, at `gen_dir`, as whole on disk: every entry
/// present, and no copy of it cut off or drifted. Only where a copy can be
/// seeded from it at all, since recording stats every file.
pub(crate) fn remember_whole(mp: &MossPaths, gen_dir: &Path, sealed: &crate::build::manifest::SealedManifest, copied: bool) {
    if !crate::build::io_utils::CLONES_DIRS {
        return;
    }
    // Promoted without a copy, its record is already the one held.
    if !copied && whole_generation(mp).is_some_and(|whole| whole.id() == sealed.generation_id()) {
        return;
    }
    let whole = crate::build::store_gc::WholeGeneration::record(gen_dir, sealed.generation_id(), sealed.all_files());
    if let Some(whole) = &whole {
        whole.save(&mp.generations_dir());
    }
    lock_for(mp).state().whole = whole.map(Arc::new);
}

/// The generation [`remember_whole`] last recorded, in this process or, failing
/// that, the record kept beside the generation `current` names, so a one-shot
/// build seeds as a long-lived one does. Nothing here says it is still on disk,
/// or unchanged: `GenerationWriteLock::seed_from` and
/// `WholeGeneration::still_holds` check that.
pub(crate) fn whole_generation(mp: &MossPaths) -> Option<Arc<crate::build::store_gc::WholeGeneration>> {
    let record = lock_for(mp);
    if let Some(whole) = record.state().whole.clone() {
        return Some(whole);
    }
    let id = mp.current_generation_id().ok()?;
    let whole = Arc::new(crate::build::store_gc::WholeGeneration::load(&mp.generations_dir(), &id)?);
    let kept = record.state().whole.get_or_insert(whole).clone();
    Some(kept)
}

/// Forget the in-process record, as a new process starts without one.
#[cfg(test)]
pub(crate) fn forget_whole(mp: &MossPaths) {
    lock_for(mp).state().whole = None;
}

/// Whether a promotion must rewrite `current`. Re-promoting what it already
/// serves still records the render and epoch but leaves the pointer alone,
/// since on Windows repointing is a full copy — unless the generation was just
/// copied again: Windows `current` is a copy of the generation, not a link to
/// it, so bytes the re-copy fixed reach the preview only through a repoint.
pub(crate) fn repoint_needed(copied: bool, serves: impl FnOnce() -> bool) -> bool {
    copied || !serves()
}

/// Whether `current` serves `gen_id`: the marker names it AND the pointer
/// resolves to it. The marker alone is not enough — `retire_legacy_roots` can
/// remove `current` and leave the marker behind.
fn serves(mp: &MossPaths, gen_id: &str) -> bool {
    if mp.current_generation_id().ok().as_deref() != Some(gen_id) {
        return false;
    }
    #[cfg(unix)]
    return std::fs::read_link(mp.current_ptr()).is_ok_and(|target| {
        // The relative form the build writes now, or the absolute one a folder
        // built by an older moss still carries.
        target == Path::new("generations").join(gen_id) || target == mp.generation_dir(gen_id)
    });
    // `set_current_ptr` writes the marker only once `current` is whole.
    #[cfg(not(unix))]
    return mp.current_ptr().is_dir();
}

/// A build that may still write the object store or staging: held from
/// `run_pipeline` entry through the seal tail's `materialize_and_promote`
/// (`ship_phase`) — `BackgroundHandle::await_completion` hands it back
/// instead of dropping it once the background workers have joined, and
/// `build::advertise_sealed` drops it explicitly right after, before that
/// same tail's own `collect_build_store` can trigger cache GC. Widened from
/// stopping at `await_completion`, which dropped it before the seal tail had
/// even started — letting a concurrent `collect_build_store` GC a CAS blob
/// the seal tail's own `ship_phase` still needed to read.
pub(crate) struct CacheWriteLease {
    record: Arc<LifecycleCell>,
}

/// Wait out a running cache GC — the one wait a lease can have, as long as the
/// GC walk — then count one more writer of kind `writers`.
fn enter_writer(mp: &MossPaths, writers: fn(&mut FolderLifecycle) -> &mut usize) -> Arc<LifecycleCell> {
    let record = lock_for(mp);
    let started = std::time::Instant::now();
    let mut st = record.state();
    while st.gc_running {
        st = record.changed.wait(st).unwrap_or_else(PoisonError::into_inner);
    }
    *writers(&mut st) += 1;
    drop(st);
    let waited = started.elapsed();
    if waited > std::time::Duration::from_secs(1) {
        log::info!("cache write lease waited {} ms for a running cache GC", waited.as_millis());
    }
    record
}

pub(crate) fn cache_write_lease(mp: &MossPaths) -> CacheWriteLease {
    CacheWriteLease { record: enter_writer(mp, |st| &mut st.build_writers) }
}

impl Drop for CacheWriteLease {
    fn drop(&mut self) {
        let mut st = self.record.state();
        st.build_writers = st.build_writers.saturating_sub(1);
    }
}

/// Detached work retains cache objects without owning staging. It prevents
/// cache GC until the native operation completes, while independent builds
/// may still park and clean staging.
pub(crate) struct DetachedCacheLease {
    record: Arc<LifecycleCell>,
}

pub(crate) fn detached_cache_lease(mp: &MossPaths) -> DetachedCacheLease {
    DetachedCacheLease { record: enter_writer(mp, |st| &mut st.detached_writers) }
}

impl Drop for DetachedCacheLease {
    fn drop(&mut self) {
        let mut st = self.record.state();
        st.detached_writers = st.detached_writers.saturating_sub(1);
    }
}

/// Licence to collect this folder's object store: no build or detached cache job holds a
/// lease, and none can take one until the token drops. `cache::gc` requires
/// one, so "never during a build" is a type rather than a comment.
pub(crate) struct CacheGcToken {
    record: Arc<LifecycleCell>,
}

/// A GC token when nothing is writing the store, else the open lease counts
/// (build, detached). Never waits.
pub(crate) fn try_begin_cache_gc(mp: &MossPaths) -> Result<CacheGcToken, (usize, usize)> {
    let record = lock_for(mp);
    let mut st = record.state();
    if st.build_writers > 0 || st.detached_writers > 0 || st.gc_running {
        return Err((st.build_writers, st.detached_writers));
    }
    st.gc_running = true;
    drop(st);
    Ok(CacheGcToken { record })
}

impl Drop for CacheGcToken {
    fn drop(&mut self) {
        self.record.state().gc_running = false;
        self.record.changed.notify_all();
    }
}

/// What the record holds, for tests: (shown render, current render, open
/// build leases).
#[cfg(test)]
pub(crate) fn snapshot(mp: &MossPaths) -> (Option<u64>, Option<u64>, usize) {
    let record = lock_for(mp);
    let st = record.state();
    (st.shown_render, st.current_render, st.build_writers)
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
