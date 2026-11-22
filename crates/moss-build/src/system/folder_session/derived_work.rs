//! Derived work waits while nobody is looking.
//!
//! Some of what a watch-mode build sets off feeds only what is on screen or
//! what a later publish reads: materializing the sealed generation, the
//! generation GC behind it, and the search re-index the seal's tail requests.
//! While the host reports that nobody is looking (`Cadence::Background`),
//! running that after every edit spends CPU on output nobody reads. So the
//! seal's debounce lane asks the folder session's [`DerivedWorkGate`] first.
//! While the gate is closed it keeps its latest request pending (the lane is
//! already latest-wins), and when the cadence turns `Live` the gate opens and
//! the seal runs once over everything that happened in between, then requests
//! one index of the generation it promoted.
//!
//! Forced paths never wait. A publish, a quit or a folder switch settles the
//! lane on its own task, which does not pass through the gate. A process
//! without a visibility signal attaches no cadence (a one-shot build) or a
//! constant `Live` one (`moss build --serve --watch`), so its gate is always
//! open.
//!
//! The gate also owns the core budget of picture and video encodes and of
//! plugin derivers. Once a cadence is attached the folder is open beside
//! whatever else the person is doing, so they share a quarter of the cores;
//! a publish, which waits on them, gets all of them. See [`DerivedWorkGate::cores`].

use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::watch;

use super::FolderSession;
use crate::deploy::freeze::publish_in_flight;
use crate::ops::watch::cadence::Cadence;

/// Where a folder session keeps the cadence its watch attached. Every gate
/// taken from the session shares it, so a gate taken before the watch starts
/// (the first build's image batch, which outlives that build) follows the
/// cadence attached after it.
pub(super) type CadenceSlot = Arc<Mutex<Option<watch::Receiver<Cadence>>>>;

/// Whether derived work may run now, and how many cores its encodes may use.
/// Cheap to clone; a clone follows the same cadence. The default gate is
/// always open and has every core.
#[derive(Clone, Debug)]
pub struct DerivedWorkGate {
    cadence: Option<CadenceSlot>,
    pool: &'static CorePool,
}

impl Default for DerivedWorkGate {
    fn default() -> Self {
        Self { cadence: None, pool: &POOL }
    }
}

impl DerivedWorkGate {
    #[cfg(test)]
    pub(crate) fn following(cadence: watch::Receiver<Cadence>) -> Self {
        Self { cadence: Some(Arc::new(Mutex::new(Some(cadence)))), ..Self::default() }
    }

    /// This gate, drawing its cores from `pool` instead of the process's.
    #[cfg(test)]
    pub(crate) fn drawing_from(self, pool: &'static CorePool) -> Self {
        Self { pool, ..self }
    }

    /// How many takers wait ahead of the rest.
    #[cfg(test)]
    pub(crate) fn urgent_waiting(&self) -> usize {
        self.pool.lock().urgent
    }

    /// The gate of `session`, or an open one when there is no session.
    pub fn of(session: Option<&Arc<FolderSession>>) -> Self {
        session.map(|s| s.derived_work_gate()).unwrap_or_default()
    }

    fn cadence(&self) -> Option<watch::Receiver<Cadence>> {
        self.cadence.as_ref()?.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// True while derived work should wait. A cadence whose sender is gone
    /// can never flip again, so it counts as open rather than strand the work.
    pub fn is_closed(&self) -> bool {
        self.cadence().is_some_and(|rx| rx.has_changed().is_ok() && *rx.borrow() == Cadence::Background)
    }

    /// Resolve once derived work may run: immediately when the gate is open,
    /// otherwise on the flip to `Live` or when the cadence's sender goes.
    pub async fn opened(&self) {
        let Some(mut rx) = self.cadence() else { return };
        let _ = rx.wait_for(|c| *c == Cadence::Live).await;
    }

    /// Run the calling thread at background QoS while nobody is looking, and
    /// at its own class otherwise. For long CPU work already under way (an
    /// image batch), which a hidden window slows rather than stops: call it
    /// before each item. While someone may be looking the core budget alone
    /// keeps encodes polite: a lower class moves them to the efficiency cores,
    /// where an encode costs about half again the CPU time and the page waits
    /// several times longer for its pictures.
    ///
    /// A publish in flight is the exception: it waits on this work (and a
    /// deploy's own rebuild runs under it), and a user action is never slowed.
    ///
    /// macOS only, where a thread may lower and raise its own QoS class
    /// freely. Elsewhere this does nothing: an unprivileged thread that lowers
    /// its nice value cannot raise it again.
    pub fn pace_this_thread(&self) {
        #[cfg(target_os = "macos")]
        crate::infra::qos::set_class(self.encode_class());
    }

    #[cfg(target_os = "macos")]
    fn encode_class(&self) -> Option<libc::qos_class_t> {
        (self.is_closed() && !publish_in_flight()).then_some(libc::qos_class_t::QOS_CLASS_BACKGROUND)
    }

    /// How many cores derived encodes may use at once, across the process: a
    /// quarter of them (at least one) once a host attached a cadence, since
    /// the folder is open beside whatever the person is doing; all of them
    /// while a publish waits on the work, or when no host attached one (a
    /// one-shot build nobody watches).
    pub fn cores(&self) -> usize {
        budget(*ALL_CORES, self.cadence().is_some(), publish_in_flight())
    }

    /// Take `encoder`'s share of the core budget, waiting until it is free;
    /// `None` once `stop` turns true while waiting.
    pub fn take_cores(&self, encoder: Encoder, stop: &dyn Fn() -> bool) -> Option<Cores<'static>> {
        self.pool.take(|| self.cores(), encoder, false, stop)
    }

    /// [`take_cores`](Self::take_cores) ahead of every other waiter, for work
    /// someone is waiting on: a read of the page on screen, or one a build
    /// cannot finish without.
    pub fn take_cores_first(&self, encoder: Encoder, stop: &dyn Fn() -> bool) -> Option<Cores<'static>> {
        self.pool.take(|| self.cores(), encoder, true, stop)
    }
}

fn budget(all: usize, attached: bool, publishing: bool) -> usize {
    if attached && !publishing {
        (all / 4).max(1)
    } else {
        all
    }
}

static ALL_CORES: LazyLock<usize> = LazyLock::new(|| std::thread::available_parallelism().map_or(1, |n| n.get()));

/// The one pool every picture and video encode and every plugin deriver in
/// the process draws from, and outside tests the only one there is. Each gate
/// sets its own budget against it, so a session with no cadence (every core)
/// can fill it while a session with one waits. A test may give a build its own ([`CorePool::own`]).
static POOL: CorePool = CorePool::new();

/// How often a waiter re-reads the budget: a publish that starts while
/// workers wait widens it without anyone signalling the pool.
const RECHECK: Duration = Duration::from_millis(100);

/// What asks for cores. A picture takes one per item. A video takes half the
/// budget (at least one) and hands it to ffmpeg as its thread count, so an
/// encode that runs for minutes never holds every core while the pictures a
/// page shows wait. Pictures come first: a picture retakes a freed core
/// before a waiting video gathers its half, so a video starts once the
/// picture batch thins out. A plugin's deriver, an outside program reading
/// one file, takes one like a picture.
#[derive(Clone, Copy, Debug)]
pub enum Encoder {
    Picture,
    Video,
    Deriver,
}

impl Encoder {
    fn share(self, budget: usize) -> usize {
        match self {
            Encoder::Picture | Encoder::Deriver => 1,
            Encoder::Video => (budget / 2).max(1),
        }
    }
}

/// A counting semaphore whose size is read at every take.
#[derive(Debug)]
pub(crate) struct CorePool {
    load: Mutex<Load>,
    freed: Condvar,
}

#[derive(Debug)]
struct Load {
    in_use: usize,
    /// Takers waiting for work someone is waiting on; while any wait, nobody
    /// else is granted a core.
    urgent: usize,
}

/// Cores held from a [`CorePool`], given back on drop. A running ffmpeg keeps
/// the count it started with even if the budget changes under it.
#[must_use]
pub struct Cores<'a> {
    pool: &'a CorePool,
    held: usize,
}

impl Cores<'_> {
    pub fn count(&self) -> usize {
        self.held
    }
}

impl Drop for Cores<'_> {
    fn drop(&mut self) {
        self.pool.lock().in_use -= self.held;
        self.pool.freed.notify_all();
    }
}

impl CorePool {
    const fn new() -> Self {
        Self { load: Mutex::new(Load { in_use: 0, urgent: 0 }), freed: Condvar::new() }
    }

    /// A pool no other build draws from, for a test whose encodes must not
    /// wait on a sibling test's.
    #[cfg(test)]
    pub(crate) fn own() -> &'static Self {
        Box::leak(Box::new(Self::new()))
    }

    fn lock(&self) -> MutexGuard<'_, Load> {
        self.load.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wait for `encoder`'s share. An `urgent` taker is served before every
    /// other waiter.
    pub(crate) fn take(&self, budget: impl Fn() -> usize, encoder: Encoder, urgent: bool, stop: &dyn Fn() -> bool) -> Option<Cores<'_>> {
        let mut load = self.lock();
        load.urgent += usize::from(urgent);
        let cores = loop {
            if stop() {
                break None;
            }
            if let Some(cores) = self.grant(&mut load, budget(), encoder, urgent) {
                break Some(cores);
            }
            load = self.freed.wait_timeout(load, RECHECK).unwrap_or_else(PoisonError::into_inner).0;
        };
        load.urgent -= usize::from(urgent);
        cores
    }

    fn grant(&self, load: &mut Load, budget: usize, encoder: Encoder, urgent: bool) -> Option<Cores<'_>> {
        let want = encoder.share(budget);
        ((urgent || load.urgent == 0) && load.in_use + want <= budget).then(|| {
            load.in_use += want;
            Cores { pool: self, held: want }
        })
    }
}

impl FolderSession {
    /// Gate this folder's derived work on `cadence`. Called once per watch
    /// start, so a re-opened folder follows the cadence its new watch was
    /// given.
    pub fn follow_cadence(&self, cadence: watch::Receiver<Cadence>) {
        *self.cadence.lock().unwrap_or_else(PoisonError::into_inner) = Some(cadence);
    }

    /// The gate derived work for this folder waits on. Open until
    /// [`follow_cadence`](Self::follow_cadence) attaches a cadence.
    pub fn derived_work_gate(&self) -> DerivedWorkGate {
        DerivedWorkGate { cadence: Some(self.cadence.clone()), ..DerivedWorkGate::default() }
    }
}

#[cfg(test)]
#[path = "derived_work_tests.rs"]
mod tests;
