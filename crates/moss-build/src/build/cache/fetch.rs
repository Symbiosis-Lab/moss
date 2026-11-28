//! Fetching a transform another machine already made, instead of making it again.
//!
//! The record and its blobs travel with the folder's sync. On a machine where
//! they are still cloud placeholders, the work they record is downloaded, not
//! redone: an encode is not byte-identical across runs, so redoing it would
//! also change what is published.
use super::records::RecordRead;
use super::{ObjectStore, RecordMode, TransformCache};
use crate::build::cloud_readiness::{await_ready, request_download, Settled, POLL_INTERVAL};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
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

/// One run's work list, local work first. The first pass hands items out in
/// order; an item whose cached work the cloud still holds is [`deferred`]
/// (its download already asked for) and handed out again once every
/// first-pass item has ended, so no download delays an item that is local.
///
/// [`deferred`]: Ticket::defer
pub(crate) struct LocalFirst {
    len: usize,
    next: AtomicUsize,
    state: Mutex<Passes>,
    changed: Condvar,
    fetched: AtomicUsize,
    healed: AtomicUsize,
}

struct Passes {
    /// First-pass items handed out or yet to be, not yet ended.
    first_open: usize,
    deferred: VecDeque<usize>,
}

/// One handed-out item. Dropping it ends a first-pass item, deferred or not,
/// so an item that panics cannot keep the second pass waiting.
pub(crate) struct Ticket<'a> {
    queue: &'a LocalFirst,
    pub(crate) index: usize,
    /// Handed out again after being deferred: its cache was cloud-held.
    pub(crate) deferred: bool,
}

impl LocalFirst {
    pub(crate) fn new(len: usize) -> Self {
        Self { len, next: AtomicUsize::new(0), state: Mutex::new(Passes { first_open: len, deferred: VecDeque::new() }), changed: Condvar::new(), fetched: AtomicUsize::new(0), healed: AtomicUsize::new(0) }
    }

    /// The next item, or `None` once both passes are done.
    pub(crate) fn take(&self) -> Option<Ticket<'_>> {
        let index = self.next.fetch_add(1, Ordering::Relaxed);
        if index < self.len {
            return Some(Ticket { queue: self, index, deferred: false });
        }
        let mut state = self.lock();
        loop {
            if let Some(index) = state.deferred.pop_front() {
                return Some(Ticket { queue: self, index, deferred: true });
            }
            if state.first_open == 0 {
                return None;
            }
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// One line for the run of `what`, which made `encodes` outputs.
    pub(crate) fn report(&self, what: &str, encodes: usize) {
        let (fetched, healed) = (self.fetched.load(Ordering::Relaxed), self.healed.load(Ordering::Relaxed));
        log::info!("{what}: {encodes} encoded; {fetched} fetched from the cloud, {healed} made again after the cloud failed them");
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Passes> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Ticket<'_> {
    /// Hand this first-pass item out again after every local one.
    pub(crate) fn defer(&self) {
        debug_assert!(!self.deferred);
        self.queue.lock().deferred.push_back(self.index);
    }

    /// On the first pass, defer the item when `source_oid`'s `kind` work is
    /// in the cloud, its download asked for. True when deferred.
    pub(crate) fn defer_if_cloud_held(&self, transforms: &TransformCache, source_oid: &str, kind: &str) -> bool {
        let held = !self.deferred && !transforms.request_cloud_held(source_oid, kind).is_empty();
        if held {
            self.defer();
        }
        held
    }

    /// On the second pass, wait for that work to arrive. Only cancellation
    /// ends the wait; work the provider fails to deliver is made again.
    pub(crate) fn fetch(&self, transforms: &TransformCache, source_oid: &str, kind: &str, cancelled: &dyn Fn() -> bool) {
        if self.deferred {
            let arrived = transforms.fetch_cloud_held(source_oid, kind, cancelled);
            if !cancelled() {
                (if arrived { &self.queue.fetched } else { &self.queue.healed }).fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        if self.deferred {
            return;
        }
        self.queue.lock().first_open -= 1;
        self.queue.changed.notify_all();
    }
}
