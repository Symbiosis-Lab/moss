//! Deriving many files at once, within a small budget of concurrent children.

use std::path::PathBuf;
use std::sync::Mutex;

use rayon::prelude::*;

use super::cache::{derive_cached, derive_remembering, DeriveResult};
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

/// The threads derivers run on: [`derive_budget`] of them, so that many
/// children at once and no more.
static DERIVE_POOL: std::sync::LazyLock<rayon::ThreadPool> = std::sync::LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("moss-derive-{i}"))
        .num_threads(derive_budget())
        .build()
        .expect("moss-derive pool")
});

/// Derive the jobs, [`derive_budget`] at a time. Results come back in job
/// order; a job not started because `stop()` turned true is `None`.
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
    let total = jobs.len();
    let done = Mutex::new(0usize);
    DERIVE_POOL.install(|| {
        jobs.par_iter()
            .map(|job| {
                if stop() {
                    return None;
                }
                let result = if remember {
                    derive_remembering(site, job.entry, &job.input)
                } else {
                    derive_cached(site, job.entry, &job.input)
                };
                let mut finished = done.lock().unwrap_or_else(|e| e.into_inner());
                *finished += 1;
                progress(*finished, total);
                Some(result)
            })
            .collect()
    })
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod batch_tests;
