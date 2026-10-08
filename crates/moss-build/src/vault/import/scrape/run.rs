//! The import engine: fetch a website (or read a local capture) and write
//! markdown into a folder.
//!
//! Host-agnostic by construction — no `AppHandle`, no event bus. Progress
//! leaves through the `on_progress` closure the caller supplies, which is a
//! Tauri emit in the app, a `TaskRegistry` handle behind the import panel,
//! and a no-op in `moss import`. The one Tauri wrapper that supplies the
//! second of those stays in the desktop app.

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use tokio::sync::Semaphore;

use super::converter::{
    extract_article_with_snapshot, linked_file_urls, rewrite_file_links, rewrite_image_links,
};
use super::crawl_state::CrawlState;
use super::design::{self, norm_url, Capture, ChromeSummary, HomePage};
use super::crawler::{
    extract_canonical_url, extract_links, host_of, is_non_page_file_url,
    looks_like_html_page,
};
use super::fetch::{
    download_asset, download_linked_file, fetch_page, linked_filename, observe_pace, Downloaded,
};
use super::scope::{is_within_scope, UrlScope};
use super::service::{generate_frontmatter, render_error_markdown, rewrite_links, ScrapeConfig};
use super::writer::{rename_for_collision, url_to_file_path};
use crate::vault::import::widgets::WidgetCount;

// Re-exported at this module's old path: a consumer outside this crate
// already names `refuse_unsafe_scrape_url` here, from before the SSRF/fetch
// stack moved into its own sibling module.
pub use super::fetch::refuse_unsafe_scrape_url;

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
    /// Fetched responses whose Content-Type (declared, or sniffed when
    /// missing/generic) was not HTML/XHTML — a PDF, an image, a calendar
    /// file, an RSS feed, and the like. Never written as a `.md` page.
    pub skipped_pages: usize,
    /// Fetched HTML pages whose `<link rel="canonical">` identity already
    /// matched a page already written in this crawl, or — lacking a
    /// canonical — whose URL path (query string and fragment ignored)
    /// AND extracted body both matched an already-written page's. A
    /// recursive crawl's queue routinely fills with query-string variants
    /// of one page (lightbox `?itemId=…`, listing filters, calendar
    /// exports). Never written as a `.md` page, and, like `skipped_pages`,
    /// does not count against `max_pages`; its own links are still
    /// harvested before the duplicate verdict is reached.
    pub duplicate_pages: usize,
    /// A URL whose fetch failed (429 that outlasted every retry, host
    /// unreachable, …) but whose own [`path_identity`](super::crawler::path_identity)
    /// already names a path this crawl has already CONFIRMED produces
    /// duplicates (a prior URL at that same path was itself resolved as
    /// `Duplicate`, by rule 1 or rule 2) — a query-string variant (lightbox
    /// `?itemId=…`, a filter, a calendar export) of content already safely
    /// on disk, not a page gone missing. Deliberately narrower than "any
    /// page already written at that path": an old-CMS site that routes
    /// distinct posts through the same path (`?p=1`, `?p=2`, each with its
    /// own self-canonical) must not lose a later sibling's stub just
    /// because an earlier sibling at the same path happened to import
    /// first — only a path with a CONFIRMED duplicate on it is treated as
    /// one that fails closed. Never written as the `scrape_error` stub
    /// `failed_pages` gets, and — like `duplicate_pages` — never counted
    /// against `max_pages`.
    pub unreachable_variants: usize,
    /// A URL whose fetch failed outright but whose own path extension
    /// already names a file that is never a page (a PDF, an image, an
    /// archive, an Office document, `.ics`, a feed/data format, a
    /// stylesheet or script — see
    /// [`is_non_page_file_url`](super::crawler::is_non_page_file_url)). A
    /// successful fetch of the same URL would have been skipped by
    /// `skipped_pages` once its response came back; this is that same
    /// verdict reached from the URL alone, for a fetch that never got that
    /// far. Never written as the `scrape_error` stub `failed_pages` gets,
    /// and — like `skipped_pages` — never counted against `max_pages`: the
    /// URL was never a page candidate to begin with, fetch or no fetch.
    pub unreachable_files: usize,
    /// Files an imported page links to on its own site (a PDF, a score, an
    /// office document, an image, audio, an archive) downloaded into the
    /// assets folder, with the link rewritten to the copy. A file that
    /// failed to download counts in `unreachable_files` instead; one over
    /// the size cap counts in `oversized_files`. Both keep the original link.
    pub linked_files_downloaded: usize,
    /// Linked files over the size cap: not downloaded, link left as written.
    pub oversized_files: usize,
    /// Iframes and forms replaced by a link (a hosted page, or the page's
    /// contact address) because a static site cannot run them.
    pub widgets_carried: usize,
    /// Iframes and forms with no static form at all (a contact form and no
    /// address anywhere on the page): removed, and counted here instead of
    /// marked in the page.
    pub widgets_dropped: usize,
    /// True when `max_pages` stopped a recursive crawl before every
    /// discovered in-scope URL had been visited — distinct from a crawl that
    /// finished because its queue simply ran out.
    pub capped: bool,
    /// Discovered, in-scope URLs still unvisited when the cap stopped the
    /// crawl (deduplicated). Zero whenever `capped` is false.
    pub remaining_urls: usize,
    /// In-scope URLs the site's own sitemap declared (after locale-alternate
    /// collapsing — see `sitemap::collapse_locale_alternates`), for a
    /// recursive crawl that found one. These are exempt from `max_pages`
    /// (rule 3: a site's declared page list is always imported) and are
    /// seeded ahead of link-discovered URLs, so they also win survivor
    /// choice when a link-discovered duplicate names the same page. Zero
    /// when the crawl was not recursive or the site declared no sitemap.
    pub sitemap_urls: usize,
    /// True when the sitemap itself declared more than
    /// `sitemap::MAX_SITEMAP_URLS` entries and was truncated.
    pub sitemap_truncated: bool,
    /// Hosts this crawl's own per-host pacing (`crawl_state::HostPacer`)
    /// slowed down after a 429/503 — see [`RateLimitedHost`]. Empty when
    /// every fetch stayed under every host's rate limit for the whole
    /// crawl, same as before pacing existed.
    pub rate_limited_hosts: Vec<RateLimitedHost>,
    /// What the import wrote for the site's chrome (name, nav, logo, footer,
    /// icon). Default when the run did not look: a single-page import.
    pub chrome: ChromeSummary,
    pub error: Option<String>,
}

/// One host [`ScrapeResult::rate_limited_hosts`] names, with the PEAK
/// pacing interval it reached — kept even if later successes decayed the
/// interval back down before the crawl finished, since "this host needed
/// slowing down" is the fact worth surfacing, not whatever the interval
/// happened to be at the very last request.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RateLimitedHost {
    pub host: String,
    /// Peak pacing interval enforced for this host during the crawl, in
    /// milliseconds.
    pub interval_ms: u64,
}

/// What became of one URL popped from the crawl queue.
enum PageOutcome {
    /// Composed into a note; the full markdown ready to write.
    Written(String, WidgetCount),
    /// Fetched (or attempted) but refused with a reason — written as the
    /// error placeholder, so an in-scope link elsewhere that points at it
    /// still resolves to a real file on disk.
    Failed(String),
    /// The response was not an HTML/XHTML page. Never written as a `.md`
    /// file, and — because this is decided before any link discovery or
    /// asset download runs — its own body is never treated as a source of
    /// further links or media either.
    Skipped,
    /// Its `<link rel="canonical">` identity — or, lacking one, its own
    /// URL path together with its extracted body — already matched a page
    /// already written in this crawl. Unlike `Skipped`, its links were
    /// harvested (same as `Written`) before this verdict; unlike
    /// `Written`, nothing reaches disk and it costs no slot in the page
    /// cap.
    Duplicate,
    /// The fetch itself failed (a 429 that outlasted every retry, a host
    /// that went away mid-crawl, …) for a URL whose [`path_identity`]
    /// already names a path this crawl has already CONFIRMED produces
    /// duplicates (see `unreachable_variants`) — a query-string variant
    /// (`?itemId=…`) of a page already safely on disk, not a page gone
    /// missing. Unlike `Failed`, this never reaches disk as a
    /// `scrape_error` stub: the content it would have pointed at is already
    /// imported under its own URL, so a stub here would be a confusing,
    /// redundant duplicate of a real page. Counted on its own rather than
    /// folded into either `Failed` or `Duplicate`, since it is neither — a
    /// duplicate that could not be confirmed.
    VariantFetchFailed,
    /// The fetch itself failed, but the URL's own path extension already
    /// names a file that is never a page (see [`is_non_page_file_url`]) — a
    /// successful fetch of the same URL would have been [`Skipped`](Self::Skipped)
    /// once its Content-Type came back, so a failed one must not become a
    /// `scrape_error` stub just because it never got that far. Counted on
    /// its own (`unreachable_files`) rather than folded into `Skipped`,
    /// since unlike a real `Skipped` outcome nothing was actually fetched or
    /// sniffed here — the verdict comes from the URL alone.
    UnreachableFile,
}

/// Content hash half of the rule-2 duplicate fallback used when a page
/// declares no `<link rel="canonical">` of its own: xxh3 of the extracted
/// markdown BODY, never the raw HTML, so two pages that differ only in
/// boilerplate (ad slots, nonce attributes, a request-scoped id) that htmd
/// discards along with the rest of the clutter still compare equal. Never
/// consulted alone — rule 2 also requires [`path_identity`] to match, so a
/// short body two DIFFERENT pages happen to share (two templated "coming
/// soon" stubs at different addresses) never collapses them into one.
fn hash_body(body: &str) -> u64 {
    xxhash_rust::xxh3::xxh3_64(body.as_bytes())
}

/// The site a note's links are resolved against.
pub(crate) struct LinkScope<'a> {
    pub(crate) scope: &'a UrlScope,
    /// Whether in-scope page links become links to the notes written beside it.
    pub(crate) rewrite_pages: bool,
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
/// `links` is `None` for a local-file import (no site to resolve links
/// against). `remote_to_local` holds paths already relative to the note.
pub(crate) fn compose_note(
    article: &mut super::converter::Article,
    remote_to_local: &HashMap<String, String>,
    cover_remote: Option<String>,
    source_url: &str,
    links: Option<LinkScope<'_>>,
) -> Option<String> {
    if let Some(remote) = cover_remote {
        if let Some(local) = remote_to_local.get(&remote) {
            article.metadata.cover = Some(local.clone());
        }
    }

    let mut markdown = rewrite_image_links(&article.markdown, remote_to_local);
    markdown = super::converter::rewrite_embed_links(&markdown, remote_to_local);
    if let Some(LinkScope { scope, rewrite_pages }) = links {
        markdown = rewrite_file_links(&markdown, source_url, scope, remote_to_local);
        // Only a recursive crawl can point an in-scope link at the sibling
        // note it is also writing.
        if rewrite_pages {
            markdown = rewrite_links(&markdown, scope);
        }
    }

    if markdown.trim().is_empty() {
        return None;
    }

    if let Some(title) = article.metadata.title.as_deref() {
        let names: Vec<&str> = article.metadata.publisher.iter().map(String::as_str).collect();
        if let Some(rest) = super::finalize::without_title_heading(&markdown, title, &names) {
            // A page that is only its title keeps it: an empty note is a failure.
            if !rest.trim().is_empty() {
                markdown = rest;
            }
        }
    }

    // No "Originally published at …" attribution: the vault copy is
    // canonical, and the source is recorded as `origin` frontmatter
    // (provenance) rather than a linkblog banner implying the fetched page
    // is still where the content lives.
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

    // The site's own declared page list — read before the link-following
    // walk starts, so an unlinked page is queued regardless and, seeded
    // ahead of every link-discovered URL below, is also the one on disk
    // when a link-discovered duplicate names the same page (rule 1 compares
    // only against identities already WRITTEN, so whichever is written
    // first survives).
    let sitemap = if config.recursive {
        super::sitemap::discover(&scope, &config.start_url, &config.user_agent).await
    } else {
        super::sitemap::SitemapDiscovery::default()
    };

    // One record per crawl concern (frontier, page cap, duplicate
    // detection, the asset map, the outcome tally) instead of a dozen loose
    // mutables threaded through the loop by hand — see `crawl_state` for
    // the rules each sub-record owns and why.
    let mut state = CrawlState::new(&config.start_url, &sitemap);
    // Paths (relative to the output folder) of the pages this run wrote.
    let mut written: Vec<String> = Vec::new();
    // Normalised page URL → the note written for it, for the chrome pass.
    let mut written_urls: HashMap<String, String> = HashMap::new();
    // Where the site's chrome is read from: the start page, else the first page.
    let mut home_page: Option<HomePage> = None;

    while let Some(url) = state.frontier.pop() {
        if state.frontier.is_visited(&url) {
            continue;
        }

        let is_declared = state.cap.is_declared(&url);

        if !is_declared && !state.cap.has_room(config.max_pages) {
            // Popped but never visited or processed: put it back so the
            // leftover count computed after the loop includes it.
            state.frontier.requeue_front(url);
            state.cap.mark_capped();
            break;
        }
        state.frontier.visit(&url);

        on_progress(ScrapeProgress {
            pages_scraped: state.tally.scraped(),
            pages_failed: state.tally.failed(),
            current_url: Some(url.clone()),
            complete: false,
        });

        let _permit = SCRAPE_SEMAPHORE.acquire().await.map_err(|e| e.to_string())?;

        // Every way this page can end — fetched and composed, refused with a
        // reason, or skipped as non-HTML — leaves through one value, so the
        // file it lands in (or doesn't) is decided in exactly one place
        // below. A fourth outcome cannot forget to do that.
        let outcome: PageOutcome = 'page: {
        let fetch_host = host_of(&url);
        state.pacer.wait(&fetch_host).await;
        let page_result = fetch_page(&url, &config.user_agent).await;
        observe_pace(&mut state.pacer, &fetch_host, &page_result);
        let (html, content_type) = match page_result {
            Ok(v) => v,
            Err(e) => {
                // A URL whose own path extension already names a file that
                // is never a page (a PDF, an image, …) would have been
                // skipped once its Content-Type came back — see
                // `is_non_page_file_url`. A failed fetch of it must not
                // become a `scrape_error` stub just because it never got
                // far enough to prove that itself. Checked first: this URL
                // was never a page candidate regardless of the dedupe state
                // the check below reads.
                if is_non_page_file_url(&url) {
                    log::warn!(
                        "import: fetch failed for a URL whose extension already names a \
                         non-page file, skipping the stub: {url}: {e}"
                    );
                    break 'page PageOutcome::UnreachableFile;
                }
                // A failed fetch on a path this crawl has already
                // CONFIRMED produces duplicates is a query-string variant
                // that couldn't be reconfirmed, not a page gone missing —
                // see `Dedupe::is_known_variant` and
                // `PageOutcome::VariantFetchFailed`. A genuinely new URL
                // that fails still gets `Failed`'s stub below, exactly as
                // before this check existed.
                if state.dedupe.is_known_variant(&url) {
                    log::warn!(
                        "import: fetch failed for a query-string variant of an \
                         already-imported page, skipping the stub: {url}: {e}"
                    );
                    break 'page PageOutcome::VariantFetchFailed;
                }
                break 'page PageOutcome::Failed(e.into());
            }
        };

        // A recursive crawl follows every same-host, same-prefix link it
        // finds, and not everything at such a link is a page: a linked PDF,
        // image, calendar file, or the site's own RSS feed all live at
        // in-scope URLs. Decided before any extraction, link discovery, or
        // asset download runs, so a non-page's body is never mined for
        // further links or images either.
        if !looks_like_html_page(&content_type, &html) {
            break 'page PageOutcome::Skipped;
        }

        if config.recursive {
            let is_start = norm_url(&url) == norm_url(&config.start_url);
            if home_page.as_ref().is_none_or(|h| is_start && !h.is_start) {
                home_page = Some(HomePage { url: url.clone(), html: html.clone(), is_start });
            }
        }

        // The page's own declared identity — same-host `<link
        // rel="canonical">` only (see `extract_canonical_url`). Read before
        // any of the more expensive extraction below, so a duplicate never
        // pays for a snapshot fetch or article extraction it will discard.
        let canonical = extract_canonical_url(&html, &url);

        if config.recursive {
            for link in extract_links(&html, &url) {
                if state.block_locale_alternate(&link, &sitemap.locale_alternates) {
                    continue;
                }
                if is_within_scope(&scope, &link) {
                    state.frontier.enqueue_if_unvisited(link);
                }
            }
            // Manifest-declared pages (client-rendered viewers have no
            // server-side anchors to follow).
            for page in crate::vault::import::engine::discover_pages(&html, &url) {
                if state.block_locale_alternate(&page, &sitemap.locale_alternates) {
                    continue;
                }
                if is_within_scope(&scope, &page) {
                    state.frontier.enqueue_if_unvisited(page);
                }
            }
        }

        // Rule 1 — see `Dedupe::is_duplicate_identity` for why this is
        // checked against identities already WRITTEN, never against an
        // unvisited target still sitting in the queue.
        if let Some(canon) = &canonical {
            if state.dedupe.is_duplicate_identity(&url, canon) {
                break 'page PageOutcome::Duplicate;
            }
        }

        // Client-rendered builders keep their content in a pre-rendered
        // snapshot the page's state JSON points at — one extra fetch, scoped
        // to the site's own account. Fetch failure just falls back to the
        // page's own DOM (generic scorer).
        let snapshot_html = match crate::vault::import::engine::snapshot_request(&html, &url) {
            Some(snapshot_url) => {
                let snapshot_host = host_of(&snapshot_url);
                state.pacer.wait(&snapshot_host).await;
                let snapshot_result = fetch_page(&snapshot_url, &config.user_agent).await;
                observe_pace(&mut state.pacer, &snapshot_host, &snapshot_result);
                match snapshot_result {
                    Ok((s, _content_type)) => Some(s),
                    // The page degrades to its own (often empty) DOM — loud,
                    // not silent: for a client-rendered viewer this loses
                    // the body.
                    Err(e) => {
                        log::warn!("import: snapshot fetch failed for {url}: {snapshot_url}: {e}");
                        None
                    }
                }
            }
            None => None,
        };
        let mut article = extract_article_with_snapshot(&html, snapshot_html.as_deref(), &url);
        let body_hash = hash_body(&article.markdown);

        // Rule 2, the fallback for a page with no canonical of its own —
        // see `Dedupe::is_duplicate_body`.
        if canonical.is_none() && state.dedupe.is_duplicate_body(&url, body_hash) {
            break 'page PageOutcome::Duplicate;
        }

        // Prefer og:image for the card cover — it is explicitly declared for
        // social sharing and is always the best choice. Fall back to the first
        // in-order content image (never a header/nav/footer logo or an icon, see
        // `metadata::fallback_cover`) when og:image is absent. We capture the REMOTE
        // URL here so we can look up the local hash filename after the download
        // loop below. If the chosen image's download fails, `cover_remote`
        // simply isn't in the asset map and the `cover:` frontmatter stays
        // unset — better than emitting a path pointing at a file that didn't
        // land on disk.
        let cover_remote = article
            .metadata
            .og_image
            .clone()
            .or_else(|| {
                super::metadata::fallback_cover(&article.markdown, &article.metadata.chrome_images, &url)
            });

        for media_url in &article.media_urls {
            if state.assets.is_settled(media_url) {
                continue;
            }
            // Keyed by the asset's OWN host, not the page's — a page's
            // images routinely redirect to a separate CDN subdomain (see
            // `fetch_following_redirects`'s doc below), which shares no
            // rate limit with the page host and must pace independently.
            let asset_host = host_of(media_url);
            state.pacer.wait(&asset_host).await;
            let download_result = download_asset(media_url, &assets_dir, &config.user_agent).await;
            observe_pace(&mut state.pacer, &asset_host, &download_result);
            match download_result {
                Ok(filename) => {
                    let local_rel = format!("./{}/{}", ASSETS_SUBDIR, filename);
                    state.assets.insert(media_url.clone(), local_rel);
                }
                // The reference keeps its remote URL — degraded, not broken —
                // but never fail silently.
                Err(e) => {
                    log::warn!("import: asset download failed, keeping remote URL: {media_url}: {e}");
                    state.assets.keep_remote(media_url.clone());
                    continue;
                }
            }
        }

        // Files the page links to (a score, a document) get the same
        // treatment as its images: downloaded once into the assets folder,
        // the link pointed at the copy by `compose_note`.
        for file_url in linked_file_urls(&article.markdown, &url, &scope) {
            if !state.assets.is_settled(&file_url) {
                let asset_host = host_of(&file_url);
                state.pacer.wait(&asset_host).await;
                let filename = state.assets.claim_name(linked_filename(&file_url), &file_url);
                let result = download_linked_file(
                    &file_url,
                    &assets_dir,
                    &config.user_agent,
                    config.linked_file_max_bytes,
                    filename,
                )
                .await;
                observe_pace(&mut state.pacer, &asset_host, &result);
                match result {
                    Ok(Downloaded::Saved(filename)) => {
                        state.assets.insert(file_url.clone(), format!("./{ASSETS_SUBDIR}/{filename}"));
                        state.tally.record_linked_file_downloaded();
                    }
                    Ok(Downloaded::TooLarge) => {
                        log::warn!(
                            "import: linked file over {} MB, not downloaded and left as a remote link: {file_url}",
                            config.linked_file_max_bytes / (1024 * 1024)
                        );
                        state.assets.keep_remote(file_url.clone());
                        state.tally.record_oversized_file();
                    }
                    Err(e) => {
                        log::warn!("import: linked file download failed, keeping remote URL: {file_url}: {e}");
                        state.assets.keep_remote(file_url.clone());
                        state.tally.record_unreachable_file();
                    }
                }
            }
        }

        let page_depth = url_to_file_path(&url, &scope).matches('/').count();
        // Nothing extracted is a page failure, not a crawl failure: one
        // unparseable page in a hundred must not cost the other ninety-nine.
        match compose_note(
            &mut article,
            &state.assets.local_map(page_depth),
            cover_remote,
            &url,
            Some(LinkScope { scope: &scope, rewrite_pages: config.recursive }),
        ) {
            Some(note) => {
                // Record this page's identity so a later duplicate of it —
                // by canonical (rule 1) or by same-path body (rule 2) — is
                // caught. See `Dedupe::record_written`.
                state.dedupe.record_written(&url, canonical.as_deref(), body_hash);
                PageOutcome::Written(note, article.widgets)
            }
            None => PageOutcome::Failed(
                "no article content found (unsupported page or empty body)".to_string(),
            ),
        }
        };

        match outcome {
            PageOutcome::Written(note, widgets) => {
                let relative = rename_for_collision(out_dir, &url_to_file_path(&url, &scope));
                write_note(out_dir, &relative, &note)?;
                if let Some(key) = norm_url(&url) {
                    written_urls.insert(key, relative.clone());
                }
                written.push(relative);
                state.tally.record_scraped();
                state.tally.record_widgets(&widgets);
                state.cap.record_progress(is_declared);
            }
            PageOutcome::Failed(reason) => {
                let relative = rename_for_collision(out_dir, &url_to_file_path(&url, &scope));
                write_note(out_dir, &relative, &render_error_markdown(&url, &reason))?;
                state.tally.record_failed();
                state.cap.record_progress(is_declared);
            }
            // Never written as a `.md` file at all — that is the whole fix.
            PageOutcome::Skipped => {
                state.tally.record_skipped();
                state.cap.record_progress(is_declared);
            }
            // Also never written — its links were already harvested above.
            // Never counted against the cap regardless of provenance, same
            // as before this fix.
            PageOutcome::Duplicate => {
                state.tally.record_duplicate();
            }
            // Never written as a `.md` stub — the content it would have
            // pointed at is already on disk under its own path. Not counted
            // against the cap, same as `Duplicate`: this URL was never a
            // real additional page to begin with.
            PageOutcome::VariantFetchFailed => {
                state.tally.record_unreachable_variant();
            }
            // Never written as a `.md` stub — the fetch never even reached
            // the point of proving the URL wasn't a page; the extension
            // already did. Not counted against the cap, same as
            // `VariantFetchFailed`: this URL was never a real additional
            // page to begin with.
            PageOutcome::UnreachableFile => {
                state.tally.record_unreachable_file();
            }
        }
    }

    // The site's chrome is written first: the home note's title becomes the
    // site name, which is the name the pass below strips from every title.
    let chrome = match &home_page {
        Some(home) => {
            let capture = Capture {
                out_dir,
                scope: &scope,
                user_agent: &config.user_agent,
                home,
                written: &written_urls,
            };
            design::capture_site_chrome(&capture, &mut state.pacer).await?
        }
        None => ChromeSummary::default(),
    };

    // What only the finished crawl can decide: a folder written after its
    // index page, and the site name every title shares.
    super::finalize::finalize_written_pages(out_dir, &written)?;

    on_progress(ScrapeProgress {
        pages_scraped: state.tally.scraped(),
        pages_failed: state.tally.failed(),
        current_url: None,
        complete: true,
    });

    // Discovered, in-scope URLs the cap left behind — see
    // `Frontier::remaining_unvisited_count`.
    let remaining_urls =
        if state.cap.capped() { state.frontier.remaining_unvisited_count() } else { 0 };

    let rate_limited_hosts: Vec<RateLimitedHost> = state
        .pacer
        .rate_limited_hosts()
        .into_iter()
        .map(|(host, interval)| RateLimitedHost { host, interval_ms: interval.as_millis() as u64 })
        .collect();

    Ok(ScrapeResult {
        success: true,
        total_pages: state.tally.scraped(),
        failed_pages: state.tally.failed(),
        skipped_pages: state.tally.skipped(),
        duplicate_pages: state.tally.duplicate(),
        unreachable_variants: state.tally.unreachable_variants(),
        unreachable_files: state.tally.unreachable_files(),
        linked_files_downloaded: state.tally.linked_files_downloaded(),
        oversized_files: state.tally.oversized_files(),
        widgets_carried: state.tally.widgets_carried(),
        widgets_dropped: state.tally.widgets_dropped(),
        capped: state.cap.capped(),
        remaining_urls,
        sitemap_urls: sitemap.urls.len(),
        sitemap_truncated: sitemap.truncated,
        rate_limited_hosts,
        chrome,
        error: None,
    })
}

/// The one place the importer writes a note. `relative` has already been
/// through the filename policy and the collision rename; the parent is created
/// here rather than from a precomputed ancestor list, because `create_dir_all`
/// on the deepest path already makes every level above it.
pub(crate) fn write_note(output_dir: &Path, relative: &str, content: &str) -> Result<(), String> {
    let file_path = output_dir.join(relative);
    if let Some(dir) = file_path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    // allow:raw_write the author's own source content, not `.moss/build.nosync/` output — an imported note
    fs::write(&file_path, content).map_err(|e| format!("Failed to write file: {}", e))
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
