//! Writing into a cache folder the cloud has not downloaded here.

use super::RecordMode;
use crate::build::cloud_readiness::{breaker_for, request_download, test_poll, Breakers, WaitBreaker};
use crate::build::icloud::{is_dataless_dir, is_dataless_unavailable};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What a write is doing when a directory refuses it: creating the directory
/// it needs, or writing a file into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Touch {
    Dir,
    File,
}

/// In a test, a directory marked cloud-only refuses writes below it, the way a
/// provider does. Runs inside [`retry_in_shard`] before every attempt, so the
/// refusal is there whether or not the retry is. Creating a directory that
/// already exists is not refused.
#[cfg(test)]
fn seam(path: &Path, touch: Touch) -> io::Result<()> {
    if touch == Touch::Dir && path.parent().is_some_and(Path::is_dir) {
        return Ok(());
    }
    crate::build::icloud::pretend::refusal_below(path).map_or(Ok(()), Err)
}

#[cfg(not(test))]
fn seam(_path: &Path, _touch: Touch) -> io::Result<()> {
    Ok(())
}

/// How long a write waits for a directory the cloud is delivering: long enough
/// for a provider round trip (about two seconds measured), short enough that a
/// build writing many files is not held up by one that never comes.
const SHARD_DEADLINE: Duration = Duration::from_secs(10);
/// How often the refused write is tried again.
const SHARD_POLL: Duration = Duration::from_millis(250);

#[cfg(test)]
fn shard_deadline() -> Duration {
    crate::build::cloud_readiness::TEST_DEADLINE.with(|d| d.get()).unwrap_or(SHARD_DEADLINE)
}

#[cfg(not(test))]
fn shard_deadline() -> Duration {
    SHARD_DEADLINE
}

/// The directory whose download makes a write to `path` possible: the
/// outermost directory between `root` and `path` that is still in the cloud,
/// since one inside it is then not even found. `None` when none is, which means
/// the refusal has another cause and waiting would not help. Includes `root`, but never anything above it.
fn responsible_dir(root: &Path, path: &Path) -> Option<PathBuf> {
    if !path.starts_with(root) {
        return None;
    }
    let below: Vec<&Path> = path.ancestors().skip(1).filter(|dir| dir.starts_with(root)).collect();
    below.into_iter().rev().find(|dir| is_dataless_dir(dir)).map(Path::to_path_buf)
}

/// One breaker per cache root for writes, apart from the one reads of the same
/// store sit behind: a read wait that timed out says nothing about a write, and
/// the other way round.
static WRITE_BREAKERS: Breakers = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// [`retry_in_shard`] behind the cache root's breaker for writes.
pub(super) fn in_shard<T>(
    root: &Path,
    path: &Path,
    mode: RecordMode,
    touch: Touch,
    op: impl Fn() -> io::Result<T>,
) -> io::Result<T> {
    let breaker = breaker_for(&WRITE_BREAKERS, root, "cache folders for new files");
    retry_in_shard(&breaker, root, path, mode, touch, op)
}

/// Run `op`, a write into the cache below `root` that ends up at `path`; if
/// the cloud refuses it because a directory on the way has not been downloaded
/// here, ask for that directory.
///
/// With [`RecordMode::Wait`], run `op` again every [`SHARD_POLL`] for up to
/// [`SHARD_DEADLINE`]: the write succeeding is the sign the directory arrived,
/// and past the deadline the first refusal is returned unchanged. With
/// [`RecordMode::Request`], return the refusal at once; a later build finds the
/// directory downloaded. A refusal with no directory in the cloud on the way is
/// returned at once in either mode, and counts for nothing.
///
/// The wait sits behind `breaker`: three waits in a row that ran to the
/// deadline pause it, and while it is paused the directory is asked for and the
/// write fails at once. The directory is never removed or recreated and never
/// listed from this thread — the cache syncs with the folder, and this thread
/// must not wait on the cloud.
fn retry_in_shard<T>(
    breaker: &WaitBreaker,
    root: &Path,
    path: &Path,
    mode: RecordMode,
    touch: Touch,
    op: impl Fn() -> io::Result<T>,
) -> io::Result<T> {
    let op = || {
        seam(path, touch)?;
        op()
    };
    let first = match op() {
        Err(e) if is_dataless_unavailable(&e) => e,
        done => return done,
    };
    let mut asked: Vec<PathBuf> = Vec::new();
    // Whether a directory in the cloud is the cause, asking for it once.
    let mut ask = || {
        let Some(dir) = responsible_dir(root, path) else { return false };
        if !asked.contains(&dir) {
            request_download(&dir);
            asked.push(dir);
        }
        true
    };
    if !ask() || mode == RecordMode::Request || breaker.is_paused(Instant::now()) {
        return Err(first);
    }
    let deadline = shard_deadline();
    let start = Instant::now();
    // Polls in a row that found no directory in the cloud: the one that just
    // arrived gets a second try, then the refusal is someone else's.
    let mut not_the_cause = 0;
    loop {
        std::thread::sleep(test_poll(SHARD_POLL));
        match op() {
            Err(e) if is_dataless_unavailable(&e) => {
                not_the_cause = if ask() { 0 } else { not_the_cause + 1 };
                if not_the_cause >= 2 {
                    return Err(first);
                }
            }
            done => {
                if done.is_ok() {
                    breaker.succeeded();
                }
                return done;
            }
        }
        if start.elapsed() >= deadline {
            breaker.timed_out(Instant::now());
            return Err(first);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::cache::blob_wait::BLOB_BREAKERS;
    use crate::build::cache::{ObjectStore, RecordMode, TransformCache, TransformEntry};
    use crate::build::cloud_readiness::TEST_DEADLINE;
    use crate::build::icloud::pretend;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    /// Shorten the wait on this thread (20 ms deadline, 1 ms poll) for the life of the guard.
    struct ShortWaits;
    impl ShortWaits {
        fn new() -> Self {
            TEST_DEADLINE.with(|d| d.set(Some(Duration::from_millis(20))));
            Self
        }
    }
    impl Drop for ShortWaits {
        fn drop(&mut self) {
            TEST_DEADLINE.with(|d| d.set(None));
        }
    }

    fn oid_of(data: &[u8]) -> String {
        format!("{:x}", Sha256::digest(data))
    }

    /// A store in its own temp dir with the shard for `data` already on disk.
    fn store_with_shard(data: &[u8]) -> (ObjectStore, PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::new(dir.path().join("objects"));
        let shard = store.blob_path(&oid_of(data)).parent().unwrap().to_path_buf();
        fs::create_dir_all(&shard).unwrap();
        (store, shard, dir)
    }

    fn cache_with_shard(oid: &str) -> (TransformCache, PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let cache = TransformCache::new(dir.path().join("transforms"), ObjectStore::new(dir.path().join("objects")));
        let shard = cache.record_path(oid).parent().unwrap().to_path_buf();
        fs::create_dir_all(&shard).unwrap();
        (cache, shard, dir)
    }

    fn merge_one(cache: &TransformCache, oid: &str, mode: RecordMode) -> Result<crate::build::cache::Merged, String> {
        cache.merge(oid, 1, mode, |r| {
            r.transforms.insert(
                "k".into(),
                TransformEntry { oid: "o".into(), size: 1, params: serde_json::Value::Null },
            );
        })
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_shard_the_cloud_has_not_delivered_is_requested_and_then_written_by_store_bytes() {
        let _short = ShortWaits::new();
        let (store, shard, _dir) = store_with_shard(b"bytes one");
        let _cloud = pretend::evicted_until_requested(&shard);
        let oid = store.store_bytes(b"bytes one", RecordMode::Wait).expect("stored once the shard arrived");
        assert_eq!(fs::read(store.blob_path(&oid)).unwrap(), b"bytes one");
        assert_eq!(pretend::requests_for(&shard), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_shard_the_cloud_has_not_delivered_is_requested_and_then_written_by_store_file() {
        let _short = ShortWaits::new();
        let (store, shard, dir) = store_with_shard(b"file one");
        let src = dir.path().join("src.bin");
        fs::write(&src, b"file one").unwrap();
        let _cloud = pretend::evicted_until_requested(&shard);
        let oid = store.store_file(&src, RecordMode::Wait).expect("stored once the shard arrived");
        assert_eq!(fs::read(store.blob_path(&oid)).unwrap(), b"file one");
        assert_eq!(pretend::requests_for(&shard), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_shard_the_cloud_has_not_delivered_is_requested_and_then_written_by_a_record_merge() {
        let _short = ShortWaits::new();
        let (cache, shard, _dir) = cache_with_shard("abcdef0123456789");
        let _cloud = pretend::evicted_until_requested(&shard);
        merge_one(&cache, "abcdef0123456789", RecordMode::Wait).expect("written once the shard arrived");
        assert!(cache.get_with("abcdef0123456789", RecordMode::Request).is_some());
        assert_eq!(pretend::requests_for(&shard), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_first_level_directory_the_cloud_has_not_delivered_is_requested_before_the_shard() {
        let _short = ShortWaits::new();
        let (store, shard, _dir) = store_with_shard(b"bytes two");
        let first = shard.parent().unwrap().to_path_buf();
        let _outer = pretend::evicted_until_requested(&first);
        let _inner = pretend::evicted_until_requested(&shard);
        store.store_bytes(b"bytes two", RecordMode::Wait).expect("stored once both arrived");
        assert_eq!(pretend::requests_for(&first), 1);
        assert_eq!(pretend::requests_for(&shard), 1);
        assert_eq!(pretend::requests_for(store.root()), 0, "never above the shard's first-level directory");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_missing_shard_inside_a_first_level_directory_not_delivered_is_created_after_the_request() {
        let _short = ShortWaits::new();
        let (store, shard, _dir) = store_with_shard(b"bytes three");
        let first = shard.parent().unwrap().to_path_buf();
        fs::remove_dir(&shard).unwrap();
        let _outer = pretend::evicted_until_requested(&first);
        let oid = store.store_bytes(b"bytes three", RecordMode::Wait).expect("created and stored once the directory arrived");
        assert_eq!(fs::read(store.blob_path(&oid)).unwrap(), b"bytes three");
        assert_eq!(pretend::requests_for(&first), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_store_root_itself_can_arrive_without_touching_its_parent() {
        let _short = ShortWaits::new();
        let (store, _shard, _dir) = store_with_shard(b"cloud root");
        let _cloud = pretend::evicted_until_requested(store.root());
        let oid = store.store_bytes(b"cloud root", RecordMode::Wait).expect("root arrived");
        assert_eq!(fs::read(store.blob_path(&oid)).unwrap(), b"cloud root");
        assert_eq!(pretend::requests_for(store.root()), 1);
        assert_eq!(pretend::requests_for(store.root().parent().unwrap()), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_shard_that_stays_refused_fails_with_the_usual_error_and_stops_waiting_after_three_timeouts() {
        let _short = ShortWaits::new();
        let (store, shard, _dir) = store_with_shard(b"never");
        let _cloud = pretend::evicted(&shard);
        for _ in 0..3 {
            let err = store.store_bytes(b"never", RecordMode::Wait).unwrap_err();
            assert!(err.contains("Resource deadlock avoided"), "{err}");
        }
        let asked = pretend::requests_for(&shard);
        // The fourth fails at once: with a wait it would run for the whole deadline.
        TEST_DEADLINE.with(|d| d.set(Some(Duration::from_secs(5))));
        let started = Instant::now();
        let err = store.store_bytes(b"never", RecordMode::Wait).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(2), "the fourth write waited");
        assert!(err.contains("Resource deadlock avoided"), "{err}");
        assert_eq!(pretend::requests_for(&shard), asked + 1, "a paused wait still asks for the directory");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn another_root_is_not_paused_and_a_success_resets_the_count() {
        let _short = ShortWaits::new();
        let (stuck, stuck_shard, _d1) = store_with_shard(b"stuck");
        let _stuck_cloud = pretend::evicted(&stuck_shard);
        for _ in 0..3 {
            stuck.store_bytes(b"stuck", RecordMode::Wait).unwrap_err();
        }
        let (other, other_shard, _d2) = store_with_shard(b"other");
        let _other_cloud = pretend::evicted_until_requested(&other_shard);
        other.store_bytes(b"other", RecordMode::Wait).expect("a root of its own is still waited for");

        // Two timeouts, a wait that succeeds, two timeouts: never three in a row, so never paused.
        let (flaky, flaky_shard, _d3) = store_with_shard(b"flaky");
        let timeouts = || {
            let _cloud = pretend::evicted(&flaky_shard);
            flaky.store_bytes(b"flaky", RecordMode::Wait).unwrap_err();
            flaky.store_bytes(b"flaky", RecordMode::Wait).unwrap_err();
        };
        timeouts();
        {
            let _cloud = pretend::evicted_until_requested(&flaky_shard);
            flaky.store_bytes(b"flaky", RecordMode::Wait).expect("arrived");
        }
        fs::remove_file(flaky.blob_path(&oid_of(b"flaky"))).unwrap();
        timeouts();
        // Paused, this would fail at once instead of waiting for the arrival.
        TEST_DEADLINE.with(|d| d.set(Some(Duration::from_secs(5))));
        let _cloud = pretend::evicted_until_requested(&flaky_shard);
        flaky.store_bytes(b"flaky", RecordMode::Wait).expect("not paused: this waits and succeeds");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_requesting_write_into_a_refused_shard_fails_at_once_after_one_request() {
        let _short = ShortWaits::new();
        let (store, shard, _dir) = store_with_shard(b"asked");
        let _cloud = pretend::evicted_until_requested(&shard);
        // Waiting would have succeeded: the shard arrives as soon as it is asked for.
        let err = store.store_bytes(b"asked", RecordMode::Request).unwrap_err();
        assert!(err.contains("Resource deadlock avoided"), "{err}");
        assert_eq!(pretend::requests_for(&shard), 1);
        // A later build finds the shard downloaded.
        store.store_bytes(b"asked", RecordMode::Request).expect("downloaded by now");
        assert_eq!(pretend::requests_for(&shard), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_write_timeout_does_not_pause_reads_and_a_read_timeout_does_not_pause_writes() {
        let _short = ShortWaits::new();
        let (stuck, stuck_shard, _d1) = store_with_shard(b"stuck");
        let _stuck_cloud = pretend::evicted(&stuck_shard);
        for _ in 0..3 {
            stuck.store_bytes(b"stuck", RecordMode::Wait).unwrap_err();
        }
        assert!(!breaker_for(&BLOB_BREAKERS, stuck.root(), "cached files").is_paused(Instant::now()));

        let (other, other_shard, _d2) = store_with_shard(b"other");
        let reads = breaker_for(&BLOB_BREAKERS, other.root(), "cached files");
        for _ in 0..3 {
            reads.timed_out(Instant::now());
        }
        assert!(reads.is_paused(Instant::now()));
        let _other_cloud = pretend::evicted_until_requested(&other_shard);
        other.store_bytes(b"other", RecordMode::Wait).expect("paused reads leave writes waiting");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_refusal_with_no_dataless_directory_on_the_path_returns_at_once_and_counts_nothing() {
        let _short = ShortWaits::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ab").join("cd").join("blob");
        let breaker = WaitBreaker::new();
        let tries = std::cell::Cell::new(0);
        let refused = || -> io::Result<()> {
            tries.set(tries.get() + 1);
            Err(io::Error::from_raw_os_error(libc::EDEADLK))
        };
        for _ in 0..3 {
            let err = retry_in_shard(&breaker, dir.path(), &path, RecordMode::Wait, Touch::File, refused).unwrap_err();
            assert!(is_dataless_unavailable(&err));
        }
        assert_eq!(tries.get(), 3, "one attempt each, no polling");
        assert!(!breaker.is_paused(Instant::now()), "nothing was counted");
        assert_eq!(pretend::requests_for(&path.parent().unwrap().to_path_buf()), 0);
    }


    #[cfg(unix)]
    #[test]
    fn a_refusal_that_is_not_dataless_fails_at_once_without_a_request() {
        use std::os::unix::fs::PermissionsExt;
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
            }
        }
        // Running as root ignores the mode, which would make this test vacuous.
        let (store, shard, _dir) = store_with_shard(b"read only");
        let _restore = Restore(shard.clone());
        fs::set_permissions(&shard, fs::Permissions::from_mode(0o555)).unwrap();
        if fs::write(shard.join("probe"), b"x").is_ok() {
            return;
        }
        TEST_DEADLINE.with(|d| d.set(Some(Duration::from_secs(5))));
        let _reset = ShortWaits;
        let before = pretend::requests_for(&shard);
        let err = store.store_bytes(b"read only", RecordMode::Wait).unwrap_err();
        assert!(err.contains("Permission denied"), "{err}");
        assert_eq!(pretend::requests_for(&shard), before);
    }
    // ----- Manual checks against a real cloud provider. Never run by default. -----
    //
    // A cache folder is created on one computer and synced; on another computer
    // its directories exist but have never been listed, so the provider keeps
    // them cloud-only until something asks for them. Each such directory can be
    // used once: after a check has fetched it, it is local for good, so every
    // run needs a shard that has not been touched on this computer.
    //
    // `MOSS_DATALESS_DIR_PROBE` names a cache root holding `objects/` and
    // `transforms/`; `MOSS_PROBE_SHARD` is the shard to write into, as `xx/yy`.
    // To cover a first-level directory too, name a shard whose `xx` is still
    // cloud-only. None of these lists a directory itself.
    //
    //   MOSS_DATALESS_DIR_PROBE=<root> MOSS_PROBE_SHARD=e1/00 \
    //     cargo test -p moss-build --lib real_provider_blob -- --ignored --nocapture
    //   MOSS_DATALESS_DIR_PROBE=<root> MOSS_PROBE_SHARD=e1/01 \
    //     cargo test -p moss-build --lib real_provider_request -- --ignored --nocapture
    //   (the record check: filter `real_provider_record`)

    fn probe_root_and_shard() -> (PathBuf, String, String) {
        let root = std::env::var("MOSS_DATALESS_DIR_PROBE").expect("MOSS_DATALESS_DIR_PROBE");
        let shard = std::env::var("MOSS_PROBE_SHARD").expect("MOSS_PROBE_SHARD, as xx/yy");
        let digits: String = shard.chars().filter(char::is_ascii_hexdigit).collect();
        assert_eq!(digits.len(), 4, "MOSS_PROBE_SHARD is xx/yy in hex");
        assert!(crate::platform::set_dataless_fail_fast(), "the fail-fast policy must be on, as in the app");
        (PathBuf::from(root), shard, digits)
    }

    fn state(dir: &Path) -> String {
        format!("{} dataless={}", dir.display(), is_dataless_dir(dir))
    }

    fn timed<T>(what: &str, shard_dir: &Path, f: impl FnOnce() -> T) -> T {
        let first = shard_dir.parent().unwrap();
        println!("before: {} / {}", state(first), state(shard_dir));
        let started = Instant::now();
        let out = f();
        println!("{what}: {:?}; after: {} / {}", started.elapsed(), state(first), state(shard_dir));
        out
    }

    /// Bytes whose SHA-256 starts with `digits`.
    fn bytes_for_shard(digits: &str) -> Vec<u8> {
        (0u64..).map(|n| n.to_le_bytes().to_vec()).find(|b| oid_of(b).starts_with(digits)).unwrap()
    }

    /// Waiting write: also covers a shard whose first-level directory is
    /// cloud-only too, when `MOSS_PROBE_SHARD` names one.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "needs MOSS_DATALESS_DIR_PROBE and MOSS_PROBE_SHARD on a real cloud folder"]
    fn real_provider_blob_into_a_cloud_only_directory() {
        let (root, shard, digits) = probe_root_and_shard();
        let store = ObjectStore::new(root.join("objects"));
        let data = bytes_for_shard(&digits);
        let dir = root.join("objects").join(&shard);
        let oid = timed("store_bytes", &dir, || store.store_bytes(&data, RecordMode::Wait)).expect("stored");
        println!("stored {oid}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "needs MOSS_DATALESS_DIR_PROBE and MOSS_PROBE_SHARD on a real cloud folder"]
    fn real_provider_request_mode_write_is_refused_at_once_and_lands_on_a_later_try() {
        let (root, shard, digits) = probe_root_and_shard();
        let store = ObjectStore::new(root.join("objects"));
        let data = bytes_for_shard(&digits);
        let dir = root.join("objects").join(&shard);
        let started = Instant::now();
        let first = timed("first store_bytes", &dir, || store.store_bytes(&data, RecordMode::Request));
        let refused_after = started.elapsed();
        let err = first.expect_err("a cloud-only shard refuses the first write");
        assert!(err.contains("Resource deadlock avoided"), "{err}");
        assert!(refused_after < Duration::from_secs(1), "refused after {refused_after:?}, not at once");
        let landed = loop {
            std::thread::sleep(Duration::from_millis(250));
            match store.store_bytes(&data, RecordMode::Request) {
                Ok(oid) => break oid,
                Err(e) => {
                    assert!(started.elapsed() < Duration::from_secs(15), "still refused after 15 s: {e}");
                }
            }
        };
        println!("stored {landed} on a retry {:?} after the first write", started.elapsed());
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "needs MOSS_DATALESS_DIR_PROBE and MOSS_PROBE_SHARD on a real cloud folder"]
    fn real_provider_record_into_a_cloud_only_shard() {
        let (root, shard, digits) = probe_root_and_shard();
        let cache = TransformCache::new(root.join("transforms"), ObjectStore::new(root.join("objects")));
        let oid = format!("{digits}{}", "0".repeat(60));
        let dir = root.join("transforms").join(&shard);
        let outcome = timed("merge", &dir, || {
            cache.merge(&oid, 1, RecordMode::Wait, |r| {
                r.transforms.insert(
                    "probe".into(),
                    TransformEntry { oid: "o".into(), size: 1, params: serde_json::Value::Null },
                );
            })
        })
        .expect("merged");
        // A record that cannot be read is never written over, so a shard that is
        // still cloud-only at the read makes `merge` stop there; write directly then.
        if matches!(outcome, crate::build::cache::Merged::Kept) {
            println!("merge kept the record unread; writing it directly");
            let record = crate::build::cache::TransformRecord {
                source_oid: oid.clone(),
                source_size: 1,
                transforms: Default::default(),
            };
            timed("put", &dir, || cache.put(&record, RecordMode::Wait)).expect("put");
        }
        println!("record written for {oid}");
    }
}
