//! What a publish needs from the process running it.
//!
//! [`super::progress::DeploySink`] carries what a publish has to *say*; this
//! carries what it has to *ask for*. Both exist for the same reason: the body
//! of a publish is the same work whoever runs it, and the differences between
//! the app and a terminal are a handful of values, not a fork in the code.
//!
//! Four methods, and each is here because the app answers it with *behaviour*
//! the open binary has no equivalent of — a generation pin held against a
//! concurrent build's GC, a change-set the frontend re-reads after a publish
//! lands, and the domain orchestrator. A headless publish has no concurrent
//! build and no frontend, so most of its implementation is honestly empty;
//! that emptiness is the point, and it is why they are methods rather than
//! `Option`s threaded down every signature.
//!
//! A *value* the process merely holds does not belong here, however
//! app-shaped it looks. The per-site event-sync lock was a method until the
//! C4d review: it took an argument the publish already carried and returned a
//! plain `Arc`, so it is resolved once at the entry point and carried on
//! `PushContext` beside the sink. The rule is `build::ports::host`'s: the host
//! resolves at the entry point, the body consumes and never asks.
//!
//! Beside [`super::host`] rather than next to the publish body it serves, for
//! one measured reason: ratchet row (o) `seam_surface` counts only `ports*`
//! paths, and the first draft of this file sat in `deploy/` where the counter
//! could not see it — a seam growing unobserved is the failure row (v)
//! `app_host_surface` was added after. The whole directory moves to the crate
//! root at M6a; it moves as one.
//!
//! Crossed here 2026-09-09 ahead of track C4d, which gives `moss deploy` a
//! headless route. Two implementors since: the app (`deploy::app_seam::AppPorts`)
//! and the empty one every windowless publish takes
//! (`deploy::one_shot::HeadlessDeployPorts`).

use std::path::Path;

/// The process-shaped half of a publish.
#[async_trait::async_trait]
pub trait DeployPorts: Send + Sync {
    /// Hold a build generation against garbage collection.
    ///
    /// Prefer [`pin_generation`], which releases on every exit path. This pair
    /// is the primitive the guard is built from, and calling it directly means
    /// owning the release yourself.
    fn pin_generation_raw(&self, generation_id: &str);

    /// Release a pin. Must be a no-op for a generation that is not pinned —
    /// the guard cannot know whether it is unwinding from a path that already
    /// released.
    fn unpin_generation(&self, generation_id: &str);

    /// Re-derive what is left to publish, now that something landed, and tell
    /// whoever is watching.
    ///
    /// A landed publish moves one of the change set's two inputs, so it owes
    /// the same re-derivation a seal does. Nobody is watching a terminal
    /// publish, so there the answer is computed by nobody and this does
    /// nothing.
    ///
    /// **Re-justified and kept at track P slice P3**, which is what the
    /// condition below was waiting for. Both callers were app-side when this
    /// was written, so it was then a bounce within one crate; the condition
    /// was that `landed::record_landed` cross with the publish body while
    /// `recompute_after_landing` still needed the app's read model. Both
    /// halves now hold and are checkable: `record_landed` lives in
    /// `moss_build::deploy::landed` and its only two callers are
    /// `deploy::push` and `deploy::plugin_push`, neither of which can name
    /// `AppState`, while the app's implementation classifies against
    /// `AppState::current_sealed_manifest` and emits a Tauri event. A headless
    /// publish answers it with an empty body because nobody is watching.
    ///
    /// `summary` is the completion-scoped Added/Moved/Removed page rows
    /// `landed::record_what_is_live` computed against what was live an
    /// instant before this publish overwrote it (publish-receipt design,
    /// step 4) — a second, independent answer from `recompute_after_landing`'s
    /// own `classify`, which diffs the *live* sealed manifest against the
    /// record to say what is left to publish next, not what this publish
    /// itself changed.
    async fn after_landing(&self, folder: &Path, target: &str, summary: &crate::deploy::change_record::PageChangeSummary);

    /// Bring the folder's custom domain to a usable state, if it has one.
    ///
    /// Called after the commit returns 200, so the URL a publish reports is the
    /// address the author actually publishes under rather than the mosspub
    /// subdomain. Best-effort by contract: a domain that cannot be readied
    /// costs the nicer URL, never the publish.
    async fn ensure_domain_ready(&self, folder_path: &str);

    /// Kick off the one-shot verification burst — does the origin now say
    /// this generation is live, and does the public address reach the site —
    /// without blocking the publish's own return.
    ///
    /// Design §4a's two probes, called from `push.rs` right after
    /// `record_landed` returns (task 4-6 moved this past landing: the page
    /// rows below only exist once landing has computed them). This crate
    /// cannot fold them into a verdict: `classify_moss_verification` and the
    /// `PublishVerdict` event it feeds live in the desktop app's stack-serving
    /// verification code, beside the
    /// OnionPress supervisor whose announce path they never enter. So an
    /// implementation with real work to do constructs its own seta client
    /// (mirroring `push.rs`'s), spawns the burst, and returns immediately;
    /// one with nothing to announce to does nothing.
    ///
    /// `summary` is the same completion-scoped Added/Moved/Removed rows
    /// [`after_landing`](DeployPorts::after_landing) receives — computed once
    /// by `landed::record_what_is_live` and threaded to both callers, never
    /// recomputed. No manifest entries ride along: the public half of the
    /// burst proves reachability, not byte identity, since an HTML-rewriting
    /// CDN in front of a site makes the served bytes differ from the
    /// published bytes on every request.
    async fn begin_moss_verification(
        &self,
        identity: &crate::identity::Identity,
        environment: crate::config::environment::HostingEnvironment,
        site_id: &str,
        folder_path: &str,
        generation_id: &str,
        summary: &crate::deploy::change_record::PageChangeSummary,
    );
}

/// Holds a generation pinned for as long as the value lives.
///
/// RAII because the release has to survive every way a publish can end —
/// normal return, `?`, panic, and the stall watchdog dropping the whole future
/// out from under it. The app had this guard already; it lives here now so an
/// implementation cannot ship without one, which is the failure a hand-placed
/// `unpin` invites.
pub struct GenerationPin<'a> {
    ports: &'a dyn DeployPorts,
    generation_id: String,
}

impl Drop for GenerationPin<'_> {
    fn drop(&mut self) {
        self.ports.unpin_generation(&self.generation_id);
    }
}

/// Pin a generation until the returned guard is dropped.
pub fn pin_generation<'a>(ports: &'a dyn DeployPorts, generation_id: &str) -> GenerationPin<'a> {
    ports.pin_generation_raw(generation_id);
    GenerationPin { ports, generation_id: generation_id.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records pin/unpin so the guard's contract can be read off it. The other
    /// two methods are never reached here and say so.
    #[derive(Default)]
    struct Recording {
        pinned: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl DeployPorts for Recording {
        fn pin_generation_raw(&self, generation_id: &str) {
            self.pinned.lock().unwrap().push(generation_id.to_string());
        }
        fn unpin_generation(&self, generation_id: &str) {
            self.pinned.lock().unwrap().retain(|g| g != generation_id);
        }
        async fn after_landing(&self, _folder: &Path, _target: &str, _summary: &crate::deploy::change_record::PageChangeSummary) {}
        async fn ensure_domain_ready(&self, _folder_path: &str) {}
        async fn begin_moss_verification(
            &self,
            _identity: &crate::identity::Identity,
            _environment: crate::config::environment::HostingEnvironment,
            _site_id: &str,
            _folder_path: &str,
            _generation_id: &str,
            _summary: &crate::deploy::change_record::PageChangeSummary,
        ) {
        }
    }

    /// The whole reason the guard exists: a publish that ends any way at all
    /// releases its pin. `panic` stands in for the two paths a test cannot
    /// easily stage — a `?` early return and the stall watchdog dropping the
    /// publish future — because all three are one mechanism, `Drop`.
    #[test]
    fn a_pin_is_released_however_the_publish_ends() {
        let ports = Recording::default();

        {
            let _pin = pin_generation(&ports, "gen042");
            assert_eq!(*ports.pinned.lock().unwrap(), vec!["gen042".to_string()]);
        }
        assert!(ports.pinned.lock().unwrap().is_empty(), "normal scope exit releases");

        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _pin = pin_generation(&ports, "gen043");
            panic!("publish blew up");
        }));
        assert!(unwound.is_err());
        assert!(ports.pinned.lock().unwrap().is_empty(), "unwinding releases too");
    }
}
