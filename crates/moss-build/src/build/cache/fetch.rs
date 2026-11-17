//! Fetching a transform another machine already made, instead of making it again.
//!
//! The record and its blobs travel with the folder's sync. On a machine where
//! they are still cloud placeholders, the work they record is downloaded, not
//! redone: an encode is not byte-identical across runs, so redoing it would
//! also change what is published.
use super::records::RecordRead;
use super::{ObjectStore, RecordMode, TransformCache};
use crate::build::cloud_readiness::{await_ready, request_download, Settled, POLL_INTERVAL};
use std::path::PathBuf;
use std::time::Duration;

/// A wait with no deadline of its own: `give_up` decides when it ends.
const NO_DEADLINE: Duration = Duration::from_secs(24 * 60 * 60);

impl TransformCache {
    /// The files `source_oid`'s `kind` entries need that are still in the
    /// cloud, each asked for: the record itself while it is cloud-only, then
    /// the blobs it names. Empty when everything is local or nothing is cached.
    pub(crate) fn request_cloud_held(&self, source_oid: &str, kind: &str) -> Vec<PathBuf> {
        let record_path = self.record_path(source_oid);
        let record_held = crate::build::icloud::is_still_in_the_cloud(&record_path);
        let held: Vec<PathBuf> = match if record_held { RecordRead::Offline } else { self.read(source_oid, RecordMode::Request) } {
            RecordRead::Offline => vec![record_path],
            RecordRead::Present(record) => record.transforms.iter()
                .filter(|(name, entry)| name.starts_with(kind) && self.objects.ready_blob(&entry.oid).is_none())
                .map(|(_, entry)| self.objects.blob_path(&entry.oid))
                .filter(|blob| ObjectStore::cloud_holds(blob))
                .collect(),
            _ => Vec::new(),
        };
        held.iter().for_each(|path| request_download(path));
        held
    }

    /// Wait until every cloud-held file of `source_oid`'s `kind` entries is
    /// local, or `give_up` says to stop. True when they all arrived.
    pub(crate) fn fetch_cloud_held(&self, source_oid: &str, kind: &str, give_up: &dyn Fn() -> bool) -> bool {
        // The record first, then the blobs it names: two rounds, and a third
        // for a record that arrived naming a blob another machine just wrote.
        for _ in 0..3 {
            let held = self.request_cloud_held(source_oid, kind);
            if held.is_empty() {
                return true;
            }
            for path in &held {
                if await_ready(path, NO_DEADLINE, POLL_INTERVAL, give_up, &|| {}) != Settled::Ready {
                    return false;
                }
            }
        }
        self.request_cloud_held(source_oid, kind).is_empty()
    }
}

/// One run's fetches: what is cloud-held, asked for up front, and one deadline
/// for the whole run, the estimated time of encoding all of it instead. Waiting
/// past that would cost more than the encodes it saves.
pub(crate) struct FetchPlan {
    held: std::collections::HashSet<String>,
    deadline: std::time::Instant,
    fetched: std::sync::atomic::AtomicUsize,
    missed: std::sync::atomic::AtomicUsize,
}

impl FetchPlan {
    /// `items` are each source with its estimated encode seconds; `threads`
    /// is how many encode at once.
    pub(crate) fn new<'a>(transforms: &TransformCache, kind: &str, items: impl Iterator<Item = (&'a str, f64)>, threads: usize) -> Self {
        let mut held = std::collections::HashSet::new();
        let mut secs = 0.0;
        for (source_oid, estimate) in items {
            if !source_oid.is_empty() && !held.contains(source_oid) && !transforms.request_cloud_held(source_oid, kind).is_empty() {
                held.insert(source_oid.to_string());
                secs += estimate;
            }
        }
        let wait = Duration::from_secs_f64(secs / threads.max(1) as f64);
        Self { held, deadline: std::time::Instant::now() + wait, fetched: Default::default(), missed: Default::default() }
    }

    fn is_held(&self, source_oid: &str) -> bool {
        self.held.contains(source_oid)
    }

    /// The order to work through `sources` in: those whose cache is local
    /// first, so no wait delays them, then the cloud-held ones.
    pub(crate) fn local_first<'a>(&self, sources: impl Iterator<Item = &'a str>) -> Vec<usize> {
        let (mut order, held): (Vec<_>, Vec<_>) = sources.enumerate().partition(|(_, oid)| !self.is_held(oid));
        order.extend(held);
        order.into_iter().map(|(index, _)| index).collect()
    }

    /// Wait for `source_oid`'s cloud-held files until they arrive, `cancelled`
    /// says so, or the run's deadline passes while someone is looking. With
    /// nobody looking no one needs a stand-in, so the deadline does not apply.
    pub(crate) fn wait(&self, transforms: &TransformCache, source_oid: &str, kind: &str, cancelled: &dyn Fn() -> bool, nobody_looking: &dyn Fn() -> bool) {
        if !self.is_held(source_oid) {
            return;
        }
        let give_up = || cancelled() || (!nobody_looking() && std::time::Instant::now() >= self.deadline);
        let counter = if transforms.fetch_cloud_held(source_oid, kind, &give_up) { &self.fetched } else { &self.missed };
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// (cloud-held, fetched, not arrived in time).
    pub(crate) fn counts(&self) -> (usize, usize, usize) {
        use std::sync::atomic::Ordering::Relaxed;
        (self.held.len(), self.fetched.load(Relaxed), self.missed.load(Relaxed))
    }

    /// One line for a run of `total` items that found any in the cloud.
    pub(crate) fn report(&self, total: usize) {
        let (held, fetched, missed) = self.counts();
        if held > 0 {
            log::info!("cache fetch: {held} of {total} were in the cloud; {fetched} fetched, {missed} made again after waiting");
        }
    }
}
