//! Pre-parse extraction of `:::shortcode` blocks from markdown source.
//!
//! Walks the markdown line-by-line, skipping the inert lines
//! [`crate::inert_regions`] reports (so `:::buttons` inside a code fence,
//! an indented code block, an inline code span or an HTML comment stays
//! literal text) and recognizing
//! `:::name ...args` / `:::` openers/closers. Each block is replaced with
//! a sentinel HTML comment (`<!--MOSS_SC_{nonce}_N-->`) that pulldown-cmark
//! emits as a `Block::Other` raw HTML; the final parser pass walks the
//! AST and substitutes the sentinels with typed [`Shortcode`] variants.
//!
//! Why this design:
//!
//! - `:::` block syntax is not standard CommonMark; pulldown-cmark sees
//!   it as plain text inside a paragraph. Post-parse text-matching is
//!   fragile (works only when the shortcode is the entire paragraph).
//! - Pre-parse extraction with a sentinel is the same pattern Zola uses
//!   and preserves parsing correctness for adjacent content.
//! - The sentinel is an HTML comment so it survives pulldown-cmark intact
//!   (pulldown-cmark passes HTML comments through `Event::Html` as
//!   `Block::HtmlBlock`).

use super::attrs::gather_multi_line_attrs;
use super::cells::split_cells;
use super::node::Block;
use super::parser::{parse_fragment_with_config, ParseConfig};
use super::shortcode::{
    ApplyShortcode, ButtonItem, ButtonsShortcode, GalleryItem, GalleryShortcode, GridShortcode,
    RecentShortcode, Shortcode, SubscribeShortcode,
};
use super::url::Url;
use crate::resolve::md_extract::{line_table, AssetPathSpan, MediaLineSpan, PathContainer};

/// One extracted shortcode block, with its body parsed into a typed variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedShortcode {
    /// 0-based index used in the placeholder sentinel.
    pub index: usize,
    /// Parsed shortcode (typed variants per Phase B).
    pub shortcode: Shortcode,
}

/// Result of pre-parse extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionResult {
    /// Markdown source with `:::shortcode` blocks replaced by sentinel
    /// HTML comments. Pulldown-cmark sees this as the input.
    pub markdown_with_placeholders: String,
    /// One entry per extracted block, indexed by sentinel number.
    pub extracted: Vec<ExtractedShortcode>,
    /// Per-extraction nonce (8 hex chars). Derived from a hash of the
    /// input markdown so it's deterministic but collision-resistant
    /// against authored content. The placeholder format is
    /// `<!--MOSS_SC_{nonce}_{index}-->`; an authored markdown comment
    /// matching that exact shape would have to embed the same hash of
    /// itself, which is computationally improbable for any input shorter
    /// than the SHA universe.
    pub nonce: String,
    /// Build warnings collected during extraction (e.g. unknown shortcode
    /// names). Each entry is a one-line human-readable string. Caller
    /// surfaces these in the build log; presence does not abort the build.
    pub warnings: Vec<String>,
}

/// Names recognized by the typed AST. Other names fall through to the
/// unknown-name renderer (`<div class="moss-unknown-shortcode" data-name="…">`)
/// with a build warning.
const TYPED_KNOWN: &[&str] = &["subscribe", "buttons", "gallery", "hero", "grid", "recent", "apply"];

fn is_typed_known(name: &str) -> bool {
    TYPED_KNOWN.contains(&name)
}

/// Recognized shortcode names (Phase B Task 7+ adds variants here).
///
/// `args` is the trailing text after `:::name ` on the opening line
/// (e.g. for `:::buttons {.primary}`, args is `{.primary}`).
///
/// Returns `(Some(Shortcode), Vec<String>)` where the second element is
/// parse-time deprecation warnings. An empty warning vec means the block
/// used only current-grammar syntax.
fn parse_shortcode_block(
    name: &str,
    args: &str,
    body: &str,
    config: &ParseConfig,
) -> (Option<Shortcode>, Vec<String>) {
    match name {
        "subscribe" => (Some(Shortcode::Subscribe(parse_subscribe_args(args))), vec![]),
        "buttons" => (Some(Shortcode::Buttons(parse_buttons_body(args, body))), vec![]),
        "gallery" => (Some(Shortcode::Gallery(parse_gallery_body(args, body))), vec![]),
        "hero" => {
            // Body media lines are the CANONICAL multi-slide grammar (the
            // only way to express a crossfading hero), not a deprecated
            // fallback — the old Priority-3 deprecation warning retired
            // with the multi-image hero.
            let (sc, _used_p3, fragment_warnings) = super::extract_hero::parse_hero(args, body, config);
            let mut warns = fragment_warnings;
            if let Some(ref v) = sc.mobile {
                if v != "overlay" {
                    warns.push(format!(
                        "shortcode `:::hero` has unrecognized `mobile={v}`. \
                         Only `mobile=overlay` is recognized. The attribute is ignored."
                    ));
                }
            }
            (Some(Shortcode::Hero(sc)), warns)
        }
        "grid" => {
            let (sc, legacy, fragment_warnings) = parse_grid(args, body, config);
            let mut warns = fragment_warnings;
            if legacy {
                warns.push(
                    "shortcode `:::grid` uses `---` cell dividers (deprecated). Migrate to `+++`.\n\
                     `---` support will be removed in a future release."
                        .to_string(),
                );
            }
            (Some(Shortcode::Grid(sc)), warns)
        }
        "recent" => (Some(Shortcode::Recent(parse_recent_args(args, body))), vec![]),
        "apply" => (Some(Shortcode::Apply(parse_apply_args(args))), vec![]),
        _ => (None, vec![]),
    }
}

/// Parse `:::recent {since=... last=... count=...}` body into a typed struct.
///
/// `args` is the attribute block (e.g. `{since="2026-04-01" count="5"}`);
/// `body` is the content between the opening and closing `:::` fences,
/// captured verbatim (trimmed) as the fallback markdown for the zero-match
/// render path.
///
/// Tolerant: unknown keys are ignored. A `count=` value that fails to parse
/// as a `u32` becomes `None`; the renderer falls back to its default (10).
/// `since` and `last` are passed through as raw strings — the rendering
/// layer parses them into a `DateTime` / `Duration` so this stays I/O-free
/// and chrono-free (moss-core invariant: pure data in / data out).
pub fn parse_recent_args(args: &str, body: &str) -> RecentShortcode {
    let attrs = super::attrs::parse_attrs(args).unwrap_or_default();
    RecentShortcode {
        since: attrs.get("since").map(str::to_string),
        last: attrs.get("last").map(str::to_string),
        count: attrs.get("count").and_then(|v| v.parse::<u32>().ok()),
        fallback_markdown: body.trim().to_string(),
    }
}

/// Parse a `:::grid` block.
///
/// Args parsing supports both:
/// - **Positional** (legacy moss-releases): `:::grid 2 1:2 {.classes}` —
///   first token is column count, second optional token is the ratio.
/// - **Attribute** (new grammar): `:::grid {cols=2}` or `:::grid {cols=1:1:2}` —
///   `cols=integer` sets the column count; `cols=ratio` sets both the
///   ratio and the count (= ratio length).
///
/// Cells are split on lines containing only `+++` (new grammar) or
/// `---` (legacy moss-releases). Step 3 of #613 rewrites `---` to `+++`
/// in moss-releases content; the parser accepts both during the
/// migration window.
///
/// Returns `(GridShortcode, bool, Vec<String>)` where the bool is `true`
/// when any `---` legacy divider was encountered (triggers a deprecation
/// warning), and the `Vec<String>` carries warnings collected while
/// re-parsing cell bodies as fragments (e.g. a misspelled shortcode nested
/// inside a cell) — see [`parse_cell_to_blocks`].
fn parse_grid(args: &str, body: &str, config: &ParseConfig) -> (GridShortcode, bool, Vec<String>) {
    let trimmed = args.trim();
    let (positional, attr_block): (&str, &str) = if let Some(pos) = trimmed.find('{') {
        // char-aligned: pos points to ASCII '{' from str::find — safe to slice.
        #[allow(clippy::string_slice)]
        (trimmed[..pos].trim(), &trimmed[pos..])
    } else {
        (trimmed, "")
    };

    let parsed = if attr_block.is_empty() {
        Default::default()
    } else {
        super::attrs::parse_attrs(attr_block).unwrap_or_default()
    };
    let classes = parsed.class_string();
    let width = parsed.width.map(str::to_string);

    let mut columns: u32 = 1;
    let mut ratio: Option<String> = None;

    if let Some(cols_value) = parsed.get("cols") {
        if cols_value.contains(':') {
            ratio = Some(cols_value.to_string());
            columns = cols_value.split(':').count() as u32;
        } else if let Ok(n) = cols_value.parse::<u32>() {
            columns = n.max(1);
        }
    } else {
        // Positional fallback: e.g. `2 1:2`.
        let parts: Vec<&str> = positional.split_whitespace().collect();
        if let Some(first) = parts.first() {
            if first.contains(':') {
                ratio = Some(first.to_string());
                columns = first.split(':').count() as u32;
            } else if let Ok(n) = first.parse::<u32>() {
                columns = n.max(1);
                if let Some(second) = parts.get(1) {
                    if second.contains(':') {
                        ratio = Some(second.to_string());
                    }
                }
            }
        }
    }

    let (raw_cells, found_legacy_dash) = split_grid_cells(body);

    // Phase 4 PR4.5 (2026-05-28): cells become Vec<Vec<Block>>. Each raw
    // cell string is either:
    //
    // - A "compound-link" cell whose entire content is wrapped in a markdown
    //   link `[inner](url)` and whose `inner` carries block-level content
    //   (image + heading + paragraphs — the SoCiviC pattern). CommonMark's
    //   inline parser cannot represent a `[](url)` with `### heading` inside,
    //   so we detect this shape at the cell-string level FIRST and emit a
    //   typed [`Block::LinkCard { url, children }`] where `children` is the
    //   inner content parsed as blocks via [`super::parser::parse`].
    //
    // - A plain markdown cell. Parse via [`super::parser::parse`] (which
    //   re-runs extract_shortcodes so any nested `::::buttons` etc. get
    //   substituted) and drop the wrapping `Document`.
    let mut fragment_warnings: Vec<String> = Vec::new();
    let cells: Vec<Vec<Block>> = raw_cells
        .iter()
        .map(|raw| {
            let (blocks, warns) = parse_cell_to_blocks(raw, config);
            fragment_warnings.extend(warns);
            blocks
        })
        .collect();

    (
        GridShortcode {
            columns,
            ratio,
            classes,
            cells,
            width,
        },
        found_legacy_dash,
        fragment_warnings,
    )
}

/// Parse one grid cell's raw markdown source into a `Vec<Block>`.
///
/// Phase 4 PR4.5 (2026-05-28): detects the compound-link shape first
/// (`[inner](url)` wrapping the entire trimmed cell content, optionally
/// followed by blank-line-separated caption paragraphs for an image-link
/// cell). On match, emits `vec![Block::LinkCard { url, children }, ...]`
/// where `children` is the inner parsed as blocks and any trailing caption
/// content is appended as sibling blocks after the card. On no match,
/// parses the cell directly via [`super::parser::parse`].
///
/// Returns `(Vec<Block>, Vec<String>)`: the parsed blocks, plus any
/// warnings collected by the fragment parse(s) underneath (e.g. a
/// misspelled `:::name` shortcode nested inside this cell). Each internal
/// [`parse_fragment_with_config`] call produces its own independent
/// `Document`, whose `warnings` would otherwise be dropped on the floor —
/// this is the plumbing that carries them out to [`parse_shortcode_block`],
/// which already merges its `Vec<String>` into `doc.warnings`.
fn parse_cell_to_blocks(raw: &str, config: &ParseConfig) -> (Vec<Block>, Vec<String>) {
    if let Some((url, inner, trailing)) = detect_compound_link(raw) {
        let inner_trimmed = inner.trim();
        let trailing_trimmed = trailing.trim();
        // Simple compound-link special case: when the inner content is
        // plain phrasing text (no images, no nested links, no
        // block-level markdown) AND the URL is external, fall through
        // to the normal markdown parse so the cell stays a
        // `[Paragraph([Link])]` — the typed shape the host's grid-cell
        // classifier (`build/render/grid_cells.rs`) turns into a link
        // preview (title + favicon + domain). A `LinkCard` cell is final
        // markup that already carries its own `<a class="moss-grid-card">`
        // chrome, so the host leaves it alone and the title row is lost.
        //
        // Mirrors the pre-PR4.5 carve-out in
        // `crate::build::markdown::typed_renderers::render_compound_link_cell`
        // (the `if !inner.contains('!') && !inner.contains('[') && !inner.contains('\n')`
        // branch).
        let inner_is_plain_text = !inner_trimmed.contains('!')
            && !inner_trimmed.contains('[')
            && !inner_trimmed.contains('\n');
        let is_external = url.starts_with("http://") || url.starts_with("https://");
        if inner_is_plain_text && is_external {
            // Re-emit as standard markdown link inside a paragraph so the
            // host's link-preview pass owns the rendering.
            let linkified = format!("[{}]({})", inner_trimmed, url);
            let doc = parse_fragment_with_config(&linkified, config);
            return (doc.blocks, doc.warnings);
        }
        let inner_doc = parse_fragment_with_config(inner_trimmed, config);
        let mut warnings = inner_doc.warnings;
        let mut blocks = vec![Block::LinkCard {
            url: Url::unresolved(url),
            children: inner_doc.blocks,
        }];
        if !trailing_trimmed.is_empty() {
            let trailing_doc = parse_fragment_with_config(trailing_trimmed, config);
            blocks.extend(trailing_doc.blocks);
            warnings.extend(trailing_doc.warnings);
        }
        return (blocks, warnings);
    }
    // Phase 4 PR4.5 (2026-05-28): bare-URL cell auto-promotion. When the
    // entire cell content is a single bare URL on its own line (no
    // markdown link syntax), parse it as `[](URL)` so the cell renders as
    // `<p><a href="URL"></a></p>` (an empty-text link inside a paragraph).
    // The host's grid-cell pass (`build/render/grid_cells.rs`) reads this
    // shape off the typed cell and replaces it with a
    // `<span class="link-preview-domain">…</span>` wrapper carrying
    // title/favicon (from cached link metadata).
    //
    // Matches the pre-PR4.5 `linkify_bare_urls_in_cell` behavior — the
    // helper turned `https://...` into `[](https://...)` so the downstream
    // compound-link pass picked it up. PR4.5 ports the linkification to
    // parse time so the bytes flow through the typed AST.
    if let Some(url) = detect_bare_url_cell(raw) {
        let linkified = format!("[]({})", url);
        let doc = parse_fragment_with_config(&linkified, config);
        return (doc.blocks, doc.warnings);
    }
    let doc = parse_fragment_with_config(raw, config);
    (doc.blocks, doc.warnings)
}

/// Detect a "bare URL cell": the entire cell content (after trim) is a
/// single `https?://...` URL on its own line, with no other content.
///
/// Returns the URL string on match, `None` otherwise. Used by
/// [`parse_cell_to_blocks`] to linkify bare-URL cells via `[](URL)` so
/// they thread through the host's link-preview pass like authored
/// `[Title](URL)` cells.
fn detect_bare_url_cell(cell_text: &str) -> Option<String> {
    let trimmed = cell_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.lines().count() > 1 {
        return None;
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return None;
    }
    if trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Detect the compound-link shape in a grid cell's markdown content.
///
/// Matches cells whose content (after trimming whitespace) begins with `[`
/// and, after balanced-bracket scanning, `](url)`. The inner content may
/// span blank lines and contain any markdown block syntax (headings,
/// images, paragraphs, lists, emphasis).
///
/// Returns `Some((url, inner_content, trailing_content))` on a match,
/// `None` otherwise. `trailing_content` is non-empty only for the
/// image-link-plus-caption shape described below; it is `""` for the
/// classic whole-cell-is-the-link shape.
///
/// Ported from src-tauri's `crate::build::markdown::typed_renderers::
/// detect_compound_link` (Phase 4 PR4.5, 2026-05-28) — the AST-level
/// equivalent of the same string-level detection. The src-tauri version
/// is deleted in PR4.5. NOT the general cure for `[![[x.png]]](/url)` —
/// [`super::linked_embed`] is; this is the block-level grid *card*.
///
/// Safety rules that cause this function to return `None`:
/// - Cell contains a top-level code fence (\`\`\` or ~~~).
/// - Cell content starts with a backtick (inline code on first line).
/// - The outer `[…](url)` shape cannot be confirmed by bracket-balance
///   scanning (multiple top-level links, bare `]` / `(` without a pair).
/// - There is content after the closing `)` that continues the SAME
///   paragraph (no blank line before it) — CommonMark already renders
///   `[Link](url) more text` correctly as one inline paragraph, and
///   hijacking it into a card would change ordinary link cells.
/// - There is content after the closing `)`, separated by a blank line,
///   but the inner content does not lead with a WIKILINK image (`![[`).
///   Trailing caption paragraphs are only recognized for this exact shape,
///   the image-card-plus-caption cell (see moss#928-adjacent). A cell led by
///   an ordinary markdown image (`![alt](src)`) is deliberately excluded:
///   pulldown-cmark parses `![alt](src)` fine on its own, so
///   `[![alt](src)](url)\n\ncaption` already reaches the plain block
///   parser and comes out as `Paragraph([Link([Image])])` +
///   `Paragraph([caption])` — exactly the shape the host's grid-cell
///   classifier (`build/render/grid_cells.rs`) needs to recognize an
///   external link preview. Widening this gate to any `!` would silently
///   swallow that into a `LinkCard` and drop the preview chrome.
///
/// Detection uses bracket balancing so nested `](` sequences inside images
/// (`![alt](src)`) or inline code do NOT prematurely end the outer link.
pub(super) fn detect_compound_link(cell_text: &str) -> Option<(String, String, String)> {
    let stripped = cell_text.trim();

    if !stripped.starts_with('[') {
        return None;
    }
    if stripped.len() > 1 && stripped.as_bytes()[1] == b'`' {
        return None;
    }

    for line in stripped.lines() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            return None;
        }
    }

    let bytes = stripped.as_bytes();

    // Phase 1: find the outer closing `]` via bracket-balance scan.
    let mut i: usize = 1;
    let mut depth: usize = 1;
    let mut outer_close: Option<usize> = None;

    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'`' => {
                let tick_start = i;
                while i < bytes.len() && bytes[i] == b'`' {
                    i += 1;
                }
                let fence_len = i - tick_start;
                'code_scan: while i < bytes.len() {
                    if bytes[i] == b'`' {
                        let close_start = i;
                        while i < bytes.len() && bytes[i] == b'`' {
                            i += 1;
                        }
                        if i - close_start == fence_len {
                            break 'code_scan;
                        }
                    } else {
                        i += 1;
                    }
                }
                continue;
            }
            b'[' => {
                depth += 1;
            }
            b']' => {
                depth -= 1;
                if depth == 0 {
                    outer_close = Some(i);
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let close_bracket = outer_close?;

    if bytes.get(close_bracket + 1) != Some(&b'(') {
        return None;
    }

    // Phase 2: find the matching `)` with paren balance.
    let mut j = close_bracket + 2;
    let mut pdepth: usize = 1;
    let mut paren_close: Option<usize> = None;

    while j < bytes.len() {
        match bytes[j] {
            b'\\' => {
                j += 2;
                continue;
            }
            b'(' => pdepth += 1,
            b')' => {
                pdepth -= 1;
                if pdepth == 0 {
                    paren_close = Some(j);
                    break;
                }
            }
            _ => {}
        }
        j += 1;
    }

    let close_paren = paren_close?;

    // Phase 3: after `)`, either nothing/whitespace (classic shape), or a
    // blank-line-separated caption for an image-link cell (see doc comment).
    let tail = stripped.get(close_paren + 1..)?;
    let inner = stripped.get(1..close_bracket)?;
    let trailing = if tail.chars().all(char::is_whitespace) {
        ""
    } else {
        let blank_line_before_trailing = {
            let mut newlines = 0;
            for c in tail.chars() {
                if c == '\n' {
                    newlines += 1;
                } else if !c.is_whitespace() {
                    break;
                }
            }
            newlines >= 2
        };
        let candidate = tail.trim();
        // Scoped to the wikilink-image shape only (see doc comment): an
        // ordinary `![alt](src)` inner already reaches the plain block
        // parser and renders correctly on its own, so it's excluded here
        // rather than swallowed into a LinkCard.
        if !blank_line_before_trailing || !inner.trim_start().starts_with("![[") {
            return None;
        }
        candidate
    };

    // Phase 4: validate inner content.
    if inner.trim().is_empty() {
        return None;
    }

    // Phase 5: reject multiple top-level links (images allowed).
    {
        let inner_bytes = inner.as_bytes();
        let mut k: usize = 0;
        let mut image_stack: Vec<bool> = Vec::new();

        while k < inner_bytes.len() {
            match inner_bytes[k] {
                b'\\' => {
                    k += 2;
                    continue;
                }
                b'`' => {
                    let tick_start = k;
                    while k < inner_bytes.len() && inner_bytes[k] == b'`' {
                        k += 1;
                    }
                    let fence_len = k - tick_start;
                    'inner_code: while k < inner_bytes.len() {
                        if inner_bytes[k] == b'`' {
                            let cs = k;
                            while k < inner_bytes.len() && inner_bytes[k] == b'`' {
                                k += 1;
                            }
                            if k - cs == fence_len {
                                break 'inner_code;
                            }
                        } else {
                            k += 1;
                        }
                    }
                    continue;
                }
                b'[' => {
                    let preceded_by_bang = k > 0 && inner_bytes[k - 1] == b'!';
                    image_stack.push(preceded_by_bang);
                }
                b']' => {
                    if let Some(is_image) = image_stack.pop() {
                        if image_stack.is_empty() && inner_bytes.get(k + 1) == Some(&b'(') {
                            if !is_image {
                                return None;
                            }
                        }
                    }
                }
                _ => {}
            }
            k += 1;
        }
    }

    let url = stripped.get(close_bracket + 2..close_paren)?;
    Some((url.to_string(), inner.to_string(), trailing.to_string()))
}

/// Split a grid body into cells on lines containing only `+++` (new
/// grammar) or `---` (legacy moss-releases backward-compat).
///
/// Mirrors [`super::cells::split_cells`] but accepts either divider.
/// Step 3 of #613 rewrites `---` to `+++` in moss-releases content;
/// after that, this helper retires in favor of `split_cells`.
///
/// **A divider only counts when it belongs to THIS grid.** A `+++` inside a
/// nested block, a code fence or an HTML comment is somebody else's. It used
/// to split this grid anyway, so `::::grid` around `:::grid` — the nesting
/// the grammar recommends — handed the inner grid's cells to the outer one,
/// with no warning and plausible-looking output. Nested bodies pass through
/// verbatim and are re-extracted when the cell is parsed, which is where the
/// inner block gets its own dividers. Depth is tracked as
/// [`super::editor_scan`] tracks it: an opener pushes its arity, a closer of
/// the innermost arity pops it. Same-arity nesting is a different bug with
/// its own warning (`nested_arity_warning`).
///
/// Returns `(cells, found_legacy_dash)` where `found_legacy_dash` is
/// `true` when at least one `---` divider **of this grid** was encountered,
/// signaling the caller to emit a deprecation warning. A `---` belonging to
/// a nested block is reported when that block is parsed, not here.
fn split_grid_cells(body: &str) -> (Vec<String>, bool) {
    if body.is_empty() {
        return (vec![String::new()], false);
    }
    // One flag per line, aligned with `str::lines`, which agrees index-for-
    // index with `split_inclusive('\n')` for every input.
    let inert = crate::inert_regions::inert_lines(body);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut first_line_in_cell = true;
    let mut found_legacy_dash = false;
    // Arities of the nested blocks we are currently inside; empty means
    // "at this grid's own depth", which is the only place a divider counts.
    let mut nested_arities: Vec<usize> = Vec::new();

    for (idx, line) in body.split_inclusive('\n').enumerate() {
        let content_no_eol = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = content_no_eol.trim();
        let live = !inert.get(idx).copied().unwrap_or(false);
        if live {
            if let Some((inner_arity, _, _)) = parse_shortcode_opener(trimmed) {
                nested_arities.push(inner_arity);
            } else if let Some(&innermost) = nested_arities.last() {
                if is_close_fence(trimmed, innermost) {
                    nested_arities.pop();
                }
            } else if trimmed == "+++" || trimmed == "---" {
                if trimmed == "---" {
                    found_legacy_dash = true;
                }
                if let Some(stripped) = current.strip_suffix('\n') {
                    current.truncate(stripped.len());
                }
                cells.push(std::mem::take(&mut current));
                first_line_in_cell = true;
                continue;
            }
        }
        if first_line_in_cell {
            first_line_in_cell = false;
            if trimmed.is_empty() {
                continue;
            }
        }
        current.push_str(line);
    }
    if let Some(stripped) = current.strip_suffix('\n') {
        current.truncate(stripped.len());
    }
    cells.push(current);
    (cells, found_legacy_dash)
}

/// Byte-offset asset paths inside `:::gallery` / `:::hero` blocks.
///
/// The structural half of rename tracking: these paths carry NO markdown
/// reference syntax, so [`crate::resolve::md_extract::extract_md_references`]
/// cannot see them and a rename silently broke them. Offsets are absolute in
/// `source`; the body is never joined, so CRLF sources are exact by
/// construction (unlike `extract_with_state`, which parses a
/// `lines().join("\n")` copy and destroys offsets one level above the
/// per-line parsers).
///
/// Recognition reads [`crate::inert_regions::mask_inert`] — the same mask
/// [`crate::resolve::md_extract`] scans, so the two passes agree byte for
/// byte on what is live syntax. Emitted STRINGS are sliced from `source`.
///
/// # Where the mask applies, and where it must not
///
/// The mask decides which lines carry a live `:::` OPENER or CLOSER — the
/// same question `extract_with_state` asks it. It does NOT filter body
/// lines: once a `:::gallery` opener is live, the block grammar owns every
/// line until the closer, and `parse_gallery_body` reads them verbatim. A
/// path inside a fenced code block in a gallery body really does render as
/// an image, so masking it would leave exactly the silent rename break this
/// scanner exists to fix.
///
/// # Deliberate divergence from `extract_with_state`
///
/// **Descends into every unrecognized block**, where the extractor recurses
/// only into CssRegion and Unknown bodies. A `:::gallery` nested inside
/// `::::grid` is therefore found. Over-approximating is safe: a rename only
/// fires on an exact path match.
///
/// Kept in step with the parsers by `spans_agree_with_parsers`.
pub fn shortcode_asset_spans(source: &str) -> Vec<AssetPathSpan> {
    let mask = crate::inert_regions::mask_inert(source);
    // Line table over the RAW source. Index-aligned with `str::lines()`, but
    // additionally carrying each line's absolute base and terminator length
    // so `outer` can cover the whole physical line.
    let table = line_table(source);
    let mask_lines: Vec<&str> = mask.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < table.len() {
        let Some(mline) = mask_lines.get(i) else { break };
        let Some((arity, name, single_line_args)) = parse_shortcode_opener(mline.trim()) else {
            i += 1;
            continue;
        };

        // Where does the opener's attribute block end? Reuse the extractor's
        // own ladder so a multi-line `{ … }` is measured identically.
        let (_, opener_lines_consumed) =
            gather_multi_line_attrs(single_line_args, &mask_lines[i + 1..]);
        let body_start = i + 1 + opener_lines_consumed;

        // Matching closer at this arity.
        let mut close = None;
        for j in body_start..table.len() {
            if is_close_fence(mask_lines.get(j).map_or("", |l| l.trim()), arity) {
                close = Some(j);
                break;
            }
        }
        let Some(j) = close else {
            // Unclosed: `extract_with_state` emits the block verbatim, so
            // nothing inside it is live. Descend in place.
            i += 1;
            continue;
        };

        match name {
            "gallery" => {
                for k in body_start..j {
                    if let Some(span) = gallery_body_span(source, &table, k) {
                        out.push(span);
                    }
                }
                i = j + 1;
            }
            "hero" => {
                super::extract_hero::hero_asset_spans(source, &mask, &table, i, body_start, j, &mut out);
                i = j + 1;
            }
            // Unknown / CssRegion / other typed block: descend in place so a
            // gallery nested inside it is still found.
            _ => i += 1,
        }
    }

    out
}

/// One `:::gallery` body line → an [`AssetPathSpan`], or `None`.
///
/// The **emission filter** lives here, not in [`gallery_item_span`]: a
/// gallery body line is only reported as an asset reference when it carried
/// markdown reference syntax or its bare path has a known media extension
/// ([`HERO_MEDIA_EXTENSIONS`], the SSOT — extending that list extends gallery
/// rename tracking). Without it, every line of prose in a gallery body would
/// show up in the delete-confirmation modal.
fn gallery_body_span(
    source: &str,
    table: &[(usize, usize, usize)],
    k: usize,
) -> Option<AssetPathSpan> {
    let (base, content_len, term_len) = *table.get(k)?;
    #[allow(clippy::string_slice)]
    // `base` and `base + content_len` are line boundaries from `line_table`,
    // which splits on ASCII '\n'/'\r' only.
    let line = &source[base..base + content_len];
    let it = gallery_item_span(line)?;
    if !it.is_token && !super::extract_hero::is_bare_hero_media(&it.path) {
        return None;
    }
    Some(AssetPathSpan {
        path: crate::media::strip_wikilink(&it.path).to_string(),
        attrs: it.value_attrs,
        quote: None,
        value: base + it.value.start..base + it.value.end,
        outer: base..base + content_len + term_len,
        container: PathContainer::GalleryBody,
    })
}

/// Recognize one `:::gallery` body line, with LINE-RELATIVE byte offsets.
///
/// This is the single grammar for a gallery body line: [`parse_gallery_body`]
/// is a thin loop over it, and [`shortcode_asset_spans`] lifts its offsets to
/// absolute. The three arms and their ORDER mirror the historical parser
/// exactly — `![[…]]` first, then the pipe split, then the markdown-image
/// pattern — because that ordering is observable (`![alt](x.png|cover)`
/// splits on the pipe first and therefore is NOT a markdown image).
///
/// Returns a span for every non-blank line, prose included; the media filter
/// lives in [`shortcode_asset_spans`], which is what must not put prose in
/// the delete-confirmation modal.
pub(crate) fn gallery_item_span(line: &str) -> Option<MediaLineSpan> {
    let lead = line.len() - line.trim_start().len();
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Wikilink embed: ![[path|attrs]] — checked BEFORE the generic pipe
    // split below, mirroring `hero_media_line_span`'s ordering (the
    // wikilink's own `|` separates path from attrs and must be split on
    // the INNER content, not the whole `![[...]]` line).
    if let Some(inner) = trimmed.strip_prefix("![[").and_then(|s| s.strip_suffix("]]")) {
        let (src_raw, attrs) = split_pipe(inner);
        // `![[` is 3 ASCII bytes; the inner text is contiguous.
        let inner_start = lead + 3;
        return Some(MediaLineSpan {
            path: src_raw.trim().to_string(),
            alt: String::new(),
            attrs: attrs.to_string(),
            value: inner_start..inner_start + inner.len(),
            value_attrs: attrs.to_string(),
            is_token: true,
        });
    }

    // Each line: `path|attrs`, `![alt](path)|attrs`, or bare `path`.
    // The pipe split (if any) is BEFORE the markdown-image pattern check.
    let (src_raw, attrs) = split_pipe(trimmed);
    match parse_markdown_image(src_raw) {
        Some((alt, path)) => {
            // The rename span is the destination inside the parens only. A
            // `|attrs` suffix here sits AFTER the closing paren, outside the
            // span, so `value_attrs` is empty and the suffix is left alone.
            let s2 = src_raw.trim();
            let s2_lead = lead + (src_raw.len() - src_raw.trim_start().len());
            // `](` is ASCII and `parse_markdown_image` already matched it.
            let path_start = s2_lead + s2.find("](").map_or(0, |i| i + 2);
            Some(MediaLineSpan {
                value: path_start..path_start + path.len(),
                path,
                alt,
                attrs: attrs.to_string(),
                value_attrs: String::new(),
                is_token: true,
            })
        }
        None => {
            let path = src_raw.trim().to_string();
            let path_start = lead + (src_raw.len() - src_raw.trim_start().len());
            // With attrs, the span covers `path|attrs` so a rewrite can
            // rebuild the pair; without, just the path.
            let value_end = if attrs.is_empty() {
                path_start + path.len()
            } else {
                lead + trimmed.len()
            };
            Some(MediaLineSpan {
                value: path_start..value_end,
                path,
                alt: String::new(),
                attrs: attrs.to_string(),
                value_attrs: attrs.to_string(),
                is_token: false,
            })
        }
    }
}

fn parse_gallery_body(args: &str, body: &str) -> GalleryShortcode {
    // Args: `N {.classes width}` where N is optional columns count and
    // `width` is one of the spec § P9 width tokens (handled inside
    // `split_positional_and_classes`).
    let (positional, classes, width) = split_positional_classes_and_width(args);
    let columns = if positional.is_empty() {
        None
    } else {
        positional.parse::<u32>().ok()
    };
    let mut items: Vec<GalleryItem> = Vec::new();
    for line in body.lines() {
        if let Some(it) = gallery_item_span(line) {
            items.push(GalleryItem {
                src: Url::unresolved(it.path),
                alt: it.alt,
                attrs: it.attrs,
            });
        }
    }
    GalleryShortcode {
        columns,
        classes,
        items,
        width,
    }
}

/// Split `args` into `(positional_text, classes, width)`.
///
/// Same routing as [`split_positional_and_classes`], but also surfaces the
/// spec § P9 width token (`body | wide | page | screen`, with `full`
/// aliased to `screen`). Returns `width = None` when the author did not
/// set one, or when the legacy fallback path fires (malformed attrs
/// where the structured parser bailed).
fn split_positional_classes_and_width(args: &str) -> (String, String, Option<String>) {
    let trimmed = args.trim();
    if let Some(brace_start) = trimmed.find('{') {
        #[allow(clippy::string_slice)]
        let after_open = &trimmed[brace_start..];
        if let Some(brace_end) = after_open.find('}') {
            #[allow(clippy::string_slice)]
            let positional = trimmed[..brace_start].trim().to_string();
            #[allow(clippy::string_slice)]
            let attr_block_str = &trimmed[brace_start..=brace_start + brace_end];
            if let Ok(parsed) = super::attrs::parse_attrs(attr_block_str) {
                return (
                    positional,
                    parsed.class_string(),
                    parsed.width.map(str::to_string),
                );
            }
            // Legacy fallback for malformed inputs: scan only for `.class`.
            // Width tokens are skipped here on purpose — if attrs are
            // malformed enough to bail, the author's intent is unclear and
            // omitting the width is safer than guessing.
            #[allow(clippy::string_slice)]
            let inner = &trimmed[brace_start + 1..brace_start + brace_end];
            let mut classes = Vec::new();
            for token in inner.split_whitespace() {
                if let Some(class) = token.strip_prefix('.') {
                    if !class.is_empty() {
                        classes.push(class);
                    }
                }
            }
            return (positional, classes.join(" "), None);
        }
    }
    (trimmed.to_string(), String::new(), None)
}

/// Split `args` into `(positional_text, classes)` from `{...}` syntax.
///
/// Routes the attribute portion through [`crate::ast::attrs::parse_attrs`]
/// so the unified grammar's full surface (`.class`, `#id`, `key=value`,
/// quoted values, multi-line) is recognized — even though the legacy
/// shortcodes (Subscribe / Buttons / Gallery) only consume the class
/// list today. Step 2 migrates Hero / Grid; once they read `kvs` and
/// `id` via `parse_attrs` directly, this helper retires.
///
/// Falls back to the legacy whitespace-tokenized class scan when
/// `parse_attrs` returns `Err` (malformed attrs, unterminated quote,
/// etc.) so existing content with edge-case `{}` shapes still parses
/// the way it did before.
fn split_positional_and_classes(args: &str) -> (String, String) {
    let trimmed = args.trim();
    if let Some(brace_start) = trimmed.find('{') {
        // char-aligned: brace_start points to ASCII '{' from str::find — the
        // byte index is a char boundary, so slicing `trimmed[brace_start..]`
        // is safe to feed into the next find.
        #[allow(clippy::string_slice)]
        let after_open = &trimmed[brace_start..];
        if let Some(brace_end) = after_open.find('}') {
            // char-aligned: brace_start (ASCII '{') and brace_start+brace_end
            // (ASCII '}') are both char boundaries; `brace_start + 1` lands on
            // the byte after '{', also a boundary.
            #[allow(clippy::string_slice)]
            let positional = trimmed[..brace_start].trim().to_string();
            #[allow(clippy::string_slice)]
            let attr_block_str = &trimmed[brace_start..=brace_start + brace_end];
            if let Ok(parsed) = super::attrs::parse_attrs(attr_block_str) {
                return (positional, parsed.class_string());
            }
            // Legacy fallback for malformed inputs that the structured
            // parser rejects (e.g. unterminated quote on a single line).
            #[allow(clippy::string_slice)]
            let inner = &trimmed[brace_start + 1..brace_start + brace_end];
            let mut classes = Vec::new();
            for token in inner.split_whitespace() {
                if let Some(class) = token.strip_prefix('.') {
                    if !class.is_empty() {
                        classes.push(class);
                    }
                }
            }
            return (positional, classes.join(" "));
        }
    }
    (trimmed.to_string(), String::new())
}

/// Split `s` on `|` into `(before, after)`. If no pipe, returns `(s, "")`.
fn split_pipe(s: &str) -> (&str, &str) {
    match s.split_once('|') {
        Some((before, after)) => (before, after.trim()),
        None => (s, ""),
    }
}

/// Parse `![alt](path)` into `(alt, path)`. Returns `None` if not a
/// markdown image. Mirrors the legacy parser at shortcode.rs:1615.
fn parse_markdown_image(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    let rest = s.strip_prefix("![")?;
    let (alt, after) = rest.split_once("](")?;
    let close_paren = after.rfind(')')?;
    // char-aligned: close_paren points to ASCII ')' from str::rfind.
    #[allow(clippy::string_slice)]
    let path = &after[..close_paren];
    if path.contains('(') {
        return None;
    }
    Some((alt.to_string(), path.to_string()))
}

fn parse_buttons_body(args: &str, body: &str) -> ButtonsShortcode {
    let (_positional, classes) = split_positional_and_classes(args);
    let mut items: Vec<ButtonItem> = Vec::new();
    // Split the body on `+++` cell dividers (unified grammar).
    // Bodies without `+++` produce a single cell containing the entire
    // body — backward-compatible with the legacy "one link per line"
    // shape.
    for cell in split_cells(body) {
        for line in cell.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((text, url)) = extract_markdown_link(trimmed) {
                items.push(ButtonItem {
                    text,
                    url: Url::unresolved(url),
                });
            }
            // Non-link lines silently ignored (matches legacy behavior).
        }
    }
    ButtonsShortcode { classes, items }
}

/// Extract a markdown link `[text](url)` from a single trimmed line.
/// Returns `(text, url)` if the line is a single link, else `None`.
fn extract_markdown_link(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    let inside = s.strip_prefix('[')?;
    let (text, after) = inside.split_once(']')?;
    let url = after.strip_prefix('(').and_then(|r| r.strip_suffix(')'))?;
    if url.is_empty() {
        return None;
    }
    Some((text.to_string(), url.to_string()))
}

/// Parse `:::subscribe {placeholder="..." button="..."}` into a typed struct.
///
/// Reads `placeholder` and `button` from the attribute block; ignores
/// classes/id (the renderer uses fixed `moss-subscribe` chrome). Body
/// must be empty under the unified grammar — caller is responsible for
/// surfacing a deprecation warning if non-empty.
fn parse_subscribe_args(args: &str) -> SubscribeShortcode {
    // Empty args produce an empty AttrBlock; both fields stay None
    // and the renderer falls back to language defaults.
    let parsed = match super::attrs::parse_attrs(args) {
        Ok(b) => b,
        Err(_) => return SubscribeShortcode::default(),
    };
    let placeholder = parsed
        .get("placeholder")
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let button = parsed
        .get("button")
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    SubscribeShortcode {
        placeholder,
        button,
    }
}

/// Parse `:::apply {placeholder="..." button="..."}` into a typed struct.
///
/// Reads `placeholder` and `button` from the attribute block; ignores
/// classes/id (the renderer uses fixed `moss-apply` chrome). Body must be
/// empty under the unified grammar. Mirrors `parse_subscribe_args`.
pub fn parse_apply_args(args: &str) -> ApplyShortcode {
    let parsed = match super::attrs::parse_attrs(args) {
        Ok(b) => b,
        Err(_) => return ApplyShortcode::default(),
    };
    let placeholder = parsed
        .get("placeholder")
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let button = parsed
        .get("button")
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    ApplyShortcode {
        placeholder,
        button,
    }
}

/// The sentinel HTML comment used to mark an extracted shortcode in the
/// markdown source. Pulldown-cmark emits these as [`Event::Html`] inside
/// a [`Tag::HtmlBlock`], which surfaces as [`Block::Other`] in our AST.
///
/// `nonce` is the per-extraction hash from [`ExtractionResult::nonce`],
/// which forecloses the namespace-collision case where an author writes
/// `<!--MOSS_SC_*-->` literally in their markdown.
pub fn placeholder_for(nonce: &str, index: usize) -> String {
    format!("<!--MOSS_SC_{nonce}_{index}-->")
}

/// Try to interpret a [`Block::Other`] payload as a shortcode placeholder
/// matching the given `nonce`. Returns the `index` if it matches.
///
/// Any sentinel with a different (or absent) nonce is rejected — that's
/// what makes authored content with a similar comment shape inert.
pub fn parse_placeholder(nonce: &str, html: &str) -> Option<usize> {
    let trim = html.trim();
    let prefix = format!("<!--MOSS_SC_{nonce}_");
    let inner = trim.strip_prefix(&prefix)?;
    let inner = inner.strip_suffix("-->")?;
    inner.parse::<usize>().ok()
}

/// Compute the per-extraction nonce from the input markdown. Uses
/// `std::hash::DefaultHasher` (FxHash-like; not cryptographic, but good
/// enough to make a literal authored-content collision computationally
/// improbable for any short input). Returns 8 hex characters.
fn compute_nonce(input: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    // Truncate to 32 bits for an 8-char hex; collisions across two
    // sites are not a concern (each extraction uses its own nonce
    // for its own substitution). Per-extraction collision-resistance
    // requires only that the nonce differs from any literal string
    // in the same input — 32 bits is overkill for that.
    let h = hasher.finish() as u32;
    format!("{h:08x}")
}

/// Walk the markdown line-by-line, replace `:::name` blocks with sentinels.
///
/// Tracks fenced code blocks (` ``` ` and `~~~`) so `:::buttons` inside a
/// code fence stays inert. Currently recognizes `:::subscribe`; other
/// shortcodes are added in Phase B Tasks 8-11. Unrecognized `:::name`
/// blocks pass through verbatim (the legacy string-rewriter still
/// processes them during the staged migration).
pub fn extract_shortcodes(markdown: &str) -> ExtractionResult {
    extract_shortcodes_with_config(markdown, &ParseConfig::default())
}

/// [`extract_shortcodes`], parsing shortcode bodies with the caller's
/// [`ParseConfig`].
///
/// Shortcode inner content is sub-parsed, so a config that stops here does
/// not stop being observable — it produces a page written in two dialects.
/// With math on, `$E=mc^2$` one line outside a `:::hero` became an
/// equation while the identical bytes inside it stayed literal text,
/// because every sub-parse called the default-config `parse()`.
/// `shortcode_config_leak_invariant` pins the bare call out of existence.
pub fn extract_shortcodes_with_config(
    markdown: &str,
    config: &ParseConfig,
) -> ExtractionResult {
    let nonce = compute_nonce(markdown);
    let mut extracted: Vec<ExtractedShortcode> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let output = extract_with_state(markdown, &nonce, &mut extracted, &mut warnings, config);
    ExtractionResult {
        markdown_with_placeholders: output,
        extracted,
        nonce,
        warnings,
    }
}

/// Recursive worker for [`extract_shortcodes`]. Walks `markdown`
/// line-by-line and returns the body string with sentinels substituted
/// for typed shortcode blocks. Inner CssRegion / Unknown blocks recurse
/// here so their bodies also get scanned for typed shortcodes — the
/// shared `extracted` and `warnings` accumulators ensure all sentinels
/// across nesting levels share the same nonce and a flat index space.
fn extract_with_state(
    markdown: &str,
    nonce: &str,
    extracted: &mut Vec<ExtractedShortcode>,
    warnings: &mut Vec<String>,
    config: &ParseConfig,
) -> String {
    let mut output = String::with_capacity(markdown.len());
    let lines: Vec<&str> = markdown.lines().collect();
    // Which lines are code or comment, and therefore carry no live `:::`
    // syntax. One shared scanner for every pre-parse pass in the tree —
    // this module used to track fenced code itself and knew nothing about
    // HTML comments, which is how a `:::gallery` inside an authored
    // `<!-- TODO … -->` block got extracted, spliced a sentinel into the
    // middle of the comment, and deleted the rest of the page (#903 bug 2).
    let inert = crate::inert_regions::inert_lines(markdown);
    let is_inert = |idx: usize| inert.get(idx).copied().unwrap_or(false);
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Inert line: emit verbatim, recognize nothing.
        if is_inert(i) {
            output.push_str(line);
            output.push('\n');
            i += 1;
            continue;
        }

        // Try to recognize a `:::name` (or `::::name`, etc.) opener.
        if let Some((arity, name, single_line_args)) = parse_shortcode_opener(trimmed) {
            // Multi-line attribute block support: if the args contain an
            // unclosed `{`, gather subsequent lines into the args string
            // until the brace closes (respecting quoted strings). The
            // body starts on the line AFTER the close-brace line.
            //
            // `:::name {key=value\n  key2=value2\n}` is valid; the
            // attribute parser sees the joined string and treats newlines
            // as whitespace.
            let (args_owned, opener_lines_consumed) =
                gather_multi_line_attrs(single_line_args, &lines[i + 1..]);
            let args: &str = args_owned.as_deref().unwrap_or(single_line_args);
            let body_start = i + 1 + opener_lines_consumed;

            // Look for the matching closer (same arity) on a subsequent line.
            //
            // While scanning, remember the first body line that is itself an
            // OPENER at the same arity. First-closer-wins means the closer we
            // are about to accept is that inner block's, not ours — see
            // `nested_same_arity` below.
            let mut body_lines: Vec<&str> = Vec::new();
            let mut j = body_start;
            let mut closed = false;
            let mut nested_same_arity: Option<&str> = None;
            while j < lines.len() {
                // An inert line cannot close the block either: a bare `:::`
                // inside a code fence or a comment in the body used to end
                // the shortcode early and strand the rest of it as prose.
                let body_trimmed = lines[j].trim();
                if !is_inert(j) && is_close_fence(body_trimmed, arity) {
                    closed = true;
                    break;
                }
                if nested_same_arity.is_none() && !is_inert(j) {
                    if let Some((inner_arity, _, _)) = parse_shortcode_opener(body_trimmed) {
                        if inner_arity == arity {
                            nested_same_arity = Some(body_trimmed);
                        }
                    }
                }
                body_lines.push(lines[j]);
                j += 1;
            }

            if !closed {
                // Unclosed block: emit verbatim, let the legacy rewriter
                // surface the syntax error.
                output.push_str(line);
                output.push('\n');
                i += 1;
                continue;
            }

            // The block closed — but if a same-arity opener sat in the body,
            // the fence we just stopped on belongs to THAT block, and this
            // one ended early. Nothing about the parse changes; the author
            // just gets told, because the page still builds and only looks
            // wrong (stray `+++`, cells outside the grid). See #1014.
            if let Some(inner) = nested_same_arity {
                warnings.push(nested_arity_warning(arity, trimmed, inner));
            }

            let body = body_lines.join("\n");

            // Branch on the recognized name:
            //
            // 1. Pure-CSS region (empty name, e.g. `:::{.tagline}`) — emit
            //    a plain `<div class="...">` wrapper around the body markdown.
            //    Pulldown-cmark processes the body naturally because we
            //    insert blank lines around it.
            //
            // 2. Typed-known name (subscribe / buttons / gallery / hero / grid)
            //    — extract into the typed AST and substitute a sentinel.
            //    Parse-time deprecation warnings (e.g. legacy `---` dividers
            //    in grid, body-image fallback in hero) are threaded back via
            //    the warnings vector.
            //
            // 3. Anything else — render as a `moss-unknown-shortcode` div
            //    around the body markdown and emit a build warning.
            if name.is_empty() {
                // CssRegion (Task D). Recurse into the body so typed
                // shortcodes nested inside the styling wrapper (the
                // common SoCiviC pattern of `:::{.support-band}` around
                // `::::buttons`) also get extracted into sentinels.
                // Higher-arity inner blocks survive because the outer
                // closer-search only matches the outer's exact arity;
                // the recursive call then handles the inner.
                let parsed = super::attrs::parse_attrs(args).unwrap_or_default();
                let body_processed = extract_with_state(&body, nonce, extracted, warnings, config);
                output.push_str(&render_div_open(&parsed.classes, parsed.id.as_deref(), None));
                output.push_str("\n\n");
                output.push_str(&body_processed);
                if !body_processed.is_empty() && !body_processed.ends_with('\n') {
                    output.push('\n');
                }
                output.push_str("\n</div>\n");
                i = j + 1;
                continue;
            }

            if is_typed_known(name) {
                if let (Some(sc), parse_warnings) = parse_shortcode_block(name, args, &body, config) {
                    warnings.extend(parse_warnings);
                    let index = extracted.len();
                    output.push_str(&placeholder_for(&nonce, index));
                    output.push('\n');
                    // Preserve the block's original line count. The block spanned
                    // lines i..=j (opener..closer); the sentinel is a single line,
                    // so pad with (j - i) blank lines. This keeps the post-
                    // extraction LineLookup (parser.rs) line-accurate: without it,
                    // a multi-line shortcode (grid/hero) collapses to one line and
                    // every data-source-line AFTER it drifts, breaking editor↔
                    // preview scroll sync (the home page grid scrolled the preview
                    // to the bottom). Trailing blank lines after the sentinel HTML
                    // comment produce no pulldown-cmark events, so the AST is
                    // unchanged. See docs/reference/editor-preview-sync.md.
                    for _ in 0..(j - i) {
                        output.push('\n');
                    }
                    extracted.push(ExtractedShortcode {
                        index,
                        shortcode: sc,
                    });
                    i = j + 1;
                    continue;
                }
                // Should not happen — typed-known is a closed set of
                // names handled by parse_shortcode_block. Fall through
                // to verbatim emission as defense-in-depth.
                output.push_str(line);
                output.push('\n');
                i += 1;
                continue;
            }

            // Unknown name (Task E): wrap the body in a fallback div and
            // emit a build warning so authors see misspellings. Recurse
            // into the body so a misspelled outer doesn't strand any
            // valid typed shortcodes nested inside it.
            let parsed = super::attrs::parse_attrs(args).unwrap_or_default();
            warnings.push(format!("unknown shortcode `:::{}`", name));
            let mut classes = vec!["moss-unknown-shortcode".to_string()];
            classes.extend(parsed.classes.iter().cloned());
            let extra_attrs = format!(r#" data-name="{}""#, html_escape_attr(name));
            let body_processed = extract_with_state(&body, nonce, extracted, warnings, config);
            output.push_str(&render_div_open(&classes, parsed.id.as_deref(), Some(&extra_attrs)));
            output.push_str("\n\n");
            output.push_str(&body_processed);
            if !body_processed.is_empty() && !body_processed.ends_with('\n') {
                output.push('\n');
            }
            output.push_str("\n</div>\n");
            i = j + 1;
            continue;
        }

        // Regular content line.
        output.push_str(line);
        output.push('\n');
        i += 1;
    }

    output
}

/// Word the "this block ended at someone else's fence" warning (#1014).
///
/// `arity` and `outer_line` describe the OUTER opener; `inner_line` is the
/// trimmed text of the nested opener that has the same colon count. Says what
/// went wrong in plain words first, then exactly what to type: one more colon
/// on the outer fence, nested fences left alone.
///
/// The suggestion is built by prefixing one `:` to the author's own opener
/// line, so it reads back in their words (`:::grid 2` → `::::grid 2`) and
/// works for a nameless CSS region (`:::{.band}` → `::::{.band}`) too.
fn nested_arity_warning(arity: usize, outer_line: &str, inner_line: &str) -> String {
    let colons = ":".repeat(arity);
    let wider = ":".repeat(arity + 1);
    let outer_short = clip_for_warning(outer_line);
    let inner_short = clip_for_warning(inner_line);
    format!(
        "shortcode `{outer_short}` ends at the nested `{inner_short}` block's closing fence \
         instead of its own, so everything after that point falls outside the block.\n\
         A nested fence needs FEWER colons than the block around it: write `:{outer_short}` … \
         `{wider}` for the outer block and leave the nested one as `{colons}`."
    )
}

/// Opener lines are short; cap anyway so a pathological one can't flood the
/// build log.
fn clip_for_warning(line: &str) -> String {
    let mut s: String = line.chars().take(48).collect();
    if line.chars().count() > 48 {
        s.push('…');
    }
    s
}

/// Render the opening `<div>` tag for a CssRegion or Unknown wrapper.
///
/// `extra_attrs` (already with leading space) is appended before `>`,
/// used by the unknown-name renderer to add `data-name="..."`.
fn render_div_open(classes: &[String], id: Option<&str>, extra_attrs: Option<&str>) -> String {
    let mut out = String::from("<div");
    if !classes.is_empty() {
        out.push_str(" class=\"");
        for (i, c) in classes.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&html_escape_attr(c));
        }
        out.push('"');
    }
    if let Some(id_val) = id {
        out.push_str(" id=\"");
        out.push_str(&html_escape_attr(id_val));
        out.push('"');
    }
    if let Some(extra) = extra_attrs {
        out.push_str(extra);
    }
    out.push('>');
    out
}

/// HTML-attribute-safe escape. Replaces the five XML special characters
/// so attribute values can't break out of `"..."` or close the tag.
fn html_escape_attr(s: &str) -> String {
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

/// Parse an opening fence line into (colon_count, name, args). Returns
/// `None` if the line is not an opener.
///
/// Accepts any colon count >= 3 (`:::name`, `::::name`, `:::::name`, ...).
/// The colon count is preserved so the closer must match the same arity
/// (allows nested shortcodes like `::::buttons` inside `:::grid`).
///
/// **Pure-CSS region opener** — `:::{.class}` (no name, attrs only) is
/// also recognized. The returned `name` is empty, signaling the caller
/// to render the block as a plain styling wrapper. Empty name without
/// a following `{` is rejected (just colons followed by content is not
/// an opener).
pub(crate) fn parse_shortcode_opener(trimmed: &str) -> Option<(usize, &str, &str)> {
    let colons = trimmed.chars().take_while(|&c| c == ':').count();
    if colons < 3 {
        return None;
    }
    // char-aligned: `colons` is a count of ASCII ':' chars (each 1 byte in
    // UTF-8), so the byte offset equals the char count and lands on a
    // char boundary.
    #[allow(clippy::string_slice)]
    let rest = &trimmed[colons..];
    // Name = letters/digits/underscores/hyphens; rest of line is args.
    let name_end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(rest.len());
    if name_end == 0 {
        // No name. Pure-CSS region grammar requires the rest to start
        // with `{` (after whitespace).
        let after_ws = rest.trim_start();
        if !after_ws.starts_with('{') {
            return None;
        }
        return Some((colons, "", rest.trim()));
    }
    // char-aligned: name_end is a byte index returned by str::find with a
    // char predicate, which is guaranteed to be a char boundary (or rest.len()).
    #[allow(clippy::string_slice)]
    let name = &rest[..name_end];
    #[allow(clippy::string_slice)]
    let args = rest[name_end..].trim();
    Some((colons, name, args))
}

/// True if `trimmed` is a closing fence with the specified arity (`:::`
/// for arity 3, `::::` for arity 4, etc.).
///
/// Closer semantics: N colons followed by optional whitespace only. A
/// line like `::: extra` is NOT a closer (it's body content). This was
/// the legacy `parse_fence_close` contract; the typed extractor preserves
/// it so author content with trailing text after `:::` still parses the
/// same way.
///
/// Implemented via char iteration (NOT `split_at(arity)`) because the
/// `arity` is a count of `:` characters (always ASCII, 1 byte each), but
/// the `trimmed` line might start with multi-byte UTF-8 characters
/// (e.g. `[申请测试版](...)` from Chinese-language buttons). `split_at`
/// is byte-indexed and would panic mid-character on such lines. Char
/// iteration sidesteps the issue and is also slightly faster — we early-exit
/// on the first non-`:` character.
fn is_close_fence(trimmed: &str, arity: usize) -> bool {
    let mut chars = trimmed.chars();
    for _ in 0..arity {
        match chars.next() {
            Some(':') => {}
            _ => return false,
        }
    }
    // Remaining chars (if any) must all be whitespace.
    chars.all(char::is_whitespace)
}

#[cfg(test)]
#[path = "shortcode_extract_tests.rs"]
mod tests;
