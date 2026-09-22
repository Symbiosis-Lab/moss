//! The search lane: a generation-free worker, adopted from a receipt (ADR-045).
//!
//! Pagefind's cost scales with **corpus size**, not with this build's delta —
//! ~5.2 s on a 386-page vault, on every save, including the ones whose own
//! verdict is `changed=0 -> SUPPRESS`. It used to be dispatched into the
//! build's worker `JoinSet`; the seal awaited every worker, then took the
//! per-folder stage-write mutex, which the *next* build takes before doing any
//! work — so the next save's latency was a function of the previous build's
//! slowest background worker (moss#968 Finding 3).
//!
//! ADR-045's rule: **a generation-free worker may never be a manifest
//! registrant, and no seal may await one.** Instead:
//!
//! ```text
//!   build N  ──seal──► generation N frozen ──request(fp_N)──►  lane
//!                                                               │ debounce
//!                                                               │ index generations/<N>/
//!                                                               ▼
//!                                          .moss/build.nosync/index/<fp_N>/  +  receipt.json
//!   build N+1 ──adopt_into(receipt)──► staging + PendingManifest
//! ```
//!
//! # What must not change, and does not
//!
//! `PendingManifest` is mark-and-sweep: `seal()` prunes each output bucket to
//! the paths this build touched, and `remove_stale_files` deletes every staging
//! file the sealed manifest omits. `_moss/pagefind/**` gets no exemption —
//! ADR-045 rejects that as the first hole in a total invariant — so a build
//! that skipped re-registering would have the deploy diff read the bundle as
//! *removed* and delete it off the live site. Hence [`adopt_into`] runs on
//! **every** build, inside the build's own stage-write span, registering the
//! receipt's `(path, hash)` entries with zero byte reads. Only the registrant
//! changed.
//!
//! # Which tree the lane indexes
//!
//! The **frozen generation**, never `staging/`: staging is what the next build
//! rewrites, the generation is ship-transformed (`data-source-line` stripped,
//! so preview search matches deploy search), and it is immutable once
//! `materialize_and_promote` has shipped into it.
//!
//! # Staleness budget
//!
//! Preview search may lag the vault by up to `MAX_DEFER + index_time`. Nothing
//! else consumes the index, so nothing else observes the lag.
//! [`settle_for_publish`] is the publish path's sync point, so a deploy never
//! ships a stale index.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::build::feeds::search;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::moss_paths::MossPaths;
use crate::types::content::SiteHashes;

/// Quiet period a request must survive before the lane indexes.
const IDLE: Duration = Duration::from_secs(2);

/// Upper bound on deferral under sustained saving. Without it a vault being
/// edited continuously would never index at all.
const MAX_DEFER: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// PageSetFp
// ---------------------------------------------------------------------------

/// Fingerprint of the indexable page set: xxh3 over the `(served path, content
/// hash)` pairs of every `.html` entry in a sealed manifest. Same fold and data
/// as `compute_manifest_generation_id` (`build/assets/paths.rs`), **restricted
/// to HTML** so an image re-encode does not re-index a corpus whose text did
/// not move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageSetFp(u64);

impl PageSetFp {
    /// The fingerprint of a page set nobody has computed. Never equal to a real
    /// one in practice, so a bundle stamped with it is always superseded by the
    /// next real index — which is exactly what the synchronous fallback in
    /// [`adopt_into`] wants, having indexed `staging/` rather than a generation.
    pub const UNKNOWN: PageSetFp = PageSetFp(0);

    /// Fold a manifest's HTML entries. `files` is `SiteHashes::files`.
    pub fn of(files: &HashMap<String, String>) -> Self {
        use std::collections::BTreeMap;
        let sorted: BTreeMap<&String, &String> = files
            .iter()
            .filter(|(path, _)| path.ends_with(".html"))
            .collect();
        let mut buf: Vec<u8> = Vec::with_capacity(sorted.len() * 64);
        for (path, entry) in &sorted {
            buf.extend_from_slice(path.as_bytes());
            buf.push(b'\x00');
            buf.extend_from_slice(entry.as_bytes());
            buf.push(b'\n');
        }
        Self(xxhash_rust::xxh3::xxh3_64(&buf))
    }

    fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }

    fn from_hex(s: &str) -> Self {
        Self(u64::from_str_radix(s, 16).unwrap_or(0))
    }
}

/// A page set named **and counted**.
///
/// The count is the load-bearing half. A fingerprint asserts *which* pages a
/// bundle covers; nothing in the indexer's return value proves it covered them,
/// since every page-level failure there is a skip-with-a-warning and the walk
/// swallows its own errors. An evicted generation, a GC'd one and a tree moss
/// cannot read all reduce to "0 pages indexed" — which, stamped with a real
/// fingerprint, is an *authoritative empty index* no later build corrects.
/// Carrying the count lets [`publish_bundle`] refuse that stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageSet {
    pub fp: PageSetFp,
    /// How many `.html` entries the page set has.
    pub pages: usize,
}

impl PageSet {
    /// The page set of a sealed manifest. `files` is `SiteHashes::files`.
    pub fn of(files: &HashMap<String, String>) -> Self {
        Self {
            fp: PageSetFp::of(files),
            pages: files.keys().filter(|p| p.ends_with(".html")).count(),
        }
    }

    /// The page set nobody has computed — what the synchronous fallback stamps,
    /// having indexed `staging/` rather than a generation. `pages: 0` disables
    /// only the *coverage* half of the publish guard; the shortfall half still
    /// applies, so a partial index is refused there too.
    const UNKNOWN: PageSet = PageSet { fp: PageSetFp::UNKNOWN, pages: 0 };
}

// ---------------------------------------------------------------------------
// BundleReceipt
// ---------------------------------------------------------------------------

/// What the lane last published: which page set it indexed, and the
/// `(bundle-relative path, content hash)` pairs of every file it produced.
/// Written **after** every bundle file is on disk, so a torn publish reads back
/// as "no receipt" rather than as a bundle whose files are half there.
///
/// An empty `files` is meaningful, not a failure: it says "this page set has no
/// indexable content", and it stops the fallback in [`adopt_into`] from
/// re-running a whole index on every build of an empty site. It is only ever
/// written when `pages` is 0 as well — see [`publish_bundle`]'s guard.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BundleReceipt {
    /// Hex [`PageSetFp`] of the page set that produced this bundle.
    pub fp: String,
    /// How many pages were actually read and indexed. The receipt records what
    /// the indexer **measured**, not what the fingerprint asserts, so a reader
    /// can tell an empty site from an unreadable one.
    #[serde(default)]
    pub pages: usize,
    /// `(path within the bundle, xxh3 content hash)`.
    pub files: Vec<(String, String)>,
}

impl BundleReceipt {
    fn fp(&self) -> PageSetFp {
        PageSetFp::from_hex(&self.fp)
    }
}

fn read_receipt(index_dir: &Path) -> Option<BundleReceipt> {
    let raw = std::fs::read_to_string(index_dir.join("receipt.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

// ---------------------------------------------------------------------------
// The holding-area lock
// ---------------------------------------------------------------------------

/// Token proving its holder owns the holding area for one folder.
///
/// Every function that mutates `.moss/build.nosync/index/` — or reads the receipt and
/// then acts on it — takes one. Without it the lane and a build interleave: the
/// lane's [`gc_holding`] deletes `<fpA>/` while a build is halfway through
/// copying it into staging, the build then registers nothing *and* deletes the
/// receipt the lane just wrote for `<fpB>`, and the sweep takes the bundle off
/// the live site — the mark-and-sweep hole ADR-045 closes, from the other side.
///
/// Not a substitute for the build's stage-write guard, which orders builds
/// against each other; this orders the *lane* against a build. The lane never
/// takes the stage-write lock, so the pair cannot deadlock.
pub struct Holding(());

static HOLDING_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Run `f` with the holding area for `index_dir` exclusively held.
fn with_holding<T>(index_dir: &Path, f: impl FnOnce(&Holding) -> T) -> T {
    let lock = {
        let mut locks = HOLDING_LOCKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(locks.entry(index_dir.to_path_buf()).or_default())
    };
    let _guard = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&Holding(()))
}

/// Absolute path of the directory holding the bundle for `fp`.
fn holding_dir(index_dir: &Path, fp: PageSetFp) -> PathBuf {
    index_dir.join(fp.hex())
}

/// Records which bundle is currently laid into `staging/`, so the steady-state
/// adoption is ~440 stats rather than ~440 copies.
fn staged_fp_path(index_dir: &Path) -> PathBuf {
    index_dir.join("staged.fp")
}

/// Write `produced`'s files into `index_dir/<fp>/`, then the receipt. Older
/// holding dirs for the same folder are removed — a bundle nobody's receipt
/// names is dead weight, and the holding area is not swept by anything else.
///
/// # The coverage guard
///
/// **A bundle is stamped with `want.fp` only if it demonstrably covers
/// `want`.** A short bundle under a complete page set's fingerprint is not a
/// degraded result but an *absorbing* one: `run_lane` skips every later request
/// carrying that fp, [`settle_for_publish`]'s `intact` test is vacuously true
/// over an empty file list, and [`adopt_into`] registers nothing — so the sweep
/// takes the bundle out of staging and the deploy diff deletes the index off
/// the live site, with nothing scheduled to re-index.
///
/// Both halves are needed. `skipped > 0` catches a page moss could not read (an
/// iCloud-evicted `.html` under `.moss/build.nosync/`); `pages < want.pages` catches a
/// tree that is not there at all — a generation GC'd mid-debounce walks to zero
/// pages, indistinguishable from "this site has no indexable content".
///
/// A refusal is an ordinary transient failure: no receipt is written, so the
/// previous good bundle stays adopted and the next request re-indexes.
fn publish_bundle(
    _holding: &Holding,
    index_dir: &Path,
    want: PageSet,
    produced: &search::SearchIndex,
) -> Result<BundleReceipt, String> {
    if produced.skipped > 0 || produced.pages < want.pages {
        let expected = want.pages.max(produced.pages + produced.skipped);
        return Err(format!(
            "indexed {} of {} pages ({} unreadable) — refusing to publish a partial \
             bundle as page set {}",
            produced.pages,
            expected,
            produced.skipped,
            want.fp.hex(),
        ));
    }

    let fp = want.fp;
    let files = &produced.files;
    let dir = holding_dir(index_dir, fp);
    let mut receipt = BundleReceipt {
        fp: fp.hex(),
        pages: produced.pages,
        files: Vec::with_capacity(files.len()),
    };
    for file in files {
        let dest = dir.join(&file.rel_path);
        crate::build::io_utils::write_output(&dest, &file.bytes)
            .map_err(|e| format!("failed to write search bundle file {:?}: {}", dest, e))?;
        receipt.files.push((
            file.rel_path.clone(),
            crate::build::assets::paths::compute_binary_hash(&file.bytes),
        ));
    }
    receipt.files.sort();

    let json = serde_json::to_vec_pretty(&receipt)
        .map_err(|e| format!("failed to serialize search receipt: {}", e))?;
    crate::build::io_utils::write_output(&index_dir.join("receipt.json"), &json)
        .map_err(|e| format!("failed to write search receipt: {}", e))?;
    // The staged marker names a bundle that is no longer the receipt's, so the
    // next adoption must re-lay every file rather than trust its stats.
    // allow:unlink the search index under .moss/build.nosync/index, not staging
    let _ = std::fs::remove_file(staged_fp_path(index_dir));
    gc_holding(index_dir, fp);
    Ok(receipt)
}

/// Drop every holding directory except `keep`.
fn gc_holding(index_dir: &Path, keep: PageSetFp) {
    let keep = keep.hex();
    let Ok(entries) = std::fs::read_dir(index_dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == keep || !entry.path().is_dir() {
            continue;
        }
        // allow:unlink the search index under .moss/build.nosync/index, not staging
        let _ = crate::build::io_utils::remove_output_dir_all(&entry.path());
    }
}

// ---------------------------------------------------------------------------
// Adoption
// ---------------------------------------------------------------------------

/// What a build did with the search bundle. Reported in the build log; the
/// variants exist because "registered nothing" has three very different causes
/// and only one of them is fine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adoption {
    /// Search is off. Registering nothing is correct: mark-and-sweep retires
    /// whatever bundle a previous, search-enabled build left behind.
    Disabled,
    /// The receipt's files were verified on disk and registered.
    Adopted(usize),
    /// No receipt existed (search just enabled, or `.moss/build.nosync/index` was
    /// deleted). Indexed synchronously so this build still ships a search box.
    Indexed(usize),
    /// The receipt named files that are not on disk. Registered nothing,
    /// dropped the receipt; the next build takes the `Indexed` path.
    Diverged,
    /// A receipt exists and says this page set has no indexable content.
    Empty,
}

/// The search gate: `[site].search`, re-read from a `.moss` path because the
/// seal tail and the deploy pre-flight both need it long after the
/// `SiteConfig` that resolved it was consumed (off the critical path, so the
/// read costs nothing). The toggle alone decides — search graduated out of
/// `experimental.preview_features` 2026-08-31 (ADR-037 "Gating").
pub fn enabled_for(mp: &MossPaths) -> bool {
    let Some(project_root) = mp.root().parent() else {
        return false;
    };
    crate::build::site_config::get_site_search(&project_root.to_string_lossy())
        .ok()
        .flatten()
        .unwrap_or(false)
}

/// Whether a lane will ever run for this build.
///
/// ADR-045's staleness budget is a promise about the *edit loop*, payable only
/// because a next build is coming. A process that exits when the build returns
/// — CLI `moss build`, `build_sync`, the snapshot harness — spawns no lane and
/// calls [`request`] from no seal, so a receipt adopted verbatim would freeze
/// the index at whatever the *first* such build produced: add a page and it is
/// never searchable, delete one and search still returns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// A seal will hand the frozen generation to the lane. Adopt the receipt.
    Lane,
    /// Nothing will index after this build. Index now, every time — which is
    /// what the pre-ADR-045 worker did on this path anyway.
    Now,
}

impl Freshness {
    /// `exits_after_build` is the whole of the question: that flag is exactly
    /// "this process drops its runtime when the build returns".
    pub fn of(exits_after_build: bool) -> Self {
        if exits_after_build { Self::Now } else { Self::Lane }
    }
}

/// Copy the receipt's bundle into `stage_dir` and register every entry in
/// `pending`. Call once per build, inside the build's own stage-write span.
///
/// **Registration is unconditional**, on every branch and every build, because
/// `PendingManifest` is mark-and-sweep (see the module docs). Only the
/// *computation* is demand-driven.
///
/// Costs ~440 stats (1–2 ms) in the steady state and no byte reads at all: the
/// hashes come from the receipt, and files already laid into staging by the
/// bundle the `staged.fp` marker names are not re-copied.
pub fn adopt_into(
    mp: &MossPaths,
    stage_dir: &Path,
    pending: &mut PendingManifest,
    enabled: bool,
    freshness: Freshness,
) -> Adoption {
    if !enabled {
        return Adoption::Disabled;
    }
    let index_dir = mp.index_dir();
    // Held across read-receipt → lay → drop-receipt, so the lane cannot GC the
    // holding directory this call is reading, nor have its brand-new receipt
    // deleted by the divergence branch below.
    with_holding(&index_dir, |h| {
        let receipt = match freshness {
            Freshness::Lane => read_receipt(&index_dir),
            Freshness::Now => None,
        };
        let Some(receipt) = receipt else {
            // Nothing to adopt. Registering nothing here would let this build's
            // own sweep delete a bundle that no *future* build is scheduled to
            // rebuild — under the old ticket scheme a newer worker was queued
            // by construction, and under adoption none is. So fall through to a
            // synchronous index (ADR-045, "nothing to adopt").
            //
            // It indexes `staging/`, not a generation, and that is deliberate:
            // this build's own HTML is complete and, under the stage-write
            // guard the caller holds, nothing else is writing it. The
            // generation for it does not exist yet — it is materialized from
            // the manifest this call is registering into. The receipt is
            // stamped `UNKNOWN` so the lane re-indexes the real,
            // ship-transformed generation as soon as it is frozen.
            return match index_now(h, &index_dir, stage_dir, PageSet::UNKNOWN) {
                Ok(receipt) => match lay_and_register(h, &index_dir, &receipt, stage_dir, pending) {
                    Some(0) => Adoption::Empty,
                    Some(n) => Adoption::Indexed(n),
                    None => Adoption::Diverged,
                },
                Err(e) => {
                    // Transient (a page that will not parse, a full disk, a
                    // tree half of which could not be read). No receipt is
                    // written, so the next build retries — the same disposition
                    // the old worker had for a failed index.
                    log::warn!(target: "search", "search index not generated: {}", e);
                    Adoption::Diverged
                }
            };
        };

        if receipt.files.is_empty() {
            return Adoption::Empty;
        }

        match lay_and_register(h, &index_dir, &receipt, stage_dir, pending) {
            Some(n) => Adoption::Adopted(n),
            None => {
                // Receipt/disk divergence: an external delete, or the holding
                // area evicted by a cloud provider. Registering a path that is
                // not on disk is the deploy error `manifest.rs` documents, so
                // register nothing and drop the receipt — the next build
                // re-indexes.
                drop_receipt_if_still(h, &index_dir, &receipt);
                log::warn!(
                    target: "search",
                    "search bundle {} is incomplete on disk — dropped, will re-index",
                    receipt.fp,
                );
                Adoption::Diverged
            }
        }
    })
}

/// Delete `receipt.json` only while it still names the bundle that diverged.
///
/// The [`Holding`] token makes this redundant against *this* process; it is not
/// redundant against a second moss instance open on the same folder, where
/// deleting by path alone would destroy a receipt this build never read.
fn drop_receipt_if_still(_holding: &Holding, index_dir: &Path, diverged: &BundleReceipt) {
    if read_receipt(index_dir).map(|r| r.fp) == Some(diverged.fp.clone()) {
        // allow:unlink the search index under .moss/build.nosync/index, not staging
        let _ = std::fs::remove_file(index_dir.join("receipt.json"));
    }
}

/// Stat every receipt entry, copy the ones staging is missing, register them
/// all. `None` if any file is missing or any copy fails — and **nothing is
/// registered in that case**: a partial registration is the worst outcome of
/// the three, since the sweep deletes the unregistered remainder and
/// `pagefind.js` is then left fetching shards that 404. Hence three passes,
/// not two: stat all, copy all, then register all.
fn lay_and_register(
    _holding: &Holding,
    index_dir: &Path,
    receipt: &BundleReceipt,
    stage_dir: &Path,
    pending: &mut PendingManifest,
) -> Option<usize> {
    let holding = holding_dir(index_dir, receipt.fp());
    let staged = std::fs::read_to_string(staged_fp_path(index_dir)).ok();
    let refresh = staged.as_deref() != Some(receipt.fp.as_str());

    let mut planned: Vec<(ServedPath, &str, PathBuf, PathBuf)> =
        Vec::with_capacity(receipt.files.len());
    for (rel, hash) in &receipt.files {
        let sp = ServedPath::for_search_asset(rel).ok()?;
        let src = holding.join(rel);
        if !crate::build::io_utils::output_present(&src) {
            return None;
        }
        let dest = stage_dir.join(sp.as_str());
        planned.push((sp, hash, src, dest));
    }

    for (sp, _, src, dest) in &planned {
        if refresh || !crate::build::io_utils::output_present(dest) {
            if let Err(e) = crate::build::io_utils::copy_output(src, dest) {
                log::warn!(target: "search", "failed to lay {} into staging: {}", sp, e);
                return None;
            }
        }
    }

    for (sp, hash, _, _) in &planned {
        pending.register_hashed(sp, hash, HashBucket::Files);
    }

    if refresh {
        let _ = crate::build::io_utils::write_output(
            &staged_fp_path(index_dir),
            receipt.fp.as_bytes(),
        );
    }
    Some(planned.len())
}

/// Index `index_from` synchronously and publish the result. Used by the
/// nothing-to-adopt fallback and by [`settle_for_publish`] — the two places
/// where waiting is the correct answer.
fn index_now(
    holding: &Holding,
    index_dir: &Path,
    index_from: &Path,
    want: PageSet,
) -> Result<BundleReceipt, String> {
    let produced = search::build_search_index(index_from)?;
    publish_bundle(holding, index_dir, want, &produced)
}

// ---------------------------------------------------------------------------
// The lane
// ---------------------------------------------------------------------------

/// One indexing request: the frozen generation to read and the page set it
/// represents.
#[derive(Debug, Clone)]
struct LaneRequest {
    root: PathBuf,
    index_dir: PathBuf,
    gen_id: String,
    gen_dir: PathBuf,
    want: PageSet,
}

/// One lane per open folder, keyed by `.moss` directory. The `watch` channel is
/// **level**-triggered: a request that arrives while the lane is busy is not a
/// queued item to be drained but the new current value, so a burst of saves
/// collapses to one index without any ticket bookkeeping.
static LANES: LazyLock<Mutex<HashMap<PathBuf, tokio::sync::watch::Sender<LaneRequest>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Generations the lane has been asked to read and has not finished with, per
/// folder. Consulted by `gc_old_generations`.
static IN_FLIGHT: LazyLock<Mutex<HashMap<PathBuf, HashSet<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Times any lane has evaluated a request. The **only** observable of the idle
/// spin an earlier shape had: a lane re-reading the same request every `IDLE`
/// runs no index, so nothing that counts index builds can see it.
static LANE_PASSES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many requests any lane in this process has evaluated.
pub fn lane_passes() -> usize {
    LANE_PASSES.load(std::sync::atomic::Ordering::Relaxed)
}

/// True while the lane still needs `gen_id` — it is either the outstanding
/// request or the one being indexed right now. The outstanding one stays
/// pinned until a newer request replaces it; that is the generation the newest
/// build just promoted, which GC pins anyway.
///
/// The seal hands the lane a directory and then GC runs on its own schedule:
/// five newer generations inside the lane's `IDLE`+index window (ordinary
/// continuous editing) and `remove_dir_all` takes the tree out from under it.
/// The coverage guard in [`publish_bundle`] makes that *safe* — a zero-page
/// walk is refused rather than published — but safe still costs a wasted pass
/// and a stale index until the next edit. Pinning makes it not happen.
pub fn is_indexing(mp: &MossPaths, gen_id: &str) -> bool {
    IN_FLIGHT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(mp.root())
        .is_some_and(|set| set.contains(gen_id))
}

/// Ask the lane to index the generation `gen_id`, whose page set is `want`.
///
/// Called from the seal tail once the generation has been promoted — the first
/// moment the tree is both complete and immutable. Cheap and non-blocking: it
/// publishes a value and returns.
///
/// No-op outside a tokio runtime (unit tests that build a manifest by hand),
/// which is what keeps the lane from being a hidden dependency of every caller.
pub fn request(mp: &MossPaths, gen_id: &str, want: PageSet) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let key = mp.root().to_path_buf();
    let req = LaneRequest {
        root: key.clone(),
        index_dir: mp.index_dir(),
        gen_id: gen_id.to_string(),
        gen_dir: mp.generation_dir(gen_id),
        want,
    };
    IN_FLIGHT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(key.clone())
        .or_default()
        .insert(gen_id.to_string());

    let mut lanes = LANES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(tx) = lanes.get(&key) {
        // `send` fails only if the lane task is gone; fall through and respawn.
        if tx.send(req.clone()).is_ok() {
            return;
        }
    }
    let (tx, rx) = tokio::sync::watch::channel(req);
    lanes.insert(key, tx);
    tokio::spawn(run_lane(rx));
}

/// The lane loop. Level-triggered, debounced, and skips entirely when the
/// requested page set is the one already published — every no-op save, and
/// where the ~5.2 s goes.
///
/// **It parks between requests.** Every pass is driven by a value it has not
/// seen before; nothing runs a timer in the idle state. An earlier shape
/// re-evaluated the last request every `IDLE` — a wakeup plus a ~440-entry JSON
/// parse every 2 s for the life of the app, and on a folder whose index kept
/// failing, a whole-corpus pagefind run every ~7 s forever.
async fn run_lane(mut rx: tokio::sync::watch::Receiver<LaneRequest>) {
    // The channel is created carrying the first request, and `changed()` only
    // reports *later* sends, so the first pass is seeded from the current value.
    let mut next = Some(rx.borrow_and_update().clone());
    loop {
        LANE_PASSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let req = match next.take() {
            Some(req) => req,
            None => {
                // Idle. No timer, no receipt read — just parked on the channel.
                if rx.changed().await.is_err() {
                    return; // every sender dropped: the folder closed
                }
                rx.borrow_and_update().clone()
            }
        };
        let Some(req) = quiesce(&mut rx, req).await else {
            return;
        };

        if read_receipt(&req.index_dir).map(|r| r.fp()) != Some(req.want.fp) {
            index_one(&req, rx.clone()).await;
        }

        // Release every pin except the request still outstanding (if any): this
        // pass is done with its generation, and the next one re-pins its own.
        let outstanding = rx.borrow().gen_id.clone();
        if let Some(set) = IN_FLIGHT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&req.root)
        {
            set.retain(|g| *g == outstanding);
        }
    }
}

/// Index one request on the blocking pool and log what happened.
async fn index_one(req: &LaneRequest, watcher: tokio::sync::watch::Receiver<LaneRequest>) {
    let wanted = req.want;
    let for_worker = req.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        // Cooperative cancellation: a newer page set has been requested, so
        // the bundle this pass would publish is already superseded.
        let cancelled = || watcher.borrow().want.fp != wanted.fp;
        search::build_search_index_cancellable(&for_worker.gen_dir, &cancelled).and_then(
            |produced| match produced {
                Some(produced) => with_holding(&for_worker.index_dir, |h| {
                    publish_bundle(h, &for_worker.index_dir, wanted, &produced)
                })
                .map(Some),
                None => Ok(None),
            },
        )
    })
    .await;

    match outcome {
        Ok(Ok(Some(receipt))) => log::info!(
            target: "search",
            "search index: {} file{} published for page set {} ({} pages)",
            receipt.files.len(),
            if receipt.files.len() == 1 { "" } else { "s" },
            receipt.fp,
            receipt.pages,
        ),
        Ok(Ok(None)) => {
            log::debug!(target: "search", "search index superseded before the gzip pass")
        }
        Ok(Err(e)) => log::warn!(target: "search", "search index not generated: {}", e),
        Err(e) => log::warn!(target: "search", "search index worker failed: {}", e),
    }
}

/// Hold `req` until `IDLE` of quiet — but never longer than `MAX_DEFER`, or a
/// vault under sustained editing would never be indexed at all. Returns the
/// latest request seen, or `None` once every sender has dropped.
async fn quiesce(
    rx: &mut tokio::sync::watch::Receiver<LaneRequest>,
    req: LaneRequest,
) -> Option<LaneRequest> {
    let first = Instant::now();
    let mut latest = req;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(IDLE) => return Some(latest),
            changed = rx.changed() => {
                changed.ok()?;
                latest = rx.borrow_and_update().clone();
                if first.elapsed() >= MAX_DEFER {
                    return Some(latest);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Publish path
// ---------------------------------------------------------------------------

/// Bring the index up to date with the generation on `current`, synchronously.
///
/// Returns `true` when the bundle moved — i.e. the sealed manifest a publish is
/// about to upload names a *stale* index and the caller must rebuild before
/// pushing. Returns `false` (the common case) when the lane has already
/// published a receipt for this exact page set, which is what makes this a
/// no-cost check on an idle vault.
///
/// This is ADR-045's "the publish path calls `settle()`": the staleness budget
/// the lane buys for the edit loop is explicitly not extended to deploy.
pub fn settle_for_publish(mp: &MossPaths, enabled: bool) -> bool {
    if !enabled {
        return false;
    }
    let index_dir = mp.index_dir();
    // `hashes.json` is the persisted sealed manifest of the generation on
    // `current`, so its HTML entries are exactly the page set a publish ships.
    // That pairing is why `advertise_sealed` must not persist a *superseded*
    // seal's manifest: this function would then fingerprint one generation's
    // page set and index another's bytes.
    let hashes: SiteHashes = std::fs::read_to_string(mp.hashes())
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();
    let want = PageSet::of(&hashes.files);

    with_holding(&index_dir, |h| {
        if let Some(receipt) = read_receipt(&index_dir) {
            let holding = holding_dir(&index_dir, receipt.fp());
            // `all` over an empty list is vacuously true, so a bundle covering
            // no pages must not be allowed to satisfy a page set that has them.
            let intact = receipt.pages >= want.pages
                && receipt.files.iter().all(|(rel, _)| holding.join(rel).is_file());
            if receipt.fp() == want.fp && intact {
                return false;
            }
        }

        // Resolve `current` before handing it to the indexer: it is a symlink,
        // and the directory walk does not descend through one.
        let generation =
            std::fs::canonicalize(mp.current_ptr()).unwrap_or_else(|_| mp.current_ptr());
        match index_now(h, &index_dir, &generation, want) {
            Ok(receipt) => {
                log::info!(
                    target: "search",
                    "publish: re-indexed {} bundle file{} for the generation being published",
                    receipt.files.len(),
                    if receipt.files.len() == 1 { "" } else { "s" },
                );
                true
            }
            Err(e) => {
                // Publishing without search beats not publishing.
                log::warn!(target: "search", "publish: search index not generated: {}", e);
                false
            }
        }
    })
}

#[cfg(test)]
#[path = "search_lane_tests.rs"]
mod tests;
