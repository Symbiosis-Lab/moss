//! Reading files so the cloud provider downloads them — on threads moss can
//! afford to lose.
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
//! range — a one-byte read cost the 潮汐·週報 vault days of a download that
//! could not progress.
//!
//! **Everything about scheduling belongs to the provider.** What to fetch
//! first, how many at once, when to retry, how to batch — the provider has a
//! real scheduler with real knowledge (link speed, quota, what the user pinned)
//! and moss has guesses. This module has no priority queue, no backoff and no
//! retry policy, and adding one back is a regression, not a feature. It hands
//! the provider a list and gets out of the way.
//!
//! # Retry: unbounded, uniform, and bounded by dedup
//!
//! Files *are* re-read — the supervisor re-hands its whole sweep result every
//! 60 s, forever. That is deliberate, and it is not a scheduler: it is one
//! cadence for every file, not a per-file curve. The evidence cuts both ways
//! and lands here:
//!
//! - **For retrying.** Obsidian's materialize-modal retries the same OS
//!   mechanism and "eventually succeeds" where a single attempt does not.
//! - **Against a ladder.** #986's per-file backoff paced moss *slower* than the
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
//! | returns an error | entry removed | **yes**, every 60 s |
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
//! Opening a real vault (SoCivic Theatre, 724 dataless files) froze the whole
//! app permanently. `.moss/identity/secret-key` hit a materialization the OS
//! **cancelled and never retried** (`downloadCancelled(byUser: false)` →
//! `materializationFailed`) and **that read never returned** — on the main
//! thread, inside three nested locks. A `read` has no timeout and no interrupt,
//! so the thread was gone for the life of the process. moss never reached
//! `build_folder`; the waiting screen that had already shipped never fired
//! once. See docs/archive/2026-08-03-dataless-fail-fast-and-build-driven-cloud-gate.md.
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
//! their whole lives but read. A provider that stops answering costs moss one
//! thread per wedged file and nothing else — and when they are all gone, no
//! progress is possible by any means, which the supervisor reports as a stall.
//!
//! So this is not a download mechanism. It is an isolation boundary around the
//! OS's download mechanism. See ADR-047.
//!
//! Everything here is cross-platform and unit-tested on Linux against an
//! injected [`Materializer`]; the OS-specific part is one syscall, in
//! `platform::macos::iopolicy`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How many reads moss will have outstanding at once.
///
/// Chosen as a **blast radius**: the number of threads moss can lose to a
/// provider that stops answering and still function. Losing all eight is a
/// bounded, reportable condition; an unbounded pool would be a thread leak with
/// no verdict at the end of it.
///
/// It does also bound throughput, and pretending otherwise would be wrong: only
/// eight files can be materializing at a time, and at the 13–16 s per-file floor
/// observed on the vault behind #986 that is ~21 minutes for 724 files. The
/// provider batches behind the scenes, so the real figure is better than that —
/// but the ceiling is moss's, not the provider's.
///
/// Raising it is therefore a real trade, not a free win, and it needs evidence
/// from [`Snapshot::oldest_read`] to make: if the oldest read is aging in
/// seconds the pool is the limit and more readers would help; if it is aging in
/// minutes those threads are wedged and more readers would only widen the loss.
pub const READERS: usize = 8;

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
/// whole of moss#1077 — `deployed-article-map.json` is 4,827,583 bytes, needed
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
    /// from eight reads the provider abandoned and will never answer — and
    /// those two want opposite responses from a support conversation. An age in
    /// minutes means wedged; the path names the file, which in the vault this
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

struct Queue {
    /// Plain FIFO. Order is the order moss was told about the files, which for
    /// the folder-open sweep is directory-walk order. Deliberately not sorted:
    /// see the module docs — moss does not schedule.
    fifo: VecDeque<PathBuf>,
    /// Paths queued or being read. Not a scheduling structure — it just stops
    /// moss reading the same file twice when the sweep and a failed build read
    /// name it in the same breath. A path leaves the set when its read returns,
    /// so a later sweep can ask again.
    pending: HashSet<PathBuf>,
    /// Reads currently outstanding, and when each started. Keyed rather than
    /// counted so a stall can name the file and its age — see
    /// [`Snapshot::oldest_read`].
    in_flight: HashMap<PathBuf, Instant>,
    shutdown: bool,
}

struct Inner {
    queue: Mutex<Queue>,
    /// Signals a reader that a file is available, or that it should exit.
    work: Condvar,
    materialize: Materializer,
    done: AtomicU64,
}

/// A fixed set of expendable threads that read files so the provider fetches
/// them.
pub struct Prefetcher {
    inner: Arc<Inner>,
}

impl Prefetcher {
    /// Start the real thing: threads that opt themselves in to materialization
    /// and then read whatever they are handed.
    pub fn new() -> Self {
        Self::with_materializer(READERS, Arc::new(materialize), true)
    }

    /// Test seam. `opt_in` is false so a test's threads do not touch the
    /// process's I/O policy.
    pub fn with_materializer(readers: usize, materialize: Materializer, opt_in: bool) -> Self {
        let inner = Arc::new(Inner {
            queue: Mutex::new(Queue {
                fifo: VecDeque::new(),
                pending: HashSet::new(),
                in_flight: HashMap::new(),
                shutdown: false,
            }),
            work: Condvar::new(),
            materialize,
            done: AtomicU64::new(0),
        });
        for i in 0..readers {
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
                    reader_loop(&inner);
                });
        }
        Self { inner }
    }

    /// Hand a file over to be read. Non-blocking, idempotent, and cheap enough
    /// to call from anywhere — a path already queued or in flight is ignored.
    pub fn read(&self, path: &Path) {
        let mut q = match self.inner.queue.lock() {
            Ok(q) => q,
            // A poisoned queue means a reader panicked mid-file. Dropping this
            // is better than propagating a panic into a build thread; the
            // supervisor's next sweep names the file again.
            Err(_) => return,
        };
        if q.shutdown || !q.pending.insert(path.to_path_buf()) {
            return;
        }
        q.fifo.push_back(path.to_path_buf());
        drop(q);
        self.inner.work.notify_one();
    }

    /// Queue depth and in-flight count, for the stall diagnosis.
    pub fn snapshot(&self) -> Snapshot {
        let done = self.inner.done.load(Ordering::Relaxed);
        let now = Instant::now();
        match self.inner.queue.lock() {
            Ok(q) => Snapshot {
                waiting: q.fifo.len(),
                in_flight: q.in_flight.len(),
                done,
                oldest_read: q
                    .in_flight
                    .iter()
                    .min_by_key(|(_, started)| **started)
                    .map(|(path, started)| (path.clone(), now.saturating_duration_since(*started))),
            },
            Err(_) => Snapshot { waiting: 0, in_flight: 0, done, oldest_read: None },
        }
    }

    /// Stop accepting work and release every idle reader. Threads already
    /// blocked in a read stay blocked — see the note in `with_materializer`.
    pub fn shutdown(&self) {
        if let Ok(mut q) = self.inner.queue.lock() {
            q.shutdown = true;
            q.fifo.clear();
            q.pending.clear();
        }
        self.inner.work.notify_all();
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
        let path = {
            let mut q = match inner.queue.lock() {
                Ok(q) => q,
                Err(_) => return,
            };
            loop {
                if q.shutdown {
                    return;
                }
                if let Some(path) = q.fifo.pop_front() {
                    q.in_flight.insert(path.clone(), Instant::now());
                    break path;
                }
                q = match inner.work.wait(q) {
                    Ok(q) => q,
                    Err(_) => return,
                };
            }
        };

        // Outside the lock: this is the part that blocks, for as long as the
        // provider takes — or forever. Holding the queue here would put every
        // reader behind the slowest file and, worse, one wedged file would take
        // the whole pool down with it.
        let result = (inner.materialize)(&path);

        // A failure is logged and dropped. There is no retry ledger on purpose:
        // the provider owns retry policy, and moss's own record of what still
        // needs fetching is the supervisor's sweep, which re-stats the vault
        // anyway. Anything more would be moss reimplementing a scheduler.
        if let Err(e) = result {
            log::debug!("cloud: reading {} did not materialize it: {e}", path.display());
        }

        inner.done.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut q) = inner.queue.lock() {
            q.in_flight.remove(&path);
            q.pending.remove(&path);
        }
    }
}

#[cfg(test)]
#[path = "cloud_prefetch_tests.rs"]
mod tests;
