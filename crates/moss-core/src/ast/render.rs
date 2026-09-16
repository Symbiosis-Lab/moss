//! Render typed AST → HTML via [`RenderHooks`].
//!
//! Walks every variant; calls hooks at interceptable points. Debug-asserts
//! on `Url::Unresolved` reaching the renderer — a missing visitor is a bug.
//!
//! # Phase 4: render_document IS the production rendering path (target)
//!
//! Today (2026-05-27) this function runs as a parallel observer via
//! `observe_typed_ast` in `src-tauri/src/build/markdown/pipeline.rs`;
//! production HTML still comes from `pulldown_cmark::html::push_html` over
//! the event stream. Phase 4 PR7a flips this: `render_document` becomes
//! the production renderer, `html::push_html` is no longer called in the
//! main pipeline, and `transform_events` is reduced to a thin
//! events-to-Document adapter (or deleted).
//!
//! # Why the AST renders (not pulldown-cmark)
//!
//! Cross-SSG research (2026-05-27) — see
//! [docs/archive/2026-05-27-typed-ast-cross-ssg-research.md](../../../../docs/archive/2026-05-27-typed-ast-cross-ssg-research.md)
//! — confirms every AST-bearing SSG with secondary consumers (link
//! graphs, editors, validators, multi-target rendering) puts the AST at
//! the rendering source:
//!
//! - **mdBook** (same parser as moss) recently migrated from
//!   `html::push_html` to a typed `Tree<Node>` via `ego_tree`. Same
//!   destination, same motivation.
//! - **Hugo** dispatches NodeRenderer per AST node-kind; render hooks
//!   fire during AST walk.
//! - **Markdoc** ships `AstNode → RenderableTreeNode → HTML/React`.
//! - **Pandoc** has been AST-first since 2006; output is a writer per
//!   target format.
//! - **Quarto 2** is mid-migration from Stage 1 pre-parsers to AST-first
//!   for three reasons: performance, fragility, information loss.
//!
//! Streaming-only SSGs (Zola, markdown-it ecosystem) live without an AST,
//! but pay the cost: structural reshape requires fragile token-window
//! pattern matching; secondary consumers can't ride on event streams.
//! moss has secondary consumers (#599 page threading, editor's
//! `scan_shortcodes`, `has_shortcode_recursive`, future WASM editor,
//! future LSP-style diagnostics) — AST is non-optional.
//!
//! See [docs/reference/typed-body-ast.md](../../../../docs/reference/typed-body-ast.md)
//! for the design intent + 7 principles, and
//! [docs/archive/2026-05-27-phase4-typed-ast-completion.md](../../../../docs/archive/2026-05-27-phase4-typed-ast-completion.md)
//! for the Phase 4 execution plan.

use super::document::{BlockMeta, Document};
use super::footnotes::{self, FootnoteCtx};
use super::hooks::{escape_attr, escape_text, RenderHooks};
use super::node::{Block, ColumnAlignment, Fold, Inline};
use super::url::Url;

/// Resolved (post-detection) per-column alignment used only for table HTML
/// emission. Distinct from [`ColumnAlignment`] (the source-faithful AST value):
/// this folds in numeric auto-detection and never carries a `None`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CellAlign {
    Left,
    Center,
    Right,
}

impl CellAlign {
    /// `class="…"` attribute fragment (with a leading space) for a cell of this
    /// alignment; empty for the default left alignment so unaligned tables emit
    /// exactly `<td>`.
    fn class_attr(self) -> &'static str {
        match self {
            CellAlign::Left => "",
            CellAlign::Center => " class=\"moss-col-center\"",
            CellAlign::Right => " class=\"moss-col-right\"",
        }
    }
}

/// Whether a single cell reads as a number for right-alignment: an optional
/// sign and currency mark, an ASCII digit run (with `,`/`.` group/decimal
/// separators), an optional percent, and an optional short unit tail (≤3
/// non-digit chars, e.g. `天`, `人`, `%`). Deliberately conservative so
/// CJK-mixed labels like `第1名` and ranges like `2020-2021` stay left.
fn cell_reads_as_number(s: &str) -> bool {
    let chars: Vec<char> = s.trim().chars().collect();
    if chars.is_empty() {
        return false;
    }
    let mut i = 0;
    if matches!(chars.get(i), Some('+' | '-')) {
        i += 1;
    }
    if matches!(chars.get(i), Some('¥' | '$' | '€' | '£')) {
        i += 1;
    }
    let mut saw_digit = false;
    while let Some(&c) = chars.get(i) {
        if c.is_ascii_digit() {
            saw_digit = true;
            i += 1;
        } else if c == ',' || c == '.' {
            i += 1;
        } else {
            break;
        }
    }
    if !saw_digit {
        return false;
    }
    if matches!(chars.get(i), Some('%' | '‰')) {
        i += 1;
    }
    let tail: String = chars[i..].iter().collect();
    let tail = tail.trim();
    tail.is_empty() || (tail.chars().count() <= 3 && !tail.chars().any(|c| c.is_ascii_digit()))
}

/// Whether body-column `col` is numeric: ≥1 non-empty cell and ≥80% of the
/// non-empty cells read as numbers. The header is intentionally excluded (a
/// numeric header label like `2024` should not flip an otherwise-text column).
fn column_reads_as_numeric(rows: &[Vec<Vec<Inline>>], col: usize) -> bool {
    let mut non_empty = 0usize;
    let mut numeric = 0usize;
    for row in rows {
        let Some(cell) = row.get(col) else { continue };
        let text = crate::ast::plain_text::inlines_to_plain_text(cell);
        if text.trim().is_empty() {
            continue;
        }
        non_empty += 1;
        if cell_reads_as_number(&text) {
            numeric += 1;
        }
    }
    non_empty > 0 && numeric * 5 >= non_empty * 4
}

/// Resolve effective per-column alignment. Author GFM alignment always wins;
/// an unaligned column right-aligns iff its body reads as numeric. Length
/// equals `header.len()`; cells beyond it fall back to left in emission.
fn resolve_column_alignment(
    header: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    alignments: &[ColumnAlignment],
) -> Vec<CellAlign> {
    (0..header.len())
        .map(
            |col| match alignments.get(col).copied().unwrap_or(ColumnAlignment::None) {
                ColumnAlignment::Left => CellAlign::Left,
                ColumnAlignment::Center => CellAlign::Center,
                ColumnAlignment::Right => CellAlign::Right,
                ColumnAlignment::None => {
                    if column_reads_as_numeric(rows, col) {
                        CellAlign::Right
                    } else {
                        CellAlign::Left
                    }
                }
            },
        )
        .collect()
}

/// Render a [`Document`] to an HTML string using the given hooks.
///
/// # Panics (debug only)
///
/// If any URL is still `Url::Unresolved` when the renderer reaches it.
/// `visit_urls_mut` must run before this function. In release builds the
/// raw unresolved string is emitted as-is to avoid crashing on a bug.
pub fn render_document<H: RenderHooks>(doc: &Document, hooks: &H) -> String {
    let mut out = String::new();
    // Walk blocks + meta in lockstep. Invariant: block_meta.len() ==
    // blocks.len() (asserted in debug, defensive in release).
    debug_assert_eq!(
        doc.blocks.len(),
        doc.block_meta.len(),
        "Document invariant: blocks.len() == block_meta.len()"
    );
    let mut fnotes = FootnoteCtx::for_document(&doc.blocks);
    for (i, block) in doc.blocks.iter().enumerate() {
        let meta = doc.block_meta.get(i).copied().unwrap_or_default();
        render_block(hooks, &mut out, block, &meta, &mut fnotes);
    }
    footnotes::render_section(hooks, &mut out, &doc.blocks, &mut fnotes);
    out
}

/// Render ONE top-level block with its [`BlockMeta`] — the body of
/// [`render_document`]'s loop, exposed.
///
/// A host that serializes a document one top-level block at a time (so it can
/// record where each block's output begins and ends, instead of scanning the
/// finished string for structure later — see src-tauri's `BodyPlan` and
/// ADR-034) must go through this rather than [`render_blocks`]: the latter has
/// no meta vec and would silently drop every `data-source-line` annotation.
///
/// Takes the caller's [`FootnoteCtx`] (built once via `FootnoteCtx::
/// for_document`, same as [`render_document`]) rather than owning one, so
/// footnote numbering/hoisting stays document-wide even when the caller
/// walks blocks one at a time instead of via `render_document`'s loop.
///
/// Concatenating this over `doc.blocks`/`doc.block_meta` in lockstep, sharing
/// one `FootnoteCtx` across the walk, is byte-identical to [`render_document`]
/// by construction.
pub fn render_block_with_meta<H: RenderHooks + ?Sized>(
    hooks: &H,
    out: &mut String,
    block: &Block,
    meta: &BlockMeta,
    fnotes: &mut FootnoteCtx,
) {
    render_block(hooks, out, block, meta, fnotes);
}

/// Render a sequence of blocks to HTML. Used by shortcode-body renderers —
/// grid cells and, via src-tauri's `render_hero_html_typed` (Phase 4 PR4.5),
/// the hero overlay — to render a `Vec<Block>` that didn't come from a full
/// `Document`. [`render_document`] does NOT call this: it walks its blocks
/// and meta in lockstep, calling `render_block` directly.
///
/// **Not table cells.** A `Block::Table` holds `Vec<Vec<Vec<Inline>>>` — its
/// cells are inlines, rendered by `render_inlines` inside the table arm, so
/// they never reach any block-level entry point. Listing them here (and in
/// ADR-035) described a caller that cannot exist.
///
/// **Source-line caveat:** this entry point has no per-block meta vec, so
/// every block renders without `data-source-line`. Callers that need
/// source-line annotations must walk meta-block pairs themselves (see
/// [`render_document`]). Today only [`render_document`] consumes meta;
/// nested-block walks (list items, callout bodies, blockquotes) are
/// also meta-free — `data-source-line` is a top-level-block-only
/// concern, matching the legacy `transform_events` emit shape.
///
/// `H: ?Sized` so the function can be called with `&dyn RenderHooks` or
/// with `self: &Self` from inside a trait default method (where `Self`
/// is not statically `Sized`). The hook surface is a thin dispatch
/// boundary; monomorphization across all concrete impls is not required.
/// **Footnote caveat:** this entry point carries no [`FootnoteIndex`], so a
/// marker inside `blocks` renders as its literal `[^label]` source and a
/// definition renders in place. That is the honest shape for the callers
/// this function has — shortcode bodies, which are their own little
/// documents with no endnote section of their own. Nested walks INSIDE a
/// document use `render_blocks_with`. ADR-035 § Three call paths.
pub fn render_blocks<H: RenderHooks + ?Sized>(hooks: &H, out: &mut String, blocks: &[Block]) {
    render_blocks_with(hooks, out, blocks, &mut FootnoteCtx::default());
}

/// The in-document nested-block walk: same as [`render_blocks`] but keeps
/// the caller's footnote state, so a marker inside a blockquote, callout,
/// multi-paragraph list item or link card is numbered like any other.
pub(super) fn render_blocks_with<H: RenderHooks + ?Sized>(
    hooks: &H,
    out: &mut String,
    blocks: &[Block],
    fnotes: &mut FootnoteCtx,
) {
    for block in blocks {
        // Nested blocks render without source-line annotations (the
        // legacy transform_events emitted `data-source-line` on the
        // outer `<ul>`/`<ol>`/`<blockquote>` and inner `<li>` only —
        // top-level + list-item depth. We omit the `<li>` annotation
        // for now; the iframe-bridge consumer picks the outer wrapper
        // when no inner annotation exists.
        render_block(hooks, out, block, &BlockMeta::default(), fnotes);
    }
}

fn render_block<H: RenderHooks + ?Sized>(
    hooks: &H,
    out: &mut String,
    block: &Block,
    meta: &BlockMeta,
    fnotes: &mut FootnoteCtx,
) {
    match block {
        Block::Heading {
            level,
            children,
            id,
        } => {
            let mut content = String::new();
            render_inlines(hooks, &mut content, children, fnotes);
            hooks.render_heading(out, *level, id.as_deref(), meta.source_line, &content);
            out.push('\n');
        }
        Block::Paragraph(children) => {
            out.push_str("<p");
            push_source_line_attr(out, meta.source_line);
            out.push('>');
            render_inlines(hooks, out, children, fnotes);
            out.push_str("</p>\n");
        }
        Block::Callout {
            kind,
            fold,
            title,
            children,
        } => {
            // Phase 4 PR4: byte-shape mirrors the (now-deleted) Stage 1
            // `resolve/callouts.rs` output that production HTML still
            // assumes — `<div class="callout" data-type="{slug}"> /
            //   <div class="callout-title">{title}</div> /
            //   <div class="callout-content">…</div>
            // </div>`. The `data-fold` attribute is new in PR4 (Obsidian
            // foldable callouts); absent on non-foldable callouts so
            // existing fixtures remain byte-identical.
            //
            // `data-source-line` injected when meta carries it; matches the
            // legacy `transform_events` shape on the blockquote-promoted
            // callout (the legacy emit was for `<blockquote>` since
            // callouts hadn't moved to a typed `<div>` shape yet at the
            // time; downstream consumer (iframe-bridge) accepts the attr
            // on any wrapper element).
            out.push_str(r#"<div class="callout" data-type=""#);
            out.push_str(kind.as_slug());
            out.push_str(r#"""#);
            push_source_line_attr(out, meta.source_line);
            if let Some(fold_state) = fold {
                let fold_attr = match fold_state {
                    Fold::Open => "open",
                    Fold::Closed => "closed",
                };
                out.push_str(r#" data-fold=""#);
                out.push_str(fold_attr);
                out.push_str(r#"""#);
            }
            out.push_str(">\n");
            // Title slot: prefer the parser-extracted title; fall back
            // to the kind's capitalized default (matches Stage 1).
            let display_title = title
                .as_deref()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(|t| escape_text(t))
                .unwrap_or_else(|| kind.default_title().to_string());
            out.push_str(r#"  <div class="callout-title">"#);
            out.push_str(&display_title);
            out.push_str("</div>\n");
            // The box itself always survives: its title is text the AUTHOR
            // typed, and hoisting a footnote definition moves the note — it
            // must not delete the prose around it. What goes is the empty
            // content div a hoist leaves behind.
            let body = render_children_to_string(hooks, children, fnotes);
            if !hoist_emptied(children, &body) {
                out.push_str(r#"  <div class="callout-content">"#);
                out.push('\n');
                out.push_str(&body);
                out.push_str("</div>\n");
            }
            out.push_str("</div>\n");
        }
        Block::List {
            ordered,
            start,
            items,
            item_source_lines,
        } => {
            // Parallel-vec invariant: when the parser populated per-item
            // source lines, the vector must align 1:1 with `items` so the
            // `idx`-keyed lookup at `item_source_lines.get(idx)` is
            // well-defined. Empty (default) means "parser ran without
            // `emit_source_lines`" — that's the legitimate skip case.
            // Mirrors the document-level `blocks.len() == block_meta.len()`
            // invariant asserted at the top of `render_document`.
            debug_assert!(
                item_source_lines.is_empty() || item_source_lines.len() == items.len(),
                "Block::List invariant: item_source_lines.len() ({}) must equal items.len() ({}) when populated",
                item_source_lines.len(),
                items.len()
            );
            // Items render into a buffer first so an item the hoist emptied
            // can be dropped — and, if that was the only item, so can the
            // list. The render still happens exactly once, in order, so the
            // `FootnoteCtx` side effects (hoist decisions, marker ids,
            // back-link counts) are identical either way.
            let mut body = String::new();
            let mut kept = 0usize;
            for (idx, item_blocks) in items.iter().enumerate() {
                let mut item = String::new();
                // Single-paragraph items render their inline content inline
                // (no extra <p>). Mirrors pulldown-cmark's "tight list" output.
                let tight = matches!(item_blocks.as_slice(), [Block::Paragraph(_)]);
                if let [Block::Paragraph(inlines)] = item_blocks.as_slice() {
                    render_inlines(hooks, &mut item, inlines, fnotes);
                } else {
                    render_blocks_with(hooks, &mut item, item_blocks, fnotes);
                }
                if hoist_emptied(item_blocks, &item) {
                    continue;
                }
                // The non-tight `<li>\n…</li>` shape is restored only AFTER the
                // emptiness test, so the buffer under test is purely child
                // output. Prefixing first made every non-tight item look
                // non-empty, which forced `hoist_emptied` to `.trim()` — and
                // that trim deleted whitespace the author typed.
                if !tight {
                    item.insert(0, '\n');
                }
                kept += 1;
                // Per-`<li>` source line — populated only when the parser
                // ran with `emit_source_lines: true` (otherwise
                // `item_source_lines` is empty). Mirrors the legacy
                // transform_events shape (commit f91aca8fa, 2026-04-01) that
                // emitted `data-source-line` on `<li>` for proportional
                // scroll-sync interpolation between editor and preview.
                body.push_str("<li");
                let item_line = item_source_lines.get(idx).copied().flatten();
                push_source_line_attr(&mut body, item_line);
                body.push('>');
                body.push_str(&item);
                body.push_str("</li>\n");
            }
            if items.is_empty() || kept > 0 {
                if *ordered {
                    out.push_str("<ol");
                    // Emit `start="N"` when the parser captured an explicit
                    // non-default start number (`3. foo` → `Some(3)`).
                    // `None` for the default `1. foo` case keeps the
                    // shorter `<ol>` shape. Attribute order mirrors other
                    // typed-AST blocks: existing tag attrs first, then
                    // `data-source-line`. Phase 4 followup B (2026-05-28).
                    if let Some(n) = start {
                        out.push_str(" start=\"");
                        out.push_str(&n.to_string());
                        out.push('"');
                    }
                    push_source_line_attr(out, meta.source_line);
                    out.push_str(">\n");
                } else {
                    out.push_str("<ul");
                    push_source_line_attr(out, meta.source_line);
                    out.push_str(">\n");
                }
                out.push_str(&body);
                if *ordered {
                    out.push_str("</ol>\n");
                } else {
                    out.push_str("</ul>\n");
                }
            }
        }
        Block::CodeBlock { lang, value } => {
            out.push_str("<pre");
            push_source_line_attr(out, meta.source_line);
            out.push('>');
            match lang {
                Some(l) => {
                    out.push_str(r#"<code class="language-"#);
                    out.push_str(&escape_attr(l));
                    out.push_str(r#"">"#);
                }
                None => out.push_str("<code>"),
            }
            out.push_str(&escape_text(value));
            out.push_str("</code></pre>\n");
        }
        Block::Table {
            header,
            rows,
            alignments,
            header_source_line,
            row_source_lines,
        } => {
            // Parallel-vec invariant: when the parser populated per-row
            // source lines, the vector must align 1:1 with `rows`.
            // Empty (default) means "parser ran without
            // `emit_source_lines`" — that's the legitimate skip case.
            // Mirrors the `Block::List` and document-level invariants.
            debug_assert!(
                row_source_lines.is_empty() || row_source_lines.len() == rows.len(),
                "Block::Table invariant: row_source_lines.len() ({}) must equal rows.len() ({}) when populated",
                row_source_lines.len(),
                rows.len()
            );
            // Per-column alignment: author GFM `|--:|` wins; otherwise numeric
            // columns auto-right-align (so figure columns stop reading ragged).
            let col_align = resolve_column_alignment(header, rows, alignments);
            let cell_class = |col: usize| -> &'static str {
                col_align.get(col).copied().map_or("", CellAlign::class_attr)
            };
            // Accessible horizontal-scroll wrapper: keeps the `<table>`
            // semantically intact (unlike a `display:block` table), and
            // `tabindex` makes an overflowing table keyboard-scrollable.
            out.push_str("<div class=\"moss-table-scroll\" tabindex=\"0\">\n");
            out.push_str("<table");
            push_source_line_attr(out, meta.source_line);
            out.push_str(">\n<thead>\n<tr");
            // Header `<tr>` source line. Same f91aca8fa shape — annotated
            // when the parser tracked lines, omitted otherwise.
            push_source_line_attr(out, *header_source_line);
            out.push('>');
            for (col, cell) in header.iter().enumerate() {
                out.push_str("<th");
                out.push_str(cell_class(col));
                out.push('>');
                render_inlines(hooks, out, cell, fnotes);
                out.push_str("</th>");
            }
            out.push_str("</tr>\n</thead>\n");
            if !rows.is_empty() {
                out.push_str("<tbody>\n");
                for (idx, row) in rows.iter().enumerate() {
                    out.push_str("<tr");
                    let row_line = row_source_lines.get(idx).copied().flatten();
                    push_source_line_attr(out, row_line);
                    out.push('>');
                    for (col, cell) in row.iter().enumerate() {
                        out.push_str("<td");
                        out.push_str(cell_class(col));
                        out.push('>');
                        render_inlines(hooks, out, cell, fnotes);
                        out.push_str("</td>");
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</tbody>\n");
            }
            out.push_str("</table>\n");
            out.push_str("</div>\n");
        }
        Block::BlockQuote(children) => {
            let body = render_children_to_string(hooks, children, fnotes);
            if !hoist_emptied(children, &body) {
                out.push_str("<blockquote");
                push_source_line_attr(out, meta.source_line);
                out.push_str(">\n");
                out.push_str(&body);
                out.push_str("</blockquote>\n");
            }
        }
        Block::Shortcode(sc) => {
            hooks.render_shortcode(out, sc, meta.source_line);
            out.push('\n');
        }
        Block::ThematicBreak => {
            out.push_str("<hr");
            push_source_line_attr(out, meta.source_line);
            out.push_str(" />\n");
        }
        Block::Figure {
            image,
            caption,
            width,
            align,
            class_names,
            img_style,
        } => {
            // Phase 4 PR3 (2026-05-27): image-only paragraphs promoted at
            // parse time become Block::Figure. The render shape is a
            // `<figure class="moss-image">` wrap around the image hook's
            // output, optionally followed by `<figcaption>{caption}</figcaption>`.
            //
            // The inner image renders via `hooks.render_image` (the same
            // path as Inline::Image — production wires this through
            // `DefaultHooks::with_snapshot` / `PipelineHooks` which uses
            // `ImageContext::MarkdownInline`, producing the bare
            // `<picture><img></picture>` shape). The structural `<figure>`
            // wrapper is the Figure renderer's responsibility — this keeps
            // the byte shape contract with shape-spec § 1: the spec sample
            // shows `<figure>` containing exactly the MarkdownInline inner.
            //
            // Caption omission: `caption: None` means "no figcaption" (the
            // empty-alt case). Empty caption Vec is also treated as no
            // figcaption — defensive, since `caption: Some(vec![])` would
            // otherwise emit `<figcaption></figcaption>`.
            //
            // Figure-level display params (`width`, `align`, `class_names`,
            // `img_style`) are populated only by parameterized wikilink
            // embeds (image-embed synth-collapse). The class list /
            // `data-width=` byte shape matches
            // `render::image::wrap_in_figure_full` so an embed-sourced figure
            // and a CommonMark `![](url)` figure with the same params are
            // byte-identical. For the CommonMark path these are all defaults,
            // so `class="moss-image"` with no `data-width=` — unchanged from
            // before the collapse.
            let mut class_value = String::from("moss-image");
            if let Some(a) = align {
                class_value.push(' ');
                class_value.push_str(a);
            }
            for cn in class_names {
                if cn.is_empty() {
                    continue;
                }
                class_value.push(' ');
                class_value.push_str(cn);
            }
            out.push_str(r#"<figure class=""#);
            out.push_str(&escape_attr(&class_value));
            out.push('"');
            if let Some(w) = width {
                if w.ends_with('%') {
                    // Content-relative percent → inline style on the figure.
                    // The figure has never carried `style=` (img_style lives on
                    // the inner <img>), so there is no collision; the centering
                    // CSS keys off the same shape.
                    out.push_str(r#" style="width:"#);
                    out.push_str(&escape_attr(w));
                    out.push('"');
                } else {
                    // Named token → data-width contract (unchanged).
                    out.push_str(r#" data-width=""#);
                    out.push_str(&escape_attr(w));
                    out.push('"');
                }
            }
            push_source_line_attr(out, meta.source_line);
            out.push('>');
            // A caption that is about to be shown OWNS the description, so
            // the image inside it is decorative *relative to that caption*
            // and takes `alt=""`.
            //
            // Every figure moss builds derives its caption from the same
            // authored string as the alt — the implicit-figure promotion
            // uses the markdown alt (`try_promote_to_figure`), and a
            // wikilink embed uses the pothole alias for both
            // (`resolve::wikilink_dispatch`). Emitting the sentence twice
            // makes a screen reader announce it twice: once as the image's
            // accessible name, once as the caption. Per W3C/WAI guidance on
            // captioned images, `alt` and the caption must not duplicate
            // each other; when only one text exists, the visible caption is
            // the one to keep.
            //
            // Keyed off the caption that is ACTUALLY emitted below (same
            // `Some(non-empty)` test), so an uncaptioned figure — and every
            // non-figure image, which never reaches this arm — keeps its
            // alt as its only accessible name.
            let caption_owns_text = caption.as_ref().is_some_and(|c| !c.is_empty());
            // Render the inner image. Pattern-match the constrained shape;
            // any other inline falls back to the standard inline path so
            // the renderer never panics on a malformed Figure.
            match image {
                Inline::Image {
                    src, alt, title, ..
                } => match src {
                    Url::Resolved(r) => {
                        let alt = if caption_owns_text { "" } else { alt.as_str() };
                        hooks.render_image(
                            out,
                            r,
                            alt,
                            title.as_deref(),
                            img_style.as_deref(),
                            width.as_deref(),
                        );
                    }
                    Url::Unresolved(s) => {
                        debug_assert!(
                                false,
                                "Url::Unresolved({s:?}) reached Block::Figure renderer — visit_urls_mut missing or buggy"
                            );
                        out.push_str(r#"<img src=""#);
                        out.push_str(&escape_attr(s));
                        out.push_str(r#"" alt=""#);
                        out.push_str(&escape_attr(alt));
                        out.push_str(r#"" />"#);
                    }
                },
                _ => {
                    // Defensive: a non-Image inline in a Figure violates
                    // the parser-enforced shape, but the renderer must
                    // still emit something rather than crash.
                    render_inline(hooks, out, image, fnotes);
                }
            }
            if let Some(cap_inlines) = caption {
                if !cap_inlines.is_empty() {
                    out.push_str("<figcaption>");
                    render_inlines(hooks, out, cap_inlines, fnotes);
                    out.push_str("</figcaption>");
                }
            }
            out.push_str("</figure>\n");
        }
        Block::LinkCard { url, children } => {
            // Phase 4 PR4.5 (2026-05-28): the compound-link grid-cell shape.
            // External URLs render as a link-preview wrapper; internal URLs
            // render as `data-kind="link"` grid-card.
            //
            // Production byte shape matches today's src-tauri
            // `render_compound_link_cell` output (ported here so that
            // shape was deleted from src-tauri in PR4.5). The wrapping
            // `<div class="moss-grid">` chrome lives in the Grid render
            // arm in hooks.rs; LinkCard is the per-cell shape.
            let body = render_children_to_string(hooks, children, fnotes);
            if hoist_emptied(children, &body) {
                return;
            }
            let resolved = match url {
                Url::Resolved(r) => r,
                Url::Unresolved(s) => {
                    debug_assert!(
                        false,
                        "Url::Unresolved({s:?}) reached Block::LinkCard renderer — visit_urls_mut missing or buggy"
                    );
                    out.push_str(r#"<a href=""#);
                    out.push_str(&escape_attr(s));
                    out.push_str(r#"" class="moss-grid-card" data-kind="link">"#);
                    out.push_str(&body);
                    out.push_str("</a>");
                    return;
                }
            };
            use super::url::UrlKind;
            let is_external = matches!(resolved.kind, UrlKind::External | UrlKind::AssetNewtab);
            if is_external {
                out.push_str(r#"<a href=""#);
                out.push_str(&escape_attr(&resolved.href));
                out.push_str(
                    r#"" class="moss-grid-card link-preview" target="_blank" rel="noopener">"#,
                );
            } else {
                out.push_str(r#"<a href=""#);
                out.push_str(&escape_attr(&resolved.href));
                out.push_str(r#"" class="moss-grid-card" data-kind="link">"#);
            }
            out.push_str(&body);
            out.push_str("</a>");
        }
        Block::FootnoteDefinition { label, children } => {
            // The first definition of a label is hoisted into the endnote
            // section, so it emits nothing here. Identity-decided, so
            // skipping this subtree leaves no counter stale for a nested
            // definition inside it.
            if footnotes::is_hoisted(label, children, fnotes) {
                return;
            }
            render_blocks_with(hooks, out, children, fnotes);
        }
        Block::Other(html) => {
            out.push_str(html);
        }
    }
}

/// Render a container's children into their own buffer.
///
/// The buffer is what lets a container ask "did my children produce
/// anything?" before it commits to a wrapper element. Rendering still
/// happens exactly once and in document order, so the [`FootnoteCtx`]
/// side effects — hoist decisions, marker ids, back-link counts — are
/// byte-for-byte what they were when the children rendered straight into
/// `out`.
fn render_children_to_string<H: RenderHooks + ?Sized>(
    hooks: &H,
    children: &[Block],
    fnotes: &mut FootnoteCtx,
) -> String {
    let mut buf = String::new();
    render_blocks_with(hooks, &mut buf, children, fnotes);
    buf
}

/// True when a container HAD children and they all rendered to nothing —
/// which today means the only thing inside it was a footnote definition, and
/// `footnotes::render_section` hoisted that to the end of the page.
///
/// The `!children.is_empty()` half is the whole guard: a container the AUTHOR
/// left empty (`- one\n-\n- two`, a bare `>`) still renders, because pruning
/// is about the hoist moving content out from under a wrapper, not about
/// tidying up the author's markup. Without it, `- one\n-\n- two` would drop
/// from three bullets to two.
///
/// The test is EXACT emptiness, never `.trim()`: `str::trim` strips every
/// `char::is_whitespace`, so an item holding only U+00A0 or U+3000 (the
/// full-width space CJK authors type to indent) read as residue and was
/// deleted. Callers must hand this child output and nothing else — the list
/// arm adds its `'\n'` prefix only after this returns.
fn hoist_emptied(children: &[Block], rendered: &str) -> bool {
    !children.is_empty() && rendered.is_empty()
}

/// Append ` data-source-line="N"` to `out` when `source_line` is `Some`.
/// No-op otherwise.
///
/// Used at every top-level block's opening tag arm so the preview's
/// `cm-scroll-sync` (in `frontend/bridge/iframe-bridge.ts`) can locate
/// the DOM element that corresponds to a given editor source line.
///
/// Matches the legacy `transform_events` emit byte shape — leading space,
/// double-quoted attribute value, decimal integer — verified against
/// `src-tauri/src/build/ship.rs::apply_strip_removes_data_source_line`
/// which scrubs this exact pattern from the ship-stage output.
fn push_source_line_attr(out: &mut String, source_line: Option<usize>) {
    if let Some(n) = source_line {
        use std::fmt::Write as _;
        // unwrap_or: writing into a String never fails, but the API
        // returns Result. Keep this honest.
        let _ = write!(out, r#" data-source-line="{}""#, n);
    }
}

pub(super) fn render_inlines<H: RenderHooks + ?Sized>(
    hooks: &H,
    out: &mut String,
    inlines: &[Inline],
    fnotes: &mut FootnoteCtx,
) {
    for inline in inlines {
        render_inline(hooks, out, inline, fnotes);
    }
}

fn render_inline<H: RenderHooks + ?Sized>(
    hooks: &H,
    out: &mut String,
    inline: &Inline,
    fnotes: &mut FootnoteCtx,
) {
    match inline {
        Inline::Text(t) => out.push_str(&escape_text(t)),
        Inline::Link {
            url,
            title: _title,
            children,
            is_wikilink,
        } => {
            let resolved = match url {
                Url::Resolved(r) => r,
                Url::Unresolved(s) => {
                    debug_assert!(
                        false,
                        "Url::Unresolved({s:?}) reached renderer — visit_urls_mut missing or buggy"
                    );
                    // In release: emit href as-is so we don't crash, but
                    // the wide-net invariant test will catch the leak.
                    out.push_str(r#"<a href=""#);
                    out.push_str(&escape_attr(s));
                    out.push_str(r#"">"#);
                    render_inlines(hooks, out, children, fnotes);
                    out.push_str("</a>");
                    return;
                }
            };
            let mut content = String::new();
            render_inlines(hooks, &mut content, children, fnotes);
            // Phase 4 PR7a-flip-core-A (2026-05-28): pass the
            // `is_wikilink` flag directly to the hook. Pre-flip-core-A,
            // this arm synthesized a wikilink-kinded `ResolvedUrl` to
            // coax the hook's wikilink branch — a lossy workaround that
            // dropped the original `UrlKind` (`AssetNewtab` wikilinks
            // lost their `target="_blank" rel="noopener"`). The hook's
            // new signature carries both concerns orthogonally.
            hooks.render_link(out, resolved, *is_wikilink, &content);
        }
        Inline::Image {
            src, alt, title, ..
        } => {
            let resolved = match src {
                Url::Resolved(r) => r,
                Url::Unresolved(s) => {
                    debug_assert!(
                        false,
                        "Url::Unresolved({s:?}) reached renderer — visit_urls_mut missing or buggy"
                    );
                    out.push_str(r#"<img src=""#);
                    out.push_str(&escape_attr(s));
                    out.push_str(r#"" alt=""#);
                    out.push_str(&escape_attr(alt));
                    out.push_str(r#"" />"#);
                    return;
                }
            };
            hooks.render_image(out, resolved, alt, title.as_deref(), None, None);
        }
        Inline::Emphasis(children) => {
            out.push_str("<em>");
            render_inlines(hooks, out, children, fnotes);
            out.push_str("</em>");
        }
        Inline::Strong(children) => {
            out.push_str("<strong>");
            render_inlines(hooks, out, children, fnotes);
            out.push_str("</strong>");
        }
        Inline::Code(c) => {
            out.push_str("<code>");
            out.push_str(&escape_text(c));
            out.push_str("</code>");
        }
        Inline::Strikethrough(children) => {
            out.push_str("<del>");
            render_inlines(hooks, out, children, fnotes);
            out.push_str("</del>");
        }
        Inline::FootnoteRef(label) => footnotes::render_marker(hooks, out, label, fnotes),
        // GFM's shape, which is also Obsidian's: a disabled checkbox inline at
        // the head of the item. `disabled` because a published page is not an
        // app — the state is whatever the source says. The `<li>` needs no
        // class; `site.css` selects both shapes this renderer produces —
        // `li:has(> input[type="checkbox"])` for tight items, and
        // `li:has(> p:first-child > input[type="checkbox"]:first-child)` for
        // loose/multi-block items whose leading paragraph is wrapped in `<p>`.
        Inline::TaskMarker(checked) => {
            out.push_str(if *checked {
                "<input type=\"checkbox\" disabled checked /> "
            } else {
                "<input type=\"checkbox\" disabled /> "
            });
        }
        Inline::LineBreak => out.push_str("<br />\n"),
        Inline::Other(html) => {
            // A math node (ADR-030) is an `Inline::Other` carrying the P1
            // escaped-source `<code class="moss-math">` payload. Route it
            // through `render_math` so a typesetting hook (src-tauri's
            // `PipelineHooks`) can replace it with an SVG; the default hook
            // re-emits `html` verbatim, so non-pipeline renders are byte-
            // identical to P1. Any non-math `Inline::Other` falls straight
            // through to a raw push.
            match super::math_text::math_node_parts(html) {
                Some((tex, display)) => hooks.render_math(out, &tex, display, html),
                None => out.push_str(html),
            }
        }
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
