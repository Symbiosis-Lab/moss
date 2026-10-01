//! Sitemap discovery for a recursive crawl.
//!
//! A crawl that only follows links never reaches a page nothing else on the
//! site links to, and stops discovering new pages at all once
//! [`super::service::DEFAULT_MAX_PAGES`] is hit. A site's sitemap is its own
//! declared page list — reading it first, before the link-following BFS in
//! [`super::run::scrape_to_folder`] starts, fixes both: an unlinked page gets
//! queued regardless, and a declared URL is exempt from the page cap that
//! still limits link-discovered extras.
//!
//! Fetches go through [`super::run::fetch_raw`] / [`super::run::fetch_raw_bytes`]
//! — the same SSRF-hardened, proxy-aware, retrying, size-capped fetch a page
//! fetch itself uses — so a hostile `Sitemap:` line or a redirect out of a
//! sitemap document is refused the same way a hostile page link would be.

use std::collections::{HashSet, VecDeque};
use std::io::Read;
use std::time::Duration;

use super::crawler::normalize_url;
use super::scope::{is_within_scope, UrlScope};

/// Hard ceiling on how many `<url>` entries a sitemap (or a chain of
/// `<sitemapindex>` documents reached from it) may contribute — independent
/// of the crawl's own `max_pages`, since a site's declared page list is
/// always imported (see [`super::run::scrape_to_folder`]'s cap check) and
/// nothing else would otherwise bound it. Counts every `<url>` entry seen,
/// including ones later dropped as out-of-scope or unparsable, so a
/// pathological sitemap cannot force unbounded work by other means.
pub const MAX_SITEMAP_URLS: usize = 5_000;

/// How many `<sitemapindex>` hops [`discover`] will follow before giving up.
/// A real site nests at most one or two levels (a top-level index naming
/// per-locale or per-section sitemaps); this is only a backstop against a
/// misconfigured or cyclical chain, not a limit real sitemaps approach.
const MAX_SITEMAP_INDEX_DEPTH: u32 = 5;

/// What discovering a site's sitemap turned up.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SitemapDiscovery {
    /// In-scope, normalized URLs the sitemap declares, in document order.
    /// Locale-alternate siblings are collapsed to one representative per
    /// group — see [`collapse_locale_alternates`] — so a UI-language mirror
    /// of the same content is not seeded as a second page.
    pub urls: Vec<String>,
    /// In-scope URLs the sitemap names as a `hreflang` alternate of one of
    /// the `urls` above, and which [`collapse_locale_alternates`] therefore
    /// dropped rather than seeding as a second page. Known duplicates, by
    /// the sitemap's own declaration — not merely unseeded. The crawl loop
    /// must never enqueue or fetch one of these EITHER, even when a link
    /// (not the sitemap) is what names it: a locale mirror is routinely
    /// self-canonical, so it escapes the crawl's own canonical-identity
    /// dedup on its own.
    pub locale_alternates: HashSet<String>,
    /// [`MAX_SITEMAP_URLS`] was hit and the sitemap's own declared list was
    /// truncated; the caller should say so rather than silently importing a
    /// partial list with no sign anything was cut.
    pub truncated: bool,
    /// The site's own `robots.txt` `Crawl-delay` (see
    /// [`parse_robots_crawl_delay`]), if it declared one — the crawl's
    /// per-host pacer (`crawl_state::HostPacer`) takes this as the floor
    /// for the crawl's own host, so a site that already told every crawler
    /// how fast it wants to be hit is never paced faster than that, even
    /// before the crawl has seen a single 429. `None` when `robots.txt`
    /// named no directive, or there is no `robots.txt` at all.
    pub crawl_delay: Option<Duration>,
}

/// Discover a site's sitemap and return its declared, in-scope URLs.
///
/// Tries `Sitemap:` lines in `robots.txt` first — a site can declare more
/// than one, e.g. one per locale — and falls back to `/sitemap.xml` at the
/// site root only when `robots.txt` itself named none. A site with no
/// sitemap at all (both fetches fail, or every named sitemap fails to fetch
/// or parse) returns an empty, non-truncated discovery, and the caller's
/// crawl then runs exactly as it did before this existed.
pub(crate) async fn discover(
    scope: &UrlScope,
    start_url: &str,
    user_agent: &str,
) -> SitemapDiscovery {
    let Some(root) = site_root(start_url) else {
        return SitemapDiscovery::default();
    };

    let robots_body = fetch_robots_txt(&root, user_agent).await;
    let mut seeds = robots_body.as_deref().map(parse_robots_sitemaps).unwrap_or_default();
    if seeds.is_empty() {
        seeds.push(format!("{root}/sitemap.xml"));
    }
    let crawl_delay = robots_body.as_deref().and_then(parse_robots_crawl_delay);

    let mut visited_docs: HashSet<String> = HashSet::new();
    let mut worklist: VecDeque<(String, u32)> = seeds.into_iter().map(|u| (u, 0)).collect();

    // One entry per `<url>` block, in document order: its own `loc`, plus
    // every in-scope `hreflang` alternate href it names. Collapsed into the
    // final URL list only after every sitemap document has been read, so an
    // alternate declared in a LATER document (e.g. a locale's own sitemap
    // naming the default-locale page as its alternate) still collapses
    // correctly regardless of which sitemap the index lists first.
    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    let mut raw_locs: usize = 0;
    let mut truncated = false;

    while let Some((doc_url, depth)) = worklist.pop_front() {
        if truncated {
            break;
        }
        if depth > MAX_SITEMAP_INDEX_DEPTH || !visited_docs.insert(doc_url.clone()) {
            continue;
        }
        let Ok(bytes) = super::run::fetch_raw_bytes(&doc_url, user_agent).await else {
            continue;
        };
        let body = decode_sitemap_bytes(&bytes);
        let Ok(xml) = roxmltree::Document::parse(&body) else {
            continue;
        };
        let root_el = xml.root_element();

        if root_el.has_tag_name("sitemapindex") {
            for sitemap_node in child_elements(root_el, "sitemap") {
                if let Some(loc) = child_text(sitemap_node, "loc") {
                    worklist.push_back((loc, depth + 1));
                }
            }
            continue;
        }
        if !root_el.has_tag_name("urlset") {
            continue;
        }

        for url_node in child_elements(root_el, "url") {
            if raw_locs >= MAX_SITEMAP_URLS {
                truncated = true;
                break;
            }
            raw_locs += 1;

            let Some(loc) = child_text(url_node, "loc") else {
                continue;
            };
            let Some(normalized) = normalize_url(&loc) else {
                continue;
            };
            if !is_within_scope(scope, &normalized) {
                continue;
            }
            let alternates: Vec<String> = url_node
                .children()
                .filter(|n| {
                    n.is_element() && n.has_tag_name("link") && n.attribute("rel") == Some("alternate")
                })
                .filter_map(|n| n.attribute("href"))
                .filter_map(normalize_url)
                .filter(|u| is_within_scope(scope, u))
                .collect();
            entries.push((normalized, alternates));
        }
    }

    let (urls, locale_alternates) = collapse_locale_alternates(entries);
    SitemapDiscovery {
        urls,
        locale_alternates,
        truncated,
        crawl_delay,
    }
}

/// Collapse a sitemap's own `hreflang`-alternate annotations — the sitemap
/// protocol's standard, host-agnostic extension for declaring the SAME
/// content at different locale URLs (`xmlns:xhtml`'s `<xhtml:link
/// rel="alternate" hreflang="…">` inside a `<url>` block) — so a UI-language
/// mirror is never seeded as a second page. An entry already named as
/// another, earlier entry's alternate is dropped; the earlier entry (in
/// document order) is kept.
///
/// This is deliberately independent of `<link rel="canonical">` (the crawl
/// loop's own rule 1, `extract_canonical_url`): a locale mirror is routinely
/// self-canonical — its OWN URL, never the other locale's — precisely
/// because each locale is meant to be independently indexable, so the
/// canonical tag carries no signal that would let rule 1 catch this case.
/// The sitemap's `hreflang` annotation is the generic signal that does.
/// Returns the kept representatives, plus every URL ever claimed as an
/// alternate that ISN'T one of them — a URL can be claimed more than once
/// (a three-way `zh`/`en`/`fr` group each naming the other two) and can be
/// claimed without ever appearing as its own `<url>` entry at all; either
/// way, it is a known duplicate the moment the sitemap names it as someone
/// else's alternate.
fn collapse_locale_alternates(entries: Vec<(String, Vec<String>)>) -> (Vec<String>, HashSet<String>) {
    let mut claimed: HashSet<String> = HashSet::new();
    let mut kept: Vec<String> = Vec::new();
    for (loc, alternates) in entries {
        if claimed.contains(&loc) {
            continue;
        }
        kept.push(loc.clone());
        claimed.insert(loc);
        claimed.extend(alternates);
    }
    let kept_set: HashSet<&String> = kept.iter().collect();
    let dropped: HashSet<String> =
        claimed.into_iter().filter(|u| !kept_set.contains(u)).collect();
    (kept, dropped)
}

/// A discovered `link` (or manifest-declared page) that the sitemap itself
/// already named as a `hreflang` alternate of a page it declared — a known
/// duplicate, whether the sitemap or, as here, an ordinary link is what led
/// the crawl to it. The crawl loop calls this at discovery time, on every
/// link and manifest-declared page it finds, so a locale mirror is never
/// enqueued or fetched at all, not merely deduped after the fact. `visited`
/// doubles as "already counted": the SAME locale mirror is routinely linked
/// from every sibling page in the group (a per-page language toggle), and
/// this must count it once, not once per page that links to it.
pub(crate) fn block_if_locale_alternate(
    link: &str,
    locale_alternates: &HashSet<String>,
    visited: &mut HashSet<String>,
    pages_duplicate: &mut usize,
) -> bool {
    if !locale_alternates.contains(link) {
        return false;
    }
    if visited.insert(link.to_string()) {
        *pages_duplicate += 1;
    }
    true
}

/// `parent`'s direct child elements named `tag` (namespace-agnostic, same as
/// every other tag match in this module — see `Node::has_tag_name`'s own
/// documented behavior for a plain `&str`).
fn child_elements<'a, 'input>(
    parent: roxmltree::Node<'a, 'input>,
    tag: &'a str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    parent.children().filter(move |n| n.is_element() && n.has_tag_name(tag))
}

/// The trimmed text of `parent`'s first direct child element named `tag`.
/// `None` for a missing child or one with no (or only blank) text content.
fn child_text(parent: roxmltree::Node<'_, '_>, tag: &str) -> Option<String> {
    child_elements(parent, tag)
        .find_map(|n| n.text())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `scheme://host[:port]`, no trailing slash — the base a `robots.txt` and a
/// default `/sitemap.xml` fetch are both resolved against.
fn site_root(start_url: &str) -> Option<String> {
    let mut url = url::Url::parse(start_url).ok()?;
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Some(url.as_str().trim_end_matches('/').to_string())
}

/// Parse `Sitemap:` lines out of a `robots.txt` body, in the order they
/// appear. Matched case-insensitively — the directive name's casing is not
/// consistent across real `robots.txt` files in the wild, even though every
/// spec example spells it `Sitemap:`.
fn parse_robots_sitemaps(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            let lower = line.to_ascii_lowercase();
            let value = lower.strip_prefix("sitemap:")?;
            let cut = line.len() - value.len();
            Some(line[cut..].trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// A fetched sitemap document's bytes, decoded to UTF-8 text — transparently
/// decompressed first when the sitemap protocol's own gzip convention
/// applies (sitemaps.org: a site may serve `sitemap.xml.gz` directly, no
/// `Content-Encoding` header involved, since ureq itself is never built with
/// gzip transfer-encoding support here — see `run::fetch_raw_bytes`'s doc).
/// Detected by the gzip magic bytes rather than the URL's own suffix, since
/// a CDN or redirect can serve one without the other. A body that claims to
/// be gzip but fails to inflate decodes to an empty string, which fails the
/// caller's XML parse exactly like any other malformed document — this
/// function never itself signals success or failure, only text.
fn decode_sitemap_bytes(bytes: &[u8]) -> String {
    const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
    if bytes.starts_with(&GZIP_MAGIC) {
        let mut decompressed = String::new();
        return match flate2::read::GzDecoder::new(bytes).read_to_string(&mut decompressed) {
            Ok(_) => decompressed,
            Err(_) => String::new(),
        };
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Fetches `robots.txt` once, for both the sitemap seeds
/// ([`parse_robots_sitemaps`]) and the `Crawl-delay` floor
/// ([`parse_robots_crawl_delay`]) [`discover`] reads out of the same body —
/// a site with none (or an unreachable one) returns `None`, and the caller
/// falls back to `/sitemap.xml` and an unpaced crawl exactly as before
/// either existed.
async fn fetch_robots_txt(root: &str, user_agent: &str) -> Option<String> {
    super::run::fetch_raw(&format!("{root}/robots.txt"), user_agent).await.ok()
}

/// Parse a `Crawl-delay:` directive out of a `robots.txt` body — not part of
/// the original robots.txt standard, but a de facto convention several
/// major crawlers honor, for a host to declare the minimum gap (in seconds,
/// a plain number, fractional allowed) it wants between requests. Matched
/// case-insensitively, same as `Sitemap:` above, with no user-agent-block
/// scoping — the first valid value anywhere in the file wins, the same
/// simple reading this parser already gives `Sitemap:` lines. A missing,
/// unparsable, or non-positive value is `None` rather than "no delay at
/// all" — the same non-contradiction `run.rs`'s `Retry-After` parsing
/// already applies to a zero or negative wait.
fn parse_robots_crawl_delay(body: &str) -> Option<Duration> {
    body.lines().find_map(|line| {
        let line = line.trim();
        let lower = line.to_ascii_lowercase();
        let value = lower.strip_prefix("crawl-delay:")?;
        let cut = line.len() - value.len();
        let secs: f64 = line[cut..].trim().parse().ok()?;
        (secs > 0.0).then(|| Duration::from_secs_f64(secs))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_robots_sitemaps (pure, no network) ────────────────────────

    #[test]
    fn parse_robots_sitemaps_reads_one_line() {
        let body = "User-agent: *\nDisallow: /admin\nSitemap: https://example.test/sitemap.xml\n";
        assert_eq!(
            parse_robots_sitemaps(body),
            vec!["https://example.test/sitemap.xml".to_string()]
        );
    }

    #[test]
    fn parse_robots_sitemaps_reads_several_in_order() {
        let body = "Sitemap: https://example.test/a.xml\nSitemap: https://example.test/b.xml\n";
        assert_eq!(
            parse_robots_sitemaps(body),
            vec![
                "https://example.test/a.xml".to_string(),
                "https://example.test/b.xml".to_string(),
            ]
        );
    }

    #[test]
    fn parse_robots_sitemaps_is_case_insensitive() {
        let body = "sitemap: https://example.test/lower.xml\nSITEMAP: https://example.test/upper.xml\n";
        assert_eq!(
            parse_robots_sitemaps(body),
            vec![
                "https://example.test/lower.xml".to_string(),
                "https://example.test/upper.xml".to_string(),
            ]
        );
    }

    #[test]
    fn parse_robots_sitemaps_absent_returns_empty() {
        let body = "User-agent: *\nDisallow: /\n";
        assert!(parse_robots_sitemaps(body).is_empty());
    }

    // ── parse_robots_crawl_delay (pure, no network) ─────────────────────

    #[test]
    fn parse_robots_crawl_delay_reads_a_plain_integer() {
        let body = "User-agent: *\nCrawl-delay: 5\n";
        assert_eq!(parse_robots_crawl_delay(body), Some(Duration::from_secs(5)));
    }

    #[test]
    fn parse_robots_crawl_delay_reads_a_fractional_value() {
        assert_eq!(
            parse_robots_crawl_delay("Crawl-delay: 1.5\n"),
            Some(Duration::from_millis(1500))
        );
    }

    #[test]
    fn parse_robots_crawl_delay_is_case_insensitive() {
        assert_eq!(parse_robots_crawl_delay("CRAWL-DELAY: 2\n"), Some(Duration::from_secs(2)));
    }

    #[test]
    fn parse_robots_crawl_delay_absent_returns_none() {
        assert_eq!(parse_robots_crawl_delay("User-agent: *\nDisallow: /\n"), None);
    }

    /// Same non-contradiction `run.rs`'s `Retry-After` parsing applies to a
    /// zero or negative wait: a directive naming one is not read as license
    /// for an instant request, only as naming nothing useful.
    #[test]
    fn parse_robots_crawl_delay_rejects_zero_and_negative() {
        assert_eq!(parse_robots_crawl_delay("Crawl-delay: 0\n"), None);
        assert_eq!(parse_robots_crawl_delay("Crawl-delay: -1\n"), None);
    }

    // ── collapse_locale_alternates (pure, no network) ───────────────────

    #[test]
    fn collapse_locale_alternates_keeps_the_first_of_a_mutual_pair() {
        let entries = vec![
            (
                "https://example.test/post".to_string(),
                vec!["https://example.test/en/post".to_string()],
            ),
            (
                "https://example.test/en/post".to_string(),
                vec!["https://example.test/post".to_string()],
            ),
        ];
        let (kept, dropped) = collapse_locale_alternates(entries);
        assert_eq!(kept, vec!["https://example.test/post".to_string()]);
        assert_eq!(
            dropped,
            HashSet::from(["https://example.test/en/post".to_string()])
        );
    }

    #[test]
    fn collapse_locale_alternates_leaves_unrelated_entries_alone() {
        let entries = vec![
            ("https://example.test/a".to_string(), vec![]),
            ("https://example.test/b".to_string(), vec![]),
        ];
        let (kept, dropped) = collapse_locale_alternates(entries);
        assert_eq!(
            kept,
            vec!["https://example.test/a".to_string(), "https://example.test/b".to_string()]
        );
        assert!(dropped.is_empty());
    }

    #[test]
    fn collapse_locale_alternates_drops_an_alternate_never_seen_as_its_own_entry() {
        // The alternate href is claimed the moment a KEPT entry names it —
        // even if that exact URL never shows up as its own `<url>` block
        // anywhere in the sitemap.
        let entries = vec![(
            "https://example.test/post".to_string(),
            vec!["https://example.test/en/post".to_string()],
        )];
        let (kept, dropped) = collapse_locale_alternates(entries);
        assert_eq!(kept, vec!["https://example.test/post".to_string()]);
        assert_eq!(
            dropped,
            HashSet::from(["https://example.test/en/post".to_string()])
        );
    }

    // ── block_if_locale_alternate (the crawl loop's own discovery-time
    // block, independent of whether the URL was seeded by the sitemap) ────

    #[test]
    fn block_if_locale_alternate_blocks_and_counts_once() {
        let locale_alternates = HashSet::from(["https://example.test/en/post".to_string()]);
        let mut visited = HashSet::new();
        let mut duplicates = 0;

        assert!(block_if_locale_alternate(
            "https://example.test/en/post",
            &locale_alternates,
            &mut visited,
            &mut duplicates,
        ));
        assert_eq!(duplicates, 1);

        // A second page linking the SAME mirror must not double-count it.
        assert!(block_if_locale_alternate(
            "https://example.test/en/post",
            &locale_alternates,
            &mut visited,
            &mut duplicates,
        ));
        assert_eq!(duplicates, 1, "the same URL is counted once, not once per linking page");
    }

    #[test]
    fn block_if_locale_alternate_leaves_a_non_alternate_url_alone() {
        let locale_alternates = HashSet::from(["https://example.test/en/post".to_string()]);
        let mut visited = HashSet::new();
        let mut duplicates = 0;

        assert!(!block_if_locale_alternate(
            "https://example.test/en/other",
            &locale_alternates,
            &mut visited,
            &mut duplicates,
        ));
        assert_eq!(duplicates, 0);
        assert!(visited.is_empty(), "a non-alternate URL is left for the ordinary crawl to handle");
    }

    // ── site_root ────────────────────────────────────────────────────────

    #[test]
    fn site_root_strips_path_query_and_fragment() {
        assert_eq!(
            site_root("https://example.test/blog/post?x=1#frag"),
            Some("https://example.test".to_string())
        );
    }

    #[test]
    fn site_root_keeps_a_nonstandard_port() {
        assert_eq!(
            site_root("http://example.test:8080/"),
            Some("http://example.test:8080".to_string())
        );
    }

    // ── decode_sitemap_bytes (a site may serve `sitemap.xml.gz` directly,
    // per the sitemap protocol's own gzip convention, with no
    // `Content-Encoding` header involved) ───────────────────────────────

    #[test]
    fn decode_sitemap_bytes_passes_plain_xml_through() {
        let xml = "<?xml version=\"1.0\"?><urlset></urlset>";
        assert_eq!(decode_sitemap_bytes(xml.as_bytes()), xml);
    }

    #[test]
    fn decode_sitemap_bytes_inflates_a_gzip_body() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let xml = "<?xml version=\"1.0\"?><urlset><url><loc>https://example.test/a</loc></url></urlset>";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(xml.as_bytes()).unwrap();
        let gz_bytes = encoder.finish().unwrap();

        // Not merely non-empty: a caller that forgot to gunzip at all would
        // hand the raw compressed bytes to `String::from_utf8_lossy`, which
        // never fails — it would produce a REPLACEMENT-CHARACTER-laden
        // string that still fails the caller's XML parse, but for the wrong
        // reason. Asserting the exact original text is what proves
        // decompression actually ran, not merely that failure was avoided.
        assert_eq!(decode_sitemap_bytes(&gz_bytes), xml);
    }

    #[test]
    fn decode_sitemap_bytes_on_a_body_merely_claiming_to_be_gzip_returns_empty() {
        // The gzip magic bytes with a truncated/corrupt body behind them:
        // must not panic, and must fail the caller's XML parse rather than
        // silently succeed on garbage.
        let fake_gzip = [0x1f, 0x8b, 0x00, 0x00];
        assert_eq!(decode_sitemap_bytes(&fake_gzip), "");
    }
}
