//! URL metadata fetching and caching for link preview auto-population.
//!
//! When a grid cell contains a bare URL or empty link text, we fetch the
//! page's Open Graph / meta tags to populate the link preview automatically.

use crate::moss_paths::MossPaths;
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::path::Path;

/// Metadata extracted from a URL's HTML page.
///
/// `description` is retained for backward compatibility with on-disk cache
/// entries written by older moss versions, but is no longer rendered or
/// parsed for new entries — the link-preview card design intentionally
/// shows only title + favicon/domain. New writes always set it to `None`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LinkMeta {
    pub url: String,
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub favicon: Option<String>,
    /// The linked page's `og:image` (falling back to `twitter:image`),
    /// resolved to an absolute `http(s)://` URL. This is the REMOTE URL
    /// only — never emitted in built HTML. The build downloads it into
    /// moss's own content-addressed cache and stores the resulting LOCAL
    /// asset in a separate cache (see `build::media::remote_cover`); this
    /// field exists so that download step knows what to fetch, and so a
    /// change in the remote URL (a site swapping its social-preview image)
    /// is visible on the next refresh even though the LOCAL cover cache is
    /// keyed by content hash, not by this URL.
    #[serde(default)]
    pub og_image: Option<String>,
    /// Content-store id of the downloaded `og_image` bytes (sha256, via
    /// `ObjectStore::store_bytes`), and the format it was sniffed as
    /// (never trusted from the URL or a `Content-Type` header). `None`
    /// when there's no `og_image`, or the download/decode failed.
    #[serde(default)]
    pub cover_oid: Option<String>,
    #[serde(default)]
    pub cover_ext: Option<String>,
    /// The rest of these are filled in by
    /// `build::media::remote_cover::materialize_remote_covers`, once the
    /// downloaded bytes above have actually been encoded into THIS build's
    /// output — a separate step from the network fetch above, run with
    /// `output_dir`/`PendingManifest` in scope. `cover_served_path` is a
    /// root-relative path already staged on disk (checked with
    /// `io_utils::output_present` before use — a stale entry whose file
    /// didn't survive a rebuild must fall back to the placeholder, not
    /// synthesize a 404ing `<source>`).
    #[serde(default)]
    pub cover_served_path: Option<String>,
    #[serde(default)]
    pub cover_width: Option<u32>,
    #[serde(default)]
    pub cover_height: Option<u32>,
    #[serde(default)]
    pub cover_lqip: Option<String>,
    #[serde(default)]
    pub cover_color: Option<String>,
    pub fetched_at: String, // ISO 8601
}

/// Parse OG/meta tags from HTML content.
///
/// Priority: og:title > `<title>` for title.
///
/// Description is intentionally not parsed: the external grid card renders
/// only title + favicon/domain (see `build::components::grid_card::render_external_card`). Skipping the
/// description selectors saves disk bytes per cache entry and parser work
/// per fetch — both small but free wins.
pub fn parse_link_meta(url: &str, html: &str) -> LinkMeta {
    let document = Html::parse_document(html);

    // og:title
    let og_title = Selector::parse(r#"meta[property="og:title"]"#)
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // <title> fallback
    let title_tag = Selector::parse("title")
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty());

    let title = og_title.or(title_tag);

    // Favicon: <link rel="icon" href="..."> or <link rel="shortcut icon" href="...">
    let favicon_href = Selector::parse(r#"link[rel="icon"]"#)
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .and_then(|el| el.value().attr("href"))
        .or_else(|| {
            Selector::parse(r#"link[rel="shortcut icon"]"#)
                .ok()
                .and_then(|sel| document.select(&sel).next())
                .and_then(|el| el.value().attr("href"))
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let origin = extract_origin(url);

    // Cover image: og:image, falling back to twitter:image (the same
    // priority order as title). Resolved with the same `resolve_url` a
    // relative favicon uses, but restricted to http(s) — unlike a favicon,
    // this is never inlined; the build downloads it, so a `data:` or other
    // opaque-scheme value is simply not a fetchable image. Computed before
    // the favicon match below, which consumes `origin` by value on its
    // `None` arm.
    let og_image_content = Selector::parse(r#"meta[property="og:image"]"#)
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .and_then(|el| el.value().attr("content"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            Selector::parse(r#"meta[name="twitter:image"]"#)
                .ok()
                .and_then(|sel| document.select(&sel).next())
                .and_then(|el| el.value().attr("content"))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    let og_image = og_image_content
        .map(|href| resolve_url(&href, &origin))
        .filter(|url| is_http_url(url));

    let favicon = match favicon_href {
        Some(href) => {
            let resolved = resolve_url(&href, &origin);
            // The fetched page is untrusted, and this string is copied
            // verbatim into `<img src="…">` on every page that links out
            // to it (`build::components::grid_card::render_external_card`). `data:,` (no payload after
            // the comma) is a deliberate "we have no favicon" placeholder
            // some sites emit to stop browsers guessing /favicon.ico, and
            // anything else that isn't a small http(s)/raster-data URL is
            // dropped the same way rather than handed to the sink — see
            // `is_safe_favicon_url`.
            if is_safe_favicon_url(&resolved) {
                Some(resolved)
            } else {
                None
            }
        }
        None => origin.map(|o| format!("{o}/favicon.ico")),
    };

    LinkMeta {
        url: url.to_string(),
        title,
        description: None,
        favicon,
        og_image,
        // Filled in by `fetch_link_meta_with_timeout` after this returns —
        // downloading the image is I/O, and this function stays pure so it
        // can be unit-tested with a plain HTML string, no network.
        cover_oid: None,
        cover_ext: None,
        cover_served_path: None,
        cover_width: None,
        cover_height: None,
        cover_lqip: None,
        cover_color: None,
        fetched_at: now_iso8601(),
    }
}

/// True for a URL a build may issue an HTTP(S) request against. Used to
/// gate `og_image`: a favicon may be inlined as `data:`, but a cover image
/// is always downloaded, so anything without an http(s) scheme is simply
/// not a candidate.
fn is_http_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Extract origin (scheme + host) from a URL string.
/// Returns `Some("https://example.com")` for `https://example.com/path`.
fn extract_origin(url: &str) -> Option<String> {
    let (scheme, after_scheme) = url.split_once("://")?;
    // The host runs to whichever of path / query / fragment starts first.
    let host = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        None
    } else {
        Some(format!("{scheme}://{host}"))
    }
}

/// True when `href` already carries a URI scheme (`data:`, `mailto:`,
/// `blob:`, `https:`, ...) per RFC 3986 §3.1: a letter, then letters,
/// digits, `+`, `-`, or `.`, then `:`. Anything matching this is already
/// absolute and must never be joined to an origin. Protocol-relative
/// (`//host/...`) has no scheme and is handled separately.
fn has_uri_scheme(href: &str) -> bool {
    let Some(colon) = href.find(':') else {
        return false;
    };
    let scheme = &href[..colon];
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Raster favicon MIME types kept verbatim as a `data:` URL. SVG is
/// deliberately excluded even though it is `image/*`: an SVG payload can
/// carry `<script>` or an `onload` handler, and while every current
/// browser disables script execution for an `<img>`-context image, that is
/// a property of the browser, not of these bytes — the same fetched page
/// is untrusted regardless.
const ALLOWED_DATA_FAVICON_TYPES: [&str; 5] =
    ["image/png", "image/jpeg", "image/gif", "image/webp", "image/x-icon"];

/// A fetched page's `data:` favicon fully controls this string, and it is
/// copied into every generated page that links there — 8 KiB comfortably
/// fits a 16×16–32×32 icon.
const MAX_DATA_FAVICON_BYTES: usize = 8 * 1024;

/// True for a favicon URL safe to hand to `render_external_card`'s `<img
/// src="…">`: `http(s)://` (already the fetch origin's own scheme, or an
/// absolute URL the page named), or a small `data:` URL of a raster image
/// type with an actual payload. Everything else — `data:,` (the "no
/// favicon" placeholder), `data:image/svg+xml` and `data:text/html`
/// (script-capable payloads), and opaque schemes such as `javascript:`,
/// `file:`, `blob:`, `mailto:` (never a renderable image, `file:` a small
/// local-disclosure risk if the built page is ever opened over `file://`)
/// — comes from an untrusted fetched page and is dropped to `None` rather
/// than reaching the sink.
fn is_safe_favicon_url(url: &str) -> bool {
    if url.starts_with("https://") || url.starts_with("http://") {
        return true;
    }
    let Some(rest) = url.strip_prefix("data:") else {
        return false;
    };
    let Some((media_type, data)) = rest.split_once(',') else {
        return false;
    };
    let mime = media_type.split(';').next().unwrap_or("");
    !data.is_empty()
        && ALLOWED_DATA_FAVICON_TYPES.contains(&mime)
        && url.len() <= MAX_DATA_FAVICON_BYTES
}

/// Resolve a potentially relative URL against an origin.
fn resolve_url(href: &str, origin: &Option<String>) -> String {
    if has_uri_scheme(href) {
        // Already absolute: https:, http:, data:, mailto:, blob:, ... A
        // scheme means the href is opaque and self-contained — joining it
        // onto an origin produced `https://example.org/data:,` for a
        // page's `data:,` favicon placeholder, which 404s in the browser.
        href.to_string()
    } else if href.starts_with("//") {
        // Protocol-relative
        if let Some(ref o) = origin {
            let scheme = o.split("://").next().unwrap_or("https");
            format!("{scheme}:{href}")
        } else {
            format!("https:{href}")
        }
    } else if href.starts_with('/') {
        // Root-relative
        if let Some(ref o) = origin {
            format!("{o}{href}")
        } else {
            href.to_string()
        }
    } else {
        // Relative path — prepend origin + /
        if let Some(ref o) = origin {
            format!("{o}/{href}")
        } else {
            href.to_string()
        }
    }
}

/// Return current time as ISO 8601 string (UTC, second precision).
fn now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Simple UTC timestamp without pulling in chrono
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;

    // Convert days since epoch to Y-M-D (simplified Gregorian)
    let (y, mo, d) = days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Cache directory for link metadata.
fn cache_dir(moss_dir: &Path) -> std::path::PathBuf {
    MossPaths::from_moss_dir(moss_dir.to_path_buf()).cache_link_meta()
}

/// Cache file path for a given URL.
fn cache_path(moss_dir: &Path, url: &str) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(url.as_bytes());
    let hex = bytes_to_hex(&hash);
    cache_dir(moss_dir).join(format!("{hex}.json"))
}

/// Maximum age (in days) for a cached entry to be considered fresh.
const CACHE_FRESHNESS_DAYS: u64 = 7;

/// Check if a cached entry is still fresh (< CACHE_FRESHNESS_DAYS old).
///
/// Uses the same epoch-day algorithm as `now_iso8601` (round-trip consistent).
/// A prior review found the previous arithmetic mixed `m*30+d` which
/// double-counted month boundaries (a Jan 31 → Feb 1 diff read as 0 days,
/// while the threshold "7" was actually 217 because we compared in the same
/// broken metric). Fixed by going through the inverse of `days_to_ymd`.
fn is_fresh(fetched_at: &str) -> bool {
    let cached_days = match parse_iso8601_to_days(fetched_at) {
        Some(d) => d,
        None => return false, // unparseable → treat as stale
    };
    let now_days = epoch_days_now();
    now_days.saturating_sub(cached_days) < CACHE_FRESHNESS_DAYS
}

/// Days since 1970-01-01 for the current system time.
fn epoch_days_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86400)
        .unwrap_or(0)
}

/// Parse "YYYY-MM-DDTHH:MM:SSZ" → days since 1970-01-01.
/// Returns `None` on any parse error. Inverse of `days_to_ymd`.
fn parse_iso8601_to_days(s: &str) -> Option<u64> {
    // `get` rather than `s[0..4]`: the old `len() < 10` guard counted bytes, so
    // a non-ASCII value long enough in bytes reached a mid-character slice.
    let mut parts = s.get(..10)?.split('-');
    let y: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let d: u64 = parts.next()?.parse().ok()?;
    Some(ymd_to_days(y, m, d))
}

/// Convert (year, month, day) to days since 1970-01-01.
/// Inverse of `days_to_ymd`. Algorithm from
/// http://howardhinnant.github.io/date_algorithms.html
fn ymd_to_days(y: u64, m: u64, d: u64) -> u64 {
    if m == 0 || d == 0 {
        return 0;
    }
    let y = if m <= 2 { y.saturating_sub(1) } else { y };
    let era = y / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe).saturating_sub(719468)
}

/// Read cached metadata, returning it if fresh.
fn read_cache(moss_dir: &Path, url: &str) -> Option<LinkMeta> {
    let path = cache_path(moss_dir, url);
    let data = std::fs::read_to_string(&path).ok()?;
    let meta: LinkMeta = serde_json::from_str(&data).ok()?;
    Some(meta)
}

/// Read cached metadata only if it's fresh (< 7 days).
fn read_fresh_cache(moss_dir: &Path, url: &str) -> Option<LinkMeta> {
    let meta = read_cache(moss_dir, url)?;
    if is_fresh(&meta.fetched_at) {
        Some(meta)
    } else {
        None
    }
}

/// Write metadata to cache, atomically. Uses tempfile + rename so a reader
/// (concurrent render or another prewarm worker) can't observe a half-written
/// file. Same pattern as `write_url_list_atomic` further down. Same-FS
/// rename is atomic on POSIX, which is enough here.
pub(crate) fn write_cache(moss_dir: &Path, meta: &LinkMeta) {
    let dir = cache_dir(moss_dir);
    let _ = crate::build::io_utils::create_output_dir_all(&dir);
    let path = cache_path(moss_dir, &meta.url);
    let Ok(json) = serde_json::to_string_pretty(meta) else { return };
    // Tempfile name must collide-avoid across parallel workers writing
    // distinct URLs to distinct final paths — the destination's filename
    // (sha256 hex) is unique per URL, so the tempfile names are too.
    let mut tmp = path.clone();
    let mut tmp_name = path.file_name().map(|s| s.to_os_string()).unwrap_or_default();
    tmp_name.push(format!(".tmp.{}", std::process::id()));
    tmp.set_file_name(tmp_name);
    if std::fs::write(&tmp, &json).is_err() {  // allow:raw_write the temp for this cache's own atomic save, not the output tree
        // allow:unlink the link-meta cache under .moss, not staging
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    // allow:unlink the link-meta cache under .moss, not staging
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Fetch metadata for a URL, using cache if available and fresh.
///
/// Cache location: `{moss_dir}/build/cache/link-meta/{sha256-of-url}.json`
/// Response body size cap for a link-metadata fetch. Title and favicon
/// live in `<head>`, so nothing legitimate needs more; a page that never
/// closes its head or a misconfigured server pointed at a large binary
/// must not tie up a fetch worker copying megabytes it will never parse.
/// Enforced by capping the reader (`Read::take`), not by trusting a
/// `Content-Length` header — an untrusted server can lie about or omit it.
const MAX_LINK_META_RESPONSE_BYTES: u64 = 512 * 1024;

/// Read at most [`MAX_LINK_META_RESPONSE_BYTES`] from `reader`, lossy-UTF8
/// decoded, and whether the read itself succeeded (a genuine I/O error, not
/// hitting the cap, is the only way this is `false`). Split out from
/// [`fetch_link_meta_with_timeout`] so the cap's behavior can be pinned
/// with a plain in-memory reader — no network, no timing — rather than a
/// wall-clock race against a throttled test server.
fn read_capped_body(reader: impl std::io::Read) -> (String, bool) {
    use std::io::Read as _;
    let mut buf = Vec::new();
    let read_ok = reader
        .take(MAX_LINK_META_RESPONSE_BYTES)
        .read_to_end(&mut buf)
        .is_ok();
    (String::from_utf8_lossy(&buf).into_owned(), read_ok)
}

/// Build a ureq agent with `timeout` set THREE ways: the overall per-call
/// timeout (`.timeout`, already in use), plus the connect and read phases
/// individually. Belt and suspenders, not redundant: ureq's overall/connect
/// timeout has been found unreliable against some stalls, so a
/// caller that only trusted `.timeout()` could still hang past the budget
/// it asked for. Neither line alone is proven reliable in every case; the
/// orchestrator (`fetch_all_link_meta_parallel_bounded`) is the actual
/// backstop — it stops WAITING on a stuck worker at the deadline regardless
/// of what ureq does internally. These are the second line, not the fix.
fn ureq_agent_with_timeout(timeout: std::time::Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(timeout)
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .build()
}

/// The bare host from an `http(s)://` URL — same lightweight ad-hoc parsing
/// style as [`extract_origin`] above, not the `url` crate (not already a
/// direct dependency of this crate). Handles a bracketed IPv6-with-port form
/// (`[::1]:8080`) and a plain `host:port`/`host` form; anything else
/// (malformed input) falls through to the raw string, which then simply
/// fails the `IpAddr` parse in [`is_unsafe_fetch_target`] and is treated as
/// an ordinary hostname.
fn bare_host(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or(after_scheme);
    let authority = authority.rsplit('@').next().unwrap_or(authority); // drop userinfo
    match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(rest),
        None => authority.split(':').next().unwrap_or(authority),
    }
}

/// True when `url`'s literal host is private (RFC1918), link-local
/// (169.254.0.0/16 — the address every major cloud provider serves instance
/// metadata from — and its IPv6 twin `fe80::/10`), unspecified, multicast, or
/// unique-local IPv6 (`fc00::/7`).
///
/// Deliberately does NOT include loopback: this crate's own test suite fakes
/// "a real remote server" with a `127.0.0.1` `TcpListener` throughout (every
/// `spawn_*_server` helper in this file and in `html_tests.rs`), so blocking
/// it here would make the fetch this guards untestable in this codebase's
/// own idiom, not just harder to exploit. Loopback is also the narrower
/// threat of the two: reaching the build machine's OWN local services is
/// less valuable to an attacker than reaching its private network or its
/// cloud metadata endpoint, which is what link-local and RFC1918 actually
/// gate.
///
/// A build fetches a URL the VAULT AUTHOR wrote in their own content — on a
/// human's own machine that is not a privilege boundary, since they already
/// have whatever network access this check would otherwise be guarding.
/// This exists for the case this fetch actually runs unattended
/// (`exits_after_build`: a CI runner or a hosted build), where a linked
/// THIRD-PARTY page's own link could otherwise point the build machine's
/// request at its own private network or cloud metadata endpoint. It is a
/// literal-host check only: a hostname that resolves to one of these
/// addresses only at connect time (DNS rebinding), or a redirect `Location`
/// pointing at one, is NOT caught here — closing those needs a custom
/// resolver/connector, more machinery than this proportional guard is
/// trying to be.
fn is_unsafe_fetch_target(url: &str) -> bool {
    let host = bare_host(url);
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => v4.is_private() || v4.is_link_local() || v4.is_unspecified() || v4.is_multicast(),
        Ok(std::net::IpAddr::V6(v6)) => {
            v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // unique-local, fc00::/7
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // link-local unicast, fe80::/10
        }
        Err(_) => false,
    }
}

/// Fetch (or serve from a fresh cache) one URL's link metadata. Cache TTL:
/// 7 days.
///
/// The two callers — [`fetch_all_link_meta_parallel`] (10s per request,
/// [`FETCH_BUDGET`] total) and [`fetch_new_link_meta_for_build`]
/// ([`BUILD_FETCH_PER_REQUEST_TIMEOUT`], [`BUILD_FETCH_BUDGET`] total) —
/// both go through [`fetch_all_link_meta_parallel_bounded`]'s worker pool,
/// so this function itself takes the timeout as a parameter rather than
/// picking one.
///
/// # Visibility
///
/// **`private` only — do not widen.** This function does blocking network
/// I/O (ureq's connect timeout is unreliable and can stall well past the
/// timeout given). Calling it from the build's render path freezes the
/// pipeline; both real callers run it on a background thread pool instead.
fn fetch_link_meta_with_timeout(url: &str, moss_dir: &Path, timeout: std::time::Duration) -> LinkMeta {
    // 1. Check fresh cache
    if let Some(cached) = read_fresh_cache(moss_dir, url) {
        return cached;
    }

    // 2. Refuse an unsafe target rather than fetch it — see `is_unsafe_fetch_target`.
    if is_unsafe_fetch_target(url) {
        return stale_or_empty(moss_dir, url);
    }

    // 3. Try HTTP fetch
    let result = ureq_agent_with_timeout(timeout)
        .get(url)
        .set("User-Agent", "moss/0.1 (+https://moss.sh)")
        .call();

    match result {
        Ok(resp) => {
            let (html, read_ok) = read_capped_body(resp.into_reader());
            if read_ok {
                let mut meta = parse_link_meta(url, &html);
                // Author content always wins over anything fetched — but
                // there's no author content to compare against here, only
                // the fetch itself. "Keep the last successful copy until a
                // refresh succeeds": carry the PREVIOUS entry's cover
                // forward whenever this fetch doesn't produce a new one
                // (no og:image this time, or the download/decode failed),
                // so a transient failure or a page that dropped its
                // og:image doesn't blank out a cover that was working.
                let previous = read_cache(moss_dir, url);
                match meta.og_image.as_deref().and_then(|img| download_og_image(img, moss_dir, timeout)) {
                    Some((oid, ext)) => {
                        meta.cover_oid = Some(oid);
                        meta.cover_ext = Some(ext);
                    }
                    None => {
                        if let Some(prev) = previous {
                            meta.cover_oid = prev.cover_oid;
                            meta.cover_ext = prev.cover_ext;
                            meta.cover_served_path = prev.cover_served_path;
                            meta.cover_width = prev.cover_width;
                            meta.cover_height = prev.cover_height;
                            meta.cover_lqip = prev.cover_lqip;
                            meta.cover_color = prev.cover_color;
                        }
                    }
                }
                write_cache(moss_dir, &meta);
                meta
            } else {
                // Return stale cache or empty
                stale_or_empty(moss_dir, url)
            }
        }
        Err(_) => stale_or_empty(moss_dir, url),
    }
}

/// Response body size cap for a downloaded cover image — generous enough
/// for a real social-preview image, small enough that a misconfigured
/// server pointed at a large file can't tie up a fetch worker.
const MAX_OG_IMAGE_DOWNLOAD_BYTES: u64 = 5 * 1024 * 1024;

/// Decode-time dimension cap for a downloaded cover image. No real
/// social-preview image is anywhere near this large; the point is rejecting
/// a "decompression bomb" — a few compressed bytes whose header claims an
/// enormous canvas — from its declared dimensions alone, before the decoder
/// allocates a pixel buffer for it. `image::Limits::default()`'s 512MiB
/// allocation ceiling alone still lets every one of `FETCH_CONCURRENCY`'s
/// parallel workers hold up to 512MiB decoding one at once; this tightens
/// that ceiling rather than replacing it.
const MAX_OG_IMAGE_DIMENSION: u32 = 10_000;

/// Download `image_url`, verify it's actually a raster image moss's
/// pipeline can re-encode, and store the bytes in the site's
/// content-addressed cache. Returns `(oid, ext)` — `ext` is derived from
/// the SNIFFED format (magic bytes via `image::guess_format`), never
/// trusted from the URL or a `Content-Type` header alone, since both come
/// from an untrusted server. `None` on any failure: non-image content
/// type, oversized body, bytes that don't actually decode, or a target
/// [`is_unsafe_fetch_target`] refuses — every case degrades to "no cover
/// this build" (or the carried-forward previous cover), never a build
/// failure.
fn download_og_image(image_url: &str, moss_dir: &Path, timeout: std::time::Duration) -> Option<(String, String)> {
    if is_unsafe_fetch_target(image_url) {
        return None;
    }
    let resp = ureq_agent_with_timeout(timeout)
        .get(image_url)
        .set("User-Agent", "moss/0.1 (+https://moss.sh)")
        .call()
        .ok()?;
    // Cheap early exit on an explicit non-image Content-Type. Not the
    // authoritative check — a server can omit or lie about it — but avoids
    // downloading a body we're going to discard.
    if let Some(ct) = resp.header("Content-Type") {
        let ct = ct.split(';').next().unwrap_or(ct).trim();
        if !ct.is_empty() && !ct.starts_with("image/") {
            return None;
        }
    }
    let mut buf = Vec::new();
    {
        use std::io::Read as _;
        resp.into_reader()
            .take(MAX_OG_IMAGE_DOWNLOAD_BYTES)
            .read_to_end(&mut buf)
            .ok()?;
    }
    // The authoritative check: sniff the magic bytes and require moss's
    // pipeline to actually be able to decode it. `should_skip`'s CMYK/
    // animated/SVG rejections in the real conversion still apply later —
    // this only rules out "not an image at all" and a truncated download
    // (a cap mid-stream will usually fail to decode as a complete image).
    let format = image::guess_format(&buf).ok()?;
    let ext = match format {
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Png => "png",
        image::ImageFormat::WebP => "webp",
        _ => return None,
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_OG_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_OG_IMAGE_DIMENSION);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(&buf));
    reader.set_format(format);
    reader.limits(limits);
    reader.decode().ok()?;
    let store = crate::build::cache::ObjectStore::for_site(&MossPaths::from_moss_dir(moss_dir.to_path_buf()));
    let oid = store.store_bytes(&buf).ok()?;
    Some((oid, ext.to_string()))
}

/// Return stale cached data if available, otherwise empty LinkMeta.
fn stale_or_empty(moss_dir: &Path, url: &str) -> LinkMeta {
    read_cache(moss_dir, url).unwrap_or(LinkMeta {
        url: url.to_string(),
        title: None,
        description: None,
        favicon: None,
        og_image: None,
        cover_oid: None,
        cover_ext: None,
        cover_served_path: None,
        cover_width: None,
        cover_height: None,
        cover_lqip: None,
        cover_color: None,
        fetched_at: now_iso8601(),
    })
}

/// Number of concurrent in-flight fetches for parallel prewarm.
///
/// 8 is the sweet spot: high enough that ~30 typical URLs finish in roughly
/// one slow-host's worth of wall time, low enough that we don't open more
/// sockets than a residential ISP appreciates. Bumping this won't help when
/// the latency floor is server-side rate limits per origin: a single slow
/// host with 8+ URLs in the set still serializes through that host's
/// throughput.
const FETCH_CONCURRENCY: usize = 8;

/// Total wall-time budget for the parallel prewarm. Workers stop pulling new
/// URLs from the queue once the budget is exceeded; in-flight requests are
/// not interrupted (ureq lacks a cooperative cancel API), but the queue
/// drains quickly past that point. Bounds worst-case CLI hang on a long URL
/// list with no network from `(N / 8) * 30s` (per-URL ureq stall) down to
/// this value. URLs not fetched this build are picked up next build.
const FETCH_BUDGET: std::time::Duration = std::time::Duration::from_secs(60);

/// Fetch metadata for multiple URLs in parallel. Skips URLs whose cache is
/// already fresh, so calling this repeatedly is cheap (no network). Returns
/// the count of URLs that were actually fetched (i.e. cold or stale entries).
///
/// Used by the CLI build path to warm the cache *before* render — see
/// [`prewarm_link_meta_for_build`]. Render then reads from a warm cache.
///
/// # Visibility
///
/// **`pub(crate)` only — do not widen.** Same blocking-I/O constraint as
/// `fetch_link_meta_with_timeout`.
pub(crate) fn fetch_all_link_meta_parallel(urls: &[&str], moss_dir: &Path) -> usize {
    fetch_all_link_meta_parallel_bounded(
        urls,
        moss_dir,
        FETCH_BUDGET,
        std::time::Duration::from_secs(10),
        "prewarm",
    )
}

/// Shared worker pool behind both [`fetch_all_link_meta_parallel`] (the
/// next-build prewarm, generous budget) and
/// [`fetch_new_link_meta_for_build`] (this build's own short-budget fetch).
/// `log_label` names the caller in the budget-exceeded warning, so a log
/// line can tell which pass ran out of time.
/// Enforces `budget` in the ORCHESTRATOR, not per request. A prior version
/// of this function used `std::thread::scope`, which JOINS every worker
/// before returning — so a single stuck fetch (ureq's own timeout is
/// unreliable, and a blackholed public host can stall ~30s past whatever
/// timeout it was given) held the whole call hostage regardless of
/// `per_request_timeout`, defeating `budget` entirely. This version spawns
/// detached workers (plain `std::thread::spawn`, never joined) that each
/// send a completion signal over a channel as they finish; the orchestrator
/// only waits on that channel via `recv_timeout`, so it returns at the
/// deadline no matter how long a straggler worker keeps running.
///
/// A straggler is ABANDONED for this build's purposes: its result never
/// reaches this function's return value, so the render that follows never
/// sees it. But `fetch_link_meta_with_timeout` writes the cache internally
/// as its very last step before returning — a straggler that eventually
/// finishes (while the process is still alive; a one-shot CLI build that
/// exits right after this call kills it first) still leaves a fresh cache
/// entry for whatever reads it next, exactly like a URL this budget never
/// reached at all. Nothing needs to special-case that: it falls out of
/// simply not cancelling the thread (ureq has no cancellation API, and Rust
/// threads can't be force-killed either).
fn fetch_all_link_meta_parallel_bounded(
    urls: &[&str],
    moss_dir: &Path,
    budget: std::time::Duration,
    per_request_timeout: std::time::Duration,
    log_label: &'static str,
) -> usize {
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Instant;

    // Filter out URLs that are already cached fresh — those need no fetch.
    let cold: Vec<String> = urls
        .iter()
        .filter(|u| read_fresh_cache(moss_dir, u).is_none())
        .map(|u| (*u).to_string())
        .collect();

    if cold.is_empty() {
        return 0;
    }

    let queue = Arc::new(Mutex::new(cold.clone().into_iter()));
    let moss_dir_owned = moss_dir.to_path_buf();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let worker_count = FETCH_CONCURRENCY.min(cold.len());

    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let moss_dir = moss_dir_owned.clone();
        let done_tx = done_tx.clone();
        // Not joined, not scoped: a worker that outlives this function's
        // deadline is exactly the case this design exists to not wait on.
        std::thread::spawn(move || {
            loop {
                let next = match queue.lock() {
                    Ok(mut q) => q.next(),
                    // Mutex poisoning means a sibling worker panicked. Bail
                    // quietly rather than propagating a poison panic here —
                    // the orchestrator's own budget-exceeded warning below
                    // already reports an incomplete run.
                    Err(_) => break,
                };
                let Some(url) = next else { break };
                let _ = fetch_link_meta_with_timeout(&url, &moss_dir, per_request_timeout);
                // A send error means the receiver was already dropped (the
                // orchestrator returned at its deadline) — that IS the
                // abandonment this function provides; nothing to do about it.
                let _ = done_tx.send(());
            }
        });
    }
    // Drop our own sender: once every worker's clone is ALSO dropped (they
    // all reach the end of their loop), `recv`/`recv_timeout` correctly
    // reports the channel as disconnected instead of hanging forever.
    drop(done_tx);

    let deadline = Instant::now() + budget;
    let mut completed = 0usize;
    while completed < cold.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match done_rx.recv_timeout(remaining) {
            Ok(()) => completed += 1,
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            // Every worker finished (queue drained) before the deadline —
            // not the timeout path, but the same "nothing left to wait for"
            // outcome.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if completed < cold.len() {
        log::warn!(
            target: "link-meta",
            "{log_label} budget ({:?}) exceeded: {} of {} cold URLs fetched before the deadline; \
             stragglers keep running and may still warm the cache for a later reader, but this build won't wait on them",
            budget, completed, cold.len()
        );
    }
    completed
}

/// Total wall-time budget for THIS build's own link-metadata fetch — the
/// short pass that runs before render so a card can be complete on the
/// very build that introduces its link, rather than waiting for the next
/// build's prewarm to catch up. Deliberately much shorter than
/// [`FETCH_BUDGET`] (60s): this one is on the critical path of every
/// build, prewarm is a best-effort catch-up for URLs already known from a
/// previous build's `.urls.json`.
pub(crate) const BUILD_FETCH_BUDGET: std::time::Duration = std::time::Duration::from_secs(3);

/// Per-request timeout for the build-time fetch. Shorter than the prewarm
/// path's 10s: on a 3s total budget, one slow host must not be allowed to
/// spend the whole thing.
const BUILD_FETCH_PER_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

/// Fetch metadata for external grid-cell links this build's own pages
/// introduce, before render — so a card is complete on its first build
/// instead of showing the no-metadata form until the next one. `urls` is
/// this build's candidate set (gathered from the already-parsed documents;
/// see `build::render::grid_cells`); a URL with no fresh cache entry is
/// fetched, under [`BUILD_FETCH_BUDGET`] total and
/// [`BUILD_FETCH_PER_REQUEST_TIMEOUT`] per request.
///
/// A URL the budget doesn't reach, or that fails/times out, is left cold —
/// its card renders in the no-metadata form, and `record_urls_for_prewarm`
/// (called by render as usual) still records it, so the *next* build's
/// prewarm picks it up exactly as it always has. This function only closes
/// the one-build lag for the common case (a fast, reachable host); it
/// never removes the fallback.
///
/// # Visibility
///
/// **`pub(crate)` only — do not widen.** Same blocking-I/O constraint as
/// `fetch_link_meta_with_timeout`.
pub(crate) fn fetch_new_link_meta_for_build(urls: &[&str], moss_dir: &Path) -> usize {
    fetch_all_link_meta_parallel_bounded(
        urls,
        moss_dir,
        BUILD_FETCH_BUDGET,
        BUILD_FETCH_PER_REQUEST_TIMEOUT,
        "build-time link-meta fetch",
    )
}

/// Synchronously prewarm the link-meta cache for the persisted URL list,
/// blocking until all stale or cold entries have been (re)fetched.
///
/// Intended for the CLI/headless build path, where the process exits soon
/// after render and there's no long-lived runtime to host
/// [`crate::build::features::sync::spawn_native_process_sync`]'s background
/// task. Calling this *before* render means render reads a warm cache.
///
/// First-ever build for a folder is always cold: `.urls.json` doesn't exist
/// yet, so this returns `0`. The build runs cold, render records URLs, the
/// next CLI invocation finds `.urls.json` and prewarms successfully.
///
/// # Returns
/// `(urls_seen, urls_fetched)`. `urls_seen` is the size of `.urls.json`;
/// `urls_fetched` is the number of cold/stale entries we actually hit the
/// network for. Difference = entries served from fresh cache.
pub(crate) fn prewarm_link_meta_for_build(moss_dir: &Path) -> (usize, usize) {
    prewarm_link_meta_for_build_with_progress(moss_dir, |_| {})
}

/// Same as [`prewarm_link_meta_for_build`], but invokes `on_cold_count`
/// with the number of cold/stale URLs *before* the network phase begins.
/// The callback fires once and only when the URL list is non-empty; callers
/// use it to print progress before a potentially slow network round-trip.
pub(crate) fn prewarm_link_meta_for_build_with_progress(
    moss_dir: &Path,
    on_cold_count: impl FnOnce(usize),
) -> (usize, usize) {
    let urls = load_persisted_url_list(moss_dir);
    if urls.is_empty() {
        return (0, 0);
    }
    let cold_count = urls
        .iter()
        .filter(|u| read_fresh_cache(moss_dir, u).is_none())
        .count();
    on_cold_count(cold_count);
    let url_refs: Vec<&str> = urls.iter().map(String::as_str).collect();
    let fetched = fetch_all_link_meta_parallel(&url_refs, moss_dir);
    (urls.len(), fetched)
}

// ---------------------------------------------------------------------------
// Render-side API: cache reads + per-build URL recording for prewarm bridge.
//
// Design (from a prior review's feedback):
//
// 1. Render is a pure cache reader — `read_link_meta_from_cache` has NO
//    side effects, so two pages racing don't lose-update a shared file.
//
// 2. URL recording is explicit and per-build — `record_urls_for_prewarm` adds
//    to an in-memory session set. `start_build_session` clears it; render
//    populates it; `flush_build_session` writes the set to disk atomically.
//    This means the persisted list contains exactly THIS build's URLs, not
//    a union with prior builds' (no unbounded growth, removed URLs vanish).
//
// 3. The bridge file lives in `.moss/cache/link-meta/.urls.json` next to
//    the cache it indexes — clearing the cache (`rm -rf .moss/cache/`) also
//    clears the bridge, so they can never disagree about "what's available".
//
// 4. Render returns `read_cache` (allows stale entries) — first build with a
//    new URL renders the cached title even if expired, while the next build's
//    sync prewarm refreshes it. Better degraded UX than refusing to show.
// ---------------------------------------------------------------------------

use std::sync::Mutex;

/// In-memory accumulator for URLs encountered during the current build.
/// Cleared by `start_build_session` at run_pipeline entry; flushed to disk
/// by `flush_build_session` at run_pipeline exit (or near it).
///
/// Keyed by `moss_dir.to_string_lossy()` so multiple folders open in the
/// same process don't trample each other.
fn build_session() -> &'static Mutex<HashMap<String, std::collections::BTreeSet<String>>> {
    use std::sync::OnceLock;
    static SESSION: OnceLock<Mutex<HashMap<String, std::collections::BTreeSet<String>>>> =
        OnceLock::new();
    SESSION.get_or_init(|| Mutex::new(HashMap::new()))
}

fn session_key(moss_dir: &Path) -> String {
    moss_dir.to_string_lossy().to_string()
}

/// Begin a build session — clear any prior URL accumulator for this folder.
/// Call once at the top of `run_pipeline` (or before render starts).
pub fn start_build_session(moss_dir: &Path) {
    let key = session_key(moss_dir);
    if let Ok(mut g) = build_session().lock() {
        g.entry(key).or_default().clear();
    }
}

/// Record URLs encountered during render, to be prewarmed for the next build.
/// Called by render once per page that has bare-URL grids. Pure in-memory,
/// no I/O — so concurrent calls are safe (Mutex synchronizes the insert).
pub fn record_urls_for_prewarm(urls: &[&str], moss_dir: &Path) {
    let key = session_key(moss_dir);
    if let Ok(mut g) = build_session().lock() {
        let entry = g.entry(key).or_default();
        for u in urls {
            entry.insert((*u).to_string());
        }
    }
}

/// Flush the current build's URL set to the bridge file (atomic write).
/// Returns the number of URLs written. Call once at the end of `run_pipeline`,
/// after all pages have rendered. Best-effort: errors are logged at warn.
pub fn flush_build_session(moss_dir: &Path) -> usize {
    let key = session_key(moss_dir);
    let urls: Vec<String> = match build_session().lock() {
        Ok(g) => g.get(&key).map(|s| s.iter().cloned().collect()).unwrap_or_default(),
        Err(_) => return 0,
    };
    if urls.is_empty() {
        // Don't write an empty file — could mask "I'm in a session that hasn't
        // recorded anything yet" vs "this build genuinely has no URLs". Just
        // leave whatever is on disk; sync will work from the prior list.
        return 0;
    }
    write_url_list_atomic(moss_dir, &urls);
    urls.len()
}

/// Path to the persisted URL list (bridge between render-side recording and
/// sync-side prewarming). Lives under `.moss/cache/link-meta/` (next to the
/// per-URL cache files), so wiping the cache clears the bridge too.
///
/// The dot-prefix avoids any collision with the SHA256-hex-named cache files
/// (which are 64 hex chars + ".json"; the dot keeps `.urls.json` distinct).
pub(crate) fn url_list_path(moss_dir: &Path) -> std::path::PathBuf {
    cache_dir(moss_dir).join(".urls.json")
}

/// Atomic write: serialize to a tempfile in the same directory, then rename.
/// `rename` within the same filesystem is atomic on POSIX, so a crash mid-
/// write can't leave a half-written JSON for sync to choke on.
fn write_url_list_atomic(moss_dir: &Path, urls: &[String]) {
    let path = url_list_path(moss_dir);
    let parent = match path.parent() {
        Some(p) => p,
        None => return,
    };
    if let Err(e) = crate::build::io_utils::create_output_dir_all(parent) {
        log::warn!(target: "link-meta", "could not create {:?}: {}", parent, e);
        return;
    }
    let json = match serde_json::to_string_pretty(urls) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(target: "link-meta", "serialize url list failed: {}", e);
            return;
        }
    };
    // Tempfile name colocated with target so rename stays on the same FS.
    let tmp = parent.join(format!(".urls.json.tmp.{}", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, &json) {  // allow:raw_write the temp for this cache's own atomic save, not the output tree
        log::warn!(target: "link-meta", "write tempfile failed: {}", e);
        return;
    }
    // allow:unlink the link-meta URL list under .moss, not staging
    if let Err(e) = std::fs::rename(&tmp, &path) {
        log::warn!(target: "link-meta", "rename tempfile failed: {}", e);
        // allow:unlink the link-meta URL list under .moss, not staging
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Read the cached metadata for a set of URLs. Pure cache reader, no I/O
/// other than disk reads of the existing cache files; never makes HTTP calls.
///
/// For URLs not in the cache, returns an empty `LinkMeta` (URL only, no title)
/// so render can still produce a card with the URL displayed; the next build's
/// sync prewarm populates the cache so the build after that renders fully.
///
/// **Returns stale entries too** (no freshness gate). Rationale: a stale title
/// is much better UX than "no title". The background prewarm refreshes stale
/// entries on its next run, so staleness is bounded by sync cadence
/// (currently every build cycle that triggers `spawn_native_process_sync`).
pub fn read_link_meta_from_cache(urls: &[&str], moss_dir: &Path) -> HashMap<String, LinkMeta> {
    let mut map = HashMap::new();
    let mut missing = 0usize;
    for url in urls {
        match read_cache(moss_dir, url) {
            Some(meta) => {
                map.insert(url.to_string(), meta);
            }
            None => {
                missing += 1;
                map.insert(
                    url.to_string(),
                    LinkMeta {
                        url: url.to_string(),
                        title: None,
                        description: None,
                        favicon: None,
                        og_image: None,
                        cover_oid: None,
                        cover_ext: None,
                        cover_served_path: None,
                        cover_width: None,
                        cover_height: None,
                        cover_lqip: None,
                        cover_color: None,
                        fetched_at: now_iso8601(),
                    },
                );
            }
        }
    }
    if missing > 0 {
        // User-visible signal — without this, "link previews look broken on
        // first build" has no breadcrumb. Once the background sync runs, the
        // next build will render with full metadata.
        log::info!(
            target: "link-meta",
            "{} URL(s) not yet in cache (will appear on next build after background prewarm)",
            missing
        );
    }
    map
}

/// Load the URL list previously written by `flush_build_session`.
/// Used by the background sync to know which URLs to pre-warm.
pub fn load_persisted_url_list(moss_dir: &Path) -> Vec<String> {
    let path = url_list_path(moss_dir);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_og_title() {
        let html = r#"
        <html><head>
            <meta property="og:title" content="My Cool Site">
            <meta property="og:description" content="A description of the site">
            <title>Fallback Title</title>
        </head><body></body></html>
        "#;
        let meta = parse_link_meta("https://example.com", html);
        assert_eq!(meta.title.as_deref(), Some("My Cool Site"));
        // Description is intentionally not parsed for new entries (renderer
        // ignores it; saves bandwidth + bytes per cache file).
        assert_eq!(meta.description, None);
    }

    #[test]
    fn test_parse_og_image() {
        let html = r#"
        <html><head>
            <meta property="og:image" content="https://example.com/cover.jpg">
        </head><body></body></html>
        "#;
        let meta = parse_link_meta("https://example.com", html);
        assert_eq!(meta.og_image.as_deref(), Some("https://example.com/cover.jpg"));
    }

    #[test]
    fn test_parse_og_image_relative_resolves_against_origin() {
        let html = r#"
        <html><head>
            <meta property="og:image" content="/assets/cover.png">
        </head></html>
        "#;
        let meta = parse_link_meta("https://example.com/post", html);
        assert_eq!(meta.og_image.as_deref(), Some("https://example.com/assets/cover.png"));
    }

    #[test]
    fn test_parse_twitter_image_fallback_when_no_og_image() {
        let html = r#"
        <html><head>
            <meta name="twitter:image" content="https://example.com/twitter-card.jpg">
        </head></html>
        "#;
        let meta = parse_link_meta("https://example.com", html);
        assert_eq!(meta.og_image.as_deref(), Some("https://example.com/twitter-card.jpg"));
    }

    #[test]
    fn test_parse_og_image_prefers_og_over_twitter() {
        let html = r#"
        <html><head>
            <meta property="og:image" content="https://example.com/og.jpg">
            <meta name="twitter:image" content="https://example.com/twitter.jpg">
        </head></html>
        "#;
        let meta = parse_link_meta("https://example.com", html);
        assert_eq!(meta.og_image.as_deref(), Some("https://example.com/og.jpg"));
    }

    #[test]
    fn test_parse_og_image_absent_when_no_meta_tag() {
        let html = "<html><head><title>No cover here</title></head></html>";
        let meta = parse_link_meta("https://example.com", html);
        assert_eq!(meta.og_image, None);
    }

    #[test]
    fn test_parse_og_image_data_url_is_not_a_fetch_candidate() {
        // Unlike a favicon, a cover image is never inlined — the build
        // downloads it, and a `data:` URL is not something to download.
        let html = r#"
        <html><head>
            <meta property="og:image" content="data:image/png;base64,iVBORw0KGgo=">
        </head></html>
        "#;
        let meta = parse_link_meta("https://example.com", html);
        assert_eq!(meta.og_image, None);
    }

    #[test]
    fn test_parse_fallback_to_title_tag() {
        let html = r#"
        <html><head>
            <title>Page Title</title>
        </head><body></body></html>
        "#;
        let meta = parse_link_meta("https://example.com", html);
        assert_eq!(meta.title.as_deref(), Some("Page Title"));
        assert_eq!(meta.description, None);
    }

    #[test]
    fn test_parse_does_not_extract_description() {
        // Even when a page provides og:description AND <meta name="description">,
        // we no longer extract them — `description` is always `None` on new
        // entries. The field stays in the struct only for backward-compat
        // deserialization of cache files written by older moss versions.
        let html = r#"
        <html><head>
            <meta property="og:title" content="OG Title">
            <meta property="og:description" content="OG description">
            <meta name="description" content="Meta desc fallback">
        </head><body></body></html>
        "#;
        let meta = parse_link_meta("https://example.com", html);
        assert_eq!(meta.title.as_deref(), Some("OG Title"));
        assert_eq!(meta.description, None);
    }

    #[test]
    fn test_parse_empty_html() {
        let meta = parse_link_meta("https://example.com", "");
        assert_eq!(meta.title, None);
        assert_eq!(meta.description, None);

        let meta2 = parse_link_meta("https://example.com", "<html><body>no head</body></html>");
        assert_eq!(meta2.title, None);
        assert_eq!(meta2.description, None);
    }

    #[test]
    fn test_cache_write_and_read() {
        let tmp = std::env::temp_dir().join("moss_link_meta_test_cache");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let meta = LinkMeta {
            url: "https://example.com".to_string(),
            title: Some("Test Title".to_string()),
            description: Some("Test Desc".to_string()),
            favicon: None,
            og_image: None,
            cover_oid: None,
            cover_ext: None,
            cover_served_path: None,
            cover_width: None,
            cover_height: None,
            cover_lqip: None,
            cover_color: None,
            fetched_at: now_iso8601(),
        };

        write_cache(&tmp, &meta);
        let read_back = read_cache(&tmp, "https://example.com");
        assert!(read_back.is_some());
        let read_back = read_back.unwrap();
        assert_eq!(read_back.title.as_deref(), Some("Test Title"));
        assert_eq!(read_back.description.as_deref(), Some("Test Desc"));

        // Fresh cache should be returned
        let fresh = read_fresh_cache(&tmp, "https://example.com");
        assert!(fresh.is_some());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Use a unique tempdir per test so concurrent test runs (and the global
    /// `build_session()` Mutex) don't trample each other.
    fn unique_tmp(label: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "moss_link_meta_{}_{}_{}",
            label,
            std::process::id(),
            // Nanos give per-test uniqueness even within the same process.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn read_link_meta_from_cache_returns_cached_or_empty_no_io_side_effect() {
        let tmp = unique_tmp("read");

        // Pre-populate cache for one URL only.
        let cached = LinkMeta {
            url: "https://cached.example".to_string(),
            title: Some("Cached".to_string()),
            description: None,
            favicon: None,
            og_image: None,
            cover_oid: None,
            cover_ext: None,
            cover_served_path: None,
            cover_width: None,
            cover_height: None,
            cover_lqip: None,
            cover_color: None,
            fetched_at: now_iso8601(),
        };
        write_cache(&tmp, &cached);

        let urls = vec!["https://cached.example", "https://uncached.example"];
        let result = read_link_meta_from_cache(&urls, &tmp);

        assert_eq!(
            result.get("https://cached.example").and_then(|m| m.title.as_deref()),
            Some("Cached")
        );
        assert_eq!(
            result.get("https://uncached.example").and_then(|m| m.title.as_deref()),
            None,
            "uncached URL gets empty meta (URL-only card, not a hang or panic)"
        );

        // The bridge file must NOT exist — render-side reads have no side effect.
        assert!(
            !url_list_path(&tmp).exists(),
            "read_link_meta_from_cache must not write the bridge file"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_session_truncates_then_accumulates_then_flushes() {
        let tmp = unique_tmp("session");

        // Stale URL from a prior build still on disk — should be replaced, not merged.
        let stale_path = url_list_path(&tmp);
        std::fs::create_dir_all(stale_path.parent().unwrap()).unwrap();
        std::fs::write(&stale_path, r#"["https://removed-from-content.example"]"#).unwrap();

        start_build_session(&tmp);
        record_urls_for_prewarm(&["https://page1.example", "https://shared.example"], &tmp);
        record_urls_for_prewarm(&["https://page2.example", "https://shared.example"], &tmp);
        let n = flush_build_session(&tmp);
        assert_eq!(n, 3, "deduped: page1 + page2 + shared (the stale URL must be gone)");

        let loaded = load_persisted_url_list(&tmp);
        let loaded_set: std::collections::HashSet<_> = loaded.iter().map(|s| s.as_str()).collect();
        assert!(loaded_set.contains("https://page1.example"));
        assert!(loaded_set.contains("https://page2.example"));
        assert!(loaded_set.contains("https://shared.example"));
        assert!(
            !loaded_set.contains("https://removed-from-content.example"),
            "removed URLs MUST NOT persist (no unbounded growth)"
        );
        assert_eq!(loaded.len(), 3);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn flush_with_no_urls_keeps_prior_list_intact() {
        let tmp = unique_tmp("emptyflush");

        // Establish a prior list.
        start_build_session(&tmp);
        record_urls_for_prewarm(&["https://stable.example"], &tmp);
        flush_build_session(&tmp);

        // Now a build with zero URLs (e.g. user removed all link previews).
        // We deliberately leave the prior list intact rather than
        // overwriting with empty — sync still has something to prewarm,
        // and the next build that DOES record URLs replaces cleanly.
        start_build_session(&tmp);
        let n = flush_build_session(&tmp);
        assert_eq!(n, 0);

        let loaded = load_persisted_url_list(&tmp);
        assert_eq!(loaded, vec!["https://stable.example"]);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn malformed_bridge_file_is_treated_as_empty() {
        let tmp = unique_tmp("malformed");
        let path = url_list_path(&tmp);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not a valid json [{ url:").unwrap();

        let loaded = load_persisted_url_list(&tmp);
        assert_eq!(loaded, Vec::<String>::new(), "malformed JSON yields empty, never panics");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn special_chars_in_urls_round_trip_through_bridge_file() {
        let tmp = unique_tmp("special");
        start_build_session(&tmp);
        record_urls_for_prewarm(
            &[
                "https://example.com/path?q=hello%20world&x=\"quoted\"",
                "https://example.com/path/with/中文/字符",
                "https://example.com/path/with\\backslash",
            ],
            &tmp,
        );
        flush_build_session(&tmp);

        let loaded = load_persisted_url_list(&tmp);
        assert_eq!(loaded.len(), 3);
        let set: std::collections::HashSet<_> = loaded.iter().map(|s| s.as_str()).collect();
        assert!(set.contains("https://example.com/path?q=hello%20world&x=\"quoted\""));
        assert!(set.contains("https://example.com/path/with/中文/字符"));
        assert!(set.contains("https://example.com/path/with\\backslash"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ymd_to_days_round_trips_with_days_to_ymd() {
        // Exercise some boundaries: leap year, month transitions, century rule.
        let cases = [
            (1970, 1, 1),
            (1970, 1, 2),
            (1970, 2, 1),
            (1970, 12, 31),
            (1971, 1, 1),
            (2000, 2, 29), // leap (divisible by 400)
            (2024, 2, 29),
            (2026, 4, 26),
        ];
        for &(y, m, d) in &cases {
            let days = ymd_to_days(y, m, d);
            let (y2, m2, d2) = days_to_ymd(days);
            assert_eq!((y2, m2, d2), (y, m, d), "round-trip failed for {y}-{m}-{d}");
        }
    }

    #[test]
    fn is_fresh_is_actually_seven_days() {
        // Build a "now" string and a "now - 6 days" string, both should be fresh.
        // Then a "now - 8 days" should be stale.
        let now_d = epoch_days_now();

        let to_iso = |d: u64| -> String {
            let (y, m, day) = days_to_ymd(d);
            format!("{y:04}-{m:02}-{day:02}T00:00:00Z")
        };

        assert!(is_fresh(&to_iso(now_d)), "today is fresh");
        assert!(is_fresh(&to_iso(now_d.saturating_sub(6))), "6 days old is fresh");
        assert!(!is_fresh(&to_iso(now_d.saturating_sub(8))), "8 days old is stale");
        assert!(!is_fresh(&to_iso(now_d.saturating_sub(365))), "1 year old is stale");
        assert!(!is_fresh("not a timestamp"), "garbage is stale");
    }

    #[test]
    fn test_cache_staleness() {
        let tmp = std::env::temp_dir().join("moss_link_meta_test_stale");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let meta = LinkMeta {
            url: "https://example.com/old".to_string(),
            title: Some("Old Title".to_string()),
            description: None,
            favicon: None,
            og_image: None,
            cover_oid: None,
            cover_ext: None,
            cover_served_path: None,
            cover_width: None,
            cover_height: None,
            cover_lqip: None,
            cover_color: None,
            fetched_at: "2020-01-01T00:00:00Z".to_string(), // Very old
        };

        write_cache(&tmp, &meta);

        // Fresh cache should be None (stale)
        let fresh = read_fresh_cache(&tmp, "https://example.com/old");
        assert!(fresh.is_none());

        // But raw read_cache should still return it
        let raw = read_cache(&tmp, "https://example.com/old");
        assert!(raw.is_some());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_favicon_from_link_rel_icon() {
        let html = r#"
        <html><head>
            <link rel="icon" href="https://example.com/icon.png">
            <title>Test</title>
        </head><body></body></html>
        "#;
        let meta = parse_link_meta("https://example.com", html);
        assert_eq!(meta.favicon.as_deref(), Some("https://example.com/icon.png"));
    }

    #[test]
    fn test_favicon_shortcut_icon_relative() {
        let html = r#"
        <html><head>
            <link rel="shortcut icon" href="/favicon.ico">
            <title>Test</title>
        </head><body></body></html>
        "#;
        let meta = parse_link_meta("https://example.com/some/page", html);
        assert_eq!(meta.favicon.as_deref(), Some("https://example.com/favicon.ico"));
    }

    #[test]
    fn test_favicon_fallback_when_no_link_tag() {
        let html = r#"
        <html><head><title>Test</title></head><body></body></html>
        "#;
        let meta = parse_link_meta("https://example.com/page", html);
        assert_eq!(meta.favicon.as_deref(), Some("https://example.com/favicon.ico"));
    }

    #[test]
    fn test_favicon_protocol_relative() {
        let html = r#"
        <html><head>
            <link rel="icon" href="//cdn.example.com/icon.png">
        </head><body></body></html>
        "#;
        let meta = parse_link_meta("https://example.com", html);
        assert_eq!(meta.favicon.as_deref(), Some("https://cdn.example.com/icon.png"));
    }

    // `resolve_url` unit tests: a `data:` (or any other scheme-carrying)
    // href is already absolute and must never be joined to the origin —
    // that join produced the malformed `https://example.org/data:,`, a
    // real 404 seen for pages that declare `<link rel="icon" href="data:,">`
    // as a "no favicon" placeholder.
    #[test]
    fn resolve_url_data_scheme_is_not_joined_to_origin() {
        let origin = Some("https://example.org".to_string());
        assert_eq!(resolve_url("data:,", &origin), "data:,");
    }

    #[test]
    fn resolve_url_data_image_base64_is_not_joined_to_origin() {
        let origin = Some("https://example.org".to_string());
        let data_url = "data:image/png;base64,iVBORw0KGgo=";
        assert_eq!(resolve_url(data_url, &origin), data_url);
    }

    #[test]
    fn resolve_url_absolute_https_is_unchanged() {
        let origin = Some("https://example.org".to_string());
        assert_eq!(
            resolve_url("https://cdn.example.com/icon.png", &origin),
            "https://cdn.example.com/icon.png"
        );
    }

    #[test]
    fn resolve_url_protocol_relative_takes_scheme_from_origin() {
        let origin = Some("https://example.org".to_string());
        assert_eq!(
            resolve_url("//cdn.example.com/icon.png", &origin),
            "https://cdn.example.com/icon.png"
        );
    }

    #[test]
    fn resolve_url_root_relative_is_joined_to_origin() {
        let origin = Some("https://example.org".to_string());
        assert_eq!(resolve_url("/favicon.ico", &origin), "https://example.org/favicon.ico");
    }

    #[test]
    fn resolve_url_relative_path_is_joined_to_origin() {
        let origin = Some("https://example.org".to_string());
        assert_eq!(resolve_url("icon.png", &origin), "https://example.org/icon.png");
    }

    #[test]
    fn test_favicon_empty_data_url_placeholder_yields_no_favicon() {
        // Some sites declare `data:,` (empty payload) as a deliberate
        // "we have no favicon, don't bother guessing /favicon.ico either"
        // signal. Render must produce no <img> at all — same as when a
        // page has no <link rel="icon"> and (unlike that case) we must
        // not fall back to guessing /favicon.ico, since the site already
        // told us explicitly there is nothing to show.
        let html = r#"
        <html><head>
            <link rel="icon" href="data:,">
            <title>Test</title>
        </head><body></body></html>
        "#;
        let meta = parse_link_meta("https://example.org/page", html);
        assert_eq!(meta.favicon, None);
    }

    #[test]
    fn test_favicon_data_image_base64_is_kept_verbatim() {
        let html = r#"
        <html><head>
            <link rel="icon" href="data:image/png;base64,iVBORw0KGgo=">
        </head><body></body></html>
        "#;
        let meta = parse_link_meta("https://example.org/page", html);
        assert_eq!(
            meta.favicon.as_deref(),
            Some("data:image/png;base64,iVBORw0KGgo=")
        );
    }

    // The favicon string is copied verbatim into `<img src="…">` on every
    // page that links out to the fetched site (`render_external_card`). A
    // fetched page is untrusted input, so any scheme other than http(s) or
    // a small raster `data:` image must be dropped to `None` rather than
    // handed to the sink — `has_uri_scheme`/`resolve_url` only decide
    // whether to join to an origin, not whether the result is safe to emit.
    #[test]
    fn favicon_javascript_scheme_is_dropped() {
        let html = r#"<html><head><link rel="icon" href="javascript:alert(1)"></head></html>"#;
        let meta = parse_link_meta("https://example.org/page", html);
        assert_eq!(meta.favicon, None);
    }

    #[test]
    fn favicon_file_scheme_is_dropped() {
        let html = r#"<html><head><link rel="icon" href="file:///etc/passwd"></head></html>"#;
        let meta = parse_link_meta("https://example.org/page", html);
        assert_eq!(meta.favicon, None);
    }

    #[test]
    fn favicon_mailto_scheme_is_dropped() {
        let html = r#"<html><head><link rel="icon" href="mailto:x@example.org"></head></html>"#;
        let meta = parse_link_meta("https://example.org/page", html);
        assert_eq!(meta.favicon, None);
    }

    #[test]
    fn favicon_data_svg_is_dropped() {
        // SVG can carry `<script>`/event-handler content; excluded even
        // though it is `image/*` — see `is_safe_favicon_url`.
        let html = r#"<html><head><link rel="icon" href="data:image/svg+xml,<svg onload=alert(1)>"></head></html>"#;
        let meta = parse_link_meta("https://example.org/page", html);
        assert_eq!(meta.favicon, None);
    }

    #[test]
    fn favicon_data_text_html_is_dropped() {
        let html = r#"<html><head><link rel="icon" href="data:text/html,<script>alert(1)</script>"></head></html>"#;
        let meta = parse_link_meta("https://example.org/page", html);
        assert_eq!(meta.favicon, None);
    }

    #[test]
    fn favicon_oversized_data_url_is_dropped() {
        let huge = "A".repeat(9 * 1024);
        let html = format!(
            r#"<html><head><link rel="icon" href="data:image/png;base64,{huge}"></head></html>"#
        );
        let meta = parse_link_meta("https://example.org/page", &html);
        assert_eq!(meta.favicon, None);
    }

    #[test]
    fn is_safe_favicon_url_accepts_http_https_and_small_raster_data_urls() {
        assert!(is_safe_favicon_url("https://example.org/icon.png"));
        assert!(is_safe_favicon_url("http://example.org/icon.png"));
        assert!(is_safe_favicon_url("data:image/png;base64,iVBORw0KGgo="));
        assert!(is_safe_favicon_url("data:image/x-icon;base64,AA=="));
    }

    #[test]
    fn is_safe_favicon_url_rejects_non_raster_and_opaque_schemes() {
        assert!(!is_safe_favicon_url("data:,"));
        assert!(!is_safe_favicon_url("data:image/svg+xml,<svg/>"));
        assert!(!is_safe_favicon_url("data:text/html,<script></script>"));
        assert!(!is_safe_favicon_url("javascript:alert(1)"));
        assert!(!is_safe_favicon_url("file:///etc/passwd"));
        assert!(!is_safe_favicon_url("mailto:x@example.org"));
        assert!(!is_safe_favicon_url("blob:https://example.org/abc"));
    }

    /// Helper: write a fresh (today-stamped) cache entry for a URL so
    /// prewarm sees it as already cached.
    fn seed_fresh_cache(moss_dir: &Path, url: &str, title: &str) {
        let meta = LinkMeta {
            url: url.to_string(),
            title: Some(title.to_string()),
            description: None,
            favicon: None,
            og_image: None,
            cover_oid: None,
            cover_ext: None,
            cover_served_path: None,
            cover_width: None,
            cover_height: None,
            cover_lqip: None,
            cover_color: None,
            fetched_at: now_iso8601(),
        };
        write_cache(moss_dir, &meta);
    }

    /// Helper: write a STALE (8 days old) cache entry. Stale entries should
    /// be re-fetched by prewarm; we use this to verify staleness detection
    /// in `prewarm_link_meta_for_build` without triggering real network.
    fn seed_stale_cache(moss_dir: &Path, url: &str, title: &str) {
        // 8 days ago in ISO 8601. We synthesize the timestamp by hand to
        // avoid pulling chrono into the test path.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            .saturating_sub(8 * 86400);
        let days = secs / 86400;
        let (y, m, d) = days_to_ymd(days);
        let stale_ts = format!("{y:04}-{m:02}-{d:02}T00:00:00Z");
        let meta = LinkMeta {
            url: url.to_string(),
            title: Some(title.to_string()),
            description: None,
            favicon: None,
            og_image: None,
            cover_oid: None,
            cover_ext: None,
            cover_served_path: None,
            cover_width: None,
            cover_height: None,
            cover_lqip: None,
            cover_color: None,
            fetched_at: stale_ts,
        };
        write_cache(moss_dir, &meta);
    }

    #[test]
    fn test_prewarm_returns_zero_when_no_url_list() {
        let tmp = std::env::temp_dir().join(format!(
            "moss_prewarm_no_list_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        // First-ever build for a folder: no `.urls.json` exists yet. Prewarm
        // must return (0, 0) without attempting any network I/O.
        let (seen, fetched) = prewarm_link_meta_for_build(&tmp);
        assert_eq!(seen, 0);
        assert_eq!(fetched, 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_prewarm_skips_fresh_cache() {
        let tmp = std::env::temp_dir().join(format!(
            "moss_prewarm_fresh_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        // Seed two fresh cache entries and persist their URLs as if a previous
        // build had recorded them.
        seed_fresh_cache(&tmp, "https://a.example.test/", "A");
        seed_fresh_cache(&tmp, "https://b.example.test/", "B");
        let urls = vec![
            "https://a.example.test/".to_string(),
            "https://b.example.test/".to_string(),
        ];
        write_url_list_atomic(&tmp, &urls);

        // Both URLs are fresh — prewarm should hit zero network and return
        // (2 seen, 0 fetched). This is the steady-state hot path: idempotent,
        // cheap re-runs.
        let (seen, fetched) = prewarm_link_meta_for_build(&tmp);
        assert_eq!(seen, 2);
        assert_eq!(fetched, 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_fetch_all_parallel_skips_fresh() {
        let tmp = std::env::temp_dir().join(format!(
            "moss_parallel_fresh_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        seed_fresh_cache(&tmp, "https://x.example.test/", "X");
        seed_fresh_cache(&tmp, "https://y.example.test/", "Y");
        // All URLs fresh → fetched count is 0, no thread is spawned (loop
        // bound is `cold.len()` which is zero).
        let urls = ["https://x.example.test/", "https://y.example.test/"];
        let fetched = fetch_all_link_meta_parallel(&urls, &tmp);
        assert_eq!(fetched, 0);
        // And the cached titles are still readable as-is.
        assert_eq!(
            read_cache(&tmp, "https://x.example.test/").and_then(|m| m.title),
            Some("X".to_string())
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_prewarm_idempotent_after_first_run() {
        // Two consecutive prewarm calls on a steady-state cache (all entries
        // fresh) must both report (N, 0). This ensures running CLI builds
        // back-to-back stays cheap — no redundant fetches.
        let tmp = std::env::temp_dir().join(format!(
            "moss_prewarm_idempotent_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        seed_fresh_cache(&tmp, "https://stable.example.test/", "Stable");
        write_url_list_atomic(&tmp, &["https://stable.example.test/".to_string()]);

        let (s1, f1) = prewarm_link_meta_for_build(&tmp);
        let (s2, f2) = prewarm_link_meta_for_build(&tmp);
        assert_eq!((s1, f1), (1, 0));
        assert_eq!((s2, f2), (1, 0));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Minimal HTTP server for tests. Serves a fixed response for any GET
    /// request, then exits. Avoids pulling in a full HTTP test crate; we just
    /// need to drive the parallel-fetch path against something deterministic.
    fn spawn_test_server(html: &'static str, requests: usize) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");
        let handle = std::thread::spawn(move || {
            for _ in 0..requests {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = stream.write_all(body.as_bytes());
            }
        });
        (url, handle)
    }

    #[test]
    fn test_parallel_fetch_actually_populates_cold_cache() {
        // Exercise the genuine parallel-fetch path: cold URLs, real (loopback)
        // HTTP, real cache files written by the fetch. Closes the gap that
        // the other tests leave open by only seeding fresh caches.
        let html: &'static str = r#"<html><head>
            <meta property="og:title" content="Test Title">
            <link rel="icon" href="/favicon.ico">
        </head><body>hi</body></html>"#;

        let (url, server) = spawn_test_server(html, 3);
        let url1 = format!("{url}/a");
        let url2 = format!("{url}/b");
        let url3 = format!("{url}/c");

        let tmp = std::env::temp_dir().join(format!(
            "moss_parallel_cold_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        // Three cold URLs → parallel fetcher actually spawns workers.
        let urls = [url1.as_str(), url2.as_str(), url3.as_str()];
        let fetched = fetch_all_link_meta_parallel(&urls, &tmp);
        assert_eq!(fetched, 3, "all 3 cold URLs should have been fetched");

        // Each URL now has a populated cache entry with the parsed og:title.
        for u in &urls {
            let cached = read_cache(&tmp, u).expect("cache miss after fetch");
            assert_eq!(cached.title.as_deref(), Some("Test Title"));
        }

        // Re-running on hot cache: zero fetches.
        let refetch = fetch_all_link_meta_parallel(&urls, &tmp);
        assert_eq!(refetch, 0, "hot cache must produce zero fetches");

        let _ = server.join();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_freshness_check_rejects_stale() {
        // Stale entries (>7 days) are NOT considered fresh. Pins the boundary
        // that drives parallel-fetch's filter. The full cold→fetch→hot flow
        // is exercised by `test_parallel_fetch_actually_populates_cold_cache`
        // against a localhost HTTP server.
        let tmp = std::env::temp_dir().join(format!(
            "moss_prewarm_stale_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        seed_stale_cache(&tmp, "https://stale.example.test/", "Old Title");

        // Stale: read_cache returns Some, but read_fresh_cache returns None.
        assert!(read_cache(&tmp, "https://stale.example.test/").is_some());
        assert!(read_fresh_cache(&tmp, "https://stale.example.test/").is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── build-time fetch: budget, per-request timeout, response cap ────────

    fn fresh_tmp(label: &str) -> PathBuf {
        let tmp = std::env::temp_dir().join(format!(
            "moss_{label}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    #[test]
    fn build_fetch_budget_and_timeout_are_short_and_named() {
        // Pins the numbers this feature reports: a regression that silently
        // widened them back toward the prewarm path's 60s/10s would still
        // compile and pass every other test here.
        assert_eq!(BUILD_FETCH_BUDGET, std::time::Duration::from_secs(3));
        assert_eq!(
            BUILD_FETCH_PER_REQUEST_TIMEOUT,
            std::time::Duration::from_millis(1500)
        );
        assert!(BUILD_FETCH_BUDGET < FETCH_BUDGET, "must stay well under the prewarm budget");
    }

    /// A server that sleeps `delay_ms` before responding, `requests` times —
    /// each accepted connection handled on its OWN thread, so the client's
    /// concurrent workers are actually served in parallel rather than
    /// serialized behind a single accept loop (which is fine for the other
    /// tests' instant responses, but would hide the budget's effect here).
    fn spawn_slow_test_server(
        html: &'static str,
        delay_ms: u64,
        requests: usize,
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");
        let handle = std::thread::spawn(move || {
            let mut workers = Vec::with_capacity(requests);
            for _ in 0..requests {
                let Ok((mut stream, _)) = listener.accept() else { break };
                workers.push(std::thread::spawn(move || {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf);
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    let body = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n{}",
                        html.len(),
                        html
                    );
                    let _ = stream.write_all(body.as_bytes());
                }));
            }
            for w in workers {
                let _ = w.join();
            }
        });
        (url, handle)
    }

    #[test]
    fn a_short_total_budget_leaves_the_slow_remainder_cold() {
        // More URLs than `FETCH_CONCURRENCY` (8), each slow enough that the
        // budget expires before a second round starts. Real proof the
        // budget bounds WALL TIME, not just a per-request timeout: with no
        // budget at all this would take 2 rounds * 150ms. The 200ms budget
        // is comfortably above one 150ms round (so it isn't testing the
        // stricter "shorter than any single response" case — that's
        // `the_orchestrator_never_waits_on_a_stuck_worker` below) and
        // comfortably below two.
        let html = "<html><head><title>Slow</title></head></html>";
        let url_count = 16;
        let (base, server) = spawn_slow_test_server(html, 150, url_count);
        let urls: Vec<String> = (0..url_count).map(|i| format!("{base}/{i}")).collect();
        let url_refs: Vec<&str> = urls.iter().map(String::as_str).collect();
        let tmp = fresh_tmp("short_budget");

        let start = std::time::Instant::now();
        let fetched = fetch_all_link_meta_parallel_bounded(
            &url_refs,
            &tmp,
            std::time::Duration::from_millis(200),
            std::time::Duration::from_secs(5),
            "test",
        );
        let elapsed = start.elapsed();

        assert!(
            fetched < url_count,
            "a 200ms budget must not let a second 150ms round start: fetched {fetched} of {url_count}"
        );
        assert!(fetched > 0, "a budget comfortably above one round's response time should still see it complete");
        assert!(
            elapsed < std::time::Duration::from_millis(800),
            "must not run anywhere near the un-budgeted 2-round time: {elapsed:?}"
        );

        // Deliberately not joined: the whole point of this test is that the
        // budget stops the client short of `url_count` connections, so the
        // server's accept loop is left waiting for ones that never arrive.
        // Joining it here would hang the test on exactly the behavior being
        // proven correct.
        drop(server);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn the_orchestrator_never_waits_on_in_flight_work_past_the_deadline() {
        // A budget SHORTER than a single response: the old `thread::scope`
        // implementation joined every worker before returning, so it would
        // have waited out the in-flight 150ms request regardless of the
        // 20ms budget. The fix returns AT the deadline — zero completions
        // is the correct answer here, not a flake.
        let html = "<html><head><title>Slow</title></head></html>";
        let (base, server) = spawn_slow_test_server(html, 150, 1);
        let url = format!("{base}/only");
        let urls = [url.as_str()];
        let tmp = fresh_tmp("never_waits");

        let start = std::time::Instant::now();
        let fetched = fetch_all_link_meta_parallel_bounded(
            &urls,
            &tmp,
            std::time::Duration::from_millis(20),
            std::time::Duration::from_secs(5),
            "test",
        );
        let elapsed = start.elapsed();

        assert_eq!(fetched, 0, "the in-flight request had not finished by the 20ms deadline");
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "must return at the deadline, not wait for the 150ms response: {elapsed:?}"
        );

        drop(server); // straggler left running deliberately — see the test above
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_time_fetch_returns_within_budget_against_a_blackholed_connection() {
        // ureq's own timeout has been unreliable in the
        // past, and a genuinely blackholed PUBLIC host (packets silently
        // dropped mid-CONNECT, no RST) can stall well past the timeout
        // given, up to ~30s in the worst case. A local listener can't
        // reproduce that exact phase (loopback always completes the TCP
        // handshake instantly) — this reproduces the nearest local analog,
        // an accepted connection that never responds, which on this
        // ureq/OS combination `.timeout_read()` alone already catches in
        // ~1.5s (measured by ablating `ureq_agent_with_timeout`'s explicit
        // connect/read lines: same result, timeout() alone also caught it
        // here). The orchestrator fix is what removes the DEPENDENCY on
        // that being reliable at all: `the_orchestrator_never_waits_on_in_
        // flight_work_past_the_deadline` above proves its own budget
        // enforcement against a real, fast, non-hanging response, with no
        // ureq timeout involved either way. This test stays as the
        // regression guard for the scenario actually named in the issue —
        // generous slack, hard ceiling, proving "nowhere near 30s".
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            // Accept and hold — never read the request, never write a
            // response, never close. The listener (and every accepted
            // stream) leaks for the rest of the test process's life, which
            // is fine: nothing here is ever joined or waited on.
            while let Ok((stream, _)) = listener.accept() {
                std::mem::forget(stream);
            }
        });
        let url = format!("http://127.0.0.1:{port}/blackhole");
        let urls = [url.as_str()];
        let tmp = fresh_tmp("blackhole_budget");

        let start = std::time::Instant::now();
        let _ = fetch_new_link_meta_for_build(&urls, &tmp);
        let elapsed = start.elapsed();

        assert!(
            elapsed < BUILD_FETCH_BUDGET + std::time::Duration::from_secs(3),
            "the orchestrator must enforce the budget itself rather than trust ureq's own \
             (documented-unreliable) timeout — nowhere near the ~30s a blackholed \
             host can otherwise cause: {elapsed:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fetch_new_link_meta_for_build_populates_cache_for_a_fast_server() {
        // The actual public entry point `blocking.rs` calls, against a real
        // (loopback) server — proves the wiring end to end, not just the
        // shared bounded-fetch helper.
        let html = r#"<html><head><meta property="og:title" content="Build Fetched"></head></html>"#;
        let (base, server) = spawn_test_server(html, 5);
        let urls: Vec<String> = (0..5).map(|i| format!("{base}/{i}")).collect();
        let url_refs: Vec<&str> = urls.iter().map(String::as_str).collect();
        let tmp = fresh_tmp("build_fetch_fast");

        let fetched = fetch_new_link_meta_for_build(&url_refs, &tmp);
        assert_eq!(fetched, 5);
        for u in &urls {
            let cached = read_cache(&tmp, u).expect("cache miss after build-time fetch");
            assert_eq!(cached.title.as_deref(), Some("Build Fetched"));
        }

        let _ = server.join();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fetch_new_link_meta_for_build_finishes_promptly_when_the_server_is_down() {
        // "Offline build still finishes promptly": bind an ephemeral port,
        // then drop the listener before fetching — nothing listens there
        // any more, so the connection is refused immediately (a real local
        // refusal, not a guess about some fixed port's behavior under a
        // sandboxed network stack), well under the per-request timeout.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let url = format!("http://127.0.0.1:{port}/unreachable");
        let urls = [url.as_str()];
        let tmp = fresh_tmp("build_fetch_offline");
        let start = std::time::Instant::now();
        let fetched = fetch_new_link_meta_for_build(&urls, &tmp);
        let elapsed = start.elapsed();
        assert_eq!(fetched, 1, "a failed fetch still counts as attempted, not skipped");
        assert!(
            elapsed < BUILD_FETCH_BUDGET,
            "connection-refused must fail fast, not eat the whole budget: {elapsed:?}"
        );
        let cached = read_cache(&tmp, urls[0]);
        assert!(
            cached.map(|c| c.title.is_none()).unwrap_or(true),
            "no metadata on a failed fetch, but no panic either"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn read_capped_body_stops_at_the_byte_ceiling_not_the_stream_end() {
        // Deterministic — no network, no timing race. `io::repeat` is an
        // infinite reader; if the cap didn't apply, `.take()` inside
        // `read_capped_body` would never stop and this test would hang
        // rather than fail, which is exactly why the size is finite but
        // still 100x the cap: enough to prove truncation, never enough to
        // make an ablated cap's "read everything" branch slow.
        use std::io::Read as _;
        let source = std::io::repeat(b'x').take(100 * MAX_LINK_META_RESPONSE_BYTES);
        let (body, read_ok) = read_capped_body(source);
        assert!(read_ok);
        assert_eq!(body.len() as u64, MAX_LINK_META_RESPONSE_BYTES);
    }

    #[test]
    fn read_capped_body_passes_through_a_response_under_the_cap_untouched() {
        let html = "<html><head><title>Small</title></head></html>";
        let (body, read_ok) = read_capped_body(html.as_bytes());
        assert!(read_ok);
        assert_eq!(body, html);
    }

    /// A server that sends a real `<title>` immediately, then pads the body
    /// far past the response cap. Proves the cap is actually WIRED into the
    /// network fetch path (`read_capped_body` above pins the cap's own byte
    /// logic in isolation) — the outcome checked is the parsed title, not a
    /// timing race, so this stays reliable under parallel test-suite load.
    fn spawn_oversized_test_server(body_len: usize, requests: usize) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");
        let handle = std::thread::spawn(move || {
            for _ in 0..requests {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let prefix = "<html><head><title>Oversized</title></head><body>";
                let suffix = "</body></html>";
                let pad_len = body_len.saturating_sub(prefix.len() + suffix.len());
                let mut body = String::with_capacity(body_len);
                body.push_str(prefix);
                body.extend(std::iter::repeat('x').take(pad_len));
                body.push_str(suffix);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
        });
        (url, handle)
    }

    #[test]
    fn a_response_far_over_the_cap_still_yields_the_title_that_fits_near_the_front() {
        let (base, server) = spawn_oversized_test_server(4 * 1024 * 1024, 1);
        let tmp = fresh_tmp("oversized_ok");
        let url = format!("{base}/big");
        let meta = fetch_link_meta_with_timeout(&url, &tmp, std::time::Duration::from_secs(10));
        assert_eq!(meta.title.as_deref(), Some("Oversized"));
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── og:image download ───────────────────────────────────────────────

    /// A tiny real (encoded) PNG, generated in-memory — a genuine decodable
    /// image, not a hand-rolled byte guess.
    fn tiny_png_bytes() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(2, 2, image::Rgb([200, 40, 40]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    /// A server that responds once with `body` under `content_type` (or no
    /// header at all when `content_type` is `None`), then a 404 for every
    /// request after the first — mirrors a real "image present at first
    /// path, missing everywhere else" origin without a second listener.
    fn spawn_byte_server(
        body: Vec<u8>,
        content_type: Option<&'static str>,
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let ct_header = content_type
                .map(|ct| format!("Content-Type: {ct}\r\n"))
                .unwrap_or_default();
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{ct_header}\r\n", body.len());
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        });
        (url, handle)
    }

    fn spawn_404_server() -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");
        let handle = std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        });
        (url, handle)
    }

    #[test]
    fn download_og_image_stores_a_real_image_and_returns_its_oid_and_format() {
        let png = tiny_png_bytes();
        let (base, server) = spawn_byte_server(png.clone(), Some("image/png"));
        let tmp = fresh_tmp("download_og_image_ok");
        let result = download_og_image(&format!("{base}/cover.png"), &tmp, std::time::Duration::from_secs(5));
        let (oid, ext) = result.expect("a real PNG must download and decode");
        assert_eq!(ext, "png");
        let store = crate::build::cache::ObjectStore::for_site(&MossPaths::from_moss_dir(tmp.clone()));
        let stored = store.ready_blob(&oid).expect("oid must resolve in the object store");
        assert_eq!(std::fs::read(stored).unwrap(), png, "stored bytes should be identical to the download");
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_og_image_rejects_a_non_image_content_type() {
        let (base, server) = spawn_byte_server(b"<html>not a real image</html>".to_vec(), Some("text/html"));
        let tmp = fresh_tmp("download_og_image_wrong_ct");
        let result = download_og_image(&format!("{base}/page.html"), &tmp, std::time::Duration::from_secs(5));
        assert!(result.is_none(), "an explicit non-image Content-Type must short-circuit before decoding");
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_og_image_rejects_bytes_that_dont_actually_decode() {
        // A server can claim any Content-Type it likes — the sniff+decode
        // is the real gate, not the header.
        let (base, server) = spawn_byte_server(b"this is not image data at all".to_vec(), Some("image/jpeg"));
        let tmp = fresh_tmp("download_og_image_fake_ct");
        let result = download_og_image(&format!("{base}/fake.jpg"), &tmp, std::time::Duration::from_secs(5));
        assert!(result.is_none(), "a lying Content-Type must not bypass the decode check");
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_og_image_returns_none_on_404() {
        let (base, server) = spawn_404_server();
        let tmp = fresh_tmp("download_og_image_404");
        let result = download_og_image(&format!("{base}/missing.png"), &tmp, std::time::Duration::from_secs(5));
        assert!(result.is_none());
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fetch_link_meta_carries_forward_the_previous_cover_when_a_refresh_fails() {
        // "Keep serving the last successfully cached copy until the
        // metadata refresh succeeds": seed a cache entry with a cover
        // already materialized-looking, then fetch again against a page
        // whose og:image now 404s. The cover fields must survive.
        let tmp = fresh_tmp("carry_forward_cover");
        let html_with_dead_image = r#"<html><head>
            <meta property="og:title" content="Still Here">
            <meta property="og:image" content="/gone.png">
        </head></html>"#;
        let (base, server) = spawn_test_server(html_with_dead_image, 1);
        let url = format!("{base}/post");

        let previous = LinkMeta {
            url: url.clone(),
            title: Some("Old Title".to_string()),
            description: None,
            favicon: None,
            og_image: Some(format!("{base}/old-cover.png")),
            cover_oid: Some("deadbeefdeadbeef".to_string()),
            cover_ext: Some("png".to_string()),
            cover_served_path: Some("_moss/link/deadbeefdeadbeef.png".to_string()),
            cover_width: Some(400),
            cover_height: Some(300),
            cover_lqip: Some("data:image/webp;base64,AAAA".to_string()),
            cover_color: Some("#ff0000".to_string()),
            fetched_at: "2020-01-01T00:00:00Z".to_string(),
        };
        write_cache(&tmp, &previous);

        let meta = fetch_link_meta_with_timeout(&url, &tmp, std::time::Duration::from_secs(5));
        assert_eq!(meta.title.as_deref(), Some("Still Here"), "title still refreshes normally");
        assert_eq!(
            meta.cover_served_path.as_deref(),
            Some("_moss/link/deadbeefdeadbeef.png"),
            "the old cover must survive a failed image refresh"
        );
        assert_eq!(meta.cover_width, Some(400));

        let _ = server.join();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── unsafe fetch targets (SSRF-style guard) ─────────────────────────

    #[test]
    fn bare_host_strips_scheme_port_userinfo_and_ipv6_brackets() {
        assert_eq!(bare_host("https://example.com/a?b#c"), "example.com");
        assert_eq!(bare_host("http://example.com:8080/"), "example.com");
        assert_eq!(bare_host("http://user:pass@example.com/"), "example.com");
        assert_eq!(bare_host("http://[::1]:8080/"), "::1");
        assert_eq!(bare_host("http://[fe80::1]/"), "fe80::1");
    }

    #[test]
    fn is_unsafe_fetch_target_blocks_private_and_link_local_v4() {
        assert!(is_unsafe_fetch_target("http://10.0.0.5/"));
        assert!(is_unsafe_fetch_target("http://172.16.0.1/"));
        assert!(is_unsafe_fetch_target("http://192.168.1.1/"));
        // The address every major cloud provider serves instance metadata
        // from — the highest-value real-world target this guard exists for.
        assert!(is_unsafe_fetch_target("http://169.254.169.254/latest/meta-data/"));
        assert!(is_unsafe_fetch_target("http://0.0.0.0/"));
    }

    #[test]
    fn is_unsafe_fetch_target_blocks_link_local_and_unique_local_v6() {
        assert!(is_unsafe_fetch_target("http://[fe80::1]/"));
        assert!(is_unsafe_fetch_target("http://[fc00::1]/"));
        assert!(is_unsafe_fetch_target("http://[fd12:3456::1]/"));
        assert!(is_unsafe_fetch_target("http://[::]/"));
    }

    #[test]
    fn is_unsafe_fetch_target_allows_loopback_and_ordinary_hosts() {
        // Loopback is deliberately NOT blocked — see the doc comment on
        // `is_unsafe_fetch_target`: this crate's own test suite fakes a real
        // remote server with a `127.0.0.1` listener throughout, so blocking
        // it would make the guarded fetch untestable in this codebase's own
        // idiom, for a narrower threat than link-local/private already cover.
        assert!(!is_unsafe_fetch_target("http://127.0.0.1:4000/"));
        assert!(!is_unsafe_fetch_target("http://[::1]/"));
        assert!(!is_unsafe_fetch_target("http://localhost/"));
        assert!(!is_unsafe_fetch_target("https://example.com/post"));
        assert!(!is_unsafe_fetch_target("https://slykiten.com/"));
    }

    #[test]
    fn fetch_link_meta_with_timeout_refuses_a_private_target_without_a_cache_entry() {
        // Elapsed time, not just the `None` result, is the decisive proof:
        // 169.254.169.254 is unreachable from this machine either way, so a
        // guard that quietly did nothing would still return `None` here —
        // just after actually attempting (and timing out on) the connection.
        // The guard's whole point is to skip that attempt, so a real network
        // round trip taking any meaningful fraction of the 5s timeout would
        // mean it didn't fire.
        let tmp = fresh_tmp("unsafe_target_no_cache");
        let start = std::time::Instant::now();
        let meta = fetch_link_meta_with_timeout(
            "http://169.254.169.254/latest/meta-data/",
            &tmp,
            std::time::Duration::from_secs(5),
        );
        let elapsed = start.elapsed();
        assert_eq!(meta.title, None, "an unsafe target must never be fetched");
        assert!(read_cache(&tmp, "http://169.254.169.254/latest/meta-data/").is_none());
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "must reject before attempting any connection, not after timing one out: {elapsed:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_og_image_refuses_a_private_target() {
        let tmp = fresh_tmp("unsafe_image_target");
        let start = std::time::Instant::now();
        let result = download_og_image(
            "http://169.254.169.254/latest/meta-data/iam/",
            &tmp,
            std::time::Duration::from_secs(5),
        );
        let elapsed = start.elapsed();
        assert!(result.is_none(), "an unsafe target must never be downloaded");
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "must reject before attempting any connection, not after timing one out: {elapsed:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ── og:image decompression-bomb guard ───────────────────────────────

    /// A REAL, validly-encoded, small (well under a megabyte on the wire)
    /// PNG that is nonetheless absurdly wide — the shape a "declared
    /// dimensions far past what a real image needs" bomb takes, but genuinely
    /// decodable if nothing capped it. Deliberately real rather than a
    /// truncated/fake file: a fake one would be rejected by the decode step
    /// regardless of any dimension limit, which would prove nothing about
    /// `MAX_OG_IMAGE_DIMENSION` specifically. Deliberately narrow height (10px)
    /// so the total pixel count — and so `image::Limits::default()`'s
    /// pre-existing 512MiB allocation ceiling — stays far under its own
    /// threshold, isolating THIS cap rather than riding on that one.
    fn absurdly_wide_png_bytes() -> Vec<u8> {
        let width = MAX_OG_IMAGE_DIMENSION * 5;
        let img = image::RgbImage::from_pixel(width, 10, image::Rgb([10, 10, 10]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn download_og_image_rejects_a_canvas_over_the_dimension_cap() {
        let bomb = absurdly_wide_png_bytes();
        // Real image, but its declared width alone (5x the cap) already
        // dwarfs anything a real og:image needs — the point of the test.
        let (base, server) = spawn_byte_server(bomb, Some("image/png"));
        let tmp = fresh_tmp("og_image_over_cap");
        let result = download_og_image(&format!("{base}/wide.png"), &tmp, std::time::Duration::from_secs(5));
        assert!(result.is_none(), "a canvas over MAX_OG_IMAGE_DIMENSION must be rejected");
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn download_og_image_accepts_a_real_image_well_under_the_dimension_cap() {
        // Guards against an over-tight cap breaking the ordinary case.
        let (base, server) = spawn_byte_server(tiny_png_bytes(), Some("image/png"));
        let tmp = fresh_tmp("og_image_normal");
        let result = download_og_image(&format!("{base}/cover.png"), &tmp, std::time::Duration::from_secs(5));
        assert!(result.is_some(), "an ordinary small image must still download");
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
