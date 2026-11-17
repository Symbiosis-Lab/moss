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
