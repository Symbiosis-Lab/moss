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

use super::attrs::{gather_multi_line_attrs, html_attribute_name};
use super::cells::split_cells;
use super::document::{BlockMeta, Document};
use super::node::{Block, Inline};
use super::parser::{parse_fragment_with_config, ParseConfig};
use super::shortcode::{
    ButtonItem, ButtonsShortcode, GalleryItem, GalleryShortcode, GridShortcode,
    RecentShortcode, Shortcode, ShortcodeSource, SourceFragment, SubscribeShortcode,
};
use super::url::Url;
use crate::resolve::md_extract::{
    extract_md_references, line_table, AssetPathSpan, MediaLineSpan, PathContainer, RefSyntax,
};

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
    /// Advisories that are not build problems: the author wrote something
    /// that parses but has no effect on the output. They are logged at the
    /// plain warning level, outside the problem count `--strict` reads.
    pub advisories: Vec<String>,
    /// One entry per empty-name fenced div (`:::{.x}`) the extractor wrapped,
    /// in source order of their openers, nested regions included. Lets a
    /// caller audit which classes a page uses without re-parsing the fences.
    pub css_regions: Vec<CssRegionSpan>,
}

/// An empty-name fenced div the extractor wrapped in a plain `<div>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssRegionSpan {
    /// The classes written on the opener, without the leading dot.
    pub classes: Vec<String>,
    /// 1-based source line of the opener, counted with
    /// `ParseConfig::warning_line_offset` applied.
    pub line: usize,
}

/// What a shortcode's parse reports back to the extractor. `warnings` are
/// build problems and are counted by `--strict`; `advisories` are notes that
/// change nothing in the output and are logged without being counted.
#[derive(Debug, Default, Clone)]
pub(crate) struct Diagnostics {
    pub(crate) warnings: Vec<String>,
    pub(crate) advisories: Vec<String>,
}

/// Split a fragment `Document` into its blocks, their source positions and the
/// diagnostics its own parse raised, so nothing a nested body reports is
/// dropped on the way out.
pub(crate) fn fragment_parts(doc: Document) -> (Vec<Block>, Vec<BlockMeta>, Diagnostics) {
    (doc.blocks, doc.block_meta, Diagnostics { warnings: doc.warnings, advisories: doc.advisories })
}

/// `config` for a fragment whose first line is `body_line` (0-based) of the
/// body `config` describes. The one place a fragment's start is carried, so
/// the fragment's blocks and warnings report document lines instead of
/// fragment lines.
pub(crate) fn config_at_body_line(config: &ParseConfig, body_line: usize) -> ParseConfig {
    ParseConfig {
        source_line_offset: config.source_line_offset + body_line,
        warning_line_offset: config.warning_line_offset + body_line,
        ..*config
    }
}

/// The source position of a fragment whose first line is `body_line` and
/// which holds `raw`.
pub(crate) fn source_fragment(
    config: &ParseConfig,
    body_line: usize,
    raw: &str,
    metas: Vec<BlockMeta>,
) -> SourceFragment {
    let first = config.emit_source_lines.then(|| config.source_line_offset + body_line + 1);
    SourceFragment {
        first_line: first,
        last_line: first.map(|f| f + raw.lines().count().saturating_sub(1)),
        blocks: metas,
    }
}

/// Names recognized by the typed AST. Other names fall through to the
/// unknown-name renderer (`<div class="moss-unknown-shortcode" data-name="…">`)
/// with a build warning.
const TYPED_KNOWN: &[&str] = &["subscribe", "buttons", "gallery", "hero", "grid", "recent"];

fn is_typed_known(name: &str) -> bool {
    TYPED_KNOWN.contains(&name)
}

/// Recognized shortcode names (Phase B Task 7+ adds variants here).
///
/// `args` is the trailing text after `:::name ` on the opening line
/// (e.g. for `:::buttons {.primary}`, args is `{.primary}`).
///
/// Returns `(Some(Shortcode), Vec<String>)` where the second element is
/// parse-time deprecation warnings. An empty `warnings` means the block
/// used only current-grammar syntax.
fn parse_shortcode_block(
    name: &str,
    args: &str,
    body: &str,
    config: &ParseConfig,
) -> (Option<Shortcode>, Diagnostics) {
    match name {
        "subscribe" => (Some(Shortcode::Subscribe(parse_subscribe_args(args))), Diagnostics::default()),
        "buttons" => {
            let (sc, warnings) = parse_buttons_body(args, body);
            (Some(Shortcode::Buttons(sc)), Diagnostics { warnings, ..Default::default() })
        }
        "gallery" => {
            let (sc, warns) = parse_gallery_body(args, body, config);
            (Some(Shortcode::Gallery(sc)), Diagnostics { warnings: warns, ..Default::default() })
        }
        "hero" => {
            // Body media lines are the CANONICAL multi-slide grammar (the
            // only way to express a crossfading hero), not a deprecated
            // fallback — the old Priority-3 deprecation warning retired
            // with the multi-image hero.
            let (sc, _used_p3, fragment_warnings) = super::extract_hero::parse_hero(args, body, config);
            let mut warns = fragment_warnings;
            if let Some(ref v) = sc.mobile {
                if v != "overlay" {
                    warns.warnings.push(format!(
                        "shortcode `:::hero` has unrecognized `mobile={v}`. \
                         Only `mobile=overlay` is recognized. The attribute is ignored."
                    ));
                }
            }
            if let Some(ref v) = sc.align {
                if !matches!(v.as_str(), "top" | "end" | "center" | "start") {
                    warns.warnings.push(format!(
                        "shortcode `:::hero` has unrecognized `align={v}`. \
                         Only `align=top`, `align=end` and `align=center` are recognized. The attribute is ignored."
                    ));
                }
            }
            if let Some(dark) = &sc.dark {
                let is_video = |u: &super::url::Url| {
                    let raw = match u {
                        super::url::Url::Unresolved(s) => s.as_str(),
                        super::url::Url::Resolved(r) => r.href.as_str(),
                    };
                    matches!(
                        crate::resolve::ext_kind::reference_kind_for_ext(raw.rsplit('.').next().unwrap_or("")),
                        crate::resolve::ext_kind::ExtKind::Video
                    )
                };
                if sc.image.is_none() || !sc.extra_images.is_empty() || sc.image.as_ref().is_some_and(is_video) || is_video(dark) {
                    warns.warnings.push(
                        "shortcode `:::hero` has `dark=` without a single picture to pair it with (a slideshow, a video, or no `image=`). The attribute is ignored.".to_string(),
                    );
                }
            }
            if let Some(ref v) = sc.header {
                if v != "over" {
                    warns.warnings.push(format!(
                        "shortcode `:::hero` has unrecognized `header={v}`. \
                         Only `header=over` is recognized. The attribute is ignored."
                    ));
                }
            }
            (Some(Shortcode::Hero(sc)), warns)
        }
        "grid" => {
            let (sc, legacy, fragment_warnings) = parse_grid(args, body, config);
            let mut warns = fragment_warnings;
            if let Some(body_line) = legacy {
                warns.warnings.push(format!(
                    "`---` on line {} divides `:::grid` cells but is deprecated: write `+++` instead \
                     (`---` support will be removed in a future release).",
                    config.warning_line_offset + body_line + 1
                ));
            }
            (Some(Shortcode::Grid(sc)), warns)
        }
        "recent" => (Some(Shortcode::Recent(parse_recent_args(args, body))), Diagnostics::default()),
        _ => (None, Diagnostics::default()),
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

/// Reads the `per-line=` column-count attribute, with the deprecated
/// `cols=` spelling as a fallback. Shared between `:::grid` and
/// `:::gallery` so the alias and its warning exist in exactly one place.
///
/// `per-line` is the current name — under vertical typesetting the grid's
/// tracks run along the line, not down a "column", so "N per line" is true
/// in both writing modes where "N columns" is not. When both are written,
/// `per-line` wins, but the second element is still `true` — the author
/// wrote the deprecated key, so they still see the warning even though it
/// lost the value.
fn per_line_attr<'a>(parsed: &'a super::attrs::AttrBlock) -> (Option<&'a str>, bool) {
    let cols = parsed.get("cols");
    (parsed.get("per-line").or(cols), cols.is_some())
}

/// Deprecation warning for the `cols=` alias, shared between `:::grid` and
/// `:::gallery` so the message can't drift between the two shortcodes.
fn cols_alias_warning(shortcode_name: &str) -> String {
    format!(
        "shortcode `:::{shortcode_name}` uses `cols=` (deprecated). \
         Use `per-line=` or the positional form `:::{shortcode_name} 3`."
    )
}

/// Most columns a `:::grid` can lay out. The site stylesheet has a column
/// rule for each count from 1 to this value; a larger count would match no
/// rule and collapse to one column without a word.
pub const MAX_GRID_COLUMNS: u32 = 6;

/// Parse a `:::grid` block.
///
/// Args parsing supports both:
/// - **Positional** (legacy moss-releases): `:::grid 2 1:2 {.classes}` —
///   first token is column count, second optional token is the ratio.
/// - **Attribute** (current grammar): `:::grid {per-line=2}` or
///   `:::grid {per-line=1:1:2}` — `per-line=integer` sets the column
///   count; `per-line=ratio` sets both the ratio and the count (= ratio
///   length). `cols=` is a deprecated alias for `per-line=` — see
///   [`per_line_attr`].
///
/// Cells are split on lines containing only `+++` (new grammar) or
/// `---` (legacy moss-releases). A migration step rewrites `---` to `+++`
/// in moss-releases content; the parser accepts both during the
/// migration window.
///
/// Returns `(GridShortcode, bool, Vec<String>)` where the bool is `true`
/// when any `---` legacy divider was encountered (triggers a deprecation
/// warning), and the `Vec<String>` carries warnings collected while
/// re-parsing cell bodies as fragments (e.g. a misspelled shortcode nested
/// inside a cell) — see [`parse_cell_to_blocks`] — plus the `cols=`
/// deprecation warning when that alias was written.
fn parse_grid(args: &str, body: &str, config: &ParseConfig) -> (GridShortcode, Option<usize>, Diagnostics) {
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
    let scroll = parsed.scroll;
    let collage = parsed.collage;
    let label = parsed.get("label").map(str::to_string);

    let mut columns: u32 = 1;
    let mut ratio: Option<String> = None;
    let (per_line_value, used_cols_alias) = per_line_attr(&parsed);

    if let Some(cols_value) = per_line_value {
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

    let (raw_cells, legacy_dash_line) = split_grid_cells(body);
    let mut fragments = Vec::with_capacity(raw_cells.len());

    // Phase 4 PR4.5 (2026-05-28): cells become Vec<Vec<Block>>. Each raw
    // cell string is either:
    //
    // - A "compound-link" cell whose entire content is wrapped in a markdown
    //   link `[inner](url)` and whose `inner` carries block-level content
    //   (image + heading + paragraphs — the poster-card pattern). CommonMark's
    //   inline parser cannot represent a `[](url)` with `### heading` inside,
    //   so we detect this shape at the cell-string level FIRST and emit a
    //   typed [`Block::LinkCard { url, children }`] where `children` is the
    //   inner content parsed as blocks via [`super::parser::parse`].
    //
    // - A plain markdown cell. Parse via [`super::parser::parse`] (which
    //   re-runs extract_shortcodes so any nested `::::buttons` etc. get
    //   substituted) and drop the wrapping `Document`.
    let mut fragment = Diagnostics::default();
    let cells: Vec<Vec<Block>> = raw_cells
        .iter()
        .map(|(raw, body_line)| {
            let (blocks, metas, diag) =
                parse_cell_to_blocks(raw, &config_at_body_line(config, *body_line));
            fragment.warnings.extend(diag.warnings);
            fragment.advisories.extend(diag.advisories);
            fragments.push(source_fragment(config, *body_line, raw, metas));
            blocks
        })
        .collect();
    if used_cols_alias {
        fragment.warnings.push(cols_alias_warning("grid"));
    }
    if columns > MAX_GRID_COLUMNS {
        fragment.warnings.push(format!(
            "shortcode `:::grid` asks for {columns} columns, but the layout supports up to {MAX_GRID_COLUMNS}; this grid uses {MAX_GRID_COLUMNS}."
        ));
        columns = MAX_GRID_COLUMNS;
    }

    (
        GridShortcode {
            columns,
            ratio,
            classes,
            cells,
            source: ShortcodeSource { extra_lines: 0, fragments },
            width,
            scroll,
            collage,
            label,
        },
        legacy_dash_line,
        fragment,
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
/// Returns the parsed blocks, their positions, and the warnings and advisories
/// the fragment parses raised (e.g. a misspelled `:::name` nested in the cell),
/// which [`parse_shortcode_block`] merges into `doc.warnings`.
fn parse_cell_to_blocks(raw: &str, config: &ParseConfig) -> (Vec<Block>, Vec<BlockMeta>, Diagnostics) {
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
            return fragment_parts(parse_fragment_with_config(&linkified, config));
        }
        // A card's inner text is cut out of the cell, so its lines are not
        // tracked; the cell's own range still reaches the card.
        let inner_doc = parse_fragment_with_config(inner_trimmed, config);
        let mut diag = Diagnostics { warnings: inner_doc.warnings, advisories: inner_doc.advisories };
        let mut blocks = vec![Block::LinkCard {
            url: Url::unresolved(url),
            children: inner_doc.blocks,
        }];
        if !trailing_trimmed.is_empty() {
            let trailing_doc = parse_fragment_with_config(trailing_trimmed, config);
            blocks.extend(trailing_doc.blocks);
            diag.warnings.extend(trailing_doc.warnings);
            diag.advisories.extend(trailing_doc.advisories);
        }
        let metas = vec![BlockMeta::default(); blocks.len()];
        return (blocks, metas, diag);
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
        return fragment_parts(parse_fragment_with_config(&linkified, config));
    }
    fragment_parts(parse_fragment_with_config(raw, config))
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
/// NOT the general cure for `[![[x.png]]](/url)` — [`super::linked_embed`]
/// is; this is the block-level grid *card*.
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
///   the image-card-plus-caption cell. A cell led by
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
/// A migration step rewrites `---` to `+++` in moss-releases content;
/// after that, this helper retires in favor of `split_cells`.
///
/// **A divider only counts when it belongs to THIS grid.** A `+++` inside a
/// nested block, a code fence or an HTML comment is somebody else's. It used
/// to split this grid anyway, so `::::grid` around `:::grid` — the nesting
/// the grammar recommends — handed the inner grid's cells to the outer one,
/// with no warning and plausible-looking output. Nested bodies pass through
/// verbatim and are re-extracted when the cell is parsed, which is where the
/// inner block gets its own dividers. Depth is a plain counter: an opener
/// adds one, a bare colon line takes one off, colon counts are ignored (the
/// same rule as [`find_block_end`]).
///
/// Returns `(cells, legacy_dash_line)` where `legacy_dash_line` is the
/// 0-based body line of the first `---` divider **of this grid**, if any,
/// signaling the caller to emit a deprecation warning. A `---` belonging to
/// a nested block is reported when that block is parsed, not here.
fn split_grid_cells(body: &str) -> (Vec<(String, usize)>, Option<usize>) {
    if body.is_empty() {
        return (vec![(String::new(), 0)], None);
    }
    // One flag per line, aligned with `str::lines`, which agrees index-for-
    // index with `split_inclusive('\n')` for every input.
    let inert = crate::inert_regions::inert_lines(body);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut first_line_in_cell = true;
    let mut legacy_dash_line = None;
    // Body line the current cell's text starts on.
    let mut cell_start = 0;
    // Blocks we are currently inside; 0 means "at this grid's own depth",
    // which is the only place a divider counts.
    let mut depth: usize = 0;

    for (idx, line) in body.split_inclusive('\n').enumerate() {
        let content_no_eol = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = content_no_eol.trim();
        let live = !inert.get(idx).copied().unwrap_or(false);
        if live {
            match fence_step(trimmed) {
                1 => depth += 1,
                -1 if depth > 0 => depth -= 1,
                _ if depth == 0 && (trimmed == "+++" || trimmed == "---") => {
                    if trimmed == "---" && legacy_dash_line.is_none() {
                        legacy_dash_line = Some(idx);
                    }
                    if let Some(stripped) = current.strip_suffix('\n') {
                        current.truncate(stripped.len());
                    }
                    cells.push((std::mem::take(&mut current), cell_start));
                    cell_start = idx + 1;
                    first_line_in_cell = true;
                    continue;
                }
                _ => {}
            }
        }
        if first_line_in_cell {
            first_line_in_cell = false;
            if trimmed.is_empty() {
                cell_start = idx + 1;
                continue;
            }
        }
        current.push_str(line);
    }
    if let Some(stripped) = current.strip_suffix('\n') {
        current.truncate(stripped.len());
    }
    cells.push((current, cell_start));
    (cells, legacy_dash_line)
}

/// Byte-offset asset paths inside `:::gallery` / `:::hero` blocks.
///
/// The structural half of rename tracking: these paths carry NO markdown
/// reference syntax, so [`crate::resolve::md_extract::extract_md_references`]
/// cannot see them and a rename would silently break them. Offsets are absolute in
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
    structural_asset_spans(source, StructuralAssetMode::RenameSuperset)
}

/// Byte-offset media paths which the shortcode parser actually renders.
///
/// Unlike [`shortcode_asset_spans`], this follows the parser's ownership of
/// nested blocks: CSS and unknown wrappers recurse, grids recurse into their
/// parsed bodies, and a recognized hero or gallery owns its body. It is for
/// source evidence, where reporting a path from an unrendered unknown shape
/// would create a false blocking error; rename tracking deliberately has the
/// broader contract above.
pub fn authored_asset_spans(source: &str) -> Vec<AssetPathSpan> {
    structural_asset_spans(source, StructuralAssetMode::Rendered)
}

#[derive(Clone, Copy)]
enum StructuralAssetMode {
    RenameSuperset,
    Rendered,
}

fn structural_asset_spans(source: &str, mode: StructuralAssetMode) -> Vec<AssetPathSpan> {
    let mask = crate::inert_regions::mask_inert(source);
    // Line table over the RAW source. Index-aligned with `str::lines()`, but
    // additionally carrying each line's absolute base and terminator length
    // so `outer` can cover the whole physical line.
    let table = line_table(source);
    let mask_lines: Vec<&str> = mask.lines().collect();
    let mut out = Vec::new();
    collect_structural_asset_spans(
        source, &mask, &table, &mask_lines, 0, table.len(), mode, &mut out,
    );
    out.sort_by_key(|span| span.value.start);
    out
}

fn collect_structural_asset_spans(
    source: &str,
    mask: &str,
    table: &[(usize, usize, usize)],
    mask_lines: &[&str],
    start: usize,
    end: usize,
    mode: StructuralAssetMode,
    out: &mut Vec<AssetPathSpan>,
) {
    let mut i = start;
    while i < end {
        let Some(block) = structural_block_at(mask_lines, i, end) else {
            i += 1;
            continue;
        };
        let Some(close) = block.close else {
            // Matches extract_with_state: an unclosed opener is literal and
            // scanning resumes on its next physical line.
            i += 1;
            continue;
        };
        match block.name {
            "gallery" => {
                let lines = (block.body_start..close).map(|k| gallery_line_text(source, table, k));
                let kinds = gallery_lines(lines, &ParseConfig::default());
                for (k, kind) in (block.body_start..close).zip(kinds) {
                    if let Some(GalleryLine::Item(it, _)) = kind {
                        if let Some(span) = gallery_body_span(table, k, it) {
                            out.push(span);
                        }
                    }
                }
            }
            "hero" => super::extract_hero::hero_asset_spans(
                source, mask, table, i, block.body_start, close, out,
            ),
            // These are the only containers whose parser re-parses body
            // markdown. Other typed shortcodes own their body as data.
            "grid" | "" if matches!(mode, StructuralAssetMode::Rendered) => {
                collect_structural_asset_spans(
                    source, mask, table, mask_lines, block.body_start, close, mode, out,
                )
            }
            name if !is_typed_known(name) && matches!(mode, StructuralAssetMode::Rendered) => {
                collect_structural_asset_spans(
                    source, mask, table, mask_lines, block.body_start, close, mode, out,
                )
            }
            _ if matches!(mode, StructuralAssetMode::RenameSuperset) => {
                // Rename follows every physical line after a non-media block,
                // deliberately finding paths the renderer will not own.
                i += 1;
                continue;
            }
            _ => {}
        }
        i = close + 1;
    }
}

struct StructuralBlock<'a> {
    name: &'a str,
    body_start: usize,
    close: Option<usize>,
}

/// Parse one block boundary using the same multi-line-attribute and
/// closer rules as `extract_with_state`.
fn structural_block_at<'a>(
    mask_lines: &'a [&str],
    opener: usize,
    end: usize,
) -> Option<StructuralBlock<'a>> {
    let line = mask_lines.get(opener)?;
    let (name, args) = parse_shortcode_opener(line.trim())?;
    let (_, attrs_lines) = gather_multi_line_attrs(args, &mask_lines[opener + 1..end]);
    let body_start = opener + 1 + attrs_lines;
    if body_start > end {
        return Some(StructuralBlock { name, body_start, close: None });
    }
    let close = find_block_end(&mask_lines[..end], body_start, |_| false);
    Some(StructuralBlock { name, body_start, close })
}

/// The text of physical line `k`, without its terminator.
fn gallery_line_text<'a>(source: &'a str, table: &[(usize, usize, usize)], k: usize) -> &'a str {
    let Some(&(base, content_len, _)) = table.get(k) else { return "" };
    #[allow(clippy::string_slice)]
    // `base` and `base + content_len` are line boundaries from `line_table`,
    // which splits on ASCII '\n'/'\r' only.
    &source[base..base + content_len]
}

/// One `:::gallery` item line (`it`, read from physical line `k`) → an
/// [`AssetPathSpan`], or `None`.
///
/// The **emission filter** lives here, not in [`gallery_item_span`]: a
/// gallery body line is only reported as an asset reference when it carried
/// markdown reference syntax or its bare path has a known media extension
/// ([`HERO_MEDIA_EXTENSIONS`], the SSOT — extending that list extends gallery
/// rename tracking). Without it, every line of prose in a gallery body would
/// show up in the delete-confirmation modal.
fn gallery_body_span(
    table: &[(usize, usize, usize)],
    k: usize,
    it: MediaLineSpan,
) -> Option<AssetPathSpan> {
    let (base, content_len, term_len) = *table.get(k)?;
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
/// This is the single grammar for one gallery item line: [`gallery_lines`]
/// reads every body line through it (adding the caption rule), and
/// [`shortcode_asset_spans`] lifts its offsets to absolute. The three arms and
/// their ORDER mirror the historical parser exactly — `![[…]]` first, then the
/// pipe split, then the markdown-image pattern — because that ordering is
/// observable (`![alt](x.png|cover)` splits on the pipe first and therefore is
/// NOT a markdown image).
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
    // A whole image is read first so a pipe inside its alt text (`![a|b](x)`)
    // is not mistaken for the `|attrs` separator; any other line splits on
    // its first pipe.
    let whole_image = extract_md_references(trimmed).into_iter().next().and_then(|r| {
        if r.byte_from != 0 || !matches!(r.syntax, RefSyntax::MarkdownImage { .. }) {
            return None;
        }
        let tail = trimmed.get(r.byte_to..)?.trim_start();
        let attrs = if tail.is_empty() { "" } else { tail.strip_prefix('|')?.trim() };
        Some((trimmed.get(..r.byte_to)?, attrs))
    });
    let (src_raw, attrs) = whole_image.unwrap_or_else(|| split_pipe(trimmed));
    match parse_markdown_image(src_raw) {
        Some((alt, path, dest_start)) => {
            // The rename span is the destination inside the parens only. A
            // `|attrs` suffix here sits AFTER the closing paren, outside the
            // span, so `value_attrs` is empty and the suffix is left alone.
            let s2_lead = lead + (src_raw.len() - src_raw.trim_start().len());
            let path_start = s2_lead + dest_start;
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

/// The inside of a line that is one `*italic*` span and nothing else, parsed as
/// inline markdown: the caption rule a figure in running text follows.
fn italic_line(line: &str, config: &ParseConfig) -> Option<Vec<Inline>> {
    match super::extract_hero::parse_caption_inlines(line.trim(), config).as_mut_slice() {
        [Inline::Emphasis(inner)] if !inner.is_empty() => Some(std::mem::take(inner)),
        _ => None,
    }
}

/// What a `:::gallery` body line is, once read in order.
enum GalleryLine {
    Item(MediaLineSpan, Option<String>),
    /// The italic line directly under an item.
    Caption(Vec<Inline>),
}

/// Read the body lines of a gallery in order: the one place that decides
/// whether a line is an item or the caption of the item above it, shared by
/// [`parse_gallery_body`] and the asset-span enumerator. Blank lines are `None`.
fn gallery_lines<'a>(lines: impl Iterator<Item = &'a str>, config: &ParseConfig) -> Vec<Option<GalleryLine>> {
    let mut after_item = false;
    lines
        .map(|line| {
            if after_item {
                if let Some(caption) = italic_line(line, config) {
                    after_item = false;
                    return Some(GalleryLine::Caption(caption));
                }
            }
            let item = gallery_item_span(line);
            after_item = item.is_some();
            item.map(|it| GalleryLine::Item(it, markdown_image_title(line.trim())))
        })
        .collect()
}

/// Parse a `:::gallery` block.
///
/// Args: `N {.classes width per-line=N}` where `N` is optional column
/// count and `width` is one of the spec § P9 width tokens (handled inside
/// [`split_positional_classes_width_and_per_line`]). `per-line=` wins over
/// a positional `N` when both are present; `cols=` is a deprecated alias
/// for `per-line=` — see [`per_line_attr`].
///
/// Returns the shortcode plus any deprecation warnings (currently just the
/// `cols=` alias warning, when that key was written).
fn parse_gallery_body(args: &str, body: &str, config: &ParseConfig) -> (GalleryShortcode, Vec<String>) {
    let attrs = split_positional_classes_width_and_per_line(args);
    let mut warnings = Vec::new();
    // `rows` is a layout word, not a count: `:::gallery rows` is justified rows.
    let (rows, count_text) = match attrs.positional.strip_prefix("rows") {
        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
            (true, rest.trim())
        }
        _ => (false, attrs.positional.as_str()),
    };
    if rows && (attrs.per_line.is_some() || !count_text.is_empty()) {
        warnings.push(
            "`:::gallery rows` lays out justified rows, so the column count is ignored"
                .to_string(),
        );
    }
    let count = attrs
        .per_line
        .as_deref()
        .or(if count_text.is_empty() { None } else { Some(count_text) })
        .and_then(|v| v.parse::<u32>().ok());
    let columns = if rows { None } else { count };
    let mut items: Vec<GalleryItem> = Vec::new();
    for line in gallery_lines(body.lines(), config).into_iter().flatten() {
        match line {
            GalleryLine::Caption(caption) => {
                if let Some(item) = items.last_mut() {
                    item.caption = caption;
                }
            }
            GalleryLine::Item(it, title) => {
                let (alt, alt_only) = match super::parser::alt_only_text(&it.alt) {
                    Some(alt) => (alt, true),
                    None => (it.alt, false),
                };
                items.push(GalleryItem {
                    src: Url::unresolved(it.path),
                    alt,
                    alt_only,
                    attrs: it.attrs,
                    caption: Vec::new(),
                    title,
                });
            }
        }
    }
    if attrs.used_cols_alias {
        warnings.push(cols_alias_warning("gallery"));
    }
    (
        GalleryShortcode {
            columns,
            classes: attrs.classes,
            items,
            width: attrs.width,
            rows,
        },
        warnings,
    )
}

/// [`split_positional_classes_width_and_per_line`]'s result.
struct GalleryPositionalAttrs {
    positional: String,
    classes: String,
    width: Option<String>,
    /// Resolved `per-line=` value, with `cols=` as a fallback — see
    /// [`per_line_attr`].
    per_line: Option<String>,
    /// Whether `cols=` was written (whichever value won), so the caller
    /// knows to emit the deprecation warning.
    used_cols_alias: bool,
}

/// Split `args` into positional text, classes, the spec § P9 width token,
/// and the resolved `per-line=`/`cols=` column count.
///
/// Same routing as [`split_positional_and_classes`]. `width` and `per_line`
/// are `None` when the author did not set them, or when the legacy
/// fallback path fires (malformed attrs where the structured parser
/// bailed) — that path only recovers `.class` tokens, on the same
/// reasoning the width token already skipped: malformed enough to bail
/// means the author's intent is unclear, and omitting is safer than
/// guessing.
fn split_positional_classes_width_and_per_line(args: &str) -> GalleryPositionalAttrs {
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
                let (per_line, used_cols_alias) = per_line_attr(&parsed);
                return GalleryPositionalAttrs {
                    positional,
                    classes: parsed.class_string(),
                    width: parsed.width.map(str::to_string),
                    per_line: per_line.map(str::to_string),
                    used_cols_alias,
                };
            }
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
            return GalleryPositionalAttrs {
                positional,
                classes: classes.join(" "),
                width: None,
                per_line: None,
                used_cols_alias: false,
            };
        }
    }
    GalleryPositionalAttrs {
        positional: trimmed.to_string(),
        classes: String::new(),
        width: None,
        per_line: None,
        used_cols_alias: false,
    }
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

/// Parse `![alt](path)` into `(alt, path, offset of path in the trimmed
/// input)`. Returns `None` if not a markdown image. The destination is read
/// by the same scanner every other reference goes through, so balanced
/// parentheses and `<…>` are as welcome here as in a page body.
fn parse_markdown_image(s: &str) -> Option<(String, String, usize)> {
    let s = s.trim();
    let r = extract_md_references(s).into_iter().next()?;
    match r.syntax {
        RefSyntax::MarkdownImage { alt } if r.byte_from == 0 && r.byte_to == s.len() => {
            Some((alt, r.text, r.ref_from))
        }
        _ => None,
    }
}

/// The CommonMark title of a line that starts with a markdown image, if any.
fn markdown_image_title(line: &str) -> Option<String> {
    use pulldown_cmark::{Event, Parser, Tag};
    Parser::new(line).find_map(|e| {
        if let Event::Start(Tag::Image { title, .. }) = e {
            Some(title.to_string()).filter(|t| !t.is_empty())
        } else {
            None
        }
    })
}

fn parse_buttons_body(args: &str, body: &str) -> (ButtonsShortcode, Vec<String>) {
    let (_positional, classes) = split_positional_and_classes(args);
    let mut items: Vec<ButtonItem> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
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
            // A trailing `{...}` is an attribute block. Buttons take none (the
            // first is primary, the rest secondary), but the link is still the
            // author's button: keep it and say the attributes are dropped.
            let (link, dropped_attrs) = match trimmed.strip_suffix('}').and_then(|r| r.rsplit_once('{')) {
                Some((head, _)) if head.trim_end().ends_with(')') => (head.trim_end(), true),
                _ => (trimmed, false),
            };
            match extract_markdown_link(link) {
                Some((text, url)) => {
                    if url.contains("](") || url.contains(") [") {
                        warnings.push(format!(
                            "shortcode `:::buttons` takes one link per line; `{}` has more than one, so its address is wrong",
                            clip_for_warning(trimmed)
                        ));
                    }
                    if dropped_attrs {
                        warnings.push(format!(
                            "shortcode `:::buttons` ignores the attribute block on `{}`: \
                             the first button is primary and the rest are secondary. The button is kept.",
                            clip_for_warning(trimmed)
                        ));
                    }
                    items.push(ButtonItem { text, url: Url::unresolved(url) });
                }
                None => warnings.push(format!(
                    "shortcode `:::buttons` expects one `[text](url)` link per line; `{}` is not one and is left out",
                    clip_for_warning(trimmed)
                )),
            }
        }
    }
    (ButtonsShortcode { classes, items }, warnings)
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
    let mut collected = Collected::default();
    let output = extract_with_state(markdown, &nonce, &mut collected, config);
    ExtractionResult {
        markdown_with_placeholders: output,
        extracted: collected.extracted,
        nonce,
        warnings: collected.warnings,
        advisories: collected.advisories,
        css_regions: collected.css_regions,
    }
}

/// What one extraction pass accumulates across every nesting level. The
/// levels share one instance, so placeholder indices, the nonce-indexed
/// `extracted` list and every diagnostic come out flat.
#[derive(Default)]
struct Collected {
    extracted: Vec<ExtractedShortcode>,
    warnings: Vec<String>,
    advisories: Vec<String>,
    css_regions: Vec<CssRegionSpan>,
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
    collected: &mut Collected,
    config: &ParseConfig,
) -> String {
    let mut output = String::with_capacity(markdown.len());
    let lines: Vec<&str> = markdown.lines().collect();
    // Which lines are code or comment, and therefore carry no live `:::`
    // syntax. One shared scanner for every pre-parse pass in the tree —
    // this module used to track fenced code itself and knew nothing about
    // HTML comments, which is how a `:::gallery` inside an authored
    // `<!-- TODO … -->` block got extracted, spliced a sentinel into the
    // middle of the comment, and deleted the rest of the page.
    let inert = crate::inert_regions::inert_lines(markdown);
    let is_inert = |idx: usize| inert.get(idx).copied().unwrap_or(false);
    let mut i = 0;
    // (opener, closer) line indices of the last block closed at this level,
    // so a stray closer can point at the block it most likely doubles.
    let mut last_closed: Option<(usize, usize)> = None;

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
        if let Some((name, single_line_args)) = parse_shortcode_opener(trimmed) {
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

            // The block ends at the first bare colon line that no nested
            // opener claims (an inert line cannot close it either: a `:::`
            // inside a code fence or a comment used to end the shortcode
            // early and strand the rest as prose).
            let Some(j) = find_block_end(&lines, body_start, is_inert) else {
                // Unclosed block: emit verbatim, and say where it opened.
                collected.warnings.push(unclosed_opener_warning(
                    trimmed,
                    config.warning_line_offset + i + 1,
                ));
                output.push_str(line);
                output.push('\n');
                i += 1;
                continue;
            };
            last_closed = Some((i, j));
            let body_lines = lines.get(body_start..j).unwrap_or_default();

            let body = body_lines.join("\n");

            if let Some(w) = super::attrs::attr_block_warning(trimmed, args) {
                collected.warnings.push(w);
            }

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
                // common site pattern of `:::{.support-band}` around
                // `::::buttons`) also get extracted into sentinels.
                // The body is balanced (see `find_block_end`), so the
                // recursive call sees every inner block whole.
                let parsed = match super::attrs::parse_attrs(args) {
                    Ok(block) => block,
                    Err(_) => {
                        collected.advisories.push(format!(
                            "fenced div attributes `{}` could not be read; the div renders without its classes, id and width",
                            args.trim()
                        ));
                        Default::default()
                    }
                };
                collected.css_regions.push(CssRegionSpan {
                    classes: parsed.classes.clone(),
                    line: config.warning_line_offset + i + 1,
                });
                // The body's first line sits `body_start` lines into this
                // level, so the recursion's line numbers are shifted by it.
                let body_config = config_at_body_line(config, body_start);
                let body_processed =
                    extract_with_state(&body, nonce, collected, &body_config);
                output.push_str(&render_div_open(
                    &parsed.classes,
                    parsed.id.as_deref(),
                    parsed.width,
                    Some(&data_attrs_for(&parsed.kvs)),
                ));
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
                // The body's first line is `body_start` lines into this level.
                let body_config = config_at_body_line(config, body_start);
                if let (Some(mut sc), diag) = parse_shortcode_block(name, args, &body, &body_config) {
                    if let Some(source) = sc.source_mut() {
                        source.extra_lines = j - i;
                    }
                    collected.warnings.extend(diag.warnings);
                    collected.advisories.extend(diag.advisories);
                    if let Shortcode::Grid(g) = &sc {
                        if g.collage && !g.is_collage() {
                            collected.warnings.push(format!(
                                "`:::grid {{collage}}` on line {} has {} cells, but a collage needs {} to {}; it renders as a plain grid.",
                                config.warning_line_offset + i + 1,
                                g.cells.len(),
                                super::shortcode::COLLAGE_TILES.start(),
                                super::shortcode::COLLAGE_TILES.end(),
                            ));
                        }
                    }
                    let index = collected.extracted.len();
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
                    // unchanged.
                    for _ in 0..(j - i) {
                        output.push('\n');
                    }
                    collected.extracted.push(ExtractedShortcode {
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
            collected.warnings.push(format!("unknown shortcode `:::{}`", name));
            let mut classes = vec!["moss-unknown-shortcode".to_string()];
            classes.extend(parsed.classes.iter().cloned());
            let extra_attrs = format!(r#" data-name="{}""#, html_escape_attr(name));
            let body_config = config_at_body_line(config, body_start);
            let body_processed =
                extract_with_state(&body, nonce, collected, &body_config);
            output.push_str(&render_div_open(&classes, parsed.id.as_deref(), None, Some(&extra_attrs)));
            output.push_str("\n\n");
            output.push_str(&body_processed);
            if !body_processed.is_empty() && !body_processed.ends_with('\n') {
                output.push('\n');
            }
            output.push_str("\n</div>\n");
            i = j + 1;
            continue;
        }

        // Regular content line. A live bare colon line here closed nothing.
        // Inside a raw HTML block or display math it is more likely literal
        // text than a miscount, so it stays quiet there; a list-item
        // continuation is NOT exempt, because the extractor reads that line
        // like any other and an author who wrote it meant a closer.
        if fence_step(trimmed) < 0 {
            let quiet = crate::inert_regions::html_and_math_lines(markdown, &inert);
            if !quiet.get(i).copied().unwrap_or(false) {
                collected.warnings.push(stray_closer_warning(
                    trimmed,
                    config.warning_line_offset + i + 1,
                    last_closed.map(|(o, c)| {
                        (config.warning_line_offset + o + 1, config.warning_line_offset + c + 1)
                    }),
                ));
            }
        }
        output.push_str(line);
        output.push('\n');
        i += 1;
    }

    output
}

/// Word the warning for an opener no bare colon line ever closes.
fn unclosed_opener_warning(opener: &str, line: usize) -> String {
    format!(
        "`{}` on line {line} is never closed: a bare `:::` line closes the innermost \
         open block, and none follows, so the block is shown as plain text.",
        clip_for_warning(opener)
    )
}

/// Word the warning for a bare colon line with nothing left to close.
/// `last_closed` is the (opener line, closer line) of the block closed just
/// above, which an extra closer most often doubles.
fn stray_closer_warning(closer: &str, line: usize, last_closed: Option<(usize, usize)>) -> String {
    let hint = match last_closed {
        Some((open, close)) => {
            format!(" The last block closed on line {close} and started on line {open}.")
        }
        None => String::new(),
    };
    format!(
        "`{}` on line {line} closes nothing: every block opened above it is already closed.{hint}",
        clip_for_warning(closer)
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
/// `width` is a fenced div's width word (`body` emits nothing); `extra_attrs`
/// (already with leading space) is appended before `>`, used by the
/// unknown-name renderer to add `data-name="..."` and by the unnamed one
/// for the block's `key=value` pairs (`data_attrs_for`).
fn render_div_open(
    classes: &[String],
    id: Option<&str>,
    width: Option<&str>,
    extra_attrs: Option<&str>,
) -> String {
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
    if let Some(w) = width.filter(|w| *w != "body") {
        out.push_str(" data-width=\"");
        out.push_str(&html_escape_attr(w));
        out.push('"');
    }
    if let Some(extra) = extra_attrs {
        out.push_str(extra);
    }
    out.push('>');
    out
}

/// The `key=value` pairs of a fenced div's attribute block as HTML attributes,
/// each with a leading space, in source order; keys are named by
/// [`html_attribute_name`] (`fig=teapot` is `data-fig`, `title=Tea` stays;
/// `contents=end` is the flag `data-contents-end`).
fn data_attrs_for(kvs: &[(String, String)]) -> String {
    kvs.iter()
        .map(|(key, value)| {
            // `contents=end` marks where a page's main sections stop: a flag
            // attribute the contents ruler reads, not a `data-contents` value.
            if key == "contents" && value == "end" {
                " data-contents-end".to_string()
            } else {
                format!(" {}=\"{}\"", html_attribute_name(key), html_escape_attr(value))
            }
        })
        .collect()
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

/// Parse an opening fence line into (name, args). Returns `None` if the line
/// is not an opener.
///
/// Accepts any colon count >= 3 (`:::name`, `::::name`, `:::::name`, ...);
/// the count is not returned, because a bare colon line of any length closes
/// the block.
///
/// **Pure-CSS region opener** — `:::{.class}` (no name, attrs only) is
/// also recognized. The returned `name` is empty, signaling the caller
/// to render the block as a plain styling wrapper. Empty name without
/// a following `{` is rejected (just colons followed by content is not
/// an opener).
pub(crate) fn parse_shortcode_opener(trimmed: &str) -> Option<(&str, &str)> {
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
        return Some(("", rest.trim()));
    }
    // char-aligned: name_end is a byte index returned by str::find with a
    // char predicate, which is guaranteed to be a char boundary (or rest.len()).
    #[allow(clippy::string_slice)]
    let name = &rest[..name_end];
    #[allow(clippy::string_slice)]
    let args = rest[name_end..].trim();
    Some((name, args))
}

/// True if `trimmed` is a closing fence: three or more colons and nothing
/// else. The count does not matter; a closer closes the innermost open block.
///
/// A line like `::: extra` is NOT a closer (it's body content). Iterates
/// chars, never byte offsets, because the line may begin with multi-byte
/// UTF-8 (a byte-indexed split panicked mid-character on Chinese button
/// text).
pub(crate) fn is_close_fence(trimmed: &str) -> bool {
    let mut colons = 0;
    let mut chars = trimmed.chars();
    for c in chars.by_ref() {
        if c != ':' {
            return colons >= 3 && c.is_whitespace() && chars.all(char::is_whitespace);
        }
        colons += 1;
    }
    colons >= 3
}

/// What one live line does to block depth: `1` for an opener, `-1` for a
/// closer, `0` for anything else. The single place that decides which of the
/// two wins, shared by every walk that tracks nesting.
pub(crate) fn fence_step(trimmed: &str) -> i8 {
    if parse_shortcode_opener(trimmed).is_some() {
        1
    } else if is_close_fence(trimmed) {
        -1
    } else {
        0
    }
}

/// Index of the line that closes a block whose body starts at `from`, or
/// `None` when nothing closes it.
///
/// Pandoc's rule: an opener (a name or `{attributes}` after the colons) opens
/// a nested block, and a bare colon line closes the innermost open one. The
/// colon count is only for the eye. Every caller that asks "where does this
/// block end" goes through here, so the build, the grid splitter and the
/// rename scanner cannot disagree. A body between opener and closer is
/// balanced, which is what lets bodies be re-scanned on their own.
pub(crate) fn find_block_end(
    lines: &[&str],
    from: usize,
    is_inert: impl Fn(usize) -> bool,
) -> Option<usize> {
    let mut depth = 0usize;
    for (j, line) in lines.iter().enumerate().skip(from) {
        if is_inert(j) {
            continue;
        }
        match fence_step(line.trim()) {
            1 => depth += 1,
            -1 if depth == 0 => return Some(j),
            -1 => depth -= 1,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
#[path = "shortcode_extract_tests.rs"]
mod tests;
