//! Deriving many files at once, within a small budget of concurrent children.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};

use super::cache::{derive_cached, derive_remembering, DeriveResult};
use super::priority::Tier;
use crate::moss_paths::MossPaths;
use crate::system::folder_session::{DerivedWorkGate, Encoder};
use crate::system::derive::registry::DerivationEntry;

/// One file to derive, with the derivation that handles it.
pub struct DeriveJob<'a> {
    pub entry: &'a DerivationEntry,
    pub input: PathBuf,
}

/// The most derivers that run at once: half the cores, between one and four
/// (a deriver is usually itself multi-threaded and memory-hungry), or
/// `MOSS_DERIVE_JOBS` when set to a whole number from 1 to 64. Each also holds
/// a core of the build's [`DerivedWorkGate`], shared with picture and video
/// encodes, so fewer run while the encodes fill it.
pub fn derive_budget() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    budget_from(std::env::var("MOSS_DERIVE_JOBS").ok().as_deref(), cores)
}

fn budget_from(value: Option<&str>, cores: usize) -> usize {
    value
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|jobs| (1..=64).contains(jobs))
        .unwrap_or_else(|| (cores / 2).clamp(1, 4))
}

/// Every batch in the process waits in one queue, and at most
/// [`derive_budget`] reads run at once across all of them, each holding a core
/// of its batch's gate. A batch that arrives while another is reading joins
/// the same ordering instead of waiting behind it.
struct Queue {
    batches: Vec<Waiting>,
    next_id: u64,
    running: usize,
}

struct Waiting {
    id: u64,
    /// Indices into the batch's jobs, in job order.
    jobs: Vec<usize>,
    rank: Arc<dyn Fn(usize) -> Tier + Send + Sync>,
    /// Workers waiting for a core ahead of the pictures, at most one per
    /// waiting read of the page on screen, so none holds such a core with no
    /// such read left to spend it on.
    claims: usize,
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue { batches: Vec::new(), next_id: 0, running: 0 });
static CHANGED: Condvar = Condvar::new();
static LIMIT: LazyLock<usize> = LazyLock::new(derive_budget);

impl Queue {
    /// The batch, position and tier of the read to start next: best tier,
    /// then the earlier batch, then job order.
    fn best(&self) -> Option<(u64, usize, Tier)> {
        self.batches
            .iter()
            .filter_map(|b| {
                let (at, tier) = b.jobs.iter().enumerate().map(|(at, &job)| (at, (b.rank)(job))).min_by_key(|&(_, tier)| tier)?;
                Some((tier, b.id, at))
            })
            .min_by_key(|&(tier, id, _)| (tier, id))
            .map(|(tier, id, at)| (id, at, tier))
    }

    fn leave(&mut self, id: u64) {
        self.batches.retain(|b| b.id != id);
    }
}

fn lock() -> MutexGuard<'static, Queue> {
    QUEUE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Derive the jobs within the process-wide budget. Results come back in job
/// order; a job not started because `stop()` turned true is `None`.
/// `progress(done, total)` is called after each finish, never concurrently
/// with itself, with `done` rising by one each time. `remember` keeps a result
/// the cache cannot hold for the rebuild that follows (see `cache::remembered`).
///
/// The start order is chosen as it runs: whenever a read can start, the
/// best-ranked job not yet started, across this batch and every other one in
/// the process, goes next (ties: the earlier batch, then job order). A job
/// that has started is never revisited, so a changed rank reorders only what
/// waits. A job starts only once `gate` has a core free.
pub fn derive_batch_by(
    site: &MossPaths,
    jobs: &[DeriveJob<'_>],
    remember: bool,
    gate: &DerivedWorkGate,
    stop: &(dyn Fn() -> bool + Sync),
    rank: Arc<dyn Fn(usize) -> Tier + Send + Sync>,
    progress: impl Fn(usize, usize) + Sync,
) -> Vec<Option<DeriveResult>> {
    let total = jobs.len();
    let id = {
        let mut queue = lock();
        let id = queue.next_id;
        queue.next_id += 1;
        queue.batches.push(Waiting { id, jobs: (0..total).collect(), rank, claims: 0 });
        id
    };
    CHANGED.notify_all();
    let finished = Mutex::new((0usize, (0..total).map(|_| None).collect::<Vec<Option<DeriveResult>>>()));
    std::thread::scope(|scope| {
        for _ in 0..(*LIMIT).min(total) {
            scope.spawn(|| loop {
                let next = {
                    let mut queue = lock();
                    let mut held = None;
                    loop {
                        if stop() {
                            queue.leave(id);
                            break None;
                        }
                        if !queue.batches.iter().any(|b| b.id == id && !b.jobs.is_empty()) {
                            break None;
                        }
                        if queue.running < *LIMIT {
                            if let Some((best, at, tier)) = queue.best().filter(|&(best, ..)| best == id) {
                                let batch = queue.batches.iter_mut().find(|b| b.id == best).expect("best names a batch");
                                if let Some(cores) = held.take() {
                                    let job = batch.jobs.remove(at);
                                    queue.running += 1;
                                    break Some((job, cores));
                                }
                                let urgent = tier == Tier::Viewed && batch.claims < batch.jobs.iter().filter(|&&job| (batch.rank)(job) == Tier::Viewed).count();
                                batch.claims += usize::from(urgent);
                                // Wait for a core with the queue unlocked, then
                                // look again: the best job may have changed
                                // meanwhile.
                                drop(queue);
                                held = if urgent {
                                    gate.take_cores_first(Encoder::Deriver, stop)
                                } else {
                                    gate.take_cores(Encoder::Deriver, stop)
                                };
                                queue = lock();
                                if let Some(batch) = queue.batches.iter_mut().find(|b| b.id == id) {
                                    batch.claims -= usize::from(urgent);
                                }
                                continue;
                            }
                        }
                        held = None;
                        queue = CHANGED.wait_timeout(queue, std::time::Duration::from_millis(250)).unwrap_or_else(|e| e.into_inner()).0;
                    }
                };
                let Some((next, cores)) = next else {
                    CHANGED.notify_all();
                    return;
                };
                let job = &jobs[next];
                let result = if remember {
                    derive_remembering(site, job.entry, &job.input)
                } else {
                    derive_cached(site, job.entry, &job.input)
                };
                drop(cores);
                lock().running -= 1;
                CHANGED.notify_all();
                let mut finished = finished.lock().unwrap_or_else(|e| e.into_inner());
                finished.0 += 1;
                finished.1[next] = Some(result);
                progress(finished.0, total);
            });
        }
    });
    lock().leave(id);
    CHANGED.notify_all();
    finished.into_inner().unwrap_or_else(|e| e.into_inner()).1
}

// Its fixtures are sh scripts.
#[cfg(all(test, unix))]
#[path = "batch_tests.rs"]
mod batch_tests;
