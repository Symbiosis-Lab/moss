//! What a publish reports, and who it reports to.
//!
//! [`DeployProgress`] is the vocabulary; [`DeploySink`] is the one seam
//! between it and a user interface. The app's sink emits a Tauri event on the
//! "deploy-progress" channel; a terminal one would print. A publish body knows
//! neither — it calls [`DeploySink::stage`] and hands the sink onward.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::Arc;

/// Stages of a push operation, emitted as Tauri events.
///
/// Ordered roughly in the sequence they fire, though `Preparing` and
/// `Rebuilding` are skipped when the pre-deploy rebuild is a no-op (see
/// `should_skip_rebuild` in `deploy.rs`). `Finalizing` fires after the commit
/// for post-commit bookkeeping (redirect snapshot, analytics sync, domain
/// orchestrator) that used to run silently.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DeployStage {
    /// Waiting for in-flight background tasks (video conversion, etc.)
    /// and checking whether a rebuild is needed.
    Preparing,
    /// Running the pre-deploy rebuild (skipped when hashes match).
    Rebuilding,
    Syncing,
    Uploading,
    Committing,
    /// Post-commit bookkeeping: redirects, analytics, domain orchestrator.
    Finalizing,
    /// The upload committed and moss is confirming the site is actually
    /// serving it (OnionPress publishes only). Emitted by `deploy_site` right
    /// after the publish guard's `Complete`, so the LAST stage a verifying
    /// publish shows is this one — the green success moment belongs to the
    /// `PublishVerdict` event, not to upload completion.
    Verifying,
    Complete,
    Failed,
}

/// Progress event emitted during push.
/// Frontend listens on "deploy-progress" event.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct DeployProgress {
    pub stage: DeployStage,
    pub current: u32,
    pub total: u32,
    pub message: String,
    /// Cumulative bytes uploaded so far this deploy. `Some` only during the
    /// Uploading stage; `None` on every other stage so the frontend falls back
    /// to the file-count fraction. (File-count progress is
    /// actively misleading for video-heavy sites — it reaches ~97% before the
    /// videos start uploading.)
    ///
    /// `f64`, not `u64`, deliberately: specta types `u64` as a TS `string`
    /// (precision-safety), but serde emits a JSON number at runtime — a type/
    /// runtime mismatch that breaks the frontend's `bytes_uploaded/bytes_total`
    /// arithmetic. `f64` types as `number` and is exact for byte counts up to
    /// 2^53 (≈ 9 PB). The hot accumulator stays `AtomicU64`; we widen at emit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_uploaded: Option<f64>,
    /// Total bytes to upload this deploy (sum of `diff.need` file sizes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes_total: Option<f64>,
    /// Files this deploy will DELETE from the live site (`diff.remove.len()`).
    ///
    /// Here rather than left to the resting change set, which is the answer to
    /// a different question ("what would publishing change?"). Sending both
    /// down one channel is what forced the frontend to guess which of the two
    /// it was holding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removing: Option<u32>,
    /// The largest upload currently in flight, as a site-relative path.
    ///
    /// "The current file" is a choice, not a fact: 20 files upload at once. The
    /// largest is the one whose seconds the author is actually waiting on — a
    /// 40 MB video holds this slot for as long as it really takes, while the
    /// hundreds of small files that flash past were never readable anyway.
    ///
    /// Sampled at the ticker's 4 Hz. Minimum dwell and the swap animation are
    /// display policy and live in the frontend; this reports only what is true
    /// at the instant it is read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_file: Option<String>,
}

/// Where a publish's progress goes.
///
/// One method, because a publish has one thing to say. The app implements it
/// by emitting the Tauri event the frontend listens on, and tests by keeping
/// the events in a vector. Nothing below this trait knows which it is holding,
/// which is what will let one publish body serve both binaries.
///
/// Passed as `Arc<dyn DeploySink>` rather than a reference because the upload
/// ticker and the per-file upload tasks are spawned and outlive the call
/// frame.
pub trait DeploySink: Send + Sync {
    /// Report one progress event.
    fn deploy_progress(&self, event: DeployProgress);

    /// Report a stage change — the shape every caller outside the upload loop
    /// wants, with the four upload-only fields left empty.
    fn stage(&self, stage: DeployStage, current: u32, total: u32, message: &str) {
        self.deploy_progress(DeployProgress {
            stage,
            current,
            total,
            message: message.to_string(),
            bytes_uploaded: None,
            bytes_total: None,
            removing: None,
            current_file: None,
        });
    }

    /// Report one Uploading sample, read entirely from the shared upload
    /// accounting.
    ///
    /// One argument rather than seven: everything the upload stage reports —
    /// files done, files total, bytes, removals, which file is in flight — is
    /// already in [`UploadProgressState`], and threading them out one by one
    /// only created call sites that could disagree with each other. Both
    /// callers (the 4 Hz ticker and the final authoritative tick after
    /// `window.drain()`) send the same shape; the difference is only *when*.
    fn upload_sample(&self, st: &UploadProgressState) {
        use std::sync::atomic::Ordering;
        let done = st.completed_files.load(Ordering::Relaxed);
        self.deploy_progress(DeployProgress {
            stage: DeployStage::Uploading,
            current: done,
            total: st.total_files,
            message: format!("Uploading ({}/{})", done, st.total_files),
            // Widen to f64 for a clean TS `number` (see the struct field note).
            bytes_uploaded: Some(st.bytes_uploaded.load(Ordering::Relaxed) as f64),
            bytes_total: Some(st.bytes_total as f64),
            removing: Some(st.removing),
            current_file: st.largest_in_flight(),
        });
    }
}

/// A publish nobody is watching: `moss deploy` under `--quiet`, and every test
/// that only cares about the bytes on the wire.
struct SilentSink;

impl DeploySink for SilentSink {
    fn deploy_progress(&self, _event: DeployProgress) {}
}

/// The sink to pass when there is nothing to report to.
pub fn silent() -> Arc<dyn DeploySink> {
    Arc::new(SilentSink)
}

/// Everything a publish reported, in order.
///
/// The invariant this exists for is an ABSENCE: a publish rejected as a
/// duplicate must emit no terminal stage at all, because the in-flight publish
/// owns the panel. A test that passed no sink could not tell "emitted nothing"
/// from "emitted the wrong thing".
#[derive(Default)]
pub struct RecordingSink {
    events: std::sync::Mutex<Vec<DeployProgress>>,
}

impl RecordingSink {
    /// Take everything recorded so far.
    pub(crate) fn take(&self) -> Vec<DeployProgress> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// The same, as the `(stage, message)` pairs most assertions want.
    pub fn drain(&self) -> Vec<(DeployStage, String)> {
        self.take()
            .into_iter()
            .map(|e| (e.stage, e.message))
            .collect()
    }
}

impl DeploySink for RecordingSink {
    fn deploy_progress(&self, event: DeployProgress) {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(event);
    }
}

/// Shared upload accounting, created once per deploy and cloned into every
/// upload task via `Arc`. The hot path is `bytes_uploaded.fetch_add` (one per
/// chunk for large files, one per small file); a 4 Hz ticker reads the atomics
/// and emits. ("Ticker pattern": the 20 concurrent upload tasks would
/// otherwise flood the event channel with bursts then go silent for tens of
/// seconds.)
pub struct UploadProgressState {
    pub bytes_uploaded: AtomicU64,
    /// Immutable after creation: sum of `diff.need` file sizes.
    pub bytes_total: u64,
    pub completed_files: AtomicU32,
    /// Immutable after creation: `diff.need.len()`.
    pub total_files: u32,
    /// Immutable after creation: `diff.remove.len()`.
    pub removing: u32,
    /// Uploads that have started and not yet finished, with their sizes.
    ///
    /// At most `CONCURRENCY` (20) entries, so the linear scans below cost
    /// nothing at the 4 Hz they are read. A `std::sync::Mutex` deliberately:
    /// every operation is a push/retain/max over a 20-element vector and the
    /// guard is NEVER held across an `.await`.
    in_flight: std::sync::Mutex<Vec<(String, u64)>>,
}

impl UploadProgressState {
    pub fn new(bytes_total: u64, total_files: u32, removing: u32) -> Self {
        Self {
            bytes_uploaded: AtomicU64::new(0),
            bytes_total,
            completed_files: AtomicU32::new(0),
            total_files,
            removing,
            in_flight: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// An upload task has started work on `path`.
    pub fn begin_file(&self, path: &str, size: u64) {
        self.lock_in_flight().push((path.to_string(), size));
    }

    /// An upload task has finished `path` — drop it from the in-flight set and
    /// count it done. Both halves together, because a file that left the set
    /// without incrementing the counter (or vice versa) is a progress bar that
    /// disagrees with itself.
    pub fn finish_file(&self, path: &str) {
        // `position` + `swap_remove`, not `retain`: a path can legitimately
        // appear twice only if the same file were queued twice, and removing
        // exactly one occurrence per completion keeps the two in step even then.
        let mut guard = self.lock_in_flight();
        if let Some(i) = guard.iter().position(|(p, _)| p == path) {
            guard.swap_remove(i);
        }
        drop(guard);
        self.completed_files
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// The largest upload in flight, or `None` when nothing is.
    ///
    /// Ties break toward the earlier entry, which is stable enough for a label
    /// that is re-read four times a second.
    pub fn largest_in_flight(&self) -> Option<String> {
        self.lock_in_flight()
            .iter()
            .max_by_key(|(_, size)| *size)
            .map(|(path, _)| path.clone())
    }

    /// A poisoned lock here means a task panicked mid-push. The vector is a
    /// display hint, so recovering the guard is strictly better than taking the
    /// whole publish down over a filename.
    fn lock_in_flight(&self) -> std::sync::MutexGuard<'_, Vec<(String, u64)>> {
        self.in_flight.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time proof the `specta::Type` derive is present on both the
    /// deploy stage stream and its envelope (Step 3 Phase 3: they become typed
    /// bindings feeding `nextPublishStage`, not raw JSON).
    #[test]
    fn deploy_progress_is_specta_typed() {
        fn assert_type<T: specta::Type>() {}
        assert_type::<DeployProgress>();
        assert_type::<DeployStage>();
    }

    /// Verify JSON shape contains all expected fields.
    #[test]
    fn test_deploy_progress_json_shape() {
        let progress = DeployProgress {
            stage: DeployStage::Uploading,
            current: 3,
            total: 10,
            message: "Uploading file 3 of 10".to_string(),
            bytes_uploaded: None,
            bytes_total: None,
            removing: None,
            current_file: None,
        };

        let json = serde_json::to_value(&progress).unwrap();
        assert_eq!(json["stage"], "uploading");
        assert_eq!(json["current"], 3);
        assert_eq!(json["total"], 10);
        assert_eq!(json["message"], "Uploading file 3 of 10");
    }

    /// Byte fields are omitted from JSON when None (non-upload stages), and
    /// present when set (Uploading) so the frontend can drive the hairline.
    #[test]
    fn byte_fields_omitted_when_none_and_present_when_set() {
        let none = DeployProgress {
            stage: DeployStage::Syncing,
            current: 0,
            total: 10,
            message: "Comparing".into(),
            bytes_uploaded: None,
            bytes_total: None,
            removing: None,
            current_file: None,
        };
        let v = serde_json::to_value(&none).unwrap();
        assert!(
            v.get("bytes_uploaded").is_none(),
            "None byte field must be omitted from JSON"
        );

        let some = DeployProgress {
            stage: DeployStage::Uploading,
            current: 3,
            total: 10,
            message: "Uploading".into(),
            bytes_uploaded: Some(1024.0),
            bytes_total: Some(4096.0),
            removing: Some(2),
            current_file: Some("assets/video/a.mp4".into()),
        };
        let v = serde_json::to_value(&some).unwrap();
        assert_eq!(v["bytes_uploaded"], 1024.0);
        assert_eq!(v["bytes_total"], 4096.0);
    }

    /// `UploadProgressState` accumulates bytes (per-chunk / per-file) and the
    /// completed-file counter independently.
    #[test]
    fn upload_state_accumulates_bytes_and_files() {
        use std::sync::atomic::Ordering;
        let st = UploadProgressState::new(4096, 10, 0);
        st.bytes_uploaded.fetch_add(1024, Ordering::Relaxed);
        st.bytes_uploaded.fetch_add(2048, Ordering::Relaxed);
        st.begin_file("a.html", 10);
        st.finish_file("a.html");
        assert_eq!(st.bytes_uploaded.load(Ordering::Relaxed), 3072);
        assert_eq!(st.completed_files.load(Ordering::Relaxed), 1);
    }

    /// "The current file" names the upload actually costing the author time —
    /// not whichever of the twenty in flight happened to start last.
    #[test]
    fn largest_in_flight_names_the_file_worth_waiting_for() {
        let st = UploadProgressState::new(60_000_000, 3, 0);
        assert_eq!(st.largest_in_flight(), None, "nothing started yet");

        st.begin_file("assets/css/site.css", 6_000);
        st.begin_file("assets/video/tour.mp4", 40_000_000);
        st.begin_file("index.html", 12_000);
        assert_eq!(st.largest_in_flight().as_deref(), Some("assets/video/tour.mp4"));

        // The small files finish while the video is still going: the label must
        // not start flicking between them.
        st.finish_file("assets/css/site.css");
        st.finish_file("index.html");
        assert_eq!(st.largest_in_flight().as_deref(), Some("assets/video/tour.mp4"));

        st.finish_file("assets/video/tour.mp4");
        assert_eq!(st.largest_in_flight(), None);
        assert_eq!(st.completed_files.load(std::sync::atomic::Ordering::Relaxed), 3);
    }

    /// A symlink admits 0 bytes, so it can never win the slot away from real
    /// content — but it must still count as a completed file.
    #[test]
    fn a_zero_byte_entry_still_completes() {
        let st = UploadProgressState::new(0, 1, 0);
        st.begin_file("link", 0);
        assert_eq!(st.largest_in_flight().as_deref(), Some("link"));
        st.finish_file("link");
        assert_eq!(st.completed_files.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    /// Each variant serializes to its snake_case string.
    #[test]
    fn test_deploy_stage_variants_serialize() {
        let cases = vec![
            (DeployStage::Preparing, "preparing"),
            (DeployStage::Rebuilding, "rebuilding"),
            (DeployStage::Syncing, "syncing"),
            (DeployStage::Uploading, "uploading"),
            (DeployStage::Committing, "committing"),
            (DeployStage::Finalizing, "finalizing"),
            (DeployStage::Verifying, "verifying"),
            (DeployStage::Complete, "complete"),
            (DeployStage::Failed, "failed"),
        ];

        for (variant, expected) in cases {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, format!("\"{}\"", expected));
        }
    }

    /// The silent sink is a real sink: a publish with nobody watching runs the
    /// same code path as one with a window, it just drops what it says.
    #[test]
    fn the_silent_sink_swallows_a_stage() {
        silent().stage(DeployStage::Syncing, 0, 5, "Starting sync");
    }

    /// `stage` leaves the upload-only fields empty and `upload_sample` fills
    /// them from the shared accounting — the two shapes a sink ever receives.
    #[test]
    fn a_sink_sees_stages_and_upload_samples() {
        let sink = RecordingSink::default();
        sink.stage(DeployStage::Syncing, 0, 0, "Comparing with server...");

        let st = UploadProgressState::new(4096, 2, 1);
        st.begin_file("assets/video/tour.mp4", 4096);
        st.bytes_uploaded.store(1024, std::sync::atomic::Ordering::Relaxed);
        sink.upload_sample(&st);

        let events = sink.take();
        assert_eq!(events[0].stage, DeployStage::Syncing);
        assert_eq!(events[0].bytes_total, None, "a stage carries no byte counts");
        assert_eq!(events[1].stage, DeployStage::Uploading);
        assert_eq!(events[1].bytes_uploaded, Some(1024.0));
        assert_eq!(events[1].removing, Some(1));
        assert_eq!(events[1].current_file.as_deref(), Some("assets/video/tour.mp4"));
    }
}
