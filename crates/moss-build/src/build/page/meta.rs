//! Semantic markup generation for articles.
//!
//! Generates Open Graph and Schema.org JSON-LD metadata for article pages
//! to help crawlers and social platforms identify content correctly.

use crate::build::served_path::ServedPath;
use crate::build::site_url::SiteUrl;
use crate::i18n::link::TranslationLink;
use std::collections::HashSet;

/// A cover image reference. Two variants because the cover field is
/// uniquely allowed to be an external HTTPS URL (e.g., a stock-photo CDN
/// in frontmatter `cover:`), in addition to the typed local case.
///
/// Every other URL emitted by moss goes through `ServedPath` directly;
/// this enum is the one place where an "either" sum type is justified.
#[derive(Debug, Clone)]
pub enum CoverRef {
    /// A typed served path: the cover lives inside the build output.
    Local(ServedPath),
    /// Already-absolute http(s):// URL: the cover lives on a CDN or
    /// other external host. Pass-through as-is.
    External(String),
}

impl CoverRef {
    /// Resolve to the absolute URL form for emission in og:image,
    /// twitter:image, JSON-LD, RSS, etc.
    pub fn to_absolute_url(&self, site_url: &SiteUrl) -> String {
        match self {
            Self::Local(sp) => sp.to_absolute_url(site_url),
            Self::External(url) => url.clone(),
        }
    }

    /// Resolve to a URL for embedding in long-lived HTML (og:image,
    /// twitter:image, JSON-LD `image`, RSS, sitemap). Deployed sites
    /// (https `SiteUrl`) get absolute URLs — crawlers have no document
    /// base to resolve relative paths against. Preview / unconfigured
    /// builds (http localhost fallback) get the served-path's relative
    /// form so the dev port doesn't bake into static HTML. `External`
    /// covers pass through (already absolute).
    pub fn to_meta_url(&self, site_url: &SiteUrl) -> String {
        match self {
            Self::Local(sp) => {
                if site_url.is_deployed() {
                    sp.to_absolute_url(site_url)
                } else {
                    sp.to_relative_url()
                }
            }
            Self::External(url) => url.clone(),
        }
    }
}

/// Generate Open Graph meta tags for an article.
///
/// # Arguments
/// * `title` - Article title
/// * `description` - Article description (auto-generated if not provided)
/// * `url` - Full canonical URL of the article
/// * `site_name` - Name of the website
/// * `date` - Publication date in ISO 8601 format
/// * `cover` - Optional typed CoverRef (Local served path or External URL)
/// * `cover_dims` - Optional `(width, height)` for the cover image. Should only be
///   `Some` when the cover came from the auto-generated card (where dimensions are
///   known to be 1200x630); `None` for user-supplied covers of unknown size.
/// * `tags` - Article tags
/// * `locale` - BCP 47-style locale, e.g. "en_US". Empty string skips `og:locale` emission.
/// * `site_url` - Base URL used to resolve cover paths via
///   [`CoverRef::to_meta_url`]: absolute on deployed sites (https; per the
///   Open Graph spec crawlers have no document base), root-relative on
///   preview / unconfigured builds (so the dev-server URL doesn't bake
///   into static HTML).
///
/// # Returns
/// HTML string containing Open Graph meta tags
pub fn build_og_tags(
    title: &str,
    description: &str,
    url: &str,
    site_name: &str,
    date: Option<&str>,
    cover: Option<&CoverRef>,
    cover_dims: Option<(u32, u32)>,
    tags: &[String],
    locale: &str,
    site_url: &SiteUrl,
) -> String {
    let cover_url = cover.map(|c| c.to_meta_url(site_url));

    // Skip empty title/description rather than emitting empty `content=""`.
    // Absent is better than empty: an empty meta tells scrapers "we deliberately
    // have no value here" instead of falling through to other sources.
    let mut tags_html = vec![
        r#"<meta property="og:type" content="article">"#.to_string(),
    ];
    if !title.is_empty() {
        tags_html.push(format!(
            r#"<meta property="og:title" content="{}">"#,
            escape_html_attr(title)
        ));
    }
    tags_html.push(format!(
        r#"<meta property="og:url" content="{}">"#,
        escape_html_attr(url)
    ));
    tags_html.push(format!(
        r#"<meta property="og:site_name" content="{}">"#,
        escape_html_attr(site_name)
    ));
    if !description.is_empty() {
        tags_html.push(format!(
            r#"<meta property="og:description" content="{}">"#,
            escape_html_attr(description)
        ));
    }

    if let Some(date) = date {
        tags_html.push(format!(
            r#"<meta property="article:published_time" content="{}">"#,
            escape_html_attr(date)
        ));
    }

    if let Some(image) = cover_url.as_deref() {
        tags_html.push(format!(
            r#"<meta property="og:image" content="{}">"#,
            escape_html_attr(image)
        ));
        tags_html.push(format!(
            r#"<meta property="og:image:alt" content="{}">"#,
            escape_html_attr(title)
        ));
        if let Some((w, h)) = cover_dims {
            tags_html.push(format!(
                r#"<meta property="og:image:width" content="{}">"#,
                w
            ));
            tags_html.push(format!(
                r#"<meta property="og:image:height" content="{}">"#,
                h
            ));
        }
    }

    if !locale.is_empty() {
        tags_html.push(format!(
            r#"<meta property="og:locale" content="{}">"#,
            escape_html_attr(locale)
        ));
    }

    for tag in tags {
        tags_html.push(format!(
            r#"<meta property="article:tag" content="{}">"#,
            escape_html_attr(tag)
        ));
    }

    tags_html.join("\n    ")
}

/// Generate Open Graph meta tags for a website homepage.
///
/// Uses `og:type = "website"` instead of `"article"`, and omits
/// article-specific fields like `published_time` and `article:tag`.
///
/// `locale` empty string skips `og:locale` emission.
/// `site_url` resolves cover paths via [`CoverRef::to_meta_url`]: absolute
/// on deployed sites (https; OG spec requires it for crawlers), relative
/// on preview / unconfigured builds.
pub fn build_og_tags_website(
    title: &str,
    site_name: &str,
    description: &str,
    url: &str,
    cover: Option<&CoverRef>,
    cover_dims: Option<(u32, u32)>,
    locale: &str,
    site_url: &SiteUrl,
) -> String {
    let cover_url = cover.map(|c| c.to_meta_url(site_url));

    // Skip empty fields rather than emitting empty `content=""`. `title` is the
    // page label (== site name on the homepage); `site_name` is the real site
    // name, kept separate so per-page Pages emit og:site_name = site, not label.
    let mut tags_html = vec![
        r#"<meta property="og:type" content="website">"#.to_string(),
    ];
    if !title.is_empty() {
        tags_html.push(format!(
            r#"<meta property="og:title" content="{}">"#,
            escape_html_attr(title)
        ));
    }
    tags_html.push(format!(
        r#"<meta property="og:url" content="{}">"#,
        escape_html_attr(url)
    ));
    if !site_name.is_empty() {
        tags_html.push(format!(
            r#"<meta property="og:site_name" content="{}">"#,
            escape_html_attr(site_name)
        ));
    }
    if !description.is_empty() {
        tags_html.push(format!(
            r#"<meta property="og:description" content="{}">"#,
            escape_html_attr(description)
        ));
    }

    if let Some(image) = cover_url.as_deref() {
        tags_html.push(format!(
            r#"<meta property="og:image" content="{}">"#,
            escape_html_attr(image)
        ));
        tags_html.push(format!(
            r#"<meta property="og:image:alt" content="{}">"#,
            escape_html_attr(title)
        ));
        if let Some((w, h)) = cover_dims {
            tags_html.push(format!(
                r#"<meta property="og:image:width" content="{}">"#,
                w
            ));
            tags_html.push(format!(
                r#"<meta property="og:image:height" content="{}">"#,
                h
            ));
        }
    }

    if !locale.is_empty() {
        tags_html.push(format!(
            r#"<meta property="og:locale" content="{}">"#,
            escape_html_attr(locale)
        ));
    }

    tags_html.join("\n    ")
}

/// Generate Twitter Card meta tags. Uses `summary_large_image` when an image
/// is available, falls back to `summary` otherwise.
///
/// # Arguments
///
/// * `title` - Card title
/// * `description` - Card description
/// * `cover` - Optional typed CoverRef (Local served path or External URL). When None, emits `summary` card type.
/// * `cover_alt` - Optional alt text for the image. Falls back to `title` when None.
/// * `site_url` - Base URL used to resolve cover paths via
///   [`CoverRef::to_meta_url`]: absolute on deployed sites (Twitter Card spec
///   requires it to ingest the image), relative on preview / unconfigured
///   builds (accepts that no card image renders in preview rather than
///   baking the dev-server URL into static HTML).
pub fn build_twitter_tags(
    title: &str,
    description: &str,
    cover: Option<&CoverRef>,
    cover_alt: Option<&str>,
    site_url: &SiteUrl,
) -> String {
    // Resolve the cover URL: absolute when deployed, relative in preview
    // (see CoverRef::to_meta_url). Twitter requires absolute URLs to ingest
    // images for the card; in preview we accept that no card image renders
    // rather than baking the dev-server URL into static HTML.
    let cover_url: Option<String> = cover.map(|c| c.to_meta_url(site_url));

    let card_type = if cover_url.is_some() { "summary_large_image" } else { "summary" };
    // Skip empty title/description rather than emitting empty `content=""`.
    let mut tags_html = vec![
        format!(r#"<meta name="twitter:card" content="{}">"#, card_type),
    ];
    if !title.is_empty() {
        tags_html.push(format!(
            r#"<meta name="twitter:title" content="{}">"#,
            escape_html_attr(title)
        ));
    }
    if !description.is_empty() {
        tags_html.push(format!(
            r#"<meta name="twitter:description" content="{}">"#,
            escape_html_attr(description)
        ));
    }
    if let Some(image) = cover_url.as_deref() {
        tags_html.push(format!(
            r#"<meta name="twitter:image" content="{}">"#,
            escape_html_attr(image)
        ));
        let alt = cover_alt.unwrap_or(title);
        tags_html.push(format!(
            r#"<meta name="twitter:image:alt" content="{}">"#,
            escape_html_attr(alt)
        ));
    }
    tags_html.join("\n    ")
}

/// Generate Schema.org JSON-LD for an article.
///
/// # Arguments
/// * `title` - Article title (headline)
/// * `description` - Article description
/// * `url` - Full canonical URL
/// * `site_name` - Publisher name
/// * `date` - Publication date in ISO 8601 format
/// * `cover` - Optional typed CoverRef (Local served path or External URL)
/// * `tags` - Article keywords
/// * `site_url` - Base URL used to resolve cover paths via
///   [`CoverRef::to_meta_url`]: absolute on deployed sites, relative on
///   preview / unconfigured builds (same deploy-gated policy as `og:image`)
///
/// # Returns
/// HTML script tag containing JSON-LD structured data
pub fn build_schema_json_ld(
    title: &str,
    description: &str,
    url: &str,
    site_name: &str,
    date: Option<&str>,
    cover: Option<&CoverRef>,
    tags: &[String],
    site_url: &SiteUrl,
) -> String {
    let mut json_parts = vec![
        r#"  "@context": "https://schema.org""#.to_string(),
        r#"  "@type": "Article""#.to_string(),
        format!(r#"  "headline": "{}""#, escape_json_string(title)),
        format!(r#"  "url": "{}""#, escape_json_string(url)),
        format!(r#"  "description": "{}""#, escape_json_string(description)),
    ];

    if let Some(date) = date {
        json_parts.push(format!(r#"  "datePublished": "{}""#, escape_json_string(date)));
    }

    if let Some(cover_ref) = cover {
        // Use the deploy-gated form: JSON-LD image is meta data crawlers
        // ingest, same constraints as og:image (see CoverRef::to_meta_url).
        let image = cover_ref.to_meta_url(site_url);
        json_parts.push(format!(r#"  "image": "{}""#, escape_json_string(&image)));
    }

    if !tags.is_empty() {
        let keywords: Vec<String> = tags.iter()
            .map(|t| format!(r#""{}""#, escape_json_string(t)))
            .collect();
        json_parts.push(format!(r#"  "keywords": [{}]"#, keywords.join(", ")));
    }

    // Publisher (always included, required by Google)
    json_parts.push(format!(
        r#"  "publisher": {{
    "@type": "Organization",
    "name": "{}"
  }}"#,
        escape_json_string(site_name)
    ));

    format!(
        r#"<script type="application/ld+json">
{{
{}
}}
</script>"#,
        json_parts.join(",\n")
    )
}

/// Generate Schema.org `WebSite` JSON-LD for the homepage.
///
/// Establishes the site's entity identity for crawlers — the homepage is the
/// one page that represents the site as a whole (the article builder above
/// represents individual posts). Mirrors [`build_schema_json_ld`]'s style: a
/// `Vec<String>` of JSON lines joined with `escape_json_string`, wrapped in the
/// same `<script type="application/ld+json">` tag.
///
/// # Arguments
/// * `site_name` - Site name (also the Organization publisher name)
/// * `description` - Site description; the line is omitted when empty
///   (same empty-field-skip policy as the og builders)
/// * `url` - Homepage URL (absolute when deployed, "/" otherwise — same value
///   passed to [`build_og_tags_website`])
/// * `lang_code` - Site language code (e.g. "en"); the `inLanguage` line is
///   omitted when empty
///
/// # Returns
/// HTML script tag containing JSON-LD structured data
pub fn build_schema_website(
    site_name: &str,
    description: &str,
    url: &str,
    lang_code: &str,
) -> String {
    let mut json_parts = vec![
        r#"  "@context": "https://schema.org""#.to_string(),
        r#"  "@type": "WebSite""#.to_string(),
        format!(r#"  "name": "{}""#, escape_json_string(site_name)),
        format!(r#"  "url": "{}""#, escape_json_string(url)),
    ];

    if !description.is_empty() {
        json_parts.push(format!(r#"  "description": "{}""#, escape_json_string(description)));
    }

    if !lang_code.is_empty() {
        json_parts.push(format!(r#"  "inLanguage": "{}""#, escape_json_string(lang_code)));
    }

    // Publisher (always included — mirrors build_schema_json_ld)
    json_parts.push(format!(
        r#"  "publisher": {{
    "@type": "Organization",
    "name": "{}"
  }}"#,
        escape_json_string(site_name)
    ));

    format!(
        r#"<script type="application/ld+json">
{{
{}
}}
</script>"#,
        json_parts.join(",\n")
    )
}

/// Extract description from article content.
///
/// Takes the opening prose, strips markdown formatting, and truncates at a
/// word boundary with "...". A paragraph break (blank line) is the cut point,
/// except where the first paragraph says nothing on its own and the excerpt
/// reads on into the next — see [`reads_as_fragment`].
///
/// **Deliberately not an `ast::plain_text` lowering.** ADR-036's "parse once, lower to many"
/// unified email HTML/plain-text onto the typed AST, and this function was investigated for the
/// same move — the doc comments below on `first_paragraph_excerpt`/`non_prose_view` are what that
/// investigation left in place: consult the parser (`FootnoteIndex`, `extract_shortcodes`)
/// wherever block context genuinely disambiguates something, but keep the line/shape-based
/// scanning everywhere the goal is excerpt SAFETY rather than render parity — an image's alt text
/// and a paragraph the parser would still render (as literal text, if its markdown is malformed)
/// are deliberately not description material, per `test_extract_description_strips_image_syntax`
/// and `a_broken_image_paragraph_is_deliberately_not_the_description`. See ADR-036's
/// "Investigated and excluded: meta.rs" note for the full reasoning.
///
/// Shortcode handling matches the renderer exactly (the excerpt is computed
/// on the output of moss-core's `extract_shortcodes` — see
/// [`first_paragraph_excerpt`]). A **typed** shortcode's body (grid / buttons
/// cells, `+++` dividers, nested wikilinks) is internal grammar and is
/// excluded in its entirety — never excerpt-safe prose. A **pure-CSS region**
/// (`:::{.class}` — an empty shortcode name, just a styling wrapper) is NOT
/// excluded: its surrounding prose is genuine, user-visible page content, so
/// it stays excerpt-eligible while any nested typed shortcode inside it is
/// still removed. See docs/reference/shortcode-grammar.md.
/// (Shared with [`extract_preview`] via [`first_paragraph_excerpt`].)
/// `math` is the site's `[site].math` setting (ADR-030, default ON) and must
/// be the SAME value the page renders with: `$…$` swallows any footnote
/// marker inside it, so a math-blind parse here reads `$[^a]$` as a marker
/// and deletes characters the published page displays. Same rule, same
/// reason as `newsletter::footnote_numbers`.
pub fn extract_description(content: &str, math: bool) -> String {
    truncate_at_word_boundary(&first_paragraph_excerpt(content, true, math), 160)
}

/// Extract a page's opening prose from markdown content, with inline markdown
/// already stripped — usually the first real paragraph, and where that
/// paragraph is only a fragment, it plus the one after it (see
/// [`reads_as_fragment`]). Shared implementation behind
/// [`extract_description`] and [`extract_preview`] — the only difference
/// between the two public excerpt functions is the truncation length and
/// whether Obsidian callout markers (`[!type]`) get stripped, so the actual
/// line-scanning logic lives here once.
///
/// The raw markdown is first run through moss-core's `extract_shortcodes`
/// (the renderer's own pre-parse pass) so shortcodes are handled with the
/// exact same "what is real visible prose" view the page renders — see the
/// detailed comment inside the function body.
///
/// Then groups lines into paragraphs (separated by blank lines), skipping:
/// - headings (`#`), thematic breaks (`---`)
/// - code fence marker lines (```` ``` ````) — fence *interior* filtering
///   is a pre-existing, documented limitation, not changed here
/// - any residual `:::` marker line (only unclosed openers / code-fence
///   examples survive the `extract_shortcodes` pass as literal `:::` lines)
fn first_paragraph_excerpt(content: &str, strip_callout_marker: bool, math: bool) -> String {
    // Route the raw markdown through the SAME pre-parse pass the renderer
    // uses (moss-core `shortcode_extract::extract_shortcodes`) so the excerpt
    // sees exactly the renderer's "what is real visible prose" view — rather
    // than maintaining a second, subtly-divergent fence scanner here. After
    // this pass:
    //
    // - a typed shortcode (grid / buttons / hero / gallery / recent / …) is
    //   replaced by a one-line `<!--MOSS_SC_…-->` sentinel; its cell content,
    //   `+++` dividers and nested wikilinks are GONE, so none of that internal
    //   grammar can leak into the excerpt;
    // - a **pure-CSS region** (`:::{.class}` / `::::{.class}` — an EMPTY
    //   shortcode name, just a styling wrapper) becomes a plain
    //   `<div class="…">` around its body prose, with any NESTED typed
    //   shortcode inside recursively turned into a sentinel. This mirrors the
    //   real parser's `name.is_empty()` branch, which RECURSES into the region
    //   body: the surrounding prose stays visible on the page (and so stays
    //   excerpt-eligible here) while nested typed cells are still removed;
    // - an unknown `:::name` becomes a `moss-unknown-shortcode` div wrapper,
    //   also preserving its rendered body prose;
    // - an UNCLOSED opener (the author may be mid-keystroke — moss auto-saves
    //   2s after a keystroke, so a build can run before the closer is typed)
    //   is emitted verbatim, and the per-line `:::` filter below drops just
    //   that one marker line — matching the real parser's verbatim-emission
    //   contract for unclosed blocks.
    //
    // The sentinel comment and the `<div …>` / `</div>` wrapper lines are
    // HTML; `strip_markdown_inline` reduces each to an empty string, so they
    // never surface as excerpt text — only the region's genuine prose does.
    //
    // Consolidating onto the real parser (rather than hand-patching a parallel
    // scanner) keeps the excerpt view and the rendered view from drifting:
    // the shortcode grammar has one source of truth.
    let processed =
        moss_core::ast::shortcode_extract::extract_shortcodes(content).markdown_with_placeholders;

    let mut paragraphs: Vec<String> = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    // Indices of paragraphs a heading, rule, fence or shortcode line follows.
    // Only the read-on below consults this: a heading between two paragraphs
    // is strong evidence the second does not continue the first.
    let mut broken_after: HashSet<usize> = HashSet::new();

    // Which labels the page's index owns. A `[^…]` token is rendered as a
    // superscript marker only when the index owns its label — exactly
    // `render_marker`'s `ctx.index.number(label)` gate. `[^0-9]` in prose,
    // `[^a b]`, `[^]` and any undefined label render literally, so the
    // redaction below leaves them alone.
    //
    // Ask the PARSER, do not re-derive it from the line walk below. A line
    // walk cannot see block context, and every place it guessed differently
    // from the renderer was a bug: a `[^x]:` inside a fenced or indented code
    // block counted as a definition though the renderer ignores it (so real
    // prose vanished from the description), while a definition nested in a
    // blockquote, callout or list item did not count though the renderer
    // hoists it (so a raw `[^x]` marker shipped in the meta tag). Continuation
    // lines of a definition had the same split. `FootnoteIndex` is what
    // `render_marker` consults, so gating on it makes the two agree by
    // construction — including for block contexts that do not exist yet.
    //
    // Parsed with the SITE's math setting for the same reason (see
    // `extract_description`): a math-blind parse moves markers.
    let defined_footnotes: HashSet<String> = {
        let config = moss_core::ast::ParseConfig {
            math,
            ..Default::default()
        };
        let doc = moss_core::ast::parse_with_config(&processed, &config);
        moss_core::ast::footnotes::FootnoteIndex::build(&doc.blocks)
            .entries()
            .iter()
            .map(|(_, label)| label.clone())
            .collect()
    };

    // Everything the excerpt must not read as prose — from the parser, for
    // the same reason `defined_footnotes` above comes from the parser: a
    // line scan cannot see block context, and both earlier line-predicate
    // attempts here were wrong in both directions (see `non_prose_view`).
    let NonProseView {
        skip_lines,
        redacted,
    } = non_prose_view(&processed, math, &defined_footnotes);

    for (index, line) in redacted.lines().enumerate() {
        if skip_lines.contains(&index) {
            broken_after.insert(paragraphs.len());
            continue;
        }
        if line.trim().is_empty() {
            if !current_lines.is_empty() {
                paragraphs.push(current_lines.join(" "));
                current_lines.clear();
            }
        } else if line.starts_with('#')
            || line.starts_with("```")
            || line.starts_with("---")
            || line.starts_with(":::")
        {
            // Not prose, and a boundary: `paragraphs.len()` is the index this
            // break falls after, whether or not a paragraph is mid-flush.
            broken_after.insert(paragraphs.len());
        } else {
            // Strip blockquote prefix(es): "> text" -> "text", ">text" ->
            // "text", "  > text" -> "text" (indented — CommonMark allows up
            // to 3 leading spaces per marker), "> > text" -> "text" (nested
            // — each level repeats the marker, and moss-core's own parser
            // folds nesting the same way when it promotes a callout).
            let mut rest = line;
            let mut is_blockquote_line = false;
            loop {
                let indent = rest.chars().take_while(|&c| c == ' ').count().min(3);
                let Some(after_marker) = rest.get(indent..).and_then(|s| s.strip_prefix('>')) else {
                    break;
                };
                is_blockquote_line = true;
                rest = after_marker.trim_start_matches(' ');
            }
            let stripped = rest;
            // Strip Obsidian callout marker: "[!type] body" -> "body",
            // "[!type]-" -> "" (foldable arrow), "[!type] Custom Title" -> "Custom Title".
            // Callouts are typed blockquotes; their marker is metadata that
            // should not leak into <meta name="description"> or OG tags.
            // The AST parser handles this structurally at render time
            // (Block::Callout); this stays plain-text-level for the
            // description extractor's pre-parse line walk. Gated by
            // `strip_callout_marker` — only `extract_description` opted
            // into this historically; `extract_preview` preserves its
            // prior behavior of leaving `[!type]` markers untouched.
            //
            // ALSO gated on `is_blockquote_line`: Obsidian callout syntax is
            // ALWAYS a blockquote (`> [!note]`), never bare `[!type]` text.
            // Without this gate, a paragraph-leading linked image
            // (`[![alt](src)](url)` — a real, CommonMark-legal, non-callout
            // construct, e.g. a README badge) also starts with the two bytes
            // `[!` and was misread as a callout marker: the alt's closing `]`
            // was taken for the marker's close, and the mangled remainder
            // (raw image path + link URL syntax) shipped as the published
            // description instead of falling through to the real next
            // paragraph.
            let stripped = if strip_callout_marker && is_blockquote_line {
                if let Some(rest) = stripped.strip_prefix("[!") {
                    match rest.split_once(']') {
                        Some((_marker, body)) => {
                            body.trim_start_matches(['-', '+']).trim_start()
                        }
                        None => stripped,
                    }
                } else {
                    stripped
                }
            } else {
                stripped
            };
            current_lines.push(stripped);
        }
    }
    if !current_lines.is_empty() {
        paragraphs.push(current_lines.join(" "));
    }

    // Take the first non-empty paragraph after stripping inline markdown. If
    // it is only a fragment (see `reads_as_fragment`), read on into the ONE
    // paragraph after it — but only where that paragraph plausibly continues
    // the same thought: adjacent (no heading, rule or fence between them) and
    // prose rather than a list. A lead-in like `Contents:` introduces its
    // list, it does not run into it, and gluing the bullets on would publish
    // the page's navigation as its description.
    //
    // Footnote markers are already gone: `non_prose_view` redacted their exact
    // parser-reported spans above, so the stripper needs no label set and
    // cannot delete a `[^…]` the page renders as prose.
    let mut excerpt = String::new();
    let mut lead: Option<usize> = None;
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let stripped = strip_markdown_inline(paragraph);
        let stripped = stripped.trim();
        if stripped.is_empty() {
            continue;
        }
        match lead {
            None => {
                excerpt.push_str(stripped);
                if !reads_as_fragment(&excerpt) {
                    break;
                }
                lead = Some(index);
            }
            Some(lead) => {
                if (lead..index).any(|i| broken_after.contains(&i)) || starts_a_list(paragraph) {
                    break;
                }
                excerpt.push(' ');
                excerpt.push_str(stripped);
                break;
            }
        }
    }
    excerpt
}

/// Whether `paragraph` (pre-strip, so its markers survive) is a list rather
/// than prose.
fn starts_a_list(paragraph: &str) -> bool {
    let text = paragraph.trim_start();
    ["- ", "* ", "+ "].iter().any(|m| text.starts_with(m))
        || text
            .split_once(['.', ')'])
            .is_some_and(|(head, rest)| {
                !head.is_empty()
                    && head.chars().all(|c| c.is_ascii_digit())
                    && rest.starts_with(' ')
            })
}

/// Longest an opening that doesn't end like a sentence may be before the
/// excerpt accepts it anyway. Past this there is enough text to summarise
/// with, whatever its punctuation.
const FRAGMENT_LEAD_CHARS: usize = 60;

/// Whether `excerpt` is still only a fragment — an opening that says nothing
/// on its own and should be joined to the paragraph after it.
///
/// A letter opens `Dear Friends & Family,` on its own line, a report opens
/// with a dateline, a recipe opens with its ingredients. Each is a paragraph
/// by the blank-line rule above, so taking "the first paragraph" verbatim
/// published `Dear Friends,` as a page's description and as its card summary
/// in a folder listing — true to the source and useless to a reader.
///
/// A fragment is text carrying no finished sentence *anywhere*, not merely
/// text that fails to end on one: `Welcome to my vault! Check out:` trails a
/// colon but has already said something, and reading on would append the link
/// list it introduces. Testing for the presence of a sentence rather than its
/// position keeps the widening to openings that are genuinely empty of
/// meaning, and treats a closing quote or bracket (`He wrote, "Come home."`)
/// correctly without special-casing it.
///
/// Length bounds the reach independently: at [`FRAGMENT_LEAD_CHARS`] there is
/// enough text to summarise with however it is punctuated, so an opening that
/// runs on without a full stop still stops the walk rather than swallowing the
/// paragraph after it.
///
/// Deliberately *under*-detecting. A full stop inside an abbreviation reads as
/// a finished sentence here, so `Dear Dr. Smith,` and the dateline `Aug. 2,
/// 2026` get no widening at all. That is the old behaviour — a stubby
/// description rather than a wrong one — and buying the rest back costs a
/// list of honorifics that would then need maintaining in every language moss
/// publishes. The caller carries the other half of the guard: presence of a
/// sentence is what stops `Welcome to my vault!`, but a bare `Contents:` is
/// still a fragment, and only the caller's "don't read on into a list" rule
/// keeps its bullets out of the description.
fn reads_as_fragment(excerpt: &str) -> bool {
    if excerpt.chars().count() >= FRAGMENT_LEAD_CHARS {
        return false;
    }
    // ASCII stops, then the CJK full-width ones and the ellipsis.
    !excerpt.contains(['.', '!', '?', '。', '！', '？', '…'])
}

/// The excerpt's parser-derived view of `markdown`: which source lines are
/// not prose at all, and the text with every footnote *marker* removed.
struct NonProseView {
    /// 0-based indices of lines the excerpt must skip entirely: code blocks
    /// and the HOISTED (doc-order-first) definition of each footnote label.
    skip_lines: HashSet<usize>,
    /// `markdown` with the spans the page never renders as prose text blanked
    /// out: every indexed `[^label]` reference (the page renders a
    /// superscript number) and the `[^label]:` marker of a REPEAT definition
    /// (the page renders the repeat's body in place, marker-less). Newlines
    /// are preserved, so line indices agree with `skip_lines`.
    redacted: String,
}

/// Build the excerpt's [`NonProseView`], asking pulldown for byte ranges
/// because no line predicate can decide any of these questions. A footnote
/// definition can be nested in a blockquote, a callout or a list item
/// (`> [^a]: note` — which the renderer hoists out of the body), and a line
/// that LOOKS like one can sit inside a code block (`    [^a]: sample` —
/// which the renderer leaves as code). Two earlier versions of this skip
/// used a line test and were wrong in both directions.
///
/// Code blocks are skipped because the line walk already intends to skip
/// them: it drops the fence lines but used to collect everything between
/// them, so a page opening with a code sample published that sample as its
/// `<meta name="description">`.
///
/// Only the doc-order-FIRST definition of a label is skipped — the one the
/// renderer hoists (`footnotes::is_hoisted` decides by that same
/// first-in-doc-order identity). A REPEAT definition renders its body in
/// place per ADR-035, so its lines stay prose; only its `[^label]:` marker
/// (which the page never shows) is redacted, span-exact from the parser:
/// the marker runs from the definition's start to its first child's start.
///
/// References are redacted here — at their parser-reported spans — rather
/// than string-matched later, so a `[^a]` inside a code span or (on a
/// math-ON site) inside `$…$` survives exactly when the page displays it.
///
/// `indexed` gates reference redaction the way `render_marker` is gated:
/// a token whose label the index does not own renders literally.
fn non_prose_view(markdown: &str, math: bool, indexed: &HashSet<String>) -> NonProseView {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    // Byte offset at which each line starts, so a range maps to line indices.
    let mut line_starts: Vec<usize> = vec![0];
    line_starts.extend(markdown.match_indices('\n').map(|(i, _)| i + 1));
    let line_of = |offset: usize| match line_starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };

    let mut skip_lines = HashSet::new();
    let mut redact_spans: Vec<std::ops::Range<usize>> = Vec::new();
    let mut seen_definitions: HashSet<String> = HashSet::new();

    // The SAME math the page parses with — see `extract_description`.
    let options = moss_core::ast::parser_options(math);
    let mut events = Parser::new_ext(markdown, options)
        .into_offset_iter()
        .peekable();
    while let Some((event, range)) = events.next() {
        // allow:math-events-ignored — this match routes on event KIND for
        // side effects (marking skip lines, recording redaction spans); the
        // excerpt's text comes from the SOURCE string, not from rebuilding
        // the event stream, so an unmatched math event loses nothing: the
        // equation's bytes stay in `redacted` untouched, which is exactly
        // what the description should carry.
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                // `range.end` is exclusive and typically points at the
                // newline that ends the block, so step back one byte
                // before mapping it.
                let last = line_of(range.end.saturating_sub(1).max(range.start));
                for l in line_of(range.start)..=last {
                    skip_lines.insert(l);
                }
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                if seen_definitions.insert(label.to_string()) {
                    // Doc-order-first: hoisted to the endnotes, so none of
                    // its lines are body prose. (A definition nested INSIDE
                    // this range is a first sighting of its own label and
                    // gets marked on its own event; ranges overlapping is
                    // idempotent.)
                    let last = line_of(range.end.saturating_sub(1).max(range.start));
                    for l in line_of(range.start)..=last {
                        skip_lines.insert(l);
                    }
                } else {
                    // Repeat: the body renders in place, the marker does
                    // not. The marker is everything before the first child
                    // event; an empty repeat renders nothing at all.
                    // allow:math-events-ignored — peeks one event for its
                    // OFFSET only; a math first child yields its range like
                    // any other, no payload is consumed.
                    let content_start = match events.peek() {
                        Some((Event::End(TagEnd::FootnoteDefinition), _)) | None => range.end,
                        Some((_, child)) => child.start.clamp(range.start, range.end),
                    };
                    redact_spans.push(range.start..content_start);
                }
            }
            Event::FootnoteReference(label) => {
                if indexed.contains(label.as_ref()) {
                    redact_spans.push(range);
                }
            }
            _ => {}
        }
    }

    // Drop the redacted bytes, keeping every newline so the line indices in
    // `skip_lines` stay true. Span bounds are parser offsets, so they land
    // on char boundaries.
    let mut cut = vec![false; markdown.len()];
    for span in redact_spans {
        for flag in &mut cut[span.start.min(markdown.len())..span.end.min(markdown.len())] {
            *flag = true;
        }
    }
    let mut redacted = String::with_capacity(markdown.len());
    for (i, ch) in markdown.char_indices() {
        if ch == '\n' || !cut[i] {
            redacted.push(ch);
        }
    }

    NonProseView {
        skip_lines,
        redacted,
    }
}

/// Index of the delimiter closing the `open` at `start`, honouring nesting, or
/// `None` when it never closes.
///
/// Depth (rather than the first close) is what keeps both call sites right.
/// Link text that holds brackets survives — `[see [1]](url)` still yields
/// `see [1]` — and on the destination side CommonMark allows a bare (non-`<...>`)
/// URL to contain balanced parentheses, the canonical case being a Wikipedia
/// disambiguation link (`.../Rust_(programming_language)`), so the FIRST `)` is
/// not necessarily the destination's close.
fn matching_delim(text: &str, start: usize, open: char, close: char) -> Option<usize> {
    // Must start ON the opening delimiter, or depth underflows.
    if !text.get(start..)?.starts_with(open) {
        return None;
    }
    let mut depth = 0usize;
    text.get(start..)?.char_indices().find_map(|(i, c)| {
        if c == open {
            depth += 1;
            None
        } else if c == close {
            depth -= 1;
            (depth == 0).then_some(start + i)
        } else {
            None
        }
    })
}

/// If `text[start..]` opens a well-formed HTML tag, return the byte index
/// just past its `>`. Otherwise `None` — the `<` is ordinary prose.
///
/// Strict on purpose: a false positive silently deletes everything up to
/// the next `>` anywhere downstream, and this text becomes the meta /
/// og: / twitter: description, JSON-LD, and embed excerpts. Checking only
/// the byte after `<` is what let `$a <b$ and $c >d$` collapse to `$a d$`.
/// Required shape: `<`, optional `/`, an ASCII-alpha name
/// (`[A-Za-z][A-Za-z0-9-]*`), then a byte that may follow one (whitespace,
/// `/`, `>`), and no second `<` inside. `<!` keeps the comment path. TeX is
/// safe because it writes comparisons unspaced and punctuation cannot
/// follow a tag name — `strip_markdown_inline_residual_ambiguity_is_documented`
/// records the limit.
fn tag_span_end(text: &str, start: usize) -> Option<usize> {
    let rest = text.get(start + 1..)?;

    // `<!-- comment -->` / `<!DOCTYPE …>` have no name to validate.
    if !rest.starts_with('!') {
        let mut chars = rest.strip_prefix('/').unwrap_or(rest).chars();
        if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        // Whatever ends the name must be able to. Anything else (`$`, `,`,
        // `.`) means the `<` was prose — a comparison, most often — and
        // running off the end without one means there is no tag at all.
        match chars.find(|c| !(c.is_ascii_alphanumeric() || *c == '-')) {
            Some(c) if c.is_whitespace() || c == '/' || c == '>' => {}
            _ => return None,
        }
    }

    let (inside, _) = rest.split_once('>')?;
    // A real tag cannot contain another `<`.
    (!inside.contains('<')).then_some(start + inside.len() + 2)
}

/// `text` with the bytes `from..to` replaced by `with`. Every caller's bounds
/// come from matching ASCII markers, so `get` always hits; it is used instead of
/// a slice so that an off-by-one returns `text` rather than aborting the build.
fn splice(text: &str, from: usize, to: usize, with: &str) -> String {
    match (text.get(..from), text.get(to..)) {
        (Some(head), Some(tail)) => format!("{head}{with}{tail}"),
        _ => text.to_string(),
    }
}

/// Strip inline markdown formatting: bold/italic markers, link syntax
/// (keeping link text), image syntax, inline code backticks, and HTML tags.
///
/// Image and link stripping is shape-based, DELIBERATELY looser than the
/// parser: a token the parser rejects as an image (e.g. a footnote marker
/// broke its alt text, so the page shows the literal syntax) is still
/// dropped here. Garbled markdown syntax is not description material even
/// when the page displays it — the excerpt moves on to the next real
/// paragraph, which serves the reader better than quoting the breakage.
/// Pinned by `a_broken_image_paragraph_is_deliberately_not_the_description`.
///
/// Footnote markers are none of this function's business. Body prose has
/// its indexed `[^label]` tokens redacted at their parser-reported spans by
/// [`non_prose_view`] BEFORE it gets here, and every other input (an
/// explicit frontmatter `description:`, a hero overlay, a share-card
/// string) is never rendered with footnotes at all — so any `[^…]` token
/// that reaches this function is prose (`[^0-9]`, a kaomoji, an undefined
/// label) and survives, exactly as the page renders it.
pub fn strip_markdown_inline(text: &str) -> String {
    let mut result = text.to_string();

    // Strip HTML tags: <tag...> -> "". The span must look like a tag END TO
    // END, not merely start like one — see `tag_span_end`. Checking only the
    // byte after `<` is not enough: `<b` in `$a <b$ and $c >d$` passes that
    // test, so the `>` two comparisons later is treated as the tag's close
    // and the prose between them is deleted. Unspaced inequalities against a
    // named variable are the ordinary way TeX is written, so this fires on
    // exactly the math prose the description is most likely to contain.
    let mut search_from = 0;
    while let Some(rel) = result.get(search_from..).and_then(|s| s.find('<')) {
        let start = search_from + rel;
        match tag_span_end(&result, start) {
            Some(end) => result = splice(&result, start, end, ""),
            None => search_from = start + 1, // not a tag — keep it, scan on
        }
    }

    // Phase 3 PR2: strip wikilinks `[[file]]`, `[[file|alias]]`,
    // `![[file]]`, etc. Stage 1's wikilink resolver used to rewrite these
    // into `[text](url)` shape before description extraction; Phase 3
    // pushes wikilink resolution down into pulldown-cmark's event stream,
    // which is invisible to this text-level stripper. The replacement
    // matches what `dispatch_wikilink_embed` produces for the link case:
    // alias text (when present) or the file part. Image embeds (`![[…]]`)
    // drop entirely — they're media references, not text — including the
    // case where pulldown-cmark would emit the file part as inner text.
    while let Some(start) = result.find("[[") {
        // Find matching `]]`.
        let after = start + 2;
        if let Some(end_rel) = result.get(after..).and_then(|s| s.find("]]")) {
            let end = after + end_rel;
            let inner = result.get(after..end).unwrap_or_default();
            let is_image_embed = start > 0 && result.as_bytes()[start - 1] == b'!';
            if is_image_embed {
                // `![[file.ext]]` or `![[file.ext|alias]]` — drop entirely.
                // Even when pulldown-cmark would put alias text in the
                // event body, the description excerpt should treat the
                // embed as a media block (like `![alt](url)` is dropped
                // below), not as caption text.
                result = splice(&result, start - 1, end + 2, "");
                continue;
            }
            // Pothole: text after `|` becomes alias display text.
            let display = match inner.split_once('|') {
                Some((file, alias)) => {
                    if alias.is_empty() {
                        if let Some((f, s)) = file.split_once('#') {
                            format!("{} > {}", f, s)
                        } else {
                            file.to_string()
                        }
                    } else {
                        alias.to_string()
                    }
                }
                None => {
                    if let Some((f, s)) = inner.split_once('#') {
                        format!("{} > {}", f, s)
                    } else {
                        inner.to_string()
                    }
                }
            };
            result = splice(&result, start, end + 2, &display);
            continue;
        }
        break;
    }

    // Strip images: ![alt](url) -> "" and links: [text](url) -> text.
    //
    // A `[` opens a link only when its MATCHING `]` is followed by `(`. Pairing
    // it with the next `](` anywhere downstream deletes the wrong span the
    // moment prose holds a bracket the author meant literally — a regex class,
    // a kaomoji, an undefined footnote marker — and those now reach this loop
    // by design. An unmatched `[` is prose too, and a later `[` may still open
    // a real link, so the scan steps past it instead of giving up.
    let mut cursor = 0;
    while let Some(rel) = result.get(cursor..).and_then(|s| s.find('[')) {
        let start = cursor + rel;
        let close = matching_delim(&result, start, '[', ']').filter(|&close| result.get(close..).is_some_and(|s| s.starts_with("](")));
        let Some(close) = close else {
            cursor = start + 1;
            continue;
        };
        // Check if this is an image: ![alt](url). Byte indexing is boundary-safe: '!' is ASCII.
        let is_image = start > 0 && result.as_bytes()[start - 1] == b'!';
        let strip_start = if is_image { start - 1 } else { start };
        let Some(end) = matching_delim(&result, close + 1, '(', ')') else {
            // The destination never closes: `(` nesting stays unbalanced all
            // the way to the end of this text, so there is no well-formed
            // span to keep past this point. Same contract as the rest of
            // this function — garbled markdown syntax (here, a runaway
            // destination like `[doc](https://x.dev/f(x)`) is not
            // description material even though the page displays it.
            result.truncate(strip_start);
            break;
        };
        if is_image {
            // Strip entire ![alt](url) -> ""
            result = splice(&result, start - 1, end + 1, "");
            cursor = start - 1;
        } else {
            // Strip [text](url) -> text
            let link_text = result.get(start + 1..close).unwrap_or_default().to_string();
            result = splice(&result, start, end + 1, &link_text);
            cursor = start;
        }
    }

    // Strip bold/italic: **text** -> text, *text* -> text
    result = result.replace("**", "");
    result = result.replace("*", "");

    // Strip inline code: `code` -> code
    result = result.replace("`", "");

    // Collapse multiple spaces into one
    while result.contains("  ") {
        result = result.replace("  ", " ");
    }

    result.trim().to_string()
}

/// Extract preview text from article content.
///
/// Same as [`extract_description`] but with a configurable character limit.
/// Used for hover link preview data where a longer excerpt is appropriate.
/// Shares [`first_paragraph_excerpt`] with `extract_description`, so it
/// gets the same shortcode-fence-body exclusion (see that function's docs)
/// — a linked article whose only body content is a `:::grid:::` (etc.)
/// block now yields an empty preview instead of leaking `+++`/`:::` into
/// the `.moss-preview-popup` excerpt.
pub fn extract_preview(content: &str, max_chars: usize, math: bool) -> String {
    truncate_at_word_boundary(&first_paragraph_excerpt(content, false, math), max_chars)
}

/// Truncate text at word boundary (UTF-8 safe)
fn truncate_at_word_boundary(text: &str, max_chars: usize) -> String {
    // Find byte index at max_chars characters (not bytes)
    let truncate_byte_idx = text
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len());

    if truncate_byte_idx >= text.len() {
        return text.to_string();
    }

    // Find last space before truncation point. `truncate_byte_idx` came from
    // `char_indices`, so it is a char boundary; `get` says so to the compiler.
    //
    // Backing up to a space only avoids cutting a word in half — it must not
    // cost most of the excerpt. Chinese and Japanese prose carries no spaces
    // at all, so the nearest space can be the one word of Latin text near the
    // start (or the join between an opening and the paragraph read on into
    // above), and honouring it would throw away everything after it. Below
    // half the budget, take the hard character cut instead: mid-word is a
    // smaller wrong than mid-sentence.
    let truncated = text.get(..truncate_byte_idx).unwrap_or(text);
    match truncated.rsplit_once(' ') {
        Some((head, _)) if head.chars().count() * 2 >= max_chars => format!("{}...", head),
        _ => format!("{}...", truncated),
    }
}

/// Resolve the page description, preferring explicit frontmatter over auto-extraction.
///
/// Strips inline markdown from explicit descriptions and falls back to extracting the
/// first paragraph from content. Returns `None` only when both sources are empty.
///
/// Used by call sites that only need per-page resolution (child lists, grid cards).
/// Meta-tag callers should use [`resolve_page_description_with_fallbacks`] so
/// homepage-hero text and the homepage cascade can fill in share cards.
pub fn resolve_page_description(
    frontmatter_desc: Option<&str>,
    content: &str,
    math: bool,
) -> Option<String> {
    frontmatter_desc
        .map(strip_markdown_inline)
        .or_else(|| Some(extract_description(content, math)))
        .filter(|d| !d.is_empty())
}

/// Inputs to the share-card description fallback chain.
///
/// Six rungs, two tiers. Within each tier the precedence is the same:
/// explicit `description:` > hero overlay text > body first paragraph.
/// Tiers run page-then-homepage so a page's own lede still beats the
/// site-wide tagline — see the asymmetry rationale in
/// docs/archive/2026-05-16-homepage-hero-og-fallback-design.md.
pub struct DescriptionChainInputs<'a> {
    /// Current page's `description:` frontmatter (may carry inline markdown).
    pub page_description: Option<&'a str>,
    /// Current page's hero overlay text — pre-extracted at hoisting from
    /// `HeroShortcode.overlay_markdown` via the same [`extract_description`]
    /// pipeline used for body fallback. `None` when the hero had no
    /// extractable paragraph or the page had no hero.
    pub page_hero_overlay_text: Option<&'a str>,
    /// Current page's raw markdown body (fed to [`extract_description`]).
    pub page_content: &'a str,
    /// Homepage `description:` frontmatter. The caller passes `None` when
    /// the current page IS the homepage so rungs 4-6 are skipped and the
    /// homepage's own rungs 1-3 don't double-walk.
    pub homepage_description: Option<&'a str>,
    /// Homepage hero overlay text. `None` for the homepage itself.
    pub homepage_hero_overlay_text: Option<&'a str>,
    /// Homepage raw markdown body. `None` for the homepage itself.
    pub homepage_content: Option<&'a str>,
    /// The site's `[site].math` setting — the SAME value the pages render
    /// with, so the body-extraction rungs read `$…$` the way the page does.
    pub math: bool,
}

/// Resolve a page's share-card description by walking the six-rung fallback
/// chain in [`DescriptionChainInputs`]. Returns `None` only when every rung
/// is empty.
pub fn resolve_page_description_with_fallbacks(
    inputs: &DescriptionChainInputs,
) -> Option<String> {
    description_from_explicit(inputs.page_description)
        .or_else(|| description_from_extracted(inputs.page_hero_overlay_text))
        .or_else(|| description_from_content(Some(inputs.page_content), inputs.math))
        .or_else(|| description_from_explicit(inputs.homepage_description))
        .or_else(|| description_from_extracted(inputs.homepage_hero_overlay_text))
        .or_else(|| description_from_content(inputs.homepage_content, inputs.math))
}

/// Strip inline markdown from an explicit `description:` field. `None` /
/// empty become `None` so the chain falls through cleanly.
fn description_from_explicit(s: Option<&str>) -> Option<String> {
    let raw = s?.trim();
    if raw.is_empty() {
        return None;
    }
    let stripped = strip_markdown_inline(raw);
    if stripped.trim().is_empty() {
        None
    } else {
        Some(stripped)
    }
}

/// Pass-through for already-extracted text (hero overlay text). The value
/// already went through [`extract_description`] at hoisting time; treat as
/// a finished string. Empty becomes `None`.
fn description_from_extracted(s: Option<&str>) -> Option<String> {
    let raw = s?.trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

/// Run [`extract_description`] on raw markdown content. Empty becomes `None`.
pub(crate) fn description_from_content(content: Option<&str>, math: bool) -> Option<String> {
    let raw = content?;
    let extracted = extract_description(raw, math);
    if extracted.trim().is_empty() {
        None
    } else {
        Some(extracted)
    }
}

/// Escape HTML attribute value
pub fn escape_html_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Build the `<head>` block of `<link rel="alternate" hreflang="...">` tags
/// for a multilingual page.
///
/// Returns `None` when the page has no translations (`translations.is_empty()`)
/// — single-language pages emit no hreflang. Returns `Some(html)` otherwise,
/// containing one `<link>` per language (the current page's own lang plus each
/// alternate) and an `x-default` entry pointing to the site-default-language
/// version when one exists in this translation group.
///
/// `site_host` is a bare host (`"example.com"`) — leading scheme and trailing
/// slash are tolerated and stripped. URL form is `https://{host}/{path}` with
/// any trailing `index.html` removed (matches the canonical_link policy in
/// `render/html.rs`). www-prefix canonicalization is intentionally NOT applied
/// here: hreflang URLs should match the actual served URL of each variant, and
/// the per-variant www decision is already captured in the document's
/// `url_path` if relevant.
///
/// TODO: if `www_canonical` ever gets plumbed into `canonical_url()` (see the
/// comment at the `canonical_link` site in `render/html.rs`), this helper must
/// also accept it and run the same `canonical_url()` transform on each href.
/// Otherwise canonical will emit `https://www.example.com/about/` while
/// hreflang emits `https://example.com/about/` and Google's hreflang validator
/// will flag the canonical/hreflang URL mismatch.
///
/// `current_lang` and `current_url_path` describe the page being rendered;
/// `translations` holds links to OTHER translations (not the current page —
/// see `build_translation_links` line 42). `site_default_lang` controls the
/// `x-default` target: emit `x-default` when the current page IS the site
/// default lang, OR when one of the alternates is. Skip otherwise (no safe
/// fallback target).
///
/// Every value here is a DECLARED BCP-47 tag — the same string the page emits
/// as its own `<html lang>`. Taking them from `Language` instead made a `fr`
/// page advertise `hreflang="en"` against its own `<html lang>: fr`, and
/// collapsed two pages of one unshipped language into a single alternate
/// (moss#1177).
pub fn build_hreflang_link_tags(
    current_lang_tag: &str,
    current_url_path: &str,
    translations: &[TranslationLink],
    site_host: &str,
    site_default_lang_tag: &str,
) -> Option<String> {
    if translations.is_empty() {
        return None;
    }

    let host = site_host
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');

    // Build a properly-escaped href from a relative url_path. Defends against:
    // - leading slash on url_path (would produce https://host//path)
    // - HTML-unsafe characters in the path (matches canonical_link policy)
    // - trailing index.html (matches canonical_link policy in render/html.rs)
    let to_href = |url_path: &str| -> String {
        let trimmed = url_path
            .trim_start_matches('/')
            .trim_end_matches("index.html");
        let raw = format!("https://{}/{}", host, trimmed);
        escape_html_attr(&raw)
    };

    // Dedup by language TAG. `build_translation_links` already dedups by
    // url_path, but two same-lang entries with different url_paths can
    // appear when both stem-grouping and translationKey-grouping produce
    // results for the same lang. Keep the first occurrence; emit each
    // language at most once.
    use std::collections::HashSet;
    let mut seen_langs: HashSet<&str> = HashSet::new();
    let mut links: Vec<String> = Vec::new();

    let alternate = |hreflang: &str, url_path: &str| {
        format!("<link rel=\"alternate\" hreflang=\"{hreflang}\" href=\"{}\">", to_href(url_path))
    };

    // Self entry first — Google docs: each page lists itself + all alternates.
    seen_langs.insert(current_lang_tag);
    links.push(alternate(current_lang_tag, current_url_path));

    // Alternates
    for link in translations {
        if seen_langs.insert(link.lang_tag.as_str()) {
            links.push(alternate(&link.lang_tag, &link.url_path));
        }
    }

    // x-default — points to the site-default-language EDITION of this page,
    // which is not the same question as tag equality: see
    // `i18n::same_language_edition` for why two tests are needed.
    let same_edition = |a: &str| crate::i18n::same_language_edition(a, site_default_lang_tag);
    let x_default = if same_edition(current_lang_tag) {
        Some(current_url_path)
    } else {
        translations.iter().find(|t| same_edition(&t.lang_tag)).map(|t| t.url_path.as_str())
    };
    if let Some(url_path) = x_default {
        links.push(alternate("x-default", url_path));
    }

    Some(links.join("\n    "))
}

/// Escape JSON string value
fn escape_json_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
#[path = "meta_tests.rs"]
mod tests;
