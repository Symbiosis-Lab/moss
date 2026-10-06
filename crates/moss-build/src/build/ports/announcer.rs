//! The seal tail's announcer: who is told that a build became the site.
//!
//! An earlier design decision (2026-08-24 extension): the post-seal tail is one function and
//! takes no `AppHandle`; the observer differences between the desktop app, a
//! headless serve, and a one-shot build cross the seam through ports. Most of
//! what the tail says is already `PipelineEvent`-shaped and rides
//! [`super::reporter::BuildReporter`] — the "Sealed" progress tick and
//! `AssetsSettled` needed no new surface at all. What is left is exactly three
//! things, and they are this trait:
//!
//! - a **promotion** is announced from inside the tail, immediately after the
//!   `current` swap, so nothing between the swap and the announcement can fail
//!   and swallow it (the GC and stale-clean steps run after);
//! - the sealed **manifest** is handed to whoever holds it for deploy;
//! - the publish **change set** is stashed-and-pushed under the
//!   obeys-the-stash contract (`AppState::apply_change_set`'s doc), which is a
//!   read-model update plus a conditional emit, not a broadcast.
//!
//! The last two are state handoffs rather than events, which is why they are
//! not a fold onto `BuildReporter`: a reporter that adopted manifests would be
//! exactly the grab-bag the abort-threshold rule exists to catch. This trait moving ratchet
//! row (o) is that threshold doing its job — the raise is logged in the
//! baseline's `accepts[]` with this module as the reason.
//!
//! Implementations: `events::AppAnnouncer` (webviews + `AppState`),
//! [`LogAnnouncer`] (one-shot `moss build`); that same design decision names a
//! carrier-shaped one for headless watch, which lands with its caller.

use std::path::Path;

use crate::build::manifest::change_set::ChangeSet;
use crate::build::manifest::SealedManifest;

#[async_trait::async_trait]
pub trait SealAnnouncer: Send + Sync {
    /// `current` now points at this generation. Called only for a real swap —
    /// `Superseded`, `Withheld` and a failed materialize all leave `current`
    /// where it was, and none of them may claim otherwise.
    fn promoted(&self, generation_id: &str);

    /// Hand the sealed manifest to whoever holds it for deploy. Called only
    /// when the generation materialized on disk (`mat_ok`); an implementation
    /// with no in-memory holder simply drops it — deploy then reads
    /// `hashes.json`, which the tail persisted before this call.
    async fn adopt_sealed(&self, sealed: SealedManifest);

    /// Stash what publishing this generation would change and push it to
    /// listeners. `None` means this tail has nothing to say (a superseded or
    /// withheld seal), and the standing stash must be left alone — see
    /// `AppState::apply_change_set` for the pair contract this preserves.
    async fn publish_change_set(&self, project_root: &Path, change_set: Option<ChangeSet>);
}

/// The one-shot announcer: log lines. The process exits when the build
/// returns, so there is nobody to hand state to — deploy in a later process
/// reads `hashes.json`, which the tail persisted before `adopt_sealed`, and
/// no webview exists to hear a change set.
pub struct LogAnnouncer;

#[async_trait::async_trait]
impl SealAnnouncer for LogAnnouncer {
    fn promoted(&self, generation_id: &str) {
        log::info!("promoted generation {generation_id} to current");
    }

    async fn adopt_sealed(&self, _sealed: SealedManifest) {}

    async fn publish_change_set(&self, _project_root: &Path, change_set: Option<ChangeSet>) {
        if let Some(cs) = change_set {
            log::debug!(target: "publish", "publish change set: {cs:?}");
        }
    }
}
