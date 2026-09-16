//! HTML → Markdown extraction.
//!
//! Three-stage hand-rolled pipeline (replaces defuddle-rs as of 2026-05-20):
//!
//! 1. [`super::extractor::extract_main_content`] — strip clutter, score
//!    candidates, return the article's inner HTML. Distilled from
//!    [kepano/defuddle](https://github.com/kepano/defuddle).
//! 2. [`htmd::convert`] — convert that HTML to Markdown. Turndown-inspired,
//!    handles headings/lists/blockquotes/code/tables/links/images.
//! 3. [`super::metadata::derive`] — parse JSON-LD, OpenGraph, `<html lang>`
//!    on the *raw* HTML (before extraction) into an [`ArticleMetadata`]
//!    aligned with moss's `BUILTIN_FIELDS`.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;
use url::Url;

use super::extractor::extract_main_content;
use super::metadata::{derive, ArticleMetadata};

/// Output of the extraction pipeline.
pub struct Article {
    /// Markdown body produced from the article's main-content subtree.
    pub markdown: String,
    /// Absolute URLs of every image referenced by the markdown.
    pub media_urls: HashSet<String>,
    /// Metadata fields ready for YAML frontmatter, schema-aligned with
    /// moss's `BUILTIN_FIELDS`.
    pub metadata: ArticleMetadata,
}

/// What a per-site adapter can supply. Body may be pre-converted markdown
/// (adapter built it from structured data, e.g. Strikingly's `$S` JSON) or
/// HTML for htmd conversion (douban's selected content subtree).
pub(crate) enum AdapterBody {
    Html(String),
    Markdown(String),
}

/// Metadata fields a per-site adapter knows better than the generic
/// `derive()` pass. `Some` wins over the derived value; `None` leaves it.
#[derive(Default)]
pub(crate) struct MetadataOverrides {
    pub title: Option<String>,
    pub date: Option<String>,
    pub publisher: Option<String>,
    pub author: Option<String>,
}

/// A per-site adapter's full answer: the body plus metadata overrides.
pub(crate) struct SiteContent {
    pub body: AdapterBody,
    pub overrides: MetadataOverrides,
}

/// Apply adapter metadata overrides onto the derived metadata: an override
/// replaces the derived value only when `Some`.
fn apply_overrides(metadata: &mut ArticleMetadata, overrides: MetadataOverrides) {
    let MetadataOverrides {
        title,
        date,
        publisher,
        author,
    } = overrides;
    if title.is_some() {
        metadata.title = title;
    }
    if date.is_some() {
        metadata.date = date;
    }
    if publisher.is_some() {
        metadata.publisher = publisher;
    }
    if author.is_some() {
        metadata.author = author;
    }
}

/// Extract article markdown + metadata from raw HTML.
///
/// `base_url` is used to resolve relative image URLs into absolute ones.
pub fn extract_article(html: &str, base_url: &str) -> Article {
    extract_article_with_snapshot(html, None, base_url)
}

/// Evidence-aware extraction: `snapshot` is the page's pre-rendered mirror
/// when the crawl loop fetched one (client-rendered builders keep their
/// content there, not in the served DOM).
pub fn extract_article_with_snapshot(
    html: &str,
    snapshot: Option<&str>,
    base_url: &str,
) -> Article {
    let base = Url::parse(base_url).ok();

    // Metadata is parsed from the raw HTML so JSON-LD / OG /
    // <html lang> survive the clutter strip.
    let mut metadata = derive(html);

    // Main content as markdown. Known sites whose DOM defeats the generic
    // scorer get an explicit adapter first (which may hand back ready
    // markdown or HTML for htmd conversion, plus metadata overrides); every
    // other site falls back to the defuddle-distilled scorer.
    let markdown = match crate::vault::import::engine::extract_with_evidence(
        html, snapshot, base_url,
    ) {
        Some(SiteContent {
            body: AdapterBody::Markdown(md),
            overrides,
        }) => {
            apply_overrides(&mut metadata, overrides);
            md
        }
        Some(SiteContent {
            body: AdapterBody::Html(h),
            overrides,
        }) => {
            apply_overrides(&mut metadata, overrides);
            htmd::convert(&h).unwrap_or(h)
        }
        None => {
            let h = extract_main_content(html);
            htmd::convert(&h).unwrap_or_else(|_| h.clone())
        }
    };

    let mut media_urls: HashSet<String> = HashSet::new();
    let mut raw_to_resolved: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for raw in extract_image_urls_in_markdown(&markdown) {
        if let Some(abs) = resolve_url(&raw, &base) {
            if abs != raw {
                raw_to_resolved.insert(raw.clone(), abs.clone());
            }
            media_urls.insert(abs);
        }
    }
    // Normalize markdown tokens to the resolved form — downstream download
    // and rewrite maps are keyed by it. Protocol-relative (`//host/...`) and
    // root-relative (`/path`) tokens would otherwise never match the
    // resolved-URL keys in `media_urls` / `remote_to_local`.
    let markdown = if raw_to_resolved.is_empty() {
        markdown
    } else {
        rewrite_image_links(&markdown, &raw_to_resolved)
    };
    // File-carrying wikilink embeds (`![[https://…]]` audio/documents, e.g.
    // localized Drive downloads) are queued for the same asset pass;
    // provider embeds (youtube/vimeo) stay remote by the localizable test.
    for url in extract_localizable_embed_urls(&markdown) {
        media_urls.insert(url);
    }
    // Ensure the og:image URL is queued for download so the cover selection
    // in `scrape::run` can find it in `remote_to_local` after the asset
    // loop. og:image is always an absolute URL — resolve against base anyway
    // to normalise trailing slashes, but fall back to inserting it verbatim.
    if let Some(ref og_url) = metadata.og_image {
        let abs = resolve_url(og_url, &base).unwrap_or_else(|| og_url.clone());
        media_urls.insert(abs.clone());
        // Keep og_image in the resolved form: the cover selection in
        // `scrape::run` uses it as a lookup key into maps keyed by the
        // resolved URL, and the two forms differ whenever Url::to_string
        // normalizes (percent-encoding of non-ASCII paths, trailing slashes).
        metadata.og_image = Some(abs);
    }

    Article {
        markdown,
        media_urls,
        metadata,
    }
}

/// Pages whose DOM defeats the generic [`extract_main_content`] scorer route
/// through the import engine's dialect table (content-detected builders like
/// Strikingly, host-gated selector hints like douban). `None` → the generic
/// scorer. Kept as a named seam so the converter tests pin the behavior.
#[cfg(test)]
fn site_specific_content(html: &str, url: &str) -> Option<SiteContent> {
    crate::vault::import::engine::extract_with_evidence(html, None, url)
}

/// Absolute wikilink-embed URLs (`![[https://…]]`) that name downloadable
/// FILES the import should localize — Drive direct-downloads and bare
/// file-extension URLs. Provider embeds are excluded.
fn extract_localizable_embed_urls(md: &str) -> Vec<String> {
    WIKILINK_EMBED_PATTERN
        .captures_iter(md)
        .map(|c| c[1].to_string())
        .filter(|u| crate::vault::import::media::is_localizable_file_url(u))
        .collect()
}

/// Rewrite `![[URL]]` embeds through the same `remote → local` map the
/// image rewrite uses, so downloaded files (audio, documents) point into
/// the vault. URLs not in the map are left untouched.
pub fn rewrite_embed_links(
    markdown: &str,
    remote_to_local: &std::collections::HashMap<String, String>,
) -> String {
    WIKILINK_EMBED_PATTERN
        .replace_all(markdown, |caps: &regex::Captures| {
            match remote_to_local.get(&caps[1]) {
                Some(local) => format!("![[{}]]", local),
                None => caps[0].to_string(),
            }
        })
        .to_string()
}

/// Rewrite every `![alt](URL)` in the markdown using a `remote → local`
/// lookup map. References whose URL isn't in the map are left untouched.
pub fn rewrite_image_links(
    markdown: &str,
    remote_to_local: &std::collections::HashMap<String, String>,
) -> String {
    IMG_PATTERN
        .replace_all(markdown, |caps: &regex::Captures| {
            let alt = &caps[1];
            let url = &caps[2];
            match remote_to_local.get(url) {
                Some(local) => format!("![{}]({})", alt, local),
                None => caps[0].to_string(),
            }
        })
        .to_string()
}

fn extract_image_urls_in_markdown(md: &str) -> Vec<String> {
    IMG_PATTERN
        .captures_iter(md)
        .map(|c| c[2].to_string())
        .filter(|u| !u.is_empty() && !u.starts_with("data:"))
        .collect()
}

/// Return the first non-`data:` image URL in the markdown, resolved against
/// `base_url`. Used by the import pipeline to populate `cover:` frontmatter
/// from the article's hero image. Returns `None` if no image survives the
/// extraction.
pub fn first_image_url(md: &str, base_url: &str) -> Option<String> {
    let base = Url::parse(base_url).ok();
    IMG_PATTERN.captures_iter(md).find_map(|c| {
        let raw = c[2].to_string();
        if raw.is_empty() || raw.starts_with("data:") {
            None
        } else {
            resolve_url(&raw, &base)
        }
    })
}

fn resolve_url(href: &str, base: &Option<Url>) -> Option<String> {
    if let Some(b) = base {
        b.join(href).ok().map(|u| u.to_string())
    } else {
        Url::parse(href).ok().map(|u| u.to_string())
    }
}

// Matches `![alt](url)` markdown image references. Captures alt and URL.
//
// The URL group uses `(?:[^()\s"'<>]|\([^()]*\))+` instead of the naive
// `[^)]+` to handle URLs that contain literal parentheses — e.g.
// `https://cdn.example.com/photo(2024).jpg`.  The pattern accepts either a
// non-paren, non-whitespace character OR a single balanced `(…)` group, which
// covers the full CommonMark spec allowance for one level of nested parens in
// link destinations (CommonMark spec §6.6, link destination grammar).
static IMG_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"!\[([^\]]*)\]\(((?:[^()\s"'<>]|\([^()]*\))+)\)"#).expect("img regex")
});

// Matches `![[https://…]]` wikilink embeds with an absolute URL target (no
// alias pipe). Captures the URL. Local-path embeds never match.
static WIKILINK_EMBED_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"!\[\[(https?://[^\]\|]+)\]\]"#).expect("wikilink embed regex")
});

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn extracts_realistic_article() {
        let html = r##"<!doctype html><html lang="en"><head>
            <title>Title — The Site</title>
            <script type="application/ld+json">
            {"@type":"NewsArticle","headline":"Title",
             "datePublished":"2024-12-22T00:00:00Z",
             "author":{"@type":"Person","name":"Yi Liu"},
             "publisher":{"@id":"#org"}}
            </script>
            <script type="application/ld+json">
            {"@type":"Organization","@id":"#org","name":"The Site"}
            </script>
            </head><body>
            <header>site logo</header>
            <article>
              <h1>Title</h1>
              <p>Lede paragraph with enough words for the scorer to like it.</p>
              <p>Second paragraph keeps the article body weight up.</p>
            </article>
            <footer>site footer</footer>
            </body></html>"##;
        let art = extract_article(html, "https://example.com/p");
        assert_eq!(art.metadata.title.as_deref(), Some("Title"));
        assert_eq!(art.metadata.date.as_deref(), Some("2024-12-22"));
        assert_eq!(art.metadata.author.as_deref(), Some("Yi Liu"));
        assert_eq!(art.metadata.publisher.as_deref(), Some("The Site"));
        assert_eq!(art.metadata.lang.as_deref(), Some("en"));
        assert!(art.markdown.contains("Lede paragraph"));
        assert!(!art.markdown.contains("site logo"));
        assert!(!art.markdown.contains("site footer"));
    }

    #[test]
    fn extract_image_urls_finds_absolute_and_relative() {
        let md = "![A](https://cdn.example.com/a.png)\n\nBody\n\n![B](/img/b.jpg)";
        let base = Some(Url::parse("https://example.com/post/").unwrap());
        let mut urls = HashSet::new();
        for raw in extract_image_urls_in_markdown(md) {
            if let Some(abs) = resolve_url(&raw, &base) {
                urls.insert(abs);
            }
        }
        assert!(urls.contains("https://cdn.example.com/a.png"));
        assert!(urls.contains("https://example.com/img/b.jpg"));
    }

    #[test]
    fn rewrite_image_links_substitutes_known_urls() {
        let md = "![alt](https://cdn.example.com/a.png) and ![other](https://elsewhere.example/b.jpg).";
        let mut map = HashMap::new();
        map.insert(
            "https://cdn.example.com/a.png".to_string(),
            "./assets/imported/abcd.png".to_string(),
        );
        let out = rewrite_image_links(md, &map);
        assert!(out.contains("![alt](./assets/imported/abcd.png)"));
        assert!(out.contains("https://elsewhere.example/b.jpg"));
    }

    #[test]
    fn wikilink_embeds_localize_files_but_not_providers() {
        let md = "![[https://drive.google.com/uc?export=download&id=A]]\n\n                  ![[https://www.youtube.com/watch?v=x1]]\n\n![[local/song.mp3]]";
        assert_eq!(
            extract_localizable_embed_urls(md),
            vec!["https://drive.google.com/uc?export=download&id=A"]
        );
        let mut map = HashMap::new();
        map.insert(
            "https://drive.google.com/uc?export=download&id=A".to_string(),
            "./assets/imported/h.mp3".to_string(),
        );
        let out = rewrite_embed_links(md, &map);
        assert!(out.contains("![[./assets/imported/h.mp3]]"), "got: {out}");
        assert!(
            out.contains("![[https://www.youtube.com/watch?v=x1]]"),
            "provider embed must stay remote: {out}"
        );
        assert!(out.contains("![[local/song.mp3]]"), "local untouched: {out}");
    }


    #[test]
    fn data_uri_images_ignored() {
        let md = "![inline](data:image/png;base64,iVBOR)";
        let urls = extract_image_urls_in_markdown(md);
        assert!(urls.is_empty());
    }

    #[test]
    fn image_url_with_parentheses_preserved() {
        // URLs like https://cdn.example.com/photo(2024).jpg contain literal
        // parentheses. The regex must capture the full URL, not truncate at the
        // first ')'. Both extraction and rewrite must round-trip correctly.
        let md = "![img](https://cdn.example.com/photo(2024).jpg)";

        // extract: full URL including the parens must be returned
        let urls = extract_image_urls_in_markdown(md);
        assert_eq!(
            urls,
            vec!["https://cdn.example.com/photo(2024).jpg"],
            "extract_image_urls_in_markdown truncated URL at first ')'"
        );

        // rewrite: the substitution map keyed on the full URL must match
        let mut map = std::collections::HashMap::new();
        map.insert(
            "https://cdn.example.com/photo(2024).jpg".to_string(),
            "./assets/imported/photo2024.jpg".to_string(),
        );
        let out = rewrite_image_links(md, &map);
        assert!(
            out.contains("./assets/imported/photo2024.jpg"),
            "rewrite_image_links did not substitute URL with parens; got: {out}"
        );

        // first_image_url must also resolve correctly
        let first = first_image_url(md, "https://cdn.example.com/");
        assert_eq!(
            first.as_deref(),
            Some("https://cdn.example.com/photo(2024).jpg"),
            "first_image_url returned wrong URL: {first:?}"
        );
    }

    /// og:image beats the first body image for cover selection.
    ///
    /// When both `og:image` and a body image are present, `og_image` should be
    /// populated from the meta tag and included in `media_urls`, so that
    /// `scrape::run` can prefer it over the first body image.
    #[test]
    fn og_image_normalized_to_resolved_form_for_cover_lookup() {
        // The cover selection in `scrape::run` looks up `og_image` in a
        // map keyed by the RESOLVED URL form (what `media_urls` holds). A raw
        // og:image whose resolved form differs — CJK path chars get
        // percent-encoded by Url::to_string — must be normalized in the
        // metadata too, or the cover silently vanishes.
        let html = r#"<html><head><title>t</title>
            <meta property="og:image" content="https://cdn.example.com/封面.jpg">
            </head><body><article>
            <p>body text long enough to be the main content candidate.</p>
            </article></body></html>"#;
        let art = extract_article(html, "https://site.example.com/post");
        let og = art.metadata.og_image.as_deref().expect("og_image set");
        assert!(
            art.media_urls.contains(og),
            "og_image ({og}) must equal its media_urls key exactly"
        );
        assert_eq!(og, "https://cdn.example.com/%E5%B0%81%E9%9D%A2.jpg");
    }

    #[test]
    fn og_image_preferred_over_first_body_image() {
        let html = r##"<!doctype html><html lang="en"><head>
            <title>Test Article</title>
            <meta property="og:image" content="https://cdn.example.com/og-cover.jpg">
            </head><body>
            <article>
              <p>Article body with enough text to pass content scoring.</p>
              <img src="https://cdn.example.com/body-image.png" alt="body image">
              <p>More text to keep the article scorer happy with the candidate.</p>
            </article>
            </body></html>"##;

        let art = extract_article(html, "https://example.com/article");

        // og_image must be captured from the meta tag.
        assert_eq!(
            art.metadata.og_image.as_deref(),
            Some("https://cdn.example.com/og-cover.jpg"),
            "og_image should be set from <meta property=\"og:image\">"
        );

        // og:image URL must be in media_urls so the download loop fetches it.
        assert!(
            art.media_urls
                .contains("https://cdn.example.com/og-cover.jpg"),
            "og:image URL must be queued in media_urls for download"
        );
    }

    // ---- douban site-adapter --------------------------------------------

    const DOUBAN_REVIEW: &str = r##"<html><head>
        <title>笔记（Man's Search for Meaning）书评</title></head><body>
        <div id="db-global-nav"><ul class="nav">
          <li><a href="https://www.douban.com">豆瓣</a></li>
          <li><a href="https://movie.douban.com">电影</a></li>
          <li><a href="https://music.douban.com">音乐</a></li>
        </ul></div>
        <div id="content"><div class="article">
          <div id="link-report-8218385"><div class="review-content clearfix">
            <p>维克多·弗兰克认为驱动人前进的根源动力是意义意志。</p>
            <p>书中这句最令我印象深刻。</p>
          </div></div>
        </div></div>
        </body></html>"##;

    const DOUBAN_NOTE: &str = r##"<html><head><title>长青的乌羽玉</title></head><body>
        <div id="db-global-nav"><ul class="nav"><li><a href="/">豆瓣</a></li>
          <li><a href="https://movie.douban.com">电影</a></li></ul></div>
        <div id="link-report"><div class="note">
          <h2>通灵的仙人掌</h2><p>乌羽玉是一种原产美洲的仙人球。</p>
        </div></div>
        </body></html>"##;

    /// Unwrap an adapter result expected to carry an HTML body.
    fn expect_html_body(content: SiteContent) -> String {
        match content.body {
            AdapterBody::Html(h) => h,
            AdapterBody::Markdown(_) => panic!("douban adapter returns HTML, not markdown"),
        }
    }

    #[test]
    fn douban_review_adapter_selects_review_body_not_nav() {
        let content =
            site_specific_content(DOUBAN_REVIEW, "https://book.douban.com/review/8218385/")
                .expect("douban host should match the site adapter");
        let out = expect_html_body(content);
        assert!(out.contains("驱动人前进的根源动力"), "got: {out}");
        assert!(!out.contains("电影"), "nav chrome leaked in: {out}");
    }

    #[test]
    fn douban_note_adapter_selects_note_body() {
        let content = site_specific_content(DOUBAN_NOTE, "https://www.douban.com/note/614422027/")
            .expect("douban host should match the site adapter");
        let out = expect_html_body(content);
        assert!(out.contains("通灵的仙人掌"), "got: {out}");
        assert!(out.contains("原产美洲的仙人球"), "got: {out}");
        assert!(!out.contains("电影"), "nav chrome leaked in: {out}");
    }

    #[test]
    fn site_adapter_returns_none_for_non_douban() {
        let html = "<html><body><article><p>hello</p></article></body></html>";
        assert!(site_specific_content(html, "https://example.com/post").is_none());
    }

    #[test]
    fn protocol_relative_images_resolved_in_markdown_and_media_urls() {
        let html = r#"<html><head><title>t</title></head><body><article>
            <p>body text long enough to be the main content candidate.</p>
            <img src="//cdn.example.com/pic.jpg" alt="a">
            <img src="/local/pic2.jpg" alt="b">
            </article></body></html>"#;
        let art = extract_article(html, "https://site.example.com/post");
        assert!(art.media_urls.contains("https://cdn.example.com/pic.jpg"));
        assert!(art
            .media_urls
            .contains("https://site.example.com/local/pic2.jpg"));
        // The markdown tokens must be the SAME resolved URLs, so that
        // rewrite_image_links (keyed by resolved URL) finds them.
        assert!(
            art.markdown.contains("(https://cdn.example.com/pic.jpg)"),
            "got: {}",
            art.markdown
        );
        assert!(art
            .markdown
            .contains("(https://site.example.com/local/pic2.jpg)"));
    }

    #[test]
    fn metadata_overrides_win_when_some() {
        // An adapter override replaces the derived value only when Some —
        // None leaves the derived metadata untouched.
        let mut meta = ArticleMetadata {
            title: Some("Derived Title — Site".into()),
            date: None,
            author: Some("Derived Author".into()),
            ..Default::default()
        };
        apply_overrides(
            &mut meta,
            MetadataOverrides {
                title: Some("Clean Title".into()),
                date: Some("2024-10-13".into()),
                publisher: None,
                author: None,
            },
        );
        assert_eq!(meta.title.as_deref(), Some("Clean Title"));
        assert_eq!(meta.date.as_deref(), Some("2024-10-13"));
        assert_eq!(meta.author.as_deref(), Some("Derived Author"));
        assert_eq!(meta.publisher, None);
    }

    #[test]
    fn extract_article_routes_douban_review_through_adapter() {
        // End to end: the generic scorer would let `#content` swallow the nav,
        // so this asserts the adapter path produces a clean review body.
        let art = extract_article(DOUBAN_REVIEW, "https://book.douban.com/review/8218385/");
        assert!(art.markdown.contains("书中这句最令我印象深刻"), "got: {}", art.markdown);
        assert!(!art.markdown.contains("电影"), "nav leaked into markdown: {}", art.markdown);
    }
}
