//! HTML post-processing helpers.
//!
//! Covers: CriticMarkup accept mode (5 `LazyLock` regex statics +
//! `accept_criticmarkup`), `%%` comment stripping (`strip_percent_comments`),
//! self-closing-tag fixup, relative-path adjustment for pretty URLs, and
//! `strip_html_tags`.

// ── CriticMarkup accept mode ──────────────────────────────────────────
//
// moss-releases uses CriticMarkup (https://fletcher.github.io/MultiMarkdown-6/syntax/critic.html)
// as a transient authoring layer. Any markup reaching the builder should be
// accepted (edits applied, markup stripped) so stray annotations never leak
// to the published site.

static CM_SUB_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)\{~~(.*?)~>(.*?)~~\}").unwrap()
});
static CM_ADD_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)\{\+\+(.*?)\+\+\}").unwrap()
});
static CM_DEL_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)\{--(.*?)--\}").unwrap()
});
static CM_HL_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)\{==(.*?)==\}").unwrap()
});
static CM_COM_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)\{>>(.*?)<<\}").unwrap()
});

/// The pre-parse source-hygiene pass: everything moss does to a page's
/// markdown *before* it becomes an AST.
///
/// In order: accept CriticMarkup edits, strip `%%…%%` Obsidian comments, then
/// warn if the body ends inside an unterminated `<!--`. One entry point rather
/// than three call-site-sequenced passes, because the order is load-bearing
/// (CriticMarkup first — a `{--…--}` deletion can contain a `%%` delimiter)
/// and a caller that gets it wrong has no way to notice.
///
/// `file_path` and `frontmatter_line_count` are for the warning only: the line
/// number it prints must name a line in the author's file, and this function
/// receives the frontmatter-stripped body.
pub fn prepare_markdown_source(
    body: &str,
    file_path: &str,
    frontmatter_line_count: usize,
) -> String {
    let accepted = accept_criticmarkup(body);
    let stripped = strip_percent_comments(&accepted).into_owned();
    if let Some(warning) = unterminated_comment_warning(&stripped, frontmatter_line_count) {
        crate::build::cli_output::cli_warn!("[{}] {}", file_path, warning);
    }
    stripped
}

/// Message for a body that ends inside an unterminated `<!--`, or `None` when
/// every comment is closed.
///
/// An unterminated comment swallows the rest of the page: CommonMark runs an
/// unclosed HTML comment to end-of-input, and moss's inert-regions scanner
/// agrees, so a mistyped `-->` silently deletes everything after it. The
/// pre-parse scanners are deliberately quiet about author syntax; this is the
/// one shape where quiet means "half your page is gone".
///
/// `frontmatter_line_count` is added so the line number names a line in the
/// author's file — this function receives the frontmatter-stripped body. Pure
/// so the message and the arithmetic are testable; the caller logs.
fn unterminated_comment_warning(body: &str, frontmatter_line_count: usize) -> Option<String> {
    let at = moss_core::inert_regions::InertRegions::scan(body).unterminated_comment()?;
    let line = frontmatter_line_count
        + body
            .get(..at)
            .map_or(1, |head| head.matches('\n').count() + 1);
    Some(format!(
        "unterminated HTML comment: the `<!--` on line {line} is never closed by a \
         `-->`, so everything after it on this page is treated as comment text \
         and will not render."
    ))
}

/// Accept every CriticMarkup edit in the input and return the clean markdown.
/// Highlights and insertions are kept, deletions and comments are removed,
/// substitutions resolve to the "new" side. Inert content — fenced and
/// indented code, inline code spans, HTML comments — is left untouched, so
/// documentation examples survive and an annotation parked inside a comment
/// stays parked (see [`mask_code_regions`]).
pub fn accept_criticmarkup(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('{') {
        return std::borrow::Cow::Borrowed(text);
    }

    let masked = mask_code_regions(text);

    // Collect (start, end, replacement) from all five patterns.
    let mut reps: Vec<(usize, usize, String)> = Vec::new();
    // The mask is byte-length-preserving, so a match range in it indexes
    // `text` — and every match starts at an ASCII `{`, which a masked byte
    // can never be. `get` keeps a future mask change from aborting a build.
    for cap in CM_SUB_RE.captures_iter(&masked) {
        let m = cap.get(0).unwrap();
        let Some(new_side) = cap.get(2).and_then(|g| text.get(g.range())) else {
            continue;
        };
        reps.push((m.start(), m.end(), new_side.to_string()));
    }
    for cap in CM_ADD_RE.captures_iter(&masked) {
        let m = cap.get(0).unwrap();
        let Some(content) = cap.get(1).and_then(|g| text.get(g.range())) else {
            continue;
        };
        reps.push((m.start(), m.end(), content.to_string()));
    }
    for cap in CM_DEL_RE.captures_iter(&masked) {
        let m = cap.get(0).unwrap();
        reps.push((m.start(), m.end(), String::new()));
    }
    for cap in CM_HL_RE.captures_iter(&masked) {
        let m = cap.get(0).unwrap();
        let Some(content) = cap.get(1).and_then(|g| text.get(g.range())) else {
            continue;
        };
        reps.push((m.start(), m.end(), content.to_string()));
    }
    for cap in CM_COM_RE.captures_iter(&masked) {
        let m = cap.get(0).unwrap();
        reps.push((m.start(), m.end(), String::new()));
    }

    if reps.is_empty() {
        return std::borrow::Cow::Borrowed(text);
    }

    reps.sort_by_key(|r| r.0);

    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    for (start, end, repl) in reps {
        if start < pos {
            continue; // overlap guard — patterns shouldn't overlap in well-formed input
        }
        out.push_str(text.get(pos..start).unwrap_or_default());
        out.push_str(&repl);
        pos = end;
    }
    out.push_str(text.get(pos..).unwrap_or_default());
    std::borrow::Cow::Owned(out)
}

/// Strip `%%...%%` Obsidian-style comments from markdown before rendering.
///
/// Two forms are supported:
///
/// **Block form** — the opening `%%` and closing `%%` each appear alone on
/// their own line (optional surrounding whitespace). The entire span from the
/// opening line through the closing line (inclusive) is removed. The block
/// form may span multiple lines.
///
/// **Inline form** — a balanced `%%...%%` pair on a *single* line with no
/// intervening `%%` marker. The entire `%%content%%` is removed from the
/// line. A lone `%%` without a partner on the same line is left untouched.
///
/// Inert content is never stripped — [`mask_code_regions`] guards fenced and
/// indented code, inline code spans and HTML comments, so examples like
/// `` `%%code%%` `` survive. This guard also prevents accidentally stripping
/// `50%% off` style prose that happens to contain a double-`%` inside a code
/// literal.
///
/// The prose guard: a lone `%%` in the middle of normal prose (e.g.
/// `50% off ... 80% done`) contains no second `%%` on the same line and so
/// is never stripped. Only a *balanced* pair on one line (inline) or a pair
/// where each delimiter is alone on its line (block) triggers removal.
pub fn strip_percent_comments(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("%%") {
        return std::borrow::Cow::Borrowed(text);
    }

    let masked = mask_code_regions(text);

    // Collect (byte_start, byte_end) ranges in the ORIGINAL text to remove.
    // Ranges must be non-overlapping and will be applied in order.
    let mut removals: Vec<(usize, usize)> = Vec::new();

    // --- Pass 1: block form ---
    // A line is a "block delimiter" when, after stripping leading/trailing
    // whitespace, it equals exactly `%%`. We find the first such line, then
    // the next such line, and treat everything from the start of the first
    // delimiter line to the end of the second (including its newline) as a
    // removal range.
    {
        let masked_lines: Vec<&str> = masked.split('\n').collect();
        let orig_lines: Vec<&str> = text.split('\n').collect();
        // Build byte offsets for each line start in the original text.
        let mut line_starts: Vec<usize> = Vec::with_capacity(orig_lines.len());
        let mut offset = 0usize;
        for line in &orig_lines {
            line_starts.push(offset);
            offset += line.len() + 1; // +1 for '\n'
        }

        let mut i = 0usize;
        while i < masked_lines.len() {
            if masked_lines[i].trim() == "%%" {
                // Found opening delimiter. Search for the next delimiter.
                let open_line = i;
                let mut j = i + 1;
                while j < masked_lines.len() {
                    if masked_lines[j].trim() == "%%" {
                        // Found closing delimiter at line j.
                        let start = line_starts[open_line];
                        // End = start of next line after closing delimiter, or
                        // the total text length if this is the last line.
                        let end = if j + 1 < line_starts.len() {
                            line_starts[j + 1]
                        } else {
                            text.len()
                        };
                        removals.push((start, end));
                        i = j + 1; // resume after the closing delimiter
                        break;
                    }
                    j += 1;
                }
                if j >= masked_lines.len() {
                    // No closing delimiter found — leave the opening `%%` alone.
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }

    // --- Pass 2: inline form ---
    // Scan each line of the masked text. On lines that are NOT already fully
    // covered by a block removal, find balanced `%%...%%` pairs and remove them.
    // "Balanced" = two `%%` tokens on the same line with no lone `%%` between.
    {
        let masked_lines: Vec<&str> = masked.split('\n').collect();
        let orig_lines: Vec<&str> = text.split('\n').collect();
        let mut line_byte_start = 0usize;
        for (li, mline) in masked_lines.iter().enumerate() {
            let line_byte_end = line_byte_start + orig_lines[li].len();
            // Skip lines already fully consumed by a block removal.
            let already_removed = removals.iter().any(|&(rs, re)| {
                rs <= line_byte_start && re >= line_byte_end
            });
            if !already_removed {
                // Find `%%...%%` pairs on this line. A pair is two consecutive
                // `%%` occurrences on the same masked line.
                let mut search_from = 0usize;
                loop {
                    let Some(first) = mline.get(search_from..).and_then(|s| s.find("%%")) else {
                        break;
                    };
                    let abs_first = search_from + first;
                    let after_first = abs_first + 2;
                    let Some(second) = mline.get(after_first..).and_then(|s| s.find("%%")) else {
                        break;
                    };
                    let abs_second = after_first + second;
                    let abs_second_end = abs_second + 2;
                    // Map back to byte offsets in the original text.
                    let rm_start = line_byte_start + abs_first;
                    let rm_end = line_byte_start + abs_second_end;
                    removals.push((rm_start, rm_end));
                    search_from = abs_second_end;
                }
            }
            line_byte_start += orig_lines[li].len() + 1; // +1 for '\n'
        }
    }

    if removals.is_empty() {
        return std::borrow::Cow::Borrowed(text);
    }

    // Sort and apply removals. Overlapping ranges are merged as we go.
    removals.sort_by_key(|r| r.0);

    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    for (start, end) in removals {
        if start < pos {
            // Partial or full overlap with a previous range — advance end only.
            if end > pos {
                pos = end;
            }
            continue;
        }
        out.push_str(text.get(pos..start).unwrap_or_default());
        pos = end;
    }
    out.push_str(text.get(pos..).unwrap_or_default());
    std::borrow::Cow::Owned(out)
}

/// Return a byte-identical copy of `text` with every inert byte — fenced
/// code, indented code, inline code spans, HTML comments — replaced by an
/// ASCII space. Newlines are kept, so byte offsets and line positions in the
/// mask index the original text.
///
/// This is [`moss_core::inert_regions`], the one shared scanner (moss#903
/// bug 2). It used to be a private fence-and-inline-code copy here, which is
/// why CriticMarkup and `%%` stripping still fired inside authored HTML
/// comments and indented code blocks — a `{++draft++}` parked in a comment
/// was accepted and unwrapped, and an indented documentation example showing
/// `%%comment%%` syntax was stripped out of the docs page showing it.
fn mask_code_regions(text: &str) -> String {
    moss_core::inert_regions::mask_inert(text)
}

/// HTML5 void elements — these are the ONLY tags that can self-close.
/// Everything else (iframe, video, script, textarea, div, span, etc.)
/// requires an explicit closing tag.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input",
    "link", "meta", "param", "source", "track", "wbr",
];

/// Fix self-closing non-void HTML tags.
///
/// Converts `<tag .../>` to `<tag ...></tag>` for any tag NOT in the
/// HTML5 void elements list. Browsers treat self-closing non-void tags
/// as unclosed, swallowing subsequent content into the element.
pub fn fix_self_closing_non_void_tags(html: &str) -> String {
    use regex::Regex;
    let re = Regex::new(r"(?i)<([a-zA-Z][a-zA-Z0-9]*)\b([^>]*?)\s*/\s*>").unwrap();
    re.replace_all(html, |caps: &regex::Captures| {
        let tag = caps[1].to_lowercase();
        if VOID_ELEMENTS.contains(&tag.as_str()) {
            caps[0].to_string()
        } else {
            format!("<{}{}></{}>", &caps[1], &caps[2], &caps[1])
        }
    })
    .to_string()
}

/// Strips HTML tags from a string, keeping only the text content.
/// Handles nested tags and decodes common HTML entities (&amp;, &lt;, &gt;, &quot;, &#39;, &nbsp;).
/// Trims leading/trailing whitespace.
///
/// Used to derive plain-text labels from H1 content and from other rich-text
/// surfaces that need a plain form for nav, breadcrumb, meta tags, and RSS.
pub fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;

    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }

    // Decode common HTML entities
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}


/// Prepend a moss-injected article title `<h1>` to a rendered body.
///
/// Used by the article-page branch of the markdown pipeline when the body has
/// no `# H1` of its own. The injected heading carries the stable class
/// `moss-article-title` so themes can target it (e.g. to `display: none` on a
/// specific page that uses a hero image as its visual title instead).
///
/// The title text is HTML-escaped — moss receives it as plain text from the
/// frontmatter cascade or the title-cased filename, so we treat it as
/// untrusted relative to the HTML output context.
///
/// See `docs/archive/2026-04-28-auto-h1-injection-design.md` for why injection
/// is article-only and why the `moss-article-title` class is stable contract.
pub(crate) fn inject_article_title_h1(html_body: &str, title: &str, emit_source_lines: bool) -> String {
    let escaped = html_escape(title);
    let fm_attr = if emit_source_lines { r#" data-source-fm="title""# } else { "" };
    if html_body.is_empty() {
        format!("<h1 class=\"moss-article-title\"{fm_attr}>{escaped}</h1>\n")
    } else {
        format!("<h1 class=\"moss-article-title\"{fm_attr}>{escaped}</h1>\n{html_body}")
    }
}

/// Splice an HTML fragment (e.g. the date-line + reading-prefs control)
/// into `html_body` at the position immediately after the article's title
/// block. The title block is the first top-level `<h1>...</h1>` plus an
/// optional directly-following `<blockquote>...</blockquote>` "deck"
/// (the H1 + blockquote pattern moss styles as a subtitle/lede).
///
/// Layout intent: the date row and reading-preferences control go
/// immediately after the resolved-title `<h1>`. No walking past blockquotes
/// or other "title block" tail elements. The strict-contract title pipeline
/// (see `docs/reference/title-rendering.md`) injects a single `<h1>` at
/// the top of the body; the date row goes right under it. Predictable
/// position over inference.
///
/// **The title block is the h1 the body STARTS with, not the first h1 it
/// contains.** A page can legitimately have no title heading — a body that
/// opens with `:::hero` suppresses it, because the hero is the visual title —
/// and such a body may still contain an authored `# Heading` further down,
/// which renders as an `<h1>` like any other. Matching the first `</h1>`
/// anywhere would then bury the date row and byline in the middle of the
/// prose, under a heading they have nothing to do with. So the match is
/// anchored: no leading `<h1`, no title block.
///
/// A claimed leaf's own cover (`folder_cover::render`) wraps that same
/// leading `<h1>` behind an EMPTY, CSS-collapsed `<h1 class="moss-folder-title">`
/// inside `.moss-collection-cover-row` > `-cover-body` — so the body no
/// longer literally starts with the real title. [`split_empty_folder_title_prefix`]
/// steps past exactly that placeholder first, so the anchored test still
/// finds the real title instead of falling through to prepend (which would
/// land the date row and byline above the cover image).
///
/// With no title block the fragment is prepended, which puts it at the top of
/// `<article>` — under a hero, since the hero is hoisted out of the body and
/// rendered above `<main>`. That is where a byline belongs on a page whose
/// title is a photograph. This mirrors the same anchored test in
/// `credits::splice_byline_at_page_head`, so moss has one rule for it.
pub(crate) fn splice_after_title_block(html_body: &str, fragment: &str) -> String {
    if fragment.is_empty() {
        return html_body.to_string();
    }
    let (prefix, body) = split_empty_folder_title_prefix(html_body);
    if !body.trim_start().starts_with("<h1") {
        return format!("{}{}{}", prefix, fragment, body);
    }
    let Some((before, after)) = body.split_once("</h1>") else {
        return format!("{}{}{}", prefix, fragment, body);
    };

    let mut result = String::with_capacity(html_body.len() + fragment.len() + 1);
    result.push_str(prefix);
    result.push_str(before);
    result.push_str("</h1>");
    result.push('\n');
    result.push_str(fragment);
    result.push_str(after);
    result
}

/// Splits off a leading `.moss-collection-cover-row` > `-cover-body` wrapper
/// up through its collapsed, empty `<h1 class="moss-folder-title">` — the
/// exact shape `folder_cover::render` emits for a claimed leaf's own cover.
/// Returns `("", html_body)` unchanged for every other caller: a folder
/// index's own non-empty title never matches this, nor does an ordinary
/// article's `<h1>` with no cover wrapper.
fn split_empty_folder_title_prefix(html_body: &str) -> (&str, &str) {
    let leading_ws = html_body.len() - html_body.trim_start().len();
    let rest = &html_body[leading_ws..];
    const ROW_OPEN: &str = "<div class=\"moss-collection-cover-row\">";
    const BODY_OPEN: &str = "<div class=\"moss-collection-cover-body\">";
    const TITLE_OPEN: &str = "<h1 class=\"moss-folder-title\"";
    if !rest.starts_with(ROW_OPEN) {
        return ("", html_body);
    }
    // Cover media (image/video/iframe) between the two opening tags is
    // arbitrary — search for the body column's marker rather than assume one.
    let Some(body_rel) = rest.find(BODY_OPEN) else {
        return ("", html_body);
    };
    let after_body_open = body_rel + BODY_OPEN.len();
    let tail = &rest[after_body_open..];
    if !tail.starts_with(TITLE_OPEN) {
        return ("", html_body);
    }
    let Some(gt) = tail[TITLE_OPEN.len()..].find('>') else {
        return ("", html_body);
    };
    let tag_end = TITLE_OPEN.len() + gt + 1; // just past the opening tag's '>'
    if tail[tag_end..].starts_with("</h1>") {
        let split_at = leading_ws + after_body_open + tag_end + "</h1>".len();
        (&html_body[..split_at], &html_body[split_at..])
    } else {
        ("", html_body)
    }
}

/// Minimal HTML text-content escaper for the five characters that change
/// document structure or trigger entity parsing.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}


/// Resolve a relative path (e.g., "../../assets/img.jpg") to a root-relative path
/// by resolving it against the document's directory derived from its url_path.
///
/// This ensures cover images stored relative to their collection directory
/// are resolved to root-relative paths that work on any page (e.g., homepage).
///
/// # Examples
/// - cover "../../assets/img.jpg" from doc "articles/travel/index.html" → "assets/img.jpg"
/// - cover "assets/img.jpg" from doc "index.html" → "assets/img.jpg"
/// - cover "https://example.com/img.jpg" from any doc → "https://example.com/img.jpg" (unchanged)
pub(crate) fn resolve_to_root_relative(path: &str, doc_url_path: &str) -> String {
    // Skip absolute URLs and already root-relative paths
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    // Leading `/` means site root — strip it to get a root-relative path
    if let Some(stripped) = path.strip_prefix('/') {
        return stripped.to_string();
    }

    // Get document directory parts from url_path
    let doc_dir: Vec<&str> = doc_url_path
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .unwrap_or("")
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    // Resolve the relative path against the document directory
    let mut parts = doc_dir.clone();
    for segment in path.split('/') {
        match segment {
            ".." => { parts.pop(); }
            "." | "" => {}
            other => { parts.push(other); }
        }
    }

    parts.join("/")
}

/// True if `token` is a valid srcset width (`400w`) or density (`2x`) or
/// height (`800h`) descriptor.
fn is_descriptor(token: &str) -> bool {
    let Some(num) = token
        .strip_suffix('w')
        .or_else(|| token.strip_suffix('x'))
        .or_else(|| token.strip_suffix('h'))
    else {
        return false;
    };
    !num.is_empty() && num.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Split one trimmed srcset candidate into `(url, descriptor)`.
fn split_candidate(candidate: &str) -> (&str, Option<&str>) {
    match candidate.rsplit_once(char::is_whitespace) {
        Some((url, descriptor)) if is_descriptor(descriptor) => (url.trim_end(), Some(descriptor)),
        _ => (candidate, None),
    }
}

/// Filters a comma-separated `srcset` value down to the candidates whose
/// resolved URL is NOT in `failed`. Returns `None` when nothing was dropped
/// (every candidate survived) — the caller's fast-path signal. An empty
/// `Some("")` means every candidate was dropped.
///
/// Shared by both [`degrade_failed_variants`] element handlers (the
/// `<picture><source>` ladder AND the bare `<img srcset>` ladder — see that
/// function's doc comment) so the candidate-splitting/URL-resolution logic
/// — already shared with [`rewrite_srcset_candidates`] via
/// [`split_candidate`]/[`is_descriptor`] — has exactly one implementation
/// for this pass too.
fn filter_srcset_candidates(
    srcset: &str,
    doc_url_path: &str,
    failed: &std::collections::HashSet<String>,
) -> Option<String> {
    let candidates: Vec<&str> = srcset.split(',').map(str::trim).filter(|c| !c.is_empty()).collect();
    let surviving: Vec<&str> = candidates
        .iter()
        .copied()
        .filter(|candidate| {
            let (url, _descriptor) = split_candidate(candidate);
            // Attribute text arrives HTML-escaped and entity-intact, so only
            // the shared normalizer yields the registry key.
            let decoded = decode_html_reference_path(url);
            let key = resolve_to_root_relative(&decoded, doc_url_path);
            !failed.contains(&key)
        })
        .collect();

    if surviving.len() == candidates.len() {
        return None;
    }
    Some(surviving.join(", "))
}

/// Post-seal honest-degradation pass (moss#867): drop webp `srcset`
/// candidates that reference a terminally-failed image variant, in BOTH
/// shapes moss-core's `synthesize_inner` (zero I/O) can emit them:
///
/// - `<picture><source type="image/webp" srcset=X.webp></picture>` —
///   raster originals (png/jpg/jpeg).
/// - bare `<img srcset=X.webp>` (no `<picture>` wrapper) — webp-source
///   images, whose responsive ladder rungs are re-encodes of the source
///   itself (Phase B/Task 12: a `<picture><source>` would be byte-identical
///   to the inner `<img>`, so the ladder goes directly on it instead).
///
/// Neither emission site has visibility into whether the background
/// encoder actually produced each rung (that runs later, async, tracked by
/// `AssetRegistry`). A *chosen* srcset candidate that 404s is unrecoverable
/// in both shapes: per the HTML spec's "update-the-source-set" /
/// "update-the-image-data" algorithms, the browser commits to the picked
/// candidate and does NOT retry a different one, and does NOT fall back to
/// a `<picture>`'s inner `<img>` or an `<img>`'s bare `src`
/// (`docs/decisions/ADR-013-asset-publish-invariant.md`).
///
/// Run this once, post-seal — after the background encoder drain, so every
/// `AssetState` has settled to `Ready` or `Failed`, none still `Pending` —
/// against the same annotated staging bytes that were hashed into the
/// manifest at render time. `failed` is the set of root-relative registry
/// keys (`AssetRegistry::failed_keys`) that ended in `AssetState::Failed`:
///
/// - Every srcset candidate whose URL resolves (via
///   [`resolve_to_root_relative`]) to a failed key is dropped.
/// - `<picture><source>`: if EVERY candidate was dropped, the `<source>`
///   element itself is removed — the browser falls through to the inner
///   `<img>` (the original, unconverted file; never registry-tracked,
///   always materialized by `copy_deferred_assets` as a plain file copy,
///   so it isn't subject to the encode-failure mode this pass exists for).
/// - `<img srcset>`: if EVERY candidate was dropped, the `srcset`/`sizes`
///   attributes are removed instead (there is no element to fall through
///   to) — the browser is left with plain `<img src>`. `src` here is the
///   webp source file itself (`build_srcset`'s `base_url == src`), which is
///   never registry-tracked (same non-goal as the `<picture>` case above),
///   so this is always safe.
/// - An element with no failed candidates is left byte-identical.
///
/// Returns `Cow::Borrowed` when nothing changed, so the caller
/// (`build::degrade`) can skip the disk write + re-hash for a page with no
/// failed images on it — the overwhelmingly common case.
pub(crate) fn degrade_failed_variants<'a>(
    html: &'a str,
    doc_url_path: &str,
    failed: &std::collections::HashSet<String>,
) -> Result<std::borrow::Cow<'a, str>, String> {
    if failed.is_empty() {
        return Ok(std::borrow::Cow::Borrowed(html));
    }

    let changed = std::cell::Cell::new(false);
    let mut output = Vec::new();
    {
        let mut rewriter = lol_html::HtmlRewriter::new(
            lol_html::Settings {
                element_content_handlers: vec![
                    lol_html::element!("picture source[type=\"image/webp\"]", |el| {
                        let Some(srcset) = el.get_attribute("srcset") else {
                            return Ok(());
                        };
                        let Some(surviving) =
                            filter_srcset_candidates(&srcset, doc_url_path, failed)
                        else {
                            return Ok(());
                        };
                        changed.set(true);
                        if surviving.is_empty() {
                            el.remove();
                        } else {
                            el.set_attribute("srcset", &surviving)?;
                        }
                        Ok(())
                    }),
                    lol_html::element!("img[srcset]", |el| {
                        let Some(srcset) = el.get_attribute("srcset") else {
                            return Ok(());
                        };
                        let Some(surviving) =
                            filter_srcset_candidates(&srcset, doc_url_path, failed)
                        else {
                            return Ok(());
                        };
                        changed.set(true);
                        if surviving.is_empty() {
                            el.remove_attribute("srcset");
                            el.remove_attribute("sizes");
                        } else {
                            el.set_attribute("srcset", &surviving)?;
                        }
                        Ok(())
                    }),
                ],
                ..lol_html::Settings::default()
            },
            |c: &[u8]| output.extend_from_slice(c),
        );
        rewriter.write(html.as_bytes()).map_err(|e| e.to_string())?;
        rewriter.end().map_err(|e| e.to_string())?;
    }

    if !changed.get() {
        return Ok(std::borrow::Cow::Borrowed(html));
    }
    String::from_utf8(output)
        .map(std::borrow::Cow::Owned)
        .map_err(|e| e.to_string())
}

/// Adjust relative paths in HTML for pretty URLs.
///
/// When `blog/article.md` becomes `blog/article/index.html`, relative paths
/// need one extra `../` to compensate for the added directory level.
/// - `foo.mp4` → `../foo.mp4` (bare relative)
/// - `./foo.mov` → `../foo.mov`
/// - `../assets/img.jpg` → `../../assets/img.jpg`
///
/// Applies to `src`, `poster`, and `data-placeholder-src` attributes.
/// Absolute paths (`/assets/...`) and external URLs (`https://...`) are unchanged.
/// `href` attributes are NOT adjusted (links are handled by the markdown link processor).
pub fn adjust_relative_paths_for_pretty_urls(html: &str) -> String {
    adjust_relative_paths_for_pretty_urls_with_overrides(html, &std::collections::HashMap::new())
}

/// Adjusts relative asset paths in HTML for pretty-URL output structure,
/// and applies `dir_overrides` to map source directory names to URL slugs.
///
/// Pretty URLs add one nesting level (`article/` → `article/index.html`),
/// so relative paths need an extra `../`. After depth adjustment, intermediate
/// directory segments are always resolved through `resolve_path_with_overrides`
/// — an explicit `dir_overrides` entry maps a source folder to its URL slug
/// (e.g., `图片/配图` → `image/assets`), and with an empty map the base
/// slugification still fires (`The Large Colour Prints` →
/// `the-large-colour-prints`) so the emitted src matches the slugified output
/// tree. The leaf filename is preserved.
pub fn adjust_relative_paths_for_pretty_urls_with_overrides(
    html: &str,
    dir_overrides: &std::collections::HashMap<String, String>,
) -> String {
    rewrite_relative_src_attrs(html, |decoded| {
        // Pretty-URL depth correction: every relative `src` shifts one level
        // deeper (the page is wrapped in its own directory), so prepend `../`.
        let adjusted = if let Some(rest) = decoded.strip_prefix("./") {
            format!("../{}", rest)
        } else {
            format!("../{}", decoded)
        };
        // Strip the leading `../` chain so resolve_path_with_overrides only
        // sees real directory names.
        let mut prefix = String::new();
        let mut rest = adjusted.as_str();
        while let Some(stripped) = rest.strip_prefix("../") {
            prefix.push_str("../");
            rest = stripped;
        }
        let resolved = crate::build::render::resolve_path_with_overrides(rest, dir_overrides);
        format!("{}{}", prefix, resolved)
    })
}

/// Applies `dir_overrides` to asset paths already present in HTML content.
///
/// This post-processes paths that were resolved by moss-core using source
/// filesystem names, mapping directory segments to their URL slugs.
/// e.g., `src="../../图片/配图/photo.jpg"` → `src="../../image/assets/photo.jpg"`.
/// Unlike [`adjust_relative_paths_for_pretty_urls_with_overrides`] this does
/// NOT add a `../` depth prefix — the resolve phase has already produced
/// pretty-URL-correct paths; only the directory-name mapping remains.
pub fn apply_dir_overrides_to_asset_paths(
    html: &str,
    dir_overrides: &std::collections::HashMap<String, String>,
) -> String {
    rewrite_relative_src_attrs(html, |decoded| {
        // Strip the leading `../` chain, then any `./` head, before applying
        // overrides — overrides operate on bare directory names.
        let mut prefix = String::new();
        let mut rest = decoded;
        while let Some(stripped) = rest.strip_prefix("../") {
            prefix.push_str("../");
            rest = stripped;
        }
        if let Some(stripped) = rest.strip_prefix("./") {
            prefix.push_str("./");
            rest = stripped;
        }
        let resolved = crate::build::render::resolve_path_with_overrides(rest, dir_overrides);
        format!("{}{}", prefix, resolved)
    })
}

/// Shared scaffolding for the two URL-attribute-rewriting post-processors.
///
/// Walks every URL-bearing attribute on a renderer-emitted element
/// (`src=` for iframe/audio source/video/img/model-viewer, `poster=` for
/// video, `data-placeholder-src=` for staged image loading, and `data=` for
/// `<object>` PDF embeds), skips values that aren't relative URLs (absolute
/// paths, scheme-prefixed URLs, data URIs, fragments, mailto), splits off
/// `?query`/`#fragment` so the path encoder cannot mangle them, decodes the
/// path part, applies the caller's transform, re-encodes via
/// [`percent_encode_path_segments`], and re-attaches the suffix. The
/// transform sees the decoded path and returns the transformed (still
/// decoded) path.
///
/// Both callers share the same regex, the same skip-list, the same
/// decode→encode boundary, and the same suffix-passthrough rule. Splitting
/// these into two near-identical bodies has caused the same bug class to
/// land twice (e.g. the `?` → `%3F` regression in commit c50bbc299) — a
/// single point of attack-surface eliminates that risk.
///
/// `href=` is intentionally NOT in the alternation. Markdown link hrefs are
/// resolved upstream by `process_markdown_file`'s link-transform closure,
/// which knows the page's pretty-URL depth from `page_map` and applies the
/// same suffix-passthrough rule there. The contract is: `src=` / `data=`
/// (URL-bearing attributes that come out of renderer-emitted HTML) → this
/// post-processor; `href=` (URL-bearing attributes that come out of the
/// markdown parser) → the link-transform closure. Both sides share the
/// `fuzzy_path` encoder so the byte sets agree.
fn rewrite_relative_src_attrs<F>(html: &str, mut transform: F) -> String
where
    F: FnMut(&str) -> String,
{
    // `data` (without `-`) targets `<object data="...">` only — every other
    // `data-*` attribute keeps its hyphen and so doesn't match this token.
    //
    // `srcset` is included because the structural-html synthesizer
    // (`build/markdown/image_render.rs`) emits `<picture><source srcset="X.webp">`
    // alongside `<img src="X.jpg">` at event-iterator time, *before* the
    // pretty-URL depth adjuster runs. Without `srcset` here, the WebP
    // `<source>` would not gain the `../` prefix that the matching `<img src>`
    // gets, and the browser would 404 the WebP — recreating the bug we
    // fixed by deploying the variants in the first place. Since the
    // responsive ladder (responsive-image-variants Task 3) `srcset` may
    // carry multiple comma-separated "URL descriptor" candidates —
    // `rewrite_srcset_candidates` splits those and rewrites each URL
    // independently; single-URL srcset stays on the shared path below.
    let re = regex::Regex::new(r#"((?:src|srcset|poster|data-placeholder-src|data)=")([^"]+)"#).unwrap();
    re.replace_all(html, |caps: &regex::Captures| {
        let attr_prefix = &caps[1];
        let value = &caps[2];

        // Don't adjust absolute paths, protocol URLs, data URIs, fragments, or mailto
        if should_skip_url(value) {
            return caps[0].to_string();
        }

        // Decode entities BEFORE splitting on `?`/`#` and before the percent
        // round-trip, for the reason `decode_html_reference_path` documents.
        // What is specific here: this is a rewrite, not a key derivation, so
        // getting it wrong aborts the path rewrite and leaks the raw
        // (un-slugified) directory name, and the assembled result is
        // re-escaped at the end to restore the attribute form.
        let unescaped = moss_core::html_entities::decode(value);

        // Ladder srcset: rewrite each comma-separated candidate's URL
        // independently so descriptors (800w, 2x, …) and the ", "
        // separators survive as literal bytes — percent-encoding them
        // makes the browser parse "URL%20800w" as one URL → 404.
        // Returns None for single-URL srcset (legacy shape), which keeps
        // the byte-identical shared path below.
        let is_srcset = attr_prefix.starts_with("srcset");
        if is_srcset {
            if let Some(rewritten) = rewrite_srcset_candidates(&unescaped, &mut transform) {
                return format!(
                    "{}{}",
                    attr_prefix,
                    moss_core::media::html_escape(&rewritten)
                );
            }
        }

        // Split off `?query`/`#fragment` so the path-only round-trip below
        // doesn't corrupt them. `percent_encode_path_segments` would turn
        // `?` into `%3F` and silently 404 any iframe `src=` carrying a query
        // string. The suffix passes through verbatim.
        let (path_only, suffix) = split_url_path(&unescaped);

        // URL-decode so dir_overrides can match non-ASCII directory names.
        let decoded = percent_decode_path(path_only);

        // Caller-supplied path transform (depth correction, override mapping).
        let resolved = transform(&decoded);

        // Re-encode so spaces and non-ASCII bytes survive the markdown→HTML
        // boundary as %20 / %XX. `srcset` (derived downstream by
        // `placeholder.rs` from this `src` value) uses spaces as URL/descriptor
        // delimiters and would mis-parse a literal-space URL.
        let encoded = percent_encode_path_segments(&resolved);

        // In `srcset` the comma is STRUCTURAL — it separates candidates — while
        // in `src`/`href` it is an ordinary legal byte, which is why
        // `percent_encode_path_segments` keeps it literal. So the decode →
        // re-encode round-trip above turns the synthesizer's emitted `%2C` back
        // into a literal comma and the browser splits one candidate into two,
        // both 404, with `<picture>` already committed. Re-apply the same
        // `,` → `%2C` the ladder branch applies, scoped to `srcset` only.
        //
        // This is the descriptorless single-URL `<source srcset="X.webp">`
        // shape, which `rewrite_srcset_candidates` deliberately declines
        // (returning None) — so without this line the encoder fix in
        // `render::image::encode_srcset_url` is undone on every non-index page,
        // whose body runs through this rewriter.
        let encoded = if is_srcset {
            encoded.replace(',', "%2C")
        } else {
            encoded
        };

        // Restore the HTML-escaped attribute form (mirrors the synthesizer's
        // `html_escape` over the emitted path), so a literal `&`/`'`/`<`/`>`
        // that percent-encoding keeps — and the decoded `?query` suffix's `&`
        // separators — become valid attribute bytes again.
        let escaped = moss_core::media::html_escape(&format!("{}{}", encoded, suffix));

        format!("{}{}", attr_prefix, escaped)
    })
    .to_string()
}

/// URL values the attribute rewriters must never touch: absolute paths,
/// protocol/external URLs, data URIs, fragments, mailto. Shared by the
/// whole-value check in [`rewrite_relative_src_attrs`] and the
/// per-candidate check in [`rewrite_srcset_candidates`] so the two
/// skip-lists cannot drift.
fn should_skip_url(url: &str) -> bool {
    url.starts_with('/')
        || url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("//")
        || url.starts_with("data:")
        || url.starts_with('#')
        || url.starts_with("mailto:")
}

/// Comma-aware srcset splitter for the responsive ladder
/// (responsive-image-variants Task 3).
///
/// Returns `Some(rewritten)` ONLY when `value` (already entity-decoded)
/// carries at least one candidate whose LAST whitespace-delimited token
/// is a width/density/height descriptor (`800w`, `2x`, `1.5x`, …) —
/// i.e. the ladder shape, where every candidate has a descriptor. Each
/// candidate URL then runs the same skip → query-split → decode →
/// transform → re-encode pipeline as single-URL attributes; the descriptor
/// and the ", " separators are re-attached verbatim, and each re-encoded
/// candidate URL is kept comma-free (`,` → `%2C`) so the list stays
/// unambiguous (follow-up #4 — a moss-emitted LADDER is already comma-free at
/// emission via `render::image::encode_srcset_url`, and this preserves it).
///
/// Returns `None` for any descriptorless value — the bare single-URL legacy
/// shape, including one whose filename contains a comma. Splitting on the comma
/// here would be wrong (it is part of the name, not a separator), so those stay
/// on the caller's whole-value path. That path applies the same `,` → `%2C`
/// when the attribute is `srcset`, so the single candidate stays unambiguous;
/// the two paths agree on the wire even though only one of them splits.
///
/// The URL part of a candidate may contain literal spaces — the
/// descriptor is recognized from the END of the candidate, so interior
/// spaces stay with the URL and get percent-encoded (the same repair the
/// single-URL path performs).
fn rewrite_srcset_candidates<F>(value: &str, transform: &mut F) -> Option<String>
where
    F: FnMut(&str) -> String,
{
    let candidates: Vec<&str> = value.split(',').collect();
    // Engage only for descriptor-bearing srcset (the ladder shape). A
    // moss-emitted LADDER is comma-free (the synthesizer `%2C`-encodes each
    // candidate URL — follow-up #4, RESOLVED 2026-07-23 — and the re-encode
    // below preserves it), so `split(',')` here hits only real separators. A
    // descriptorless single-URL srcset (`a,b.webp`) is not split here at all —
    // it stays on the whole-value path via the None-return below, which
    // `%2C`-encodes the comma itself.
    if !candidates
        .iter()
        .any(|c| split_candidate(c.trim()).1.is_some())
    {
        return None; // descriptorless — whole-value legacy path handles it
    }

    let mut out: Vec<String> = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (url, descriptor) = split_candidate(trimmed);
        // Same skip-list as the whole-value check in
        // `rewrite_relative_src_attrs`: absolute / external / data URLs
        // pass through untouched.
        let new_url = if should_skip_url(url) {
            url.to_string()
        } else {
            let (path_only, suffix) = split_url_path(url);
            let decoded = percent_decode_path(path_only);
            let resolved = transform(&decoded);
            // Re-apply the synthesizer's srcset comma-encoding (`,` → `%2C`):
            // the decode → re-encode round-trip above would otherwise turn an
            // emitted `%2C` back into a LITERAL comma (`percent_encode_path_
            // segments` keeps `,` literal), re-introducing the candidate mis-split
            // follow-up #4 closes. Scoped to this srcset-only path — the
            // whole-value `<img src>` path keeps its one literal comma.
            let encoded = percent_encode_path_segments(&resolved).replace(',', "%2C");
            format!("{}{}", encoded, suffix)
        };
        out.push(match descriptor {
            Some(d) => format!("{} {}", new_url, d),
            None => new_url,
        });
    }
    Some(out.join(", "))
}

// Canonical URL helpers from moss-core. Both rewriting functions above flow
// `src=`/`poster=`/`data-placeholder-src=` values through a decode →
// caller-transform → re-encode pipeline; `split_url_path` keeps the encoder
// away from `?query`/`#fragment` and `percent_encode_path_segments` enforces
// the same keep-list as moss-core's wikilink resolver.
use moss_core::resolve::fuzzy_path::{
    decode_html_reference_path, percent_decode_path, percent_encode_path_segments, split_url_path,
};

#[cfg(test)]
mod tests {
    use super::*;

    // ── prepare_markdown_source ───────────────────────────────────────

    #[test]
    fn prepare_applies_criticmarkup_then_percent_stripping() {
        // Order is the reason this is one function: the CriticMarkup deletion
        // carries the `%%` delimiters, so accepting it first leaves nothing
        // for the `%%` pass to mis-pair.
        assert_eq!(
            prepare_markdown_source("keep {--%%--} end", "page.md", 0),
            "keep  end"
        );
    }

    #[test]
    fn prepare_leaves_clean_source_byte_identical() {
        let body = "# Title\n\nJust prose, 50% done.\n";
        assert_eq!(prepare_markdown_source(body, "page.md", 0), body);
    }

    #[test]
    fn unterminated_comment_warning_names_the_line_in_the_authors_file() {
        // Body line 3, and the page has a 4-line frontmatter block, so the
        // author's file line is 7. Reporting the body-relative line would send
        // them to the wrong place in their editor.
        let body = "intro\n\n<!-- TODO owner assets\n\nlost content\n";
        let warning = unterminated_comment_warning(body, 4)
            .unwrap_or_else(|| panic!("expected a warning for {body:?}"));
        assert!(
            warning.contains("line 7"),
            "must point at the file line, got: {warning}"
        );
        assert!(warning.contains("will not render"), "{warning}");
    }

    #[test]
    fn unterminated_comment_warning_is_silent_on_closed_comments() {
        assert!(unterminated_comment_warning("<!-- fine -->\n\nbody\n", 0).is_none());
        assert!(unterminated_comment_warning("no comments at all\n", 0).is_none());
        // `<!--` inside a code fence is not a comment.
        assert!(unterminated_comment_warning("```\n<!-- sample\n```\n", 0).is_none());
    }

    #[test]
    fn prepare_is_a_passthrough_for_an_unterminated_comment() {
        // The warning is a side effect on the diagnostics channel; the body
        // itself must survive untouched (the tail is inert, not deleted).
        let body = "intro\n\n<!-- TODO owner assets\n\nlost content\n";
        assert_eq!(prepare_markdown_source(body, "page.md", 4), body);
    }

    // ── strip_percent_comments tests ──────────────────────────────────

    #[test]
    fn pct_inline_stripped() {
        assert_eq!(strip_percent_comments("before %%note%% after"), "before  after");
    }

    #[test]
    fn pct_inline_multiple_on_one_line() {
        assert_eq!(
            strip_percent_comments("a %%one%% b %%two%% c"),
            "a  b  c"
        );
    }

    #[test]
    fn pct_block_stripped() {
        let input = "before\n%%\nhidden text\n%%\nafter";
        assert_eq!(strip_percent_comments(input), "before\nafter");
    }

    #[test]
    fn pct_block_multiline_stripped() {
        let input = "before\n%%\nline one\nline two\n%%\nafter";
        assert_eq!(strip_percent_comments(input), "before\nafter");
    }

    #[test]
    fn pct_inside_fenced_code_preserved() {
        let input = "```\nbefore %%note%% after\n```";
        assert_eq!(strip_percent_comments(input), input);
    }

    #[test]
    fn pct_inside_inline_code_preserved() {
        let input = "Use `%%comment%%` syntax in Obsidian.";
        assert_eq!(strip_percent_comments(input), input);
    }

    #[test]
    fn pct_prose_fifty_percent_not_stripped() {
        // A lone `%` repeated twice but with space between is NOT a `%%` token.
        let input = "50% off and 80% done";
        assert_eq!(strip_percent_comments(input), input);
    }

    #[test]
    fn pct_single_percent_percent_prose_balanced_but_unintended() {
        // Two `%%` tokens on one line ARE stripped — this is the inline form.
        // Authors who want a literal `%%` in prose must escape one `%` or use code.
        let input = "profit: 50%%, cost: 30%%";
        // No space between the `%%` tokens — the first `%%` at offset 9 and the
        // second at offset 18 are a balanced pair and will be stripped.
        // Result: "profit: 50 cost: 30"
        let result = strip_percent_comments(input);
        assert!(!result.contains("%%"), "should have stripped the pair: {result}");
    }

    #[test]
    fn pct_no_double_percent_is_fast_path() {
        use std::borrow::Cow;
        let input = "plain prose with 50% discount";
        match strip_percent_comments(input) {
            Cow::Borrowed(s) => assert_eq!(s, input),
            Cow::Owned(_) => panic!("expected Cow::Borrowed (fast path) for input with no %%"),
        }
    }

    #[test]
    fn pct_block_delimiter_alone_on_line_with_whitespace() {
        // Whitespace around `%%` on its own line is allowed.
        let input = "text\n  %%  \nhidden\n  %%  \nend";
        assert_eq!(strip_percent_comments(input), "text\nend");
    }

    #[test]
    fn pct_unclosed_block_left_alone() {
        // An opening `%%` with no matching closing line is left untouched.
        let input = "text\n%%\nhidden\nend";
        let result = strip_percent_comments(input);
        // Should not have removed anything because there's no closing `%%`.
        assert!(result.contains("%%"), "unclosed block delimiter should remain: {result}");
    }

    // ── CriticMarkup accept-mode tests ────────────────────────────────

    #[test]
    fn cm_accept_addition() {
        assert_eq!(accept_criticmarkup("x {++added++} y"), "x added y");
    }

    #[test]
    fn cm_accept_deletion() {
        assert_eq!(accept_criticmarkup("x {--gone--} y"), "x  y");
    }

    #[test]
    fn cm_accept_substitution() {
        assert_eq!(accept_criticmarkup("say {~~hi~>hello~~} there"), "say hello there");
    }

    #[test]
    fn cm_accept_highlight_keeps_content() {
        assert_eq!(accept_criticmarkup("note {==key==} end"), "note key end");
    }

    #[test]
    fn cm_accept_comment_strips_entirely() {
        assert_eq!(accept_criticmarkup("text {>>TODO<<} here"), "text  here");
    }

    #[test]
    fn cm_accept_nested_wikilink_preserves_inner_syntax() {
        // The outer highlight is peeled, leaving the wiki-link intact for
        // downstream resolution.
        assert_eq!(
            accept_criticmarkup("{==[[media|media files]]==}"),
            "[[media|media files]]"
        );
    }

    #[test]
    fn cm_accept_adjacent_highlight_plus_comment() {
        // The idiomatic annotation pattern from moss-releases docs.
        assert_eq!(
            accept_criticmarkup("{==[[media]]==}{>> why <<}"),
            "[[media]]"
        );
    }

    #[test]
    fn cm_accept_skips_fenced_code_backticks() {
        let input = "```\n{==fake==}\n```";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn cm_accept_skips_fenced_code_tildes() {
        let input = "~~~\n{==fake==}\n~~~";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn cm_accept_skips_inline_code() {
        let input = "Use `{==text==}` for highlights.";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn cm_accept_skips_indented_code() {
        // New coverage from the shared inert scanner: the private mask here
        // knew fences and inline spans only, so a CriticMarkup example inside
        // an indented code block was accepted and unwrapped in the very page
        // documenting it.
        let input = "How to annotate:\n\n    {==fake==}\n";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn cm_accept_skips_html_comments() {
        // Same fix as moss#903 bug 2 one layer over: an annotation parked in
        // an authored comment stays parked instead of being unwrapped inside
        // it.
        let input = "<!-- draft: {++new text++} -->\n\nlive";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn pct_skips_html_comments() {
        let input = "<!-- note: %%hidden%% -->\n\nlive";
        assert_eq!(strip_percent_comments(input), input);
    }

    #[test]
    fn pct_skips_indented_code() {
        let input = "Obsidian comments:\n\n    %%hidden%%\n";
        assert_eq!(strip_percent_comments(input), input);
    }

    #[test]
    fn cm_accept_real_mark_outside_following_code_block() {
        let input = "```\ncode\n```\n\n{==real==}";
        assert_eq!(accept_criticmarkup(input), "```\ncode\n```\n\nreal");
    }

    #[test]
    fn cm_accept_unclosed_addition_is_passthrough() {
        assert_eq!(accept_criticmarkup("start {++ oops"), "start {++ oops");
    }

    #[test]
    fn cm_accept_substitution_without_arrow_is_passthrough() {
        let input = "{~~ no arrow ~~}";
        assert_eq!(accept_criticmarkup(input), input);
    }

    #[test]
    fn cm_accept_empty_input_returns_borrowed() {
        use std::borrow::Cow;
        match accept_criticmarkup("") {
            Cow::Borrowed(s) => assert_eq!(s, ""),
            Cow::Owned(_) => panic!("expected Cow::Borrowed for empty input"),
        }
    }

    #[test]
    fn cm_accept_plain_markdown_returns_borrowed() {
        use std::borrow::Cow;
        let input = "# Heading\n\nJust prose. No markup.";
        match accept_criticmarkup(input) {
            Cow::Borrowed(s) => assert_eq!(s, input),
            Cow::Owned(_) => panic!("expected Cow::Borrowed when no marks present"),
        }
    }

    #[test]
    fn cm_accept_multiline_mark_within_paragraph() {
        assert_eq!(
            accept_criticmarkup("{==first line\nsecond line==}"),
            "first line\nsecond line"
        );
    }

    #[test]
    fn span_comment_publishes_as_plain_text() {
        // Highlight keeps its content (markers stripped); adjacent comment is
        // stripped entirely — the idiomatic "annotated span" pattern must never
        // leak the comment text into published HTML.
        assert_eq!(
            accept_criticmarkup("a {==brown fox==}{>>too vague<<} z").as_ref(),
            "a brown fox z"
        );
    }

    #[test]
    fn point_comment_at_line_start_is_stripped() {
        // Distinct from cm_accept_comment_strips_entirely (mid-sentence).
        // A comment at the very start of input with following text must not
        // prepend any marker residue.
        assert_eq!(
            accept_criticmarkup("{>>note<<}lead text").as_ref(),
            "lead text"
        );
    }

    // ── strip_html_tags tests ─────────────────────────────────────────

    #[test]
    fn test_strip_html_tags_basic() {
        assert_eq!(strip_html_tags("<p>Hello</p>"), "Hello");
    }

    #[test]
    fn test_strip_html_tags_nested() {
        assert_eq!(strip_html_tags("<p><strong>Bold</strong> text</p>"), "Bold text");
    }

    #[test]
    fn test_strip_html_tags_entities() {
        assert_eq!(strip_html_tags("A &amp; B &lt; C"), "A & B < C");
    }

    #[test]
    fn test_strip_html_tags_trims_whitespace() {
        assert_eq!(strip_html_tags("  <p>  Hello  </p>  "), "Hello");
    }

    // ── fix_self_closing_non_void_tags tests ─────────────────────────

    #[test]
    fn test_fix_self_closing_non_void_iframe() {
        assert_eq!(
            fix_self_closing_non_void_tags(r#"<iframe src="./app/"/>"#),
            r#"<iframe src="./app/"></iframe>"#
        );
    }

    #[test]
    fn test_fix_self_closing_non_void_video() {
        assert_eq!(
            fix_self_closing_non_void_tags(r#"<video src="./video.mp4"/>"#),
            r#"<video src="./video.mp4"></video>"#
        );
    }

    #[test]
    fn test_fix_self_closing_void_elements_unchanged() {
        assert_eq!(
            fix_self_closing_non_void_tags(r#"<img src="./img.jpg"/>"#),
            r#"<img src="./img.jpg"/>"#
        );
    }

    #[test]
    fn test_fix_self_closing_already_closed_unchanged() {
        assert_eq!(
            fix_self_closing_non_void_tags(r#"<iframe src="./app/"></iframe>"#),
            r#"<iframe src="./app/"></iframe>"#
        );
    }

    #[test]
    fn test_fix_self_closing_with_spaces() {
        assert_eq!(
            fix_self_closing_non_void_tags(r#"<iframe src="./app/" />"#),
            r#"<iframe src="./app/"></iframe>"#
        );
    }

    #[test]
    fn test_fix_self_closing_mixed_content() {
        let input = r#"<img src="./img.jpg"/><iframe src="./app/"/>"#;
        let expected = r#"<img src="./img.jpg"/><iframe src="./app/"></iframe>"#;
        assert_eq!(fix_self_closing_non_void_tags(input), expected);
    }

    // ── adjust_relative_paths tests ───────────────────────────────────

    #[test]
    fn test_adjust_relative_paths_for_pretty_urls() {
        // Bare relative (no ./ prefix)
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(r#"<video src="video.mp4">"#),
            r#"<video src="../video.mp4">"#
        );
        // ./ prefix
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(r#"<img src="./image.jpg">"#),
            r#"<img src="../image.jpg">"#
        );
        // ../ prefix
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(r#"<img src="../assets/img.jpg">"#),
            r#"<img src="../../assets/img.jpg">"#
        );
        // Absolute path — unchanged
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(r#"<img src="/assets/img.jpg">"#),
            r#"<img src="/assets/img.jpg">"#
        );
        // External URL — unchanged
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(r#"<img src="https://example.com/img.jpg">"#),
            r#"<img src="https://example.com/img.jpg">"#
        );
    }

    #[test]
    fn adjust_relative_paths_slugifies_intermediate_with_empty_overrides() {
        // Bug 2: even with NO dir_overrides, intermediate asset-dir segments
        // must be slugified so the emitted src matches the slugified output
        // tree that copy_deferred_assets writes (source `The Large Colour
        // Prints/` → output `the-large-colour-prints/`). The leaf filename is
        // preserved, and exactly one `../` pretty-URL depth prefix is added.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(r#"<img src="./The Large Colour Prints/x.jpg">"#),
            r#"<img src="../the-large-colour-prints/x.jpg">"#
        );
    }

    #[test]
    fn slugifies_dir_with_html_escaped_apostrophe() {
        // Bug 2 (apostrophe folders): the `<img src>` arrives HTML-escaped —
        // the synthesizer runs `html_escape()` over the percent-encoded path,
        // so a folder like "Designs to Blair's Grave" becomes
        // `Designs%20to%20Blair&#39;s%20Grave` (space→%20, apostrophe→&#39;).
        // The `#` inside `&#39;` must NOT be mistaken for a URL `#fragment` by
        // `split_url_path`; the directory must slugify like any other.
        // Regression: before the entity-decode boundary, this dir leaked raw.
        let empty = std::collections::HashMap::new();
        assert_eq!(
            apply_dir_overrides_to_asset_paths(
                r#"<img src="../../assets/Designs%20to%20Blair&#39;s%20Grave/x.jpg">"#,
                &empty,
            ),
            r#"<img src="../../assets/designs-to-blair-s-grave/x.jpg">"#
        );
        // The pretty-URL depth adjuster (sister call site) must handle it too.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(
                r#"<img src="./Designs%20to%20Blair&#39;s%20Grave/x.jpg">"#
            ),
            r#"<img src="../designs-to-blair-s-grave/x.jpg">"#
        );
    }

    #[test]
    fn adjust_relative_paths_re_encodes_spaces_and_non_ascii() {
        // Asset filenames with spaces (Obsidian's "Pasted image YYYYMMDD…")
        // must arrive in the final HTML attribute percent-encoded. Otherwise
        // `srcset` parses the space as the URL/descriptor delimiter and the
        // image breaks entirely.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(
                r#"<img src="../assets/Pasted image 20260505.png">"#
            ),
            r#"<img src="../../assets/Pasted%20image%2020260505.png">"#
        );

        // Input that arrived already percent-encoded (from moss-core's
        // wikilink resolver) round-trips through the decode/encode cycle
        // unchanged when there are no dir_overrides to apply.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(
                r#"<img src="../assets/Pasted%20image.png">"#
            ),
            r#"<img src="../../assets/Pasted%20image.png">"#
        );

        // Non-ASCII bytes also encode rather than appearing literally —
        // `图片` → `%E5%9B%BE%E7%89%87`.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(r#"<img src="../图片/cover.jpg">"#),
            r#"<img src="../../%E5%9B%BE%E7%89%87/cover.jpg">"#
        );
    }

    #[test]
    fn srcset_ladder_candidates_rewritten_independently() {
        // Responsive ladder srcset (responsive-image-variants Task 3)
        // carries multiple comma-separated "URL descriptor" candidates.
        // Each URL gets the same depth adjustment as the matching
        // <img src>; the descriptors and ", " separators must survive as
        // literal bytes — encoding them (e.g. space → %20 across the whole
        // value) makes the browser parse "URL%20800w" as one URL → 404.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(
                r#"<source srcset="photo.w800.webp 800w, photo.w1600.webp 1600w, photo.webp 2400w" type="image/webp">"#
            ),
            r#"<source srcset="../photo.w800.webp 800w, ../photo.w1600.webp 1600w, ../photo.webp 2400w" type="image/webp">"#
        );
        // Literal spaces INSIDE a candidate URL still percent-encode (same
        // repair the single-URL path performs): the descriptor is the LAST
        // whitespace-delimited token, everything before it is the URL.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(
                r#"<source srcset="../assets/Pasted image.w800.webp 800w, ../assets/Pasted image.webp 1200w">"#
            ),
            r#"<source srcset="../../assets/Pasted%20image.w800.webp 800w, ../../assets/Pasted%20image.webp 1200w">"#
        );
        // Single-URL srcset (legacy shape, no descriptor) keeps the exact
        // pre-ladder byte behavior via the shared single-URL path.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(
                r#"<source srcset="../assets/Pasted image.webp">"#
            ),
            r#"<source srcset="../../assets/Pasted%20image.webp">"#
        );
    }

    #[test]
    fn srcset_ladder_comma_encoded_candidates_stay_comma_free_round_trip() {
        // Follow-up #4: the synthesizer now `%2C`-encodes commas in every
        // ladder srcset candidate URL (a comma-named source `a,b.webp` emits
        // `a%2Cb.w800.webp 800w, …`). The comma-aware splitter must keep each
        // candidate comma-free through the decode → depth-adjust → re-encode
        // round-trip — otherwise the emitted `%2C` would decay back to a literal
        // comma and re-introduce the browser/splitter mis-split this fix closes.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(
                r#"<source srcset="a%2Cb.w800.webp 800w, a%2Cb.w1600.webp 1600w, a%2Cb.webp 2000w" type="image/webp">"#
            ),
            r#"<source srcset="../a%2Cb.w800.webp 800w, ../a%2Cb.w1600.webp 1600w, ../a%2Cb.webp 2000w" type="image/webp">"#
        );
    }

    #[test]
    fn srcset_descriptorless_comma_filename_is_comma_encoded_on_the_legacy_path() {
        // A vault file literally named `a,b.jpg` emits the descriptorless
        // single-URL `<source srcset="a%2Cb.webp">`. The splitter must NOT
        // engage (there is one candidate, and the comma is part of the name,
        // not a separator) — but the whole-value path it falls through to must
        // still hand back a `%2C`, or the decode → re-encode round-trip decays
        // it to a literal comma, the browser reads two candidates, and both
        // 404 with `<picture>` already committed. That is the live harbor
        // bug; this test previously asserted the broken output.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(r#"<source srcset="a%2Cb.webp">"#),
            r#"<source srcset="../a%2Cb.webp">"#
        );
        // Same guarantee for a value that reaches the rewriter with the comma
        // still literal (an author-written raw `<source>`, or any emitter that
        // predates the encoder fix).
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(r#"<source srcset="a,b.webp">"#),
            r#"<source srcset="../a%2Cb.webp">"#
        );
        // `src` is NOT srcset: the comma is a legal, unstructural byte there
        // and must stay literal, or every existing comma-named image URL moves.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(r#"<img src="a,b.webp">"#),
            r#"<img src="../a,b.webp">"#
        );
    }

    #[test]
    fn srcset_ladder_dir_overrides_apply_per_candidate() {
        // The dir-overrides sister pass must map every candidate URL, not
        // just the first (or worse, treat the whole value as one path).
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("图片".to_string(), "image".to_string());
        assert_eq!(
            apply_dir_overrides_to_asset_paths(
                r#"<source srcset="../%E5%9B%BE%E7%89%87/photo.w800.webp 800w, ../%E5%9B%BE%E7%89%87/photo.webp 1600w" type="image/webp">"#,
                &overrides,
            ),
            r#"<source srcset="../image/photo.w800.webp 800w, ../image/photo.webp 1600w" type="image/webp">"#
        );
    }

    #[test]
    fn srcset_ladder_dir_overrides_keep_comma_encoded() {
        // The dir-overrides pass shares `rewrite_srcset_candidates` with the
        // pretty-URL pass, so it must also preserve the synthesizer's `%2C`
        // (follow-up #4): a comma-named source under a CJK directory keeps every
        // candidate comma-free after the override maps `图片` → `image`.
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("图片".to_string(), "image".to_string());
        assert_eq!(
            apply_dir_overrides_to_asset_paths(
                r#"<source srcset="../%E5%9B%BE%E7%89%87/a%2Cb.w800.webp 800w, ../%E5%9B%BE%E7%89%87/a%2Cb.webp 1600w" type="image/webp">"#,
                &overrides,
            ),
            r#"<source srcset="../image/a%2Cb.w800.webp 800w, ../image/a%2Cb.webp 1600w" type="image/webp">"#
        );
    }

    #[test]
    fn apply_dir_overrides_preserves_query_string_in_iframe_src() {
        // Sister function to `adjust_relative_paths_for_pretty_urls` — it
        // runs after the resolve phase whenever a site has dir_overrides
        // (e.g., Chinese-named folders mapped to URL slugs). It must apply
        // the same path/query split, otherwise iframe embeds with `?…` get
        // their `?` encoded to `%3F` and 404. The query is opaque here too:
        // dir_overrides only ever rewrite directory names, never query
        // parameter values.
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("交互".to_string(), "interactive".to_string());

        // (a) Override does not fire — query still passes through untouched.
        assert_eq!(
            apply_dir_overrides_to_asset_paths(
                r#"<iframe src="../scale-compare.html?a=major_pent%2Cmajor_blues&amp;r=major_pent%3AD"></iframe>"#,
                &overrides,
            ),
            r#"<iframe src="../scale-compare.html?a=major_pent%2Cmajor_blues&amp;r=major_pent%3AD"></iframe>"#
        );

        // (b) Override DOES fire (`交互` → `interactive`) — the suffix must
        // survive concatenation after the resolve step rewrites the path.
        // This is the case that exercises the post-resolve format string at
        // the bottom of the function; without the suffix arg, `?…` is lost.
        assert_eq!(
            apply_dir_overrides_to_asset_paths(
                r#"<iframe src="../交互/scale-compare.html?a=1&amp;r=2"></iframe>"#,
                &overrides,
            ),
            r#"<iframe src="../interactive/scale-compare.html?a=1&amp;r=2"></iframe>"#
        );

        // (c) Fragment-only and combined `?`+`#` — both opaque, both pass
        // through after the resolve step.
        assert_eq!(
            apply_dir_overrides_to_asset_paths(
                r#"<img src="../交互/scale-compare.html#section-2">"#,
                &overrides,
            ),
            r#"<img src="../interactive/scale-compare.html#section-2">"#
        );
        assert_eq!(
            apply_dir_overrides_to_asset_paths(
                r#"<iframe src="../交互/scale-compare.html?a=1#part"></iframe>"#,
                &overrides,
            ),
            r#"<iframe src="../interactive/scale-compare.html?a=1#part"></iframe>"#
        );
    }

    #[test]
    fn adjust_relative_paths_preserves_query_string_in_iframe_src() {
        // ![[scale-compare.html?a=...&r=...]] — the typed HTML embed renders
        // an iframe whose `src=` carries a query string. The post-processor
        // must not encode the `?` separator (would 404 the iframe — browser
        // would treat the whole string as a literal filename) nor rewrite the
        // query parameter values. The path part is still subject to the
        // standard encode round-trip; only the query is opaque.
        assert_eq!(
            adjust_relative_paths_for_pretty_urls(
                r#"<iframe src="scale-compare.html?a=major_pent%2Cmajor_blues&amp;r=major_pent%3AD"></iframe>"#
            ),
            r#"<iframe src="../scale-compare.html?a=major_pent%2Cmajor_blues&amp;r=major_pent%3AD"></iframe>"#
        );
    }

    // ── resolve_to_root_relative tests ───────────────────────────────

    #[test]
    fn test_resolve_to_root_relative_leading_slash_stripped() {
        assert_eq!(
            resolve_to_root_relative("/assets/img.jpg", "posts/hello/index.html"),
            "assets/img.jpg"
        );
    }

    #[test]
    fn test_resolve_to_root_relative_external_url_unchanged() {
        assert_eq!(
            resolve_to_root_relative("https://example.com/img.jpg", "posts/hello/index.html"),
            "https://example.com/img.jpg"
        );
    }

    #[test]
    fn test_resolve_to_root_relative_relative_path() {
        assert_eq!(
            resolve_to_root_relative("../../assets/img.jpg", "articles/travel/index.html"),
            "assets/img.jpg"
        );
    }

    #[test]
    fn inject_article_title_h1_prepends_heading_with_class() {
        let body = "<p>Hello world.</p>";
        let result = inject_article_title_h1(body, "AI 带来写作的黄金时代", false);
        assert_eq!(
            result,
            "<h1 class=\"moss-article-title\">AI 带来写作的黄金时代</h1>\n<p>Hello world.</p>"
        );
    }

    #[test]
    fn inject_article_title_h1_html_escapes_title() {
        let body = "<p>x</p>";
        let result = inject_article_title_h1(body, "A & B <script>alert(1)</script>", false);
        assert!(
            result.starts_with("<h1 class=\"moss-article-title\">A &amp; B &lt;script&gt;alert(1)&lt;/script&gt;</h1>"),
            "title not properly HTML-escaped: {}",
            result
        );
    }

    #[test]
    fn inject_article_title_h1_handles_empty_body() {
        // No body content (rare but possible for stub pages).
        let result = inject_article_title_h1("", "Stub", false);
        assert_eq!(result, "<h1 class=\"moss-article-title\">Stub</h1>\n");
    }

    #[test]
    fn inject_article_title_h1_emits_source_fm_when_flag_on() {
        let result = inject_article_title_h1("", "My Title", true);
        assert!(
            result.contains(r#"data-source-fm="title""#),
            "emit_source_lines=true should add data-source-fm=\"title\", got: {result}"
        );
    }

    #[test]
    fn inject_article_title_h1_omits_source_fm_when_flag_off() {
        let result = inject_article_title_h1("", "My Title", false);
        assert!(
            !result.contains("data-source-fm"),
            "emit_source_lines=false must not emit data-source-fm, got: {result}"
        );
    }

    // --- splice_after_title_block ---

    #[test]
    fn splice_after_title_block_inserts_after_lone_h1() {
        let body = "<h1 class=\"moss-article-title\">Hi</h1>\n<p>One.</p>";
        let frag = "<div class=\"date-line\">2026-04</div>";
        let out = splice_after_title_block(body, frag);
        assert_eq!(
            out,
            "<h1 class=\"moss-article-title\">Hi</h1>\n<div class=\"date-line\">2026-04</div>\n<p>One.</p>"
        );
    }

    #[test]
    fn splice_after_title_block_handles_h1_with_attributes() {
        let body = "<h1 id=\"top\" class=\"moss-article-title\">Title</h1><p>x</p>";
        let frag = "<div>D</div>";
        let out = splice_after_title_block(body, frag);
        assert_eq!(out, "<h1 id=\"top\" class=\"moss-article-title\">Title</h1>\n<div>D</div><p>x</p>");
    }

    #[test]
    fn splice_after_title_block_no_h1_prepends_fallback() {
        // Defensive: no H1 in body shouldn't drop the fragment.
        let body = "<p>only body</p>";
        let frag = "<div>D</div>";
        let out = splice_after_title_block(body, frag);
        assert_eq!(out, "<div>D</div><p>only body</p>");
    }

    /// A body that opens with `:::hero` has no title heading — the hero is the
    /// title. It may still contain an authored `# Heading` later, which renders
    /// as an `<h1>`. The date row and byline belong at the top of the article,
    /// under the hero; matching the first `</h1>` anywhere would bury them
    /// mid-prose under a heading they have nothing to do with.
    #[test]
    fn splice_after_title_block_ignores_an_h1_that_is_not_the_title() {
        let body = "<p>Standfirst.</p>\n<h1>A section</h1>\n<p>More.</p>";
        let frag = "<div>D</div>";
        assert_eq!(
            splice_after_title_block(body, frag),
            "<div>D</div><p>Standfirst.</p>\n<h1>A section</h1>\n<p>More.</p>"
        );
    }

    #[test]
    fn splice_after_title_block_empty_fragment_returns_body() {
        let body = "<h1>Hi</h1><p>x</p>";
        assert_eq!(splice_after_title_block(body, ""), body);
    }

    /// A claimed leaf's own cover wraps its title: `folder_cover::render`
    /// puts an EMPTY `<h1 class="moss-folder-title">` first (collapsed by
    /// `.moss-folder-title:empty`), then the page's real `<h1>`, inside
    /// `.moss-collection-cover-body`. The fragment must land after the REAL
    /// title, still inside that column — not before the whole cover row,
    /// which is what the old anchored "starts with h1" test did here (the
    /// body starts with `<div`, not `<h1`).
    #[test]
    fn splice_after_title_block_lands_inside_a_claimed_leafs_cover_column() {
        let body = concat!(
            r#"<div class="moss-collection-cover-row">"#,
            r#"<div class="moss-collection-cover"><img src="ada.png" /></div>"#,
            r#"<div class="moss-collection-cover-body">"#,
            r#"<h1 class="moss-folder-title"></h1>"#,
            r#"<h1 class="moss-article-title">Ada Lin</h1>"#,
            r#"<p>Body.</p></div></div>"#,
        );
        let frag = "<div class=\"date-line\">2026-04</div>";
        let out = splice_after_title_block(body, frag);
        assert_eq!(
            out,
            concat!(
                r#"<div class="moss-collection-cover-row">"#,
                r#"<div class="moss-collection-cover"><img src="ada.png" /></div>"#,
                r#"<div class="moss-collection-cover-body">"#,
                r#"<h1 class="moss-folder-title"></h1>"#,
                r#"<h1 class="moss-article-title">Ada Lin</h1>"#,
                "\n",
                r#"<div class="date-line">2026-04</div>"#,
                r#"<p>Body.</p></div></div>"#,
            )
        );
    }

    /// The `data-source-fm="title"` editor-preview variant of the empty
    /// placeholder must be recognized too, not just the bare tag.
    #[test]
    fn splice_after_title_block_recognizes_empty_folder_title_with_source_fm() {
        let body = concat!(
            r#"<div class="moss-collection-cover-row">"#,
            r#"<div class="moss-collection-cover"></div>"#,
            r#"<div class="moss-collection-cover-body">"#,
            r#"<h1 class="moss-folder-title" data-source-fm="title"></h1>"#,
            r#"<h1 class="moss-article-title">Ada Lin</h1>"#,
            r#"<p>Body.</p></div></div>"#,
        );
        let out = splice_after_title_block(body, "<div>D</div>");
        assert!(
            out.contains("<h1 class=\"moss-article-title\">Ada Lin</h1>\n<div>D</div><p>Body.</p>"),
            "got: {}",
            out
        );
    }

    // ── degrade_failed_variants ─────────────────────────────────────

    #[test]
    fn degrade_noop_when_nothing_failed() {
        let html = r#"<picture><source srcset="assets/a.w800.webp 800w, assets/a.webp 1600w" type="image/webp" sizes="100vw"><img src="assets/a.jpg" alt=""></picture>"#;
        let failed = std::collections::HashSet::new();
        let out = degrade_failed_variants(html, "index.html", &failed).unwrap();
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)), "empty failed set must not allocate");
        assert_eq!(out, html);
    }

    #[test]
    fn degrade_drops_whole_source_when_every_candidate_failed() {
        // The Anthro_C.jpg shape: base webp AND every rung failed.
        let html = r#"<picture><source srcset="assets/Anthro_C.w800.webp 800w, assets/Anthro_C.w1600.webp 1600w, assets/Anthro_C.webp 2400w" type="image/webp" sizes="100vw"><img src="assets/Anthro_C.jpg" alt=""></picture>"#;
        let mut failed = std::collections::HashSet::new();
        failed.insert("assets/Anthro_C.w800.webp".to_string());
        failed.insert("assets/Anthro_C.w1600.webp".to_string());
        failed.insert("assets/Anthro_C.webp".to_string());

        let out = degrade_failed_variants(html, "index.html", &failed).unwrap();
        assert!(matches!(out, std::borrow::Cow::Owned(_)));
        assert!(!out.contains("<source"), "source must be dropped: {out}");
        assert!(out.contains(r#"<img src="assets/Anthro_C.jpg" alt="">"#));
        assert!(out.starts_with("<picture>") && out.ends_with("</picture>"));
    }

    #[test]
    fn degrade_filters_partial_failure_keeping_surviving_candidates() {
        let html = r#"<picture><source srcset="assets/a.w800.webp 800w, assets/a.w1600.webp 1600w, assets/a.webp 2400w" type="image/webp" sizes="100vw"><img src="assets/a.jpg" alt=""></picture>"#;
        let mut failed = std::collections::HashSet::new();
        failed.insert("assets/a.w1600.webp".to_string());

        let out = degrade_failed_variants(html, "index.html", &failed).unwrap();
        assert!(out.contains("assets/a.w800.webp 800w"));
        assert!(!out.contains("a.w1600.webp"));
        assert!(out.contains("assets/a.webp 2400w"));
        assert!(out.contains("<source"), "partial survival keeps the source element");
    }

    #[test]
    fn degrade_drops_legacy_single_url_srcset_when_failed() {
        // Unknown-dims raster: no ladder, srcset is one bare URL (no `Nw` descriptor).
        let html = r#"<picture><source srcset="assets/small.webp" type="image/webp"><img src="assets/small.jpg" alt=""></picture>"#;
        let mut failed = std::collections::HashSet::new();
        failed.insert("assets/small.webp".to_string());

        let out = degrade_failed_variants(html, "index.html", &failed).unwrap();
        assert!(!out.contains("<source"));
        assert!(out.contains("assets/small.jpg"));
    }

    #[test]
    fn degrade_resolves_relative_candidate_url_against_doc_url_path() {
        // Page nested one level deep: srcset candidate carries `../` per the
        // pretty-URL depth adjustment; the failed key is root-relative.
        let html = r#"<picture><source srcset="../assets/deep.webp" type="image/webp"><img src="../assets/deep.jpg" alt=""></picture>"#;
        let mut failed = std::collections::HashSet::new();
        failed.insert("assets/deep.webp".to_string());

        let out = degrade_failed_variants(html, "research/index.html", &failed).unwrap();
        assert!(!out.contains("<source"), "must resolve ../assets/deep.webp -> assets/deep.webp before matching: {out}");
    }

    #[test]
    fn degrade_leaves_non_webp_source_and_video_untouched() {
        let html = r#"<picture><source srcset="assets/a.webp" type="image/webp"><img src="assets/a.jpg"></picture><video><source src="assets/clip.mp4" type="video/mp4"></video>"#;
        let mut failed = std::collections::HashSet::new();
        failed.insert("assets/clip.mp4".to_string()); // not a webp source — must not match the selector
        let out = degrade_failed_variants(html, "index.html", &failed).unwrap();
        assert_eq!(out, html);
    }

    #[test]
    fn degrade_multiple_pictures_only_the_failed_ones_source_changes() {
        // Two independent <picture> elements in one document; only the
        // second image has a failed variant. The first must survive
        // byte-identical — lol_html invokes the element handler once per
        // match with no shared state beyond `changed`, so this also proves
        // that flag isn't mistakenly gating/skipping later matches.
        let html = concat!(
            r#"<picture><source srcset="assets/ok.webp" type="image/webp"><img src="assets/ok.jpg" alt=""></picture>"#,
            r#"<picture><source srcset="assets/bad.webp" type="image/webp"><img src="assets/bad.jpg" alt=""></picture>"#,
        );
        let mut failed = std::collections::HashSet::new();
        failed.insert("assets/bad.webp".to_string());

        let out = degrade_failed_variants(html, "index.html", &failed).unwrap();
        assert!(out.contains(r#"<source srcset="assets/ok.webp" type="image/webp">"#), "untouched picture must survive byte-identical: {out}");
        assert!(!out.contains("assets/bad.webp"), "failed picture's source must be dropped: {out}");
        assert!(out.contains(r#"<img src="assets/bad.jpg" alt="">"#));
    }

    // ── degrade_failed_variants: bare <img srcset> (webp-source ladder) ──
    //
    // A webp SOURCE's responsive ladder is emitted directly on `<img
    // srcset>` with no `<picture>` wrapper (moss-core's `synthesize_inner`,
    // Phase B/Task 12) — the exact same terminal-404 risk as the
    // `<picture><source>` shape above, just a different element.

    #[test]
    fn degrade_img_srcset_filters_failed_rung_keeping_surviving_and_base() {
        let html = r#"<img src="assets/photo.webp" srcset="assets/photo.w800.webp 800w, assets/photo.w1600.webp 1600w, assets/photo.webp 2400w" sizes="100vw" alt="">"#;
        let mut failed = std::collections::HashSet::new();
        failed.insert("assets/photo.w800.webp".to_string());

        let out = degrade_failed_variants(html, "index.html", &failed).unwrap();
        assert!(!out.contains("photo.w800.webp"), "failed rung must be dropped: {out}");
        assert!(out.contains("assets/photo.w1600.webp 1600w"));
        assert!(out.contains("assets/photo.webp 2400w"));
        assert!(out.contains(r#"src="assets/photo.webp""#), "src attribute must be untouched: {out}");
    }

    #[test]
    fn degrade_img_srcset_all_rungs_failed_drops_srcset_and_sizes_leaving_plain_img() {
        let html = r#"<img src="assets/photo.webp" srcset="assets/photo.w800.webp 800w, assets/photo.w1600.webp 1600w" sizes="100vw" alt="">"#;
        let mut failed = std::collections::HashSet::new();
        failed.insert("assets/photo.w800.webp".to_string());
        failed.insert("assets/photo.w1600.webp".to_string());

        let out = degrade_failed_variants(html, "index.html", &failed).unwrap();
        assert!(!out.contains("srcset"), "srcset must be removed entirely — there is no element to fall through to: {out}");
        assert!(!out.contains("sizes"), "sizes is meaningless without srcset and must go with it: {out}");
        assert!(out.contains(r#"src="assets/photo.webp""#), "the base <img src> is never registry-tracked and must survive: {out}");
    }

    #[test]
    fn degrade_img_srcset_noop_when_no_rung_failed() {
        let html = r#"<img src="assets/photo.webp" srcset="assets/photo.w800.webp 800w, assets/photo.webp 2400w" sizes="100vw" alt="">"#;
        let mut failed = std::collections::HashSet::new();
        failed.insert("assets/unrelated.webp".to_string());

        let out = degrade_failed_variants(html, "index.html", &failed).unwrap();
        assert_eq!(out, html);
    }

    #[test]
    fn degrade_leaves_picture_inner_img_untouched_since_it_never_carries_srcset() {
        // The <picture> fallback <img> never gets a srcset (render_img_tag
        // always passes None for it) — the img[srcset] selector must not
        // spuriously match it even when the sibling <source> is degraded.
        let html = r#"<picture><source srcset="assets/a.w800.webp 800w, assets/a.webp 1600w" type="image/webp" sizes="100vw"><img src="assets/a.jpg" alt=""></picture>"#;
        let mut failed = std::collections::HashSet::new();
        failed.insert("assets/a.w800.webp".to_string());

        let out = degrade_failed_variants(html, "index.html", &failed).unwrap();
        assert!(out.contains(r#"<img src="assets/a.jpg" alt="">"#), "inner <img> must be untouched: {out}");
    }

    // ── degrade_failed_variants: entity-escaped filenames ────────────
    //
    // `&` and `'` are legal in a filename and deliberately NOT percent-encoded
    // by `percent_encode_path_segments`, so the emitter's `html_escape` is what
    // puts them in the attribute as `&amp;` / `&#39;` — and `lol_html`'s
    // `get_attribute` hands that text back raw, entities intact. A repair pass
    // that skips the entity decode derives `assets/a&amp;b.w800.webp` for a
    // registry key of `assets/a&b.w800.webp`, matches nothing, and ships a
    // chosen-source 404 that `<picture>` cannot fall back from (ADR-013).

    /// One asset path, encoded the way the real emitter encodes it into an
    /// attribute: percent-encode the segments, then HTML-escape. Building the
    /// fixture from the production encoders is what keeps this test honest if
    /// either of them changes its keep-list.
    fn emitted_url(path: &str) -> String {
        moss_core::media::html_escape(
            &moss_core::resolve::fuzzy_path::percent_encode_path_segments(path),
        )
    }

    #[test]
    fn degrade_repairs_variants_whose_filename_carries_an_ampersand_or_apostrophe() {
        let amp_rung = "assets/a&b.w800.webp";
        let apos_rung = "assets/Grandma's-House.w800.webp";
        let html = format!(
            concat!(
                r#"<picture><source srcset="{} 800w, {} 1600w" type="image/webp">"#,
                r#"<img src="{}" alt=""></picture>"#,
                r#"<img src="{}" srcset="{} 800w, {} 1600w" sizes="100vw" alt="">"#,
            ),
            emitted_url(amp_rung),
            emitted_url("assets/a&b.w1600.webp"),
            emitted_url("assets/a&b.jpg"),
            emitted_url("assets/Grandma's-House.webp"),
            emitted_url(apos_rung),
            emitted_url("assets/Grandma's-House.w1600.webp"),
        );
        // Anti-vacuity: the fixture only exercises the bug if the emitter's
        // escaping really did put entities in the attribute values.
        assert!(
            html.contains("a&amp;b.w800.webp") && html.contains("Grandma&#39;s-House.w800.webp"),
            "fixture check: emitted URLs carry no HTML entities: {html}"
        );

        // The registry holds on-disk names — no percent-encoding, no entities.
        let failed: std::collections::HashSet<String> =
            [amp_rung.to_string(), apos_rung.to_string()].into_iter().collect();

        let out = degrade_failed_variants(&html, "index.html", &failed).unwrap();
        assert!(
            !out.contains("a&amp;b.w800.webp"),
            "the failed `&` rung is still promised — a chosen-source 404: {out}"
        );
        assert!(
            !out.contains("Grandma&#39;s-House.w800.webp"),
            "the failed `'` rung is still promised — a chosen-source 404: {out}"
        );
        // Surviving rungs and the fall-through targets are untouched.
        assert!(out.contains("a&amp;b.w1600.webp"), "surviving rung must stay: {out}");
        assert!(
            out.contains("Grandma&#39;s-House.w1600.webp"),
            "surviving rung must stay: {out}"
        );
        assert!(out.contains("a&amp;b.jpg"), "the inner <img> must be untouched: {out}");
    }
}
