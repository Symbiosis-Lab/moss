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

use super::crawl_state::HostPacer;

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
                "URL scheme '{other}' is not allowed — only http/https"
            ))
        }
    }
    // `url::Host::Ipv4`/`Ipv6` carry parsed addresses directly — `host_str()`
    // returns an IPv6 literal WITH its brackets (`"[::1]"`), which fails a
    // plain `str::parse::<IpAddr>()` and would silently let every bracketed
    // IPv6 literal through unchecked.
    match url.host() {
        Some(url::Host::Domain(d)) => {
            if d.eq_ignore_ascii_case("localhost") {
                return Err("URL must not target localhost".to_string());
            }
        }
        Some(url::Host::Ipv4(v4)) => {
            let ip = std::net::IpAddr::V4(v4);
            if is_loopback_or_link_local(&ip) {
                return Err(format!(
                    "URL must not target a loopback or link-local address ({ip})"
                ));
            }
        }
        Some(url::Host::Ipv6(v6)) => {
            let ip = std::net::IpAddr::V6(v6);
            if is_loopback_or_link_local(&ip) {
                return Err(format!(
                    "URL must not target a loopback or link-local address ({ip})"
                ));
            }
        }
        None => return Err("URL has no host".to_string()),
    }
    Ok(())
}

/// True for a loopback or link-local address, including an IPv4 address
/// spelled as an IPv4-mapped IPv6 literal (`::ffff:127.0.0.1`) — a common
/// bypass for a check that only inspects the IPv6 form directly.
fn is_loopback_or_link_local(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            if v6.is_loopback() {
                return true;
            }
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return mapped.is_loopback() || mapped.is_link_local();
            }
            // fe80::/10
            (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Maximum redirect hops [`fetch_with_validated_redirects`] will follow —
/// ureq's own former default (`AgentBuilder::redirects` doc), preserved so a
/// legitimate multi-hop chain (http→https, then a trailing-slash or
/// www-prefix canonicalization) keeps working exactly as it did when ureq
/// followed redirects itself.
const MAX_REDIRECT_HOPS: u32 = 5;

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
fn fetch_with_validated_redirects(
    agent: &ureq::Agent,
    start_url: &str,
    user_agent: &str,
    accept: Option<&str>,
) -> Result<ureq::Response, ureq::Error> {
    fetch_following_redirects(agent, start_url, user_agent, accept, refuse_unsafe_scrape_url)
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
    validate: impl Fn(&str) -> Result<(), String>,
) -> Result<ureq::Response, ureq::Error> {
    // 400: a permanent, never-retried refusal (`should_retry_status` only
    // retries 429/5xx) — a redirect target this function refused, or a
    // chain that ran past the hop cap, will not look any different on a
    // retry. `call_with_retry`'s error text is `"{code} {status_text}"`
    // (it never reads the body), so the detail goes in `status_text`
    // itself rather than being silently dropped.
    const REFUSED_STATUS: u16 = 400;
    let refused = |status_text: &str| {
        ureq::Error::Status(
            REFUSED_STATUS,
            ureq::Response::new(REFUSED_STATUS, status_text, "")
                .expect("status line without newlines always builds"),
        )
    };

    let mut current = start_url.to_string();
    for _ in 0..=MAX_REDIRECT_HOPS {
        let mut req = agent.get(&current).set("User-Agent", user_agent);
        if let Some(accept) = accept {
            req = req.set("Accept", accept);
        }
        let response = req.call()?;
        if !(300..400).contains(&response.status()) {
            return Ok(response);
        }
        let Some(location) = response.header("Location") else {
            return Ok(response);
        };
        let next = match url::Url::parse(&current).and_then(|base| base.join(location)) {
            Ok(joined) => joined.to_string(),
            Err(_) => return Ok(response),
        };
        // Re-canonicalize the hop the same way the pre-download rewrite
        // does (see this function's doc comment) — a redirect can land on a
        // ruled CDN host carrying a query the table never produced. Scoped
        // to asset downloads: a page fetch (`accept: None`) has no media
        // URL to canonicalize and must not have one invented for it.
        let next = if accept.is_some() {
            crate::vault::import::media::original_media_url(&next).unwrap_or(next)
        } else {
            next
        };
        if let Err(msg) = validate(&next) {
            return Err(refused(&format!("redirected to a disallowed URL: {msg}")));
        }
        current = next;
    }
    Err(refused(&format!("too many redirects (> {MAX_REDIRECT_HOPS})")))
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
        match fetch_with_validated_redirects(&agent, url, user_agent, accept) {
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
) -> Result<ureq::Response, FetchError> {
    const TRIES: u32 = 3;
    call_with_retry_n(url, user_agent, timeout, TRIES, accept)
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
pub(crate) async fn fetch_page(url: &str, user_agent: &str) -> Result<(String, String), FetchError> {
    let url = url.to_string();
    let user_agent = user_agent.to_string();
    tokio::task::spawn_blocking(move || {
        let response =
            call_with_retry(&url, &user_agent, std::time::Duration::from_secs(30), None)?;
        let content_type = response.content_type().to_string();

        response
            .into_string()
            .map(|body| (body, content_type))
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

/// What a download did: wrote the file, or refused it for its size.
pub(crate) enum Downloaded {
    Saved(String),
    TooLarge,
}

pub(crate) async fn download_asset(
    url: &str,
    assets_dir: &Path,
    user_agent: &str,
) -> Result<String, FetchError> {
    match download_to(url, assets_dir, user_agent, hashed_filename, u64::MAX).await? {
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
    max_bytes: u64,
    filename: String,
) -> Result<Downloaded, FetchError> {
    download_to(url, assets_dir, user_agent, move |_, _, _| filename, max_bytes).await
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
    name: impl FnOnce(&str, &Path, &str) -> String + Send + 'static,
    max_bytes: u64,
) -> Result<Downloaded, FetchError> {
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

#[cfg(test)]
#[path = "fetch_tests.rs"]
mod tests;
