//! The import engine: fetch a website (or read a local capture) and write
//! markdown into a folder.
//!
//! Host-agnostic by construction — no `AppHandle`, no event bus. Progress
//! leaves through the `on_progress` closure the caller supplies, which is a
//! Tauri emit in the app, a `TaskRegistry` handle behind the import panel,
//! and a no-op in `moss import`. The one Tauri wrapper that supplies the
//! second of those stays in `src-tauri/src/scrape.rs`.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use tokio::sync::Semaphore;

use super::converter::{extract_article, extract_article_with_snapshot, rewrite_image_links};
use super::crawler::extract_links;
use super::scope::{is_within_scope, UrlScope};
use super::service::{generate_frontmatter, render_error_markdown, rewrite_links, ScrapeConfig};
use super::writer::{rename_for_collision, sanitize_filename, url_to_file_path};

/// Subdirectory inside the output folder where downloaded media lands.
/// Relative to `<output>/` so the saved markdown can reference assets
/// with portable `./assets/imported/…` paths.
pub const ASSETS_SUBDIR: &str = "assets/imported";

/// Maximum concurrent HTTP requests across the whole pipeline.
const SCRAPE_CONCURRENCY_LIMIT: usize = 5;

static SCRAPE_SEMAPHORE: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(SCRAPE_CONCURRENCY_LIMIT));

/// Progress update emitted while scraping.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ScrapeProgress {
    pub pages_scraped: usize,
    pub pages_failed: usize,
    pub current_url: Option<String>,
    pub complete: bool,
}

/// Result of the scrape operation.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ScrapeResult {
    pub success: bool,
    pub total_pages: usize,
    pub failed_pages: usize,
    pub error: Option<String>,
}

/// A short, stable name for a media URL, so the same image fetched twice
/// lands on one file. Was `scrape/assets.rs`, whose `AssetManager` had no
/// production caller and went with it.
fn hash_url(url: &str) -> String {
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(url.as_bytes()))
}

/// Compose one imported note: pick its cover, point every media reference at
/// whatever landed in `remote_to_local`, and prepend frontmatter.
///
/// The one owner of the tail both arms used to paste, under a "keep the two in
/// sync" comment they had already drifted apart under — only the local-file arm
/// refused an empty body, so a crawl that extracted nothing wrote an empty note
/// and reported success. `None` here is that refusal, and each arm answers it
/// the way its own failures are answered.
///
/// `scope` is `Some` only for a recursive crawl, where an in-scope link can be
/// rewritten to the sibling file the crawl is also writing.
fn compose_note(
    article: &mut super::converter::Article,
    remote_to_local: &HashMap<String, String>,
    cover_remote: Option<String>,
    source_url: &str,
    scope: Option<&UrlScope>,
) -> Option<String> {
    if let Some(remote) = cover_remote {
        if let Some(local) = remote_to_local.get(&remote) {
            article.metadata.cover = Some(local.clone());
        }
    }

    let mut markdown = rewrite_image_links(&article.markdown, remote_to_local);
    markdown = super::converter::rewrite_embed_links(&markdown, remote_to_local);
    if let Some(scope) = scope {
        markdown = rewrite_links(&markdown, scope);
    }

    if markdown.trim().is_empty() {
        return None;
    }

    // No "Originally published at …" attribution: an import is the user's own
    // content (POSSE), so the vault copy is canonical and the source is
    // recorded as a `syndicated` mirror in frontmatter, not as a linkblog
    // banner implying the outlet is the origin.
    let frontmatter = generate_frontmatter(&article.metadata, source_url);
    Some(format!("{}{}", frontmatter, markdown))
}

/// Run the import described by `config`.
///
/// In single-URL mode (`config.recursive == false`), fetch only `start_url`,
/// extract its article, download media, write one markdown file.
///
/// In recursive mode, BFS-walk every URL within the start URL's scope (same
/// host + path prefix), capped by `config.max_pages`.
pub async fn scrape_to_folder<F>(
    config: ScrapeConfig,
    on_progress: F,
) -> Result<ScrapeResult, String>
where
    F: Fn(ScrapeProgress) + Send + Sync + 'static,
{
    let scope = UrlScope::new(&config.start_url).map_err(|e| format!("Invalid URL: {}", e))?;
    let out_dir = config.output_dir.as_path();
    if !out_dir.exists() {
        return Err("Output directory does not exist".to_string());
    }

    let assets_dir = out_dir.join(ASSETS_SUBDIR);
    fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    let mut remote_to_local: HashMap<String, String> = HashMap::new();
    let mut pages_scraped: usize = 0;
    let mut pages_failed: usize = 0;

    queue.push_back(config.start_url.clone());

    while let Some(url) = queue.pop_front() {
        if visited.contains(&url) {
            continue;
        }
        visited.insert(url.clone());

        if let Some(cap) = config.max_pages {
            if pages_scraped + pages_failed >= cap {
                break;
            }
        }

        on_progress(ScrapeProgress {
            pages_scraped,
            pages_failed,
            current_url: Some(url.clone()),
            complete: false,
        });

        let _permit = SCRAPE_SEMAPHORE.acquire().await.map_err(|e| e.to_string())?;

        // Every way this page can end — fetched and composed, or refused with
        // a reason — leaves through one value, so the file it lands in is
        // named and collision-renamed in exactly one place below. A fourth
        // failure mode cannot forget to do that.
        let outcome: Result<String, String> = 'page: {
        let html = match fetch_page(&url, &config.user_agent).await {
            Ok(html) => html,
            Err(e) => break 'page Err(e),
        };

        // Client-rendered builders keep their content in a pre-rendered
        // snapshot the page's state JSON points at — one extra fetch, scoped
        // to the site's own account. Fetch failure just falls back to the
        // page's own DOM (generic scorer).
        let snapshot_html = match crate::vault::import::engine::snapshot_request(&html, &url) {
            Some(snapshot_url) => match fetch_page(&snapshot_url, &config.user_agent).await {
                Ok(s) => Some(s),
                // The page degrades to its own (often empty) DOM — loud, not
                // silent: for a client-rendered viewer this loses the body.
                Err(e) => {
                    log::warn!("import: snapshot fetch failed for {url}: {snapshot_url}: {e}");
                    None
                }
            },
            None => None,
        };
        let mut article = extract_article_with_snapshot(&html, snapshot_html.as_deref(), &url);

        if config.recursive {
            for link in extract_links(&html, &url) {
                if is_within_scope(&scope, &link) && !visited.contains(&link) {
                    queue.push_back(link);
                }
            }
            // Manifest-declared pages (client-rendered viewers have no
            // server-side anchors to follow).
            for page in crate::vault::import::engine::discover_pages(&html, &url) {
                if is_within_scope(&scope, &page) && !visited.contains(&page) {
                    queue.push_back(page);
                }
            }
        }

        // Prefer og:image for the card cover — it is explicitly declared for
        // social sharing and is always the best choice. Fall back to the first
        // in-order body image when og:image is absent. We capture the REMOTE
        // URL here so we can look up the local hash filename after the download
        // loop below. If the chosen image's download fails, `cover_remote`
        // simply isn't in `remote_to_local` and the `cover:` frontmatter stays
        // unset — better than emitting a path pointing at a file that didn't
        // land on disk.
        let cover_remote = article
            .metadata
            .og_image
            .clone()
            .or_else(|| super::converter::first_image_url(&article.markdown, &url));

        for media_url in &article.media_urls {
            if remote_to_local.contains_key(media_url) {
                continue;
            }
            match download_asset(media_url, &assets_dir, &config.user_agent).await {
                Ok(filename) => {
                    let local_rel = format!("./{}/{}", ASSETS_SUBDIR, filename);
                    remote_to_local.insert(media_url.clone(), local_rel);
                }
                // The reference keeps its remote URL — degraded, not broken —
                // but never fail silently.
                Err(e) => {
                    log::warn!("import: asset download failed, keeping remote URL: {media_url}: {e}");
                    continue;
                }
            }
        }

        let scope_for_links = config.recursive.then_some(&scope);
        // Nothing extracted is a page failure, not a crawl failure: one
        // unparseable page in a hundred must not cost the other ninety-nine.
        compose_note(
            &mut article,
            &remote_to_local,
            cover_remote,
            &url,
            scope_for_links,
        )
        .ok_or_else(|| "no article content found (unsupported page or empty body)".to_string())
        };

        let relative = rename_for_collision(out_dir, &url_to_file_path(&url, &scope));
        match outcome {
            Ok(note) => {
                write_note(out_dir, &relative, &note)?;
                pages_scraped += 1;
            }
            Err(reason) => {
                write_note(out_dir, &relative, &render_error_markdown(&url, &reason))?;
                pages_failed += 1;
            }
        }
    }

    on_progress(ScrapeProgress {
        pages_scraped,
        pages_failed,
        current_url: None,
        complete: true,
    });

    Ok(ScrapeResult {
        success: true,
        total_pages: pages_scraped,
        failed_pages: pages_failed,
        error: None,
    })
}

/// Import a local file (MHTML web-archive or plain `.html`) into `output_dir`.
///
/// Unlike [`scrape_to_folder`], nothing is fetched over the network. For an
/// MHTML archive the original page URL and every embedded asset come straight
/// out of the file; the archive's `Snapshot-Content-Location` becomes the
/// `syndicated` source. This is the local-file arm of `moss import`, letting
/// the user bring in a page they saved from a login-gated or JS-heavy site
/// (e.g. a douban note saved via "Save Page As").
pub(crate) async fn import_local_file(path: &Path, output_dir: &Path) -> Result<ScrapeResult, String> {
    // The user picked this path in a file dialog, so it is routinely inside an
    // iCloud or Drive folder and routinely evicted — a plain `fs::read` of one
    // answers `Resource deadlock avoided (os error 11)` and the import reports
    // that as the reason. Asking is what makes the file arrive.
    //
    // Blocking, inside an `async fn`, for up to `INTERACTIVE_DEADLINE`. Today
    // the only caller is `cli::import` on a runtime of its own, so it parks
    // nothing shared. An app-side caller must reach this through
    // `spawn_blocking`, or one evicted import stalls a runtime worker for 15s.
    let bytes = crate::build::cloud_readiness::read_with_materialize_wait(
        path,
        crate::build::cloud_readiness::INTERACTIVE_DEADLINE,
    )
    .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;

    // MHTML vs plain HTML. Trust an explicit extension; only content-sniff when
    // the extension is unknown, so a `.html` file that merely mentions
    // "multipart/related" in its text isn't misrouted to the MHTML parser.
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let looks_mhtml = match ext.as_str() {
        "mhtml" | "mht" => true,
        "html" | "htm" => false,
        _ => {
            let head =
                String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
            head.contains("multipart/related")
        }
    };

    let (html, source_url, embedded): (
        String,
        Option<String>,
        HashMap<String, super::mhtml::MhtmlResource>,
    ) = if looks_mhtml {
        let doc = super::mhtml::parse_mhtml(&bytes)?;
        (doc.html, doc.source_url, doc.resources)
    } else {
        (
            String::from_utf8_lossy(&bytes).to_string(),
            None,
            HashMap::new(),
        )
    };

    // The archived page URL is a syndication mirror (query/fragment stripped so
    // tracking params don't leak into frontmatter). Empty when unknown.
    let source_clean = source_url.as_deref().map(strip_query).unwrap_or_default();
    let base_for_urls = source_url.as_deref().unwrap_or("");

    let mut article = extract_article(&html, base_for_urls);

    let assets_dir = output_dir.join(ASSETS_SUBDIR);
    fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    // Cover: og:image, else the first in-body image (same policy as the URL
    // path — `compose_note` below owns the rest of the shared tail).
    let cover_remote = article
        .metadata
        .og_image
        .clone()
        .or_else(|| super::converter::first_image_url(&article.markdown, base_for_urls));

    // Write embedded assets (MHTML) to disk; remote images in a plain .html
    // import are left as absolute URLs (the local path never hits the network).
    let mut remote_to_local: HashMap<String, String> = HashMap::new();
    for media_url in &article.media_urls {
        if remote_to_local.contains_key(media_url) {
            continue;
        }
        if let Some(res) = embedded.get(media_url) {
            let hash = hash_url(media_url);
            let extension = content_type_to_extension(&res.content_type);
            let filename = format!("{}.{}", hash, extension);
            // allow:raw_write the author's own source content, not `.moss/build.nosync/` output — an imported media file under `assets/imported/`
            fs::write(assets_dir.join(&filename), &res.bytes)
                .map_err(|e| format!("Failed to write asset: {}", e))?;
            remote_to_local.insert(
                media_url.clone(),
                format!("./{}/{}", ASSETS_SUBDIR, filename),
            );
        }
    }

    // One file in, one file out: an empty body fails loudly here rather than
    // writing an empty note and reporting success.
    let full_content = compose_note(
        &mut article,
        &remote_to_local,
        cover_remote,
        &source_clean,
        None,
    )
    .ok_or_else(|| {
        format!(
            "no article content found in {} (unsupported page or empty body)",
            path.display()
        )
    })?;

    // Name the note after its title, falling back to the source file stem —
    // both through the one filename policy, so the fallback cannot be the
    // looser of the two.
    let base_name = [
        article.metadata.title.as_deref(),
        path.file_stem().and_then(|s| s.to_str()),
    ]
    .into_iter()
    .flatten()
    .map(sanitize_filename)
    .find(|s| !s.is_empty())
    .unwrap_or_else(|| "imported".to_string());
    let relative = rename_for_collision(output_dir, &format!("{base_name}.md"));
    write_note(output_dir, &relative, &full_content)?;

    Ok(ScrapeResult {
        success: true,
        total_pages: 1,
        failed_pages: 0,
        error: None,
    })
}

/// Strip query + fragment from a URL, leaving a clean canonical form for the
/// `syndicated` frontmatter. Falls back to the input if it doesn't parse.
fn strip_query(url: &str) -> String {
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
fn fetch_with_validated_redirects(
    agent: &ureq::Agent,
    start_url: &str,
    user_agent: &str,
) -> Result<ureq::Response, ureq::Error> {
    fetch_following_redirects(agent, start_url, user_agent, refuse_unsafe_scrape_url)
}

/// [`fetch_with_validated_redirects`]'s body, taking the per-hop validator as
/// a parameter so the redirect-following MECHANISM (hop resolution, the hop
/// cap, handing back a Location-less or unparsable 3xx as-is) is testable
/// independently of the SSRF POLICY (`refuse_unsafe_scrape_url`) — every
/// test server a unit test can stand up is itself a loopback address, so a
/// test proving a legitimate same-host redirect is still followed cannot
/// use the real policy without tripping its own loopback refusal. Production
/// always calls the two-argument wrapper above, which always wires in the
/// real policy; no caller outside this file's tests should call this
/// directly.
fn fetch_following_redirects(
    agent: &ureq::Agent,
    start_url: &str,
    user_agent: &str,
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
        let response = agent.get(&current).set("User-Agent", user_agent).call()?;
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
        if let Err(msg) = validate(&next) {
            return Err(refused(&format!("redirected to a disallowed URL: {msg}")));
        }
        current = next;
    }
    Err(refused(&format!("too many redirects (> {MAX_REDIRECT_HOPS})")))
}

/// `ureq::get` with up to 3 attempts (1 initial + 2 retries) and exponential
/// backoff (500ms, then 1s) between attempts, for transient failures only
/// (see [`should_retry_status`]). Permanent failures (4xx other than 429)
/// return immediately on the first attempt.
///
/// Note: ureq maps any non-2xx response to `Error::Status` on `.call()`
/// itself, so this single retry loop also replaces what used to be a
/// separate manual `status >= 400` check in `fetch_page`.
fn call_with_retry(
    url: &str,
    user_agent: &str,
    timeout: std::time::Duration,
) -> Result<ureq::Response, String> {
    const TRIES: u32 = 3;
    let mut last_err = String::new();
    for attempt in 0..TRIES {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(
                500 * 2u64.pow(attempt - 1),
            ));
        }
        // Proxy-aware like every other moss HTTP client (system::proxy):
        // a direct connect times out on hosts only reachable via proxy.
        // `_no_redirects`: this path re-validates every redirect hop itself
        // (see `fetch_with_validated_redirects`) rather than trusting
        // ureq's own follower, which never re-checks a Location header.
        let agent = crate::system::proxy::proxied_ureq_agent_no_redirects(url, timeout);
        match fetch_with_validated_redirects(&agent, url, user_agent) {
            Ok(resp) => return Ok(resp),
            Err(ureq::Error::Status(code, resp)) => {
                let text = format!("{} {}", code, resp.status_text());
                if !should_retry_status(Some(code)) {
                    return Err(text);
                }
                last_err = text;
            }
            Err(e) => {
                // Transport-level error (timeout, connection reset, TLS...).
                last_err = format!("HTTP error: {}", e);
            }
        }
    }
    Err(last_err)
}

async fn fetch_page(url: &str, user_agent: &str) -> Result<String, String> {
    let url = url.to_string();
    let user_agent = user_agent.to_string();
    tokio::task::spawn_blocking(move || {
        let response = call_with_retry(&url, &user_agent, std::time::Duration::from_secs(30))?;

        response
            .into_string()
            .map_err(|e| format!("Failed to read response: {}", e))
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?
}

async fn download_asset(
    url: &str,
    assets_dir: &Path,
    user_agent: &str,
) -> Result<String, String> {
    let url_clone = url.to_string();
    let assets_dir = assets_dir.to_path_buf();
    let user_agent = user_agent.to_string();

    tokio::task::spawn_blocking(move || {
        let response =
            call_with_retry(&url_clone, &user_agent, std::time::Duration::from_secs(60))?;

        let hash = hash_url(&url_clone);
        let content_type = response.content_type();
        let extension = content_type_to_extension(content_type);
        let filename = format!("{}.{}", hash, extension);

        let mut bytes = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Failed to read asset: {}", e))?;

        let file_path = assets_dir.join(&filename);
        // allow:raw_write the author's own source content, not `.moss/build.nosync/` output — a downloaded media file
        fs::write(&file_path, &bytes).map_err(|e| format!("Failed to write asset: {}", e))?;

        Ok(filename)
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?
}

fn content_type_to_extension(content_type: &str) -> &str {
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

/// The one place the importer writes a note. `relative` has already been
/// through the filename policy and the collision rename; the parent is created
/// here rather than from a precomputed ancestor list, because `create_dir_all`
/// on the deepest path already makes every level above it.
fn write_note(output_dir: &Path, relative: &str, content: &str) -> Result<(), String> {
    let file_path = output_dir.join(relative);
    if let Some(dir) = file_path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    // allow:raw_write the author's own source content, not `.moss/build.nosync/` output — an imported note
    fs::write(&file_path, content).map_err(|e| format!("Failed to write file: {}", e))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_error_classification() {
        assert!(should_retry_status(None)); // transport error / timeout
        assert!(should_retry_status(Some(429)));
        assert!(should_retry_status(Some(503)));
        assert!(!should_retry_status(Some(404)));
        assert!(!should_retry_status(Some(200)));
    }

    #[test]
    fn test_content_type_to_extension() {
        assert_eq!(content_type_to_extension("image/png"), "png");
        assert_eq!(content_type_to_extension("image/jpeg"), "jpg");
        assert_eq!(content_type_to_extension("video/mp4"), "mp4");
        assert_eq!(content_type_to_extension("unknown/type"), "bin");
    }

    /// A 1×1 transparent PNG, base64.
    const PNG_1X1_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    /// End-to-end local import: an MHTML douban-note archive becomes a markdown
    /// note whose body is the note (not douban chrome), whose embedded image is
    /// written to disk and referenced locally, and whose frontmatter carries a
    /// `syndicated` link to the archived page — no network access.
    #[tokio::test]
    async fn import_local_mhtml_writes_syndicated_note_with_embedded_asset() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path();
        let mhtml = format!(
            "From: <Saved by Blink>\r\n\
Snapshot-Content-Location: https://www.douban.com/note/1/?_i=track\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=\"B\"\r\n\
\r\n\
--B\r\n\
Content-Type: text/html\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
Content-Location: https://www.douban.com/note/1/?_i=track\r\n\
\r\n\
<html><head><title>=E4=B9=8C=E7=BE=BD=E7=8E=89</title></head><body>\r\n\
<div id=3D\"db-nav\"><ul class=3D\"nav\"><li><a href=3D\"https://movie.douban.com\">movie-nav</a></li></ul></div>\r\n\
<div id=3D\"link-report\"><div class=3D\"note\">\r\n\
<p>peyote body text.</p>\r\n\
<img src=3D\"https://img.douban.com/a.png\">\r\n\
</div></div></body></html>\r\n\
--B\r\n\
Content-Type: image/png\r\n\
Content-Transfer-Encoding: base64\r\n\
Content-Location: https://img.douban.com/a.png\r\n\
\r\n\
{PNG_1X1_B64}\r\n\
--B--\r\n"
        );
        let mhtml_path = out.join("saved.mhtml");
        std::fs::write(&mhtml_path, mhtml).unwrap();

        let res = import_local_file(&mhtml_path, out).await.unwrap();
        assert_eq!(res.total_pages, 1, "one note imported");
        assert_eq!(res.failed_pages, 0);

        // Locate the written markdown (ignore the source .mhtml).
        let md_path = std::fs::read_dir(out)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
            .expect("a .md note should be written");
        let content = std::fs::read_to_string(&md_path).unwrap();

        // syndicated points at the archived page, with the tracking query stripped.
        assert!(
            content.contains("syndicated:\n  - https://www.douban.com/note/1/\n"),
            "frontmatter should syndicate to the clean source URL; got:\n{content}"
        );
        assert!(!content.contains("external_url"), "got:\n{content}");
        // Body is the note, not douban nav chrome.
        assert!(content.contains("peyote body text"), "got:\n{content}");
        assert!(!content.contains("movie-nav"), "nav leaked; got:\n{content}");
        // Embedded image written to disk and referenced locally.
        assert!(
            content.contains("./assets/imported/"),
            "image should be rewritten to a local path; got:\n{content}"
        );
        let asset_count = std::fs::read_dir(out.join(ASSETS_SUBDIR))
            .map(|d| d.count())
            .unwrap_or(0);
        assert_eq!(asset_count, 1, "exactly one embedded asset should be written");
    }

    #[tokio::test]
    async fn import_local_plain_html_no_source_omits_syndicated() {
        // The plain-.html arm: no origin URL, so no `syndicated`; body extracted.
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path();
        let html = "<html><head><title>My Essay</title></head><body><article>\
            <p>First paragraph with enough words to be extracted as the content.</p>\
            <p>A second paragraph, also with a few words, to satisfy scoring.</p>\
            </article></body></html>";
        let p = out.join("essay.html");
        std::fs::write(&p, html).unwrap();

        let res = import_local_file(&p, out).await.unwrap();
        assert_eq!(res.total_pages, 1);
        let md = out.join("My Essay.md");
        assert!(md.exists(), "note named from <title> should exist");
        let content = std::fs::read_to_string(&md).unwrap();
        assert!(content.contains("First paragraph"), "body missing: {content}");
        assert!(!content.contains("syndicated"), "no source → no syndicated: {content}");
        assert!(!content.contains("external_url"), "{content}");
    }

    #[tokio::test]
    async fn import_local_empty_body_errors_instead_of_empty_note() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path();
        let p = out.join("empty.html");
        std::fs::write(&p, "<html><head><title>Nothing</title></head><body></body></html>").unwrap();
        let res = import_local_file(&p, out).await;
        assert!(res.is_err(), "empty extraction should error, not write an empty note");
        assert!(
            std::fs::read_dir(out).unwrap().filter_map(|e| e.ok())
                .all(|e| e.path().extension().and_then(|x| x.to_str()) != Some("md")),
            "no .md should be written on empty extraction"
        );
    }



    #[test]
    fn test_hash_url_produces_consistent_hash() {
        let url1 = "https://example.com/image.png";
        let url2 = "https://example.com/image.png";
        let url3 = "https://example.com/other.png";
    
        assert_eq!(hash_url(url1), hash_url(url2));
        assert_ne!(hash_url(url1), hash_url(url3));
    }
    
    #[test]
    fn test_hash_url_produces_valid_filename() {
        let url = "https://example.com/path/to/image.png?query=value";
        let hash = hash_url(url);
    
        // Hash should be a valid filename (alphanumeric)
        assert!(hash.chars().all(|c| c.is_alphanumeric()));
        // Should be reasonably short
        assert!(hash.len() <= 16);
    }

    /// A page that extracts to nothing is a page failure, not a crawl failure:
    /// the note is replaced by the error placeholder, `failed_pages` counts it,
    /// and the run still succeeds. Before `compose_note` had one owner, only
    /// the local-file arm refused an empty body — this arm wrote an empty note
    /// and called it a success.
    #[tokio::test]
    async fn an_unextractable_page_is_counted_as_failed_not_written_empty() {
        let mut server = mockito::Server::new_async().await;
        let empty = server
            .mock("GET", "/nothing/")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><head><title>t</title></head><body></body></html>")
            .create_async()
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let config = ScrapeConfig::new(format!("{}/nothing/", server.url()), tmp.path());
        let res = scrape_to_folder(config, |_| {}).await.expect("the crawl itself succeeds");
        empty.assert_async().await;

        assert_eq!(res.total_pages, 0, "nothing was imported");
        assert_eq!(res.failed_pages, 1, "the page is counted as failed");

        let written: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
            .collect();
        assert_eq!(written.len(), 1, "one placeholder, not an empty note");
        let body = std::fs::read_to_string(&written[0]).unwrap();
        assert!(
            body.contains("no article content found"),
            "the placeholder must say why: {body}"
        );
    }

    // ── refuse_unsafe_scrape_url ─────────────────────────────────────────

    #[test]
    fn refuses_file_scheme() {
        let err = refuse_unsafe_scrape_url("file:///etc/passwd")
            .expect_err("file:// must never be accepted");
        assert!(err.contains("scheme"), "{err}");
    }

    #[test]
    fn refuses_loopback_ipv4() {
        let err = refuse_unsafe_scrape_url("http://127.0.0.1:8080/admin")
            .expect_err("a loopback URL must be refused");
        assert!(err.contains("loopback"), "{err}");
    }

    #[test]
    fn refuses_loopback_ipv6() {
        refuse_unsafe_scrape_url("http://[::1]/").expect_err("::1 must be refused");
    }

    #[test]
    fn refuses_ipv4_mapped_loopback() {
        // ::ffff:127.0.0.1 — a common bypass for a check that only inspects
        // the plain IPv6 loopback form.
        refuse_unsafe_scrape_url("http://[::ffff:127.0.0.1]/")
            .expect_err("an IPv4-mapped loopback literal must be refused");
    }

    #[test]
    fn refuses_link_local() {
        // 169.254.169.254 — the AWS/GCP/Azure instance-metadata address, the
        // canonical SSRF target.
        let err = refuse_unsafe_scrape_url("http://169.254.169.254/latest/meta-data/")
            .expect_err("a link-local URL must be refused");
        assert!(err.contains("link-local"), "{err}");
    }

    #[test]
    fn refuses_localhost_hostname() {
        refuse_unsafe_scrape_url("http://localhost/").expect_err("localhost must be refused");
    }

    #[test]
    fn allows_a_plain_https_url() {
        assert!(refuse_unsafe_scrape_url("https://example.com/blog").is_ok());
    }

    #[test]
    fn allows_a_plain_http_url() {
        assert!(refuse_unsafe_scrape_url("http://example.com/blog").is_ok());
    }

    // ── fetch_with_validated_redirects (SSRF via redirect) ──────────────
    //
    // `refuse_unsafe_scrape_url` alone only checks the URL the caller typed
    // — a public host that PASSES that check can still 302/303/307/308 to a
    // loopback/link-local target, and ureq's own redirect-follower (what
    // `proxied_ureq_agent`'s 5-redirect default drives) would walk straight
    // into it with no re-check at all. These tests exercise the manual
    // follower instead, using a mockito server as the "allowed host" that
    // issues the redirect — mockito binds to 127.0.0.1, but that is the
    // server ISSUING the 3xx, never a target these tests ask the follower to
    // fetch, so it never trips `refuse_unsafe_scrape_url` itself.

    fn no_redirect_test_agent(url: &str) -> ureq::Agent {
        crate::system::proxy::proxied_ureq_agent_no_redirects(
            url,
            std::time::Duration::from_secs(5),
        )
    }

    /// The detail this module's own refusals carry — `ureq::Error::Status`'s
    /// `Display` only ever prints `"{url}: status code {code}"` (see
    /// `ureq::error::Error`'s impl), so a test asserting on the human-
    /// readable reason must read it off the wrapped `Response` directly.
    fn refusal_detail(err: &ureq::Error) -> String {
        match err {
            ureq::Error::Status(_, resp) => resp.status_text().to_string(),
            other => other.to_string(),
        }
    }

    #[tokio::test]
    async fn a_redirect_to_loopback_is_refused() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/start")
            .with_status(302)
            .with_header("Location", "http://127.0.0.1:1/internal")
            .create_async()
            .await;

        let start = format!("{}/start", server.url());
        let agent = no_redirect_test_agent(&start);
        let err = fetch_with_validated_redirects(&agent, &start, "test-agent")
            .expect_err("a redirect to a loopback address must be refused");
        // Not just `.expect_err(...)`: an ablated per-hop check would still
        // ATTEMPT the connection to 127.0.0.1:1 and get a real connection
        // error back (nothing listens there in the sandbox) — an `Err` for
        // the wrong reason. Asserting the detail is what actually
        // distinguishes "the check refused this" from "the network did".
        assert!(refusal_detail(&err).contains("disallowed"), "{}", refusal_detail(&err));
    }

    #[tokio::test]
    async fn a_redirect_to_link_local_metadata_is_refused() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/start")
            .with_status(302)
            .with_header("Location", "http://169.254.169.254/latest/meta-data/")
            .create_async()
            .await;

        let start = format!("{}/start", server.url());
        let agent = no_redirect_test_agent(&start);
        let err = fetch_with_validated_redirects(&agent, &start, "test-agent")
            .expect_err("a redirect to the cloud-metadata address must be refused");
        // Asserting the detail (not just "some Err") matters here: an
        // ablated check would still connect-fail refusing this unroutable
        // address in most sandboxes, which is an Err for the wrong reason —
        // see `a_redirect_to_loopback_is_refused`'s comment.
        assert!(refusal_detail(&err).contains("disallowed"), "{}", refusal_detail(&err));
    }

    #[tokio::test]
    async fn a_redirect_to_ipv6_loopback_is_refused() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/start")
            .with_status(302)
            .with_header("Location", "http://[::1]/internal")
            .create_async()
            .await;

        let start = format!("{}/start", server.url());
        let agent = no_redirect_test_agent(&start);
        let err = fetch_with_validated_redirects(&agent, &start, "test-agent")
            .expect_err("a redirect to ::1 must be refused");
        assert!(refusal_detail(&err).contains("disallowed"), "{}", refusal_detail(&err));
    }

    #[tokio::test]
    async fn a_legitimate_redirect_is_still_followed() {
        // Canonicalization (trailing slash, path rewrite — the same shape as
        // an http→https upgrade) must keep working: only a redirect to a
        // DISALLOWED target is refused, not every redirect. Exercises the
        // `fetch_following_redirects` MECHANISM (hop-following, resolving a
        // relative `Location`) with an always-allow validator — every test
        // server available here is itself a loopback address, so the REAL
        // `refuse_unsafe_scrape_url` would refuse this hop regardless of how
        // legitimate it is (that policy is proven separately, on real
        // public URLs, by `allows_a_plain_http(s)_url` above).
        let mut server = mockito::Server::new_async().await;
        let _redirect = server
            .mock("GET", "/old-path")
            .with_status(301)
            .with_header("Location", "/new-path")
            .create_async()
            .await;
        let _target = server
            .mock("GET", "/new-path")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>ok</body></html>")
            .create_async()
            .await;

        let start = format!("{}/old-path", server.url());
        let agent = no_redirect_test_agent(&start);
        let response = fetch_following_redirects(&agent, &start, "test-agent", |_| Ok(()))
            .expect("a legitimate redirect must still be followed");
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn a_redirect_chain_longer_than_the_hop_cap_is_refused() {
        let mut server = mockito::Server::new_async().await;
        let mut mocks = Vec::new();
        for i in 0..=MAX_REDIRECT_HOPS {
            mocks.push(
                server
                    .mock("GET", format!("/hop{i}").as_str())
                    .with_status(302)
                    .with_header("Location", &format!("/hop{}", i + 1))
                    .create_async()
                    .await,
            );
        }

        let start = format!("{}/hop0", server.url());
        let agent = no_redirect_test_agent(&start);
        fetch_with_validated_redirects(&agent, &start, "test-agent")
            .expect_err("a chain past MAX_REDIRECT_HOPS must be refused, not followed forever");
    }
}
