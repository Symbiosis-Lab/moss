//! Manifest coordinator: single-writer drain of background-phase emits.
//!
//! Background-phase tasks (image conversion, video conversion, asset copy)
//! send `EmitMessage` over an `mpsc::Sender` to the coordinator, which owns
//! `PendingManifest` exclusively. Channel close (all senders dropped) is the
//! materialization barrier — when the channel drains, the coordinator seals
//! the manifest and returns the read-only `SealedManifest`.
//!
//! See also: `manifest.rs`, `context.rs`.

use std::collections::HashMap;

use tokio::sync::mpsc;

use crate::build::manifest::{HashBucket, PendingManifest, SealedManifest};
use crate::build::types::SourceMetadata;
use crate::types::content::SiteHashes;

/// One coordinator-bound message. Sent by background tasks; the coordinator
/// drains and applies in arrival order.
///
/// `File` registers an output-bucket entry (the original purpose of this
/// channel). `SourcesReplace` propagates the change-detection cache produced
/// by the deferred-asset walk so the next build's `prev_sources` reflects
/// THIS build's hashing work rather than a snapshot two builds old. See the
/// `SourcesReplace` arm of `ManifestCoordinator::run_until_drained`.
///
/// Intentionally NOT `Clone`: `mpsc::Sender::send` takes ownership, and the
/// `SourcesReplace` variant carries the full sources map (potentially ~40KB
/// on a 500-source site). Removing the derive prevents an accidental
/// `msg.clone()` in test scaffolding from silently duplicating that payload.
#[derive(Debug)]
pub enum EmitMessage {
    /// Register an output artifact at `rel_path` with content `hash`, routed
    /// to the appropriate field(s) of `SiteHashes` per `bucket`. `hash` is
    /// pre-computed by the producer (xxHash3 for render-phase emits, sha2 for
    /// asset-pipeline forwards from the CAS); the coordinator stores the
    /// value verbatim. `register_with_hash` preserves any existing mode
    /// prefix; otherwise prepends `100644:`.
    File {
        rel_path: String,
        hash: String,
        bucket: HashBucket,
        /// The CAS object id already backing these exact bytes, when the
        /// sender knows one: `copy_deferred_assets`'s asset walk, the image
        /// worker's main encode path (`emit_image_outputs_via_channel`), and
        /// the video worker's (`emit_video_outputs_via_channel`) all pass
        /// `Some` for their own freshly-produced entries. A carry-forward or
        /// self-heal registration on any of those three paths still passes
        /// `None` — it relinks a cached blob without surfacing which one —
        /// and falls back to the fingerprint check below. Threaded into
        /// `PendingManifest::ship_sources` as a `ShipSource::Cas` entry so
        /// `ship_phase` can copy from the immutable CAS blob instead of the
        /// mutable stage path.
        oid: Option<String>,
    },
    /// Bulk-replace `PendingManifest::inner.sources` with the change-detection
    /// cache produced by `copy_deferred_assets`. Sent once at the end of the
    /// deferred-asset walk. Captures both insertions (cache miss → fresh
    /// `SourceMetadata`) and deletions (live-set pruning) atomically. Before
    /// this message existed, `copy_deferred_assets` updated only a local
    /// clone of `site_hashes` and the coordinator's `inner.sources` was
    /// whatever the previous build's carry-forward left — so the on-disk
    /// `sources` map was always one build stale and `prev_sources` two builds
    /// stale.
    SourcesReplace(HashMap<String, SourceMetadata>),
    /// A producer could not verify `rel_path`'s output: an I/O error other than
    /// a positive `NotFound`. Nothing is registered, failed or deleted for it,
    /// and the generation that carries the mark is withheld.
    Unverified { rel_path: String, detail: String },
}

/// Single-owner coordinator that drains the emit channel into a `PendingManifest`
/// and seals it once all senders drop.
pub struct ManifestCoordinator {
    rx: mpsc::Receiver<EmitMessage>,
    pending: PendingManifest,
}

impl ManifestCoordinator {
    /// Construct a new coordinator with carry-forward state from the previous
    /// build. Returns the coordinator + the sender end. Clone the sender for
    /// each background worker; drop all clones to seal.
    pub fn new(carry_forward: SiteHashes) -> (Self, mpsc::Sender<EmitMessage>) {
        // Buffer size 256: well above the typical emit count per build (~few hundred
        // total in moss; each emit is ~80 bytes). Backpressure is fine — workers
        // will await on send if the coordinator is slow.
        let (tx, rx) = mpsc::channel(256);
        let coordinator = Self {
            rx,
            pending: PendingManifest::new(carry_forward),
        };
        (coordinator, tx)
    }

    /// Construct a coordinator that continues accumulating from an already-populated
    /// `PendingManifest` (e.g., one carrying render-phase emits from `build_inner`).
    ///
    /// Use this instead of `new` when the render phase has already registered emits
    /// into `pending` and those must carry forward to the deferred-phase manifest.
    /// `BackgroundHandle::spawn_with_pending` calls this internally.
    pub fn from_pending(pending: PendingManifest) -> (Self, mpsc::Sender<EmitMessage>) {
        let (tx, rx) = mpsc::channel(256);
        let coordinator = Self { rx, pending };
        (coordinator, tx)
    }

    /// Drain the channel until all senders drop, then seal.
    ///
    /// Awaiting this future is the materialization barrier: the call cannot
    /// return until every emit has been applied. The returned `SealedManifest`
    /// is the only path to a value that satisfies the deploy gate.
    pub async fn run_until_drained(mut self) -> SealedManifest {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                EmitMessage::File { rel_path, hash, bucket, oid } => {
                    self.pending.apply_message(rel_path, &hash, bucket, oid);
                }
                EmitMessage::SourcesReplace(sources) => {
                    self.pending.replace_sources(sources);
                }
                EmitMessage::Unverified { rel_path, detail } => {
                    self.pending.mark_unverified(rel_path, detail);
                }
            }
        }
        self.pending.seal()
    }
}

/// Test-only helpers for unit tests that exercise the coordinator round-trip.
///
/// Used by tests in `build/media/image.rs` and `build/media/video.rs` after
/// the legacy on-disk hash-write fallbacks (`update_image_hashes` /
/// `update_video_hashes`) were removed. Direct callers that
/// previously fired with `tx: None` now build a coordinator, drain it, and
/// inspect the resulting `SealedManifest`.
#[cfg(test)]
pub mod test_utils {
    use super::*;

    /// Construct a fresh `(tx, rx)` pair for a coordinator test. The caller
    /// passes `tx` as the `ImageRunContext`/`BackgroundContext` channel,
    /// drops it after dispatch, then passes `rx` to [`drain_into_sealed`].
    pub fn build_test_coordinator() -> (mpsc::Sender<EmitMessage>, mpsc::Receiver<EmitMessage>)
    {
        mpsc::channel(64)
    }

    /// Drain `rx` into a `PendingManifest` seeded from `carry_forward` and
    /// return the sealed manifest. The channel must be closed (all `tx`
    /// clones dropped) before this is awaited; otherwise it hangs.
    pub async fn drain_into_sealed(
        rx: mpsc::Receiver<EmitMessage>,
        carry_forward: SiteHashes,
    ) -> SealedManifest {
        drain_into(rx, PendingManifest::new(carry_forward)).await
    }

    /// Drain `rx` into a manifest the caller has already registered into, for
    /// tests that need render-phase entries to be *touched* (carry-forward
    /// alone does not survive `seal`'s mark-and-sweep).
    pub async fn drain_into(
        mut rx: mpsc::Receiver<EmitMessage>,
        mut pending: PendingManifest,
    ) -> SealedManifest {
        while let Some(msg) = rx.recv().await {
            match msg {
                EmitMessage::File { rel_path, hash, bucket, oid } => {
                    pending.apply_message(rel_path, &hash, bucket, oid);
                }
                EmitMessage::SourcesReplace(sources) => {
                    pending.replace_sources(sources);
                }
                EmitMessage::Unverified { rel_path, detail } => {
                    pending.mark_unverified(rel_path, detail);
                }
            }
        }
        pending.seal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn coordinator_seals_after_all_senders_drop() {
        let (coord, tx) = ManifestCoordinator::new(SiteHashes::default());

        // Two workers
        let tx2 = tx.clone();
        let h1 = tokio::spawn(async move {
            tx.send(EmitMessage::File {
                rel_path: "a.html".to_string(),
                hash: "aaa".to_string(),
                bucket: HashBucket::Files,
                oid: None,
            })
            .await
            .unwrap();
            // tx dropped here
        });
        let h2 = tokio::spawn(async move {
            tx2.send(EmitMessage::File {
                rel_path: "b.html".to_string(),
                hash: "bbb".to_string(),
                bucket: HashBucket::Files,
                oid: None,
            })
            .await
            .unwrap();
            // tx2 dropped here
        });

        // Spawn coordinator after senders so the test doesn't hang
        let drain = tokio::spawn(coord.run_until_drained());

        h1.await.unwrap();
        h2.await.unwrap();

        let sealed = drain.await.unwrap();
        assert!(sealed.files().contains_key("a.html"));
        assert!(sealed.files().contains_key("b.html"));
        assert_eq!(sealed.files().len(), 2);
        assert!(sealed.blocking_keys().contains("a.html"));
        assert!(sealed.blocking_keys().contains("b.html"));
    }

    #[tokio::test]
    async fn coordinator_seals_empty_when_all_senders_drop_immediately() {
        let (coord, tx) = ManifestCoordinator::new(SiteHashes::default());
        drop(tx);
        let sealed = coord.run_until_drained().await;
        assert!(sealed.files().is_empty());
    }

    #[tokio::test]
    async fn coordinator_routes_image_outputs_correctly() {
        let (coord, tx) = ManifestCoordinator::new(SiteHashes::default());

        tx.send(EmitMessage::File {
            rel_path: "og/home.png".to_string(),
            hash: "ddd".to_string(),
            bucket: HashBucket::ImageOutputs,
            oid: None,
        })
        .await
        .unwrap();
        drop(tx);

        let sealed = coord.run_until_drained().await;
        assert!(sealed.image_outputs().contains("og/home.png"));
        assert!(sealed.blocking_keys().contains("og/home.png"));
        // ImageOutputs must also enter `files` so the deploy wire manifest
        // at `deploy.rs:213` (built from `sealed.files()`) carries the path
        // and the seta server requests the upload. See 2026-05-15 fix in
        // `manifest.rs::register_with_hash`.
        assert!(
            sealed.files().contains_key("og/home.png"),
            "ImageOutputs must enter files for deploy upload"
        );
    }

    /// Regression for the sources-divergence followup:
    /// `SourcesReplace` propagates the change-detection
    /// cache produced by this build into the sealed manifest. Without it,
    /// `inner.sources` remains the previous build's carry-forward snapshot
    /// and the next build's `prev_sources` is two builds stale.
    #[tokio::test]
    async fn coordinator_applies_sources_replace() {
        let mut carry = SiteHashes::default();
        carry.sources.insert(
            "stale-source.md".to_string(),
            SourceMetadata { hash: "old".into(), size: 1, mtime: 1, mtime_nanos: None, ctime: None, inode: None },
        );
        let (coord, tx) = ManifestCoordinator::new(carry);

        let mut fresh = HashMap::new();
        fresh.insert(
            "live-source.md".to_string(),
            SourceMetadata { hash: "new".into(), size: 42, mtime: 1000, mtime_nanos: None, ctime: None, inode: None },
        );
        tx.send(EmitMessage::SourcesReplace(fresh)).await.unwrap();
        drop(tx);

        let sealed = coord.run_until_drained().await;
        let inner = sealed.site_hashes_view();
        assert!(
            inner.sources.contains_key("live-source.md"),
            "fresh source must be in sealed manifest (got: {:?})",
            inner.sources.keys().collect::<Vec<_>>()
        );
        assert!(
            !inner.sources.contains_key("stale-source.md"),
            "stale carry-forward source must be replaced (got: {:?})",
            inner.sources.keys().collect::<Vec<_>>()
        );
    }

    /// Pins the invariant that `seal()`'s mark-and-sweep prunes the four
    /// output buckets but NOT `sources`. If a future change to `seal()`
    /// accidentally sweeps `sources` along with the output buckets, this
    /// test fails: file emits are touched (kept), the `SourcesReplace`
    /// payload is NOT touched (would be wrongly pruned).
    #[tokio::test]
    async fn seal_keeps_sources_alongside_pruned_output_buckets() {
        let mut carry = SiteHashes::default();
        // Carry-forward output entry that this build does NOT re-emit —
        // mark-and-sweep at seal must drop it.
        carry.files.insert(
            "projects/old-slug/index.html".to_string(),
            "100644:dead".to_string(),
        );
        let (coord, tx) = ManifestCoordinator::new(carry);

        // Emit a fresh file (mark-and-sweep retains).
        tx.send(EmitMessage::File {
            rel_path: "projects/new-slug/index.html".to_string(),
            hash: "fresh".to_string(),
            bucket: HashBucket::Files,
            oid: None,
        })
        .await
        .unwrap();

        // Send a SourcesReplace alongside the file emit.
        let mut fresh_sources = HashMap::new();
        fresh_sources.insert(
            "Projects/new.md".to_string(),
            SourceMetadata { hash: "src-hash".into(), size: 100, mtime: 2000, mtime_nanos: None, ctime: None, inode: None },
        );
        tx.send(EmitMessage::SourcesReplace(fresh_sources)).await.unwrap();
        drop(tx);

        let sealed = coord.run_until_drained().await;

        // Output bucket: stale carry-forward swept, fresh emit kept.
        assert!(sealed.files().contains_key("projects/new-slug/index.html"));
        assert!(
            !sealed.files().contains_key("projects/old-slug/index.html"),
            "stale carry-forward output must be swept at seal"
        );

        // sources: survives intact alongside the output-bucket sweep.
        let inner = sealed.site_hashes_view();
        assert!(
            inner.sources.contains_key("Projects/new.md"),
            "sources must survive seal even though the output buckets are pruned (got: {:?})",
            inner.sources.keys().collect::<Vec<_>>()
        );
    }
}
