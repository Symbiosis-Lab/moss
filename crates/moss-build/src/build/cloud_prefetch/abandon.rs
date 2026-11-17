//! A pool job the provider holds past its bound has its thread given up: the
//! thread stays parked in the kernel, and its lane gets one fresh thread so
//! the rest of the queue moves. The job itself is neither ended nor asked
//! again. Its waiters keep waiting, its path stays deduplicated, and when the
//! parked thread returns it settles the job as usual. A second attempt at the
//! same file would park beside the first; for a publication it could also land
//! older bytes after newer ones.
//!
//! So there is at most one attempt per job, and whether a returning thread was
//! given up is whether its job is still in the in-flight maps.
//!
//! Giving a thread up marks its lane stuck. That lane then admits one job at a
//! time, spaced from [`Bounds::probe_first`] doubling to [`Bounds::probe_most`];
//! the other lane is not held. Any return clears both, since it proves the
//! provider alive. A lane gets
//! a replacement only while its parked threads are no more than its width, so
//! a provider that never answers costs at most twice the pool.
use super::*;

/// How long a pool job may hold its thread before the thread is given up.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Bounds {
    /// A byte read, plus [`Bounds::bytes_per_mb`] for each MB the file holds,
    /// so a large film is not given up while it downloads.
    pub bytes: Duration,
    pub bytes_per_mb: Duration,
    pub publication: Duration,
    /// A structural read, measured since its last directory entry.
    pub structural_quiet: Duration,
    pub probe_first: Duration,
    pub probe_most: Duration,
    /// How long an idle thread sleeps between checks. While one lane is fully
    /// parked, the other lane's idle threads keep checking it.
    pub idle_check: Duration,
}

impl Bounds {
    pub(crate) const PROVIDER: Self = Self {
        bytes: Duration::from_secs(120),
        bytes_per_mb: Duration::from_secs(1),
        publication: Duration::from_secs(60),
        structural_quiet: Duration::from_secs(60),
        probe_first: Duration::from_secs(30),
        probe_most: Duration::from_secs(300),
        idle_check: Duration::from_secs(5),
    };

    pub(super) fn for_bytes(&self, len: u64) -> Duration {
        self.bytes + self.bytes_per_mb * u32::try_from(len / 1_000_000).unwrap_or(u32::MAX)
    }
}

/// Byte and publication readers, and structural readers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lane { Readers = 0, Structural = 1 }

/// A job a pool thread is running.
#[derive(Debug, Clone, Copy)]
pub(super) struct Running {
    pub started: Instant,
    /// Measured from `started`, except a structural read's, which is measured
    /// from its last directory entry.
    pub bound: Duration,
    /// A byte read that holds one of the background slots.
    pub background: bool,
}

impl Running {
    pub fn new(started: Instant, bound: Duration) -> Self { Self { started, bound, background: false } }
}

/// Set while a lane is stuck: when its next probe may start, once one has.
#[derive(Debug, Clone, Copy)]
struct Probe { next: Option<Instant>, backoff: Duration }

#[derive(Debug, Default)]
pub(super) struct Abandonment {
    /// Per [`Lane`]: the given-up jobs not yet returned, each path with when
    /// it started.
    parked: [Vec<(PathBuf, Instant)>; 2],
    width: [usize; 2],
    spawned: [usize; 2],
    probe: [Option<Probe>; 2],
    last_reap: Option<Instant>,
}

impl Abandonment {
    pub fn new(readers: usize, structural: usize) -> Self {
        Self { width: [readers, structural], spawned: [readers, structural], ..Self::default() }
    }

    pub fn parked(&self) -> impl Iterator<Item = &(PathBuf, Instant)> { self.parked.iter().flatten() }

    /// A thread was given up and nothing has returned since.
    pub fn stuck(&self) -> bool { self.probe.iter().any(Option::is_some) }

    #[cfg(test)]
    pub fn spawned(&self) -> usize { self.spawned.iter().sum() }

    /// When `lane` may take its next job: `None` now, else the probe's time.
    pub fn gate(&self, lane: Lane, now: Instant) -> Option<Instant> {
        self.probe[lane as usize].and_then(|probe| probe.next).filter(|at| *at > now)
    }

    /// `lane` took a job. While it is stuck, that job is the probe and the next waits.
    pub fn took(&mut self, lane: Lane, now: Instant, bounds: &Bounds) {
        let Some(probe) = &mut self.probe[lane as usize] else { return };
        probe.backoff = if probe.next.is_none() { bounds.probe_first } else { (probe.backoff * 2).min(bounds.probe_most) };
        probe.next = Some(now + probe.backoff);
    }

    /// A job returned; `given_up` is its path if its thread had been given up.
    /// Whether the thread serves again, which it does if its lane is short.
    pub fn returned(&mut self, lane: Lane, given_up: Option<&Path>) -> bool {
        self.probe = [None, None];
        let Some(path) = given_up else { return true };
        let i = lane as usize;
        if let Some(at) = self.parked[i].iter().position(|(parked, _)| parked == path) { self.parked[i].swap_remove(at); }
        self.parked[i].len() >= self.width[i]
    }

    /// Give up a thread of `lane`. Whether to start its replacement.
    fn give_up(&mut self, lane: Lane, path: PathBuf, started: Instant) -> bool {
        let i = lane as usize;
        self.probe[i].get_or_insert(Probe { next: None, backoff: Duration::ZERO });
        self.parked[i].push((path, started));
        let replace = self.parked[i].len() <= self.width[i];
        if replace { self.spawned[i] += 1; }
        replace
    }
}

/// Give up every thread whose job is past its bound, at most once a second.
/// Makes no file system call, so it cannot park; the caller holds the lock.
pub(super) fn reap(q: &mut Queue, inner: &Arc<Inner>, now: Instant) {
    let every = Duration::from_secs(1).min(inner.bounds.idle_check);
    if q.shutdown || q.ab.last_reap.is_some_and(|at| now.saturating_duration_since(at) < every) {
        return;
    }
    q.ab.last_reap = Some(now);
    let elapsed = |r: &Running| now.saturating_duration_since(r.started) >= r.bound;
    for path in over(&q.in_flight, |_, r| elapsed(r)) {
        let running = q.in_flight.remove(&path).expect("collected under this lock");
        give_up(q, inner, Lane::Readers, "read", path, running);
    }
    for key in over(&q.cache_in_flight, |_, r| elapsed(r)) {
        let running = q.cache_in_flight.remove(&key).expect("collected under this lock");
        give_up(q, inner, Lane::Readers, "cache write", key.path, running);
    }
    let tasks = &q.operation_pending;
    for key in over(&q.structural_in_flight, |key, r| tasks.get(key).and_then(|task| task.quiet_for()).is_some_and(|quiet| quiet >= r.bound)) {
        let running = q.structural_in_flight.remove(&key).expect("collected under this lock");
        give_up(q, inner, Lane::Structural, "folder listing", key.path, running);
    }
}

fn over<K: Clone + Eq + std::hash::Hash>(jobs: &HashMap<K, Running>, past: impl Fn(&K, &Running) -> bool) -> Vec<K> {
    jobs.iter().filter(|(key, running)| past(key, running)).map(|(key, _)| key.clone()).collect()
}

fn give_up(q: &mut Queue, inner: &Arc<Inner>, lane: Lane, what: &str, path: PathBuf, running: Running) {
    log::warn!("cloud: a {what} has had no answer for {} s; its thread is given up", running.bound.as_secs());
    if q.ab.give_up(lane, path, running.started) { spawn(inner, lane, q.ab.spawned[lane as usize] - 1); }
}

/// Start one pool thread. Deliberately not retained for `join`: a thread
/// blocked in the kernel on a provider that has stopped answering cannot be
/// interrupted, so joining at shutdown would hang the quit. The process exits
/// and takes them with it.
pub(super) fn spawn(inner: &Arc<Inner>, lane: Lane, n: usize) {
    let inner = Arc::clone(inner);
    let name = match lane { Lane::Readers => format!("moss-cloud-{n}"), Lane::Structural => format!("moss-cloud-structure-{n}") };
    let _ = std::thread::Builder::new().name(name).spawn(move || {
        if inner.opt_in { crate::platform::materialize_on_this_thread(); }
        ON_READER.with(|on| on.set(true));
        match lane { Lane::Readers => reader_loop(&inner), Lane::Structural => structural_reader_loop(&inner) }
    });
}

#[cfg(test)]
#[path = "abandon_tests.rs"]
mod tests;
