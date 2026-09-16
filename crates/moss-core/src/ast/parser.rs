//! Pulldown-cmark → typed AST parser.
//!
//! Walks `pulldown_cmark::Event` and assembles a [`Document`]. The parser
//! enables the same extensions moss's pipeline does: tables, footnotes,
//! strikethrough.
//!
//! All URL nodes start as [`Url::Unresolved`]; classifying into
//! [`Url::Resolved`] is the job of [`crate::ast::visit::visit_urls_mut`]
//! (a separate pass).
//!
//! Heading IDs ARE assigned by this parser. Phase 4 PR2: each
//! `Tag::Heading` arm computes the Obsidian-compatible anchor slug from
//! the heading's text content (only `Event::Text` / `Event::Code`,
//! matching production's `transform_events` behavior in
//! `src-tauri/src/build/markdown/pipeline.rs` lines 1776-1845); a
//! post-parse pass ([`assign_heading_id_suffixes`]) walks all headings in
//! document order (recursively into BlockQuotes, lists, callouts) and
//! applies duplicate-suffix numbering (`{slug}-1`, `-2`, …) matching the
//! `id_counts` HashMap behavior at `pipeline.rs:1798`.

use std::collections::{HashMap, HashSet};

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::document::{BlockMeta, Document};
use super::footnotes::FootnoteIndex;
use super::math_text::{math_inline, math_source};
use super::node::{Block, CalloutKind, ColumnAlignment, Fold, Inline};
use super::shortcode::Shortcode;
use super::shortcode_extract::{extract_shortcodes_with_config, parse_placeholder, ExtractedShortcode};
use super::url::Url;
use crate::heading::anchor::obsidian_heading_anchor;

/// Parser configuration flags.
///
/// Threaded through [`parse_with_config`] to gate optional parser behaviors
/// that the renderer needs to coordinate with (source-line tracking for
/// preview scroll sync, implicit-figure promotion).
///
/// [`Default`] = "production preview off" — `emit_source_lines: false`,
/// `implicit_figure: true`. The `implicit_figure` default mirrors today's
/// always-on behavior of the parser before this config existed; flipping it
/// off is opt-in for the small set of fragment-render call sites that need
/// bare `<img>` (none today, but the flag exists for symmetry with the
/// legacy `transform_events` API and the production `site_config` field).
#[derive(Debug, Clone, Copy)]
pub struct ParseConfig {
    /// When true, populates [`BlockMeta::source_line`] for top-level
    /// blocks. The renderer emits `data-source-line="N"` on the opening
    /// tag for any block whose meta carries `Some(N)`.
    ///
    /// Production wires this from `process_markdown_file`'s
    /// `emit_source_lines` argument (`true` during preview builds, `false`
    /// during ship-stage publish builds — `data-source-line` is stripped
    /// at ship time anyway, but emitting fewer attrs upstream is cheaper
    /// and keeps published HTML clean from earlier stages).
    pub emit_source_lines: bool,

    /// When true (default), image-only paragraphs promote to
    /// [`Block::Figure`] via [`try_promote_to_figure`]. When false, they
    /// stay as [`Block::Paragraph`] containing one [`Inline::Image`].
    ///
    /// Production wires this from `site_config.implicit_figure` (default
    /// `true`). The flag mirrors the legacy `transform_events`
    /// implicit-figure pass: sites that prefer bare `<img>` (no `<figure>`
    /// wrap) can opt out.
    pub implicit_figure: bool,

    /// Added to every computed `source_line` so the emitted
    /// `data-source-line` / `data-source-range` values match the editor's
    /// REAL FILE line numbers (CM6 `doc.lineAt`), not body-relative lines.
    ///
    /// The parser only ever sees the markdown BODY (frontmatter is stripped
    /// upstream), so its byte offsets — and thus `LineLookup` — are
    /// body-relative. The editor, however, reports raw-file lines including
    /// the frontmatter. Without this offset, every annotation is short by the
    /// frontmatter line count, so editor→preview scroll-sync maps to the wrong
    /// element (the home page's grid scrolled the preview to the bottom). Set
    /// to the number of lines the frontmatter consumes (0 when there is none).
    /// See `process_markdown_file` and docs/reference/editor-preview-sync.md
    /// "Known defect — source-line coordinate-system mismatch".
    pub source_line_offset: usize,

    /// When true, `$…$` / `$$…$$` parse as math ([`Options::ENABLE_MATH`])
    /// and render as escaped LaTeX source in `<code class="moss-math">`.
    /// When false (default), `$` is an ordinary character and math source
    /// passes through as literal text.
    ///
    /// Default is `false` — unlike the other flags, this one changes what
    /// the *characters* mean, so every in-crate `parse()` caller and every
    /// committed snapshot fixture keeps today's behavior until a site opts
    /// in. Production wires it from `site_config.math` (`[site].math`,
    /// default on), which is where the "is `$5` currency or an unclosed
    /// equation?" judgment belongs.
    pub math: bool,

    /// When true, a single newline inside a paragraph renders as `<br>`,
    /// matching Obsidian's default (`strictLineBreaks = false`, i.e. remark
    /// `breaks: true`). When false, CommonMark applies and the newline is a
    /// space.
    ///
    /// Default is `false` for the same reason `math` is: it changes what the
    /// author's *characters* mean, so every in-crate `parse()` caller and
    /// every committed fixture keeps today's behavior until a site opts in.
    /// Production wires it from `site_config.hard_line_breaks`.
    pub hard_line_breaks: bool,
}

impl Default for ParseConfig {
    fn default() -> Self {
        Self {
            emit_source_lines: false,
            // `true` matches today's always-on behavior of the parser
            // before ParseConfig existed; the ~40 in-crate `parse()`
            // callers all assume figure promotion happens.
            implicit_figure: true,
            source_line_offset: 0,
            // Off by default so the ~40 in-crate `parse()` callers and every
            // committed snapshot fixture are untouched by math landing.
            // Production opts in via `[site].math`.
            math: false,
            // Off by default so in-crate callers and committed fixtures keep
            // CommonMark's "newline is a space". Production opts in.
            hard_line_breaks: false,
        }
    }
}

/// **The** pulldown-cmark option set moss parses markdown with.
///
/// Every parser construction site in the repo must call this rather than
/// hand-assembling its own `Options` — moss previously had five independent
/// `Options` blocks (typed AST, newsletter ×2, `llms_txt`, the markdown
/// pipeline), and each one that drifted became a surface where the same
/// document parsed differently depending on which output it was headed for.
/// A site that legitimately needs a different set calls this and then adjusts
/// the one option, so the divergence reads as an explicit delta at the call
/// site instead of being invisibly re-hand-rolled. There is no such delta
/// today: this comment used to cite the newsletter walker omitting
/// `ENABLE_FOOTNOTES`, which stopped being true when email gained footnote
/// arms — with the bit off, CommonMark reads `[^x]: <url>` as a link reference
/// definition and deletes the note outright.
///
/// `math` gates `ENABLE_MATH` (`$…$` / `$$…$$` → [`Event::InlineMath`] /
/// [`Event::DisplayMath`]). It is a parameter rather than part of the base
/// set because it changes the meaning of a character that appears in
/// ordinary prose (`$5`), so it is the one option a site must opt into —
/// production wires it from `[site].math` on `SiteConfig`.
///
/// **Enabling `math` obliges the caller's event walker to handle both math
/// events.** pulldown emits them as leaf inline events; a walker that
/// pattern-matches known events and ignores the rest will *silently delete*
/// every equation in the document (measured: `Energy $E = mc^2$.` →
/// `<p>Energy .</p>`). See `src-tauri/tests/math_wiring_invariant_test.rs`,
/// which fails any site that turns math on without arms in the same walker.
///
/// `ENABLE_TASKLISTS` carries the same obligation, and it is met by
/// [`Inline::TaskMarker`]: the flag makes pulldown emit
/// `Event::TaskListMarker` as the first event inside `Tag::Item`, and both
/// the leaf arm in `parse_inline` and the whitelist in `parse_inline_event`
/// model it. Turning the flag on WITHOUT those arms silently deletes the
/// checkbox — measured on `- [ ] todo\n- [x] done`, which rendered
/// `<ul><li>todo</li><li>done</li></ul>`. See ADR-035 § Task lists.
pub fn parser_options(math: bool) -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);
    // Phase 3 PR2: pulldown-cmark emits `LinkType::WikiLink` events for
    // `[[…]]` / `![[…]]` natively. The typed-AST parser preserves them as
    // `Inline::Link`/`Inline::Image` with `Url::Unresolved`; resolution
    // happens in the later `visit_urls_mut` pass.
    options.insert(Options::ENABLE_WIKILINKS);
    // CJK-friendly emphasis: closes a `*`/`**` run whose delimiter sits between
    // a CJK punctuation mark and a CJK ideograph (no ASCII space, as CJK prose
    // never has one) — vanilla CommonMark flanking leaves it as a literal `**`.
    // pulldown-cmark#1059, implementing the `tats-u/markdown-cjk-friendly`
    // amendment to CommonMark 0.31.2; backward-compatible on every existing
    // CommonMark example. Feature-gated because the flag exists only in the
    // `[patch]` fork until pulldown releases it, keeping the published crate
    // buildable against crates.io. Pinned by `src-tauri/tests/cjk_emphasis.rs`.
    #[cfg(feature = "cjk-friendly-emphasis")]
    options.insert(Options::ENABLE_CJK_FRIENDLY_EMPHASIS);
    if math {
        options.insert(Options::ENABLE_MATH);
    }
    options
}

/// Parse markdown into a typed [`Document`] using the default config.
///
/// Equivalent to `parse_with_config(markdown, &ParseConfig::default())`.
/// This is the entry point for the ~40 in-crate callers that don't need
/// per-parse configuration (URL resolution tests, frontmatter round-trip
/// tests, etc.). Production paths that need source-line tracking or
/// implicit-figure toggling call [`parse_with_config`].
pub fn parse(markdown: &str) -> Document {
    parse_with_config(markdown, &ParseConfig::default())
}

/// Parse markdown into a typed [`Document`].
///
/// This is the AST entry point. The input is post-resolve markdown (the
/// upstream resolve pipeline has already rewritten wikilinks into standard
/// markdown links with `moss-resolved:` prefixes).
///
/// Two-stage parse:
/// 1. [`extract_shortcodes`] pre-scans for `:::name` blocks, replacing
///    each with a sentinel HTML comment.
/// 2. Pulldown-cmark parses the substituted markdown into events; each
///    sentinel comes back as a `Block::Other` raw HTML.
/// 3. A final pass walks the AST and substitutes `Block::Other` sentinel
///    payloads with the corresponding typed [`Block::Shortcode`].
///
/// When `config.emit_source_lines` is true, the parser walks events via
/// `into_offset_iter()` so each top-level block carries the byte offset
/// of its first event; a [`LineLookup`] converts the offset to a 1-based
/// line number stored in [`BlockMeta::source_line`].
pub fn parse_with_config(markdown: &str, config: &ParseConfig) -> Document {
    parse_document(markdown, config, HeadingIds::Number)
}

/// Parse a FRAGMENT that will be embedded in some other document's tree —
/// a `:::grid` cell, a compound-link card's inner blocks, a `:::hero`
/// overlay — and leave its heading ids holding their bare base slugs.
///
/// Duplicate-id numbering is a whole-PAGE decision: the fragment renders into
/// the same document as the body, so the only counter that can keep every
/// `id=` unique is the outer parse's. Numbering here as well suffixed twice —
/// a cell holding two `## Notes` arrived as `notes` / `notes-1`, and the outer
/// walk then bumped the first to `notes-1` (colliding with the second) and the
/// second to `notes-1-1`, a shape no slug rule can produce.
///
/// The outer [`assign_heading_id_suffixes`] reaches every fragment:
/// [`collect_heading_id_slots`] is exhaustive over `Block` and descends into
/// grid cells, hero overlays and link cards. Nested fragments inherit this
/// entry point, because [`super::shortcode_extract::parse_cell_to_blocks`] is
/// the only way a cell is parsed at any depth.
pub(super) fn parse_fragment_with_config(markdown: &str, config: &ParseConfig) -> Document {
    parse_document(markdown, config, HeadingIds::LeaveBare)
}

/// Whether a parse owns duplicate-heading-id numbering for its blocks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HeadingIds {
    /// Top-level document parse: number every id in render order.
    Number,
    /// Embedded fragment: the enclosing document's parse numbers these.
    LeaveBare,
}

fn parse_document(markdown: &str, config: &ParseConfig, heading_ids: HeadingIds) -> Document {
    let extraction = extract_shortcodes_with_config(markdown, config);

    // `[![[x.png]]](/url)`: the embed is lifted to a sentinel, restored below.
    let (source, linked_embeds) =
        super::linked_embed::substitute(&extraction.markdown_with_placeholders, &extraction.nonce);

    let options = parser_options(config.math);

    // Source-line tracking requires the `into_offset_iter` form of the
    // parser, which yields (Event, Range<usize>). When tracking is off,
    // we use the plain iterator (no per-event offset overhead).
    let (events, offsets): (Vec<Event<'_>>, Vec<Option<std::ops::Range<usize>>>) =
        if config.emit_source_lines {
            let mut evs = Vec::new();
            let mut offs = Vec::new();
            for (event, range) in Parser::new_ext(&source, options).into_offset_iter() {
                evs.push(event);
                offs.push(Some(range));
            }
            (evs, offs)
        } else {
            let evs: Vec<Event<'_>> = Parser::new_ext(&source, options).collect();
            let len = evs.len();
            (evs, vec![None; len])
        };

    // Build the prefix-sum line table once (only when needed).
    //
    // CAVEAT: the markdown that the offsets index into is `source` (the
    // post-extraction, post-linked-embed-substitution string), NOT the
    // original `markdown` passed in. Shortcode extraction may rewrite some bytes
    // into sentinel HTML comments of a different length; line numbers
    // would be off for blocks following an extracted shortcode if we
    // built the lookup against the original. We build against the
    // post-extraction string, so the line numbers match the
    // post-extraction view — which is what users see in their editor
    // before shortcode-block lines, and is "close enough" after (the
    // sentinel preserves one line per extracted block, so line counts
    // after the extracted block are within one of the source). See the
    // architecture note in `shortcode_extract.rs` for the placeholder
    // shape.
    //
    // For the source-line-off path, lookup is unused.
    let line_lookup = if config.emit_source_lines {
        Some(LineLookup::build(&source, config.source_line_offset))
    } else {
        None
    };

    // Line-tracking context handed to every recursive parser entry; the
    // Tag::List / Tag::Table arms consult it to annotate per-item / per-row
    // source lines. `None` when `emit_source_lines` is off; the inner
    // arms see this as "skip annotation" and emit empty parallel vecs.
    let line_ctx: Option<LineCtx<'_>> = line_lookup.as_ref().map(|lookup| LineCtx {
        lookup,
        offsets: &offsets,
    });

    let mut blocks = Vec::new();
    let mut block_meta: Vec<BlockMeta> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let event_start_idx = i;
        let (block, advance) = parse_block(&events, i, line_ctx.as_ref());
        if let Some(b) = block {
            // Compute source_line from the first event's byte offset, if
            // we collected offsets and a lookup is in scope.
            let source_line = match (line_lookup.as_ref(), offsets.get(event_start_idx)) {
                (Some(lookup), Some(Some(range))) => Some(lookup.line_at(range.start)),
                _ => None,
            };
            blocks.push(b);
            block_meta.push(BlockMeta { source_line });
        }
        i += advance.max(1);
    }

    // Put the lifted things back: placeholders become typed Shortcodes, `![[…]]`
    // sentinels their `Inline::Image`. Both BEFORE `assign_heading_id_suffixes`.
    substitute_shortcode_placeholders(&mut blocks, &extraction.nonce, &extraction.extracted);
    super::linked_embed::restore(&mut blocks, &linked_embeds, config);

    // Implicit-figure gating: the per-paragraph `try_promote_to_figure`
    // inside `parse_block_with_tag` always runs (so the figure promotion
    // happens at parse time inside the Tag::Paragraph arm). When
    // `config.implicit_figure` is false, we walk the assembled blocks
    // and "undo" the promotion — converting `Block::Figure { image, ..}`
    // back to `Block::Paragraph(vec![image])`.
    //
    // The unwinding-at-the-end approach was chosen over threading the
    // flag into `parse_block_with_tag` because the latter would mean
    // propagating `config` through ~14 inner parser functions whose
    // signatures are already tight. The unwind is O(N) and only fires
    // on the rare opt-out path; production keeps the default `true`.
    if !config.implicit_figure {
        for block in blocks.iter_mut() {
            unwrap_implicit_figure(block);
        }
    }

    // Apply duplicate-suffix numbering to heading IDs in document order.
    // Each Tag::Heading arm computes the base slug; this pass disambiguates
    // collisions across the whole document, matching production's id_counts
    // HashMap behavior in pipeline.rs::transform_events.
    //
    // Skipped for embedded fragments — see [`parse_fragment_with_config`].
    // The page that hosts them owns the one counter that can keep every id
    // on the rendered page unique.
    if heading_ids == HeadingIds::Number {
        assign_heading_id_suffixes(&mut blocks);
    }

    let mut doc = Document::from_blocks_with_meta(blocks, block_meta);
    // Carry the extractor's findings out of the parse. Collected since the
    // unknown-name fallback landed, dropped here until now.
    doc.warnings = extraction.warnings;

    // Obsidian parity: a single newline inside a paragraph becomes `<br>`.
    // Runs last, over the finished tree, because pulldown-cmark 0.13 has no
    // hard-break option — see `ast/line_breaks.rs` for why a post-parse
    // transform is the only mechanism and why it is exact.
    if config.hard_line_breaks {
        super::line_breaks::apply(&mut doc);
    }

    doc
}

/// Recursively undo implicit-figure promotion in `block` and its children.
///
/// Called when `ParseConfig::implicit_figure` is false. Walks the block
/// tree (descending into containers — `BlockQuote`, `Callout`, `List`,
/// `LinkCard`, `FootnoteDefinition`) and rewrites any `Block::Figure` back
/// to `Block::Paragraph(vec![image])` with the original alt text preserved.
/// The caption is discarded (matches the legacy bare-`<img>` shape).
///
/// The opt-out is a whole-document setting, so a container that holds
/// blocks and is NOT listed here silently keeps promoting — the `_ => {}`
/// below is why adding `Block::FootnoteDefinition` compiled fine while an
/// opted-out site published a `<figcaption>` in its endnotes. Deliberately
/// excluded: `Block::Shortcode` (a cell is its own parse and runs this walk
/// itself) and `Block::Table` (cells are `Vec<Inline>`, never blocks, so
/// nothing there can be a `Figure`). Every other block-holding variant
/// belongs in the arm below.
///
/// A figure the author explicitly ASKED for stays. `![pic|55%](x.png)` and
/// `![[x.png|wide]]` need the `<figure>` to hold their `style="width:…"` /
/// `data-width=` / align class, and none of that is implicit — the author
/// typed it. Only an undecorated figure is the promotion this pass undoes.
/// See [`is_implicit_figure`].
pub(crate) fn unwrap_implicit_figure(block: &mut Block) {
    // Replace this block if it's an undecorated Figure.
    if is_implicit_figure(block) {
        if let Block::Figure { image, .. } = block {
            let img = std::mem::replace(
                image,
                Inline::Text(String::new()), // placeholder, overwritten below
            );
            *block = Block::Paragraph(vec![img]);
        }
        return;
    }
    // Recurse into containers.
    match block {
        Block::BlockQuote(children)
        | Block::Callout { children, .. }
        | Block::LinkCard { children, .. }
        | Block::FootnoteDefinition { children, .. } => {
            for child in children.iter_mut() {
                unwrap_implicit_figure(child);
            }
        }
        Block::List { items, .. } => {
            for item in items.iter_mut() {
                for child in item.iter_mut() {
                    unwrap_implicit_figure(child);
                }
            }
        }
        // Grid cells and a hero overlay. At parse time this is a no-op — a
        // cell is its own parse and has already run this walk — but the
        // post-dispatch caller needs it: `dispatch_wikilink_embeds` descends
        // into shortcode bodies (see `dispatch_in_shortcode`), so a
        // `![[tile.png]]` in a grid cell becomes a figure there and nowhere
        // else. The walk is idempotent, so running it twice costs a visit.
        Block::Shortcode(sc) => match sc {
            Shortcode::Grid(args) => {
                for cell in args.cells.iter_mut() {
                    for child in cell.iter_mut() {
                        unwrap_implicit_figure(child);
                    }
                }
            }
            Shortcode::Hero(args) => {
                for child in args.overlay.iter_mut() {
                    unwrap_implicit_figure(child);
                }
            }
            Shortcode::Subscribe(_)
            | Shortcode::Buttons(_)
            | Shortcode::Gallery(_)
            | Shortcode::Recent(_)
            | Shortcode::Apply(_) => {}
        },
        _ => {}
    }
}

/// Is this block a figure moss decided on by itself?
///
/// True only for a `Block::Figure` carrying none of the display parameters an
/// author can ask for. `|55%`, `|wide`, `|left` and a class all live ON the
/// figure, so unwrapping one of those would discard what the author typed —
/// the opt-out undoes an inference, never an instruction.
fn is_implicit_figure(block: &Block) -> bool {
    matches!(
        block,
        Block::Figure {
            width: None,
            align: None,
            class_names,
            img_style: None,
            ..
        } if class_names.is_empty()
    )
}

/// Undo implicit-figure promotion across a whole document.
///
/// Public because it runs twice, at times only the caller knows.
/// `parse_with_config` runs it on the blocks it just built; the build pipeline
/// runs it again after `dispatch_wikilink_embeds`, which mints `Block::Figure`
/// nodes of its own long after the parser finished. Without that second call
/// `implicit_figure = false` was honoured for `![alt](x.png)` and ignored for
/// `![[x.png]]` — one intent, two boxes, and any theme rule keyed on
/// `.moss-image` reached only the wikilink half of a page's images.
pub fn unwrap_implicit_figures(doc: &mut Document) {
    for block in doc.blocks.iter_mut() {
        unwrap_implicit_figure(block);
    }
}

/// Bundle of borrowed line-tracking state threaded through recursive
/// parser entries. Constructed once per `parse_with_config` when
/// `emit_source_lines` is true; `None` everywhere else.
///
/// `parse_block` / `parse_block_with_tag` consult `line_at_event` to
/// annotate per-`<li>` and per-`<tr>` source lines. The outer
/// top-level-block source line is computed at the parse loop itself
/// (already in place), not here.
struct LineCtx<'a> {
    lookup: &'a LineLookup,
    offsets: &'a [Option<std::ops::Range<usize>>],
}

impl<'a> LineCtx<'a> {
    /// 1-based source line of the event at `event_index`, or `None` if
    /// the offset is missing (defensive — shouldn't happen when the
    /// parser is operating with `emit_source_lines: true`).
    fn line_at_event(&self, event_index: usize) -> Option<usize> {
        match self.offsets.get(event_index) {
            Some(Some(range)) => Some(self.lookup.line_at(range.start)),
            _ => None,
        }
    }
}

/// Prefix-sum line-number lookup for byte offsets in a source string.
///
/// Built once per parse (when `emit_source_lines` is on). Stores the byte
/// offset of every `\n` in `source`; `line_at(offset)` returns the
/// 1-based line number containing that offset via binary search.
///
/// Equivalent (slower) form: `source[..offset].matches('\n').count() + 1`
/// — O(N) per call vs. O(log N) here. For documents with ~25 blocks the
/// difference is negligible, but the binary-search form is the canonical
/// pattern and is the cheaper hot-path shape.
struct LineLookup {
    /// Byte offsets of every `\n` in the source. Sorted ascending by
    /// construction. `newline_offsets[i]` is the byte index of the i-th
    /// newline (0-based).
    newline_offsets: Vec<usize>,
    /// Added to every `line_at` result so body-relative lines become
    /// raw-file lines (the frontmatter line count). See
    /// `ParseConfig::source_line_offset`.
    line_offset: usize,
}

impl LineLookup {
    fn build(source: &str, line_offset: usize) -> Self {
        let mut newline_offsets = Vec::new();
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                newline_offsets.push(i);
            }
        }
        Self {
            newline_offsets,
            line_offset,
        }
    }

    /// 1-based line number containing `byte_offset`, plus `line_offset`.
    ///
    /// Offset 0 (before any newline) → line 1. After the first newline →
    /// line 2. Etc. Offsets past the end of the source clamp to the last
    /// line + 1. `line_offset` (the frontmatter line count) is added so the
    /// result is a raw-file line, matching the editor's `doc.lineAt`.
    fn line_at(&self, byte_offset: usize) -> usize {
        // Find the number of newlines strictly before `byte_offset`.
        // That count + 1 is the 1-based line number.
        let body_line = match self.newline_offsets.binary_search(&byte_offset) {
            // Exact match: offset IS a newline byte; the newline belongs
            // to the line that ENDS at it, so line number = idx + 1.
            // (The next byte starts line idx + 2; this matches the legacy
            // count-and-add-1 semantics, which counts newlines BEFORE the
            // offset.)
            Ok(idx) => idx + 1,
            Err(idx) => idx + 1,
        };
        body_line + self.line_offset
    }
}

/// Walk top-level blocks; replace any `Block::Other` whose payload is a
/// `<!--MOSS_SC_{nonce}_{index}-->` sentinel with the corresponding typed
/// [`Block::Shortcode`].
fn substitute_shortcode_placeholders(
    blocks: &mut Vec<Block>,
    nonce: &str,
    extracted: &[ExtractedShortcode],
) {
    for block in blocks.iter_mut() {
        if let Block::Other(html) = block {
            if let Some(index) = parse_placeholder(nonce, html) {
                if let Some(entry) = extracted.iter().find(|e| e.index == index) {
                    *block = Block::Shortcode(entry.shortcode.clone());
                }
            }
        }
        // Future: descend into BlockQuote / List items / Callouts when
        // shortcodes inside those constructs are modeled. Phase B Tasks
        // 7-10 only need top-level shortcodes.
    }
}

/// Parse one block-level construct starting at `events[start]`. Returns
/// the parsed block (or `None` if `events[start]` was a closing tag /
/// stray event we skip) and how many events to advance.
///
/// `line_ctx` carries the optional line-tracking context for per-item
/// (`<li>`) and per-row (`<tr>`) source-line annotation; threaded through
/// to `parse_block_with_tag`.
fn parse_block(
    events: &[Event<'_>],
    start: usize,
    line_ctx: Option<&LineCtx<'_>>,
) -> (Option<Block>, usize) {
    // Block-level dispatch. pulldown always wraps loose inlines (math
    // included) in Tag::Paragraph at top level, so no math event ever reaches
    // this match; the paragraph's inlines are collected by the math-aware
    // parse_inline. Pinned by `display_math_block_survives_on_its_own_lines`.
    // allow:math-events-ignored — see above.
    match &events[start] {
        Event::Start(tag) => parse_block_with_tag(events, start, tag, line_ctx),
        Event::Text(_) | Event::Code(_) | Event::Html(_) | Event::SoftBreak | Event::HardBreak => {
            // Top-level stray inlines: pulldown-cmark always wraps these in
            // `Tag::Paragraph` at top level, so this branch is dead in practice.
            //
            // The tight-list-item case where the inlines are emitted directly
            // (no Tag::Paragraph wrap) was the load-bearing reason this branch
            // looked relevant; PR0.6 moved that responsibility into
            // `collect_item_blocks`, which synthesizes a Block::Paragraph for
            // stray inlines inside Tag::Item. See parser.rs's collect_item_blocks
            // helper.
            (None, 1)
        }
        Event::End(_) => (None, 1),
        Event::Rule => (Some(Block::ThematicBreak), 1),
        _ => (None, 1),
    }
}

fn parse_block_with_tag(
    events: &[Event<'_>],
    start: usize,
    tag: &Tag<'_>,
    line_ctx: Option<&LineCtx<'_>>,
) -> (Option<Block>, usize) {
    match tag {
        Tag::Heading { level, .. } => {
            let (children, end) = collect_inlines_until(events, start + 1, |e| {
                matches!(e, Event::End(TagEnd::Heading(_)))
            });
            let level_num = match level {
                HeadingLevel::H1 => 1,
                HeadingLevel::H2 => 2,
                HeadingLevel::H3 => 3,
                HeadingLevel::H4 => 4,
                HeadingLevel::H5 => 5,
                HeadingLevel::H6 => 6,
            };
            // Phase 4 PR2: compute the heading-anchor base slug from the
            // text/code content between Start(Heading) and End(Heading),
            // matching production's transform_events behavior. Inline HTML
            // (`<br>` etc.), images, and link href text are NOT included —
            // only Event::Text and Event::Code. The post-parse
            // `assign_heading_id_suffixes` pass disambiguates collisions.
            let heading_text = crate::heading::text::events_to_text(events, start + 1, end);
            let base_slug = obsidian_heading_anchor(&heading_text);
            (
                Some(Block::Heading {
                    level: level_num,
                    children,
                    id: Some(base_slug),
                }),
                end - start + 1,
            )
        }
        Tag::Paragraph => {
            let (children, end) = collect_inlines_until(events, start + 1, |e| {
                matches!(e, Event::End(TagEnd::Paragraph))
            });
            // Phase 4 PR3 (2026-05-27): detect image-only paragraphs and
            // promote to `Block::Figure`. See shape-spec § 1 detection
            // rule: exactly one `Inline::Image` plus any number of
            // whitespace-only `Inline::Text` / `Inline::LineBreak`
            // siblings qualifies. Caption defaults to the image's alt
            // text (mirroring transform_events' implicit-figure path);
            // empty alt yields `caption: None` so no `<figcaption>` is
            // emitted.
            //
            // A paragraph with image+prose (e.g. `![img](src) caption text`)
            // does NOT qualify; it stays as `Block::Paragraph`. This is the
            // critical regression guard — see PR1 v2 (commit 71c657af3)
            // for the analogous shape decision at the inline image hook
            // level: inline images use `MarkdownInline` (no figure wrap);
            // only the standalone figure case here uses the figure wrap.
            let block = match try_promote_to_figure(children, events, start) {
                Ok(figure) => figure,
                Err(original_inlines) => Block::Paragraph(original_inlines),
            };
            (Some(block), end - start + 1)
        }
        Tag::CodeBlock(kind) => {
            let lang = match kind {
                pulldown_cmark::CodeBlockKind::Fenced(s) if !s.is_empty() => Some(s.to_string()),
                _ => None,
            };
            let mut value = String::new();
            let mut i = start + 1;
            while i < events.len() {
                match &events[i] {
                    // allow:math-events-ignored — pulldown does not parse math
                    // inside a code fence, so it emits no math event here;
                    // ```\n$x^2$\n``` is byte-identical at math on and off.
                    Event::End(TagEnd::CodeBlock) => break,
                    Event::Text(t) => value.push_str(t),
                    _ => {}
                }
                i += 1;
            }
            (Some(Block::CodeBlock { lang, value }), i - start + 1)
        }
        Tag::BlockQuote(_) => {
            // Phase 4 PR4: detect Obsidian-style callouts. A blockquote
            // whose first paragraph's leading text matches `[!<kind>]`
            // (with optional `+`/`-` foldable suffix and optional
            // inline title) promotes to `Block::Callout`. Otherwise it
            // stays a plain blockquote. See shape-spec § 1.
            //
            // Detection works on the EVENT stream (not the parsed
            // children) because pulldown-cmark's SoftBreak events
            // become `Inline::Text("\n")` during inline parsing
            // (PR4.5 aligned to CommonMark spec — see
            // `parse_inline` SoftBreak handling). Working on events
            // preserves the structural break before inline
            // collapse, which is what the marker-line-vs-body-line
            // boundary check needs.
            match detect_and_assemble_callout(events, start + 1, line_ctx) {
                Some((block, body_end)) => (Some(block), body_end - start + 1),
                None => {
                    let (children, end) = collect_blocks_until(events, start + 1, line_ctx, |e| {
                        matches!(e, Event::End(TagEnd::BlockQuote(_)))
                    });
                    (Some(Block::BlockQuote(children)), end - start + 1)
                }
            }
        }
        Tag::List(start_num) => {
            let ordered = start_num.is_some();
            // Preserve explicit ordered-list start number when it's not
            // the implicit default `1`. `3. foo` → `Some(3)` so the
            // renderer can emit `<ol start="3">`. pulldown-cmark
            // normalizes `1. foo` to `Some(1)`, which we collapse to
            // `None` because `<ol>` and `<ol start="1">` are
            // semantically identical and we prefer the cleaner attr-free
            // shape for the common case. Bound name is `list_start` to
            // avoid shadowing the outer `start: usize` event-index
            // parameter.
            let list_start = match start_num {
                Some(n) if *n != 1 => Some(*n),
                _ => None,
            };
            let mut items: Vec<Vec<Block>> = Vec::new();
            // Parallel-to-`items` per-`<li>` source-line annotations.
            // Empty when `line_ctx` is None; otherwise tracks each
            // `Event::Start(Tag::Item)`'s byte offset → line. The renderer
            // emits `<li data-source-line="N">` for entries that are Some.
            let mut item_source_lines: Vec<Option<usize>> = Vec::new();
            let track_lines = line_ctx.is_some();
            let mut i = start + 1;
            while i < events.len() {
                match &events[i] {
                    // allow:math-events-ignored — structural walk that only
                    // locates item boundaries; every item's content is parsed
                    // by the math-aware collect_item_blocks. Pinned by
                    // `math_survives_inside_list_items`.
                    Event::End(TagEnd::List(_)) => break,
                    Event::Start(Tag::Item) => {
                        if track_lines {
                            item_source_lines.push(line_ctx.and_then(|ctx| ctx.line_at_event(i)));
                        }
                        let (item_blocks, end) = collect_item_blocks(events, i + 1, line_ctx);
                        items.push(item_blocks);
                        i = end + 1;
                    }
                    _ => i += 1,
                }
            }
            (
                Some(Block::List {
                    ordered,
                    start: list_start,
                    items,
                    item_source_lines,
                }),
                i - start + 1,
            )
        }
        Tag::Table(column_alignments) => {
            // GFM per-column alignment (`|:--|`, `|:-:|`, `|--:|`). Kept
            // source-faithful in the AST; numeric auto-alignment for unaligned
            // columns is resolved later, at render time.
            //
            // pulldown emits a full-width `Vec` of `Alignment::None` for a bare
            // `|---|` table. Normalize that to an empty vec so an unaligned
            // table carries no `alignments` — which keeps serialized ASTs
            // byte-stable (via `skip_serializing_if`) and lets the renderer read
            // "empty ⇒ every column auto-detects".
            let alignments: Vec<ColumnAlignment> = if column_alignments
                .iter()
                .all(|a| matches!(a, pulldown_cmark::Alignment::None))
            {
                Vec::new()
            } else {
                column_alignments
                    .iter()
                    .map(|a| match a {
                        pulldown_cmark::Alignment::None => ColumnAlignment::None,
                        pulldown_cmark::Alignment::Left => ColumnAlignment::Left,
                        pulldown_cmark::Alignment::Center => ColumnAlignment::Center,
                        pulldown_cmark::Alignment::Right => ColumnAlignment::Right,
                    })
                    .collect()
            };
            let mut header: Vec<Vec<Inline>> = Vec::new();
            let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
            // Per-`<tr>` source-line tracking. `header_source_line` is the
            // `<thead><tr>` line; `row_source_lines` is parallel to `rows`.
            // Both stay empty / None when `line_ctx` is None.
            let mut header_source_line: Option<usize> = None;
            let mut row_source_lines: Vec<Option<usize>> = Vec::new();
            let track_lines = line_ctx.is_some();
            let mut current_row: Vec<Vec<Inline>> = Vec::new();
            let mut in_head = false;
            let mut in_body_row = false;
            let mut i = start + 1;
            while i < events.len() {
                match &events[i] {
                    // allow:math-events-ignored — structural walk over table
                    // section/row/cell boundaries; cell content is collected by
                    // the math-aware collect_inlines_until. Pinned by
                    // `math_survives_inside_a_table_cell`.
                    Event::End(TagEnd::Table) => break,
                    Event::Start(Tag::TableHead) => {
                        in_head = true;
                        // pulldown-cmark does NOT emit `Tag::TableRow` for the
                        // header row — it goes straight from `Tag::TableHead`
                        // to the cells. So we anchor the header `<tr>` line
                        // to the `TableHead` event itself (line of the
                        // markdown `| h |` row).
                        if track_lines {
                            header_source_line = line_ctx.and_then(|ctx| ctx.line_at_event(i));
                        }
                        i += 1;
                    }
                    Event::End(TagEnd::TableHead) => {
                        in_head = false;
                        i += 1;
                    }
                    Event::Start(Tag::TableRow) => {
                        in_body_row = true;
                        current_row = Vec::new();
                        if track_lines {
                            // pulldown-cmark only emits `TableRow` for body
                            // rows (header cells live directly inside
                            // `TableHead`). Always push to body lines here.
                            row_source_lines.push(line_ctx.and_then(|ctx| ctx.line_at_event(i)));
                        }
                        i += 1;
                    }
                    Event::End(TagEnd::TableRow) => {
                        if in_body_row {
                            rows.push(std::mem::take(&mut current_row));
                            in_body_row = false;
                        }
                        i += 1;
                    }
                    Event::Start(Tag::TableCell) => {
                        let (cell_inlines, end) = collect_inlines_until(events, i + 1, |e| {
                            matches!(e, Event::End(TagEnd::TableCell))
                        });
                        if in_head {
                            header.push(cell_inlines);
                        } else {
                            current_row.push(cell_inlines);
                        }
                        i = end + 1;
                    }
                    _ => i += 1,
                }
            }
            (
                Some(Block::Table {
                    header,
                    rows,
                    alignments,
                    header_source_line,
                    row_source_lines,
                }),
                i - start + 1,
            )
        }
        Tag::HtmlBlock => {
            let mut html = String::new();
            let mut i = start + 1;
            while i < events.len() {
                match &events[i] {
                    // allow:math-events-ignored — a raw HTML block is passed
                    // through verbatim; pulldown emits only Html/Text inside
                    // one, never a math event.
                    Event::End(TagEnd::HtmlBlock) => break,
                    Event::Html(s) | Event::Text(s) => html.push_str(s),
                    _ => {}
                }
                i += 1;
            }
            (Some(Block::Other(html)), i - start + 1)
        }
        // `[^label]: body`. pulldown emits this wherever the author wrote it,
        // including nested inside a blockquote or list item, so this arm is
        // reached from every block collector. Hoisting to the endnote section
        // is the renderer's job (ADR-035).
        Tag::FootnoteDefinition(label) => {
            let (children, end) = collect_blocks_until(events, start + 1, line_ctx, |e| {
                matches!(e, Event::End(TagEnd::FootnoteDefinition))
            });
            let label = label.to_string();
            (
                Some(Block::FootnoteDefinition { label, children }),
                end - start + 1,
            )
        }
        // Unmodeled containers: skip to End and emit nothing. The events
        // inside are dropped — anything moss cares about should be modeled
        // explicitly.
        _ => (None, 1),
    }
}

/// Decide whether a paragraph's inlines qualify for promotion to
/// [`Block::Figure`]. Per shape-spec § 1: exactly one [`Inline::Image`]
/// plus any number of whitespace-only [`Inline::Text`] /
/// [`Inline::LineBreak`] siblings. Any other inline shape (Emphasis,
/// Strong, Link, Code, non-whitespace Text, …) disqualifies the
/// paragraph and it stays as [`Block::Paragraph`].
///
/// **Empty-alt guard:** if the matched image has an empty alt (decorative
/// image), the paragraph is NOT promoted. This mirrors production's
/// `transform_events` implicit-figure pass which gates on non-empty alt
/// (a `<figure>` whose caption duplicates a missing alt would be useless
/// for assistive tech and adds visual noise). The empty-alt image stays
/// as `<p><img></p>`, matching the production byte shape for the same
/// input — verified via the parity probe's `other` category on 刘果 CJK
/// fixtures (image-only paragraphs with empty alt).
///
/// On qualification, returns `Ok(Block::Figure { image, caption })`. For a
/// standard-markdown image the caption renders the alt as INLINE MARKDOWN
/// (option B, matching Pandoc's implicit-figure model): `*em*`, links,
/// `` `code` `` and typeset math survive, built from the image's parsed
/// inline children (`events`/`para_start` re-parse the alt event span). The
/// `alt=` attribute stays the flat plain-text source. A plain-text alt (no
/// inline markup) keeps the flat single-[`Inline::Text`] caption, byte-
/// identical to before, so only captions that actually carry markup change.
///
/// On disqualification, returns `Err(original_inlines)` so the caller
/// can fall back to constructing the standard `Block::Paragraph` without
/// re-walking events.
fn try_promote_to_figure(
    mut inlines: Vec<Inline>,
    events: &[Event<'_>],
    para_start: usize,
) -> Result<Block, Vec<Inline>> {
    let mut image_count = 0;
    for inline in &inlines {
        match inline {
            Inline::Image { .. } => image_count += 1,
            Inline::Text(s) if s.trim().is_empty() => {} // whitespace OK
            Inline::LineBreak => {}                      // line break OK
            _ => return Err(inlines),
        }
    }
    if image_count != 1 {
        return Err(inlines);
    }

    // Non-image wikilink embeds never promote. pulldown-cmark parses every
    // `![[…]]` as an Image event, but Figure is an image concept: a video /
    // pdf / audio wikilink promoted here bypasses `dispatch_wikilink_embeds`
    // (which only dispatches Paragraph-shaped lone embeds), so its typed
    // synthesizer never runs and the page ships `<figure><img src="clip.mov">`
    // — a broken image. The gate keys off the same classifier the dispatcher
    // uses (`resolve::ext_kind`), so parse-time promotion and dispatch-time
    // synthesis cannot disagree about who owns the block. Extension-less
    // wikilinks (`![[draft|55%]]`) also stay Paragraph: only the with-graph
    // dispatcher can resolve their kind, and committing them to an image
    // Figure here would be a guess.
    if let Some(Inline::Image {
        src,
        is_wikilink: true,
        ..
    }) = inlines.iter().find(|i| matches!(i, Inline::Image { .. }))
    {
        let dest = match src {
            Url::Unresolved(s) => s.as_str(),
            Url::Resolved(r) => r.href.as_str(),
        };
        let ext = crate::path_ext::path_extension_lower(dest);
        if !matches!(
            crate::resolve::ext_kind::reference_kind_for_ext(&ext),
            crate::resolve::ext_kind::ExtKind::Image
        ) {
            return Err(inlines);
        }
    }

    // Probe the width + remaining alt on a BORROW first, so the empty-alt
    // guard can still return `Err(inlines)` with the original whitespace /
    // line-break siblings intact (production `<p><img>…</p>` parity).
    //
    // Standard-markdown images carry no structured pothole — a `|55%`/`|wide`
    // width rides in the raw alt text. Split it out so the figure carries the
    // width and the caption is the remaining alt.
    //
    // Wikilink images carry the raw pothole in `wikilink_pothole`; named width
    // tokens are already classified by `parse_pothole_params` (WidthToken arm),
    // but a content-relative percent (`55%`) is classified as `Alias` and
    // lands in `alt` (or is stripped from alt by our parser-level Alias fix).
    // Recover the percent from `wikilink_pothole` directly so the figure
    // carries the width on both the with-graph path (wikilink_dispatch) and
    // the no-graph path (fragment/test render with no ContentGraph).
    let mut figure_width: Option<String> = None;
    let mut rewritten_alt: Option<String> = None;
    match inlines.iter().find(|i| matches!(i, Inline::Image { .. })) {
        Some(Inline::Image {
            alt,
            is_wikilink: false,
            ..
        }) => {
            let (rest_alt, w) = crate::media::split_alt_width(alt);
            if w.is_some() {
                figure_width = w;
                rewritten_alt = Some(rest_alt);
            }
        }
        Some(Inline::Image {
            is_wikilink: true,
            wikilink_pothole,
            ..
        }) => {
            // Recover a content-relative percent from the raw pothole.
            // Named tokens are already absent from `alt` (WidthToken arm in
            // parse_pothole_params clears them); only the percent case falls
            // through as `Alias` and still needs extracting.
            // Sync: the with-graph twin lives in resolve/wikilink_dispatch.rs
            // (image branch, ~line 565) — both split width via media::split_alt_width.
            if let Some(pothole) = wikilink_pothole {
                let (remaining, w) = crate::media::split_alt_width(pothole);
                if w.is_some() {
                    figure_width = w;
                    // The remaining pothole (caption after stripping the %) is
                    // the intended caption; propagate it as the rewritten alt if
                    // the current alt is empty (percent-only pothole) or already
                    // stripped to the same value.
                    rewritten_alt = Some(remaining);
                }
            }
        }
        _ => {}
    }

    // The figure's caption text is the effective alt (width-stripped if a
    // width was present, else the raw alt), trimmed.
    let raw_alt = inlines.iter().find_map(|i| match i {
        Inline::Image { alt, .. } => Some(alt.as_str()),
        _ => None,
    });
    let alt_text = rewritten_alt
        .as_deref()
        .or(raw_alt)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Empty-alt guard: refuse to promote a decorative image (preserve the
    // original `<p><img></p>` shape with its whitespace siblings) — UNLESS it
    // carries a width, which needs a figure to hold the inline
    // `style="width:NN%"` / `data-width=`.
    if alt_text.is_empty() && figure_width.is_none() {
        return Err(inlines);
    }

    // Extract the single image, applying the width-stripped alt if any.
    let Some(image_pos) = inlines.iter().position(|i| matches!(i, Inline::Image { .. }))
    else {
        // `image_count == 1` was checked above, so this never fires. Handing the
        // inlines back is this function's own "can't promote" path — a better
        // failure than a panic if that invariant ever stops holding.
        return Err(inlines);
    };
    let mut image = inlines.swap_remove(image_pos);
    if let (Some(new_alt), Inline::Image { alt, .. }) = (rewritten_alt, &mut image) {
        *alt = new_alt;
    }

    // Caption. Empty alt yields None so no empty <figcaption> is emitted.
    // Otherwise, for a standard-markdown image, render the alt as inline
    // markdown (option B) — `*em*`, links, `` `code` ``, typeset math — built
    // from the image's parsed inline children. A wikilink image keeps its
    // flat pothole-derived caption (its alias is a literal string, not
    // markdown), and a plain-text alt keeps the flat single-Text caption so
    // the byte shape is unchanged for the common case.
    let caption = if alt_text.is_empty() {
        None
    } else {
        Some(build_caption_inlines(
            &image,
            events,
            para_start,
            alt_text,
            figure_width.is_some(),
        ))
    };

    Ok(Block::Figure {
        image,
        caption,
        width: figure_width,
        align: None,
        class_names: Vec::new(),
        img_style: None,
    })
}

/// Build the implicit-figure caption inlines for the promoted image.
///
/// Option B (matching Pandoc's implicit-figure model): a standard-markdown
/// image's caption is the alt CONTENT parsed as inline markdown — the typed
/// `Emphasis` / `Link` / `Code` / math nodes from the image's own event
/// span — so the renderer's hook-aware inline path (`render_inlines`)
/// emits `<em>`, `<a>`, and typeset math in the `<figcaption>`. The
/// `alt=` attribute (the `Inline::Image.alt` string) is untouched: it stays
/// the flat plain-text source (math as `$…$`) for assistive tech and
/// blocked-image fallback.
///
/// Falls back to the flat single-`Inline::Text` caption (byte-identical to
/// the pre-option-B shape) when:
/// - the image is a wikilink embed — its pothole alias is a literal
///   caption string by grammar, not markdown; and
/// - a width token was split out of the alt (`![cap|50%](p)`) — the raw
///   event span still contains the `|50%` text, so re-parsing it would
///   leak the width token into the caption.
fn build_caption_inlines(
    image: &Inline,
    events: &[Event<'_>],
    para_start: usize,
    alt_text: String,
    has_width: bool,
) -> Vec<Inline> {
    let is_wikilink = matches!(
        image,
        Inline::Image {
            is_wikilink: true,
            ..
        }
    );
    if is_wikilink || has_width {
        return vec![Inline::Text(alt_text)];
    }

    // Locate the image's own event span inside the paragraph:
    // Start(Tag::Image) … End(TagEnd::Image). The promotion invariant
    // guarantees exactly one image among the paragraph's inlines, so the
    // FIRST Start(Tag::Image) after `para_start` is that image's own start.
    //
    // Below, `collect_inlines_until` stops at the first End(TagEnd::Image)
    // its `is_end` check observes — but for a nested image
    // (`![a ![b](inner.png) c](outer.png)`, valid CommonMark) that is never
    // the inner image's own End: `parse_inline`'s `Tag::Image` arm
    // depth-tracks and fully consumes a nested inner image — including its
    // matching End — before returning control to this loop, the same way it
    // builds the depth-tracked `Inline::Image.alt` string. So the first End
    // this loop's `is_end` check actually sees is the OUTER image's own
    // close, and both surfaces (the flat alt string and this re-parsed
    // caption) agree on the span.
    let mut img_children_start: Option<usize> = None;
    let mut i = para_start + 1;
    while i < events.len() {
        // This arm set only LOCATES the image span (routes on event kind:
        // where Start(Image) is); it builds no output. The alt payload, math
        // included, is collected right below by the math-aware
        // collect_inlines_until/parse_inline, pinned by
        // implicit_figure_caption_carries_link_and_math_nodes.
        // allow:math-events-ignored — span locator, payload survives below.
        match &events[i] {
            Event::Start(Tag::Image { .. }) => {
                img_children_start = Some(i + 1);
                break;
            }
            Event::End(TagEnd::Paragraph) => break,
            _ => {}
        }
        i += 1;
    }
    let Some(children_start) = img_children_start else {
        // Defensive: no image span found (should be unreachable given the
        // promotion invariant) — keep the flat caption rather than guess.
        return vec![Inline::Text(alt_text)];
    };

    // Re-parse the alt event span through the SAME inline machinery as body
    // text, so `*em*` → Inline::Emphasis, `[l](/x)` → Inline::Link, and
    // `$x^2$` → the math Inline::Other node (which the renderer routes
    // through PipelineHooks::render_math for typesetting).
    let (mut caption, _end) = collect_inlines_until(events, children_start, |e| {
        matches!(e, Event::End(TagEnd::Image))
    });

    // A plain-text alt (every child is bare Text) keeps the flat trimmed
    // single-Text caption — byte-identical to the pre-option-B shape, so
    // only captions that actually carry markup change output.
    if caption.iter().all(|c| matches!(c, Inline::Text(_))) {
        return vec![Inline::Text(alt_text)];
    }

    // Trim the caption edges the way the flat path's `.trim()` did: leading
    // whitespace off the first Text node, trailing off the last, dropping
    // nodes that become empty.
    if let Some(Inline::Text(first)) = caption.first_mut() {
        *first = first.trim_start().to_string();
        if first.is_empty() {
            caption.remove(0);
        }
    }
    if let Some(Inline::Text(last)) = caption.last_mut() {
        *last = last.trim_end().to_string();
        if last.is_empty() {
            caption.pop();
        }
    }
    if caption.is_empty() {
        // Defensive: markup collapsed to nothing — fall back to the flat
        // alt so we never emit an empty <figcaption>.
        return vec![Inline::Text(alt_text)];
    }
    caption
}

/// Collect a contiguous run of inline events into `Vec<Inline>`. Stops
/// when `is_end(event)` returns true or events run out. Returns the
/// collected inlines and the end-event index.
fn collect_inlines_until<F>(events: &[Event<'_>], start: usize, is_end: F) -> (Vec<Inline>, usize)
where
    F: Fn(&Event<'_>) -> bool,
{
    let mut out: Vec<Inline> = Vec::new();
    let mut i = start;
    while i < events.len() {
        if is_end(&events[i]) {
            return (out, i);
        }
        let (inline, advance) = parse_inline(events, i);
        if let Some(node) = inline {
            out.push(node);
        }
        i += advance.max(1);
    }
    (out, i)
}

/// Parse one inline construct starting at `events[start]`.
fn parse_inline(events: &[Event<'_>], start: usize) -> (Option<Inline>, usize) {
    match &events[start] {
        Event::Text(t) => (Some(Inline::Text(t.to_string())), 1),
        Event::Code(c) => (Some(Inline::Code(c.to_string())), 1),
        // Phase 4 PR4.5 (2026-05-28): match pulldown-cmark's `push_html`
        // byte shape — SoftBreak emits `\n` between inline siblings, not a
        // space. The space form was a long-standing AST quirk surfaced
        // by Grid cells now flowing through the AST renderer; production
        // baselines (chps-site, SoCiviC, snapshot fixtures) preserve the
        // newline (e.g. `Flamboyan Theater · The Clemente\n107 Suffolk
        // Street`). Aligning here closes one row of the parity probe's
        // `whitespace_attribute_order` category.
        Event::SoftBreak => (Some(Inline::Text("\n".to_string())), 1),
        Event::HardBreak => (Some(Inline::LineBreak), 1),
        Event::Html(s) | Event::InlineHtml(s) => (Some(Inline::Other(s.to_string())), 1),
        // Math (ADR-030). Both are LEAF inline events carrying the raw TeX.
        // These arms are load-bearing: without them the two catch-alls below
        // return `(None, 1)` and every equation is silently deleted from the
        // document (`Energy $E = mc^2$.` → `<p>Energy .</p>`).
        //
        // P1 has no typesetting engine, so math renders as its own escaped
        // source — honest, never blank. `Inline::Other` is a RAW passthrough
        // at render time (render.rs), which is exactly why the escaping has
        // to happen HERE, at construction: the TeX is author input and is
        // full of `<`, `>` and `&`. ADR-030 §4 records why this rides
        // `Inline::Other` instead of a new `Inline::Math` variant (the enum
        // is published, serialized and not `#[non_exhaustive]`, so a variant
        // is a semver one-way door).
        Event::InlineMath(tex) => (Some(math_inline(tex, false)), 1),
        Event::DisplayMath(tex) => (Some(math_inline(tex, true)), 1),
        // `[^label]`. A LEAF event, same hazard as math: without this arm the
        // catch-all deletes the marker and the reader loses the pointer to
        // the note. pulldown only emits it when a matching definition exists,
        // so a bare `[^abc]` in prose stays literal text.
        Event::FootnoteReference(label) => (Some(Inline::FootnoteRef(label.to_string())), 1),
        // `[ ]` / `[x]` at the head of a task-list item. Another LEAF, same
        // hazard as the two above: no arm here and the checkbox disappears
        // while the item text survives, so the list silently loses its
        // meaning rather than looking broken.
        Event::TaskListMarker(checked) => (Some(Inline::TaskMarker(*checked)), 1),
        Event::Start(tag) => match tag {
            Tag::Emphasis => {
                let (children, end) = collect_inlines_until(events, start + 1, |e| {
                    matches!(e, Event::End(TagEnd::Emphasis))
                });
                (Some(Inline::Emphasis(children)), end - start + 1)
            }
            Tag::Strong => {
                let (children, end) = collect_inlines_until(events, start + 1, |e| {
                    matches!(e, Event::End(TagEnd::Strong))
                });
                (Some(Inline::Strong(children)), end - start + 1)
            }
            Tag::Strikethrough => {
                let (children, end) = collect_inlines_until(events, start + 1, |e| {
                    matches!(e, Event::End(TagEnd::Strikethrough))
                });
                (Some(Inline::Strikethrough(children)), end - start + 1)
            }
            Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            } => {
                let (children, end) = collect_inlines_until(events, start + 1, |e| {
                    matches!(e, Event::End(TagEnd::Link))
                });
                let title_opt = if title.is_empty() {
                    None
                } else {
                    Some(title.to_string())
                };
                // Phase 4 PR7a (2026-05-28): preserve pulldown-cmark's
                // `LinkType::WikiLink` discriminator on the typed AST so
                // the renderer can emit `class="wikilink"` and graph
                // builders can identify wikilink targets.
                let is_wikilink = matches!(*link_type, pulldown_cmark::LinkType::WikiLink { .. });
                (
                    Some(Inline::Link {
                        url: Url::unresolved(dest_url.to_string()),
                        title: title_opt,
                        children,
                        is_wikilink,
                    }),
                    end - start + 1,
                )
            }
            Tag::Image {
                link_type,
                dest_url,
                title,
                ..
            } => {
                // Collect alt text from text events between Start/End. A
                // nested image (`![a ![b](inner.png) c](outer.png)`, valid
                // CommonMark) emits its own Start/End(Image) pair inside this
                // span — depth-track so only the OUTER's own matching End
                // stops the loop; otherwise trailing content after the inner
                // image (here " c") escapes as a sibling paragraph inline
                // instead of folding into the outer alt, matching
                // `infra/newsletter.rs`'s `image_depth` counter on the email
                // side.
                let mut alt = String::new();
                let mut i = start + 1;
                let mut depth: u32 = 1;
                while i < events.len() {
                    match &events[i] {
                        Event::Start(Tag::Image { .. }) => depth += 1,
                        Event::End(TagEnd::Image) => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Event::Text(t) => alt.push_str(t),
                        Event::Code(c) => alt.push_str(c),
                        // `alt` is a plain-text attribute AND (via the
                        // implicit-figure path) the visible `<figcaption>`,
                        // so math is carried as its markdown source, not as
                        // the `<code>` node. Dropping it deleted the
                        // equation from both surfaces.
                        Event::InlineMath(t) => alt.push_str(&math_source(t, false)),
                        Event::DisplayMath(t) => alt.push_str(&math_source(t, true)),
                        // A line break inside alt is a SPACE — how browsers
                        // and Obsidian flatten it, and the rule
                        // `infra/newsletter.rs` already applies on the email
                        // side. Dropping the break ran a soft-wrapped
                        // sentence together (`Cover art\nby Jane` →
                        // `Cover artby Jane`) in the `alt=` attribute and, via
                        // the implicit-figure path, in the visible
                        // `<figcaption>`. pulldown hands the wrapped line's
                        // trailing spaces to the preceding Text run, so guard
                        // against emitting a second one.
                        Event::SoftBreak | Event::HardBreak => {
                            if !alt.is_empty() && !alt.ends_with(' ') {
                                alt.push(' ');
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                // PR3.5 (2026-05-28): for wikilink images (`![[file]]` /
                // `![[file|pothole]]`), pulldown-cmark synthesizes text
                // events that aren't always author-intended alt:
                //   - `![[logo.png]]` → text "logo.png" (synthesized from
                //     dest); production treats as empty alt.
                //   - `![[logo.png|contain center]]` → text "contain center"
                //     (display-attrs); production classifies as styling,
                //     NOT alt.
                //   - `![[logo.png|width=400]]` → text "width=400" (typed
                //     params); production classifies as params, NOT alt.
                //   - `![[logo.png|My caption]]` → text "My caption";
                //     genuine alt.
                //
                // Without this classification, PR3's Block::Figure
                // detection (Wave 1) promotes wikilink-image paragraphs
                // with synth-derived "alt" to Figure with bogus
                // figcaptions ("logo.png", "contain center"). Match
                // production's transform_events wikilink-dispatch by
                // running the same classifiers (`is_all_display_keywords`
                // + `parse_pothole_params`) here.
                //
                // PR7a-flip-core-B (2026-05-28): preserve the ORIGINAL
                // pothole text on `Inline::Image.wikilink_pothole`
                // BEFORE alt-classification consumes it.
                // `dispatch_wikilink_embeds` needs the raw pothole to
                // route `![[v.mp4|width=400]]` → typed video synth with
                // the `width=400` param intact (alt-classification would
                // erase it). The pothole is the substring after `|`;
                // pulldown-cmark gives us the synthesized text, so we
                // strip the dest synth case (text == dest_url ⇒ no
                // pothole) and otherwise carry the trimmed alt.
                let is_wikilink_image =
                    matches!(link_type, pulldown_cmark::LinkType::WikiLink { .. });
                let wikilink_pothole: Option<String> = if is_wikilink_image {
                    let dest_str: &str = dest_url;
                    let trimmed = alt.trim();
                    if trimmed.is_empty() || trimmed == dest_str {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                } else {
                    None
                };
                if is_wikilink_image {
                    let dest_str: &str = dest_url;
                    let trimmed = alt.trim().to_string();
                    if trimmed.is_empty() || trimmed == dest_str {
                        // Empty pothole OR pulldown-cmark synthesized
                        // dest_url as text → no author alt.
                        alt.clear();
                    } else if crate::media::is_all_display_keywords(&trimmed) {
                        // `contain center`, `left top`, etc. → display
                        // attrs (production maps to style), not alt.
                        alt.clear();
                    } else {
                        use crate::resolve::wikilink_dispatch::{
                            parse_pothole_params, PotholeContent,
                        };
                        match parse_pothole_params(&trimmed) {
                            PotholeContent::Empty | PotholeContent::Params(_) => {
                                alt.clear();
                            }
                            PotholeContent::WidthToken { rest_alias, .. } => {
                                alt = rest_alias;
                            }
                            PotholeContent::Alias(text) => {
                                // `parse_pothole_params` classifies a content-relative
                                // percent (e.g. `55%`) as `Alias` because it is not a
                                // named width token. Intercept it here: a bare percent
                                // is NOT a caption — strip it from the alt so it does
                                // not leak to `<figcaption>`. The actual width is
                                // recovered from `wikilink_pothole` by
                                // `dispatch_wikilink_embeds` (with-graph path) or
                                // directly from `split_alt_width` in the parser's
                                // `try_promote_to_figure` (no-graph path via `alt`).
                                //
                                // `split_alt_width` returns the remaining caption and
                                // the width token. If the whole alias was a width
                                // (nothing remaining), clear alt.
                                let (remaining, _w) = crate::media::split_alt_width(&text);
                                alt = remaining;
                            }
                        }
                    }
                }
                let title_opt = if title.is_empty() {
                    None
                } else {
                    Some(title.to_string())
                };
                (
                    Some(Inline::Image {
                        src: Url::unresolved(dest_url.to_string()),
                        alt,
                        title: title_opt,
                        is_wikilink: is_wikilink_image,
                        wikilink_pothole,
                    }),
                    i - start + 1,
                )
            }
            // Unmodeled inline container: skip to its End.
            _ => (None, 1),
        },
        // End / unhandled — caller handles.
        _ => (None, 1),
    }
}

/// Collect a contiguous run of block events into `Vec<Block>`. Stops when
/// `is_end(event)` returns true or events run out.
fn collect_blocks_until<F>(
    events: &[Event<'_>],
    start: usize,
    line_ctx: Option<&LineCtx<'_>>,
    is_end: F,
) -> (Vec<Block>, usize)
where
    F: Fn(&Event<'_>) -> bool,
{
    let mut out: Vec<Block> = Vec::new();
    let mut i = start;
    while i < events.len() {
        if is_end(&events[i]) {
            return (out, i);
        }
        let (block, advance) = parse_block(events, i, line_ctx);
        if let Some(b) = block {
            out.push(b);
        }
        i += advance.max(1);
    }
    (out, i)
}

/// Collect the children of a `Tag::Item` until the matching `End(Item)`.
///
/// Pulldown-cmark's **tight-list** mode emits item contents as inline
/// events (Text/Code/SoftBreak/inline-tag Start...) DIRECTLY inside
/// `Tag::Item` without wrapping in `Tag::Paragraph`. The plain
/// [`collect_blocks_until`] dispatcher would route those events through
/// [`parse_block`], which drops stray inlines — yielding empty `<li></li>`.
///
/// This helper preserves both modes:
/// - Inline events accumulate into a synthesized [`Block::Paragraph`] that
///   is flushed when a block-level event (Tag::Paragraph, Tag::List,
///   nested Tag::Item, etc.) appears or at the end of the item.
/// - Block-level events are parsed via [`parse_block_with_tag`] (the
///   standard path).
///
/// The renderer recognises a single-paragraph item shape and emits
/// `<li>...inline...</li>` without an inner `<p>`, matching production's
/// tight-list output byte-for-byte.
fn collect_item_blocks(
    events: &[Event<'_>],
    start: usize,
    line_ctx: Option<&LineCtx<'_>>,
) -> (Vec<Block>, usize) {
    let mut out: Vec<Block> = Vec::new();
    let mut pending_inlines: Vec<Inline> = Vec::new();
    let mut i = start;
    while i < events.len() {
        if matches!(&events[i], Event::End(TagEnd::Item)) {
            flush_pending_paragraph(&mut out, &mut pending_inlines);
            return (out, i);
        }
        if let Some((inline, advance)) = parse_inline_event(events, i) {
            if let Some(node) = inline {
                pending_inlines.push(node);
            }
            i += advance.max(1);
            continue;
        }
        // Block-level event: flush any accumulated inlines, then parse
        // through the standard dispatcher.
        flush_pending_paragraph(&mut out, &mut pending_inlines);
        let (block, advance) = parse_block(events, i, line_ctx);
        if let Some(b) = block {
            out.push(b);
        }
        i += advance.max(1);
    }
    flush_pending_paragraph(&mut out, &mut pending_inlines);
    (out, i)
}

/// Phase 4 PR4: detect a callout marker inside a blockquote and, if
/// found, assemble the entire `Block::Callout` (with body blocks).
///
/// `start` is the event index AFTER `Start(BlockQuote)`. Returns
/// `Some((Block::Callout, end_index))` where `end_index` is the event
/// index of the matching `End(TagEnd::BlockQuote(_))`, so the outer
/// caller can compute the advance. Returns `None` for plain
/// blockquotes (no `[!type]` marker on the first paragraph).
///
/// Detection rule (shape-spec § 1):
/// - The first event must be `Start(Tag::Paragraph)`.
/// - The leading `Event::Text` run (before the first `SoftBreak` or
///   any non-Text inline event) must match `[!<kind>]`, optionally
///   followed by `+` or `-` for foldable callouts, optionally followed
///   by space + inline title.
/// - The kind is canonicalized via [`CalloutKind::from_raw`]; unknown
///   kinds fall back to [`CalloutKind::Note`]. (Diagnostic threading
///   is a Phase 4 followup — `validation::Diagnostic` is scoped to
///   frontmatter validation today.)
///
/// Why detection runs on events (not parsed children): the inline
/// parser collapses `SoftBreak` events into `Inline::Text` (in PR4.5,
/// emitting `"\n"` to match pulldown-cmark's `push_html`), which makes
/// the marker-line vs body-line boundary an embedded `\n` rather than a
/// distinct AST node. Working at the event layer preserves the
/// SoftBreak boundary so we can split "title" (before SoftBreak) from
/// "body" (after SoftBreak) correctly.
fn detect_and_assemble_callout(
    events: &[Event<'_>],
    start: usize,
    line_ctx: Option<&LineCtx<'_>>,
) -> Option<(Block, usize)> {
    if !matches!(events.get(start), Some(Event::Start(Tag::Paragraph))) {
        return None;
    }
    // Coalesce the leading run of `Event::Text` into one logical
    // string. Stops at SoftBreak, HardBreak, any Start/End tag, or
    // any non-Text inline.
    //
    // Math events join the run as their markdown source. `Callout.title`
    // is a `String`, so source text is the only shape it can hold — and
    // breaking here instead would not merely drop the equation, it would
    // TRUNCATE the title at the first `$` and spill the remainder into the
    // callout body (`[!note] Energy $E=mc^2$ explained` → title "Energy ").
    // If a later phase needs a typed title, this is the line that has to
    // become `Vec<Inline>`.
    let mut leading = String::new();
    let mut i = start + 1;
    while let Some(event) = events.get(i) {
        match event {
            Event::Text(t) => {
                leading.push_str(t);
                i += 1;
            }
            Event::InlineMath(t) => {
                leading.push_str(&math_source(t, false));
                i += 1;
            }
            Event::DisplayMath(t) => {
                leading.push_str(&math_source(t, true));
                i += 1;
            }
            _ => break,
        }
    }
    if leading.is_empty() {
        return None;
    }

    let (raw_kind, fold, title, _marker_byte_len) = parse_callout_marker(&leading)?;
    let kind = CalloutKind::from_raw(raw_kind).unwrap_or(CalloutKind::Note);
    let title: Option<String> = title.map(|s| s.to_string()).filter(|s| !s.is_empty());

    // We've consumed the leading Text events. `i` now points at the
    // first non-Text event in the (still-open) marker paragraph.
    //
    // Three shapes from here:
    //   (A) SoftBreak / HardBreak → body lines continue in the same
    //       Paragraph. Skip the break, then collect inlines until
    //       End(Paragraph). Wrap them in a synthetic Block::Paragraph.
    //   (B) End(Paragraph) immediately → marker-only callout (no body
    //       in the marker paragraph). Skip End(Paragraph).
    //   (C) Another inline event (Start(Emphasis), Code, etc.) → the
    //       marker was actually followed by inline markup on the same
    //       line. Currently treated as title continuation — but we
    //       lack a clean event-level coalescer for inline tags, so we
    //       just collect remaining inlines and wrap them as a body
    //       paragraph. The author can use a separator paragraph for
    //       clarity if they want clean title isolation.
    let mut body_blocks: Vec<Block> = Vec::new();
    // This match chooses WHERE the callout body starts; it does not collect
    // content. A math event directly after the marker falls into the `_` arm,
    // which starts the body at `i` and hands it to the math-aware
    // collect_inlines_until. Pinned by
    // `callout_title_is_not_truncated_at_the_first_dollar`.
    // allow:math-events-ignored — see above.
    let body_paragraph_start: Option<usize> = match events.get(i) {
        Some(Event::SoftBreak) | Some(Event::HardBreak) => {
            // Skip the break; collect remaining inlines for the body
            // paragraph.
            Some(i + 1)
        }
        Some(Event::End(TagEnd::Paragraph)) => {
            // Marker was the entire paragraph. Skip past End.
            i += 1;
            None
        }
        _ => {
            // Other inline events directly following the marker —
            // collect them as body paragraph content. (Edge case;
            // see method comment.)
            Some(i)
        }
    };

    if let Some(body_start) = body_paragraph_start {
        // Collect inlines until End(Paragraph) and synthesize a
        // Block::Paragraph for the marker-paragraph body content.
        let (body_inlines, after_para) = collect_inlines_until(events, body_start, |e| {
            matches!(e, Event::End(TagEnd::Paragraph))
        });
        // Skip past End(Paragraph) itself.
        i = after_para + 1;
        // Trim leading whitespace-only Text inlines (e.g. if the
        // line-break Text(" ") leaks through).
        let trimmed_empty = body_inlines.iter().all(|x| match x {
            Inline::Text(t) => t.trim().is_empty(),
            _ => false,
        });
        if !trimmed_empty {
            body_blocks.push(Block::Paragraph(body_inlines));
        }
    }

    // Continue collecting subsequent blocks until End(BlockQuote).
    while let Some(event) = events.get(i) {
        if matches!(event, Event::End(TagEnd::BlockQuote(_))) {
            break;
        }
        let (block, advance) = parse_block(events, i, line_ctx);
        if let Some(b) = block {
            body_blocks.push(b);
        }
        i += advance.max(1);
    }

    // `i` now points at `End(BlockQuote)`. Return total event
    // span: outer caller computes `i - start + 1` (where `start` here
    // is the pre-Start-BlockQuote index in the outer scope; but we
    // were called with `start = outer_start + 1`, so the outer
    // caller's `start` correctly indexes the opening `Start(BlockQuote)`).
    // Per the call shape in `parse_block_with_tag` Tag::BlockQuote arm:
    //   `match detect_and_assemble_callout(events, start + 1)`
    //   `Some((block, body_end)) => (Some(block), body_end - start + 1)`
    // we must return `body_end = i` (the `End(BlockQuote)` index).
    let block = Block::Callout {
        kind,
        fold,
        title,
        children: body_blocks,
    };
    Some((block, i))
}

/// Parse the leading text of a callout-shaped paragraph.
///
/// Accepts text shaped like `[!kind] title text…`, `[!kind]+ title`,
/// `[!kind]-`, etc. Returns:
/// - `raw_kind` — the kind identifier verbatim (lowercased on
///   canonicalization, not here).
/// - `fold` — `Some(Fold::Open)` for `+`, `Some(Fold::Closed)` for `-`,
///   `None` otherwise.
/// - `title` — `Some(title_text)` when text follows the marker (space
///   separator consumed); `None` when the marker is the entire string.
///   Title may be empty (`""`) if author wrote `[!note] ` with trailing
///   whitespace only — caller treats empty as None.
/// - `marker_byte_len` — number of bytes from the start of `text` that
///   constituted the marker + the single separator space (if any). The
///   caller slices `&text[marker_byte_len..]` to recover trailing body
///   text that should stay in the paragraph (multi-line callouts where
///   pulldown-cmark concatenated lines).
fn parse_callout_marker(text: &str) -> Option<(&str, Option<Fold>, Option<&str>, usize)> {
    let after_open = text.strip_prefix("[!")?;
    let close_offset = after_open.find(']')?;
    let raw_kind = after_open.get(..close_offset)?;
    if raw_kind.is_empty() || raw_kind.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    // Offset within `text` immediately after the `]`.
    let after_bracket_offset = 2 + close_offset + 1;
    let rest = text.get(after_bracket_offset..)?;

    let (fold, after_fold_offset) = match rest.chars().next() {
        Some('+') => (Some(Fold::Open), after_bracket_offset + 1),
        Some('-') => (Some(Fold::Closed), after_bracket_offset + 1),
        _ => (None, after_bracket_offset),
    };

    let rest_after_fold = text.get(after_fold_offset..)?;
    let (title, marker_byte_len) = if rest_after_fold.is_empty() {
        // Marker only, no title segment.
        (None, after_fold_offset)
    } else if let Some(remainder) = rest_after_fold.strip_prefix(' ') {
        // ` title text…` — title is everything in this coalesced
        // leading-text string. Pulldown-cmark splits line breaks into
        // SoftBreak inlines, so this Text inline never contains
        // newlines; the title is bounded by the next non-Text inline.
        let title_str = remainder;
        let consumed = after_fold_offset + 1 + remainder.len();
        (Some(title_str), consumed)
    } else {
        // No separator after marker but more text follows (e.g.
        // `[!note]+body` with no space). Treat as no title; keep the
        // text intact.
        (None, after_fold_offset)
    };

    Some((raw_kind, fold, title, marker_byte_len))
}

/// If `events[i]` is an inline-level event, parse it via the existing
/// [`parse_inline`] machinery and return `(inline, advance)`. Returns
/// `None` for block-level events, end tags, or anything the inline
/// dispatcher doesn't own — letting the caller fall back to the block
/// path.
fn parse_inline_event(events: &[Event<'_>], i: usize) -> Option<(Option<Inline>, usize)> {
    match &events[i] {
        Event::Text(_)
        | Event::Code(_)
        | Event::Html(_)
        | Event::InlineHtml(_)
        | Event::SoftBreak
        | Event::HardBreak
        // Math and footnote markers are inline LEAVES. This whitelist is the
        // ONLY way they reach `parse_inline` from `collect_item_blocks` (its
        // sole caller), so omitting one deletes it in LIST ITEMS while the
        // same construct in a paragraph still looks fine — a wiring failure a
        // mechanism test cannot see. Table cells/blockquotes take other
        // routes (tests/math_parsing.rs). The Start-tag arm below carries the
        // same obligation for inline CONTAINERS, where a miss does worse than
        // delete: the unknown tag flushes the pending inlines, splitting one
        // item into two paragraphs (measured on `- ~~gone~~ stays`, which
        // rendered `<li><p>gone</p><p> stays</p></li>`).
        | Event::InlineMath(_)
        | Event::DisplayMath(_)
        | Event::FootnoteReference(_)
        // Task markers reach the AST ONLY through this arm — they occur
        // exclusively inside `Tag::Item`, whose content is collected by
        // `collect_item_blocks`, whose sole inline route is this function.
        // Omitting it deletes every checkbox in the document.
        | Event::TaskListMarker(_) => Some(parse_inline(events, i)),
        Event::Start(tag) => match tag {
            Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Link { .. }
            | Tag::Image { .. } => Some(parse_inline(events, i)),
            _ => None,
        },
        _ => None,
    }
}

/// Drain `pending_inlines` into a [`Block::Paragraph`] appended to `out`,
/// unless it's empty. No-op when there are no pending inlines.
fn flush_pending_paragraph(out: &mut Vec<Block>, pending_inlines: &mut Vec<Inline>) {
    if !pending_inlines.is_empty() {
        out.push(Block::Paragraph(std::mem::take(pending_inlines)));
    }
}

/// Post-parse pass: disambiguate duplicate heading IDs by appending `-1`,
/// `-2`, … to the slug, in the order the headings will appear ON THE PAGE.
///
/// Mirrors the `id_counts: HashMap<String, usize>` behavior at
/// `src-tauri/src/build/markdown/pipeline.rs:1798-1805`:
///
/// - First occurrence of slug `foo` keeps id `foo`; counter starts at 1.
/// - Second occurrence becomes `foo-1`; counter becomes 2.
/// - Third occurrence becomes `foo-2`; counter becomes 3.
///
/// **Render order, not source order.** [`super::render::render_document`]
/// emits the body first — the FIRST definition of every footnote label
/// emitting nothing, because [`super::footnotes::render_section`] hoists it
/// to the end of the page — and then the endnote section, in first-reference
/// order (a third ordering, agreeing with neither of the other two).
/// Numbering in source order therefore handed the bare slug to a heading
/// that renders LAST: `[^a]: ## Notes` written above a body `## Notes`
/// published `id="notes"` inside the endnote list and `id="notes-1"` on the
/// visible section, while `[[Note#Notes]]` — slugged from heading text with
/// no counter — kept pointing at `#notes`.
///
/// Headings whose base slug is `None` (shouldn't happen post-PR2, but
/// safe-guarded) are left untouched.
fn assign_heading_id_suffixes(blocks: &mut [Block]) {
    // Built BEFORE the mutable walk: `entries()` is the order
    // `render_section` emits the `<li id="fn-N">`s, and it needs a shared
    // borrow of the same tree.
    let note_order: Vec<String> = FootnoteIndex::build(blocks)
        .entries()
        .iter()
        .map(|(_, label)| label.clone())
        .collect();

    let mut body: Vec<&mut Option<String>> = Vec::new();
    let mut notes: Vec<(String, Vec<&mut Option<String>>)> = Vec::new();
    let mut hoisted: HashSet<String> = HashSet::new();
    let scope = HoistScope {
        document_notes: &note_order,
        in_shortcode: false,
    };
    collect_heading_id_slots(blocks, &mut body, &mut hoisted, &mut notes, scope);

    // Stable sort into endnote order. A label the index doesn't own is
    // impossible today (every defined label is numbered); if one ever
    // appears it keeps its document position, at the end.
    notes.sort_by_key(|(label, _)| {
        note_order
            .iter()
            .position(|l| l == label)
            .unwrap_or(usize::MAX)
    });

    let mut id_counts: HashMap<String, usize> = HashMap::new();
    for slot in body {
        disambiguate_heading_id(slot, &mut id_counts);
    }
    for (_, slots) in notes {
        for slot in slots {
            disambiguate_heading_id(slot, &mut id_counts);
        }
    }
}

/// Apply the `-N` suffix rule to one heading id slot.
fn disambiguate_heading_id(id: &mut Option<String>, id_counts: &mut HashMap<String, usize>) {
    let Some(slug) = id else { return };
    let count = *id_counts.entry(slug.clone()).or_insert(0);
    id_counts.insert(slug.clone(), count + 1);
    if count > 0 {
        let suffixed = format!("{slug}-{count}");
        *id = Some(suffixed);
    }
}

/// Collect a mutable handle to every heading id in the tree, bucketed by
/// WHERE the renderer puts it: `body` in document order, and one bucket per
/// hoisted footnote definition so the caller can order those the way
/// `render_section` emits them.
///
/// `hoisted` records which labels have had their first definition seen, in
/// document order — the same rule [`super::footnotes::FootnoteIndex::definition`]
/// uses to pick the definition whose body the endnote renders. A REPEAT
/// definition of a label is not hoisted (it renders in place), so its
/// headings stay in the surrounding bucket.
/// Where the walk currently is, for deciding whether a footnote definition
/// will be HOISTED to the endnote section (so its headings are numbered after
/// the whole body) or rendered IN PLACE (so they are numbered where they sit).
///
/// Two independent reasons a definition is not hoisted, and both must be
/// checked or the id order inverts against the DOM:
///
/// 1. The document index doesn't own the label. `footnotes::is_hoisted`
///    answers from a map built only from the index's entries, and that index
///    is exactly `document_notes`. A repeat definition of an already-defined
///    label is left in place by the renderer.
/// 2. The walk is inside a shortcode body. `footnotes::collect_definitions`
///    stops at shortcode bodies, so a `[^x]: …` written inside a `:::grid`
///    cell is never collected, never numbered, and never hoisted — it renders
///    in the cell. Bucketing it as an endnote numbered a grid heading after
///    the body even though it renders before it.
#[derive(Clone, Copy)]
struct HoistScope<'a> {
    /// Labels the document's `FootnoteIndex` owns, in endnote order.
    document_notes: &'a [String],
    /// Whether the walk is inside a `:::grid` cell or `:::hero` overlay.
    in_shortcode: bool,
}

impl HoistScope<'_> {
    /// Whether a definition of `label` here becomes an endnote. Consumes the
    /// first-wins claim on `hoisted` only when it genuinely hoists, so a
    /// repeat definition later in the body is still judged on its own terms.
    fn hoists(&self, label: &str, hoisted: &mut HashSet<String>) -> bool {
        if self.in_shortcode {
            return false;
        }
        if !self.document_notes.iter().any(|l| l == label) {
            return false;
        }
        hoisted.insert(label.to_string())
    }

    /// The same scope, entering a shortcode body.
    fn inside_shortcode(self) -> Self {
        Self {
            in_shortcode: true,
            ..self
        }
    }
}

fn collect_heading_id_slots<'a>(
    blocks: &'a mut [Block],
    sink: &mut Vec<&'a mut Option<String>>,
    hoisted: &mut HashSet<String>,
    notes: &mut Vec<(String, Vec<&'a mut Option<String>>)>,
    scope: HoistScope<'_>,
) {
    for block in blocks.iter_mut() {
        match block {
            Block::Heading { id, .. } => sink.push(id),
            Block::FootnoteDefinition { label, children } => {
                let label = label.clone();
                // A definition is bucketed as an endnote only when the
                // renderer will actually hoist it — `footnotes::is_hoisted`
                // only ever hoists labels the document index owns, and the
                // index is exactly `document_notes`. Deciding from `hoisted` alone put
                // a definition the renderer leaves IN PLACE into a bucket
                // that is numbered after the whole body, so a heading that
                // renders first was numbered last.
                if scope.hoists(&label, hoisted) {
                    let mut note_sink: Vec<&'a mut Option<String>> = Vec::new();
                    collect_heading_id_slots(children, &mut note_sink, hoisted, notes, scope);
                    notes.push((label, note_sink));
                } else {
                    collect_heading_id_slots(children, sink, hoisted, notes, scope);
                }
            }
            // Every container whose children render into the SAME page
            // shares one id counter, or the page emits duplicate DOM ids and
            // `#slug` resolves to whichever copy the browser meets first.
            // LinkCard counts: it is the compound-link grid cell, and its
            // children include headings.
            Block::BlockQuote(children)
            | Block::Callout { children, .. }
            | Block::LinkCard { children, .. } => {
                collect_heading_id_slots(children, sink, hoisted, notes, scope);
            }
            Block::List { items, .. } => {
                for item in items.iter_mut() {
                    collect_heading_id_slots(item, sink, hoisted, notes, scope);
                }
            }
            Block::Shortcode(sc) => match sc {
                // Grid cells and the Hero overlay are typed `Vec<Block>`
                // (PR4.5) rendered into this page. Each is parsed by a
                // RECURSIVE `parse_fragment_with_config`, which is told to
                // skip `assign_heading_id_suffixes` precisely so they arrive
                // here holding un-disambiguated base slugs — this walk is the
                // page's only numbering pass. When the nested parse numbered
                // too, a cell with two `## Notes` arrived as `notes` /
                // `notes-1` and got suffixed a second time, yielding a
                // duplicate `notes-1` and an impossible `notes-1-1`.
                Shortcode::Grid(args) => {
                    for cell in args.cells.iter_mut() {
                        collect_heading_id_slots(cell, sink, hoisted, notes, scope.inside_shortcode());
                    }
                }
                Shortcode::Hero(args) => {
                    collect_heading_id_slots(&mut args.overlay, sink, hoisted, notes, scope.inside_shortcode());
                }
                // No block children: Subscribe/Apply bodies must be empty,
                // Buttons/Gallery carry typed item lists, and Recent's
                // fallback is unparsed markdown.
                Shortcode::Subscribe(_)
                | Shortcode::Buttons(_)
                | Shortcode::Gallery(_)
                | Shortcode::Recent(_)
                | Shortcode::Apply(_) => {}
            },
            // Deliberately exhaustive, no `_` arm: these carry no block
            // children, and the next `Block` variant that gains a
            // `Vec<Block>` must fail to compile HERE rather than silently
            // leak duplicate ids onto the page.
            Block::Paragraph(_)
            | Block::CodeBlock { .. }
            | Block::Table { .. }
            | Block::ThematicBreak
            | Block::Figure { .. }
            | Block::Other(_) => {}
        }
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
