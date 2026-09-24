//! The event carrier: `GET /__moss/events`, a Server-Sent Events stream of the
//! same typed bus the desktop frontend receives over Tauri IPC.
//!
//! ## Why this exists
//!
//! Commands were only half the seam. The desktop app's platform listener had a
//! working Tauri branch and a documented no-op browser branch, so outside the
//! desktop shell every subscriber was silently dead — no rebuild notice, no
//! task toast, no failure report. Silently, because a subscription that never
//! fires looks exactly like a quiet system.
//!
//! ## One publisher per carrier, by construction
//!
//! A `MossEvent` reaches this bus from exactly one of two places, and which one
//! is decided by `events::tier2_reporter`'s `match app`:
//!
//!   * **Desktop** (`app: Some`) — `events::emit_moss_event`, which every
//!     desktop `MossEvent` already goes through, publishes here as well as to
//!     the Tauri bus.
//!   * **Headless** (`app: None`) — [`CarrierReporter`] replaces
//!     `HeadlessReporter`, translating each `PipelineEvent` with
//!     `events::moss_event_for` and publishing that.
//!
//! The two are mutually exclusive because `app: None` means there is no Tauri
//! bus to emit onto, so nothing can double-publish. That is a property of the
//! one `match`, not a rule someone has to remember.
//!
//! ## Why a process global
//!
//! One process serves one preview server, and the producers — the build
//! pipeline's reporter, the watcher's relay, a plugin's progress — are spread
//! across code that holds no path to the server's state. Threading a sender to
//! each would be a wide change for a value there is only ever one of.
//!
//! ## The frame
//!
//! `data: {"name":"moss-event","payload":<MossEvent>}` — the name is carried in
//! the frame rather than in SSE's own `event:` field so one stream can serve
//! every event name `listen()` takes, and `listen.ts` demultiplexes by it
//! exactly as Tauri's bus does.
//!
//! ## Auth
//!
//! Token-gated like `/__moss/read`: these events describe the vault (paths,
//! build results, task failures). The route is `GET`, so the browser client is
//! `fetch` + a `ReadableStream` reader rather than `EventSource` — `EventSource`
//! cannot set a request header, and putting the token in the query string
//! would write it into every access log.

use std::sync::{Arc, OnceLock};

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{Stream, StreamExt};
use tokio::sync::broadcast;

use crate::types::events::{MossEvent, MOSS_EVENT_CHANNEL};

/// Frames buffered per subscriber before it is considered lagged.
///
/// A build emits a few hundred `AssetReady`s in a burst, so a small buffer
/// would drop frames on a browser that blocked for a moment on layout. A
/// lagged subscriber is not disconnected — see [`event_stream`].
const CAPACITY: usize = 512;

fn bus() -> &'static broadcast::Sender<String> {
    static BUS: OnceLock<broadcast::Sender<String>> = OnceLock::new();
    BUS.get_or_init(|| broadcast::channel(CAPACITY).0)
}

/// Publish one named event. Serialization happens once here, not once per
/// subscriber.
///
/// Does nothing useful when no browser is listening (`send` returns `Err` with
/// zero receivers), which is the normal case in the desktop app — deliberately
/// silent rather than logged, since it is not a failure.
pub fn publish_named(name: &str, payload: serde_json::Value) {
    let frame = serde_json::json!({ "name": name, "payload": payload });
    match serde_json::to_string(&frame) {
        Ok(json) => {
            let _ = bus().send(json);
        }
        // A `MossEvent` that will not serialize would also have failed to reach
        // the desktop frontend, so this is a real defect rather than a carrier
        // problem — say so once instead of dropping it silently.
        Err(e) => log::error!(target: "preview", "carrier: event '{name}' is not serializable: {e}"),
    }
}

/// Publish a `MossEvent` on the same channel name the Tauri bus uses.
pub fn publish(event: &MossEvent) {
    match serde_json::to_value(event) {
        Ok(payload) => publish_named(MOSS_EVENT_CHANNEL, payload),
        Err(e) => log::error!(target: "preview", "carrier: MossEvent is not serializable: {e}"),
    }
}

/// The SSE stream handed to one browser.
///
/// A subscriber that falls behind by more than [`CAPACITY`] frames receives
/// `Lagged` and is **kept**, not dropped: the events are advisory UI news, and
/// disconnecting a slow tab would turn a hiccup into a permanently dead
/// subscription. The gap is logged so it is not invisible.
fn event_stream(
    rx: broadcast::Receiver<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(json) => return Some((Ok(Event::default().data(json)), rx)),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!(target: "preview", "carrier: SSE subscriber lagged, {n} events skipped");
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

/// `GET /__moss/events`. The token gate is a `route_layer` in `router.rs`; by
/// the time this runs the caller is authenticated and same-origin.
///
/// The stream ends when the session it was admitted under is retired by a
/// folder switch: these events describe the vault, and a subscriber holding
/// the previous vault's token must not keep reading the next one's. The
/// browser's reconnect then re-presents its token and is 401.
pub async fn handle_events(
    axum::Extension(session): axum::Extension<Arc<super::invoke::Session>>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    // Subscribe BEFORE returning, so an event emitted between the request
    // arriving and the stream being polled is buffered rather than missed.
    // `setup-panel.ts` depends on exactly this ordering on the Tauri side.
    let stream = event_stream(bus().subscribe()).take_until(session.retired().clone().cancelled_owned());
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// The `BuildReporter` for a build with no Tauri shell: publish to the carrier,
/// and otherwise behave as `HeadlessReporter` did.
///
/// `is_terminal()` stays `true` — it answers "is the terminal the surface",
/// which a browser watching the SSE stream does not change. The CLI's coarse
/// stderr progress is still the thing a person running `moss build --serve`
/// reads.
/// The headless [`SealAnnouncer`](crate::build::ports::announcer::SealAnnouncer):
/// promotions go to the SSE carrier so an attached browser hears the same
/// `SitePromoted` a webview would. The two state handoffs have no holder in a
/// headless process — deploy in a later process reads `hashes.json`, and no
/// webview exists to pull a stashed change set — so both drop, like
/// [`LogAnnouncer`](crate::build::ports::announcer::LogAnnouncer).
pub struct CarrierAnnouncer;

#[async_trait::async_trait]
impl crate::build::ports::announcer::SealAnnouncer for CarrierAnnouncer {
    fn promoted(&self, generation_id: &str) {
        log::info!("promoted generation {generation_id} to current");
        publish(&MossEvent::SitePromoted(crate::build::progress::SitePromoted {
            generation_id: generation_id.to_string(),
        }));
    }

    async fn adopt_sealed(&self, _sealed: crate::build::manifest::SealedManifest) {}

    async fn publish_change_set(
        &self,
        _project_root: &std::path::Path,
        change_set: Option<crate::build::manifest::change_set::ChangeSet>,
    ) {
        if let Some(cs) = change_set {
            log::debug!(target: "publish", "publish change set: {cs:?}");
        }
    }
}

pub struct CarrierReporter;

impl crate::build::ports::reporter::BuildReporter for CarrierReporter {
    fn report(&self, event: &crate::build::progress::PipelineEvent) {
        use crate::build::progress::PipelineEvent;
        // The plugin manager reports through THIS reporter headless, and the
        // two plugin events are its only console voice: a plugin that cannot
        // run means content missing from the build, so the needs-connection
        // line must reach stderr and the `--strict` count — the outcome a
        // plugin-bearing `moss build` once hid behind "Build complete", exit 0.
        // `StdoutReporter` owns the wording; this is the one delegation.
        if matches!(event, PipelineEvent::PluginProgress(_) | PipelineEvent::PluginNeedsConnection { .. }) {
            crate::build::ports::reporter::StdoutReporter.report(event);
        }
        if let Some(m) = crate::types::events::moss_event_for(event) {
            publish(&m);
        }
    }

    fn is_terminal(&self) -> bool {
        true
    }
}
