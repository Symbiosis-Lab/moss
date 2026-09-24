//! RSS Feed Generation
//!
//! Generates RSS 2.0 feeds from site articles for subscription.

use crate::build::markdown::AnalyticsConfig;
use moss_core::render::image::{
    synthesize_image_html, ImageContext, ImageRenderOptions,
};
use crate::build::types::ParsedDocument;
use moss_core::asset_snapshot::AssetSnapshot;
use std::collections::HashSet;

/// True when the document has an absolute `external_url:` in its frontmatter —
/// a linkblog page whose canonical home is the outlet, not the local site.
/// Thin wrapper over the shared `scan::page_map::external_url` helper. See moss#679.
fn has_external_url(doc: &ParsedDocument) -> bool {
    crate::build::scan::page_map::external_url(&doc.raw_frontmatter).is_some()
}

/// Percent-encode the path portion of an absolute URL, preserving the scheme
/// and host. Distinct from moss-core's [`percent_encode_url`], which is for
/// relative URLs without a scheme — feeding `https://example.com/...` to
/// that function would encode `https:` (the keep-list excludes `:`). After
/// splitting off scheme+host, the path part is delegated to
/// [`percent_encode_path_segments`] so the byte set matches wikilink-resolved
/// asset paths in HTML output.
///
/// [`percent_encode_url`]: moss_core::resolve::fuzzy_path::percent_encode_url
/// [`percent_encode_path_segments`]: moss_core::resolve::fuzzy_path::percent_encode_path_segments
/// Percent-encode only the PATH of an already-built absolute URL, leaving
/// `scheme://host` verbatim — encoding the host would corrupt it, and feed
/// readers reject a corrupted authority outright. Non-absolute input (no `://`,
/// or no `/` after the host) is returned unchanged rather than guessed at.
fn percent_encode_feed_url(url: &str) -> String {
    // scheme://host passes through verbatim; only the path is encoded.
    let Some((scheme, after_scheme)) = url.split_once("://") else {
        return url.to_string();
    };
    let Some((host, path_rel)) = after_scheme.split_once('/') else {
        return url.to_string();
    };
    let path = format!("/{path_rel}"); // allow:served-path-url-construct (re-assembles an already-built URL for percent-encoding, constructs nothing)
    format!(
        "{scheme}://{host}{}",
        moss_core::resolve::fuzzy_path::percent_encode_path_segments(&path)
    )
}

/// Escape special XML characters in text content.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Convert a date string (YYYY-MM-DD, YYYY-MM, or YYYY) to RFC 822 format for
/// RSS. A missing month or day defaults to 1 — RFC 822 has no partial form,
/// and a feed entry with a coarse timestamp orders better than one with none.
/// Returns None if the date cannot be parsed.
fn date_to_rfc822(date_str: &str) -> Option<String> {
    let pd = crate::build::components::date::parse_partial_date(date_str)?;
    let year = pd.year;
    let month = pd.month.unwrap_or(1);
    let day = pd.day.unwrap_or(1);

    let month_name = match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => return None,
    };

    // RFC 822 format: "DD Mon YYYY 00:00:00 +0000"
    Some(format!("{:02} {} {} 00:00:00 +0000", day, month_name, year))
}

/// Rewrite internal links in HTML content to append UTM parameters.
/// Internal links are relative (starting with `./`, `../`, `/`) or absolute matching `site_url`.
fn add_utm_to_internal_links(html: &str, site_url: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut remaining = html;

    while let Some((before, after_href)) = remaining.split_once("href=\"") {
        // Copy everything up to and including href="
        result.push_str(before);
        result.push_str("href=\"");
        remaining = after_href;

        // Find the closing quote
        if let Some((url, after_quote)) = after_href.split_once('"') {
            let is_internal = url.starts_with("./")
                || url.starts_with("../")
                || (url.starts_with('/') && !url.starts_with("//"))
                || url.starts_with(site_url);

            if is_internal && !url.contains("utm_source=") {
                let separator = if url.contains('?') { "&" } else { "?" };
                result.push_str(&format!(
                    "{}{}utm_source=rss&utm_medium=feed",
                    url, separator
                ));
            } else {
                result.push_str(url);
            }

            result.push('"');
            remaining = after_quote;
        }
    }

    result.push_str(remaining);
    result
}

/// Prefix every footnote anchor in one article's HTML so it stays unique
/// inside the feed document.
///
/// A page's endnote ids (`fn-1`, `fnref-1`) are unique per PAGE. A feed
/// concatenates every article into one XML document, so three footnoted
/// articles would ship three `id="fn-1"`s — and in a reader that renders
/// items into one document, clicking footnote 1 in article 3 jumps to
/// article 1. Scoping by the article's own path makes each set unique and
/// keeps every link resolving within its own item.
///
/// The fragment hrefs are otherwise left alone: `add_utm_to_internal_links`
/// classifies `#…` as external and skips it, so a footnote link is never
/// mangled — it is just ambiguous, which is what this fixes.
///
/// Takes the scope [`footnote_scope`] already made unique for this feed rather
/// than the raw path, so uniqueness is decided in one place.
fn scope_footnote_anchors(html: &str, scope: &str) -> String {
    if !html.contains("moss-footnotes") {
        return html.to_string();
    }
    html.replace("id=\"fn-", &format!("id=\"{scope}-fn-"))
        .replace("id=\"fnref-", &format!("id=\"{scope}-fnref-"))
        .replace("href=\"#fn-", &format!("href=\"#{scope}-fn-"))
        .replace("href=\"#fnref-", &format!("href=\"#{scope}-fnref-"))
}

/// Derive one item's anchor prefix from its URL path, guaranteed distinct from
/// every prefix already handed out for this feed.
///
/// The path is the only per-item name a feed item carries, but two properties
/// of moss URLs make a naive sanitizer useless. moss slugs preserve CJK
/// verbatim ([`moss_core::slug::generate_slug`]), so an ASCII-only character
/// map erases a Chinese site's entire path; and pretty URLs all end in the
/// constant `index.html`, so what survives is the same string for every
/// article — three articles, three identical `fn-1`s, which is the collision
/// this scoping exists to prevent. Reusing the slug SSOT keeps the CJK (HTML5
/// ids accept any character but whitespace, and fragment refs percent-encode
/// cleanly) and dropping the file segment leaves the part that actually
/// differs. Where even that ties — `posts/a-b/` against `posts/a/b/`, or the
/// site root, which slugifies to nothing — position in the feed decides.
fn footnote_scope(url_path: &str, index: usize, used: &mut HashSet<String>) -> String {
    let stem = url_path
        .strip_suffix("index.html")
        .or_else(|| url_path.strip_suffix(".html"))
        .unwrap_or(url_path);
    let mut scope = moss_core::slug::slugify_path_segments(stem).replace('/', "-");
    let mut ordinal = index + 1;
    while scope.is_empty() || used.contains(&scope) {
        scope = format!("item-{ordinal}");
        ordinal += 1;
    }
    used.insert(scope.clone());
    scope
}

/// Generates an RSS 2.0 feed XML string from articles.
///
/// # Arguments
/// * `documents` - All parsed documents from the site
/// * `site_title` - The site's title for the feed
/// * `site_url` - Base URL of the site (e.g., "https://example.com")
/// * `site_description` - Optional description for the feed
///
/// # Returns
/// RSS 2.0 XML string
pub fn generate_rss_feed(
    documents: &[ParsedDocument],
    site_title: &str,
    site_url: &str,
    site_description: Option<&str>,
    analytics: Option<&AnalyticsConfig>,
) -> String {
    // Filter to only documents with dates (articles) and sort by date descending.
    // Skip linkblog pages (`external_url:` in frontmatter) — their canonical
    // home is the outlet, which has its own feed; including them here would
    // duplicate the outlet's content under our domain. See moss#679.
    let mut articles: Vec<&ParsedDocument> = documents
        .iter()
        .filter(|doc| doc.date.is_some())
        .filter(|doc| doc.is_listable())
        .filter(|doc| !has_external_url(doc))
        .collect();

    // Sort by date descending (newest first)
    articles.sort_by(|a, b| {
        let date_a = a.date.as_ref().unwrap();
        let date_b = b.date.as_ref().unwrap();
        date_b.cmp(date_a)
    });

    let description = site_description.unwrap_or(site_title);

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">\n");
    xml.push_str("<channel>\n");

    // Channel metadata
    xml.push_str(&format!("<title>{}</title>\n", escape_xml(site_title)));
    xml.push_str(&format!("<link>{}</link>\n", escape_xml(site_url)));
    xml.push_str(&format!(
        "<description>{}</description>\n",
        escape_xml(description)
    ));

    // Atom self-link for feed discovery
    let feed_rel = crate::build::served_path::ServedPath::for_rss("").unwrap().to_relative_url();
    let feed_url = percent_encode_feed_url(&format!(
        "{}{}",
        site_url.trim_end_matches('/'),
        feed_rel
    ));
    xml.push_str(&format!(
        "<atom:link href=\"{}\" rel=\"self\" type=\"application/rss+xml\"/>\n",
        escape_xml(&feed_url)
    ));

    // Add items for each article
    let mut used_scopes: HashSet<String> = HashSet::new();
    for (index, article) in articles.into_iter().enumerate() {
        let article_url = percent_encode_feed_url(&format!(
            "{}/{}",
            site_url.trim_end_matches('/'),
            article.url_path.trim_start_matches('/')
        ));
        let article_url_with_utm =
            format!("{}?utm_source=rss&utm_medium=feed", article_url);

        xml.push_str("<item>\n");
        // Feed entry title is chrome; use the plain-text label.
        xml.push_str(&format!("<title>{}</title>\n", escape_xml(&article.label)));
        xml.push_str(&format!(
            "<link>{}</link>\n",
            escape_xml(&article_url_with_utm)
        ));
        xml.push_str(&format!(
            "<guid>{}</guid>\n",
            escape_xml(&article_url_with_utm)
        ));

        // Add pubDate in RFC 822 format
        if let Some(ref date) = article.date {
            if let Some(rfc822_date) = date_to_rfc822(date) {
                xml.push_str(&format!("<pubDate>{}</pubDate>\n", rfc822_date));
            }
        }

        // Build description: scope footnote anchors to this item (the feed is
        // one document holding every article), rewrite internal links with
        // UTM, add optional tracking pixel.
        let scope = footnote_scope(&article.url_path, index, &mut used_scopes);
        let scoped_html = scope_footnote_anchors(&article.html_content, &scope);
        let mut description_html = add_utm_to_internal_links(&scoped_html, site_url);

        // ADR-030 §3.5: the website HTML inlines typeset math as <svg>, which
        // feed readers sanitize away — the equation would silently VANISH for
        // every reader subscriber. Swap each math <svg> for the same hosted
        // 2× PNG <img> (absolute URLs) email uses; refused-math <code> nodes
        // pass through untouched.
        description_html = crate::build::emit::math_png::swap_math_svgs_for_email_imgs(
            &description_html,
            site_url,
        );

        if let Some(analytics_config) = analytics {
            // Use pretty path (strip index.html/.html) to match browser page views
            let pretty_path = article
                .url_path
                .trim_end_matches("index.html")
                .trim_end_matches(".html");
            let pretty_path = if pretty_path.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", pretty_path.trim_start_matches('/')) // allow:served-path-url-construct (GoatCounter analytics page path injected into RSS item, not a framework asset URL)
            };
            if let Some(pixel_url) = analytics_config.to_pixel_url(&pretty_path) {
                // Phase 2C of unified-image-emission: route the 1×1 read-tracking
                // pixel through the synthesizer instead of an inline format!.
                // The TrackingPixel variant short-circuits to a bare self-closing
                // <img> with no lazy-loading (must fire on read), no LQIP, no
                // <picture>, empty alt — see image_render.rs.
                description_html.push_str(&synthesize_image_html(
                    &pixel_url,
                    "",
                    &AssetSnapshot::new(),
                    ImageContext::TrackingPixel,
                    &ImageRenderOptions::default(),
                ));
            }
        }

        // Full content in description (CDATA wrapped to preserve HTML)
        xml.push_str(&format!(
            "<description><![CDATA[{}]]></description>\n",
            &description_html
        ));

        xml.push_str("</item>\n");
    }

    xml.push_str("</channel>\n");
    xml.push_str("</rss>");

    xml
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::PageKind;

    fn create_test_document(title: &str, date: Option<&str>, url_path: &str, html_content: &str) -> ParsedDocument {
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            html_content: html_content.to_string(),
            url_path: url_path.to_string(),
            date: date.map(|d| d.to_string()),
            reading_time: 5,
            slug: title.to_lowercase().replace(' ', "-"),
            permalink: format!("/{}", url_path), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    #[test]
    fn test_generates_valid_rss_xml_structure() {
        let docs = vec![
            create_test_document(
                "First Post",
                Some("2025-01-15"),
                "posts/first-post.html",
                "<p>First post content</p>",
            ),
        ];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Should have XML declaration
        assert!(rss.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        // Should have RSS 2.0 root element
        assert!(rss.contains("<rss version=\"2.0\""));
        // Should have channel element
        assert!(rss.contains("<channel>"));
        assert!(rss.contains("</channel>"));
        // Should have closing rss tag
        assert!(rss.contains("</rss>"));
    }

    /// One article's body + endnote section, the shape `footnotes.rs` emits.
    fn footnoted_html(note: &str) -> String {
        format!(
            concat!(
                r##"<p>Body<sup class="moss-footnote-ref" id="fnref-1" tabindex="-1">"##,
                r##"<a href="#fn-1" role="doc-noteref">1</a></sup>.</p>"##,
                r##"<section class="moss-footnotes" role="doc-endnotes"><ol>"##,
                r##"<li id="fn-1" tabindex="-1"><p>{}"##,
                r##" <a class="moss-footnote-backref" href="#fnref-1">&#8617;&#xFE0E;</a></p></li>"##,
                r##"</ol></section>"##
            ),
            note
        )
    }

    /// Collect every `"`-terminated value that follows `prefix`.
    fn attr_values(haystack: &str, prefix: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = haystack;
        while let Some(pos) = rest.find(prefix) {
            rest = &rest[pos + prefix.len()..];
            let Some(end) = rest.find('"') else { break };
            out.push(rest[..end].to_string());
            rest = &rest[end..];
        }
        out
    }

    /// The whole promise of per-item scoping, asserted once: inside a feed
    /// document every `href="#…"` finds its `id` in the SAME item, and no id
    /// is shared between two items. A reader that renders items into one
    /// document therefore cannot send footnote 1 of item 3 to item 1.
    fn assert_anchors_resolve_within_each_item(rss: &str) {
        let items: Vec<&str> = rss
            .split("<item>")
            .skip(1)
            .filter_map(|chunk| chunk.split("</item>").next())
            .collect();
        assert!(items.len() > 1, "need several items to prove scoping: {rss}");

        let mut seen: Vec<(String, usize)> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let ids = attr_values(item, "id=\"");
            assert!(!ids.is_empty(), "item {i} lost its footnote ids: {item}");
            for href in attr_values(item, "href=\"#") {
                assert!(
                    ids.contains(&href),
                    "item {i} links to #{href}, which lives in another item: {rss}"
                );
            }
            for id in ids {
                if let Some((_, other)) = seen.iter().find(|(seen_id, _)| *seen_id == id) {
                    panic!("id=\"{id}\" appears in items {other} and {i}: {rss}");
                }
                seen.push((id, i));
            }
        }
    }

    #[test]
    fn footnote_anchors_are_scoped_per_item_so_ids_never_collide() {
        // Two articles, each with its own footnote 1. Unscoped, the feed
        // would carry two `id="fn-1"` and clicking article two's marker
        // would jump to article one's note.
        let docs = vec![
            create_test_document(
                "One",
                Some("2025-01-15"),
                "posts/one.html",
                &footnoted_html("alpha"),
            ),
            create_test_document(
                "Two",
                Some("2025-01-16"),
                "posts/two.html",
                &footnoted_html("bravo"),
            ),
        ];

        let rss = generate_rss_feed(&docs, "Blog", "https://example.com", None, None);

        assert_eq!(rss.matches(r#"id="fn-1""#).count(), 0, "{rss}");
        assert_eq!(rss.matches(r#"id="posts-one-fn-1""#).count(), 1, "{rss}");
        assert_eq!(rss.matches(r#"id="posts-two-fn-1""#).count(), 1, "{rss}");
        assert_anchors_resolve_within_each_item(&rss);
    }

    #[test]
    fn cjk_paths_are_scoped_as_distinctly_as_ascii_ones() {
        // The shape `compute_url_path` actually emits for a Chinese site:
        // CJK directory, CJK slug, constant `index.html` tail. An ASCII-only
        // sanitizer collapses all of it to the same prefix, which would ship
        // three identical `fn-1`s into one feed document.
        let docs = vec![
            create_test_document(
                "第一篇",
                Some("2025-01-15"),
                "文章/第一篇/index.html",
                &footnoted_html("甲"),
            ),
            create_test_document(
                "第二篇",
                Some("2025-01-16"),
                "文章/第二篇/index.html",
                &footnoted_html("乙"),
            ),
            create_test_document(
                "Third",
                Some("2025-01-17"),
                "posts/third/index.html",
                &footnoted_html("gamma"),
            ),
        ];

        let rss = generate_rss_feed(&docs, "博客", "https://example.com", None, None);

        assert_eq!(rss.matches(r#"id="fn-1""#).count(), 0, "{rss}");
        assert_eq!(rss.matches(r#"id="文章-第一篇-fn-1""#).count(), 1, "{rss}");
        assert_eq!(rss.matches(r#"id="文章-第二篇-fn-1""#).count(), 1, "{rss}");
        assert_eq!(rss.matches(r#"id="posts-third-fn-1""#).count(), 1, "{rss}");
        assert_anchors_resolve_within_each_item(&rss);
    }

    #[test]
    fn paths_that_slugify_alike_still_get_distinct_scopes() {
        // `posts/a-b/` and `posts/a/b/` both flatten to `posts-a-b`, and the
        // site root slugifies to nothing at all. Position in the feed is the
        // tiebreaker no path shape can defeat.
        let docs = vec![
            create_test_document(
                "Root",
                Some("2025-01-17"),
                "index.html",
                &footnoted_html("root"),
            ),
            create_test_document(
                "Flat",
                Some("2025-01-16"),
                "posts/a-b/index.html",
                &footnoted_html("flat"),
            ),
            create_test_document(
                "Nested",
                Some("2025-01-15"),
                "posts/a/b/index.html",
                &footnoted_html("nested"),
            ),
        ];

        let rss = generate_rss_feed(&docs, "Blog", "https://example.com", None, None);

        assert_eq!(rss.matches(r#"id="fn-1""#).count(), 0, "{rss}");
        assert_anchors_resolve_within_each_item(&rss);
    }

    #[test]
    fn scoping_leaves_a_footnote_free_article_byte_identical() {
        let html = r#"<p>Plain <a href="/other/">link</a>.</p>"#;
        assert_eq!(scope_footnote_anchors(html, "posts-x"), html);
    }

    #[test]
    fn test_includes_channel_metadata() {
        let docs = vec![];
        let rss = generate_rss_feed(
            &docs,
            "My Blog",
            "https://example.com",
            Some("A blog about things"),
            None,
        );

        assert!(rss.contains("<title>My Blog</title>"));
        assert!(rss.contains("<link>https://example.com</link>"));
        assert!(rss.contains("<description>A blog about things</description>"));
    }

    #[test]
    fn test_includes_articles_with_dates_as_items() {
        let docs = vec![
            create_test_document(
                "First Post",
                Some("2025-01-15"),
                "posts/first-post.html",
                "<p>First post content</p>",
            ),
            create_test_document(
                "Second Post",
                Some("2025-01-10"),
                "posts/second-post.html",
                "<p>Second post content</p>",
            ),
        ];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Should have items
        assert!(rss.contains("<item>"));
        assert!(rss.contains("<title>First Post</title>"));
        assert!(rss.contains("<title>Second Post</title>"));
        // Should have links with UTM parameters
        assert!(rss.contains("<link>https://example.com/posts/first-post.html?utm_source=rss&amp;utm_medium=feed</link>"));
        assert!(rss.contains("<link>https://example.com/posts/second-post.html?utm_source=rss&amp;utm_medium=feed</link>"));
    }

    #[test]
    fn test_excludes_documents_without_dates() {
        let docs = vec![
            create_test_document(
                "About Page",
                None, // No date - not an article
                "about.html",
                "<p>About me</p>",
            ),
            create_test_document(
                "Blog Post",
                Some("2025-01-15"),
                "posts/blog-post.html",
                "<p>Blog content</p>",
            ),
        ];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Should include the dated article
        assert!(rss.contains("<title>Blog Post</title>"));
        // Should NOT include the undated page
        assert!(!rss.contains("<title>About Page</title>"));
    }

    #[test]
    fn test_includes_full_article_content_in_description() {
        let docs = vec![
            create_test_document(
                "Test Post",
                Some("2025-01-15"),
                "posts/test.html",
                "<p>This is the <strong>full</strong> article content.</p>",
            ),
        ];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Description should contain the HTML content (CDATA wrapped)
        assert!(rss.contains("<description>"));
        assert!(rss.contains("full"));
        assert!(rss.contains("article content"));
    }

    #[test]
    fn test_sorts_articles_by_date_descending() {
        let docs = vec![
            create_test_document("Old Post", Some("2025-01-01"), "old.html", "<p>Old</p>"),
            create_test_document("New Post", Some("2025-01-15"), "new.html", "<p>New</p>"),
            create_test_document("Mid Post", Some("2025-01-10"), "mid.html", "<p>Mid</p>"),
        ];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Find positions of each title in the RSS
        let new_pos = rss.find("<title>New Post</title>").unwrap();
        let mid_pos = rss.find("<title>Mid Post</title>").unwrap();
        let old_pos = rss.find("<title>Old Post</title>").unwrap();

        // Newest should come first
        assert!(new_pos < mid_pos, "New post should come before mid post");
        assert!(mid_pos < old_pos, "Mid post should come before old post");
    }

    #[test]
    fn test_escapes_special_xml_characters_in_title() {
        let docs = vec![
            create_test_document(
                "Post with <special> & \"characters\"",
                Some("2025-01-15"),
                "test.html",
                "<p>Content</p>",
            ),
        ];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Special characters should be escaped
        assert!(rss.contains("&lt;special&gt;"));
        assert!(rss.contains("&amp;"));
    }

    #[test]
    fn test_empty_documents_produces_valid_feed() {
        let docs = vec![];
        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Should still have valid structure
        assert!(rss.contains("<channel>"));
        assert!(rss.contains("<title>My Blog</title>"));
        // Should not have any items
        assert!(!rss.contains("<item>"));
    }

    #[test]
    fn test_includes_pub_date_in_rfc822_format() {
        let docs = vec![
            create_test_document(
                "Test Post",
                Some("2025-01-15"),
                "test.html",
                "<p>Content</p>",
            ),
        ];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Should have pubDate in RFC 822 format
        assert!(rss.contains("<pubDate>"));
        // RFC 822 format includes day name and month name
        assert!(rss.contains("Jan 2025") || rss.contains("15 Jan 2025"));
    }

    #[test]
    fn test_month_precision_date_defaults_day_to_one_in_rfc822() {
        let docs = vec![
            create_test_document(
                "Test Post",
                Some("1797-06"),
                "test.html",
                "<p>Content</p>",
            ),
        ];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        assert!(rss.contains("<pubDate>01 Jun 1797 00:00:00 +0000</pubDate>"));
    }

    #[test]
    fn test_includes_guid_for_each_item() {
        let docs = vec![
            create_test_document(
                "Test Post",
                Some("2025-01-15"),
                "posts/test.html",
                "<p>Content</p>",
            ),
        ];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Should have guid element
        assert!(rss.contains("<guid>"));
        assert!(rss.contains("https://example.com/posts/test.html"));
    }

    #[test]
    fn test_percent_encodes_non_ascii_urls() {
        let docs = vec![create_test_document(
            "中文文章",
            Some("2025-01-15"),
            "文字/测试文章/index.html",
            "<p>内容</p>",
        )];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Chinese path segments should be percent-encoded
        assert!(rss.contains("%E6%96%87%E5%AD%97")); // 文字
        assert!(rss.contains("%E6%B5%8B%E8%AF%95%E6%96%87%E7%AB%A0")); // 测试文章
        // Raw Chinese characters should NOT appear in link/guid elements
        assert!(!rss.contains("<link>https://example.com/文字/"));
        assert!(!rss.contains("<guid>https://example.com/文字/"));
    }

    #[test]
    fn test_percent_encode_feed_url_preserves_ascii_paths() {
        let url = "https://example.com/posts/hello-world/index.html";
        assert_eq!(percent_encode_feed_url(url), url);
    }

    #[test]
    fn test_percent_encode_feed_url_encodes_chinese_path() {
        let url = "https://example.com/文字/测试/index.html";
        let encoded = percent_encode_feed_url(url);
        assert_eq!(
            encoded,
            "https://example.com/%E6%96%87%E5%AD%97/%E6%B5%8B%E8%AF%95/index.html"
        );
    }

    #[test]
    fn test_percent_encode_feed_url_no_path() {
        let url = "https://example.com";
        assert_eq!(percent_encode_feed_url(url), url);
    }

    #[test]
    fn test_utm_parameters_on_article_links() {
        let docs = vec![create_test_document(
            "Test Post",
            Some("2025-01-15"),
            "posts/test-post/index.html",
            "<p>Content</p>",
        )];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Links and guids should have UTM parameters (& escaped as &amp; in XML)
        assert!(
            rss.contains("?utm_source=rss&amp;utm_medium=feed</link>"),
            "Link should contain UTM params. RSS:\n{}",
            rss
        );
        assert!(
            rss.contains("?utm_source=rss&amp;utm_medium=feed</guid>"),
            "GUID should contain UTM params. RSS:\n{}",
            rss
        );
    }

    #[test]
    fn test_goatcounter_tracking_pixel_in_description() {
        let docs = vec![create_test_document(
            "Test Post",
            Some("2025-01-15"),
            "posts/test-post/index.html",
            "<p>Content</p>",
        )];

        let analytics = AnalyticsConfig {
            provider: Some("goatcounter".to_string()),
            url: "https://mysite.goatcounter.com/count".to_string(),
            site_id: None,
        };

        let rss = generate_rss_feed(
            &docs,
            "My Blog",
            "https://example.com",
            None,
            Some(&analytics),
        );

        // Description should contain tracking pixel with article path
        assert!(
            rss.contains(r#"<img src="https://mysite.goatcounter.com/count?p=/posts/test-post/"#),
            "Should contain GoatCounter pixel. RSS:\n{}",
            rss
        );
        assert!(
            rss.contains(r#"width="1" height="1""#),
            "Pixel should be 1x1. RSS:\n{}",
            rss
        );
    }

    #[test]
    fn test_no_tracking_pixel_without_analytics() {
        let docs = vec![create_test_document(
            "Test Post",
            Some("2025-01-15"),
            "posts/test.html",
            "<p>Content</p>",
        )];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // No tracking pixel when analytics is None
        assert!(
            !rss.contains("goatcounter.com/count"),
            "Should not contain pixel without analytics"
        );
    }

    /// ADR-030 §3.5: item content built from the web HTML would carry inline
    /// math `<svg>`, and feed readers sanitize `<svg>` away — the equation
    /// vanishes for every reader subscriber. RSS must ship the hosted-PNG
    /// `<img>` form (absolute, content-addressed URLs) instead.
    #[test]
    fn rss_swaps_math_svg_for_hosted_png_img() {
        use crate::build::emit::math_png;
        use crate::build::markdown::math::render_math;

        let inline = render_math("a_i", false).unwrap();
        let display = render_math(r"\sum_{i=1}^n x_i", true).unwrap();
        // A refused equation ships as the P1 <code> node on the web.
        let refused = r#"<code class="moss-math">$\text{线性}$</code>"#;
        let body = format!("<p>inline {inline} and</p>{display}<p>{refused}</p>");

        let docs = vec![create_test_document(
            "Math Post",
            Some("2026-07-21"),
            "posts/math.html",
            &body,
        )];
        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        assert!(
            !rss.contains("<svg"),
            "no <svg> may reach feed readers (they sanitize it away): {rss}"
        );
        assert!(
            rss.contains(&format!(
                "https://example.com/_moss/math/{}.png",
                math_png::content_hash("a_i", false)
            )),
            "inline math must become the absolute content-addressed PNG: {rss}"
        );
        assert!(
            rss.contains(&format!(
                "https://example.com/_moss/math/{}.png",
                math_png::content_hash(r"\sum_{i=1}^n x_i", true)
            )),
            "display math must become the absolute content-addressed PNG: {rss}"
        );
        assert!(
            rss.contains(refused),
            "refused-math <code> nodes must pass through untouched: {rss}"
        );
    }

    #[test]
    fn rss_excludes_draft_pages() {
        let mut draft = create_test_document("Draft Post", Some("2026-01-01"), "draft/index.html", "<p>x</p>");
        draft.draft = Some(true);
        let live = create_test_document("Live Post", Some("2026-01-02"), "live/index.html", "<p>y</p>");
        let docs = vec![live, draft];
        let xml = generate_rss_feed(&docs, "Test Site", "https://example.com", None, None);
        assert!(xml.contains("Live Post"));
        assert!(!xml.contains("Draft Post"), "draft must not appear in RSS");
    }

    #[test]
    fn test_no_tracking_pixel_for_umami() {
        let docs = vec![create_test_document(
            "Test Post",
            Some("2025-01-15"),
            "posts/test.html",
            "<p>Content</p>",
        )];

        let analytics = AnalyticsConfig {
            provider: None,
            url: "https://analytics.example.com/script.js".to_string(),
            site_id: Some("abc123".to_string()),
        };

        let rss = generate_rss_feed(
            &docs,
            "My Blog",
            "https://example.com",
            None,
            Some(&analytics),
        );

        // Umami doesn't support pixel tracking
        assert!(
            !rss.contains(r#"width="1""#),
            "Should not contain pixel for Umami"
        );
    }

    #[test]
    fn test_utm_on_internal_links_in_description() {
        let docs = vec![create_test_document(
            "Test Post",
            Some("2025-01-15"),
            "posts/test.html",
            r#"<p>Read <a href="../other-post/">more here</a> and <a href="https://external.com">external</a></p>"#,
        )];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Internal relative link should get UTM params
        assert!(
            rss.contains("../other-post/?utm_source=rss&utm_medium=feed"),
            "Internal link should have UTM. RSS:\n{}",
            rss
        );
        // External link should NOT get UTM params
        assert!(
            rss.contains(r#"href="https://external.com""#),
            "External link should be unchanged. RSS:\n{}",
            rss
        );
    }

    #[test]
    fn test_utm_on_absolute_internal_links_in_description() {
        let docs = vec![create_test_document(
            "Test Post",
            Some("2025-01-15"),
            "posts/test.html",
            r#"<p><a href="https://example.com/about/">About</a> and <a href="/contact/">Contact</a></p>"#,
        )];

        let rss = generate_rss_feed(&docs, "My Blog", "https://example.com", None, None);

        // Absolute internal link matching site_url should get UTM
        assert!(
            rss.contains("https://example.com/about/?utm_source=rss&utm_medium=feed"),
            "Absolute internal link should have UTM. RSS:\n{}",
            rss
        );
        // Root-relative link should get UTM
        assert!(
            rss.contains("/contact/?utm_source=rss&utm_medium=feed"),
            "Root-relative link should have UTM. RSS:\n{}",
            rss
        );
    }
}
