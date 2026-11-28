//! The enhance-hook pass over the stage, plus the manifest re-registration it
//! forces.
//!
//! Slot injection rewrites `<!-- slot:... -->` markers into real content, so
//! every page it touches has different bytes than the render phase registered.
//! This module runs that pass and hands its receipts to the manifest — the
//! final hash, and the CAS object `ship_phase` reads the page from — which is
//! why it owns both halves: a caller that ran one without the other would seal
//! a manifest describing bytes that are no longer on disk.
//!
//! It also runs the `MOSS_INCREMENTAL_VERIFY=1` carry check
//! ([`render::incremental::carry_verify`]) — the check is defined as "final
//! bytes vs final bytes", and this is the first point where the stage holds
//! this build's final bytes.
//!
//! Lifted out of `build::pipeline` where it was a private helper.

use crate::build::outcome::BuildStopped;
use crate::build::manifest::PendingManifest;
use crate::build::render::incremental::CarryVerification;
use crate::build::enhance::{ResolvedSlots, Settled};
use crate::moss_paths::MossPaths;
use std::path::Path;

#[cfg(test)]
thread_local! {
    /// Pages the latest slot pass on this thread read and hashed.
    pub(crate) static LAST_SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(usize::MAX) };
}

/// Every registered page in the stage is one slot injection would leave as it
/// is. The pass fills only registered pages, so a page registered after it,
/// over bytes already staged, would ship with its slots empty.
#[cfg(debug_assertions)]
pub(crate) fn assert_every_page_filled(stage_dir: &Path, slots: &ResolvedSlots, pending: &PendingManifest) {
    let unfilled: Vec<&String> = pending
        .files()
        .keys()
        .filter(|page| crate::build::enhance::takes_slots(page))
        .filter(|page| {
            let Ok(html) = std::fs::read_to_string(stage_dir.join(page.as_str())) else { return false };
            crate::build::enhance::inject_slots(&html, slots, page) != html
        })
        .collect();
    assert!(unfilled.is_empty(), "registered after the slot pass, so never filled: {unfilled:?}");
}

#[allow(clippy::too_many_arguments)]
pub fn apply_to_stage_and_manifest(
    paths: &MossPaths,
    stage_dir: &Path,
    slots: &ResolvedSlots,
    pending: &mut PendingManifest,
    // Shadow-verification snapshots. `Some` only under
    // `MOSS_INCREMENTAL_VERIFY=1`; consumed at the END of this function because
    // that is the first moment the stage holds post-injection bytes.
    carry_verification: Option<CarryVerification>,
) -> Result<SlotBatch, BuildStopped> {
    // Wait for the stage's last batch, so the links below include the ones it saved.
    let unsettled = take_unsettled(stage_dir);
    // A carried page holds the previous pass's output only if that pass ran with
    // these same slots and shipping transform; otherwise every page is re-read.
    let slots_hash = crate::build::enhance::resolved_slots_hash(slots);
    let mount = pending.mount();
    let key = format!("{slots_hash}:{}", crate::build::ship::shipped_key(&mount));
    let mut record = PassRecord::load(paths);
    let same_pass = record.key == key;
    // Until this pass ends, the stage holds no one render's pages.
    if record.render.take().is_some() {
        record.save(paths);
    }
    let object_store = crate::build::cache::ObjectStore::for_slot_pass(paths);
    let transform_cache = crate::build::cache::TransformCache::for_slot_pass(paths, &object_store);
    // Emit the feature stylesheets the resolved slots link to, BEFORE the
    // injection that writes those <link> tags into pages. Slot resolution can
    // name a content-hashed file but has no manifest to write one; this is the
    // first point that has both. See `build::emit::feature_styles`.
    crate::build::emit::feature_styles::emit(slots.feature_styles(), stage_dir, pending)?;

    // The record of which blob each staged page was linked from, so a page
    // whose stage file already holds its final bytes is not written again.
    let mut staged = crate::build::lifecycle::cas_heal::StagedLinks::load(paths, "pages", stage_dir);
    let carried = if same_pass { pending.carried_pages() } else { Default::default() };
    let registered = pending.files().keys().filter(|page| crate::build::enhance::takes_slots(page)).cloned().collect();
    let pass = crate::build::enhance::inject_slots_into_directory_cached(
        paths.project_root(),
        stage_dir,
        slots,
        &slots_hash,
        &mount,
        &object_store,
        &transform_cache,
        pending.take_unwritten_pages(),
        registered,
        &carried,
        if same_pass { unsettled } else { Vec::new() },
        &mut staged,
    )
    // `with_context`, not `format!`: re-stringifying would discard the deferred
    // verdict. `BuildStopped` has no `Display`, so that mistake cannot compile.
    .map_err(|e| e.with_context("Slot injection failed on site-stage"))?;
    // Before `staged.save()`: a crash between the two must leave links that no
    // longer vouch, never a record claiming slots the staged pages were not filled with.
    if !same_pass {
        record.key = key;
        record.save(paths);
    }
    staged.save();
    if let Err(e) = object_store.save_verified_keeping_unchecked() {
        log::info!("[slots] could not save the verified-blob record: {e}");
    }
    crate::build::cache::page_sources::save(paths, pass.sources, carried.keys().map(String::as_str));
    #[cfg(test)]
    LAST_SCANNED.with(|n| n.set(pass.scanned));

    // Takes what the injection pass REPORTS, never what a read of the stage says
    // is there. The hash and the CAS object arrive in the receipt because the
    // bytes existed in memory when the pass held them and nowhere afterwards.
    //
    // The pass reports on every `.html` in the stage, including pages an earlier
    // build left there, so a receipt is not proof that this build made the page.
    // `attach_final_bytes` gives the final bytes only to pages the render phase
    // already registered, and drops the rest — see it for what a stale page
    // would otherwise do. Nothing that used to be registered here is lost: a page
    // keeps its markers only until the first pass that fills them, so the pages
    // the pass rewrites are the ones this build just rendered.
    // The served path and entry a page the store has not taken yet ships under.
    let mut attach = |receipt: &crate::build::enhance::SlotInjectionReceipt| {
        // A stage path no page of this build could have (`_moss/...` bundles a
        // search settle laid down last time) is simply not one.
        let sp = crate::build::served_path::ServedPath::from_output(&receipt.page_path).ok()?;
        let entry = pending.attach_final_bytes(&sp, &receipt.manifest_hash, receipt.content_oid.clone());
        entry.filter(|_| receipt.content_oid.is_none()).map(|entry| (sp.as_str().to_string(), entry))
    };
    for receipt in &pass.receipts {
        attach(receipt);
    }
    let mut pages = pass.deferred;
    for page in &mut pages {
        page.ship = attach(&page.receipt);
    }
    // The build that runs again finds the page missing and renders it.
    if let Some(page) = pending.rereads_left().next() {
        return Err(BuildStopped::discarded(format!("{page} left the stage before the slot pass read it")));
    }
    record.render = pending.render_id().map(str::to_string);
    record.save(paths);

    // Comparing any earlier compares the render phase's PRE-injection bytes
    // against the previous build's POST-injection bytes, which reports a
    // divergence on literally every page — which is what the first cut did.
    if let Some(verification) = carry_verification.filter(|v| !v.is_empty()) {
        verification.compare_final_bytes(stage_dir);
    }

    let handover = std::sync::Arc::new(Handover::default());
    RUNNING.lock().unwrap_or_else(|e| e.into_inner()).insert(stage_dir.to_path_buf(), handover.clone());
    Ok(SlotBatch { paths: paths.clone(), stage_dir: stage_dir.to_path_buf(), pages, handover, session: None, _hold: None })
}

/// The slot batch of each stage in this process, which the stage's next pass
/// waits for so that it carries the pages the batch stored instead of reading them.
static RUNNING: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, std::sync::Arc<Handover>>>> =
    std::sync::LazyLock::new(Default::default);

#[derive(Default)]
struct Handover {
    /// Set once, when the batch ends or is dropped unrun.
    unsettled: std::sync::Mutex<Option<Vec<crate::build::enhance::DeferredPage>>>,
    ended: std::sync::Condvar,
}

/// Wait for the stage's batch to end and take the pages it did not settle (all
/// of them when it was dropped unrun). Past the bound its pages are read
/// again, which costs time, never correctness.
fn take_unsettled(stage_dir: &Path) -> Vec<crate::build::enhance::DeferredPage> {
    let Some(handover) = RUNNING.lock().unwrap_or_else(|e| e.into_inner()).remove(stage_dir) else { return Vec::new() };
    let unsettled = handover.unsettled.lock().unwrap_or_else(|e| e.into_inner());
    let (mut unsettled, wait) = handover.ended
        .wait_timeout_while(unsettled, std::time::Duration::from_secs(30), |pages| pages.is_none())
        .unwrap_or_else(|e| e.into_inner());
    if wait.timed_out() {
        log::info!("[slots] the last slot batch did not end; its pages are read again");
    }
    unsettled.take().unwrap_or_default()
}

/// The pages the slot pass wrote with no blob yet. A worker stores their blobs,
/// records and staged links after the preview paints; the seal waits for it, so
/// a deploy does too.
pub(crate) struct SlotBatch {
    paths: MossPaths,
    stage_dir: std::path::PathBuf,
    pages: Vec<crate::build::enhance::DeferredPage>,
    handover: std::sync::Arc<Handover>,
    session: Option<std::sync::Arc<crate::system::folder_session::FolderSession>>,
    _hold: Option<crate::system::folder_session::UiBoundHold>,
}

impl SlotBatch {
    /// The batch to run after the paint, `None` when the pass stored nothing.
    /// Held from dispatch, so a publish waits even before the pool starts it.
    pub(crate) fn after_paint(mut self, session: Option<std::sync::Arc<crate::system::folder_session::FolderSession>>) -> Option<Self> {
        self._hold = session.as_ref().map(|s| s.hold_ui_bound());
        self.session = session;
        (!self.pages.is_empty()).then_some(self)
    }

    pub(crate) fn spawn(
        self,
        workers: &mut tokio::task::JoinSet<Result<(), crate::build::background::BuildError>>,
        tx: tokio::sync::mpsc::Sender<crate::build::coordinator::EmitMessage>,
    ) {
        crate::build::background::spawn_traced_worker(workers, move || self.run(&tx));
    }

    /// Run the batch here and attach its blobs to `pending`, as the coordinator would.
    #[cfg(test)]
    pub(crate) fn settle_into(self, pending: &mut PendingManifest) {
        let (tx, mut rx) = tokio::sync::mpsc::channel(self.pages.len().max(1));
        self.run(&tx).expect("slot batch");
        while let Ok(message) = rx.try_recv() {
            crate::build::coordinator::apply(pending, message);
        }
    }

    /// No stage lock: each staged link is proved by the stage file's own stat,
    /// so a save's pass that rewrites a page meanwhile only costs it its link.
    fn run(mut self, tx: &tokio::sync::mpsc::Sender<crate::build::coordinator::EmitMessage>) -> Result<(), crate::build::background::BuildError> {
        let _phase = crate::build::phase::PhaseTrace::start("background_slot_batch");
        let objects = crate::build::cache::ObjectStore::for_slot_pass(&self.paths);
        let transforms = crate::build::cache::TransformCache::for_slot_pass(&self.paths, &objects);
        let mut pages = std::mem::take(&mut self.pages);
        let session = self.session.as_deref();
        let settled = BATCH_POOL.install(|| crate::build::enhance::settle_pages(&mut pages, &self.stage_dir, &self.paths, &objects, &transforms,
            || session.is_some_and(|s| s.cancel.is_cancelled())));
        let mut closed = false;
        for (page, settled) in pages.into_iter().zip(settled) {
            match settled {
                Settled::Cancelled => self.pages.push(page),
                Settled::Stored if !closed => {
                    if let (Some((rel_path, entry)), Some(oid)) = (page.ship, page.receipt.content_oid) {
                        closed = tx.blocking_send(crate::build::coordinator::EmitMessage::Blob { rel_path, entry, oid }).is_err();
                    }
                }
                Settled::Stored | Settled::Failed => {}
            }
        }
        if closed {
            return Err(crate::build::background::BuildError::Worker("slot batch coordinator closed".into()));
        }
        Ok(())
    }
}

/// The batch settles on two workers. Its work is file creates and renames,
/// which the kernel serialises: on a 10k-page site a full pool finished it no
/// sooner than two workers and spent twice their CPU contending for those locks.
static BATCH_POOL: std::sync::LazyLock<rayon::ThreadPool> = std::sync::LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("moss-slot-batch-{i}"))
        .num_threads(rayon::current_num_threads().min(2))
        .build()
        .expect("moss-slot-batch pool")
});

impl Drop for SlotBatch {
    fn drop(&mut self) {
        *self.handover.unsettled.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::mem::take(&mut self.pages));
        self.handover.ended.notify_all();
    }
}

/// What the last slot pass left on the stage, per machine.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct PassRecord {
    /// The slots and shipping transform the pass filled pages with.
    key: String,
    /// The render whose final bytes every stage page holds: set when a pass
    /// ends, cleared before the next one first writes to the stage.
    render: Option<String>,
}

impl PassRecord {
    fn path(paths: &MossPaths) -> std::path::PathBuf {
        paths.cache_dir().join("slot-pass.json")
    }

    fn load(paths: &MossPaths) -> Self {
        std::fs::read(Self::path(paths))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self, paths: &MossPaths) {
        if let Ok(json) = serde_json::to_vec(self) {
            let _ = std::fs::write(Self::path(paths), json); // allow:raw_write per-machine cache record, never staging
        }
    }
}

/// The render the last slot pass wrote the stage for, if it ended and no pass
/// has started since.
pub(crate) fn staged_render(paths: &MossPaths) -> Option<String> {
    PassRecord::load(paths).render
}

/// Write rendered pages to the stage as rendered, slot markers and all: what
/// an entry point with no slot pass after it leaves.
pub(crate) fn write_as_rendered(
    stage_dir: &Path,
    pages: std::collections::BTreeMap<String, String>,
) -> Result<(), BuildStopped> {
    for (page_path, html) in pages {
        let path = stage_dir.join(&page_path);
        crate::build::io_utils::write_output_if_changed(&path, html.as_bytes())
            .map_err(|e| format!("Failed to write HTML file {}: {}", path.display(), e))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "slots_tests.rs"]
mod tests;
