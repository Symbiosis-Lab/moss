//! Source preparation shared by build and standalone page rendering.

static CM_SUB_RE: std::sync::LazyLock<Option<regex::Regex>> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)\{~~(.*?)~>(.*?)~~\}").ok()
});
static CM_ADD_RE: std::sync::LazyLock<Option<regex::Regex>> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)\{\+\+(.*?)\+\+\}").ok()
});
static CM_DEL_RE: std::sync::LazyLock<Option<regex::Regex>> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)\{--(.*?)--\}").ok()
});
static CM_HL_RE: std::sync::LazyLock<Option<regex::Regex>> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)\{==(.*?)==\}").ok()
});
static CM_COM_RE: std::sync::LazyLock<Option<regex::Regex>> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)\{>>(.*?)<<\}").ok()
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
/// `frontmatter_line_count` offsets warning lines into the author's file.
pub fn prepare_markdown_source(body: &str, frontmatter_line_count: usize) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let accepted = accept_criticmarkup(body);
    if let Some(warning) = unbalanced_percent_warning(&accepted, frontmatter_line_count) {
        warnings.push(warning);
    }
    let stripped = strip_percent_comments(&accepted).into_owned();
    if let Some(warning) = unterminated_comment_warning(&stripped, frontmatter_line_count) {
        warnings.push(warning);
    }
    (stripped, warnings)
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
    let at = crate::inert_regions::InertRegions::scan(body).unterminated_comment()?;
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
/// stays parked (see [`crate::inert_regions::mask_inert`]).
pub fn accept_criticmarkup(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('{') {
        return std::borrow::Cow::Borrowed(text);
    }

    let masked = crate::inert_regions::mask_inert(text);

    // Collect (start, end, replacement) from all five patterns.
    let mut reps: Vec<(usize, usize, String)> = Vec::new();
    // The mask is byte-length-preserving, so a match range in it indexes
    // `text` — and every match starts at an ASCII `{`, which a masked byte
    // can never be. `get` keeps a future mask change from aborting a build.
    for cap in CM_SUB_RE.as_ref().into_iter().flat_map(|pattern| pattern.captures_iter(&masked)) {
        let Some(m) = cap.get(0) else { continue };
        let Some(new_side) = cap.get(2).and_then(|g| text.get(g.range())) else {
            continue;
        };
        reps.push((m.start(), m.end(), new_side.to_string()));
    }
    for cap in CM_ADD_RE.as_ref().into_iter().flat_map(|pattern| pattern.captures_iter(&masked)) {
        let Some(m) = cap.get(0) else { continue };
        let Some(content) = cap.get(1).and_then(|g| text.get(g.range())) else {
            continue;
        };
        reps.push((m.start(), m.end(), content.to_string()));
    }
    for cap in CM_DEL_RE.as_ref().into_iter().flat_map(|pattern| pattern.captures_iter(&masked)) {
        let Some(m) = cap.get(0) else { continue };
        reps.push((m.start(), m.end(), String::new()));
    }
    for cap in CM_HL_RE.as_ref().into_iter().flat_map(|pattern| pattern.captures_iter(&masked)) {
        let Some(m) = cap.get(0) else { continue };
        let Some(content) = cap.get(1).and_then(|g| text.get(g.range())) else {
            continue;
        };
        reps.push((m.start(), m.end(), content.to_string()));
    }
    for cap in CM_COM_RE.as_ref().into_iter().flat_map(|pattern| pattern.captures_iter(&masked)) {
        let Some(m) = cap.get(0) else { continue };
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
/// Every `%%` outside inert content pairs with the next one, on the same line
/// or any later line, and the span between them (delimiters included) is
/// removed. When both delimiters stand alone on their lines the whole lines go,
/// so no blank line is left behind. A final `%%` with no partner is left
/// visible; [`unbalanced_percent_warning`] reports it.
///
/// Inert content is never read for delimiters — [`crate::inert_regions::mask_inert`] guards
/// fenced and indented code, inline code spans and HTML comments, so
/// `` `%%code%%` `` survives.
pub fn strip_percent_comments(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("%%") {
        return std::borrow::Cow::Borrowed(text);
    }

    let masked = crate::inert_regions::mask_inert(text);
    let markers = percent_markers(&masked);
    let alone_before = |at: usize| masked.get(..at).and_then(|h| h.rsplit('\n').next()).is_some_and(|l| l.trim().is_empty());
    let alone_after = |at: usize| masked.get(at..).and_then(|t| t.split('\n').next()).is_some_and(|l| l.trim().is_empty());

    let mut out = String::with_capacity(text.len());
    let mut pos = 0usize;
    for pair in markers.chunks_exact(2) {
        let (mut start, mut end) = (pair[0], pair[1] + 2);
        if alone_before(start) && alone_after(end) {
            start = masked.get(..start).and_then(|h| h.rfind('\n')).map_or(0, |i| i + 1);
            end = masked.get(end..).and_then(|t| t.find('\n')).map_or(text.len(), |i| end + i + 1);
        }
        out.push_str(text.get(pos..start).unwrap_or_default());
        pos = end;
    }
    if pos == 0 {
        return std::borrow::Cow::Borrowed(text);
    }
    out.push_str(text.get(pos..).unwrap_or_default());
    std::borrow::Cow::Owned(out)
}

/// Byte offsets of every `%%` in `masked`, left to right, none overlapping.
fn percent_markers(masked: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0usize;
    while let Some(i) = masked.get(from..).and_then(|s| s.find("%%")) {
        found.push(from + i);
        from += i + 2;
    }
    found
}

/// Message for a `%%` left without a partner, or `None` when every marker pairs.
///
/// Takes the text *before* stripping so the line number names a line in the
/// author's file once `frontmatter_line_count` is added.
fn unbalanced_percent_warning(body: &str, frontmatter_line_count: usize) -> Option<String> {
    if !body.contains("%%") {
        return None;
    }
    let markers = percent_markers(&crate::inert_regions::mask_inert(body));
    if markers.len() % 2 == 0 {
        return None;
    }
    let at = *markers.last()?;
    let line = frontmatter_line_count + body.get(..at).map_or(1, |head| head.matches('\n').count() + 1);
    Some(format!(
        "the `%%` on line {line} has no closing `%%`, so it shows on the page as written. \
         A `%% note %%` comment is removed when its two markers are on the same line \
         or any later line."
    ))
}


#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
