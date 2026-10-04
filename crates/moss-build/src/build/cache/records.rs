//! Reading and merge-writing transform records.
//!
//! A record file is as shared as a blob: on a cloud-synced folder it can be a
//! placeholder on this machine and real everywhere else. "Could not read it"
//! and "there is none" must therefore stay apart, because the writers merge
//! into the existing record — treating an unreadable one as empty and writing
//! back replaces the shared record with a one-entry stub.

use super::{TransformCache, TransformRecord};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// What a record lookup does about a record that is a placeholder in the cloud.
///
/// Either way the file is requested, so a later build finds it downloaded. The
/// modes differ only in whether this lookup waits for it: wait only where a
/// miss costs an image or video encode. Every other miss (a typeset equation, an
/// injected slot, a map tile, a metadata probe) is recomputed in milliseconds,
/// and a wait of up to five seconds per record would hold up the build phase a
/// preview is waiting on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordMode {
    /// Request the file and wait for it, up to a short deadline.
    Wait,
    /// Request the file and carry on: the record reads as unreadable now.
    Request,
}

/// What reading a record found.
pub(crate) enum RecordRead {
    Present(TransformRecord),
    /// No such record: the source has never been transformed.
    Absent,
    /// There may be a record, but it could not be read, or it was written by
    /// something that does not match this version. Overwriting it would destroy
    /// entries this machine cannot see.
    Unreadable,
}

impl RecordRead {
    pub(super) fn present(self) -> Option<TransformRecord> {
        match self {
            Self::Present(record) => Some(record),
            _ => None,
        }
    }
}

/// Whether [`TransformCache::merge`] wrote.
#[derive(Debug, PartialEq, Eq)]
pub enum Merged {
    Written,
    /// The existing record was left as it is: unreadable, or already what the
    /// edit would write.
    Kept,
}

/// Serialize a record's transforms in name order, so two equal records are
/// byte-equal and a re-save by another machine changes nothing.
pub(super) fn sorted<S: serde::Serializer>(
    transforms: &HashMap<String, super::TransformEntry>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_map(transforms.iter().collect::<std::collections::BTreeMap<_, _>>())
}

/// Files already reported as unusable in this process, so a build that meets the
/// same bad record for every image says so once.
static REPORTED: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Warn about `path` unless this process already has. Whether it did.
fn report_once(path: &Path, what: &str) -> bool {
    let first = REPORTED.lock().map_or(true, |mut seen| seen.insert(path.to_path_buf()));
    if first {
        log::warn!("[cache] record {} {}", path.display(), what);
    }
    first
}

impl TransformCache {
    /// The one place a record's text is read from disk.
    fn load(&self, path: &Path, mode: RecordMode) -> io::Result<String> {
        crate::build::cloud_readiness::read_record_text(path, self.root(), mode)
    }

    pub(crate) fn read(&self, source_oid: &str, mode: RecordMode) -> RecordRead {
        self.read_via(source_oid, mode, &|path, mode| self.load(path, mode))
    }

    pub(super) fn read_via(
        &self,
        source_oid: &str,
        mode: RecordMode,
        load: &dyn Fn(&Path, RecordMode) -> io::Result<String>,
    ) -> RecordRead {
        let path = self.record_path(source_oid);
        match load(&path, mode) {
            Ok(data) => match serde_json::from_str(&data) {
                Ok(record) => RecordRead::Present(record),
                // Empty, or ending mid-document: local writers rename a finished
                // temp file into place, so this is a file a sync provider has not
                // finished delivering, and another machine's full record may be
                // behind it. Replacing it would propagate the loss.
                Err(e) if e.is_eof() => {
                    report_once(&path, "is empty or cut off, probably still being delivered; it is left as it is");
                    RecordRead::Unreadable
                }
                // Not JSON, and not for running out of text: the file is damaged
                // here. Replace it on the next write.
                Err(_) if serde_json::from_str::<serde_json::Value>(&data).is_err() => {
                    report_once(&path, "is not valid JSON; it will be replaced");
                    RecordRead::Absent
                }
                // JSON of another shape: written by a different version, and its
                // entries are not ours to drop.
                Err(e) => {
                    report_once(&path, &format!("does not fit this version ({e}); it is left as it is"));
                    RecordRead::Unreadable
                }
            },
            Err(e) if crate::build::icloud::is_definitely_absent(&path, &e) => RecordRead::Absent,
            Err(e) => {
                // A placeholder that did not arrive is routine and was already
                // requested; anything else is worth saying once.
                if !crate::build::icloud::is_offline_not_absent(&path, &e) {
                    report_once(&path, &format!("cannot be read ({e}); it is left as it is"));
                } else {
                    log::debug!("[cache] record {} is in the cloud: {}", path.display(), e);
                }
                RecordRead::Unreadable
            }
        }
    }

    /// Apply `edit` to the source's record — a fresh one of `source_size` when
    /// there is none — and write it back, keeping every entry `edit` leaves
    /// alone. An unreadable record is not written over, in either mode: the work
    /// just done goes unrecorded and is found or redone by a later build.
    pub fn merge(
        &self,
        source_oid: &str,
        source_size: u64,
        mode: RecordMode,
        edit: impl FnOnce(&mut TransformRecord),
    ) -> Result<Merged, String> {
        self.merge_via(source_oid, source_size, mode, &|path, mode| self.load(path, mode), edit)
    }

    pub(super) fn merge_via(
        &self,
        source_oid: &str,
        source_size: u64,
        mode: RecordMode,
        load: &dyn Fn(&Path, RecordMode) -> io::Result<String>,
        edit: impl FnOnce(&mut TransformRecord),
    ) -> Result<Merged, String> {
        let (mut record, read) = match self.read_via(source_oid, mode, load) {
            RecordRead::Present(record) => (record.clone(), Some(record)),
            RecordRead::Absent => {
                (TransformRecord { source_oid: source_oid.to_string(), source_size, transforms: HashMap::new() }, None)
            }
            RecordRead::Unreadable => return Ok(Merged::Kept),
        };
        edit(&mut record);
        // Rewriting an identical record would make a file the sync provider
        // uploads again, on every machine sharing the folder.
        if read.as_ref() == Some(&record) {
            return Ok(Merged::Kept);
        }
        self.put(&record).map(|()| Merged::Written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::cache::{ObjectStore, TransformEntry};
    use crate::build::cloud_readiness::{read_record_text_with, RecordIo, WaitBreaker};
    use std::cell::Cell;
    use std::time::Instant;

    const OID: &str = "abcdef0123456789";

    fn cache(dir: &Path) -> TransformCache {
        TransformCache::new(dir.join("transforms"), ObjectStore::new(dir.join("objects")))
    }

    fn entry(tag: &str) -> TransformEntry {
        TransformEntry { oid: tag.to_string(), size: 1, params: serde_json::Value::Null }
    }

    /// A record file the provider has not downloaded: reads are refused until a
    /// request has been made, or for ever when `arrives` is false. The breaker
    /// and the read wrapper are the production code; the request, the wait and
    /// the classification are scripted.
    struct Placeholder {
        requested: Cell<u32>,
        arrives: Cell<bool>,
        breaker: WaitBreaker,
    }

    impl Placeholder {
        fn new(arrives: bool) -> Self {
            Self { requested: Cell::new(0), arrives: Cell::new(arrives), breaker: WaitBreaker::new() }
        }

        fn load(&self, path: &Path, mode: RecordMode) -> io::Result<String> {
            let read = || {
                if self.requested.get() > 0 && self.arrives.get() {
                    std::fs::read_to_string(path)
                } else {
                    Err(io::Error::other("in the cloud"))
                }
            };
            let wait = |attempt: &dyn Fn() -> io::Result<String>| {
                let first = attempt();
                if first.is_ok() {
                    return (first, false);
                }
                self.requested.set(self.requested.get() + 1);
                match attempt() {
                    Ok(text) => (Ok(text), false),
                    Err(_) => (first, true),
                }
            };
            read_record_text_with(
                mode,
                &self.breaker,
                &RecordIo {
                    now: &Instant::now,
                    read: &read,
                    in_cloud: &|_| true,
                    request: &|| self.requested.set(self.requested.get() + 1),
                    wait: &wait,
                },
            )
        }
    }

    fn seeded(dir: &Path, entries: &[&str]) -> TransformCache {
        let c = cache(dir);
        c.merge(OID, 10, RecordMode::Request, |r| {
            for e in entries {
                r.transforms.insert(e.to_string(), entry(e));
            }
        })
        .unwrap();
        c
    }

    #[test]
    fn a_waiting_read_fetches_a_placeholder_and_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let c = seeded(dir.path(), &["image/webp"]);
        let p = Placeholder::new(true);
        let RecordRead::Present(record) = c.read_via(OID, RecordMode::Wait, &|path, mode| p.load(path, mode)) else {
            panic!("the record should have been fetched and read")
        };
        assert!(record.transforms.contains_key("image/webp"));
        assert_eq!(p.requested.get(), 1);
    }

    #[test]
    fn a_requesting_read_asks_for_the_placeholder_and_does_not_wait_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let c = seeded(dir.path(), &["image/webp"]);
        let p = Placeholder::new(true);
        let load = |path: &Path, mode| p.load(path, mode);
        assert!(matches!(c.read_via(OID, RecordMode::Request, &load), RecordRead::Unreadable));
        assert_eq!(p.requested.get(), 1, "requested");
        // The next build finds it downloaded.
        assert!(matches!(c.read_via(OID, RecordMode::Request, &load), RecordRead::Present(_)));
    }

    #[test]
    fn a_record_that_stays_in_the_cloud_is_unreadable_and_left_alone_in_either_mode() {
        for mode in [RecordMode::Wait, RecordMode::Request] {
            let dir = tempfile::tempdir().unwrap();
            let c = seeded(dir.path(), &["image/webp", "image/webp-w800", "image/webp-w1600"]);
            let before = std::fs::read(c.record_path(OID)).unwrap();
            let p = Placeholder::new(false);
            let load = |path: &Path, mode| p.load(path, mode);
            assert!(matches!(c.read_via(OID, mode, &load), RecordRead::Unreadable));
            let merged = c.merge_via(OID, 10, mode, &load, |r| {
                r.transforms.insert("media/meta".into(), entry("m"));
            });
            assert_eq!(merged, Ok(Merged::Kept));
            assert_eq!(std::fs::read(c.record_path(OID)).unwrap(), before);
        }
    }

    #[test]
    fn merge_writes_a_fresh_record_when_there_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        let merged = c.merge(OID, 42, RecordMode::Request, |r| {
            r.transforms.insert("image/webp".into(), entry("w"));
        });
        assert_eq!(merged, Ok(Merged::Written));
        let record = c.get_with(OID, RecordMode::Request).unwrap();
        assert_eq!(record.source_size, 42);
        assert_eq!(record.transforms.len(), 1);
    }

    #[test]
    fn merge_keeps_the_entries_it_did_not_touch() {
        let dir = tempfile::tempdir().unwrap();
        let c = seeded(dir.path(), &["image/webp", "image/webp-w800", "image/webp-w1600"]);
        c.merge(OID, 10, RecordMode::Request, |r| {
            r.transforms.insert("media/meta".into(), entry("m"));
        })
        .unwrap();
        let mut keys: Vec<_> = c.get_with(OID, RecordMode::Request).unwrap().transforms.into_keys().collect();
        keys.sort();
        assert_eq!(keys, ["image/webp", "image/webp-w1600", "image/webp-w800", "media/meta"]);
    }

    fn write_record_file(c: &TransformCache, oid: &str, bytes: &[u8]) -> PathBuf {
        let path = c.record_path(oid);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_record_that_is_not_json_is_replaced_by_the_next_write() {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        let path = write_record_file(&c, OID, b"{ not json");
        assert!(matches!(c.read(OID, RecordMode::Request), RecordRead::Absent));
        let merged = c.merge(OID, 7, RecordMode::Request, |r| {
            r.transforms.insert("image/webp".into(), entry("w"));
        });
        assert_eq!(merged, Ok(Merged::Written));
        assert!(c.get_with(OID, RecordMode::Request).unwrap().transforms.contains_key("image/webp"));
        assert_ne!(std::fs::read(&path).unwrap(), b"{ not json");
    }

    /// A zero-byte or cut-off file is what a sync provider leaves while it is
    /// still delivering; local writers rename a finished temp file into place.
    fn assert_kept_byte_identical(bytes: &[u8]) {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        let path = write_record_file(&c, OID, bytes);
        assert!(matches!(c.read(OID, RecordMode::Request), RecordRead::Unreadable));
        let merged = c.merge(OID, 1, RecordMode::Wait, |r| {
            r.transforms.insert("image/webp".into(), entry("w"));
        });
        assert_eq!(merged, Ok(Merged::Kept));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn an_empty_record_file_is_unreadable_and_left_as_it_is() {
        assert_kept_byte_identical(b"");
    }

    #[test]
    fn a_record_cut_off_halfway_is_unreadable_and_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let full = seeded(dir.path(), &["image/webp", "image/webp-w800"]);
        let text = std::fs::read(full.record_path(OID)).unwrap();
        assert_kept_byte_identical(&text[..text.len() / 2]);
    }

    #[test]
    fn text_that_fails_before_its_end_is_still_replaced() {
        let bytes: &[u8] = b"{ not json at all }";
        let err = serde_json::from_slice::<serde_json::Value>(bytes).unwrap_err();
        assert!(!err.is_eof(), "the fixture must fail on syntax, not on running out of text");
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        write_record_file(&c, OID, bytes);
        assert!(matches!(c.read(OID, RecordMode::Request), RecordRead::Absent));
    }

    #[test]
    fn a_record_of_another_shape_is_unreadable_and_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        let other = br#"{"version": 9, "entries": ["written by a newer moss"]}"#;
        let path = write_record_file(&c, OID, other);
        assert!(matches!(c.read(OID, RecordMode::Request), RecordRead::Unreadable));
        assert_eq!(c.merge(OID, 1, RecordMode::Wait, |_| {}), Ok(Merged::Kept));
        assert_eq!(std::fs::read(&path).unwrap(), other);
    }

    /// Both kinds of unusable record are reported, and once per file per process
    /// however many times a build meets them.
    #[test]
    fn an_unusable_record_is_reported_once_per_file() {
        let dir = tempfile::tempdir().unwrap();
        let c = cache(dir.path());
        let broken = write_record_file(&c, "1111aaaa", b"{ not json");
        let foreign = write_record_file(&c, "2222bbbb", b"[]");
        for oid in ["1111aaaa", "2222bbbb"] {
            for _ in 0..3 {
                c.read(oid, RecordMode::Request);
            }
        }
        for path in [&broken, &foreign] {
            assert!(REPORTED.lock().unwrap().contains(path), "{} was not reported", path.display());
            assert!(!report_once(path, "again"), "a second report of {}", path.display());
        }
    }

    /// Through the production call path: no injected reader, so replacing the
    /// record read in `TransformCache::read` with a plain `read_to_string`
    /// makes this fail. The refusal is the file system's own "permission
    /// denied", which no platform treats as a missing file.
    #[test]
    fn a_record_the_file_system_refuses_is_left_alone_by_get_and_merge() {
        let dir = tempfile::tempdir().unwrap();
        let c = seeded(dir.path(), &["image/webp", "image/webp-w800"]);
        let path = c.record_path(OID);
        let before = std::fs::read(&path).unwrap();
        crate::build::io_utils::fault::fail_reads(&path, libc::EACCES, 2);
        assert!(c.get_with(OID, RecordMode::Wait).is_none());
        let merged = c.merge(OID, 10, RecordMode::Wait, |r| {
            r.transforms.insert("media/meta".into(), entry("m"));
        });
        assert_eq!(merged, Ok(Merged::Kept));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        // Once the refusals are used up, the same record reads normally.
        assert!(c.get_with(OID, RecordMode::Wait).is_some());
    }

    /// macOS only: the fail-fast policy's answer for a placeholder is `EDEADLK`,
    /// and only macOS classifies that as "in the cloud". The wait runs for real
    /// here; the file is local, so the provider "delivers" at once.
    #[cfg(target_os = "macos")]
    #[test]
    fn through_the_real_read_path_a_placeholder_is_waited_for_or_only_requested_by_mode() {
        let dir = tempfile::tempdir().unwrap();
        let c = seeded(dir.path(), &["image/webp"]);
        let path = c.record_path(OID);
        crate::build::io_utils::fault::fail_reads(&path, libc::EDEADLK, 1);
        assert!(c.get_with(OID, RecordMode::Wait).is_some(), "Wait fetches the record and reads it");
        crate::build::io_utils::fault::fail_reads(&path, libc::EDEADLK, 1);
        assert!(c.get_with(OID, RecordMode::Request).is_none(), "Request does not wait");
        assert!(c.get_with(OID, RecordMode::Request).is_some(), "and the record is there for the next read");
    }

    #[test]
    fn an_edit_that_changes_nothing_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let c = seeded(dir.path(), &["image/webp"]);
        let path = c.record_path(OID);
        let long_ago = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(long_ago).unwrap();
        let bytes = std::fs::read(&path).unwrap();

        let merged = c.merge(OID, 10, RecordMode::Request, |r| {
            r.transforms.insert("image/webp".to_string(), entry("image/webp"));
        });

        assert_eq!(merged, Ok(Merged::Kept));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), long_ago, "the file was not replaced");
    }

    #[test]
    fn equal_records_serialize_to_the_same_bytes_whatever_the_insertion_order() {
        let names: Vec<String> = (0..24).map(|n| format!("transform-{n}")).collect();
        let bytes_of = |order: &mut dyn Iterator<Item = &String>| {
            let dir = tempfile::tempdir().unwrap();
            let c = cache(dir.path());
            c.merge(OID, 10, RecordMode::Request, |r| {
                for n in order {
                    r.transforms.insert(n.clone(), entry(n));
                }
            })
            .unwrap();
            std::fs::read_to_string(c.record_path(OID)).unwrap()
        };
        let (forward, backward) = (bytes_of(&mut names.iter()), bytes_of(&mut names.iter().rev()));
        assert!(forward == backward, "same entries, different bytes:\n{forward}\n{backward}");
    }
}
