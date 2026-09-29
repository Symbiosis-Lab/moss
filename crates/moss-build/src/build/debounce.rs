//! A generic per-key debouncer: collapses a burst of requests into one call
//! to a handler, after `idle` of quiet (capped by `max_defer`) — or
//! immediately, when [`Lanes::settle_now`] forces it.
//!
//! Extracted from the search lane's own machinery
//! (`build/feeds/search_lane.rs`'s `LANES`/`run_lane`/`quiesce`, before this
//! crate had a second caller for the same shape): the level-triggered
//! `watch`-channel-plus-parked-loop pattern is generic, and only the
//! handler, the idle/max-defer durations, and the request payload are
//! caller-specific. The search lane still owns its OWN request/response
//! types and its OWN indexing logic; only the debounce timing moved here.
//!
//! # Level-triggered, not queued
//!
//! A request that arrives while a lane is already waiting REPLACES the
//! pending value; it does not queue behind it. A burst of N requests inside
//! one idle window collapses to exactly one handler call, over the LATEST
//! request — never N calls, and never a stale one.
//!
//! # Settling
//!
//! [`Lanes::settle_now`] runs the pending request (if any) right now, on the
//! caller's own task, instead of waiting out the idle timer. It races safely
//! against the background lane, which is doing the same "wait for the
//! `running` lock, then take-and-clear the pending slot" dance — whichever of
//! the two gets there first does the work; the other finds nothing left to
//! do and returns immediately. No request is ever run twice, and none is
//! ever dropped: if a NEWER request lands while one is being handled, it is
//! still there afterward for the next pass.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;
// `tokio::time::Instant`, not `std::time::Instant`: `quiesce`'s `max_defer`
// check has to see the same (possibly paused, under `#[tokio::test(start_paused
// = true)]`) clock its `sleep(idle)` races against — the same reason
// `search_lane.rs` made this switch.
use tokio::time::Instant;

pub type HandlerFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A plain function pointer, not a closure: every lane sharing one [`Lanes`]
/// registry calls the same handler, so there is nothing per-lane to capture.
pub type Handler<R> = fn(R) -> HandlerFuture;

/// One value in a lane's channel: either nothing pending, or a request handed
/// off by [`Lanes::request`] and not yet handled. Deliberately not `Clone`:
/// the seal's own request carries a non-cloneable RAII lease
/// (`SealGuards::cache_lease` in `build.rs`) that must be dropped exactly
/// once, so the debouncer takes it by move (see [`take_pending`]) rather than
/// requiring every payload to be duplicable.
enum Slot<R> {
    Idle,
    Pending(R),
    /// The key's lane is being torn down — sent only from [`Lanes::evict`],
    /// and only onto an `Idle` slot (see that method's own doc on why it
    /// refuses to overwrite a `Pending` one). [`run_lane`] treats seeing this
    /// as its own exit signal.
    Closed,
}

struct Lane<R> {
    tx: watch::Sender<Slot<R>>,
    /// Held by whichever task actually runs the handler — the background
    /// worker or a [`Lanes::settle_now`] caller — so the two can never run
    /// the handler concurrently for the same key.
    running: tokio::sync::Mutex<()>,
}

/// A registry of debounce lanes, one per key, sharing one `idle`/`max_defer`
/// policy and one handler. Construct one `static` per caller (see
/// `build/seal_phase.rs` for the seal's own instance); the search lane's
/// prior shape of one `LazyLock<Mutex<HashMap<...>>>` per concern is what
/// this factors out.
pub struct Lanes<R: Send + Sync + 'static> {
    idle: Duration,
    max_defer: Duration,
    handler: Handler<R>,
    lanes: Mutex<HashMap<PathBuf, Arc<Lane<R>>>>,
}

impl<R: Send + Sync + 'static> Lanes<R> {
    pub fn new(idle: Duration, max_defer: Duration, handler: Handler<R>) -> Self {
        Self { idle, max_defer, handler, lanes: Mutex::new(HashMap::new()) }
    }

    /// Hand off `req` for `key`, replacing whatever was pending. Cheap and
    /// non-blocking: it publishes a value and, the first time `key` is seen,
    /// spawns the lane's worker.
    ///
    /// No-op outside a tokio runtime, matching `search_lane::request`'s own
    /// rule — a caller that constructs a manifest by hand in a unit test
    /// should not silently gain a background dependency.
    pub fn request(&self, key: &Path, req: R) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let (lane, is_new) = {
            let mut lanes = self.lanes.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let is_new = !lanes.contains_key(key);
            let lane = lanes
                .entry(key.to_path_buf())
                .or_insert_with(|| {
                    Arc::new(Lane {
                        tx: watch::channel(Slot::Idle).0,
                        running: tokio::sync::Mutex::new(()),
                    })
                })
                .clone();
            // Published under the map lock, so it cannot land on a lane
            // [`Self::evict`] has closed and is about to drop from the map —
            // that request would be lost with the lane.
            lane.tx.send_replace(Slot::Pending(req));
            (lane, is_new)
        };
        if is_new {
            let rx = lane.tx.subscribe();
            tokio::spawn(run_lane(rx, lane, self.idle, self.max_defer, self.handler));
        }
    }

    /// Run the pending request for `key` right now, skipping the idle wait.
    /// A no-op when nothing is pending — including when the background lane
    /// (or a racing `settle_now`) already handled it. Awaits full completion:
    /// a caller that needs the generation on disk before it proceeds (deploy,
    /// folder close/switch, app quit) can rely on this returning only once
    /// the handler has run to its end.
    pub async fn settle_now(&self, key: &Path) {
        let lane = {
            let lanes = self.lanes.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            lanes.get(key).cloned()
        };
        let Some(lane) = lane else { return };
        run_pending(&lane, self.handler).await;
    }

    /// [`Self::settle_now`] every currently-tracked lane, one after another.
    /// For a caller with no single relevant key — a folder switch that wants
    /// to complete whichever session was previously active without having to
    /// enumerate it first. Lanes created while this runs are not included
    /// (nothing new can be pending for a key nobody has requested against
    /// yet), and a key settled here that gets a fresh request immediately
    /// after just starts a new debounce, same as any other `request`.
    pub async fn settle_all(&self) {
        let keys: Vec<PathBuf> = {
            let lanes = self.lanes.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            lanes.keys().cloned().collect()
        };
        for key in keys {
            self.settle_now(&key).await;
        }
    }

    #[cfg(test)]
    pub fn is_pending(&self, key: &Path) -> bool {
        let lanes = self.lanes.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        lanes.get(key).is_some_and(|lane| matches!(&*lane.tx.borrow(), Slot::Pending(_)))
    }

    #[cfg(test)]
    pub fn has_lane(&self, key: &Path) -> bool {
        let lanes = self.lanes.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        lanes.contains_key(key)
    }

    /// Tear down `key`'s lane — its background [`run_lane`] task exits and
    /// the map entry is dropped — if nothing is pending and no handler is
    /// running for it. Call it once a folder session ends, after that key
    /// has been settled; a lane that is still busy is left alone, never has
    /// its work discarded, and simply stays until it is idle for a later
    /// eviction. Dropping only the map's `Arc` would not be enough:
    /// `run_lane` holds its own and would stay parked on `rx.changed()`
    /// forever, so `Slot::Closed` is what wakes it and makes it exit.
    ///
    /// Everything happens under the map lock, which [`Self::request`] also
    /// publishes under: a request either lands first (and eviction sees it
    /// pending) or finds the key gone and starts a fresh lane.
    pub fn evict(&self, key: &Path) {
        let mut lanes = self.lanes.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(lane) = lanes.get(key) else { return };
        let Ok(running) = lane.running.try_lock() else { return };
        let closed = lane.tx.send_if_modified(|slot| {
            if matches!(slot, Slot::Idle) {
                *slot = Slot::Closed;
                true
            } else {
                false
            }
        });
        drop(running);
        if closed {
            lanes.remove(key);
        }
    }
}

/// Take the pending value (if any) under `lane.running`, and run the handler
/// over it. Shared by the background lane's own pass and by `settle_now`, so
/// the "did someone already do this" check lives in exactly one place.
async fn run_pending<R: Send + Sync + 'static>(lane: &Lane<R>, handler: Handler<R>) {
    let _guard = lane.running.lock().await;
    if let Some(req) = take_pending(&lane.tx) {
        handler(req).await;
    }
}

/// Atomically read the pending request (if any), MOVING it out, and reset the
/// slot to `Idle`. `send_if_modified` makes the take-and-clear one step, so a
/// concurrent `settle_now` and background pass can never both see (and both
/// run) the same request. Moves rather than clones — see [`Slot`]'s doc for
/// why `R` is not required to be `Clone`.
fn take_pending<R>(tx: &watch::Sender<Slot<R>>) -> Option<R> {
    let mut taken = None;
    tx.send_if_modified(|slot| {
        if matches!(slot, Slot::Pending(_)) {
            if let Slot::Pending(req) = std::mem::replace(slot, Slot::Idle) {
                taken = Some(req);
            }
            true
        } else {
            false // Idle: nothing to do. Closed: never overwrite it back to Idle.
        }
    });
    taken
}

/// The lane loop: park while idle, quiesce a pending request, run it, repeat.
///
/// **It parks between requests.** Every pass is driven by a value it has not
/// seen before; nothing runs a timer while idle — the same rule
/// `search_lane::run_lane`'s own doc names, and the reason this shape was
/// worth extracting rather than reimplementing per caller.
async fn run_lane<R: Send + Sync + 'static>(
    mut rx: watch::Receiver<Slot<R>>,
    lane: Arc<Lane<R>>,
    idle: Duration,
    max_defer: Duration,
    handler: Handler<R>,
) {
    loop {
        if matches!(&*rx.borrow(), Slot::Idle) {
            if rx.changed().await.is_err() {
                return; // every sender dropped: nothing left to debounce
            }
        }
        // `Lanes::evict` only ever writes `Closed` onto an `Idle` slot, so
        // seeing it here — rather than `Pending` — means exactly one thing:
        // the key's session ended and this lane is done. Exit without
        // quiescing; there is nothing pending to wait out.
        if matches!(&*rx.borrow(), Slot::Closed) {
            return;
        }
        if !quiesce(&mut rx, idle, max_defer).await {
            return;
        }
        // Between `quiesce` returning and taking this lock, a racing
        // `settle_now` may already have run the handler — `run_pending`'s
        // own `take_pending` call is what makes that safe (see its doc).
        run_pending(&lane, handler).await;
    }
}

/// Wait for `idle` of quiet on `rx` — but never longer than `max_defer`, or a
/// folder under sustained editing would never settle at all. Returns `false`
/// once every sender has dropped (the lane's owner is gone).
async fn quiesce<R>(
    rx: &mut watch::Receiver<Slot<R>>,
    idle: Duration,
    max_defer: Duration,
) -> bool {
    let first = Instant::now();
    loop {
        tokio::select! {
            _ = tokio::time::sleep(idle) => return true,
            changed = rx.changed() => {
                if changed.is_err() {
                    return false;
                }
                if first.elapsed() >= max_defer {
                    return true;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "debounce_tests.rs"]
mod tests;
