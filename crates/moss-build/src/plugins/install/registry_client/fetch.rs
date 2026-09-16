//! Fetching `index.json` and `revoked.json` from the pinned registry origin,
//! and the refresh that composes fetch → accept → cache.
//!
//! **The origin is a trust anchor, not a configuration detail.** The
//! documents are unsigned in v1, so whoever serves the bytes is trusted, and
//! exactly one GitHub-controlled origin serves them: no CDN mirrors, no
//! fallback origins, no user override. What authenticates them is TLS to that
//! origin plus the monotonic serials `super::index` enforces.
//!
//! The origin is a parameter only so unit tests can point at a local static
//! server. Production callers use [`refresh`], which hard-codes
//! [`REGISTRY_ORIGIN`] — and that constant has one door, closed at build
//! time: `MOSS_REGISTRY_ORIGIN`, for the e2e suite's fixture, which the
//! assertion under it confines to https or loopback.
//!
//! [`refresh`] blocks: two documents, each with a short leash, tried over two
//! transports. Callers on the app-launch path MUST run it off-thread — the
//! catalog is allowed to arrive late, the window is not.

use std::sync::Mutex;
use std::time::Duration;

use super::cache::{self, CachedRegistry};
use super::index::{accept_index, accept_revoked, RejectReason};
use crate::system::large_download::{plan_transport, Transport};

/// The one origin. See the module header and ADR-074 before changing it.
pub const PRODUCTION_ORIGIN: &str = "https://symbiosis-lab.org/moss-registry";

/// [`PRODUCTION_ORIGIN`], unless the binary was BUILT with
/// `MOSS_REGISTRY_ORIGIN` set — the e2e suite's static fixture, and nothing
/// else. Compile-time like `MOSS_SETA_URL`, for the same reason
/// (tauri-driver passes no runtime env to the app) and for a better one: a
/// runtime override would be the configurable origin ADR-074 rules out, and
/// a shipped binary built without the variable has no way to be pointed
/// anywhere else.
pub const REGISTRY_ORIGIN: &str = match option_env!("MOSS_REGISTRY_ORIGIN") {
    Some(origin) => origin,
    None => PRODUCTION_ORIGIN,
};

// With unsigned documents, TLS to the one pinned origin is the whole
// authentication story, so a binary built against an http:// origin has
// silently lost it. Checked on the value the binary fetches with, not the
// production literal, and at build time: a test would let a release built
// with the e2e variable in its environment ship green.
const _: () = assert!(
    has_prefix(REGISTRY_ORIGIN, "https://")
        || has_prefix(REGISTRY_ORIGIN, "http://127.0.0.1:")
        || has_prefix(REGISTRY_ORIGIN, "http://localhost:"),
    "MOSS_REGISTRY_ORIGIN must be https, or loopback for the e2e fixture"
);

const fn has_prefix(s: &str, prefix: &str) -> bool {
    let (s, p) = (s.as_bytes(), prefix.as_bytes());
    if p.len() > s.len() {
        return false;
    }
    let mut i = 0;
    while i < p.len() {
        if s[i] != p[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Both documents are a few tens of kilobytes and the refresh runs on the
/// app-launch path, so a slow registry must never hold the app: the fetch is
/// given a short leash and its failure is a cache hit, not an error.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Each document is tried over both transports before giving up. One dead
/// route must never consume every attempt: a stale PAC entry or a dead
/// split-tunnel proxy would otherwise make the registry permanently
/// unreachable, and for the kill list "permanently unreachable" means
/// revocation that never refreshes — the one thing this module must not let
/// degrade. Same reasoning, and the same `plan_transport`, as
/// `system::large_download`.
const MAX_ATTEMPTS: u32 = 2;

/// A ceiling on the bytes a refresh will read into memory. The live index is
/// ~2 KB; this is a backstop against an origin that answers a small JSON
/// request with an unbounded stream, not a real limit.
const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

/// An icon is a glyph, not a document. Kept far below [`MAX_DOCUMENT_BYTES`]
/// because an icon is written to disk under a name the index chose and is
/// never swept: at the document cap a well-formed hostile catalog could park
/// megabytes per entry, permanently. Every icon the registry publishes today
/// is under 4 KB.
pub(super) const MAX_ICON_BYTES: usize = 256 * 1024;

/// Serializes refreshes within this process. Refresh fires on both app launch
/// and catalog open, and the anchor update is a read-modify-write: without
/// this, two interleaved refreshes can write a floor derived from a stale
/// snapshot and lower it.
static REFRESH_LOCK: Mutex<()> = Mutex::new(());

/// What one refresh did, for the log and for the catalog's quiet
/// "last refreshed" line.
#[derive(Debug, Clone, PartialEq)]
pub enum RefreshOutcome {
    /// The fetched document was accepted and cached.
    Updated,
    /// The fetch failed on every transport. The cached document (if any)
    /// stays in force — offline is a normal state, not an error.
    Offline(String),
    /// The document arrived but a rule refused it. The cached document stays
    /// in force. For the kill list this is the fail-closed path: a refused
    /// file never means "nothing revoked".
    Refused(RejectReason),
    /// The document was accepted but the cache write failed. Distinct from
    /// `Offline` because it is the state that leaves the anchor unraised, and
    /// calling it "offline" would hide a full or read-only disk behind a
    /// message about the network.
    NotCached(String),
}

/// The result of refreshing both documents, plus the cache as it now stands.
#[derive(Debug, Clone)]
pub struct Refreshed {
    pub index: RefreshOutcome,
    pub revoked: RefreshOutcome,
    pub cache: CachedRegistry,
}

/// GET a small text document, alternating transport across attempts.
///
/// Serves the two registry JSON documents and, at `MAX_ICON_BYTES`, plugin
/// icons. `cap` is not a detail the caller may forget: an icon is written to
/// disk under a name the index chose and never swept, so the document cap that
/// suits an index — megabytes — would let a well-formed hostile catalog park
/// that much per entry, permanently.
pub(super) fn get_document_capped(url: &str, cap: usize) -> Result<String, String> {
    let has_proxy = crate::system::proxy::resolve_proxy_for_url(url).is_some();
    let mut last_err = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        match get_once(url, plan_transport(attempt, has_proxy), cap) {
            Ok(body) => return Ok(body),
            Err(e) => {
                log::debug!("registry fetch attempt {attempt}/{MAX_ATTEMPTS} failed: {e}");
                last_err = e;
            }
        }
    }
    Err(last_err)
}

/// One attempt over one transport.
fn get_once(url: &str, transport: Transport, cap: usize) -> Result<String, String> {
    let mut builder = ureq::AgentBuilder::new()
        // Identify moss on every outbound request, like the rest of the app.
        .user_agent(&crate::system::user_agent())
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT);
    // Match the WKWebView's routing behind a split-tunnel VPN or a GFW proxy;
    // a writer whose whole network is proxied must still see the catalog.
    if transport == Transport::Proxy {
        if let Some(proxy_url) = crate::system::proxy::resolve_proxy_for_url(url) {
            match ureq::Proxy::new(&proxy_url) {
                Ok(proxy) => builder = builder.proxy(proxy),
                Err(e) => log::warn!("registry fetch: proxy '{proxy_url}' is invalid ({e})"),
            }
        }
    }

    let response = builder
        .build()
        .get(url)
        .call()
        .map_err(|e| format!("{url} via {transport:?}: {e}"))?;

    let mut body = String::new();
    let reader = response.into_reader();
    let mut bounded = std::io::Read::take(reader, cap as u64 + 1);
    std::io::Read::read_to_string(&mut bounded, &mut body).map_err(|e| format!("{url}: {e}"))?;
    if body.len() > cap {
        return Err(format!("{url}: response exceeds {cap} bytes"));
    }
    Ok(body)
}

/// Refresh both documents from the pinned origin.
///
/// Called on app launch and on catalog open. Never returns an error: every
/// failure mode degrades to the cached documents, which is the whole point of
/// caching them. The caller reads [`Refreshed::cache`] for what is now in
/// force and the two outcomes for what to log or show.
pub fn refresh(app_data_dir: &std::path::Path) -> Refreshed {
    refresh_from(app_data_dir, REGISTRY_ORIGIN)
}

/// [`refresh`] against an explicit origin. Tests and the e2e fixture only —
/// production has exactly one origin and it is not configurable.
pub fn refresh_from(app_data_dir: &std::path::Path, origin: &str) -> Refreshed {
    // Poisoning is not meaningful here: the lock guards no invariant held in
    // memory, only the ordering of file writes.
    let _guard = REFRESH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache::load(app_data_dir);

    // The kill list is refreshed FIRST. If the process dies mid-refresh, the
    // half that landed is the one that can only ever remove capability.
    let revoked = refresh_revoked(app_data_dir, origin, &cache);
    let index = refresh_index(app_data_dir, origin, &cache);

    Refreshed {
        index,
        revoked,
        // Re-read rather than patching the in-memory copy: what the caller
        // enforces is then exactly what survived the writes AND passed
        // validation on the way back out, so a failed cache write cannot be
        // mistaken for an applied update.
        cache: cache::load(app_data_dir),
    }
}

fn refresh_index(
    app_data_dir: &std::path::Path,
    origin: &str,
    cache: &CachedRegistry,
) -> RefreshOutcome {
    let raw = match get_document_capped(&format!("{origin}/index.json"), MAX_DOCUMENT_BYTES) {
        Ok(raw) => raw,
        Err(e) => {
            log::info!("registry index not refreshed, serving cache: {e}");
            return RefreshOutcome::Offline(e);
        }
    };
    match accept_index(&raw, cache.anchor.highest_index_serial) {
        Ok(index) => {
            if let Err(e) = cache::store_index(app_data_dir, &raw, &index, &crate::system::now_iso()) {
                log::warn!("registry index accepted but not cached: {e}");
                return RefreshOutcome::NotCached(e);
            }
            RefreshOutcome::Updated
        }
        Err(reason) => {
            log::warn!("registry index refused, keeping cache: {reason}");
            RefreshOutcome::Refused(reason)
        }
    }
}

fn refresh_revoked(
    app_data_dir: &std::path::Path,
    origin: &str,
    cache: &CachedRegistry,
) -> RefreshOutcome {
    let raw = match get_document_capped(&format!("{origin}/revoked.json"), MAX_DOCUMENT_BYTES) {
        Ok(raw) => raw,
        Err(e) => {
            // Deliberately not an error: revocation enforcement is designed to
            // survive offline periods from the cached list.
            log::info!("kill list not refreshed, enforcing cached revocations: {e}");
            return RefreshOutcome::Offline(e);
        }
    };
    match accept_revoked(
        &raw,
        cache.anchor.highest_revoked_serial,
        cache.anchor.revocation_count,
    ) {
        Ok(revoked) => {
            if let Err(e) = cache::store_revoked(app_data_dir, &raw, &revoked, &crate::system::now_iso()) {
                log::warn!("kill list accepted but not cached: {e}");
                return RefreshOutcome::NotCached(e);
            }
            RefreshOutcome::Updated
        }
        Err(reason) => {
            // Fail closed: the cached list stays in force.
            log::warn!("kill list refused, enforcing cached revocations: {reason}");
            RefreshOutcome::Refused(reason)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::install::registry_client::cache::RevocationVerdict;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A throwaway static-file server: the local registry fixture the design's
    /// testing section requires, so no test here depends on GitHub being up.
    ///
    /// Serves `index.json` and `revoked.json` from the bodies it is given; a
    /// `None` body answers 404, which is how the offline path is exercised
    /// without unplugging anything.
    struct FixtureOrigin {
        origin: String,
        port: u16,
        shutdown: Option<mpsc::Sender<()>>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    /// A port with nothing listening on it: bind, read the port, drop the
    /// listener. Connections are then genuinely refused, which is what the
    /// offline paths are about — a still-bound socket that accepts and hangs
    /// up tests reset-handling instead.
    fn refused_origin() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        format!("http://127.0.0.1:{port}")
    }

    impl FixtureOrigin {
        fn serving(index: Option<String>, revoked: Option<String>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let (tx, rx) = mpsc::channel::<()>();
            let thread = std::thread::spawn(move || {
                for stream in listener.incoming() {
                    // Checked after `accept` returns, which is why `Drop`
                    // wakes the accept with a self-connect rather than
                    // relying on this alone.
                    if rx.try_recv().is_ok() {
                        return;
                    }
                    let Ok(mut stream) = stream else { return };
                    let mut request = String::new();
                    if BufReader::new(&stream).read_line(&mut request).is_err() {
                        continue;
                    }
                    let body = if request.contains("/index.json") {
                        index.clone()
                    } else if request.contains("/revoked.json") {
                        revoked.clone()
                    } else {
                        None
                    };
                    let response = match body {
                        Some(b) => format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
                            b.len()
                        ),
                        None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                    };
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            });
            FixtureOrigin {
                origin: format!("http://127.0.0.1:{port}"),
                port,
                shutdown: Some(tx),
                thread: Some(thread),
            }
        }
    }

    impl Drop for FixtureOrigin {
        fn drop(&mut self) {
            if let Some(tx) = self.shutdown.take() {
                let _ = tx.send(());
            }
            // The server thread is parked in `accept()`; the flag alone never
            // reaches it. One throwaway connection wakes it so it can see the
            // flag and exit, freeing the port for the rest of the binary.
            let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn index_body(serial: u64) -> String {
        format!(r#"{{"schema_version":1,"serial":{serial},"entries":[{{"type":"plugin","id":"github","display_name":"GitHub","version":"1.5.3","download_url":"https://example.invalid/g.zip","sha256":"aa"}}]}}"#)
    }

    fn revoked_body(serial: u64, revocations: &str) -> String {
        format!(r#"{{"schema_version":1,"serial":{serial},"revocations":[{revocations}]}}"#)
    }

    const ONE_REVOCATION: &str = r#"{"id":"bad","versions":["*"],"reason":"malware"}"#;

    #[test]
    fn a_first_refresh_fetches_and_caches_both_documents() {
        let dir = tempfile::tempdir().unwrap();
        let origin = FixtureOrigin::serving(Some(index_body(28)), Some(revoked_body(1, "")));

        let result = refresh_from(dir.path(), &origin.origin);

        assert_eq!(result.index, RefreshOutcome::Updated);
        assert_eq!(result.revoked, RefreshOutcome::Updated);
        assert_eq!(result.cache.index.unwrap().serial, 28);
        assert_eq!(result.cache.anchor.highest_index_serial, 28);
        assert!(result.cache.anchor.index_fetched_at.is_some());
    }

    #[test]
    fn an_unreachable_origin_serves_the_cache_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        {
            let origin = FixtureOrigin::serving(Some(index_body(28)), Some(revoked_body(1, "")));
            refresh_from(dir.path(), &origin.origin);
        }
        let result = refresh_from(dir.path(), &refused_origin());

        assert!(matches!(result.index, RefreshOutcome::Offline(_)));
        assert!(matches!(result.revoked, RefreshOutcome::Offline(_)));
        assert_eq!(
            result.cache.index.expect("the cached index stays in force").serial,
            28
        );
    }

    #[test]
    fn offline_on_a_cold_cache_is_not_an_error_and_reports_nothing_known() {
        let dir = tempfile::tempdir().unwrap();

        let result = refresh_from(dir.path(), &refused_origin());

        assert!(matches!(result.index, RefreshOutcome::Offline(_)));
        assert!(result.cache.index.is_none());
        assert_eq!(
            result.cache.revocation_for("anything", "1.0.0"),
            RevocationVerdict::NotRevoked,
            "nothing has ever been fetched, so there is nothing to have lost"
        );
    }

    #[test]
    fn a_replayed_older_index_is_refused_and_the_cached_one_stays() {
        let dir = tempfile::tempdir().unwrap();
        {
            let good = FixtureOrigin::serving(Some(index_body(28)), Some(revoked_body(1, "")));
            refresh_from(dir.path(), &good.origin);
        }
        let stale = FixtureOrigin::serving(Some(index_body(27)), Some(revoked_body(1, "")));

        let result = refresh_from(dir.path(), &stale.origin);

        assert!(matches!(result.index, RefreshOutcome::Refused(_)));
        assert_eq!(result.cache.index.unwrap().serial, 28);
    }

    #[test]
    fn a_kill_list_emptied_without_a_serial_bump_is_refused_and_the_old_one_keeps_enforcing() {
        // The whole point of the kill list: a hostile or buggy origin must not
        // be able to un-revoke by serving a smaller file.
        let dir = tempfile::tempdir().unwrap();
        {
            let good = FixtureOrigin::serving(
                Some(index_body(28)),
                Some(revoked_body(7, ONE_REVOCATION)),
            );
            refresh_from(dir.path(), &good.origin);
        }
        let emptied = FixtureOrigin::serving(Some(index_body(28)), Some(revoked_body(7, "")));

        let result = refresh_from(dir.path(), &emptied.origin);

        assert!(matches!(result.revoked, RefreshOutcome::Refused(_)));
        assert!(
            matches!(
                result.cache.revocation_for("bad", "1.0.0"),
                RevocationVerdict::Revoked(_)
            ),
            "the cached revocation must still be enforced"
        );
    }

    #[test]
    fn an_unparseable_kill_list_never_reads_as_nothing_revoked() {
        let dir = tempfile::tempdir().unwrap();
        {
            let good = FixtureOrigin::serving(
                Some(index_body(28)),
                Some(revoked_body(7, ONE_REVOCATION)),
            );
            refresh_from(dir.path(), &good.origin);
        }
        let garbage =
            FixtureOrigin::serving(Some(index_body(28)), Some("<!doctype html>".to_string()));

        let result = refresh_from(dir.path(), &garbage.origin);

        assert!(matches!(result.revoked, RefreshOutcome::Refused(_)));
        assert!(matches!(
            result.cache.revocation_for("bad", "1.0.0"),
            RevocationVerdict::Revoked(_)
        ));
    }

    #[test]
    fn a_missing_kill_list_does_not_block_the_index_refresh() {
        // The two documents are refreshed independently: a 404 on one must not
        // cost the user the other.
        let dir = tempfile::tempdir().unwrap();
        let origin = FixtureOrigin::serving(Some(index_body(28)), None);

        let result = refresh_from(dir.path(), &origin.origin);

        assert_eq!(result.index, RefreshOutcome::Updated);
        assert!(matches!(result.revoked, RefreshOutcome::Offline(_)));
        assert!(result.cache.index.is_some());
    }
}
