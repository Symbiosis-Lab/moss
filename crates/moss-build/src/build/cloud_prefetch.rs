//! Cloud byte reads and owned atomic cache publications — on threads moss can
//! afford to lose. Structural reads have reserved capacity in this same owner.
//!
//! # The mechanism is the OS's, not moss's
//!
//! A file in a synced folder can be *dataless*: listed in the directory, bytes
//! not on disk. Materializing it needs no provider API at all — from WWDC21
//! "Sync files to the cloud with FileProvider on macOS":
//!
//! > When the kernel detects a read access to a dataless file, that syscall is
//! > paused while your extension is called to fetch the contents of the file.
//! > […] files lose their dataless property before the reads are allowed to
//! > resume.
//!
//! So **to download a file, read it** — the same trigger Finder, Preview and
//! the user's own double-click use. The kernel routes every dataless fault to
//! one system-wide resolver whose registration is exclusive, so this works for
//! iCloud, Google Drive, Dropbox, OneDrive, Box and whatever ships next, and
//! moss never has to know which one it is talking to.
//!
//! **What is uniform is the routing, not the amount fetched.** The kernel
//! delivers every fault to the one resolver; how much of the file that
//! resolver then pulls down is the provider's own policy, and providers
//! disagree — measurably. iCloud materializes a whole 32 MiB file to satisfy a
//! one-byte read; Google Drive fetches 4 MiB and leaves the file dataless. So
//! "moss never has to know which provider" holds only for reads that ask for
//! the whole file. See [`materialize`], which asks for the file rather than a
//! range — a one-byte read cost the 河灣·週刊 vault days of a download that
//! could not progress.
//!
//! The provider schedules transfer and retry. Moss orders only its own bounded
//! read requests: inputs for the visible page move ahead of background files.
//! Two reader slots stay available for those inputs when background reads
//! wedge. There is no provider-specific policy or retry ladder here.
//!
//! # Retry: unbounded, uniform, and bounded by dedup
//!
//! Files *are* re-read — the supervisor re-hands bounded batches of its sweep
//! result every pass. The evidence cuts both ways
//! and lands here:
//!
//! - **For retrying.** Obsidian's materialize-modal retries the same OS
//!   mechanism and "eventually succeeds" where a single attempt does not.
//! - **Against a ladder.** A per-file backoff once paced moss *slower* than the
//!   provider — ~4 minutes of escalation before the rung that worked. What
//!   failed there was the cadence, not the act of asking twice.
//! - **Unknown.** Whether a fresh read re-arms materialization after our
//!   specific `downloadCancelled(byUser: false)` → `materializationFailed`
//!   signature is documented nowhere. It is cheap to attempt, so moss attempts.
//!
//! What makes unbounded retry safe is the dedup set, and the mechanism is worth
//! being explicit about because it is not obvious:
//!
//! | Read | `pending` | Re-handed by the next sweep? |
//! |---|---|---|
//! | returns an error | entry removed | **yes**, on a later pass |
//! | never returns (wedge) | entry stays forever | **no** — `read()` suppresses it |
//!
//! So the file that ate a thread is exactly the file moss will never ask for
//! again. A dead provider costs at most [`READERS`] threads total, no matter
//! how many sweeps run — the retry loop cannot compound the damage.
//!
//! # Then why does this module exist at all?
//!
//! Because a wedged dataless read cannot be abandoned, and moss reads thousands
//! of files where Preview reads one.
//!
//! Opening a real vault (724 dataless files) froze the whole
//! app permanently. `.moss/identity/secret-key` hit a materialization the OS
//! **cancelled and never retried** (`downloadCancelled(byUser: false)` →
//! `materializationFailed`) and **that read never returned** — on the main
//! thread, inside three nested locks. A `read` has no timeout and no interrupt,
//! so the thread was gone for the life of the process. moss never reached
//! `build_folder`; the waiting screen that had already shipped never fired
//! once.
//!
//! Preview survives the same wedge because the blast radius is one document the
//! user can close. moss's is the app.
//!
//! That is what the process-wide fail-fast policy
//! (`platform::set_dataless_fail_fast`) is for — not to avoid *waiting*, but to
//! make the wait **abandonable**: an ordinary read returns `EDEADLK` instead of
//! disappearing. Apple's TN3150 names the option and tells you to handle that
//! errno; WebKit and Apple's own `mtree` do the same.
//!
//! The policy is overridable *per thread*, and xnu checks the thread decoration
//! first, so the rule moss actually wants is expressible directly:
//!
//! > every thread fails fast, except a few that are expendable.
//!
//! Those threads are this module. They call
//! `platform::materialize_on_this_thread()` once and then do nothing else for
//! their whole lives but execute owned file operations. A provider that stops answering costs moss one
//! thread per wedged file and nothing else — and when they are all gone, no
//! progress is possible by any means, which the supervisor reports as a stall.
//!
//! So this is not a download mechanism. It is an isolation boundary around the
//! OS's download mechanism.
//!
//! Everything here is cross-platform and unit-tested on Linux against an
//! injected [`Materializer`]; the OS-specific part is one syscall, in
//! `platform::macos::iopolicy`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use crate::build::cloud_readiness::storage::{Operation, OperationKey, OperationTask, OperationOwner, OperationPolicy};

mod operations;
use operations::{structural_reader_loop, finish_operation};

/// How many reads moss will have outstanding at once.
///
/// Chosen as a **blast radius**: the number of threads moss can lose to a
/// provider that stops answering and still function. Losing all eight is a
/// bounded, reportable condition; an unbounded pool would be a thread leak with
/// no verdict at the end of it.
///
/// It does also bound throughput, and pretending otherwise would be wrong: only
/// eight files can be materializing at a time, and at the 13–16 s per-file floor
/// observed on a real vault that is ~21 minutes for 724 files. The
/// provider batches behind the scenes, so the real figure is better than that —
/// but the ceiling is moss's, not the provider's.
///
/// Raising it is therefore a real trade, not a free win, and it needs evidence
/// from [`Snapshot::oldest_read`] to make: if the oldest read is aging in
/// seconds the pool is the limit and more readers would help; if it is aging in
/// minutes those threads are wedged and more readers would only widen the loss.
pub const READERS: usize = 8;

/// Readers for cache-record publications, on top of [`READERS`]. The system
/// downloads a record the build finds in the cloud ([`crate::platform::request_download`]),
/// so these mostly read and write files that are already local, and the byte
/// readers' width serves: on an evicted Google Drive folder, a first build took
/// 90 s with six and 94 s with twenty-four. They count toward the blast radius
/// above.
pub const PUBLICATION_READERS: usize = 6;

/// Publications admitted without waiting. A queued publication is its record
/// edit (at most `MAX_RECORD_BYTES`, about 1–2 KB in practice) and a closure, so
/// this is a memory bound of a few tens of megabytes, sized for a folder whose
/// whole cache is in the cloud. Past it, an admission waits under the stall rule.
pub const PUBLICATION_WAITING: usize = 32_768;

/// Structural reads cannot be starved by wedged byte downloads. Native calls
/// remain bounded to eight byte readers plus these two reserved workers.
const STRUCTURAL_READERS: usize = 2;

/// Background admission is bounded here for every producer.
pub const BACKGROUND_WAITING: usize = 12;

/// How a file is made local. Injected so the queue and the accounting can be
/// tested without a cloud provider — on Linux there are no dataless files at
/// all, so the real implementation would be untestable and every test vacuous.
pub type Materializer = Arc<dyn Fn(&Path) -> std::io::Result<()> + Send + Sync>;

/// How much of a file the provider has actually delivered: whether the kernel
/// still calls it dataless, and how many 512-byte blocks are on disk.
///
/// Sampled either side of a read so a *partial* delivery names itself in the
/// log instead of looking like a file that simply never arrived. The dataless
/// bit comes from [`icloud::is_evicted`] rather than a second `SF_DATALESS`
/// constant here — one definition of "still in the cloud", not two that can
/// drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Residency {
    dataless: bool,
    /// 512-byte blocks on disk. Zero on platforms that do not report them;
    /// only ever compared against another sample of the same file.
    blocks: u64,
}

fn residency(path: &Path) -> Option<Residency> {
    let dataless = crate::build::icloud::is_evicted(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(path).ok()?;
        Some(Residency { dataless, blocks: meta.blocks() })
    }
    #[cfg(not(unix))]
    {
        std::fs::symlink_metadata(path).ok()?;
        Some(Residency { dataless, blocks: 0 })
    }
}

/// Read `r` until it reports EOF, discarding every byte.
///
/// Separated from the file handling so the part with a decision in it can be
/// tested against a reader that counts, rather than against a real dataless
/// file — which exists on exactly one platform, and only when a provider has
/// decided to evict something.
///
/// Deliberately not `io::copy(r, &mut io::sink())`: the whole purpose is that
/// every byte is genuinely read, and that must not rest on which
/// specializations `io::copy` happens to have now or later. `EINTR` is retried
/// rather than reported — a signal is not the provider refusing.
fn drain_to_eof<R: std::io::Read>(r: &mut R) -> std::io::Result<()> {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match r.read(&mut buf) {
            // A zero-byte file returns `Ok(0)` on the first pass and is
            // perfectly materialized — which is why this is not `read_exact`,
            // whose `UnexpectedEof` would report a failure for a file that is
            // fine.
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// Make `path` local, by whichever route the platform gives us.
///
/// **This used to read a single byte**, on WWDC21's promise that the *file* is
/// materialized to satisfy a read, not the byte. That promise holds only for a
/// provider that does not implement [`NSFileProviderPartialContentFetching`],
/// and Apple states the trigger exactly:
///
/// > To trigger a partial download, an app must use POSIX read operations to
/// > read part of the file. If you clone the entire file, or read the file
/// > using file coordination, the system requests the entire file.
///
/// A one-byte `read` is precisely "read part of the file". Google Drive adopts
/// the protocol and answered moss with one 4 MiB-aligned window, leaving the
/// file dataless; iCloud does not adopt it and always sent everything. Both are
/// correct. moss was asserting a guarantee the OS never gave.
///
/// Measured on real evicted Drive files, macOS 25.5, 2026-08-20:
///
/// | ask | wall | result |
/// |---|---|---|
/// | one byte, 8.5 MB file | 3.89 s | 4 MiB resident, **still dataless** |
/// | one byte again | 0.00 s | no progress — byte 0 is resident, so it faults on nothing |
/// | one byte at offset 20 MiB | — | fetches *that* window, not the head |
/// | read to EOF | 2.48 s | complete |
/// | **file coordination, no read at all** | **4.26 s** | **complete** |
///
/// Row 2 is why the vault stalled for days rather than merely slowly: after the
/// first window lands, every retry returns instantly having fetched nothing and
/// reported success, so moss's 60 s re-ask is an infinite no-op. That is the
/// whole regression: `deployed-article-map.json` is 4,827,583 bytes, needed
/// two windows, got one. Files under 4 MiB were never affected, which is why it
/// survived so long.
///
/// So moss asks for the file rather than for a range. Coordination is the
/// documented way to do that and costs one round trip instead of ⌈size/4 MiB⌉;
/// it needs no bytes to cross into moss at all. Where it is unavailable — every
/// non-macOS platform, or a provider that errors — a full POSIX read is the
/// fallback, which is slower but was measured to work, and also completes a
/// file an earlier partial fetch left half-resident.
///
/// Must only run on a thread that has called
/// `platform::materialize_on_this_thread()`. That was always true for the read;
/// it is far more important for coordination, which **beats** the process-wide
/// fail-fast policy and so can hang a thread that never opted in. See
/// `platform::macos::file_coordination`.
///
/// [`NSFileProviderPartialContentFetching`]: https://developer.apple.com/documentation/fileprovider/nsfileproviderpartialcontentfetching
fn materialize(path: &Path) -> std::io::Result<()> {
    let before = residency(path);

    // Ask for the whole file. On success nothing else is needed — the
    // coordinated call materializes without the accessor block reading a byte.
    if let Err(why) = crate::platform::materialize_whole_file(path) {
        if cfg!(target_os = "macos") {
            log::debug!(
                "cloud: coordinated fetch of {} unavailable ({why}) — falling back to a full read",
                path.display()
            );
        }
        let mut f = std::fs::File::open(path)?;
        drain_to_eof(&mut f)?;
    }

    if let (Some(before), Some(after)) = (before, residency(path)) {
        if after.dataless {
            log::debug!(
                "cloud: {} was asked for in full and is STILL dataless (blocks {} -> {})",
                path.display(),
                before.blocks,
                after.blocks
            );
        } else if before.dataless {
            log::debug!(
                "cloud: {} materialized (blocks {} -> {})",
                path.display(),
                before.blocks,
                after.blocks
            );
        }
    }
    Ok(())
}

/// What the supervisor needs to describe the episode. Not a progress bar — the
/// user-facing count comes from the sweep, which is the authority on what is
/// still in the cloud. This says what *moss* is doing, which is what separates
/// "the provider went quiet" from "moss never asked."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Handed over, not yet picked up by a reader.
    pub waiting: usize,
    /// Being read right now — at most [`READERS`].
    pub in_flight: usize,
    /// Reads that have returned since startup, successfully or not.
    pub done: u64,
    /// The read that has been outstanding longest, and for how long.
    ///
    /// This is the field that makes a stall diagnosable rather than merely
    /// reportable. `in_flight` alone cannot distinguish eight files downloading
    /// healthily (observed floor is 13–16 s each, even for a few hundred bytes)
    /// from eight reads that have made no observable progress. Age is a
    /// diagnostic, not proof of failure; the path names the file, which in the vault this
    /// design came from was the single `.moss/identity/secret-key` that took
    /// the whole app down.
    ///
    /// A wedged file is also, by construction, never handed over again: it
    /// stays in `pending` because its read never returns, so `read()` suppresses
    /// every later attempt. That is what makes the unbounded re-hand-off from
    /// the sweep safe — moss cannot accumulate threads on a dead provider,
    /// because the file that ate the thread is the one it will never re-ask for.
    pub oldest_read: Option<(PathBuf, Duration)>,
}

#[derive(Default)]
pub(crate) struct StructuralArrivals {
    pub retry_admission: bool,
    pub completed: usize,
}

impl StructuralArrivals {
    pub fn wake_count(&self) -> usize { self.completed + usize::from(self.retry_admission) }
}

enum BackgroundWork {
    Bytes(PathBuf),
    Publication(OperationKey, Arc<OperationTask>, Instant),
}

struct Queue {
    structural: VecDeque<(OperationKey, Arc<OperationTask>, Instant)>,
    operation_pending: HashMap<OperationKey, Arc<OperationTask>>,
    structural_in_flight: HashMap<OperationKey, Instant>,
    cache_in_flight: HashMap<OperationKey, Instant>,
    structural_arrivals: HashMap<OperationKey, OperationOwner>,
    capacity_waiters: HashMap<PathBuf, OperationOwner>,
    editor_intents: VecDeque<(OperationKey, OperationOwner)>,
    /// Background byte reads; the sweep feeds them in bounded batches.
    bytes: VecDeque<PathBuf>,
    /// Cache publications, each with the time it may run (a retry waits).
    publications: VecDeque<(OperationKey, Arc<OperationTask>, Instant)>,
    /// Requested page inputs can overtake files already waiting in `fifo`.
    foreground: VecDeque<PathBuf>,
    /// Paths queued or being read. Not a scheduling structure — it just stops
    /// moss reading the same file twice when the sweep and a failed build read
    /// name it in the same breath. A path leaves the set when its read returns,
    /// so a later sweep can ask again.
    pending: HashSet<PathBuf>,
    /// Reads currently outstanding, and when each started. Keyed rather than
    /// counted so a stall can name the file and its age — see
    /// [`Snapshot::oldest_read`].
    in_flight: HashMap<PathBuf, Instant>,
    bytes_in_flight: usize,
    /// Active concrete read failures, pruned by the folder’s next sweep.
    failures: HashSet<PathBuf>,
    /// Reads and operations finished. Counted in the same locked step that
    /// frees their slot, so a snapshot never sees one both done and in flight.
    done: u64,
    /// When a reader last took a job: a publication waiting for room waits
    /// only while the queue is moving.
    last_pop: Instant,
    shutdown: bool,
}

thread_local! {
    /// Set on this pool's reader threads, which must never wait for room only they can make.
    static ON_READER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Publications admitted and publications ended for good, process-wide, for the
/// progress line a build shows while it saves records a provider kept in the cloud.
pub(crate) static PUBLICATIONS_ADMITTED: AtomicU64 = AtomicU64::new(0);
pub(crate) static PUBLICATIONS_FINISHED: AtomicU64 = AtomicU64::new(0);

/// Publications that ended in a failure they will not be retried after,
/// process-wide, for the build record. Each is also logged once, where it ends.
pub(crate) static PUBLICATION_FAILURES: AtomicU64 = AtomicU64::new(0);

/// How long a publication waiting for room waits with no job taken before it gives up.
pub(crate) const PUBLICATION_STALL: Duration = Duration::from_secs(5);

impl Queue {
    fn structural_count(&self) -> usize {
        self.operation_pending.keys().filter(|key| !matches!(key.policy, OperationPolicy::CachePublication { .. })).count()
    }
}

struct Inner {
    queue: Mutex<Queue>,
    /// Signals a reader that a file is available, or that it should exit.
    work: Condvar,
    structural_work: Condvar,
    materialize: Materializer,
    settlement_emitter: Mutex<Option<crate::ops::watch::EventRelay>>,
    bytes_limit: usize,
    publication_limit: usize,
    /// Publications admitted without waiting.
    admission: usize,
    /// This pool's share of [`PUBLICATION_FAILURES`].
    publication_failures: AtomicU64,
}

/// A fixed set of expendable workers for byte reads and owned native file
/// operations. Required structural reads retain their reserved capacity.
pub struct Prefetcher {
    inner: Arc<Inner>,
}

impl Prefetcher {
    /// Start the real thing: threads that opt themselves in to materialization
    /// and then read whatever they are handed.
    pub fn new() -> Self {
        Self::with_lanes(READERS, PUBLICATION_READERS, Arc::new(materialize), true)
    }

    /// Test seam. `opt_in` is false so a test's threads do not touch the
    /// process's I/O policy. Byte reads and publications share the `readers`.
    pub fn with_materializer(readers: usize, materialize: Materializer, opt_in: bool) -> Self {
        Self::with_lanes(readers, 0, materialize, opt_in)
    }

    /// `readers` threads for byte reads, of which all but two may serve the
    /// background, plus `publication_readers` more that publications may use.
    fn with_lanes(readers: usize, publication_readers: usize, materialize: Materializer, opt_in: bool) -> Self {
        let background = readers.saturating_sub(2).max(1);
        let inner = Arc::new(Inner {
            queue: Mutex::new(Queue {
                structural: VecDeque::new(),
                operation_pending: HashMap::new(),
                structural_in_flight: HashMap::new(),
                cache_in_flight: HashMap::new(),
                structural_arrivals: HashMap::new(),
                capacity_waiters: HashMap::new(),
                editor_intents: VecDeque::new(),
                bytes: VecDeque::new(),
                publications: VecDeque::new(),
                foreground: VecDeque::new(),
                pending: HashSet::new(),
                in_flight: HashMap::new(),
                bytes_in_flight: 0,
                failures: HashSet::new(),
                done: 0,
                last_pop: Instant::now(),
                shutdown: false,
            }),
            work: Condvar::new(),
            structural_work: Condvar::new(),
            materialize,
            settlement_emitter: Mutex::new(None),
            bytes_limit: background,
            publication_limit: if publication_readers == 0 { background } else { publication_readers },
            admission: if publication_readers == 0 { BACKGROUND_WAITING } else { PUBLICATION_WAITING },
            publication_failures: AtomicU64::new(0),
        });
        for i in 0..readers + publication_readers {
            let inner = Arc::clone(&inner);
            // Deliberately not retained for `join`: a thread blocked in the
            // kernel on a provider that has stopped answering cannot be
            // interrupted, so joining at shutdown would hang the quit. These
            // are daemons — the process exits and takes them with it. That is
            // the whole point of them being separate threads.
            let _ = std::thread::Builder::new()
                .name(format!("moss-cloud-{i}"))
                .spawn(move || {
                    if opt_in {
                        crate::platform::materialize_on_this_thread();
                    }
                    ON_READER.with(|on| on.set(true));
                    reader_loop(&inner);
                });
        }
        for i in 0..STRUCTURAL_READERS {
            let inner = Arc::clone(&inner);
            let _ = std::thread::Builder::new()
                .name(format!("moss-cloud-structure-{i}"))
                .spawn(move || {
                    if opt_in { crate::platform::materialize_on_this_thread(); }
                    ON_READER.with(|on| on.set(true));
                    structural_reader_loop(&inner);
                });
        }
        Self { inner }
    }

    pub(crate) fn set_settlement_emitter(&self, emit: crate::ops::watch::EventRelay) {
        *self.inner.settlement_emitter.lock().unwrap_or_else(|e| e.into_inner()) = Some(emit);
    }

    /// Hand a file over to be read. Non-blocking, idempotent, and cheap enough
    /// to call from anywhere — a path already queued or in flight is ignored.
    pub fn read(&self, path: &Path) {
        self.enqueue(path, false);
    }

    /// Request a needed page input before queued background materialization.
    /// A read already in flight cannot be interrupted.
    pub fn read_foreground(&self, path: &Path) {
        self.enqueue(path, true);
    }

    fn enqueue(&self, path: &Path, foreground: bool) {
        let mut q = match self.inner.queue.lock() {
            Ok(q) => q,
            // A poisoned queue means a reader panicked mid-file. Dropping this
            // is better than propagating a panic into a build thread; the
            // supervisor's next sweep names the file again.
            Err(_) => return,
        };
        if q.shutdown {
            return;
        }
        if q.pending.contains(path) {
            if foreground {
                if let Some(index) = q.bytes.iter().position(|p| p == path) {
                    if let Some(queued) = q.bytes.remove(index) {
                        q.foreground.push_back(queued);
                        drop(q);
                        self.inner.work.notify_one();
                        return;
                    }
                }
            }
            return;
        }
        if !foreground && q.bytes.len() >= BACKGROUND_WAITING {
            return;
        }
        q.pending.insert(path.to_path_buf());
        if foreground {
            q.foreground.push_back(path.to_path_buf());
        } else {
            q.bytes.push_back(path.to_path_buf());
        }
        drop(q);
        self.inner.work.notify_one();
    }

    pub(crate) fn structural_operation(&self, key: OperationKey, attempt: Operation) -> Option<Arc<OperationTask>> {
        let mut q = self.inner.queue.lock().ok()?;
        if q.shutdown { return None; }
        if let Some(task) = q.operation_pending.get(&key) {
            let task = task.clone();
            let mut state = task.state.lock().unwrap_or_else(|e| e.into_inner());
            if !task.owner_active() {
                let owner = OperationOwner::for_folder(key.folder());
                if owner.active() {
                    // One fresh request waits behind the old native call. It
                    // replaces only a previous intent, never a native worker.
                    state.superseding = Some(crate::build::cloud_readiness::storage::OperationRequest { owner, attempt });
                }
            }
            state.waiters += 1;
            drop(state);
            return Some(task);
        }
        q.capacity_waiters.retain(|_, owner| owner.active());
        q.structural_arrivals.retain(|_, owner| owner.active());
        q.editor_intents.retain(|(_, owner)| owner.active());
        if q.structural_count() >= BACKGROUND_WAITING + STRUCTURAL_READERS {
            let owner = OperationOwner::for_folder(key.folder());
            if owner.active() {
                if matches!(key.policy, OperationPolicy::EditorDirectory { .. }) {
                    if !q.editor_intents.iter().any(|(waiting, _)| waiting == &key) && q.editor_intents.len() < BACKGROUND_WAITING {
                        q.editor_intents.push_back((key, owner));
                    }
                } else if matches!(owner, OperationOwner::Folder(_)) && q.capacity_waiters.len() < BACKGROUND_WAITING {
                    q.capacity_waiters.insert(key.folder().to_path_buf(), owner);
                }
            }
            return None;
        }
        q.editor_intents.retain(|(waiting, _)| waiting != &key);
        let task = Arc::new(OperationTask::new(&key, attempt));
        q.operation_pending.insert(key.clone(), task.clone());
        q.structural.push_back((key, task.clone(), Instant::now()));
        drop(q);
        self.inner.structural_work.notify_one();
        Some(task)
    }

    /// Optional atomic publications share background byte capacity. They can
    /// never occupy the reserved source/tree workers or foreground byte slots.
    #[cfg(test)]
    pub(crate) fn cache_publication(&self, key: OperationKey, attempt: Operation) -> Option<Arc<OperationTask>> {
        self.cache_publication_revision(key, attempt, None)
    }

    pub(crate) fn cache_publication_revision(&self, key: OperationKey, attempt: Operation, revision: Option<Arc<AtomicU64>>) -> Option<Arc<OperationTask>> {
        self.cache_publication_within(key, attempt, revision, Duration::ZERO, PUBLICATION_STALL)
    }

    /// [`cache_publication_revision`](Self::cache_publication_revision), holding
    /// the caller for up to `wait` while [`PUBLICATION_WAITING`] are queued
    /// instead of refusing at once, and only while a reader has taken a job within `stall`: a
    /// provider that stops answering costs the first waiter `stall` and the
    /// rest nothing. A reader thread never waits, since only the readers make room.
    pub(crate) fn cache_publication_within(&self, key: OperationKey, attempt: Operation, revision: Option<Arc<AtomicU64>>, wait: Duration, stall: Duration) -> Option<Arc<OperationTask>> {
        let end = Instant::now() + if ON_READER.with(std::cell::Cell::get) { Duration::ZERO } else { wait };
        let mut q = self.inner.queue.lock().ok()?;
        loop {
            if q.shutdown { return None; }
            if let Some(task) = q.operation_pending.get(&key) { return Some(task.clone()); }
            if q.publications.len() < self.inner.admission { break; }
            let until = end.min(q.last_pop + stall);
            let now = Instant::now();
            if now >= until { return None; }
            q = self.inner.work.wait_timeout(q, until - now).ok()?.0;
        }
        let mut task = OperationTask::new(&key, attempt);
        task.request.owner = OperationOwner::Host;
        task.revision = revision;
        task.state.lock().unwrap().waiters = 0;
        let task = Arc::new(task);
        q.operation_pending.insert(key.clone(), task.clone());
        q.publications.push_back((key, task.clone(), Instant::now()));
        PUBLICATIONS_ADMITTED.fetch_add(1, Ordering::Relaxed);
        drop(q);
        self.inner.work.notify_all();
        Some(task)
    }

    pub(crate) fn structural_recovery_retained(&self, key: &OperationKey) -> bool {
        self.inner.queue.lock().is_ok_and(|q| q.operation_pending.contains_key(key)
            || q.editor_intents.iter().any(|(waiting, owner)| waiting == key && owner.active())
            || q.capacity_waiters.get(key.folder()).is_some_and(OperationOwner::active))
    }

    #[cfg(test)]
    pub(crate) fn structural_receipts_for_test(&self) -> usize {
        self.inner.queue.lock().unwrap().structural_arrivals.len()
    }

    pub(crate) fn take_structural_arrivals(&self, folder: &Path) -> StructuralArrivals {
        let Ok(mut q) = self.inner.queue.lock() else { return StructuralArrivals::default() };
        q.capacity_waiters.retain(|_, owner| owner.active());
        q.structural_arrivals.retain(|_, owner| owner.active());
        let capacity_available = q.structural_count() < BACKGROUND_WAITING + STRUCTURAL_READERS && !q.shutdown;
        let retry_admission = capacity_available && q.capacity_waiters.remove(folder).is_some();
        let keys: Vec<_> = q.structural_arrivals.keys().filter(|key| key.folder() == folder).cloned().collect();
        let completed = keys.len();
        for key in keys { q.structural_arrivals.remove(&key); }
        StructuralArrivals { retry_admission, completed }
    }

    /// Wait for the cache publications queued or being written, retries
    /// included, for as long as they keep landing: the wait ends when none is
    /// left or none has finished for `stall`. `progress` is called at most every
    /// `every` with how many are left. How many had not landed.
    pub(crate) fn drain_publications(&self, stall: Duration, every: Duration, progress: &mut dyn FnMut(usize)) -> usize {
        let pending = |q: &Queue| q.cache_in_flight.len() + q.publications.len();
        let mut q = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
        let (mut left, mut landed_at, mut said_at) = (pending(&q), Instant::now(), Instant::now());
        loop {
            let now = Instant::now();
            if left == 0 || now >= landed_at + stall { return left; }
            if now >= said_at + every { progress(left); said_at = now; }
            // Every finished job notifies `work`.
            let until = (landed_at + stall).min(said_at + every);
            q = match self.inner.work.wait_timeout(q, until.saturating_duration_since(now)) { Ok((q, _)) => q, Err(e) => e.into_inner().0 };
            let now_left = pending(&q);
            if now_left < left { landed_at = Instant::now(); }
            left = now_left;
        }
    }

    /// Drop the publications still queued for a store under `folder`, and the
    /// cache leases their closures hold, when the folder is closed. Ones already
    /// running finish.
    pub(crate) fn drop_publications_under(&self, folder: &Path) -> usize {
        let Ok(mut q) = self.inner.queue.lock() else { return 0 };
        let mut dropped = Vec::new();
        q.publications.retain(|(key, task, _)| {
            let ours = matches!(&key.policy, OperationPolicy::CachePublication { root } if root.starts_with(folder));
            if ours { dropped.push((key.clone(), task.clone())); }
            !ours
        });
        for (key, _) in &dropped { q.operation_pending.remove(key); }
        let n = dropped.len();
        drop(q);
        drop(dropped);
        n
    }

    /// Publications of this pool that ended in a failure.
    pub(crate) fn publication_failures(&self) -> u64 {
        self.inner.publication_failures.load(Ordering::Relaxed)
    }

    /// Queue depth and in-flight count, for the stall diagnosis.
    pub fn snapshot(&self) -> Snapshot {
        let now = Instant::now();
        match self.inner.queue.lock() {
            Ok(q) => Snapshot {
                waiting: q.bytes.len() + q.publications.len() + q.foreground.len() + q.structural.len(),
                in_flight: q.in_flight.len() + q.structural_in_flight.len() + q.cache_in_flight.len(),
                done: q.done,
                oldest_read: q
                    .in_flight
                    .iter()
                    .chain(q.structural_in_flight.iter().map(|(key, started)| (&key.path, started)))
                    .chain(q.cache_in_flight.iter().map(|(key, started)| (&key.path, started)))
                    .min_by_key(|(_, started)| **started)
                    .map(|(path, started)| (path.clone(), now.saturating_duration_since(*started))),
            },
            Err(_) => Snapshot { waiting: 0, in_flight: 0, done: 0, oldest_read: None },
        }
    }

    /// Concrete errors among the sweep's current pending set. Names are a
    /// short sample; the count always covers the whole intersection.
    pub fn failures_for(&self, folder: &Path, pending: &HashSet<PathBuf>) -> (usize, Vec<String>) {
        let paths = self.failure_paths_for(folder, pending);
        let names = paths.iter().take(3).map(|p| {
            p.file_name().unwrap_or(p.as_os_str()).to_string_lossy().into_owned()
        }).collect();
        (paths.len(), names)
    }

    /// Full paths for the sweep's current pending read failures. The pending
    /// intersection is the authority: old queue failures cannot outlive the
    /// sweep's current view of the folder.
    pub fn failure_paths_for(&self, folder: &Path, pending: &HashSet<PathBuf>) -> Vec<PathBuf> {
        let Ok(mut q) = self.inner.queue.lock() else { return Vec::new() };
        q.failures.retain(|path| !path.starts_with(folder) || pending.contains(path));
        let mut paths: Vec<_> = pending
            .iter()
            .filter(|path| path.starts_with(folder) && q.failures.contains(*path))
            .cloned()
            .collect();
        paths.sort();
        paths
    }

    #[cfg(test)]
    pub(crate) fn record_failure_for_test(&self, path: PathBuf) {
        if let Ok(mut q) = self.inner.queue.lock() {
            q.failures.insert(path);
        }
    }

    /// Stop accepting work and release every idle reader. Threads already
    /// blocked in a read stay blocked — see the note in `with_materializer`.
    pub fn shutdown(&self) {
        if let Ok(mut q) = self.inner.queue.lock() {
            q.shutdown = true;
            q.bytes.clear();
            q.publications.clear();
            q.foreground.clear();
            q.pending.clear();
            q.structural.clear();
            q.operation_pending.clear();
            q.capacity_waiters.clear();
            q.editor_intents.clear();
        }
        self.inner.work.notify_all();
        self.inner.structural_work.notify_all();
    }
}

impl Default for Prefetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Prefetcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn reader_loop(inner: &Arc<Inner>) {
    loop {
        let (work, background) = {
            let mut q = match inner.queue.lock() { Ok(q) => q, Err(_) => return };
            loop {
                if q.shutdown { return; }
                if let Some(path) = q.foreground.pop_front() {
                    q.in_flight.insert(path.clone(), Instant::now());
                    break (BackgroundWork::Bytes(path), false);
                }
                let now = Instant::now();
                if q.bytes_in_flight < inner.bytes_limit {
                    if let Some(path) = q.bytes.pop_front() {
                        q.in_flight.insert(path.clone(), now);
                        q.bytes_in_flight += 1;
                        break (BackgroundWork::Bytes(path), true);
                    }
                }
                let publishing = q.cache_in_flight.len() < inner.publication_limit;
                if publishing && q.publications.front().is_some_and(|(_, _, ready_at)| *ready_at <= now) {
                    let (key, task, at) = q.publications.pop_front().unwrap();
                    // A publication may be waiting for this room.
                    q.last_pop = now;
                    inner.work.notify_all();
                    q.cache_in_flight.insert(key.clone(), now);
                    break (BackgroundWork::Publication(key, task, at), true);
                }
                let delay = publishing.then(|| q.publications.front().map(|(_, _, at)| at.saturating_duration_since(now))).flatten();
                q = match delay {
                    Some(delay) => match inner.work.wait_timeout(q, delay) { Ok((q, _)) => q, Err(_) => return },
                    None => match inner.work.wait(q) { Ok(q) => q, Err(_) => return },
                };
            }
        };
        match work {
            BackgroundWork::Publication(key, task, _) => {
                let result = task.run();
                finish_operation(inner, key, task, result);
            }
            BackgroundWork::Bytes(path) => {
                let result = (inner.materialize)(&path);
                if let Err(error) = &result { log::debug!("cloud: reading {} did not materialize it: {error}", path.display()); }
                if let Ok(mut q) = inner.queue.lock() {
                    q.in_flight.remove(&path);
                    q.pending.remove(&path);
                    match result {
                        Ok(()) => { q.failures.remove(&path); }
                        Err(_) => { q.failures.insert(path); }
                    }
                    if background { q.bytes_in_flight -= 1; }
                    q.done += 1;
                }
                inner.work.notify_all();
            }
        }
    }
}

#[cfg(test)]
#[path = "cloud_prefetch_tests.rs"]
mod tests;
