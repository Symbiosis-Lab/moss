//! The emitted body, segmented at the structural boundaries a later phase
//! needs — recorded BY the serializer, never discovered by scanning it.
//!
//! # Why this exists
//!
//! Two structural decisions about a page's body cannot be made while that page
//! renders, because both need facts that only exist once every page has parsed:
//!
//! - **Grid cells.** `:::grid` cells that link to a collection become cards
//!   showing the LINKED page's title, cover and child count.
//! - **The folder-cover column.** A `cover:`-bearing folder home renders
//!   "book-open": cover media on the left, the lede in a narrow column on the
//!   right. Everything past the lede has to be released back to full width, as
//!   a sibling AFTER the cover row.
//!
//! Before ADR-034 both were answered by re-parsing the page's own rendered HTML
//! with regexes and a hand-rolled `<div>`-depth counter. That is what produced
//! moss#903: a byte cursor advanced past a multi-byte character aborted the
//! build (`start byte index 66 is not a char boundary; it is inside '在'`), and
//! the release point could only ever be a literal `.moss-grid` match, so a
//! long-form article with no grid stayed in the ~20-character column forever.
//!
//! [`render_segmented`] fixes both at the root: it serializes the typed
//! `Document` one top-level block at a time and keeps the pieces. The
//! structural decisions are then made on typed [`Block`]s
//! ([`lede_end`], [`crate::build::render::grid_cells`]) and the string work is
//! reduced to concatenating pieces that were well-formed the moment they were
//! emitted. No cursor to walk, no tags to re-match, no balance to guess.
//!
//! # Invariant
//!
//! `BodyPlan::to_html()` is byte-identical to
//! `moss_core::ast::render_document(doc, hooks)` for the same inputs. This is
//! true by construction (the plan holds exactly the renderer's output, cut at
//! block boundaries) and is the falsifier the whole design rests on — see
//! `body_plan_tests::plan_reassembles_to_render_document_output`.

use moss_core::ast::{
    Block, Document, GridCellParts, GridParts, GridShortcode, RenderHooks, Shortcode,
    SubscribeShortcode,
};

/// A page's emitted body, kept in the pieces the serializer produced it in.
#[derive(Debug, Clone, Default)]
pub struct BodyPlan {
    /// Segments in emission order. Concatenating every segment's HTML
    /// reproduces the whole body.
    pub segments: Vec<BodySegment>,
    /// How many LEADING segments belong inside the narrow folder-cover column.
    /// `segments.len()` means "the whole body stays in the column" (the
    /// behavior for a page whose body never reaches a release point).
    ///
    /// A segment index rather than a byte offset on purpose: each side of the
    /// split is serialized independently, so neither can end mid-element and
    /// the question `html_prefix_is_balanced` existed to answer never arises.
    pub lede_segments: usize,
}

/// One piece of the emitted body.
#[derive(Debug, Clone)]
pub enum BodySegment {
    /// Verbatim HTML for one or more consecutive top-level blocks.
    Html(String),
    /// One `:::grid` block, kept typed so cell enhancement is a match on
    /// `Vec<Block>` rather than a pattern-match on markup.
    Grid(GridEmission),
    /// One inline `:::subscribe` form, kept typed because its hidden `scope`
    /// depends on the site's language sections, which exist only after every
    /// page has parsed. `email::stamp_inline_subscribe_scopes` re-renders it.
    Subscribe(SubscribeEmission),
}

/// An inline `:::subscribe` form as its typed arguments and the HTML they
/// rendered to.
#[derive(Debug, Clone)]
pub struct SubscribeEmission {
    pub args: SubscribeShortcode,
    pub html: String,
}

/// A `:::grid` block as both the HTML that was emitted and the typed cells it
/// was emitted from.
#[derive(Debug, Clone)]
pub struct GridEmission {
    /// The opener the serializer wrote, reused verbatim so `data-columns`, the
    /// ratio `style=`, `data-width` and `data-source-range` survive any cell
    /// substitution.
    pub open_tag: String,
    /// The author's `{.class}` list, read from the typed shortcode args. This
    /// is how the `no-cards` opt-out is detected — not by substring-matching
    /// the serialized opening tag.
    pub classes: String,
    pub cells: Vec<GridCellEmission>,
}

/// One grid cell: the classifier's typed input plus the pieces it serialized
/// to.
#[derive(Debug, Clone)]
pub struct GridCellEmission {
    /// The cell's typed content — the ONLY input to cell classification.
    /// Emptied by [`replace`], so a pass can never re-classify markup an
    /// earlier pass generated as if it were the author's content.
    ///
    /// [`replace`]: GridCellEmission::replace
    pub blocks: Vec<Block>,
    /// The cell's content and its card-chrome decision, as the serializer
    /// produced them. Used verbatim whenever the cell is not replaced, so a
    /// grid that resolves to nothing is byte-identical to an unenhanced render.
    pub parts: GridCellParts,
}

impl GridCellEmission {
    /// The cell's rendered content without its card chrome.
    ///
    /// A pass that re-wraps the cell — an internal-link cell becomes one big
    /// `<a class="moss-grid-card">` — needs the content, not the wrapper.
    pub fn inner(&self) -> &str {
        &self.parts.inner
    }

    /// Swap this cell for final markup that brings its own chrome (a
    /// collection card, a link preview).
    pub fn replace(&mut self, html: String) {
        self.blocks.clear();
        self.parts = GridCellParts::final_markup(html);
    }
}

impl GridEmission {
    /// The grid's HTML as the serializer emitted it, including the trailing
    /// newline `render_block`'s `Shortcode` arm appends.
    pub fn to_html(&self) -> String {
        let cells: Vec<String> = self.cells.iter().map(|c| c.parts.to_html()).collect();
        format!("{}\n", GridParts::assemble(&self.open_tag, &cells))
    }

    /// True when the author opted this grid out of collection-card conversion
    /// with `{.no-cards}`. Authors use it for navigation grids and
    /// mixed-content splits whose cells happen to contain internal links.
    ///
    /// Read from the typed class list, so a class that merely *contains* the
    /// token (`.no-cards-please`) does not opt out — which the old
    /// `grid_opener.contains("no-cards")` substring test could not tell apart.
    pub fn no_cards(&self) -> bool {
        self.classes.split_whitespace().any(|c| c == "no-cards")
    }

    /// True when the author asked for SUMMARY cards with `{.summary}`.
    ///
    /// Read the same way `no_cards` is — token-exact, off the typed class
    /// list — because fence classes are one vocabulary, not a grab bag of
    /// ad-hoc string tests. The variant replaces the grid's whole container
    /// (see `render::grid_cells::apply_summary_grids`), so nothing downstream
    /// reads this off the emitted opener.
    pub fn summary(&self) -> bool {
        self.classes.split_whitespace().any(|c| c == "summary")
    }
}

impl BodySegment {
    /// This segment's HTML.
    pub fn html(&self) -> std::borrow::Cow<'_, str> {
        match self {
            BodySegment::Html(s) => std::borrow::Cow::Borrowed(s),
            BodySegment::Grid(g) => std::borrow::Cow::Owned(g.to_html()),
            BodySegment::Subscribe(s) => std::borrow::Cow::Borrowed(&s.html),
        }
    }
}

impl BodyPlan {
    /// A plan for a body that was rendered elsewhere (a synthesized page, a
    /// fragment render, a test). One opaque segment, nothing to release.
    pub fn opaque(html: String) -> Self {
        Self {
            segments: vec![BodySegment::Html(html)],
            lede_segments: 1,
        }
    }

    /// The whole body.
    pub fn to_html(&self) -> String {
        let mut out = String::new();
        for seg in &self.segments {
            out.push_str(&seg.html());
        }
        out
    }

    /// Split the body into `(lede, rest)` at [`lede_segments`].
    ///
    /// Both sides are whole segments, so neither can end inside an element and
    /// `lede + rest == to_html()` always. That is the property the deleted
    /// `html_prefix_is_balanced` byte-walker existed to guess at.
    ///
    /// [`lede_segments`]: BodyPlan::lede_segments
    pub fn split_at_lede(&self) -> (String, String) {
        let join = |segs: &[BodySegment]| -> String {
            segs.iter().map(|s| s.html().into_owned()).collect()
        };
        let cut = self.lede_segments.min(self.segments.len());
        (join(&self.segments[..cut]), join(&self.segments[cut..]))
    }

    /// Rewrite every emitted piece with `f`.
    ///
    /// Used for the per-page HTML fixups that run after rendering
    /// (`sweep_unresolved_hrefs`, `adjust_relative_paths_for_pretty_urls`).
    /// Both rewrite an attribute value inside a single element, so applying
    /// them piecewise is equivalent to applying them to [`to_html`] — and it
    /// keeps the pieces authoritative instead of letting a second flat string
    /// drift away from them.
    ///
    /// [`to_html`]: BodyPlan::to_html
    pub fn map_html(&mut self, f: &dyn Fn(&str) -> String) {
        for seg in &mut self.segments {
            match seg {
                BodySegment::Html(s) => *s = f(s),
                BodySegment::Grid(g) => {
                    g.open_tag = f(&g.open_tag);
                    for cell in &mut g.cells {
                        cell.parts.inner = f(&cell.parts.inner);
                    }
                }
                BodySegment::Subscribe(s) => s.html = f(&s.html),
            }
        }
    }

    /// Every inline subscribe form in the body, for the Reduce pass that
    /// stamps their scope.
    pub fn subscribe_forms_mut(&mut self) -> impl Iterator<Item = &mut SubscribeEmission> {
        self.segments.iter_mut().filter_map(|s| match s {
            BodySegment::Subscribe(e) => Some(e),
            _ => None,
        })
    }

    /// Put `html` in front of the body, inside the cover column.
    ///
    /// The one caller is the auto-injected `<h1 class="moss-article-title">`,
    /// which only fires on article pages — pages that never take the cover
    /// branch. `lede_segments` still moves so the two features stay composable.
    pub fn prepend_html(&mut self, html: String) {
        self.segments.insert(0, BodySegment::Html(html));
        self.lede_segments += 1;
    }
}

/// Index into `blocks` where the narrow folder-cover column ends.
///
/// The cover thumbnail sits beside the *lede*; content past it has half the
/// viewport empty to its right, which is exactly what moss#903 bug 4 reported.
/// Release happens at whichever comes first:
///
/// - a block that is inherently full-width — `:::grid`, `:::gallery`, a table,
///   or a figure carrying a non-`body` width token (ADR-021);
/// - the end of the lede — the first heading that follows at least one
///   paragraph, i.e. where the intro stops and the article proper starts.
///
/// `blocks.len()` when neither is found: a body that is nothing but a short
/// intro belongs beside the cover in its entirety.
///
/// A body with no headings, no tables and no shortcodes stays in the column;
/// closing that case needs a length-based release point, which would change
/// existing sites' typography and wants a visual review first.
pub fn lede_end(blocks: &[Block]) -> usize {
    let mut seen_paragraph = false;
    for (i, block) in blocks.iter().enumerate() {
        if is_full_width_block(block) {
            return i;
        }
        match block {
            Block::Heading { .. } if seen_paragraph => return i,
            Block::Paragraph(_) => seen_paragraph = true,
            _ => {}
        }
    }
    blocks.len()
}

/// The nearest heading's text before index `i` in `blocks` — the accessible
/// name an unlabeled `:::grid {scroll}` row falls back to (a row directly
/// under `## Related` is named "Related"). Scans backward and stops at the
/// FIRST heading found, same parent list only: a grid nested inside a
/// callout or list item never reaches this function, since `render_segmented`
/// only special-cases `Shortcode::Grid` at the top level. `None` when nothing
/// precedes `i`, when the nearest preceding block isn't a heading, or when
/// the nearest heading is empty — never keeps looking past it for an older
/// one, or "nearest" would be a lie.
fn nearest_heading_label(preceding: &[Block]) -> Option<String> {
    for block in preceding.iter().rev() {
        if let Block::Heading { children, .. } = block {
            let text = moss_core::ast::inlines_to_plain_text(children);
            return if text.is_empty() { None } else { Some(text) };
        }
    }
    None
}

/// Render a `:::grid`'s parts, falling back to the nearest preceding
/// heading's text as the scroll row's accessible name when the author wrote
/// no explicit `label=` — see [`nearest_heading_label`]. `args` is cloned
/// only on this rare path (an actually-scrolling row, no author label, a
/// heading to borrow from); every other grid — the vast majority — renders
/// through the caller's own reference with no allocation. Kept to a
/// borrowed-`label` swap rather than a new `render_grid_parts` parameter: the
/// trait is public (moss-core is the MIT-licensed open half), and every other
/// caller of [`RenderHooks::render_grid_parts`] already has no sibling blocks
/// in view, so a parameter only this one caller could ever fill is a
/// signature every implementor pays for and none but this one uses.
fn grid_parts_with_heading_fallback<H: RenderHooks + ?Sized>(
    hooks: &H,
    args: &GridShortcode,
    source_line: Option<usize>,
    preceding: &[Block],
) -> GridParts {
    if args.scrolls() && args.label.is_none() {
        if let Some(label) = nearest_heading_label(preceding) {
            let named = GridShortcode { label: Some(label), ..args.clone() };
            return hooks.render_grid_parts(&named, source_line);
        }
    }
    hooks.render_grid_parts(args, source_line)
}

/// Blocks whose rendering assumes the full content column.
fn is_full_width_block(block: &Block) -> bool {
    match block {
        Block::Shortcode(Shortcode::Grid(_)) | Block::Shortcode(Shortcode::Gallery(_)) => true,
        Block::Table { .. } => true,
        // ADR-021 width tokens: `body` is the default measure, so only a
        // widened figure forces a release.
        Block::Figure { width: Some(w), .. } => w != "body",
        _ => false,
    }
}

/// Serialize `doc` into a [`BodyPlan`].
///
/// Byte-identical to `moss_core::ast::render_document(doc, hooks)` when
/// re-concatenated: every block goes through the same
/// `render_block_with_meta`, sharing one [`FootnoteCtx`] across the walk so
/// numbering/hoisting stays document-wide, and `:::grid` blocks go through
/// the same `RenderHooks::render_grid_parts` the flat `Grid` arm uses.
pub fn render_segmented<H: RenderHooks + ?Sized>(doc: &Document, hooks: &H) -> BodyPlan {
    let lede_end_block = lede_end(&doc.blocks);
    let mut segments: Vec<BodySegment> = Vec::new();
    let mut buf = String::new();
    let mut lede_segments: Option<usize> = None;
    let mut fnotes = moss_core::ast::footnotes::FootnoteCtx::for_document(&doc.blocks);

    for (i, block) in doc.blocks.iter().enumerate() {
        if i == lede_end_block {
            flush(&mut buf, &mut segments);
            lede_segments = Some(segments.len());
        }
        let meta = doc.block_meta.get(i).copied().unwrap_or_default();
        match block {
            Block::Shortcode(Shortcode::Grid(args)) => {
                flush(&mut buf, &mut segments);
                let parts =
                    grid_parts_with_heading_fallback(hooks, args, meta.source_line, &doc.blocks[..i]);
                segments.push(BodySegment::Grid(emission(args, parts)));
            }
            Block::Shortcode(Shortcode::Subscribe(args)) => {
                flush(&mut buf, &mut segments);
                let mut html = String::new();
                moss_core::ast::render_block_with_meta(hooks, &mut html, block, &meta, &mut fnotes);
                segments.push(BodySegment::Subscribe(SubscribeEmission { args: args.clone(), html }));
            }
            _ => moss_core::ast::render_block_with_meta(hooks, &mut buf, block, &meta, &mut fnotes),
        }
    }
    // Flush and resolve the lede cut BEFORE rendering the footnote section: on
    // a short body that never reaches `lede_end_block` (no heading/grid/table
    // release point), `lede_segments` is still `None` here. Resolving it
    // against `segments.len()` after appending footnotes to `buf` would count
    // the about-to-be-flushed footnote segment as part of the lede, landing
    // the endnote list inside the narrow cover-body column. Flushing first
    // fixes `lede_segments` at the tail of real content; the footnote section
    // then always lands in its own segment, in the trailer.
    flush(&mut buf, &mut segments);
    let lede_segments = lede_segments.unwrap_or(segments.len());
    moss_core::ast::footnotes::render_section(hooks, &mut buf, &doc.blocks, &mut fnotes);
    flush(&mut buf, &mut segments);

    BodyPlan {
        segments,
        lede_segments,
    }
}

fn flush(buf: &mut String, segments: &mut Vec<BodySegment>) {
    if !buf.is_empty() {
        segments.push(BodySegment::Html(std::mem::take(buf)));
    }
}

/// Pair each typed cell with the HTML it serialized to.
///
/// `GridParts::cells` is produced by iterating `args.cells` in order, so the
/// two vectors are parallel by construction. A length mismatch would mean
/// moss-core's grid serializer stopped being one-HTML-string-per-typed-cell;
/// the `zip` degrades to the shorter side rather than panicking on a
/// production build, and the debug assert makes it loud in development.
fn emission(args: &GridShortcode, parts: GridParts) -> GridEmission {
    debug_assert_eq!(
        args.cells.len(),
        parts.cells.len(),
        "grid serializer must emit one HTML string per typed cell"
    );
    GridEmission {
        open_tag: parts.open_tag,
        classes: args.classes.clone(),
        cells: args
            .cells
            .iter()
            .zip(parts.cells)
            .map(|(blocks, parts)| GridCellEmission {
                blocks: blocks.clone(),
                parts,
            })
            .collect(),
    }
}

#[cfg(test)]
#[path = "body_plan_tests.rs"]
mod tests;
