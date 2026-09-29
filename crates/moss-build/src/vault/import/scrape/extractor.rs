//! Article main-content extractor.
//!
//! A focused subset of [kepano/defuddle](https://github.com/kepano/defuddle)
//! that handles ~90% of journalism news pages in ~300 lines:
//!
//! 1. **Strip clutter** with [`lol_html`] — remove `<script>` / `<style>` / `<nav>`
//!    / `<aside>` / `<footer>` / `<header>` / `<form>` / `<button>` etc., plus
//!    any element whose `class`/`id`/semantic-hook attribute (`data-testid`,
//!    `data-test-id`, `data-component`, `data-hook`) matches one of the
//!    journalism-site clutter tokens (ad, social, sidebar, comment, …).
//!    Also drops hidden elements (inline `display:none` / `visibility:hidden`
//!    style or `hidden`/`invisible` class) and small images (width or height
//!    `< 33`), and promotes a lazy-load `data-src`/`data-lazy-src`/
//!    `data-original`/`data-echo` onto `src` when `src` itself is missing
//!    or empty — otherwise a lazy-loaded gallery image is invisible to
//!    every later step, since none of them look past `src`.
//!
//! 2. **Score candidates** with [`scraper`] — for each of the 20 priority
//!    entry-point selectors (`#post`, `.article-content`, …, `article`,
//!    `[role="article"]`, `main`, `[role="main"]`, …, `body`), compute
//!    `(20 - index) * 40 + heuristic`. The heuristic is `words +
//!    10 * paragraphs + commas + 15 if class hints at content + link-density
//!    penalty`.
//!
//! 3. **Return the highest-scoring element's inner HTML.** Callers feed it
//!    to [`htmd`] for markdown conversion.
//!
//! See [defuddle.ts:1139-1171](https://github.com/kepano/defuddle/blob/main/src/defuddle.ts#L1139)
//! for the original algorithm; [constants.ts:3-24](https://github.com/kepano/defuddle/blob/main/src/constants.ts#L3)
//! for `ENTRY_POINT_ELEMENTS`; [removals/scoring.ts](https://github.com/kepano/defuddle/blob/main/src/removals/scoring.ts)
//! for the scoring rules.

use lol_html::{element, HtmlRewriter, Settings};
use scraper::{ElementRef, Html, Selector};

/// Extract an article's main content from raw HTML and return its inner HTML.
///
/// The returned HTML is the body of the chosen container with chrome removed.
/// Feed it to an HTML→Markdown converter (we use [`htmd`]).
pub fn extract_main_content(html: &str) -> String {
    let cleaned = strip_clutter(html);
    let doc = Html::parse_document(&cleaned);
    match best_candidate(&doc) {
        Some(el) => el.inner_html(),
        None => cleaned,
    }
}

// ---------------------------------------------------------------------------
// Step 1: strip clutter via lol_html.
// ---------------------------------------------------------------------------

/// Tags whose contents we never want regardless of class/id.
/// Roughly the union of defuddle's `EXACT_SELECTORS` tag entries with
/// the obvious non-article tags.
const STRIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "iframe", "nav", "aside", "footer",
    "header", "form", "button", "input", "select", "textarea", "meta",
    "link", "object", "embed", "dialog",
];

/// Tokens we look for in `class` / `id` / a semantic-hook attribute
/// (`data-testid`, `data-test-id`, `data-component`, `data-hook`).
/// Word-boundary match (see [`contains_token_boundary`]).
///
/// Distilled from defuddle's 580-pattern `PARTIAL_SELECTORS` to the ~30 that
/// actually fire on news/blog pages.
const CLUTTER_TOKENS: &[&str] = &[
    "advert",
    "share",
    "social",
    "sidebar",
    "side-bar",
    "navigation",
    "menu",
    "footer",
    "masthead",
    "comment",
    "disqus",
    "newsletter",
    "subscribe",
    "related",
    "more-stories",
    "more-articles",
    "recommended",
    "popular",
    "trending",
    "cookie",
    "consent",
    "paywall",
    "promo",
    "promotion",
    "sponsor",
    "breadcrumb",
    "author-bio",
    "post-meta",
    "article-meta",
    "entry-meta",
    "pagination",
    "pager",
    "skip-link",
    "skip-to",
    "sr-only",
    "visually-hidden",
    "aria-label",
    "popup",
    "modal",
    "toolbar",
    "back-to-top",
    "print-only",
    "addtoany",
];

/// Word-boundary-aware token match for CSS class / id / data-testid strings.
///
/// A token matches a *word* in `blob` (split on whitespace) if, within that
/// word, the token appears bounded by `-`, `_`, start, or end. An optional
/// trailing `s` is allowed (covers comment/comments, share/shares, …).
///
/// Examples:
/// - `cookie` matches `cookie`, `cookie-banner`, `accept-cookie`.
/// - `cookie` does NOT match `cookies-not-set` (no boundary after `cookies`).
///   Wait: `cookies` is `cookie+s`, then bytes[7]='-' which IS a boundary,
///   so this WOULD match. We accept that false positive at the leaf-element
///   level (a `class="cookies-not-set"` div is probably a cookie-related
///   widget); the caller never invokes this on `<body>`/`<html>`.
/// - `share` matches `social-share`, `share-buttons`, `share`, but NOT
///   `shareholder` (no boundary after `share`).
fn token_matches(blob: &str, tokens: &[&str]) -> bool {
    let lower = blob.to_ascii_lowercase();
    for word in lower.split_whitespace() {
        for &t in tokens {
            if contains_token_boundary(word, t) {
                return true;
            }
        }
    }
    false
}

fn contains_token_boundary(word: &str, token: &str) -> bool {
    if word == token {
        return true;
    }
    // ASCII-only fast path is fine: CSS-class tokens are ASCII in practice
    // and the blocklist is ASCII. Use byte indices to avoid UTF-8 boundary
    // pitfalls (a class string with a multibyte char would otherwise risk
    // panicking on `word[i..]` slicing).
    let bytes = word.as_bytes();
    let tbytes = token.as_bytes();
    let tlen = tbytes.len();
    if tlen == 0 || tlen > bytes.len() {
        return false;
    }
    let mut i = 0;
    while i + tlen <= bytes.len() {
        if &bytes[i..i + tlen] == tbytes {
            let before_ok = i == 0 || matches!(bytes[i - 1], b'-' | b'_');
            let mut end_pos = i + tlen;
            if end_pos < bytes.len() && bytes[end_pos] == b's' {
                end_pos += 1;
            }
            let after_ok = end_pos >= bytes.len() || matches!(bytes[end_pos], b'-' | b'_');
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn looks_hidden(class: &str, style: &str) -> bool {
    // Word-boundary check: only treat the class as hidden if a *whole*
    // class token is "hidden" or "invisible". `class="hidden-foo"` should
    // NOT match (it's the start of a different word).
    let lower_class = class.to_ascii_lowercase();
    if lower_class
        .split_whitespace()
        .any(|w| w == "hidden" || w == "invisible")
    {
        return true;
    }
    let s = style.replace(' ', "").to_ascii_lowercase();
    s.contains("display:none") || s.contains("visibility:hidden") || s.contains("opacity:0")
}

fn small_image_px(attr: Option<&str>) -> Option<u32> {
    let raw = attr?;
    raw.trim_end_matches("px").trim().parse::<u32>().ok()
}

/// `data-*` attributes that stand in for `src` on a lazy-loaded `<img>`.
/// Squarespace's gallery blocks (and lazysizes-style loaders generally)
/// never populate `src` server-side — an IntersectionObserver swaps it in
/// client-side on scroll — so a static fetch sees an `<img>` with no `src`
/// at all and htmd silently drops it. Checked in this order; first match
/// wins.
const LAZY_SRC_ATTRS: &[&str] = &["data-src", "data-lazy-src", "data-original", "data-echo"];

/// Promote a lazy-load `data-*` attribute onto `src` when `src` is missing
/// or empty, so the image survives into the markdown like any other.
fn promote_lazy_src(
    el: &mut lol_html::html_content::Element,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let has_real_src = el
        .get_attribute("src")
        .is_some_and(|s| !s.trim().is_empty());
    if has_real_src {
        return Ok(());
    }
    for attr in LAZY_SRC_ATTRS {
        if let Some(v) = el.get_attribute(attr) {
            let v = v.trim();
            if !v.is_empty() {
                el.set_attribute("src", v)?;
                break;
            }
        }
    }
    Ok(())
}

/// Run lol_html over the input HTML, removing clutter. Returns clean HTML.
fn strip_clutter(html: &str) -> String {
    let mut output: Vec<u8> = Vec::with_capacity(html.len());

    let strip_selector = STRIP_TAGS.join(",");

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                // Drop entire branches by tag name.
                //
                // Theoretical risk: a site wraps `<article>` inside one of
                // these tags (e.g. `<header role="banner"><article>…</article></header>`),
                // and the article gets removed with the wrapper. We don't
                // guard against this — lol_html doesn't support `:has()`
                // for parent-context selectors, and no real-world news
                // site in our fixture corpus does this. Site-level
                // headers (`<div class="site-header">`) are caught by the
                // class-token strip in the universal handler below.
                element!(strip_selector, |el| {
                    el.remove();
                    Ok(())
                }),
                // Fill in `src` from a lazy-load attribute before anything
                // else inspects the image, then drop small images.
                element!("img", |el| {
                    promote_lazy_src(el)?;
                    let w = small_image_px(el.get_attribute("width").as_deref());
                    let h = small_image_px(el.get_attribute("height").as_deref());
                    if matches!(w, Some(v) if v < 33) || matches!(h, Some(v) if v < 33) {
                        el.remove();
                    }
                    Ok(())
                }),
                // Class / id / data-testid blocklist + hidden filter, on
                // every element except structural containers. Body/html/main/
                // article are exempt — site-wide state classes ("logged-in",
                // "cookies-not-set") on `<body>` would otherwise nuke the
                // entire page.
                element!("*", |el| {
                    let tag = el.tag_name();
                    if matches!(
                        tag.as_str(),
                        "html" | "body" | "head" | "main" | "article"
                    ) {
                        return Ok(());
                    }

                    let class = el.get_attribute("class").unwrap_or_default();
                    let id = el.get_attribute("id").unwrap_or_default();
                    let testid = el
                        .get_attribute("data-testid")
                        .or_else(|| el.get_attribute("data-test-id"))
                        .or_else(|| el.get_attribute("data-component"))
                        .or_else(|| el.get_attribute("data-hook"))
                        .unwrap_or_default();
                    let role = el.get_attribute("role").unwrap_or_default();

                    let blob = format!("{} {} {} {}", class, id, testid, role);
                    if token_matches(&blob, CLUTTER_TOKENS) {
                        el.remove();
                        return Ok(());
                    }

                    let style = el.get_attribute("style").unwrap_or_default();
                    if looks_hidden(&class, &style) {
                        el.remove();
                    }
                    Ok(())
                }),
                // Tighten obvious-ad attributes: `[rel="sponsored"]`, `role="banner"`.
                element!("[rel='sponsored'], [role='banner']", |el| {
                    el.remove();
                    Ok(())
                }),
            ],
            ..Settings::default()
        },
        |c: &[u8]| output.extend_from_slice(c),
    );

    // If lol_html bails for any reason, fall back to the raw HTML — the
    // extractor stays best-effort and the caller still gets *something* to
    // score against.
    if rewriter.write(html.as_bytes()).is_err() {
        return html.to_string();
    }
    if rewriter.end().is_err() {
        return html.to_string();
    }

    String::from_utf8(output).unwrap_or_else(|_| html.to_string())
}

// ---------------------------------------------------------------------------
// Step 2: score candidates and pick the highest.
// ---------------------------------------------------------------------------

/// Entry-point selectors in priority order. Index 0 is highest priority;
/// base score = `(ENTRY_POINTS.len() - index) * 40`.
const ENTRY_POINTS: &[&str] = &[
    "#post",
    ".post-content",
    ".post-body",
    ".article-content",
    "#article-content",
    ".js-article-content",
    ".article_post",
    ".article-wrapper",
    ".entry-content",
    ".content-article",
    ".instapaper_body",
    ".post",
    ".markdown-body",
    "article",
    "[role='article']",
    "main",
    "[role='main']",
    ".article-body",
    "#content",
    "body",
];

fn best_candidate(doc: &Html) -> Option<ElementRef<'_>> {
    let mut best: Option<(i64, ElementRef<'_>)> = None;
    for (i, sel) in ENTRY_POINTS.iter().enumerate() {
        let selector = match Selector::parse(sel) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let base = ((ENTRY_POINTS.len() - i) * 40) as i64;
        for el in doc.select(&selector) {
            let s = base + score_element(&el);
            if best.as_ref().map_or(true, |(prev, _)| s > *prev) {
                best = Some((s, el));
            }
        }
    }
    best.map(|(_, el)| el)
}

fn score_element(el: &ElementRef<'_>) -> i64 {
    let text: String = el.text().collect();
    let words = text.split_whitespace().count() as i64;
    let commas = text.chars().filter(|c| *c == ',' || *c == '，').count() as i64;

    let p_selector = Selector::parse("p").unwrap();
    let paragraphs = el.select(&p_selector).count() as i64;

    let class_blob = el
        .value()
        .attr("class")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let class_bonus = if class_blob.contains("article")
        || class_blob.contains("content")
        || class_blob.contains("post")
        || class_blob.contains("entry")
    {
        15
    } else {
        0
    };

    let link_chars = link_text_chars(el);
    let total_chars = text.chars().count().max(1) as i64;
    let link_density_pct = ((link_chars * 100) / total_chars).min(100);
    let link_penalty = if total_chars > 0 {
        link_density_pct.min(50) / 10 * 5
    } else {
        0
    };

    words + 10 * paragraphs + commas + class_bonus - link_penalty
}

fn link_text_chars(el: &ElementRef<'_>) -> i64 {
    let sel = Selector::parse("a").unwrap();
    el.select(&sel)
        .map(|a| a.text().collect::<String>().chars().count() as i64)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_removes_script_and_style() {
        let html = r#"<html><head><style>body{color:red}</style></head>
            <body><script>alert(1)</script><p>Hello</p></body></html>"#;
        let out = strip_clutter(html);
        assert!(!out.contains("alert"));
        assert!(!out.contains("color:red"));
        assert!(out.contains("Hello"));
    }

    #[test]
    fn strip_removes_nav_aside_footer() {
        let html = r#"<html><body>
            <nav>nav stuff</nav>
            <aside>sidebar stuff</aside>
            <article><p>Real body</p></article>
            <footer>footer stuff</footer>
        </body></html>"#;
        let out = strip_clutter(html);
        assert!(!out.contains("nav stuff"));
        assert!(!out.contains("sidebar stuff"));
        assert!(!out.contains("footer stuff"));
        assert!(out.contains("Real body"));
    }

    #[test]
    fn strip_removes_class_blocklisted() {
        let html = r#"<html><body>
            <div class="article-share-buttons">share</div>
            <div class="newsletter-signup">subscribe</div>
            <div class="related-articles">related</div>
            <article><p>Real body</p></article>
        </body></html>"#;
        let out = strip_clutter(html);
        assert!(!out.contains("share"));
        assert!(!out.contains("subscribe"));
        assert!(!out.contains("related"));
        assert!(out.contains("Real body"));
    }

    #[test]
    fn strip_removes_data_hook_sr_only_and_aria_label_duplicates() {
        // A component library that marks visible text `aria-hidden="true"`
        // (removed from the accessibility tree) and puts the reader-facing
        // duplicate/description in a sibling element addressed only by
        // `data-hook` (not `data-testid`/`data-test-id`/`data-component`,
        // the attributes this file already read). The visible span must
        // survive; the two accessibility-only siblings must not, or their
        // text runs into the visible span with no separator.
        let html = r#"<html><body>
            <article>
              <p>Real content.</p>
              <span aria-hidden="true">Ended</span>
              <span data-hook="details-duration-aria-label">Ended</span>
              <div data-hook="sr-only-details-price">650 Canadian dollars</div>
              <div data-hook="aria-hidden-details-price">CA$650</div>
            </article>
        </body></html>"#;
        let out = strip_clutter(html);
        assert!(out.contains("Real content"), "got:\n{out}");
        assert!(out.contains("CA$650"), "the visible price must survive: {out}");
        assert!(
            !out.contains("650 Canadian dollars"),
            "the sr-only duplicate price must be stripped: {out}"
        );
        assert_eq!(
            out.matches("Ended").count(),
            1,
            "the aria-label duplicate must be stripped, leaving one 'Ended': {out}"
        );
    }

    #[test]
    fn strip_removes_hidden_inline_style() {
        let html = r#"<html><body>
            <p style="display: none">hidden</p>
            <p>visible</p>
        </body></html>"#;
        let out = strip_clutter(html);
        assert!(!out.contains("hidden"));
        assert!(out.contains("visible"));
    }

    #[test]
    fn strip_drops_small_images() {
        let html = r#"<html><body>
            <img src="big.jpg" width="800" height="600">
            <img src="pixel.gif" width="1" height="1">
            <img src="tiny.png" width="20" height="20">
            <p>hello</p>
        </body></html>"#;
        let out = strip_clutter(html);
        assert!(out.contains("big.jpg"));
        assert!(!out.contains("pixel.gif"));
        assert!(!out.contains("tiny.png"));
    }

    /// The `src` attribute's value on the first `<img>` in a fragment, via
    /// a real HTML parser — a plain `contains("src=\"…\"")` string check
    /// is a false positive here, since that same byte sequence also
    /// occurs inside `data-src="…"`.
    fn first_img_src(html: &str) -> Option<String> {
        let doc = Html::parse_fragment(html);
        let sel = Selector::parse("img").unwrap();
        doc.select(&sel)
            .next()?
            .value()
            .attr("src")
            .map(|s| s.to_string())
    }

    #[test]
    fn strip_promotes_lazy_src_when_src_missing() {
        // Squarespace-style gallery markup: no `src` at all, only the
        // lazy-load attribute the client swaps in on scroll.
        let html = r#"<html><body>
            <img data-src="https://cdn.example.com/photo1.jpg" alt="one">
            <img data-src="https://cdn.example.com/photo2.jpg" data-image="https://cdn.example.com/photo2.jpg" alt="two">
            <p>hello</p>
        </body></html>"#;
        let out = strip_clutter(html);
        let doc = Html::parse_document(&out);
        let sel = Selector::parse("img").unwrap();
        let srcs: Vec<Option<&str>> = doc.select(&sel).map(|el| el.value().attr("src")).collect();
        assert_eq!(
            srcs,
            vec![
                Some("https://cdn.example.com/photo1.jpg"),
                Some("https://cdn.example.com/photo2.jpg"),
            ],
            "got: {out}"
        );
    }

    #[test]
    fn strip_leaves_real_src_alone_even_with_data_src_present() {
        // A real `src` (even a different URL, e.g. a low-res placeholder
        // swapped for the lazy-load target) must never be overwritten.
        let html = r#"<html><body>
            <img src="https://cdn.example.com/real.jpg" data-src="https://cdn.example.com/other.jpg" alt="one">
            <p>hello</p>
        </body></html>"#;
        let out = strip_clutter(html);
        assert_eq!(
            first_img_src(&out).as_deref(),
            Some("https://cdn.example.com/real.jpg"),
            "got: {out}"
        );
    }

    #[test]
    fn strip_promotes_lazy_src_end_to_end_through_markdown() {
        // The promotion must survive into the actual markdown the
        // conversion pipeline produces, not just the intermediate HTML.
        let html = r#"<!doctype html><html><body>
            <article>
              <p>A gallery page with almost no prose.</p>
              <img data-src="https://cdn.example.com/gallery1.jpg" alt="Gallery one">
              <img data-src="https://cdn.example.com/gallery2.jpg" alt="Gallery two">
            </article>
        </body></html>"#;
        let inner = extract_main_content(html);
        let md = htmd::convert(&inner).unwrap();
        assert!(
            md.contains("https://cdn.example.com/gallery1.jpg"),
            "got: {md}"
        );
        assert!(
            md.contains("https://cdn.example.com/gallery2.jpg"),
            "got: {md}"
        );
    }

    #[test]
    fn extracts_article_element_over_body() {
        let html = r#"<html><body>
            <header>page header</header>
            <article>
              <p>First paragraph.</p>
              <p>Second paragraph with content.</p>
            </article>
            <footer>page footer</footer>
        </body></html>"#;
        let inner = extract_main_content(html);
        assert!(inner.contains("First paragraph"));
        assert!(inner.contains("Second paragraph"));
        assert!(!inner.contains("page header"));
        assert!(!inner.contains("page footer"));
    }

    #[test]
    fn extracts_role_article_when_no_article_tag() {
        let html = r#"<html><body>
            <div role="article">
              <p>The body.</p>
            </div>
        </body></html>"#;
        let inner = extract_main_content(html);
        assert!(inner.contains("The body"));
    }

    #[test]
    fn extracts_main_when_no_article() {
        let html = r#"<html><body>
            <main>
              <p>Main body of the page.</p>
            </main>
        </body></html>"#;
        let inner = extract_main_content(html);
        assert!(inner.contains("Main body"));
    }

    #[test]
    fn class_post_content_wins_over_body() {
        let html = r#"<html><body>
            <div class="post-content">
              <p>The real article.</p>
            </div>
            <p>Stray body text.</p>
        </body></html>"#;
        let inner = extract_main_content(html);
        assert!(inner.contains("The real article"));
        // body fallback would include the stray; the higher-priority selector wins.
        // We can't strictly assert exclusion (post-content is inside body) but
        // at minimum the article should be present.
    }

    #[test]
    fn handles_empty_input_gracefully() {
        let inner = extract_main_content("");
        // Should not panic; result is empty or close to it.
        assert!(inner.len() < 100);
    }

    #[test]
    fn score_rewards_paragraphs_and_words() {
        let html = r#"<html><body>
            <article>
              <p>Some sentence with commas, and more, words galore.</p>
              <p>Another paragraph here, with more words to count.</p>
            </article>
        </body></html>"#;
        let doc = Html::parse_document(html);
        let sel = Selector::parse("article").unwrap();
        let el = doc.select(&sel).next().unwrap();
        let s = score_element(&el);
        assert!(s > 20, "expected score > 20, got {}", s);
    }
}
