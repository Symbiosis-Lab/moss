//! Deriving many files at once, within a small budget of concurrent children.

use std::path::PathBuf;
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};

use super::cache::{derive_cached, derive_remembering, DeriveResult};
use super::priority::Tier;
use crate::moss_paths::MossPaths;
use crate::system::derive::registry::DerivationEntry;

/// One file to derive, with the derivation that handles it.
pub struct DeriveJob<'a> {
    pub entry: &'a DerivationEntry,
    pub input: PathBuf,
}

/// How many derivers run at once: half the cores, between one and four (a
/// deriver is usually itself multi-threaded and memory-hungry, and the build
/// is doing other work), or `MOSS_DERIVE_JOBS` when set to a whole number from
/// 1 to 64.
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
/// [`derive_budget`] reads run at once across all of them. A batch that arrives
/// while another is reading joins the same ordering instead of waiting behind
/// it.
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
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue { batches: Vec::new(), next_id: 0, running: 0 });
static CHANGED: Condvar = Condvar::new();
static LIMIT: LazyLock<usize> = LazyLock::new(derive_budget);

impl Queue {
    /// The batch and position of the read to start next: best tier, then the
    /// earlier batch, then job order.
    fn best(&self) -> Option<(u64, usize)> {
        self.batches
            .iter()
            .filter_map(|b| {
                let (at, tier) = b.jobs.iter().enumerate().map(|(at, &job)| (at, (b.rank)(job))).min_by_key(|&(_, tier)| tier)?;
                Some((tier, b.id, at))
            })
            .min_by_key(|&(tier, id, _)| (tier, id))
            .map(|(_, id, at)| (id, at))
    }

    fn leave(&mut self, id: u64) {
        self.batches.retain(|b| b.id != id);
    }
}

fn lock() -> MutexGuard<'static, Queue> {
    QUEUE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Derive the jobs, in job order, within the process-wide budget. Results come
/// back in job order; a job not started because `stop()` turned true is `None`.
/// `progress(done, total)` is called after each finish, never concurrently
/// with itself, with `done` rising by one each time. `remember` keeps a result
/// the cache cannot hold for the rebuild that follows (see `cache::remembered`).
pub fn derive_batch(
    site: &MossPaths,
    jobs: &[DeriveJob<'_>],
    remember: bool,
    stop: &(dyn Fn() -> bool + Sync),
    progress: impl Fn(usize, usize) + Sync,
) -> Vec<Option<DeriveResult>> {
    derive_batch_by(site, jobs, remember, stop, Arc::new(|_| Tier::Rest), progress)
}

/// [`derive_batch`] with the start order chosen as it runs: whenever a read can
/// start, the best-ranked job not yet started, across this batch and every
/// other one in the process, goes next (ties: the earlier batch, then job
/// order). A job that has started is never revisited, so a changed rank
/// reorders only what waits.
pub fn derive_batch_by(
    site: &MossPaths,
    jobs: &[DeriveJob<'_>],
    remember: bool,
    stop: &(dyn Fn() -> bool + Sync),
    rank: Arc<dyn Fn(usize) -> Tier + Send + Sync>,
    progress: impl Fn(usize, usize) + Sync,
) -> Vec<Option<DeriveResult>> {
    let total = jobs.len();
    let id = {
        let mut queue = lock();
        let id = queue.next_id;
        queue.next_id += 1;
        queue.batches.push(Waiting { id, jobs: (0..total).collect(), rank });
        id
    };
    CHANGED.notify_all();
    let finished = Mutex::new((0usize, (0..total).map(|_| None).collect::<Vec<Option<DeriveResult>>>()));
    std::thread::scope(|scope| {
        for _ in 0..(*LIMIT).min(total) {
            scope.spawn(|| loop {
                let next = {
                    let mut queue = lock();
                    loop {
                        if stop() {
                            queue.leave(id);
                            break None;
                        }
                        if !queue.batches.iter().any(|b| b.id == id && !b.jobs.is_empty()) {
                            break None;
                        }
                        if queue.running < *LIMIT {
                            if let Some((best, at)) = queue.best().filter(|&(best, _)| best == id) {
                                let batch = queue.batches.iter_mut().find(|b| b.id == best).expect("best names a batch");
                                let job = batch.jobs.remove(at);
                                queue.running += 1;
                                break Some(job);
                            }
                        }
                        queue = CHANGED.wait_timeout(queue, std::time::Duration::from_millis(250)).unwrap_or_else(|e| e.into_inner()).0;
                    }
                };
                let Some(next) = next else {
                    CHANGED.notify_all();
                    return;
                };
                let job = &jobs[next];
                let result = if remember {
                    derive_remembering(site, job.entry, &job.input)
                } else {
                    derive_cached(site, job.entry, &job.input)
                };
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

#[cfg(test)]
#[path = "batch_tests.rs"]
mod batch_tests;
