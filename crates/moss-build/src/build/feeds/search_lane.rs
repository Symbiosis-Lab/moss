//! The search bundle: built when something reads it, adopted from a receipt.
//!
//! Pagefind's cost scales with **corpus size**, not with a build's delta:
//! about 1.2 s wall and 4 CPU s on a 361-page site, on every index. So a build
//! that keeps a process alive never indexes. Each of the two readers of the
//! index settles it when it reads ([`settle`]): the preview's search mount,
//! when the panel loads `pagefind.js`, and a publish of an existing preview.
//! Settling flushes a pending seal, then indexes the frozen `current`
//! generation if the receipt does not already cover its page set:
//!
//! ```text
//!   search opened / publish ──settle──► seal flushed, current = latest render
//!                                         │ index current/ (unless the receipt covers it)
//!                                         ▼
//!                       .moss/build.nosync/index/<fp>/  +  receipt.json
//!   next build ──adopt_into(receipt)──► staging + PendingManifest
//! ```
//!
//! A process that exits after its build ([`Freshness::Now`]) has no later
//! reader to settle for it, so it indexes its staged pages on a worker the
//! seal joins ([`SearchJob`]).
//!
//! # What must not change, and does not
//!
//! `PendingManifest` is mark-and-sweep: `seal()` prunes each output bucket to
//! the paths this build touched, and `remove_stale_files` deletes every staging
//! file the sealed manifest omits. `_moss/pagefind/**` gets no exemption, so a
//! build that skipped re-registering would have the deploy diff read the bundle
//! as *removed* and delete it off the live site. Hence [`adopt_into`] runs on
//! **every** build, inside the build's own stage-write span, registering the
//! receipt's `(path, hash)` entries with zero byte reads.
//!
//! # Which tree is indexed
//!
//! The **frozen generation**, never `staging/`: staging is what the next build
//! rewrites, and the generation is ship-transformed (`data-source-line`
//! stripped, so preview search matches deploy search). The preview serves the
//! mount from the receipt's holding dir ([`held_file`]), so what a search reads
//! is the bundle the last settle published, not the copy a build adopted
//! before it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use crate::build::feeds::search;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::moss_paths::MossPaths;
use crate::types::content::SiteHashes;

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
    /// next real index, which is what a one-shot build's bundle wants, having
    /// indexed `staging/` rather than a generation.
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

/// What the last settle published: which page set it indexed, and the
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
    /// Pages in the page set the indexer leaves out on purpose (redirect stubs,
    /// the not-found page), so `pages + left_out` is the page set it covers.
    #[serde(default)]
    pub left_out: usize,
    /// `(path within the bundle, xxh3 content hash)`.
    pub files: Vec<(String, String)>,
    /// [`search::corpus_key`] of the staged pages a one-shot build indexed;
    /// `None` on a bundle a settle indexed from a generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staged: Option<String>,
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
/// then acts on it — takes one. Without it a settle and a build interleave: the
/// settle's [`gc_holding`] deletes `<fpA>/` while a build is halfway through
/// copying it into staging, the build then registers nothing *and* deletes the
/// receipt the settle just wrote for `<fpB>`, and the sweep takes the bundle off
/// the live site — the mark-and-sweep hole this rule closes, from the other side.
///
/// Not a substitute for the build's stage-write guard, which orders builds
/// against each other; this orders a *settle* against a build. A settle never
/// takes the stage-write lock, so the pair cannot deadlock.
pub struct Holding(());

/// A folder's two locks. `holding` orders a settle against a build, which
/// takes it in [`begin`]. `settle` lets one settle run at a time, so the
/// router's concurrent reads of the mount index a page set once; no build
/// takes it, and the index runs under it, so a build never waits out an index.
#[derive(Default)]
struct FolderLocks {
    holding: Mutex<()>,
    settle: Mutex<()>,
}

static FOLDER_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<FolderLocks>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn folder_locks(index_dir: &Path) -> Arc<FolderLocks> {
    let mut locks = FOLDER_LOCKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Arc::clone(locks.entry(index_dir.to_path_buf()).or_default())
}

/// Run `f` with the holding area for `index_dir` exclusively held.
fn with_holding<T>(index_dir: &Path, f: impl FnOnce(&Holding) -> T) -> T {
    let locks = folder_locks(index_dir);
    let _guard = locks.holding.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&Holding(()))
}

/// Absolute path of the directory holding the bundle for `fp`.
fn holding_dir(index_dir: &Path, fp: PageSetFp) -> PathBuf {
    index_dir.join(fp.hex())
}

/// What every bundle file laid into `staging/` holds, by the stat it had
/// right after the copy, so an adoption copies only the files whose hash moved
/// or whose stat no longer vouches: a one-page edit changes a handful of index
/// chunks and one fragment out of ~440 files.
fn staged_links(index_dir: &Path, stage_dir: &Path) -> crate::build::lifecycle::cas_heal::StagedLinks {
    crate::build::lifecycle::cas_heal::StagedLinks::at(index_dir.join("staged-links.json"), stage_dir)
}

/// Write `produced`'s files into `index_dir/<fp>/`, then the receipt. Older
/// holding dirs for the same folder are removed — a bundle nobody's receipt
/// names is dead weight, and the holding area is not swept by anything else.
///
/// # The coverage guard
///
/// **A bundle is stamped with `want.fp` only if it demonstrably covers
/// `want`.** A short bundle under a complete page set's fingerprint is not a
/// degraded result but an *absorbing* one: [`settle`]'s `intact` test is
/// vacuously true over an empty file list, and [`adopt_into`] registers nothing — so the sweep
/// takes the bundle out of staging and the deploy diff deletes the index off
/// the live site, with nothing scheduled to re-index.
///
/// Both halves are needed. `skipped > 0` catches a page moss could not read (an
/// iCloud-evicted `.html` under `.moss/build.nosync/`); `pages < want.pages` catches a
/// tree that is not there at all — a generation GC'd mid-debounce walks to zero
/// pages, indistinguishable from "this site has no indexable content".
///
/// A refusal is an ordinary transient failure: no receipt is written, so the
/// previous good bundle stays adopted and the next settle re-indexes.
fn publish_bundle(
    _holding: &Holding,
    index_dir: &Path,
    want: PageSet,
    staged: Option<&str>,
    produced: &search::SearchIndex,
) -> Result<BundleReceipt, String> {
    if produced.skipped > 0 || produced.pages + produced.left_out < want.pages {
        let expected = want.pages.saturating_sub(produced.left_out).max(produced.pages + produced.skipped);
        return Err(format!(
            "indexed {} of {} pages ({} unreadable) — refusing to publish a partial \
             bundle as page set {}",
            produced.pages,
            expected,
            produced.skipped,
            want.fp.hex(),
        ));
    }

    let _phase = crate::build::phase::PhaseTrace::start("search_publish");
    let fp = want.fp;
    let files = &produced.files;
    let dir = holding_dir(index_dir, fp);
    let mut receipt = BundleReceipt {
        fp: fp.hex(),
        pages: produced.pages,
        left_out: produced.left_out,
        files: Vec::with_capacity(files.len()),
        staged: staged.map(str::to_string),
    };
    // A one-shot build publishes every bundle into the same `UNKNOWN`
    // directory, so a file the receipt there already names with the same hash
    // is on disk as it is. The receipt goes first, so a publish torn halfway
    // reads back as no receipt rather than one naming bytes it no longer has.
    let previous: HashMap<String, String> = read_receipt(index_dir)
        .filter(|r| r.fp == receipt.fp)
        .map(|r| r.files.into_iter().collect())
        .unwrap_or_default();
    let mut receipt_dropped = false;
    for file in files {
        let hash = crate::build::assets::paths::compute_binary_hash(&file.bytes);
        let dest = dir.join(&file.rel_path);
        if previous.get(&file.rel_path) != Some(&hash) || !crate::build::io_utils::output_present(&dest) {
            if !receipt_dropped && !previous.is_empty() {
                // allow:unlink the search index under .moss/build.nosync/index, not staging
                let _ = std::fs::remove_file(index_dir.join("receipt.json"));
                receipt_dropped = true;
            }
            crate::build::io_utils::write_output(&dest, &file.bytes)
                .map_err(|e| format!("failed to write search bundle file {:?}: {}", dest, e))?;
        }
        receipt.files.push((file.rel_path.clone(), hash));
    }
    receipt.files.sort();

    let json = serde_json::to_vec_pretty(&receipt)
        .map_err(|e| format!("failed to serialize search receipt: {}", e))?;
    crate::build::io_utils::write_output(&index_dir.join("receipt.json"), &json)
        .map_err(|e| format!("failed to write search receipt: {}", e))?;
    drop(_phase);
    gc_holding(index_dir, fp, &receipt);
    Ok(receipt)
}

/// Drop every holding directory except `keep`, and every file in it that
/// `kept` does not name: a bundle published over an older one in the same
/// directory leaves the older one's fragments and chunks behind.
fn gc_holding(index_dir: &Path, keep: PageSetFp, kept: &BundleReceipt) {
    let _phase = crate::build::phase::PhaseTrace::start("search_gc");
    let keep = keep.hex();
    let named: HashSet<&str> = kept.files.iter().map(|(rel, _)| rel.as_str()).collect();
    let keep_dir = index_dir.join(&keep);
    for entry in walkdir::WalkDir::new(&keep_dir).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file()) {
        let rel = entry.path().strip_prefix(&keep_dir).ok().and_then(|p| p.to_str()).map(|p| p.replace('\\', "/"));
        if rel.is_some_and(|rel| !named.contains(rel.as_str())) {
            // allow:unlink the search index under .moss/build.nosync/index, not staging
            let _ = std::fs::remove_file(entry.path());
        }
    }
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
    /// A one-shot build indexed its staged pages.
    Indexed(usize),
    /// No receipt yet: nothing has read search since it was enabled, or
    /// `.moss/build.nosync/index` was deleted. Registered nothing; the next
    /// [`settle`] indexes.
    Unindexed,
    /// The receipt named files that are not on disk. Registered nothing and
    /// dropped the receipt; the next [`settle`] re-indexes.
    Diverged,
    /// A receipt exists and says this page set has no indexable content.
    Empty,
}

/// Re-read `[site].search` for the seal tail or a host's publish preflight,
/// after the build's resolved `SiteConfig` has been consumed.
pub fn enabled_for(mp: &MossPaths) -> bool {
    let Some(project_root) = mp.root().parent() else {
        return false;
    };
    crate::build::site_config::get_site_search(&project_root.to_string_lossy())
        .ok()
        .flatten()
        .unwrap_or(false)
}

/// Whether anything will settle the index after this build.
///
/// A process that exits when the build returns (CLI `moss build`,
/// `build_sync`, the snapshot harness) has no later reader to settle for it, so
/// a receipt adopted verbatim would freeze the index at whatever the *first*
/// such build produced: add a page and it is never searchable, delete one and
/// search still returns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// A reader settles the index when it needs it. Adopt the receipt.
    OnRead,
    /// Nothing will index after this build. Index the staged pages now, unless
    /// the bundle held from the last such build was indexed from the same ones.
    Now,
}

impl Freshness {
    /// `exits_after_build` is the whole of the question: that flag is exactly
    /// "this process drops its runtime when the build returns".
    pub fn of(exits_after_build: bool) -> Self {
        if exits_after_build { Self::Now } else { Self::OnRead }
    }
}

/// What [`begin`] leaves the caller: a finished adoption, or a one-shot
/// build's index still to compute.
pub enum Begun {
    Done(Adoption),
    /// Run it on a worker the seal joins, so it overlaps the background phase.
    Indexing(SearchJob),
}

impl Begun {
    /// The index still owed, if any; a finished adoption is only logged.
    pub fn into_job(self) -> Option<SearchJob> {
        match self {
            Begun::Done(adoption) => {
                log::debug!(target: "search", "search bundle: {:?}", adoption);
                None
            }
            Begun::Indexing(job) => Some(job),
        }
    }
}

/// A one-shot build's index, its pages already read.
///
/// The read happens inside the build's stage-write span, after the last
/// `.html` write into staging, so the snapshot is this build's whole output.
/// What remains touches only the holding area and `_moss/pagefind/**`, which
/// no other worker writes, and it runs after the stage-write guard is
/// released, as every background worker's staging write does. What covers it
/// is the seal: it awaits this worker before it takes the guard again, and a
/// one-shot build holds the cross-process folder lock from its render through
/// that seal. Its worker sends the laid bundle as one message, so
/// the manifest gets every file or none, and the seal drains every worker's
/// sender before it seals.
pub struct SearchJob {
    index_dir: PathBuf,
    stage_dir: PathBuf,
    read: search::PageRead,
    key: String,
    t_read: std::time::Instant,
}

impl SearchJob {
    /// Run [`Self::finish`] beside the build's background workers and send
    /// the laid bundle to the seal as one message.
    pub(crate) fn spawn(
        self,
        workers: &mut tokio::task::JoinSet<Result<(), crate::build::background::BuildError>>,
        tx: tokio::sync::mpsc::Sender<crate::build::coordinator::EmitMessage>,
    ) {
        crate::build::background::spawn_traced_worker(workers, move || {
            let (adoption, laid) = self.finish();
            log::debug!(target: "search", "search bundle: {:?}", adoption);
            if !laid.0.is_empty() {
                let entries = laid.0.into_iter().map(|(sp, hash)| (sp.as_str().to_string(), hash)).collect();
                let _ = tx.blocking_send(crate::build::coordinator::EmitMessage::FileSet { entries, bucket: HashBucket::Files });
            }
            Ok(())
        });
    }

    /// Index, publish and lay the bundle. The laid files are empty when any
    /// step fails.
    pub fn finish(self) -> (Adoption, Laid) {
        let _phase = crate::build::phase::PhaseTrace::start("search_index");
        let SearchJob { index_dir, stage_dir, read, key, t_read } = self;
        with_holding(&index_dir, |h| {
            let published = search::index_pages(read, t_read)
                .and_then(|produced| publish_bundle(h, &index_dir, PageSet::UNKNOWN, Some(&key), &produced));
            lay_indexed(h, &index_dir, &stage_dir, published)
        })
    }
}

/// Every file of a bundle laid into staging, as `(served path, hash)`: register
/// all of them together, never some.
#[derive(Debug, Default)]
pub struct Laid(Vec<(ServedPath, String)>);

impl Laid {
    fn register(&self, pending: &mut PendingManifest) {
        for (sp, hash) in &self.0 {
            pending.register_hashed(sp, hash, HashBucket::Files);
        }
    }
}

/// [`begin`], with any index computed in place. Tests only: the pipeline runs
/// the index on a worker.
#[cfg(test)]
pub fn adopt_into(
    mp: &MossPaths,
    stage_dir: &Path,
    pending: &mut PendingManifest,
    enabled: bool,
    freshness: Freshness,
) -> Adoption {
    match begin(mp, stage_dir, pending, enabled, freshness) {
        Begun::Done(adoption) => adoption,
        Begun::Indexing(job) => {
            let (adoption, laid) = job.finish();
            laid.register(pending);
            adoption
        }
    }
}

/// Copy the receipt's bundle into `stage_dir` and register every entry in
/// `pending`. Call once per build, inside the build's own stage-write span.
///
/// **Registration is unconditional**, on every branch and every build, because
/// `PendingManifest` is mark-and-sweep (see the module docs). Only the
/// *computation* is demand-driven. A [`Begun::Indexing`] lays and registers
/// later, outside that span (see [`SearchJob`] for what covers it): run it on
/// a worker whose registrations the seal drains.
///
/// Costs ~440 stats (1–2 ms) in the steady state and no byte reads at all: the
/// hashes come from the receipt, and a file whose recorded stat still vouches
/// that it holds the receipt's hash is not re-copied.
pub fn begin(
    mp: &MossPaths,
    stage_dir: &Path,
    pending: &mut PendingManifest,
    enabled: bool,
    freshness: Freshness,
) -> Begun {
    if !enabled {
        return Begun::Done(Adoption::Disabled);
    }
    let index_dir = mp.index_dir();
    // Held across read-receipt → lay → drop-receipt, so a settle cannot GC the
    // holding directory this call is reading, nor have its brand-new receipt
    // deleted by the divergence branch below.
    with_holding(&index_dir, |h| {
        if freshness == Freshness::Now {
            return read_staging(h, &index_dir, stage_dir, pending);
        }
        Begun::Done(adopt_receipt(h, &index_dir, stage_dir, pending))
    })
}

fn adopt_receipt(h: &Holding, index_dir: &Path, stage_dir: &Path, pending: &mut PendingManifest) -> Adoption {
    let Some(receipt) = read_receipt(index_dir) else {
        return Adoption::Unindexed;
    };

    if receipt.files.is_empty() {
        return Adoption::Empty;
    }

    match lay(h, index_dir, &receipt, stage_dir) {
        Some(laid) => {
            laid.register(pending);
            Adoption::Adopted(laid.0.len())
        }
        None => {
            // Receipt/disk divergence: an external delete, or the holding
            // area evicted by a cloud provider. Registering a path that is
            // not on disk is the deploy error `manifest.rs` documents, so
            // register nothing and drop the receipt; the next settle
            // re-indexes.
            drop_receipt_if_still(h, index_dir, &receipt);
            log::info!(
                target: "search",
                "search bundle {} is incomplete on disk — dropped, will re-index",
                receipt.fp,
            );
            Adoption::Diverged
        }
    }
}

/// What a one-shot build ([`Freshness::Now`]) does with search: serve the
/// bundle it holds when [`search::corpus_key`] says that bundle was indexed from
/// exactly the pages staging holds now, and hand back a [`SearchJob`] over the
/// pages it read otherwise. A bundle a settle indexed carries no key, so it is
/// never taken for one.
fn read_staging(h: &Holding, index_dir: &Path, stage_dir: &Path, pending: &mut PendingManifest) -> Begun {
    let _phase = crate::build::phase::PhaseTrace::start("search_read");
    let t_read = std::time::Instant::now();
    let read = search::read_pages(stage_dir);
    let key = crate::phase!("search_key", { search::corpus_key(&read.pages) });
    let held = read_receipt(index_dir).filter(|receipt| read.unreadable == 0 && receipt.staged.as_deref() == Some(key.as_str()));
    if let Some(receipt) = held {
        if receipt.files.is_empty() {
            return Begun::Done(Adoption::Empty);
        }
        if let Some(laid) = lay(h, index_dir, &receipt, stage_dir) {
            laid.register(pending);
            crate::build::phase::record_count("search_bundle_reused", 1);
            return Begun::Done(Adoption::Adopted(laid.0.len()));
        }
    }
    Begun::Indexing(SearchJob { index_dir: index_dir.to_path_buf(), stage_dir: stage_dir.to_path_buf(), read, key, t_read })
}

/// Lay a bundle this build just indexed.
fn lay_indexed(
    h: &Holding,
    index_dir: &Path,
    stage_dir: &Path,
    published: Result<BundleReceipt, String>,
) -> (Adoption, Laid) {
    match published {
        Ok(receipt) => match lay(h, index_dir, &receipt, stage_dir) {
            Some(laid) if laid.0.is_empty() => (Adoption::Empty, laid),
            Some(laid) => (Adoption::Indexed(laid.0.len()), laid),
            None => (Adoption::Diverged, Laid::default()),
        },
        Err(e) => {
            // Transient (a page that will not parse, a full disk, a tree half
            // of which could not be read). No receipt is written, so the next
            // build retries.
            log::info!(target: "search", "search index not generated: {}", e);
            (Adoption::Diverged, Laid::default())
        }
    }
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

/// Stat every receipt entry, copy the ones staging does not provably hold
/// (see `StagedLinks`), and return them all to register. `None` if any file is
/// missing or any copy fails — and **nothing is registered in that case**: a partial registration is the worst
/// outcome of the three, since the sweep deletes the unregistered remainder and
/// `pagefind.js` is then left fetching shards that 404. Hence stat all, copy
/// all, then hand back all.
fn lay(
    _holding: &Holding,
    index_dir: &Path,
    receipt: &BundleReceipt,
    stage_dir: &Path,
) -> Option<Laid> {
    let _phase = crate::build::phase::PhaseTrace::start("search_lay");
    let holding = holding_dir(index_dir, receipt.fp());
    let mut planned: Vec<(ServedPath, &str, PathBuf)> = Vec::with_capacity(receipt.files.len());
    for (rel, hash) in &receipt.files {
        let sp = ServedPath::for_search_asset(rel).ok()?;
        let src = holding.join(rel);
        if !crate::build::io_utils::output_present(&src) {
            return None;
        }
        planned.push((sp, hash, src));
    }

    use crate::build::lifecycle::cas_heal::Placement;
    let mut links = staged_links(index_dir, stage_dir);
    let mut laid = 0;
    for (sp, hash, src) in &planned {
        match links.copy(src, hash, &stage_dir.join(sp.as_str())) {
            Placement::Held => {}
            Placement::Linked => laid += 1,
            Placement::Unverified(e) => {
                log::info!(target: "search", "could not check {} in staging: {}", sp, e);
                return None;
            }
            Placement::Failed(e) => {
                log::info!(target: "search", "failed to lay {} into staging: {}", sp, e);
                return None;
            }
        }
    }
    links.save();
    crate::build::phase::record_count("search_files_laid", laid);
    Some(Laid(planned.into_iter().map(|(sp, hash, ..)| (sp, hash.to_string())).collect()))
}

// ---------------------------------------------------------------------------
// Settling: the index is built when something reads it
// ---------------------------------------------------------------------------

/// Bring the index up to date with the folder's latest render, and report
/// whether `current` ships a bundle other than the receipt's.
///
/// A pending seal is materialized first (`seal_phase::settle`), so `current`
/// and its `hashes.json` hold the render on screen rather than the one before
/// its debounced promote. Then, unless the receipt already covers `current`'s
/// page set, this indexes `current` and publishes the bundle.
///
/// Two callers read the index, and each settles before it does: the preview's
/// search mount ([`held_file`]'s caller in `ops/serve/router.rs`) and a publish
/// of an existing preview, which rebuilds when this returns `true` so the
/// manifest it uploads names the fresh bundle. The answer is about `current`,
/// not about this call: a search read since the last build published a bundle
/// that build never adopted. A one-shot build indexes inside
/// the build instead ([`Freshness::Now`]).
pub async fn settle(mp: &MossPaths, enabled: bool) -> bool {
    if !enabled {
        return false;
    }
    crate::build::seal_phase::settle(mp).await;
    let mp = mp.clone();
    tokio::task::spawn_blocking(move || settle_index(&mp)).await.unwrap_or(false)
}

fn settle_index(mp: &MossPaths) -> bool {
    let index_dir = mp.index_dir();
    let locks = folder_locks(&index_dir);
    let _one_settle = locks.settle.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // Resolved before the manifest is read: no lock holds a promote off, so
    // `index_current` checks after its read that `current` still names this
    // generation, or the bytes it indexed may not be the page set read below.
    let generation = resolve_current(mp);
    // `hashes.json` is the persisted sealed manifest of the generation on
    // `current`, so its HTML entries are exactly the page set `current` holds.
    // That pairing is why `advertise_sealed` must not persist a *superseded*
    // seal's manifest: this function would then fingerprint one generation's
    // page set and index another's bytes.
    let hashes: SiteHashes = std::fs::read_to_string(mp.hashes())
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();
    let want = PageSet::of(&hashes.files);

    let covering = |_: &Holding| {
        read_receipt(&index_dir).filter(|receipt| {
            let holding = holding_dir(&index_dir, receipt.fp());
            // `all` over an empty list is vacuously true, so a bundle covering
            // no pages must not be allowed to satisfy a page set that has them.
            receipt.fp() == want.fp
                && receipt.pages + receipt.left_out >= want.pages
                && receipt.files.iter().all(|(rel, _)| holding.join(rel).is_file())
        })
    };
    match with_holding(&index_dir, covering).or_else(|| index_current(mp, &generation, want, covering)) {
        Some(receipt) => !ships(&hashes, &receipt),
        // Publishing or searching without a fresh index beats neither.
        None => false,
    }
}

/// Runs where [`settle_index`] starts the indexer, so a test can act while an
/// index is in flight.
#[cfg(test)]
static WHILE_INDEXING: Mutex<Option<Box<dyn FnOnce() + Send>>> = Mutex::new(None);

/// `current` resolved: it is a symlink, and the indexer's directory walk does
/// not descend through one.
fn resolve_current(mp: &MossPaths) -> PathBuf {
    std::fs::canonicalize(mp.current_ptr()).unwrap_or_else(|_| mp.current_ptr())
}

/// Index `generation` with the holding area free, then publish the bundle for
/// `want` unless `covering` finds one already there. Nothing pins the
/// generation: the indexer reads every page into memory before it segments
/// and indexes, so the tree only has to outlive that read, and a read that a
/// promote's GC cut short is refused by [`publish_bundle`]. A promote that
/// left the read whole still took `want` off these bytes, so that read is
/// refused too, as transient: the next settle indexes the new generation.
fn index_current(
    mp: &MossPaths,
    generation: &Path,
    want: PageSet,
    covering: impl FnOnce(&Holding) -> Option<BundleReceipt>,
) -> Option<BundleReceipt> {
    let index_dir = mp.index_dir();
    #[cfg(test)]
    if let Some(hook) = WHILE_INDEXING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
        hook();
    }
    let _phase = crate::build::phase::PhaseTrace::start("search_index");
    let t_read = std::time::Instant::now();
    let read = search::read_pages(generation);
    if resolve_current(mp) != generation {
        log::info!(target: "search", "search index not generated: a promote moved current during the read");
        return None;
    }
    let published = search::index_pages(read, t_read).and_then(|produced| {
        with_holding(&index_dir, |h| covering(h).map_or_else(|| publish_bundle(h, &index_dir, want, None, &produced), Ok))
    });
    match published {
        Ok(receipt) => {
            log::info!(
                target: "search",
                "search index: {} bundle file{} published for page set {} ({} pages)",
                receipt.files.len(),
                if receipt.files.len() == 1 { "" } else { "s" },
                receipt.fp,
                receipt.pages,
            );
            Some(receipt)
        }
        Err(e) => {
            log::info!(target: "search", "search index not generated: {}", e);
            None
        }
    }
}

/// Whether the sealed manifest names exactly the receipt's bundle files.
fn ships(hashes: &SiteHashes, receipt: &BundleReceipt) -> bool {
    let shipped = hashes.files.keys().filter(|key| ServedPath::is_search_asset(key)).count();
    shipped == receipt.files.len()
        && receipt.files.iter().all(|(rel, hash)| {
            ServedPath::for_search_asset(rel)
                .is_ok_and(|sp| hashes.files.get(sp.as_str()) == Some(&crate::types::content::file_entry(hash)))
        })
}

/// The file at `rel` (a path under the search mount) in the bundle the
/// receipt names, if it is there. The preview serves the mount from here, so
/// the entry file and the chunks it names come from one bundle, the newest.
pub fn held_file(mp: &MossPaths, rel: &str) -> Option<PathBuf> {
    let index_dir = mp.index_dir();
    let receipt = read_receipt(&index_dir)?;
    receipt.files.iter().any(|(named, _)| named == rel).then(|| holding_dir(&index_dir, receipt.fp()).join(rel))
}

#[cfg(test)]
#[path = "search_lane_tests.rs"]
mod tests;
