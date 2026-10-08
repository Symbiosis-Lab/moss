//! Tests for the cloud reader threads.
//!
//! Linux has no dataless files, so the real materializer is a no-op here and
//! testing it would be vacuous. What these test is the isolation boundary —
//! the reason this module exists at all: that a file handed over gets read,
//! that it is not read twice, that a wedged read costs one thread and not the
//! pool, and that the concurrency bound holds.
//!
//! Foreground requests can promote queued inputs; native reads cannot be
//! interrupted, so the pool reserves capacity for them.
//!
//! Retry is tested, but it lives one level up — the supervisor's sweep re-hands
//! every file each reconcile, and the pool's job is only to not make that
//! unsafe. So what is asserted here is the dedup property that bounds it: a
//! returned read can be handed over again, a wedged one never is.

use super::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Records every path passed to it, in completion order.
fn recording() -> (Materializer, Arc<Mutex<Vec<PathBuf>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let m: Materializer = Arc::new(move |p: &Path| {
        sink.lock().unwrap().push(p.to_path_buf());
        Ok(())
    });
    (m, seen)
}

/// Blocks every call until the returned sender is used, once per release.
fn gated() -> (Materializer, mpsc::Sender<()>, Arc<Mutex<Vec<PathBuf>>>) {
    let (tx, rx) = mpsc::channel::<()>();
    let rx = Arc::new(Mutex::new(rx));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let m: Materializer = Arc::new(move |p: &Path| {
        sink.lock().unwrap().push(p.to_path_buf());
        let _ = rx.lock().unwrap().recv();
        Ok(())
    });
    (m, tx, seen)
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// Occupy the single reader, so files handed over after this genuinely queue.
///
/// Without it the reader can pick up the first file before the second has been
/// handed over, and a test that means to assert queue state asserts a race
/// instead. Only valid with [`gated`]; clears `seen` so the park is invisible.
fn park_the_reader(p: &Prefetcher, seen: &Arc<Mutex<Vec<PathBuf>>>) {
    p.read(Path::new("/vault/park"));
    assert!(wait_until(|| p.snapshot().in_flight == 1), "the reader never picked up the park file");
    seen.lock().unwrap().clear();
}

#[test]
fn background_admission_is_bounded_for_every_producer_and_preserves_demand() {
    let (m, tx, seen) = gated();
    let p = Prefetcher::with_materializer(1, m, false);
    park_the_reader(&p, &seen);
    for n in 0..3603 {
        p.read(&PathBuf::from(format!("/vault/background-{n}.jpg")));
    }
    assert_eq!(p.snapshot().waiting, BACKGROUND_WAITING);
    p.read_foreground(Path::new("/vault/requested.md"));
    assert_eq!(p.snapshot().waiting, BACKGROUND_WAITING + 1);
    tx.send(()).unwrap();
    assert!(wait_until(|| !seen.lock().unwrap().is_empty()));
    assert_eq!(seen.lock().unwrap()[0], PathBuf::from("/vault/requested.md"));
    for _ in 0..BACKGROUND_WAITING + 1 { let _ = tx.send(()); }
    assert!(wait_until(|| p.snapshot().done == (BACKGROUND_WAITING + 2) as u64));
    p.read(Path::new("/vault/previously-rejected.jpg"));
    assert!(wait_until(|| p.snapshot().in_flight == 1));
    tx.send(()).unwrap();
}

#[test]
fn a_file_handed_over_gets_read() {
    let (m, seen) = recording();
    let p = Prefetcher::with_materializer(2, m, false);
    p.read(Path::new("/vault/index.md"));

    assert!(wait_until(|| seen.lock().unwrap().len() == 1));
    assert_eq!(seen.lock().unwrap()[0], PathBuf::from("/vault/index.md"));
}

#[test]
fn the_same_file_is_not_read_twice() {
    // The build meets a file it could not read at the same moment the sweep
    // finds it evicted. That is one file, not two — the only reason `pending`
    // exists.
    let (m, tx, seen) = gated();
    let p = Prefetcher::with_materializer(1, m, false);
    park_the_reader(&p, &seen);

    for _ in 0..5 {
        p.read(Path::new("/vault/a.md"));
    }
    assert_eq!(p.snapshot().waiting, 1, "five hand-offs of one file queue it once");

    tx.send(()).unwrap(); // release the park; the reader picks up a.md
    assert!(wait_until(|| !seen.lock().unwrap().is_empty()));
    tx.send(()).unwrap(); // release a.md
    assert!(wait_until(|| p.snapshot().done == 2));

    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(seen.lock().unwrap().len(), 1, "read once, not five times");
}

#[test]
fn a_needed_page_promotes_an_already_queued_background_file() {
    let (m, tx, seen) = gated();
    let p = Prefetcher::with_materializer(1, m, false);
    park_the_reader(&p, &seen);
    p.read(Path::new("/vault/unrelated.jpg"));
    p.read(Path::new("/vault/index.md"));
    p.read_foreground(Path::new("/vault/index.md"));
    assert_eq!(p.snapshot().waiting, 2, "promotion must not duplicate the read");

    tx.send(()).unwrap();
    assert!(wait_until(|| !seen.lock().unwrap().is_empty()));
    assert_eq!(seen.lock().unwrap()[0], PathBuf::from("/vault/index.md"));
    tx.send(()).unwrap();
    tx.send(()).unwrap();
    assert!(wait_until(|| p.snapshot().done == 3));
}

#[test]
fn foreground_has_capacity_when_background_reads_are_stuck() {
    let (m, tx, seen) = gated();
    let p = Prefetcher::with_materializer(8, m, false);
    for n in 0..20 {
        p.read(&PathBuf::from(format!("/vault/other-{n}.jpg"))); // allow:served-path-url-construct
    }
    assert!(wait_until(|| p.snapshot().in_flight == 6));
    p.read_foreground(Path::new("/vault/index.md"));
    assert!(wait_until(|| seen.lock().unwrap().contains(&PathBuf::from("/vault/index.md"))));
    p.read_foreground(Path::new("/vault/posts/second.md"));
    assert!(wait_until(|| seen.lock().unwrap().contains(&PathBuf::from("/vault/posts/second.md"))));
    assert_eq!(p.snapshot().in_flight, 8);
    for _ in 0..22 {
        let _ = tx.send(());
    }
}

#[test]
fn concrete_failures_keep_their_full_count_and_clear_after_success() {
    let fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let failing = Arc::clone(&fail);
    let m: Materializer = Arc::new(move |_| {
        if failing.load(Ordering::SeqCst) {
            Err(std::io::Error::other("provider refused read"))
        } else {
            Ok(())
        }
    });
    let p = Prefetcher::with_materializer(1, m, false);
    let pending: HashSet<PathBuf> = (0..6).map(|n| PathBuf::from(format!("/vault/post-{n}.md"))).collect();
    for path in &pending { p.read(path); }
    assert!(wait_until(|| p.snapshot().done == 6));
    let (count, names) = p.failures_for(Path::new("/vault"), &pending);
    assert_eq!(count, 6);
    assert_eq!(names.len(), 3);
    let all = p.failure_paths_for(Path::new("/vault"), &pending);
    assert_eq!(all.len(), 6);
    assert!(all.windows(2).all(|pair| pair[0] < pair[1]), "full paths are sorted");
    let other = PathBuf::from("/other/failed.md");
    p.read(&other);
    assert!(wait_until(|| p.snapshot().done == 7));
    let active = HashSet::from([pending.iter().next().unwrap().clone()]);
    assert_eq!(p.failures_for(Path::new("/vault"), &active).0, 1);
    assert_eq!(p.failure_paths_for(Path::new("/vault"), &active), active.iter().cloned().collect::<Vec<_>>());
    assert_eq!(p.inner.queue.lock().unwrap().failures.len(), 2);
    assert_eq!(p.failures_for(Path::new("/other"), &HashSet::from([other])).0, 1);

    fail.store(false, Ordering::SeqCst);
    for path in &pending { p.read(path); }
    assert!(wait_until(|| p.snapshot().done == 13));
    assert_eq!(p.failures_for(Path::new("/vault"), &pending), (0, Vec::new()));
    assert!(p.failure_paths_for(Path::new("/vault"), &pending).is_empty(), "recovery clears the disclosed set");
}

#[test]
fn a_file_can_be_handed_over_again_once_its_read_returned() {
    // Dedup must not become a permanent refusal: the provider can evict a file
    // again after it lands, and the next sweep has to be able to name it.
    let (m, seen) = recording();
    let p = Prefetcher::with_materializer(1, m, false);
    p.read(Path::new("/vault/a.md"));
    assert!(wait_until(|| p.snapshot().done == 1));

    p.read(Path::new("/vault/a.md"));
    assert!(wait_until(|| p.snapshot().done == 2));
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn a_failed_read_leaves_no_trace_that_would_block_a_later_sweep() {
    // moss keeps no retry ledger — but it does retry, via the supervisor's
    // 60s re-hand-off of the whole sweep. That only works if a failed read
    // cleans up after itself completely: no lingering `pending` entry, no
    // leaked in-flight slot. Otherwise retry silently stops after attempt one.
    let m: Materializer = Arc::new(|_: &Path| Err(std::io::Error::other("nope")));
    let p = Prefetcher::with_materializer(1, m, false);
    p.read(Path::new("/vault/a.md"));

    assert!(wait_until(|| p.snapshot().done == 1));
    let s = p.snapshot();
    assert_eq!(s.waiting, 0);
    assert_eq!(s.in_flight, 0, "a failed read must not leak an in-flight slot");

    p.read(Path::new("/vault/a.md"));
    assert!(wait_until(|| p.snapshot().done == 2), "the next sweep can hand it over again");
}

#[test]
fn one_wedged_read_does_not_block_the_others() {
    // The whole reason this module exists. If the lock were held across the
    // read, one file the provider abandons would take the pool with it — which
    // is the app-wide freeze that motivated the design, reproduced in miniature.
    let (tx, rx) = mpsc::channel::<()>();
    let rx = Arc::new(Mutex::new(rx));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let m: Materializer = Arc::new(move |p: &Path| {
        sink.lock().unwrap().push(p.to_path_buf());
        if p == Path::new("/vault/wedged.md") {
            let _ = rx.lock().unwrap().recv();
        }
        Ok(())
    });

    let p = Prefetcher::with_materializer(2, m, false);
    p.read(Path::new("/vault/wedged.md"));
    for n in 0..5 {
        p.read_foreground(&PathBuf::from(format!("/vault/{n}.md"))); // allow:served-path-url-construct
    }

    assert!(wait_until(|| p.snapshot().done == 5));
    assert_eq!(p.snapshot().in_flight, 1, "the wedged read is still holding one thread");
    tx.send(()).unwrap();
    assert!(wait_until(|| p.snapshot().done == 6));
}

#[test]
fn no_more_than_the_reader_count_are_read_at_once() {
    // The bound is the blast radius: this many threads is what moss can lose to
    // a provider that stops answering and still function.
    let (m, _tx, _seen) = gated();
    let p = Prefetcher::with_materializer(3, m, false);
    for n in 0..20 {
        p.read(&PathBuf::from(format!("/vault/{n}.md"))); // allow:served-path-url-construct
    }

    assert!(wait_until(|| p.snapshot().in_flight == 1));
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(p.snapshot().in_flight, 1, "background reads leave room for foreground inputs");
}

#[test]
fn the_snapshot_accounts_for_every_file_handed_over() {
    // This is what separates "the provider went quiet" from "moss never asked"
    // in a stall report, so it has to stay equal to what was handed over.
    let (m, tx, _seen) = gated();
    let p = Prefetcher::with_materializer(2, m, false);
    for n in 0..10 {
        p.read(&PathBuf::from(format!("/vault/{n}.md"))); // allow:served-path-url-construct
    }

    assert!(wait_until(|| p.snapshot().in_flight == 1));
    let s = p.snapshot();
    assert_eq!(s.waiting + s.in_flight + s.done as usize, 10);

    for _ in 0..10 {
        let _ = tx.send(());
    }
    assert!(wait_until(|| p.snapshot().done == 10));
    let s = p.snapshot();
    assert_eq!((s.waiting, s.in_flight), (0, 0));
}

#[test]
fn a_wedged_file_is_never_handed_over_again() {
    // The safety property behind unbounded retry. The supervisor re-hands its
    // whole sweep every 60s forever, which is only sound because a file whose
    // read never returns stays in `pending` and is therefore suppressed. If it
    // were re-queued, every sweep would burn another reader on the same dead
    // file and the pool would be gone in minutes.
    let (m, _tx, seen) = gated();
    let p = Prefetcher::with_materializer(3, m, false);
    p.read(Path::new("/vault/wedged.md"));
    assert!(wait_until(|| p.snapshot().in_flight == 1));

    for _ in 0..10 {
        p.read(Path::new("/vault/wedged.md")); // ten sweeps' worth
    }

    std::thread::sleep(Duration::from_millis(50));
    let s = p.snapshot();
    assert_eq!(s.in_flight, 1, "still exactly one thread, after ten re-hand-offs");
    assert_eq!(s.waiting, 0, "and nothing queued behind it");
    assert_eq!(seen.lock().unwrap().len(), 1, "read once");
}

#[test]
fn the_oldest_outstanding_read_is_named_and_aged() {
    // What separates "downloading, just slow" from "the provider abandoned
    // these" in a stall report. An aggregate count cannot: one stall report
    // was one file.
    let (m, tx, _seen) = gated();
    let p = Prefetcher::with_materializer(1, m, false);
    assert_eq!(p.snapshot().oldest_read, None, "nothing outstanding, nothing to name");

    p.read(Path::new("/vault/slow.md"));
    assert!(wait_until(|| p.snapshot().in_flight == 1));

    let (path, _) = p.snapshot().oldest_read.expect("a read is outstanding");
    assert_eq!(path, PathBuf::from("/vault/slow.md"));
    assert!(
        wait_until(|| {
            p.snapshot().oldest_read.is_some_and(|(_, age)| age >= Duration::from_millis(20))
        }),
        "the age grows while the read is outstanding"
    );

    tx.send(()).unwrap();
    assert!(wait_until(|| p.snapshot().oldest_read.is_none()), "cleared when the read returns");
}

#[test]
fn the_oldest_read_is_the_earliest_still_outstanding() {
    // Two wedged files: the report must name the first one, not whichever the
    // hash map happens to yield.
    let (m, _tx, seen) = gated();
    let p = Prefetcher::with_materializer(2, m, false);
    p.read(Path::new("/vault/first.md"));
    assert!(wait_until(|| seen.lock().unwrap().len() == 1));
    p.read_foreground(Path::new("/vault/second.md"));
    assert!(wait_until(|| p.snapshot().in_flight == 2));

    let (path, _) = p.snapshot().oldest_read.expect("two reads are outstanding");
    assert_eq!(path, PathBuf::from("/vault/first.md"));
}

#[test]
fn shutdown_stops_taking_work() {
    let (m, seen) = recording();
    let p = Prefetcher::with_materializer(2, m, false);
    p.shutdown();
    p.read(Path::new("/vault/a.md"));

    std::thread::sleep(Duration::from_millis(50));
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(p.snapshot().waiting, 0);
}

// ---------------------------------------------------------------------------
// The read itself.
//
// `materialize` cannot be tested directly: it asks the OS for a dataless file,
// which exists on one platform and only once a provider has evicted something.
// What *can* be tested is the fallback it drops to when coordination is
// unavailable — that the read runs to EOF rather than stopping at the first
// chunk, which is what every non-macOS platform gets.

/// A reader that hands out `chunk` bytes at a time, counts what it gave, and
/// can be told to raise `EINTR` once before its first real chunk.
struct CountingReader {
    remaining: usize,
    chunk: usize,
    handed_out: usize,
    interrupt_first: bool,
}

impl std::io::Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.interrupt_first {
            self.interrupt_first = false;
            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
        }
        let n = self.remaining.min(self.chunk).min(buf.len());
        self.remaining -= n;
        self.handed_out += n;
        Ok(n)
    }
}

impl CountingReader {
    fn of(len: usize, chunk: usize) -> Self {
        Self { remaining: len, chunk, handed_out: 0, interrupt_first: false }
    }
}

#[test]
fn a_read_runs_to_eof_and_not_to_the_first_chunk() {
    // The bug this replaces: one 1-byte read, then declare victory. A source
    // that only ever yields small chunks must still be consumed entirely.
    let mut r = CountingReader::of(5 * 1024 * 1024, 4096);
    drain_to_eof(&mut r).expect("draining a healthy reader succeeds");
    assert_eq!(
        r.handed_out,
        5 * 1024 * 1024,
        "every byte must be read — a prefix is what left deployed-article-map.json dataless"
    );
}

#[test]
fn an_empty_source_is_materialized_not_a_failure() {
    // A zero-byte file returns Ok(0) on the first pass and is perfectly
    // materialized. `read_exact` would call this UnexpectedEof and report a
    // failed download for a file that is fine.
    let mut r = CountingReader::of(0, 4096);
    drain_to_eof(&mut r).expect("an empty source is not an error");
    assert_eq!(r.handed_out, 0);
}

#[test]
fn a_signal_is_not_the_provider_refusing() {
    let mut r = CountingReader::of(8192, 4096);
    r.interrupt_first = true;
    drain_to_eof(&mut r).expect("EINTR is retried, not reported");
    assert_eq!(r.handed_out, 8192, "the interrupt must not cost any bytes");
}

#[test]
fn a_real_error_stops_the_read_and_is_reported() {
    struct Failing;
    impl std::io::Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        }
    }
    let err = drain_to_eof(&mut Failing).expect_err("a real error is reported");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
}
