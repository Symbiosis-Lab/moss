//! The event carrier: `GET /__moss/events`, a Server-Sent Events stream of the
//! same typed bus the desktop frontend receives over Tauri IPC. Beside it,
//! `GET /__moss/reload` tells a page in a plain browser tab that the site was
//! rebuilt, and nothing more ([`handle_reload`]).
//!
//! ## Why this exists
//!
//! Outside the desktop shell there is no Tauri bus, so without this stream every
//! subscriber would be silently dead — no rebuild notice, no task toast, no
//! failure report — and a subscription that never fires looks exactly like a
//! quiet system.
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
//! `/__moss/events` is token-gated like `/__moss/read`: these events describe the vault (paths,
//! build results, task failures). The route is `GET`, so the browser client is
//! `fetch` + a `ReadableStream` reader rather than `EventSource` — `EventSource`
//! cannot set a request header, and putting the token in the query string
//! would write it into every access log.
//!
//! ## Viewer activity
//!
//! [`viewer_activity`] is the server's answer to "is anyone looking": a page
//! navigation, and an event stream opening or closing, each update it. A host
//! that pauses background work while nobody watches can wake on it instead of
//! polling. It lives here because the open streams are this module's
//! receivers, and it is a process global for the same reason the bus is.

use std::sync::{Arc, OnceLock};

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{Stream, StreamExt};
use tokio::sync::{broadcast, watch};

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

/// Marked each time rebuilt bytes become servable. It carries nothing, so the
/// reload stream can be read by a page, which holds no token.
pub(crate) fn reloads() -> &'static watch::Sender<()> {
    static RELOADS: OnceLock<watch::Sender<()>> = OnceLock::new();
    RELOADS.get_or_init(|| watch::channel(()).0)
}

/// Open event and reload streams together.
fn open_streams() -> usize {
    bus().receiver_count() + reloads().receiver_count()
}

/// What the preview server has seen of its viewers. Only activity is
/// reported; how long it keeps the host awake is the host's decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewerActivity {
    /// The last page navigation, or event stream opening or closing. `None`
    /// until the first.
    pub last_activity: Option<std::time::Instant>,
    /// Browsers holding `GET /__moss/events` or `/__moss/reload` open right now. One that went
    /// away still counts until its stream fails a write, at the next event or
    /// keep-alive (about 15 s).
    pub streams: usize,
}

static ACTIVITY: OnceLock<watch::Sender<ViewerActivity>> = OnceLock::new();

/// Held by every test that opens an event stream or navigates to a page:
/// both move the one process-global viewer-activity signal, so what a test
/// reads of it is only meaningful while no sibling can move it.
#[cfg(test)]
pub(crate) fn viewer_signal_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Watch the preview's viewers. The first call installs the signal; before
/// it, a request skips it after one atomic load, so a process that never asks
/// pays nothing for it.
pub fn viewer_activity() -> watch::Receiver<ViewerActivity> {
    ACTIVITY
        .get_or_init(|| watch::channel(ViewerActivity { last_activity: None, streams: open_streams() }).0)
        .subscribe()
}

/// Record that someone is looking, now.
pub(crate) fn note_activity() {
    if let Some(tx) = ACTIVITY.get() {
        let streams = open_streams();
        tx.send_modify(|a| {
            a.last_activity = Some(std::time::Instant::now());
            a.streams = streams;
        });
    }
}

/// A page navigation means someone is looking; one the shell itself asked for
/// and got served also tells the derive scheduler which page is on screen, so
/// reads of the files it embeds start first. A nested frame carries no shell
/// marker, an asset is no navigation, and a failed request shows no page.
pub(super) async fn track_viewer(
    site_dir: std::sync::Arc<std::sync::RwLock<std::path::PathBuf>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let navigation = super::content_wrapper::is_navigation_request(request.headers());
    if navigation {
        note_activity();
    }
    let shell = navigation && super::iframe_bridge::has_shell_marker(request.uri().query());
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    let html = response.status().is_success()
        && response.headers().get(axum::http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("text/html"));
    if shell && html {
        if let Some(folder) = site_dir.read().ok().and_then(|dir| crate::vault_root::VaultRoot::find_containing(&dir)) {
            let url = urlencoding::decode(&path).map_or(path.clone(), |p| p.into_owned());
            crate::system::derive::priority::note_viewed(folder.path(), &url);
        }
    }
    response
}

/// One open stream's receiver, of events or of reloads. Opening it and
/// dropping it — however the stream ends — are both viewer activity.
struct Subscription<R>(Option<R>);

impl<R> Subscription<R> {
    fn open(rx: R) -> Self {
        note_activity();
        Self(Some(rx))
    }

    fn rx(&mut self) -> &mut R {
        self.0.as_mut().expect("taken only by drop")
    }
}

impl<R> Drop for Subscription<R> {
    fn drop(&mut self) {
        // Leave the bus first, so the count reported no longer has this stream.
        self.0 = None;
        note_activity();
    }
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
    if matches!(event, MossEvent::FileChanged(_)) {
        reloads().send_replace(());
    }
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
    sub: Subscription<broadcast::Receiver<String>>,
    browser: Option<super::ui::auth::BrowserSession>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    futures::stream::unfold((sub, browser), |(mut sub, browser)| async move {
        loop {
            if browser.as_ref().is_some_and(|b| !b.admits()) { return None; }
            let received = tokio::select! {
                result = sub.rx().recv() => result,
                _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => continue,
            };
            match received {
                Ok(json) => {
                    if browser.as_ref().is_some_and(|b| !b.admits()) { return None; }
                    return Some((Ok(Event::default().data(json)), (sub, browser)));
                },
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!(target: "preview", "carrier: SSE subscriber lagged, {n} events skipped");
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

/// `GET /__moss/reload`: one constant frame each time rebuilt bytes become
/// servable, for a page open in a plain browser tab, which reloads on it.
///
/// Token-free, unlike `/__moss/events`: the frame names no path, page or
/// result, so it tells a page only that a rebuild happened. A frame missed
/// while disconnected is not replayed; the page reloads when it reconnects.
pub(crate) async fn handle_reload() -> impl axum::response::IntoResponse {
    let stream = futures::stream::unfold(Subscription::open(reloads().subscribe()), |mut sub| async move {
        sub.rx().changed().await.ok()?;
        Some((Ok::<_, std::convert::Infallible>(Event::default().data("reload")), sub))
    });
    ([("x-accel-buffering", "no")], Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// `GET /__moss/events`. The token gate is a `route_layer` in `router.rs`; by
/// the time this runs the caller is authenticated and same-origin.
///
/// The stream ends when the session it was admitted under is retired by a
/// folder switch: these events describe the vault, and a subscriber holding
/// the previous vault's token must not keep reading the next one's. The
/// browser's reconnect then re-presents its token and is 401. Browser sessions
/// are also rechecked before each event and every 30 seconds while idle;
/// those checks do not count as activity or extend the session's lifetime.
pub(crate) async fn handle_events(
    axum::Extension(session): axum::Extension<Arc<super::invoke::Session>>,
    browser: Option<axum::Extension<super::ui::auth::BrowserSession>>,
) -> impl axum::response::IntoResponse {
    // Subscribe BEFORE returning, so an event emitted between the request
    // arriving and the stream being polled is buffered rather than missed.
    // `setup-panel.ts` depends on exactly this ordering on the Tauri side.
    let stream = event_stream(Subscription::open(bus().subscribe()), browser.map(|b| b.0)).take_until(session.retired().clone().cancelled_owned());
    // A reverse proxy that buffers responses (nginx by default) would hold events back.
    ([("x-accel-buffering", "no")], Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Printed when a served build's `current` has caught up with the preview.
pub(crate) fn promoted_line(generation_id: &str) -> String {
    format!("current updated to generation {generation_id}")
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
pub struct CarrierAnnouncer {
    pub(crate) reporter: Arc<dyn crate::build::ports::reporter::BuildReporter>,
}

#[async_trait::async_trait]
impl crate::build::ports::announcer::SealAnnouncer for CarrierAnnouncer {
    fn promoted(&self, generation_id: &str) {
        log::info!("promoted generation {generation_id} to current");
        crate::cli_eprintln!("{}", promoted_line(generation_id));
        publish(&MossEvent::SitePromoted(crate::build::progress::SitePromoted {
            generation_id: generation_id.to_string(),
        }));
    }

    async fn adopt_sealed(&self, _sealed: crate::build::manifest::SealedManifest) {}

    async fn publish_change_set(
        &self,
        project_root: &std::path::Path,
        change_set: Option<crate::build::manifest::change_set::ChangeSet>,
    ) {
        // The removed addresses reach a terminal as an advisory; the rest of
        // the set has no headless listener.
        if let Some(cs) = change_set {
            static ANNOUNCED: std::sync::Mutex<Option<std::collections::HashSet<std::path::PathBuf>>> =
                std::sync::Mutex::new(None);
            let mut announced = ANNOUNCED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let announced = announced.get_or_insert_with(Default::default);
            if let Some(event) = removal_announcement(announced, project_root, &cs.removed) {
                self.reporter.report(&event);
            }
        }
    }
}

/// The event to publish for this build's removed addresses, or `None`. A
/// non-empty list is announced every build; an empty one says nothing, except
/// the one clearing event after a build that had announced a list, so an
/// advisory a watching browser shows goes away once the author fixes it.
/// `announced` is the folders whose last announcement was non-empty. A folder
/// that never had removals puts nothing on the shared event bus.
fn removal_announcement(
    announced: &mut std::collections::HashSet<std::path::PathBuf>,
    folder: &std::path::Path,
    removed: &[crate::build::manifest::change_set::RemovedAddress],
) -> Option<crate::build::progress::PipelineEvent> {
    let accepted = crate::system::build_records::records().accepted_removals(&folder.to_string_lossy());
    let unaccepted: Vec<_> = removed.iter().filter(|r| !accepted.contains(&r.path)).cloned().collect();
    let event = crate::build::progress::make_removed_addresses_advisory(&unaccepted);
    if shows_on_terminal(&event) {
        announced.insert(folder.to_path_buf());
        Some(event)
    } else {
        announced.remove(folder).then_some(event)
    }
}

/// Does a headless build print this event on the terminal?
///
/// The plugin events are its console voice (see [`CarrierReporter`]), and so
/// is the removed-addresses advisory: it is how a `moss build` user learns a
/// publish would take an address offline. Other advisories stay with the
/// carrier's listeners.
pub fn shows_on_terminal(event: &crate::build::progress::PipelineEvent) -> bool {
    use crate::build::progress::PipelineEvent;
    matches!(event, PipelineEvent::PluginProgress(_) | PipelineEvent::PluginNeedsConnection { .. })
        || matches!(event, PipelineEvent::BackgroundProgress { task, advisories, .. }
            if task == "derive" || task.starts_with("plugin-offer-") || (task == "addresses" && !advisories.is_empty()))
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
        if shows_on_terminal(event) {
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

#[cfg(test)]
pub(crate) fn collect_removal_warnings() -> (impl Fn(&str) -> crate::build::HostPorts, impl FnMut() -> Vec<String>) {
    #[derive(Default)]
    struct Warnings(std::sync::Mutex<Vec<String>>);

    impl crate::build::ports::reporter::BuildReporter for Warnings {
        fn report(&self, event: &crate::build::progress::PipelineEvent) {
            if let crate::build::progress::PipelineEvent::BackgroundProgress { task, advisories, .. } = event {
                if task == "addresses" {
                    self.0.lock().unwrap().extend(advisories.iter().map(|a| a.what.clone()));
                }
            }
            CarrierReporter.report(event);
        }
    }

    let warnings = Arc::new(Warnings::default());
    let reporter = warnings.clone();
    let host = move |folder: &str| {
        let mut host = crate::cli::host::cli_host_ports(folder);
        host.announcer = Arc::new(CarrierAnnouncer { reporter: reporter.clone() });
        host
    };
    (host, move || std::mem::take(&mut *warnings.0.lock().unwrap()))
}

#[cfg(test)]
mod removal_announcement_tests {
    use super::removal_announcement;
    use crate::build::manifest::change_set::{RemovalReason, RemovedAddress};
    use crate::build::progress::PipelineEvent;
    use std::collections::HashSet;
    use std::path::Path;

    fn lost() -> Vec<RemovedAddress> {
        vec![RemovedAddress {
            path: "feed.xml".into(),
            reason: RemovalReason::Unexplained,
            moved_to: None,
            source: None,
            withdrawn_by: None,
        }]
    }

    fn advisories(event: PipelineEvent) -> usize {
        match event {
            PipelineEvent::BackgroundProgress { advisories, .. } => advisories.len(),
            other => panic!("expected a background tick, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn removal_warning_collectors_are_isolated() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let (first_host, mut first_warnings) = super::collect_removal_warnings();
        let (second_host, mut second_warnings) = super::collect_removal_warnings();
        let first_host = first_host(&first.path().to_string_lossy());
        let second_host = second_host(&second.path().to_string_lossy());
        let changes = crate::build::manifest::change_set::ChangeSet { removed: lost(), ..Default::default() };

        first_host.announcer.publish_change_set(first.path(), Some(changes.clone())).await;
        assert!(first_warnings().iter().any(|w| w.contains("/feed.xml")));
        assert!(second_warnings().is_empty(), "another test's advisories must not leak in");
        second_host.announcer.publish_change_set(second.path(), Some(changes)).await;
        assert!(second_warnings().iter().any(|w| w.contains("/feed.xml")));
        assert!(first_warnings().is_empty(), "another test's advisories must not leak in");
    }

    #[test]
    fn accepted_removals_are_quiet_without_hiding_unaccepted_addresses() {
        let folder = tempfile::tempdir().unwrap();
        let key = folder.path().to_string_lossy();
        let records = crate::system::build_records::records();
        let mut removed = lost();
        let mut other = removed[0].clone();
        other.path = "other.xml".into();
        removed.push(other);
        records.record_removed_addresses(&key, removed.clone());
        records.accept_unexplained_removals(&key, &["feed.xml".into()]);
        let mut announced = HashSet::new();
        let event = removal_announcement(&mut announced, folder.path(), &removed).unwrap();
        let PipelineEvent::BackgroundProgress { advisories: notices, .. } = event else { panic!("expected advisory") };
        assert_eq!(notices.len(), 1);
        assert!(notices[0].what.contains("/other.xml"));
        assert!(!notices[0].what.contains("/feed.xml"), "accepted removals must not ask for action");
        records.accept_all_pending_removals(&key);
        assert_eq!(advisories(removal_announcement(&mut announced, folder.path(), &removed).unwrap()), 0);
        assert_eq!(records.removed_addresses(&key).unwrap().len(), 2);
        assert!(removal_announcement(&mut announced, folder.path(), &removed).is_none());
    }

    /// A folder that never had removals puts nothing on the shared bus.
    #[test]
    fn a_build_that_never_had_removals_emits_nothing() {
        let mut announced = HashSet::new();
        assert!(removal_announcement(&mut announced, Path::new("/site"), &[]).is_none());
        assert!(removal_announcement(&mut announced, Path::new("/site"), &[]).is_none());
    }

    /// Under `--serve --watch` the advisory a browser shows must clear once
    /// the author fixes it: one clearing event after a non-empty list, then
    /// silence again.
    #[test]
    fn the_build_after_a_removal_is_fixed_emits_one_clearing_event() {
        let mut announced = HashSet::new();
        let shown = removal_announcement(&mut announced, Path::new("/site"), &lost()).expect("announced");
        assert_eq!(advisories(shown), 1);

        let cleared = removal_announcement(&mut announced, Path::new("/site"), &[]).expect("a clearing event");
        assert_eq!(advisories(cleared), 0);
        assert!(removal_announcement(&mut announced, Path::new("/site"), &[]).is_none());
    }

    /// Folders are independent: another folder's fix clears nothing here.
    #[test]
    fn the_clearing_event_is_per_folder() {
        let mut announced = HashSet::new();
        removal_announcement(&mut announced, Path::new("/a"), &lost());
        assert!(removal_announcement(&mut announced, Path::new("/b"), &[]).is_none());
    }
}

#[cfg(test)]
mod shows_on_terminal_tests {
    use super::shows_on_terminal;
    use crate::build::progress::PipelineEvent;

    fn tick(task: &str) -> PipelineEvent {
        PipelineEvent::BackgroundProgress {
            task: task.into(),
            current: 3,
            total: 12,
            message: "Reading PDFs \u{b7} 3 of 12".into(),
            completed: false,
            advisories: vec![],
        }
    }

    /// Under `--serve` a plugin deriving text from files is silent unless its
    /// progress reaches the terminal.
    #[test]
    fn a_derive_tick_shows_on_the_terminal() {
        assert!(shows_on_terminal(&tick("derive")));
    }

    #[test]
    fn an_addresses_tick_without_advisories_stays_off_the_terminal() {
        assert!(!shows_on_terminal(&tick("addresses")));
    }

    #[test]
    fn other_background_tasks_stay_off_the_terminal() {
        assert!(!shows_on_terminal(&tick("images")));
    }
    /// A headless build is the only place a CLI user hears that a plugin has
    /// work waiting for them to start.
    #[test]
    fn a_plugin_offer_shows_on_the_terminal() {
        assert!(shows_on_terminal(&tick("plugin-offer-matters")));
    }
}

#[cfg(test)]
mod promoted_line_tests {
    use super::promoted_line;
    use crate::ops::serve::ownership::generated_line;
    use std::path::Path;

    #[test]
    fn a_served_build_does_not_claim_current_was_written() {
        let line = generated_line("s", Path::new("/s/.moss/build.nosync/current"), Path::new("/s/.moss/build.nosync/staging"), true);
        assert!(line.contains("serves /s/.moss/build.nosync/staging"), "{line}");
        assert!(line.contains("current follows after 20 s without edits"), "{line}");
        assert!(!line.contains("generated at"), "{line}");
    }

    #[test]
    fn a_one_shot_build_names_current() {
        let line = generated_line("s", Path::new("/s/current"), Path::new("/s/staging"), false);
        assert_eq!(line, "📁 's': Site generated at /s/current");
    }

    #[test]
    fn promotion_names_its_generation() {
        assert_eq!(promoted_line("g1"), "current updated to generation g1");
    }
}

#[cfg(test)]
mod source_content_tests {
    use super::*;

    #[test]
    fn source_content_changes_keep_their_wire_shape_on_the_carrier() {
        let mut receiver = bus().subscribe();
        let payload = serde_json::json!({
            "kind": "SourceContentChanged",
            "payload": { "paths": ["notes/page.md", ".moss/config.toml"] },
        });
        let event = MossEvent::SourceContentChanged {
            paths: vec!["notes/page.md".into(), ".moss/config.toml".into()],
        };
        publish(&event);
        let expected = serde_json::json!({ "name": MOSS_EVENT_CHANNEL, "payload": payload });
        while let Ok(frame) = receiver.try_recv() {
            let actual: serde_json::Value = serde_json::from_str(&frame).unwrap();
            if actual == expected { return; }
        }
        panic!("the carrier must deliver the source event without build or output fields");
    }
}
