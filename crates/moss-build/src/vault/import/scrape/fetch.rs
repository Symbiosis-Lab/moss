//! Pure HTTP fetch infrastructure for the scrape pipeline: SSRF-safe
//! redirect validation, Retry-After-aware retry, and the two raw-fetch
//! primitives everything else (a page fetch, a media download, a sitemap
//! document fetch) is built on. No [`super::crawl_state::CrawlState`], no
//! crawl orchestration — just the HTTP calls themselves, so [`super::sitemap`]
//! can depend on this file without depending on [`super::run`], the
//! orchestrator that in turn depends on this one.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use super::crawl_state::HostPacer;
use super::scope::UrlScope;

/// Strip query + fragment from a URL, leaving a clean canonical form for the
/// `origin` frontmatter. Falls back to the input if it doesn't parse.
pub(crate) fn strip_query(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut u) => {
            u.set_query(None);
            u.set_fragment(None);
            u.to_string()
        }
        Err(_) => url.to_string(),
    }
}

/// Transient failures worth one more attempt: transport-level errors
/// (None) and 429/5xx. Client errors (4xx) are permanent.
fn should_retry_status(status: Option<u16>) -> bool {
    match status {
        None => true,
        Some(429) => true,
        Some(s) => s >= 500,
    }
}

/// Longest delay [`call_with_retry_n`] will ever honor from a `Retry-After`
/// header, regardless of what the host asked for — a host asking for an
/// hour is asking the wrong tool, and a crawl that waited it out verbatim
/// could stall the whole run on one URL.
const MAX_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

/// Parse a `Retry-After` header value (RFC 9110 §10.2.3): either an integer
/// delta-seconds, or an HTTP-date — `chrono`'s RFC 2822 parser accepts it,
/// since the HTTP-date grammar (`Sun, 06 Nov 1994 08:49:37 GMT`) is itself a
/// valid, non-obsolete RFC 2822 date-time. Returns `None` for a
/// missing/unparsable header, a non-positive delta-seconds, or a date
/// already in the past — [`call_with_retry_n`] falls back to its own
/// exponential backoff in all of those cases, same as a 429/5xx with no
/// header at all. Zero is treated as "no useful wait", same as a negative
/// value or a past date, rather than as license to retry with no delay at
/// all: a 429 pairing its rate-limit refusal with `Retry-After: 0` is a
/// contradiction the backoff schedule is a safer response to than an
/// instant retry would be, and the two branches below would otherwise
/// disagree about it — an integer `0` would have been honored while an
/// HTTP-date computing to the same "now" was already rejected. The result
/// is capped at [`MAX_RETRY_AFTER`].
fn parse_retry_after(value: &str) -> Option<std::time::Duration> {
    let value = value.trim();
    if let Ok(secs) = value.parse::<i64>() {
        return (secs > 0).then(|| std::time::Duration::from_secs(secs as u64).min(MAX_RETRY_AFTER));
    }
    let when = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    let delta = when.with_timezone(&chrono::Utc) - chrono::Utc::now();
    let secs = delta.num_seconds();
    (secs > 0).then(|| std::time::Duration::from_secs(secs as u64).min(MAX_RETRY_AFTER))
}

/// Debug builds only: let imports reach this machine, for a local test
/// server. A process flag rather than the environment variable, which a test
/// cannot set while other tests' threads read the environment.
#[cfg(debug_assertions)]
pub fn allow_loopback() {
    ALLOW_LOOPBACK.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(debug_assertions)]
static ALLOW_LOOPBACK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Refuse a URL that is not a plain `http`/`https` request to a public host.
///
/// The import path hands `start_url` (and, via [`fetch_with_validated_redirects`],
/// every redirect target reached while fetching it) straight to ureq — an
/// attacker-controlled webview could otherwise point the app's own network
/// stack at `file://` (reading local files into the imported vault, where
/// they get published) or an internal service (`http://127.0.0.1:<port>`, a
/// link-local cloud-metadata address). A URL that passes this check on
/// `start_url` alone is not enough: a public host the check allowed can
/// itself 302/303/307/308 to a loopback/link-local target, so
/// [`fetch_with_validated_redirects`] re-runs this SAME function on every
/// hop rather than trusting ureq's own redirect-following (which does not
/// re-validate at all).
///
/// Checked on the URL string alone: DNS rebinding — a hostname that
/// resolves to a loopback/link-local address only at connect time — is NOT
/// covered here; that needs a connect-time check inside the fetch itself,
/// out of scope for this string-level gate.
pub fn refuse_unsafe_scrape_url(url_str: &str) -> Result<(), String> {
    let url = url::Url::parse(url_str).map_err(|e| format!("invalid URL: {e}"))?;
    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "URL scheme '{other}' is not allowed - only http/https"
            ))
        }
    }
    // Local development is explicit and cannot be enabled in a release build.
    #[cfg(debug_assertions)]
    if ALLOW_LOOPBACK.load(std::sync::atomic::Ordering::Relaxed) || std::env::var("MOSS_IMPORT_ALLOW_LOOPBACK").as_deref() == Ok("1") {
        let loopback = match url.host() {
            Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        };
        if loopback { return Ok(()); }
    }
    // `url::Host::Ipv4`/`Ipv6` carry parsed addresses directly — `host_str()`
    // returns an IPv6 literal WITH its brackets (`"[::1]"`), which fails a
    // plain `str::parse::<IpAddr>()` and would silently let every bracketed
    // IPv6 literal through unchecked.
    let ip: std::net::IpAddr = match url.host() {
        Some(url::Host::Domain(d)) if d.eq_ignore_ascii_case("localhost") => {
            return Err("URL must not target localhost".to_string());
        }
        Some(url::Host::Domain(_)) => return Ok(()),
        Some(url::Host::Ipv4(v4)) => v4.into(),
        Some(url::Host::Ipv6(v6)) => v6.into(),
        None => return Err("URL has no host".to_string()),
    };
    use crate::system::address::{classify, AddressClass};
    if matches!(classify(ip), AddressClass::Loopback | AddressClass::LinkLocal) {
        return Err(format!("URL must not target a loopback or link-local address ({ip})"));
    }
    Ok(())
}

/// `agent.get(url)`, following redirects by hand so every hop's target is
/// re-validated by [`refuse_unsafe_scrape_url`] before it is followed —
/// `agent` must be built with `redirects(0)`
/// ([`crate::system::proxy::proxied_ureq_agent_no_redirects`]), or ureq
/// would already have followed (and connected to) the first hop before this
/// function ever saw it.
///
/// A `Location` header is resolved against the URL that sent it (it may be
/// relative) before being checked and followed. A 3xx with no `Location`,
/// or one that fails to parse even as a relative reference, is returned
/// as-is rather than chased.
///
/// `accept`, when given, is sent as the `Accept` header on every hop
/// (including the start URL) — see [`crate::vault::import::media::MEDIA_ACCEPT`].
/// `None` leaves ureq's own default (`Accept: */*`) in place, for a page
/// fetch.
///
/// With `scope` (a link being crawled), a hop off the site or to a
/// `mailto:`/`tel:` address is not requested but handed back as the 3xx
/// response, so the caller sees where the link leads; every other target is
/// still validated and followed, and a non-http(s) target still refused.
fn fetch_with_validated_redirects(
    agent: &ureq::Agent,
    start_url: &str,
    user_agent: &str,
    accept: Option<&str>,
    referer: Option<&str>,
    scope: Option<&UrlScope>,
) -> Result<ureq::Response, ureq::Error> {
    fetch_following_redirects(agent, start_url, user_agent, accept, referer, import_policy, |next| {
        scope.is_some_and(|scope| leaves_site(scope, next))
    })
}

/// [`refuse_unsafe_scrape_url`], except in this crate's own tests, where the
/// loopback servers they started with [`test_server`] are let through. Any
/// other address, loopback included, still meets the real policy.
fn import_policy(url: &str) -> Result<(), String> {
    #[cfg(test)]
    if test_servers::started(url) {
        return Ok(());
    }
    refuse_unsafe_scrape_url(url)
}

/// A mockito server the import may fetch from. mockito keeps its servers
/// bound for the life of the process, so a registered port never passes to
/// another listener.
#[cfg(test)]
pub(crate) async fn test_server() -> mockito::ServerGuard {
    let server = mockito::Server::new_async().await;
    test_servers::STARTED.lock().unwrap().insert(server.host_with_port());
    server
}

/// A loopback address no test registered, answering every request with an
/// empty 200, and how many requests reached it.
#[cfg(test)]
pub(crate) fn unregistered_loopback_server() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = hits.clone();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let _ = stream.read(&mut [0; 4096]);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    (address, hits)
}

#[cfg(test)]
mod test_servers {
    pub(super) static STARTED: std::sync::Mutex<std::collections::BTreeSet<String>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());

    pub(super) fn started(url: &str) -> bool {
        let Ok(url) = url::Url::parse(url) else { return false };
        let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else { return false };
        STARTED.lock().unwrap().contains(&format!("{host}:{port}"))
    }
}

/// Whether `next` is a contact address or an http(s) URL off the site.
fn leaves_site(scope: &UrlScope, next: &str) -> bool {
    let Ok(url) = url::Url::parse(next) else { return false };
    match url.scheme() {
        "mailto" | "tel" => true,
        "http" | "https" => super::scope::left_site_by_redirect(scope, next),
        _ => false,
    }
}

/// [`fetch_with_validated_redirects`]'s body, taking the per-hop validator as
/// a parameter so the redirect-following MECHANISM (hop resolution, the hop
/// cap, handing back a Location-less or unparsable 3xx as-is) is testable
/// independently of the SSRF POLICY (`refuse_unsafe_scrape_url`) — every
/// test server a unit test can stand up is itself a loopback address, so a
/// test proving a legitimate same-host redirect is still followed cannot
/// use the real policy without tripping its own loopback refusal. Production
/// always calls the [`fetch_with_validated_redirects`] wrapper above, which
/// always wires in the real policy; no caller outside this file's tests
/// should call this directly.
///
/// On an asset download (`accept` is `Some`), every redirect target also
/// goes through [`crate::vault::import::media::original_media_url`] before
/// it is validated or followed — the single place that rewrite applies to a
/// hop, so a CDN row never needs to be re-applied by each caller. Without
/// it, a `static1.squarespace.com` URL this function was handed already
/// rewritten (by the caller, before the URL was ever committed to markdown)
/// 301s to `images.squarespace-cdn.com` with a plain `content-type=` query
/// that dropped the rewrite, and that redirect target would download
/// un-rewritten. A page fetch (`accept: None`) never applies this rewrite —
/// it has no media URL to canonicalize, and the two ruled hosts are
/// asset-only CDNs no page redirect has a legitimate reason to land on.
fn fetch_following_redirects(
    agent: &ureq::Agent,
    start_url: &str,
    user_agent: &str,
    accept: Option<&str>,
    referer: Option<&str>,
    validate: impl Fn(&str) -> Result<(), String>,
    reports: impl Fn(&str) -> bool,
) -> Result<ureq::Response, ureq::Error> {
    // A redirect can land on a ruled CDN host carrying a query the table never
    // produced, so asset hops are re-canonicalized; a page fetch has no media
    // URL to canonicalize and must not have one invented for it.
    let rewrite = |next: String| {
        if accept.is_some() {
            crate::vault::import::media::original_media_url(&next).unwrap_or(next)
        } else {
            next
        }
    };
    crate::system::redirects::get_following_redirects(agent, start_url, user_agent, accept, referer, validate, rewrite, reports)
}

/// A fetch's final failure, carrying the HTTP status code as DATA when the
/// failure was a response status — as opposed to a transport failure, a
/// body-read failure, or a redirect refusal before any status came back —
/// so a caller that needs to know *why* a fetch failed (chiefly
/// [`observe_pace`], deciding whether a host just rate-limited this crawl)
/// matches on `status` instead of re-parsing the text composed for display.
/// `Display` prints the same `"{code} {status_text}"` / `"HTTP error: …"`
/// text [`PageOutcome::Failed`] and the crawl's `log::warn!` calls already
/// expect, and `From<FetchError> for String` lets every caller outside this
/// file's status-aware ones (`fetch_raw`/`fetch_raw_bytes`, whose callers in
/// `sitemap.rs` only ever check `.is_ok()`) keep using `?` unchanged.
#[derive(Debug, Clone)]
pub(crate) struct FetchError {
    status: Option<u16>,
    message: String,
}

impl FetchError {
    fn message(message: String) -> Self {
        Self { status: None, message }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<FetchError> for String {
    fn from(e: FetchError) -> String {
        e.message
    }
}

/// `ureq::get` with up to `tries` attempts, for transient failures only (see
/// [`should_retry_status`]). Permanent failures (4xx other than 429) return
/// immediately regardless of `tries`.
///
/// The delay before each retry is polite rather than blind: a 429 or 503
/// that named a `Retry-After` is honored (parsed by [`parse_retry_after`],
/// capped at [`MAX_RETRY_AFTER`]); anything else — no header, an unparsable
/// one, or a transport-level error with no response at all — falls back to
/// exponential backoff (500ms, then 1s, then 2s, …) the same as before this
/// header was read at all.
///
/// Note: ureq maps any non-2xx response to `Error::Status` on `.call()`
/// itself, so this single retry loop also replaces what used to be a
/// separate manual `status >= 400` check in `fetch_page`.
///
/// `accept` is forwarded to [`fetch_with_validated_redirects`] unchanged —
/// `None` for a page fetch (ureq's own `Accept: */*` default), `Some(
/// media::MEDIA_ACCEPT)` for an asset download.
fn call_with_retry_n(
    url: &str,
    user_agent: &str,
    timeout: std::time::Duration,
    tries: u32,
    accept: Option<&str>,
    referer: Option<&str>,
    scope: Option<&UrlScope>,
) -> Result<ureq::Response, FetchError> {
    let mut last_err = FetchError::message(String::new());
    // What the PREVIOUS attempt's response asked for via `Retry-After`,
    // consumed by the sleep before the NEXT attempt. Reset on every
    // iteration (not just when absent), so a Retry-After-less response
    // following one that had it doesn't keep honoring a now-stale wait.
    let mut retry_after: Option<std::time::Duration> = None;
    for attempt in 0..tries {
        if attempt > 0 {
            let delay = retry_after.take().unwrap_or_else(|| {
                std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1))
            });
            std::thread::sleep(delay);
        }
        // Proxy-aware like every other moss HTTP client (system::proxy):
        // a direct connect times out on hosts only reachable via proxy.
        // `_no_redirects`: this path re-validates every redirect hop itself
        // (see `fetch_with_validated_redirects`) rather than trusting
        // ureq's own follower, which never re-checks a Location header.
        let agent = crate::system::proxy::proxied_ureq_agent_no_redirects(url, timeout);
        match fetch_with_validated_redirects(&agent, url, user_agent, accept, referer, scope) {
            Ok(resp) => return Ok(resp),
            Err(ureq::Error::Status(code, resp)) => {
                let err = FetchError { status: Some(code), message: format!("{} {}", code, resp.status_text()) };
                if !should_retry_status(Some(code)) {
                    return Err(err);
                }
                retry_after = resp.header("Retry-After").and_then(parse_retry_after);
                last_err = err;
            }
            Err(e) => {
                // Transport-level error (timeout, connection reset, TLS...)
                // — no response at all, so no status code either.
                retry_after = None;
                last_err = FetchError::message(format!("HTTP error: {}", e));
            }
        }
    }
    Err(last_err)
}

/// [`call_with_retry_n`] with the pipeline's standard 3 attempts (1 initial
/// + 2 retries) — every page and asset fetch goes through this one.
fn call_with_retry(
    url: &str,
    user_agent: &str,
    timeout: std::time::Duration,
    accept: Option<&str>,
    referer: Option<&str>,
    scope: Option<&UrlScope>,
) -> Result<ureq::Response, FetchError> {
    const TRIES: u32 = 3;
    call_with_retry_n(url, user_agent, timeout, TRIES, accept, referer, scope)
}

/// Translates a fetch's `Result` into the bare `Result<(), Option<u16>>`
/// [`HostPacer::observe`] reacts to — `status` is read off
/// [`FetchError::status`], the typed code [`call_with_retry_n`] captured
/// straight from `ureq::Error::Status`, not re-derived from the message text
/// a wording change could drift out from under a string match. The
/// grow/decay decision itself lives on `HostPacer` (`crawl_state.rs`) so the
/// pacing module stays usable without this file's HTTP types in scope; this
/// is the one line of glue between them, called right after every page or
/// asset fetch in the crawl loop.
pub(crate) fn observe_pace<T>(pacer: &mut HostPacer, host: &str, result: &Result<T, FetchError>) {
    pacer.observe(host, result.as_ref().map(|_| ()).map_err(|e| e.status));
}

/// Fetch a page's body, alongside its declared Content-Type — the caller
/// decides whether the response is a page worth importing at all
/// ([`looks_like_html_page`]) before doing anything else with the body.
///
/// `accept: None` — a page fetch is never asked to localize a transcoded
/// variant, so it keeps ureq's own `Accept: */*` default unchanged.
///
/// `scope` is the imported site's when `url` is a link found on it: a
/// redirect from such a link to another host or to a `mailto:`/`tel:` address
/// is not requested, and the address it pointed at comes back as the final URL.
pub(crate) async fn fetch_page(
    url: &str,
    user_agent: &str,
    scope: Option<&UrlScope>,
) -> Result<(String, String, String), FetchError> {
    let url = url.to_string();
    let user_agent = user_agent.to_string();
    let scope = scope.cloned();
    tokio::task::spawn_blocking(move || {
        let response = call_with_retry(
            &url,
            &user_agent,
            std::time::Duration::from_secs(30),
            None,
            None,
            scope.as_ref(),
        )?;
        let content_type = response.content_type().to_string();
        let final_url = crate::system::redirects::redirect_target(&response)
            .unwrap_or_else(|| response.get_url().to_string());

        response
            .into_string()
            .map(|body| (body, content_type, final_url))
            .map_err(|e| FetchError::message(format!("Failed to read response: {}", e)))
    })
    .await
    .map_err(|e| FetchError::message(format!("Task error: {}", e)))?
}

/// Bytes read from [`fetch_raw`]/[`fetch_raw_bytes`] beyond this are refused
/// rather than buffered — mirrors ureq's own `into_string` cap
/// (`INTO_STRING_LIMIT`, 10 MiB), so reading raw bytes instead of a String
/// (needed to detect a gzip-compressed sitemap before any lossy UTF-8
/// decoding happens) doesn't also drop the memory bound that method gave
/// every other fetch in this file for free. A hostile or misconfigured
/// sitemap is refused the same way an oversized ordinary page response
/// already is, rather than buffered without limit.
const MAX_RAW_FETCH_BYTES: usize = 10 * 1024 * 1024;

/// A URL's raw response bytes, in a single attempt — the primitive
/// [`fetch_raw`] wraps with a lossy UTF-8 decode, and that a sitemap
/// document fetch calls directly instead so gzip-compressed bytes (see
/// `sitemap::decode_sitemap_bytes`) survive to be decompressed before any
/// text conversion happens. See [`fetch_raw`] for why a single attempt, no
/// content-type gate, and the same SSRF-revalidating fetch a page fetch
/// itself uses.
pub(crate) async fn fetch_raw_bytes(url: &str, user_agent: &str) -> Result<Vec<u8>, String> {
    let url = url.to_string();
    let user_agent = user_agent.to_string();
    tokio::task::spawn_blocking(move || {
        let response = call_with_retry_n(
            &url,
            &user_agent,
            std::time::Duration::from_secs(15),
            1,
            None,
            None,
            None,
        )?;
        let mut buf = Vec::new();
        response
            .into_reader()
            .take((MAX_RAW_FETCH_BYTES + 1) as u64)
            .read_to_end(&mut buf)
            .map_err(|e| format!("Failed to read response: {}", e))?;
        if buf.len() > MAX_RAW_FETCH_BYTES {
            return Err(format!(
                "response body exceeds {MAX_RAW_FETCH_BYTES} bytes"
            ));
        }
        Ok(buf)
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?
}

/// A URL's raw body with no content-type gate, decoded as UTF-8 (lossily —
/// a non-UTF-8 body simply fails whatever the caller does with it next,
/// same as any other malformed document), in a single attempt — for
/// `robots.txt`, which is always plain text, unlike a sitemap document
/// itself (see [`fetch_raw_bytes`], which that fetch uses directly so a
/// gzip-compressed body isn't corrupted by this function's lossy decode
/// before decompression runs). Neither `robots.txt` nor a sitemap document
/// is HTML (so [`looks_like_html_page`] is never asked to judge one) or
/// worth [`call_with_retry`]'s multi-second backoff: most sites declare no
/// sitemap at all, so a 404 here is the ordinary, expected case, not a
/// transient failure worth waiting out before falling through to a
/// link-only crawl.
pub(crate) async fn fetch_raw(url: &str, user_agent: &str) -> Result<String, String> {
    let bytes = fetch_raw_bytes(url, user_agent).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// A short, stable name for a media URL, so the same image fetched twice
/// lands on one file. Was `scrape/assets.rs`, whose `AssetManager` had no
/// production caller and went with it.
pub(crate) fn hash_url(url: &str) -> String {
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(url.as_bytes()))
}

/// Largest single file a page's link may pull into the site.
pub(crate) const LINKED_FILE_MAX_BYTES: u64 = 100 * 1024 * 1024;

/// The page's origin as `Referer`; nothing on an https to http downgrade.
fn referer_for(page_url: &str, target_url: &str) -> Option<String> {
    let page = url::Url::parse(page_url).ok()?;
    let target = url::Url::parse(target_url).ok()?;
    if !matches!(page.scheme(), "http" | "https") || (page.scheme() == "https" && target.scheme() == "http") {
        return None;
    }
    Some(format!("{}/", page.origin().ascii_serialization()))
}

/// What a download did: wrote the file, or refused it for its size.
pub(crate) enum Downloaded {
    Saved(String),
    TooLarge,
}

pub(crate) async fn download_asset(
    url: &str,
    assets_dir: &Path,
    user_agent: &str,
    page_url: Option<&str>,
) -> Result<String, FetchError> {
    match download_to(url, assets_dir, user_agent, page_url, hashed_filename, u64::MAX).await? {
        Downloaded::Saved(filename) => Ok(filename),
        Downloaded::TooLarge => Err(FetchError::message(format!("{url} is too large to download"))),
    }
}

fn hashed_filename(url: &str, _assets_dir: &Path, content_type: &str) -> String {
    format!("{}.{}", hash_url(url), content_type_to_extension(content_type))
}

/// [`download_asset`] for a file a page links to (a PDF, a score, a
/// document): same request, same folder, but the file is saved under
/// `filename` (see [`linked_filename`]) rather than a hash. A file over
/// `max_bytes` is not written.
pub(crate) async fn download_linked_file(
    url: &str,
    assets_dir: &Path,
    user_agent: &str,
    page_url: Option<&str>,
    max_bytes: u64,
    filename: String,
) -> Result<Downloaded, FetchError> {
    download_to(url, assets_dir, user_agent, page_url, move |_, _, _| filename, max_bytes).await
}

/// The name a linked file keeps: the last path segment of its URL,
/// decoded and passed through the importer's filename policy, so
/// `/s/Violin%20Part.PDF` is `Violin Part.pdf`. A URL with no usable name
/// falls back to its hash.
pub(crate) fn linked_filename(url: &str) -> String {
    let last_segment = url::Url::parse(url)
        .ok()
        .and_then(|u| u.path_segments().and_then(|s| s.last().map(str::to_string)))
        .unwrap_or_default();
    let decoded = urlencoding::decode(&last_segment)
        .map(|d| d.into_owned())
        .unwrap_or(last_segment);
    let (stem, ext) = match decoded.rsplit_once('.') {
        Some((s, e)) if !e.is_empty() => (s.to_string(), e.to_ascii_lowercase()),
        _ => (decoded, "bin".to_string()),
    };
    let stem = super::writer::sanitize_filename(&stem);
    if stem.is_empty() {
        format!("{}.{}", hash_url(url), ext)
    } else {
        format!("{stem}.{ext}")
    }
}

async fn download_to(
    url: &str,
    assets_dir: &Path,
    user_agent: &str,
    page_url: Option<&str>,
    name: impl FnOnce(&str, &Path, &str) -> String + Send + 'static,
    max_bytes: u64,
) -> Result<Downloaded, FetchError> {
    let referer = page_url.and_then(|page| referer_for(page, url));
    let url_clone = url.to_string();
    let assets_dir = assets_dir.to_path_buf();
    let user_agent = user_agent.to_string();

    tokio::task::spawn_blocking(move || {
        // `Some(MEDIA_ACCEPT)`: a media download wants the file as uploaded,
        // not whatever format a negotiating CDN would substitute for ureq's
        // own `Accept: */*` default — see that constant's doc comment.
        let response = call_with_retry(
            &url_clone,
            &user_agent,
            std::time::Duration::from_secs(60),
            Some(crate::vault::import::media::MEDIA_ACCEPT),
            referer.as_deref(),
            None,
        )?;

        if response
            .header("content-length")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .is_some_and(|len| len > max_bytes)
        {
            return Ok(Downloaded::TooLarge);
        }
        let filename = name(&url_clone, &assets_dir, response.content_type());

        // A response without a content-length is buffered up to the cap.
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| FetchError::message(format!("Failed to read asset: {}", e)))?;
        if bytes.len() as u64 > max_bytes {
            return Ok(Downloaded::TooLarge);
        }

        let file_path = assets_dir.join(&filename);
        // allow:raw_write the author's own source content, not `.moss/build.nosync/` output — a downloaded media file
        fs::write(&file_path, &bytes)
            .map_err(|e| FetchError::message(format!("Failed to write asset: {}", e)))?;

        Ok(Downloaded::Saved(filename))
    })
    .await
    .map_err(|e| FetchError::message(format!("Task error: {}", e)))?
}

pub(crate) fn content_type_to_extension(content_type: &str) -> &str {
    match content_type {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/ogg" => "ogg",
        "audio/wav" => "wav",
        "application/pdf" => "pdf",
        _ => "bin",
    }
}

// ── Progress line for a slow page ────────────────────────────────────

/// Quiet period between two lines, and the time a page's downloads may take
/// before the first one.
const INTERVAL: Duration = Duration::from_secs(30);

/// The line to print now, or `None` when `since_last_line` is under 30 s or
/// nothing is left to download. `delay` is the host's current spacing between
/// requests; zero leaves the spacing and the estimate out.
pub(crate) fn progress_line(done: usize, total: usize, delay: Duration, since_last_line: Duration) -> Option<String> {
    if since_last_line < INTERVAL || done >= total {
        return None;
    }
    let mut line = format!("    ↳ {done} of {total} files for this page");
    if !delay.is_zero() {
        let left = delay.saturating_mul((total - done) as u32);
        line.push_str(&format!(
            "; requests are {} apart, about {} left",
            human(delay),
            human(left)
        ));
    }
    Some(line)
}

fn human(d: Duration) -> String {
    let secs = d.as_secs_f64().round() as u64;
    match secs {
        0..=89 => format!("{secs} s"),
        90..=5399 => format!("{} min", (secs + 30) / 60),
        _ => format!("{:.1} h", secs as f64 / 3600.0),
    }
}

/// Counts one page's file downloads as they finish and prints the line.
pub(crate) struct FileProgress {
    total: usize,
    done: usize,
    last_line: Instant,
}

impl FileProgress {
    pub(crate) fn new(total: usize) -> Self {
        Self { total, done: 0, last_line: Instant::now() }
    }

    /// Call after each download, successful or not.
    pub(crate) fn finished_one(&mut self, delay: Duration) {
        self.done += 1;
        if let Some(line) = progress_line(self.done, self.total, delay, self.last_line.elapsed()) {
            eprintln!("{line}");
            self.last_line = Instant::now();
        }
    }
}

#[cfg(test)]
#[path = "fetch_tests.rs"]
mod tests;
