//! LLM-friendly full-site content file (llms.txt)
//!
//! Generates a single markdown file containing every published page's
//! raw markdown content, for consumption by large language models.
//! See https://llmstxt.org/

use crate::build::types::ParsedDocument;
use pulldown_cmark::{Event, Parser};

/// Remove BLOCK-LEVEL raw HTML `<style>…</style>` and `<script>…</script>`
/// regions (tags and their inner contents) from a markdown body, leaving all
/// other markdown — including inline-code spans and fenced code blocks that
/// merely *contain* `<style>`/`<script>` text — untouched.
///
/// llms.txt is intentionally *raw markdown*. A literal `<style>`/`<script>`
/// block authored at the top level of a document is pure noise for an LLM and
/// would leak verbatim, so it's excised. But moss is a publishing tool whose
/// users write web-dev posts: a `` `<script>` `` inline code span or a fenced
/// ```` ```html ```` block demonstrating a `<script>` tag is legitimate content
/// and must survive byte-for-byte.
///
/// # Why pulldown-cmark events, not regex
///
/// A whole-body regex (the previous implementation) can't tell a real block
/// from a `<script>` inside a code span or fence — it destroyed both. It also
/// needed a *global* blank-line collapse to tidy the gap a removed block left,
/// which corrupted intentional blank lines inside fenced code. Parsing with
/// pulldown-cmark solves both: only `Event::Html` (block-level raw HTML)
/// qualifies as a strip candidate. `Event::Code` (inline code), and
/// `Event::Text` inside a `Tag::CodeBlock`, are never `Event::Html` — so code
/// spans and fences are automatically safe, and no global newline rewrite is
/// performed.
///
/// This aligns with the project rule: push work into pulldown-cmark events;
/// don't post-process markdown with regex.
fn strip_html_blocks(body: &str) -> String {
    // The shared constructor, so the event stream classifies this body the way
    // the real renderer does — a `<style>` the renderer treats as an HTML block
    // is the one we strip — and so a feature added there reaches here. The
    // hand-assembled copy this replaced happened to match `parser_options`
    // exactly, but nothing kept it matched.
    //
    // allow:math-events-ignored — `math = false`, so no math event is emitted
    // here at all. Even if one were, this walker never rebuilds output from the
    // event stream: it reads only `Event::Html` to collect byte ranges, then
    // excises those ranges from the ORIGINAL markdown, so every other event —
    // math included — reaches the reader as untouched source. Raw `$…$` is also
    // the right output for an LLM feed, which consumes LaTeX better than it
    // consumes an <svg>. If this is ever changed to reassemble text from events,
    // or the `false` here becomes `true`, delete this marker and add real arms
    // (see math_wiring_invariant_test).
    let opts = moss_core::ast::parser_options(false);

    // Collect the byte ranges of block-level <style>/<script> regions.
    //
    // pulldown-cmark may emit a multi-line HTML block as SEVERAL consecutive
    // `Event::Html` events (one per line — each `Event::Html` is "a line of
    // HTML inside Tag::HtmlBlock, including the line break"). A naive
    // "first event starts with <style" check would record only the opening
    // line and leak the inner CSS/JS. So we run a tiny state machine: when an
    // `Event::Html` opens a <style>/<script> block, record `range.start`, then
    // keep extending `range.end` across subsequent `Event::Html` events until
    // one closes the block. The single-event case (open + close on one line)
    // is handled by checking for the close tag on the opening event too.
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut open: Option<(usize, usize)> = None; // (start byte, close-tag kind index)
    const CLOSE_TAGS: [&str; 2] = ["</style>", "</script>"];

    for (event, range) in Parser::new_ext(body, opts).into_offset_iter() {
        let Event::Html(html) = event else {
            continue;
        };
        let lower = html.to_ascii_lowercase();
        let trimmed = lower.trim_start();

        if let Some((start, close_idx)) = open {
            // Inside an open block: extend the end and look for the matching
            // close tag (case-insensitive) to terminate.
            if lower.contains(CLOSE_TAGS[close_idx]) {
                ranges.push(start..range.end);
                open = None;
            }
            // else: keep accumulating; the next Event::Html extends the block.
        } else if trimmed.starts_with("<style") {
            if lower.contains("</style>") {
                ranges.push(range.clone()); // open + close on one event
            } else {
                open = Some((range.start, 0));
            }
        } else if trimmed.starts_with("<script") {
            if lower.contains("</script>") {
                ranges.push(range.clone()); // open + close on one event
            } else {
                open = Some((range.start, 1));
            }
        }
    }

    // An unterminated block (malformed source: opening tag with no close) —
    // strip from the opening tag to end of body rather than leaking it.
    if let Some((start, _)) = open {
        ranges.push(start..body.len());
    }

    // Nothing to strip → return the body unchanged.
    if ranges.is_empty() {
        return body.to_string();
    }

    // Excise the collected ranges back-to-front so earlier byte offsets stay
    // valid as we remove. No global newline collapse — only the spans
    // pulldown-cmark identified as block-level <style>/<script> are touched.
    let mut out = body.to_string();
    for range in ranges.into_iter().rev() {
        out.replace_range(range, "");
    }

    // A removed block usually sat between two blank lines; excising it can leave
    // a triple-blank gap. Trim the whole result's edges (cheap, local) — the
    // caller already `.trim()`s the body for emission, but trimming here keeps
    // `strip_html_blocks` self-consistent for any other caller.
    out.trim().to_string()
}

/// Generate llms.txt content from all published documents.
pub fn generate_llms_txt(
    documents: &[ParsedDocument],
    site_title: &str,
    site_description: Option<&str>,
) -> String {
    let mut out = String::new();

    // Header
    out.push_str(&format!("# {}\n\n", site_title));
    if let Some(desc) = site_description {
        out.push_str(&format!("> {}\n\n", desc));
    }

    // Collect publishable documents (exclude drafts and slot-only
    // files). Slot-only files like `footer.md` exist in `documents` so the
    // standard pipeline can wire their HTML into layout slots (PR7b),
    // but they're chrome — not articles — and don't belong in
    // llms.txt. `is_listable` is the canonical four-rule predicate.
    let mut docs: Vec<&ParsedDocument> = documents
        .iter()
        .filter(|d| d.is_listable())
        .collect();

    // Sort: weight ascending (nulls last), date descending, label ascending
    // (label is the chrome-facing plain-text identifier)
    docs.sort_by(|a, b| {
        let wa = a.weight.unwrap_or(i32::MAX);
        let wb = b.weight.unwrap_or(i32::MAX);
        wa.cmp(&wb)
            .then_with(|| b.date.cmp(&a.date))
            .then_with(|| a.label.cmp(&b.label))
    });

    for doc in docs {
        out.push_str("---\n\n");
        // Heading is consumer-facing chrome — use plain-text label, not
        // the cascade-source `title`. Mirrors the chrome decisions made
        // for the nav link text.
        out.push_str(&format!("## {}\n\n", doc.label));
        // Strip raw HTML <style>/<script> blocks (tags + contents) before
        // trimming so leftover blank lines at the edges are removed too.
        let stripped = strip_html_blocks(&doc.content);
        let body = stripped.trim();
        if !body.is_empty() {
            out.push_str(body);
            out.push_str("\n\n");
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::PageKind;

    fn create_test_document(
        title: &str,
        content: &str,
        weight: Option<i32>,
        date: Option<&str>,
        draft: Option<bool>,
    ) -> ParsedDocument {
        ParsedDocument {
            title: title.to_string(),
            label: title.to_string(),
            content: content.to_string(),
            url_path: format!("{}.html", title.to_lowercase().replace(' ', "-")),
            date: date.map(|d| d.to_string()),
            reading_time: 1,
            slug: title.to_lowercase().replace(' ', "-"),
            permalink: format!("/{}", title.to_lowercase().replace(' ', "-")), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
            weight,
            draft,
            kind: PageKind::Article,
            ..Default::default()
        }
    }

    #[test]
    fn test_basic_generation() {
        let docs = vec![
            create_test_document("Getting Started", "Welcome to moss.", None, None, None),
        ];
        let result = generate_llms_txt(&docs, "My Site", Some("A great site"));
        assert!(result.starts_with("# My Site\n\n> A great site\n\n"));
        assert!(result.contains("## Getting Started\n\nWelcome to moss."));
    }

    #[test]
    fn test_draft_pages_excluded() {
        let docs = vec![
            create_test_document("Published", "Visible content.", None, None, None),
            create_test_document("Draft Page", "Hidden content.", None, None, Some(true)),
        ];
        let result = generate_llms_txt(&docs, "Site", None);
        assert!(result.contains("Published"));
        assert!(!result.contains("Draft Page"));
        assert!(!result.contains("Hidden content"));
    }

    #[test]
    fn test_weight_ordering() {
        let docs = vec![
            create_test_document("Third", "C", Some(3), None, None),
            create_test_document("First", "A", Some(1), None, None),
            create_test_document("Second", "B", Some(2), None, None),
        ];
        let result = generate_llms_txt(&docs, "Site", None);
        let first_pos = result.find("## First").unwrap();
        let second_pos = result.find("## Second").unwrap();
        let third_pos = result.find("## Third").unwrap();
        assert!(first_pos < second_pos);
        assert!(second_pos < third_pos);
    }

    #[test]
    fn test_date_tiebreaker() {
        let docs = vec![
            create_test_document("Older", "Old.", None, Some("2024-01-01"), None),
            create_test_document("Newer", "New.", None, Some("2024-06-15"), None),
        ];
        let result = generate_llms_txt(&docs, "Site", None);
        let newer_pos = result.find("## Newer").unwrap();
        let older_pos = result.find("## Older").unwrap();
        assert!(newer_pos < older_pos, "Newer dates should come first");
    }

    #[test]
    fn test_empty_site() {
        let result = generate_llms_txt(&[], "Empty Site", Some("Nothing here"));
        assert_eq!(result, "# Empty Site\n\n> Nothing here\n\n");
    }

    #[test]
    fn test_content_is_raw_markdown() {
        let docs = vec![
            create_test_document("Test", "**bold** and [link](url)", None, None, None),
        ];
        let result = generate_llms_txt(&docs, "Site", None);
        assert!(result.contains("**bold** and [link](url)"));
        assert!(!result.contains("<strong>"));
    }

    // ----- strip_html_blocks: markdown-aware behaviour -----
    //
    // The strip is markdown-AWARE (pulldown-cmark events), not regex. The
    // contract: excise ONLY block-level raw HTML `<style>…</style>` /
    // `<script>…</script>` regions. Code spans and fenced code blocks must
    // survive verbatim (a publishing tool's users write web-dev posts that
    // legitimately *show* `<script>` source), and blank lines inside fenced
    // code must NOT be collapsed (no global newline rewrite).

    #[test]
    fn t1_multiline_style_block_stripped_prose_preserved() {
        let body = "\
**Intro** paragraph.

<style type=\"text/css\">
body { font-size: 14px; color: red; }
</style>

**Closing** paragraph.";
        let out = strip_html_blocks(body);
        assert!(!out.contains("<style"), "style tag leaked: {out:?}");
        assert!(!out.contains("</style>"), "style close tag leaked: {out:?}");
        assert!(!out.contains("font-size"), "style inner CSS leaked: {out:?}");
        assert!(out.contains("**Intro** paragraph."), "intro lost: {out:?}");
        assert!(out.contains("**Closing** paragraph."), "closing lost: {out:?}");
    }

    #[test]
    fn t2_multiline_script_block_stripped() {
        let body = "\
Before.

<script>
const x = 1;
console.log(x);
</script>

After.";
        let out = strip_html_blocks(body);
        assert!(!out.contains("<script"), "script tag leaked: {out:?}");
        assert!(!out.contains("</script>"), "script close tag leaked: {out:?}");
        assert!(!out.contains("const x"), "script inner content leaked: {out:?}");
        assert!(!out.contains("console.log"), "script inner content leaked: {out:?}");
        assert!(out.contains("Before."), "before lost: {out:?}");
        assert!(out.contains("After."), "after lost: {out:?}");
    }

    #[test]
    fn t3_fenced_code_block_with_script_preserved() {
        // H2 regression: a fenced ```html block demonstrating a <script> tag
        // is legitimate prose content and MUST survive verbatim — the regex
        // implementation destroyed it.
        let body = "\
Here is how to add a script tag:

```html
<script>
alert(1)
</script>
```

Done.";
        let out = strip_html_blocks(body);
        assert!(out.contains("<script>"), "fenced <script> was stripped: {out:?}");
        assert!(out.contains("alert(1)"), "fenced script body stripped: {out:?}");
        assert!(out.contains("</script>"), "fenced </script> stripped: {out:?}");
        assert!(out.contains("```html"), "code fence lost: {out:?}");
        assert!(out.contains("Done."), "trailing prose lost: {out:?}");
    }

    #[test]
    fn t4_inline_code_span_with_script_preserved() {
        // The `<script>` inside an inline code span is content, not a block.
        let body = "Use the `<script>` tag to embed JS.";
        let out = strip_html_blocks(body);
        assert!(out.contains("`<script>`"), "inline code span stripped: {out:?}");
        assert!(out.contains("Use the"), "prose lost: {out:?}");
    }

    #[test]
    fn t5_fenced_block_blank_lines_preserved() {
        // H1 regression: a global 3+-newline collapse corrupts intentional
        // consecutive blank lines INSIDE a fenced code block. With no
        // style/script anywhere, the body must come back byte-identical
        // (the function only trims the overall edges).
        let body = "\
```python
x = 1


y = 2
```";
        let out = strip_html_blocks(body);
        assert_eq!(out, body, "fenced blank lines were collapsed: {out:?}");
    }

    #[test]
    fn t6_identity_no_style_or_script() {
        // No style/script → output equals input (modulo the normal trim;
        // this body has no leading/trailing whitespace so it round-trips).
        let body = "\
# Heading

A paragraph with **bold** and a [link](https://example.com).

- item one
- item two";
        let out = strip_html_blocks(body);
        assert_eq!(out, body, "identity body was altered: {out:?}");
    }

    #[test]
    fn test_strips_style_and_script_blocks() {
        // Raw HTML <style>/<script> blocks embedded in source markdown must be
        // stripped (tags AND inner contents) before emission into llms.txt —
        // they're noise for an LLM and would leak verbatim. But ordinary
        // markdown syntax around them must survive untouched (llms.txt is
        // intentionally raw markdown, NOT rendered HTML).
        let content = "\
Intro paragraph with **bold** text.

<style type=\"text/css\">
body { font-size: 14px; color: red; }
</style>

Middle paragraph stays.

<script>
const secret = 42;
console.log(secret);
</script>

Closing paragraph with *emphasis*.";
        let docs = vec![create_test_document(
            "Styled",
            content,
            None,
            None,
            None,
        )];
        let result = generate_llms_txt(&docs, "Site", None);

        // Tags must be gone.
        assert!(!result.contains("<style"), "left a <style tag: {result}");
        assert!(!result.contains("</style>"), "left a </style> tag: {result}");
        assert!(!result.contains("<script"), "left a <script tag: {result}");
        assert!(!result.contains("</script>"), "left a </script> tag: {result}");

        // Inner contents must be gone.
        assert!(!result.contains("font-size"), "style inner content leaked: {result}");
        assert!(!result.contains("const secret"), "script inner content leaked: {result}");
        assert!(!result.contains("console.log"), "script inner content leaked: {result}");

        // Surrounding markdown text + syntax must survive.
        assert!(result.contains("Intro paragraph with **bold** text."));
        assert!(result.contains("Middle paragraph stays."));
        assert!(result.contains("Closing paragraph with *emphasis*."));
    }
}
