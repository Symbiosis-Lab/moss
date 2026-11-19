//! Threads given up on a provider that stops answering: the lane is refilled,
//! the job is never failed or asked twice, and the threads stay bounded.

use super::super::*;
use super::Bounds;
use std::sync::Condvar;
use std::sync::atomic::AtomicUsize;

/// Bounds in milliseconds; the probe interval is long enough to see a lane narrowed.
fn quick() -> Bounds {
    Bounds {
        bytes: Duration::from_millis(60),
        bytes_per_mb: Duration::ZERO,
        publication: Duration::from_millis(60),
        structural_quiet: Duration::from_millis(60),
        probe_first: Duration::from_millis(400),
        probe_most: Duration::from_millis(400),
        idle_check: Duration::from_millis(20),
    }
}

/// A provider that holds every path named `wedged*` (or every path, with
/// `all`) until released, answers the rest at once, and counts each ask.
struct Provider {
    held: Mutex<bool>,
    freed: Condvar,
    all: bool,
    asks: Mutex<HashMap<PathBuf, usize>>,
}

impl Provider {
    fn new(all: bool) -> Arc<Self> {
        Arc::new(Self { held: Mutex::new(true), freed: Condvar::new(), all, asks: Mutex::new(HashMap::new()) })
    }

    fn materializer(self: &Arc<Self>) -> Materializer {
        let provider = Arc::clone(self);
        Arc::new(move |path: &Path| {
            *provider.asks.lock().unwrap().entry(path.to_path_buf()).or_default() += 1;
            if provider.all || path.to_string_lossy().contains("wedged") {
                let mut held = provider.held.lock().unwrap();
                while *held { held = provider.freed.wait(held).unwrap(); }
            }
            Ok(())
        })
    }

    fn asks(&self, path: &str) -> usize { self.asks.lock().unwrap().get(Path::new(path)).copied().unwrap_or(0) }

    fn total(&self) -> usize { self.asks.lock().unwrap().values().sum() }

    fn release(&self) {
        *self.held.lock().unwrap() = false;
        self.freed.notify_all();
    }
}

fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if done() { return true; }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

#[test]
fn a_parked_reader_is_replaced_so_the_queue_moves() {
    let provider = Provider::new(false);
    let p = Prefetcher::with_bounds(1, provider.materializer(), quick());
    p.read(Path::new("/f/wedged"));
    assert!(wait_until(Duration::from_secs(2), || provider.asks("/f/wedged") == 1));
    p.read(Path::new("/f/next"));
    assert!(wait_until(Duration::from_secs(3), || provider.asks("/f/next") == 1), "the file behind a parked read was never read");
    let snap = p.snapshot();
    assert_eq!(snap.parked, 1);
    assert_eq!(snap.oldest_read.map(|(path, _)| path), Some(PathBuf::from("/f/wedged")), "the stall no longer names the parked file");
    provider.release();
}

#[test]
fn every_parked_reader_is_noticed_with_no_new_file_and_no_caller() {
    let provider = Provider::new(true);
    let p = Prefetcher::with_bounds(2, provider.materializer(), quick());
    // Foreground, since two readers have one background slot.
    p.read_foreground(Path::new("/f/a"));
    p.read_foreground(Path::new("/f/b"));
    assert!(wait_until(Duration::from_secs(2), || provider.total() == 2), "premise: both readers parked");
    let parked_at = Instant::now();
    // Nothing enters the pool now: only its own idle threads can notice.
    assert!(wait_until(quick().bytes + Duration::from_secs(1), || p.threads_started() > 2 + STRUCTURAL_READERS),
        "no thread was given up within its bound and a second of {:?}", parked_at.elapsed());
    provider.release();
}

#[test]
fn a_wedged_file_is_neither_failed_nor_asked_again_while_others_answer() {
    let provider = Provider::new(false);
    let p = Prefetcher::with_bounds(2, provider.materializer(), quick());
    p.read(Path::new("/f/wedged"));
    assert!(wait_until(Duration::from_secs(2), || p.snapshot().parked == 1));
    for round in 0..5 {
        p.read(Path::new("/f/wedged"));
        p.read_foreground(Path::new("/f/wedged"));
        let other = format!("/f/other-{round}");
        p.read(Path::new(&other));
        assert!(wait_until(Duration::from_secs(2), || provider.asks(&other) == 1), "an answering file waited behind the wedged one");
        std::thread::sleep(quick().bytes * 2);
    }
    assert_eq!(provider.asks("/f/wedged"), 1, "a parked file was asked again");
    let snap = p.snapshot();
    assert_eq!((snap.reads.failed, snap.parked), (0, 1), "a wedged read was settled as failed");
    assert_eq!(p.threads_started(), 2 + STRUCTURAL_READERS + 1, "the lane lost width or grew past one replacement");
    provider.release();
}

#[test]
fn a_parked_read_that_returns_settles_once_and_can_be_asked_again() {
    let provider = Provider::new(false);
    let p = Prefetcher::with_bounds(1, provider.materializer(), quick());
    p.read(Path::new("/f/wedged"));
    assert!(wait_until(Duration::from_secs(2), || p.snapshot().parked == 1));
    provider.release();
    assert!(wait_until(Duration::from_secs(2), || p.snapshot().parked == 0));
    let snap = p.snapshot();
    assert_eq!((snap.reads.fetched, snap.done, snap.in_flight), (1, 1, 0));
    p.read(Path::new("/f/wedged"));
    assert!(wait_until(Duration::from_secs(2), || provider.asks("/f/wedged") == 2), "a settled file could not be asked again");
}

#[test]
fn while_stuck_one_probe_runs_and_a_return_restores_the_lanes() {
    let provider = Provider::new(true);
    let p = Prefetcher::with_bounds(2, provider.materializer(), quick());
    for name in ["a", "b", "c", "d", "e", "f"] { p.read_foreground(Path::new(&format!("/f/{name}"))); }
    assert!(wait_until(Duration::from_secs(2), || p.snapshot().parked == 2), "premise: the provider is stuck");
    // Both readers were given up and replaced; only one of the replacements may take a file before the probe's time.
    assert!(wait_until(Duration::from_secs(1), || provider.total() == 3));
    std::thread::sleep(quick().probe_first / 2);
    assert_eq!(provider.total(), 3, "a stuck pool took more than one probe");
    provider.release();
    assert!(wait_until(Duration::from_secs(2), || provider.total() == 6), "the lanes did not come back after a return");
}

#[test]
fn a_provider_that_never_answers_costs_at_most_twice_the_pool() {
    let provider = Provider::new(true);
    let bounds = Bounds { probe_first: Duration::from_millis(1), probe_most: Duration::from_millis(1), ..quick() };
    let p = Prefetcher::with_bounds(2, provider.materializer(), bounds);
    for i in 0..12 { p.read_foreground(Path::new(&format!("/f/{i}"))); }
    std::thread::sleep(Duration::from_millis(1500));
    for i in 12..24 { p.read_foreground(Path::new(&format!("/f/{i}"))); }
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(p.threads_started(), 2 * 2 + STRUCTURAL_READERS, "threads grew past twice the readers");
    assert_eq!(p.snapshot().parked, 4);
    provider.release();
}

#[test]
fn a_parked_publication_is_not_asked_again_and_a_newer_revision_waits_behind_it() {
    use crate::build::cloud_readiness::storage::{Operation, OperationKey, OperationPolicy, PublicationKind, StorageValue};
    let provider = Provider::new(true);
    let runs = Arc::new(Mutex::new(Vec::<(Instant, Instant)>::new()));
    let revision = Arc::new(AtomicU64::new(1));
    let (log, hold, current) = (Arc::clone(&runs), Arc::clone(&provider), Arc::clone(&revision));
    let publish: Operation = Arc::new(move || {
        let (started, version) = (Instant::now(), current.load(Ordering::Acquire));
        let mut held = hold.held.lock().unwrap();
        while *held { held = hold.freed.wait(held).unwrap(); }
        log.lock().unwrap().push((started, Instant::now()));
        Ok(StorageValue::Published(version))
    });
    let p = Prefetcher::with_bounds(2, provider.materializer(), quick());
    let key = OperationKey { path: "/cache/a".into(), policy: OperationPolicy::CachePublication { root: "/cache".into(), kind: PublicationKind::Blob } };
    p.cache_publication_revision(key.clone(), publish.clone(), Some(Arc::clone(&revision))).expect("queued");
    assert!(wait_until(Duration::from_secs(2), || p.snapshot().parked == 1));
    revision.store(2, Ordering::Release);
    p.cache_publication_revision(key, publish, Some(revision)).expect("joins the parked one");
    std::thread::sleep(quick().publication * 3);
    assert!(runs.lock().unwrap().is_empty(), "premise: the first write is still parked");
    provider.release();
    assert!(wait_until(Duration::from_secs(3), || runs.lock().unwrap().len() == 2), "the newer revision was not published");
    let runs = runs.lock().unwrap();
    assert!(runs[1].0 >= runs[0].1, "the newer write started before the parked one landed");
}

#[test]
fn a_parked_record_write_is_not_counted_as_landed_at_exit() {
    use crate::build::cloud_readiness::storage::{Operation, OperationKey, OperationPolicy, PublicationKind, StorageValue};
    let provider = Provider::new(true);
    let hold = Arc::clone(&provider);
    let write: Operation = Arc::new(move || {
        let mut held = hold.held.lock().unwrap();
        while *held { held = hold.freed.wait(held).unwrap(); }
        Ok(StorageValue::Published(1))
    });
    let p = Prefetcher::with_bounds(2, provider.materializer(), quick());
    let key = OperationKey { path: "/cache/record".into(), policy: OperationPolicy::CachePublication { root: "/cache".into(), kind: PublicationKind::Record } };
    p.cache_publication(key, write).expect("queued");
    assert!(wait_until(Duration::from_secs(2), || p.snapshot().parked == 1), "premise: the record write is given up");
    assert_eq!(p.drain_records_for_exit(Duration::from_millis(100), Duration::from_millis(300)), 1, "a parked record write was counted as landed");
    provider.release();
}

#[test]
fn a_stuck_folder_listing_does_not_hold_a_foreground_read() {
    use crate::build::cloud_readiness::storage::{Operation, OperationKey, OperationPolicy, StorageValue};
    let provider = Provider::new(false);
    let (hold, listings) = (Arc::clone(&provider), Arc::new(AtomicUsize::new(0)));
    let started = Arc::clone(&listings);
    let listing: Operation = Arc::new(move || {
        started.fetch_add(1, Ordering::SeqCst);
        let mut held = hold.held.lock().unwrap();
        while *held { held = hold.freed.wait(held).unwrap(); }
        Ok(StorageValue::Directory { entries: Vec::new(), hidden: 0 })
    });
    let bounds = Bounds { probe_first: Duration::from_secs(3), probe_most: Duration::from_secs(3), ..quick() };
    let p = Prefetcher::with_bounds(2, provider.materializer(), bounds);
    for i in 0..STRUCTURAL_READERS + 2 {
        p.structural_operation(OperationKey { path: format!("/f/dir-{i}").into(), policy: OperationPolicy::SourceWalk }, listing.clone()).unwrap();
    }
    // Both listings given up, and a replacement has taken the third as the probe.
    assert!(wait_until(Duration::from_secs(2), || listings.load(Ordering::SeqCst) == STRUCTURAL_READERS + 1), "premise: the structural probe runs");
    p.read_foreground(Path::new("/f/page.md"));
    assert!(wait_until(Duration::from_millis(500), || provider.asks("/f/page.md") == 1), "a foreground read waited for the structural lane's probe");
    provider.release();
}

#[test]
fn reads_that_return_with_the_file_still_in_the_cloud_say_stuck_until_one_delivers() {
    fn evicted(path: &Path) -> bool { !path.to_string_lossy().contains("arrives") }
    let provider = Provider::new(false);
    let p = Prefetcher::with_residency(2, provider.materializer(), quick(), evicted);
    let f = Path::new("/f");
    let _watch = p.watch(f);
    p.read_foreground(Path::new("/f/a"));
    p.read_foreground(Path::new("/f/b"));
    assert!(wait_until(Duration::from_secs(2), || p.snapshot().reads.still_in_cloud == 2), "premise: both reads returned");
    assert!(p.stuck_for(f), "reads that returned without delivering did not say the provider is not sending");
    p.read_foreground(Path::new("/f/arrives"));
    assert!(wait_until(Duration::from_secs(2), || p.snapshot().reads.fetched == 1), "premise: the third read delivered");
    assert!(!p.stuck_for(f), "a delivered read did not clear it");
}

#[test]
fn one_folders_held_reads_neither_mark_nor_are_cleared_by_another_folder() {
    let provider = Provider::new(false);
    let p = Prefetcher::with_bounds(2, provider.materializer(), quick());
    let (a, b) = (Path::new("/a"), Path::new("/b"));
    let _watching = (p.watch(a), p.watch(b));
    p.read(Path::new("/a/wedged"));
    assert!(wait_until(Duration::from_secs(2), || p.stuck_for(a)), "premise: folder a's read was given up");
    assert!(!p.stuck_for(b), "folder a's held read marked folder b");
    for name in ["one", "two", "three"] { p.read_foreground(&b.join(name)); }
    assert!(wait_until(Duration::from_secs(3), || p.snapshot().reads.fetched == 3), "premise: folder b's reads delivered");
    assert!(p.stuck_for(a), "folder b's deliveries cleared folder a");
    assert!(!p.stuck_for(b));
    provider.release();
    assert!(wait_until(Duration::from_secs(2), || !p.stuck_for(a)), "folder a's own return did not clear it");
}

#[test]
fn a_folder_keeps_its_verdict_until_its_last_watch_ends() {
    let provider = Provider::new(false);
    let p = Prefetcher::with_bounds(1, provider.materializer(), quick());
    let a = Path::new("/a");
    let (first, second) = (p.watch(a), p.watch(a));
    drop(first);
    p.read(Path::new("/a/wedged"));
    assert!(wait_until(Duration::from_secs(2), || p.stuck_for(a)), "a folder still watched lost its verdict");
    drop(second);
    assert!(!p.stuck_for(a), "a folder no one watches still has a verdict");
    provider.release();
}

#[test]
fn a_held_listing_marks_its_folder_and_a_held_write_neither_marks_nor_clears_it() {
    use crate::build::cloud_readiness::storage::{Operation, OperationKey, OperationPolicy, PublicationKind, StorageValue};
    fn held(provider: &Arc<Provider>, value: fn() -> StorageValue) -> Operation {
        let hold = Arc::clone(provider);
        Arc::new(move || {
            let mut held = hold.held.lock().unwrap();
            while *held { held = hold.freed.wait(held).unwrap(); }
            Ok(value())
        })
    }
    let (listing, write) = (Provider::new(false), Provider::new(false));
    let p = Prefetcher::with_bounds(2, listing.materializer(), quick());
    let f = Path::new("/f");
    let _watch = p.watch(f);
    let key = OperationKey { path: "/f/.moss/cache/a".into(), policy: OperationPolicy::CachePublication { root: "/f/.moss/cache".into(), kind: PublicationKind::Blob } };
    p.cache_publication(key, held(&write, || StorageValue::Published(1))).expect("queued");
    assert!(wait_until(Duration::from_secs(2), || p.snapshot().parked == 1), "premise: the write is given up");
    assert!(!p.stuck_for(f), "a held write said the provider is not sending");
    let walk = OperationKey { path: "/f/dir".into(), policy: OperationPolicy::SourceWalk };
    p.structural_operation(walk, held(&listing, || StorageValue::Directory { entries: Vec::new(), hidden: 0 })).unwrap();
    assert!(wait_until(Duration::from_secs(2), || p.stuck_for(f)), "a held listing did not mark its folder");
    write.release();
    assert!(wait_until(Duration::from_secs(2), || p.snapshot().parked == 1), "premise: the write returned");
    assert!(p.stuck_for(f), "a write's return cleared the folder's verdict");
    listing.release();
    assert!(wait_until(Duration::from_secs(2), || !p.stuck_for(f)), "the listing's return did not clear it");
}

#[test]
fn a_held_read_marks_the_nearest_watched_folder_only() {
    let provider = Provider::new(false);
    let p = Prefetcher::with_bounds(1, provider.materializer(), quick());
    let (outer, inner) = (Path::new("/a"), Path::new("/a/b"));
    let _watching = (p.watch(outer), p.watch(inner));
    p.read(Path::new("/a/b/wedged"));
    assert!(wait_until(Duration::from_secs(2), || p.stuck_for(inner)), "a held read did not mark its folder");
    assert!(!p.stuck_for(outer), "a nested folder's held read marked the folder around it");
    provider.release();
}
