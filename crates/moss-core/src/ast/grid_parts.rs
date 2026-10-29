//! A `:::grid` serialized into its structural pieces.
//!
//! The flat `Grid` arm of [`RenderHooks::render_shortcode`] is
//! [`GridParts::to_html`] over this value, so there is exactly ONE grid byte
//! shape and the split form can never drift from the flat one.
//!
//! The split form exists because a structural decision about a grid *cell* —
//! "is this cell a link to a collection, and if so what is that collection's
//! title, cover and child count?" — depends on facts no single page can see.
//! Previously the host answered it by regex-scraping its own emitted
//! markup; that scraping is what aborted the build when a CJK character sat in
//! front of a `:::grid`. With the pieces kept separate the
//! host pairs each cell's HTML with the typed [`Block`] cell it came from, so
//! recognition reads exactly what emission read.

use super::hooks::{collapse_tag_adjacent_newlines, escape_attr, RenderHooks};
use super::node::{Block, Inline};
use super::shortcode::GridShortcode;
use crate::inert_regions::{tag_end, VOID_ELEMENTS};

/// A `:::grid` block's serialized pieces: the opening `<div class="moss-grid"
/// …>` tag and one [`GridCellParts`] per cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridParts {
    /// Carries the author's classes, the ratio `style=`, `data-width`, and
    /// `data-source-range`.
    pub open_tag: String,
    /// One entry per [`GridShortcode::cells`] entry, in the same order.
    pub cells: Vec<GridCellParts>,
    /// `{rows=equal}`: the parts of the cards line up, so [`GridParts::assemble`] counts them.
    pub equal_rows: bool,
}

/// One grid cell's content and the card-chrome decision made about it.
///
/// Kept apart rather than pre-joined because a host pass that re-wraps a cell
/// (an internal-link cell becomes one big `<a>`) needs the content WITHOUT the
/// chrome, and the alternative — handing it the joined string to cut the
/// wrapper back off — is the string surgery this split form exists to delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridCellParts {
    /// The cell's rendered content, before any card chrome.
    pub inner: String,
    /// Whether `inner` gets the `<div class="moss-grid-card">` wrapper. False
    /// for a [`Block::LinkCard`] cell, which renders its own wrapping `<a
    /// class="moss-grid-card">`, and for host-supplied replacement markup that
    /// already carries its own chrome.
    pub carded: bool,
    /// The dominant colour of the image in the cell's cover position, as a CSS
    /// colour. Published as `--item-cover-color` on the wrapper (the private half of `var(--moss-cover-color, var(--item-cover-color))`, so a theme-set `--moss-cover-color` still wins) so a theme can
    /// paint a band behind the cell's text — the same variable, on the same
    /// class, that a collection card already carries.
    ///
    /// moss-core never computes it: extraction reads pixels off disk, so the
    /// host fills this in (`grid_cells::apply_cell_cover_colors`) and this
    /// module only decides where the bytes land.
    pub cover_color: Option<String>,
    /// Source lines the cell spans, published as `data-source-range` on the
    /// card wrapper. `None` when untracked.
    pub source_range: Option<(usize, usize)>,
    /// A collage cell's tracks, as custom-property declarations
    /// (`--moss-collage-span:2;--moss-collage-span-sm:6`). Kept when the
    /// host replaces the cell, since a tile without its span breaks the rows.
    pub layout: Option<String>,
    /// What a collage tile holds; `None` outside a collage.
    pub tile: Option<TileKind>,
    /// The cell's lead picture is taller than wide. Only the host can say,
    /// because it needs the picture's pixel size.
    pub upright: bool,
}

impl GridCellParts {
    /// Content plus chrome. The ONE place the card wrapper's bytes are written.
    pub fn to_html(&self) -> String {
        let mut attrs = String::new();
        if self.cover_color.is_some() {
            attrs.push_str(" data-cover-color");
        }
        if let Some(tile) = self.tile {
            attrs.push_str(&format!(r#" data-tile="{}""#, tile.as_str()));
        }
        if self.upright {
            attrs.push_str(" data-upright");
        }
        let style: Vec<String> = self
            .cover_color
            .as_deref()
            .map(|c| format!("--item-cover-color: {}", escape_attr(c)))
            .into_iter()
            .chain(self.layout.clone())
            .collect();
        if !style.is_empty() {
            attrs.push_str(&format!(r#" style="{}""#, style.join(";")));
        }
        if self.carded {
            let range = self
                .source_range
                .map(|(a, b)| format!(r#" data-source-range="{a}-{b}""#))
                .unwrap_or_default();
            format!(r#"<div class="moss-grid-card"{}{}>{}</div>"#, range, attrs, self.inner)
        } else {
            on_first_tag(&self.inner, &attrs)
        }
    }

    /// Final markup that brings its own chrome (a rendered collection card, a
    /// link preview).
    pub fn final_markup(html: String) -> Self {
        Self {
            inner: html,
            carded: false,
            cover_color: None,
            source_range: None,
            layout: None,
            tile: None,
            upright: false,
        }
    }

    /// Swap the content for final markup that brings its own chrome, keeping
    /// the cell's place in a collage.
    pub fn replace(&mut self, html: String) {
        let (layout, tile, upright) = (self.layout.take(), self.tile, self.upright);
        *self = Self { layout, tile, upright, ..Self::final_markup(html) };
    }
}

impl GridParts {
    /// Re-assemble the flat grid HTML — byte-identical to what the `Grid` arm
    /// of [`RenderHooks::render_shortcode`] emits.
    pub fn to_html(&self) -> String {
        let cells: Vec<String> = self.cells.iter().map(GridCellParts::to_html).collect();
        Self::assemble(&self.open_tag, &cells, self.equal_rows)
    }

    /// Re-assemble with `replacement` cells (same order) inside this grid's own
    /// opening tag, so `data-columns`, the ratio `style=`, `data-width` and
    /// `data-source-range` survive any cell substitution.
    pub fn assemble(open_tag: &str, cells: &[String], equal_rows: bool) -> String {
        let mut out = String::with_capacity(open_tag.len() + 32);
        if equal_rows {
            // A collection card counts its cover and its content, so no floor is needed.
            let parts = cells.iter().filter_map(|c| card_parts(c)).max().unwrap_or(1).max(1);
            out.push_str(&on_first_tag(open_tag, &format!(r#" style="--moss-grid-parts:{parts}""#)));
        } else {
            out.push_str(open_tag);
        }
        out.push_str(&cells.join("\n"));
        out.push_str("</div>");
        out
    }
}

/// `markup` with `attrs` added to its opening tag. Markup that brings its own
/// chrome (a link card, a collection card) carries a collage tile's span on
/// its own outer element; a `style` already there takes the declarations first.
// Every index is a `find` of ASCII ('>' or ` style="`), so each slice lands on a char boundary.
#[allow(clippy::string_slice)]
fn on_first_tag(markup: &str, attrs: &str) -> String {
    let Some(end) = markup.find('>').filter(|_| !attrs.is_empty() && markup.starts_with('<')) else {
        return markup.to_string();
    };
    let tag = &markup[..end];
    let (attrs, tag) = match (attrs.find(r#" style=""#), tag.find(r#" style=""#)) {
        (Some(a), Some(t)) => {
            let decls = attrs[a + 8..].trim_end_matches('"');
            (attrs[..a].to_string(), format!("{}{};{}", &tag[..t + 8], decls, &tag[t + 8..]))
        }
        _ => (attrs.to_string(), tag.to_string()),
    };
    format!("{}{}{}", tag, attrs, &markup[end..])
}

/// The canonical grid serialization. Called by
/// [`RenderHooks::render_grid_parts`]'s default body; overriding impls that
/// delegate the `Grid` arm elsewhere call this with the hooks they delegate to.
pub fn render_grid_parts<H: RenderHooks + ?Sized>(
    hooks: &H,
    args: &GridShortcode,
    source_line: Option<usize>,
) -> GridParts {
    let collage = args.is_collage().then(|| spans(&args.cells)).flatten();
    // Each cell renders into its own buffer; `Block::LinkCard` brings its own wrapping `<a>`.
    let mut cells: Vec<GridCellParts> = Vec::with_capacity(args.cells.len());
    // Scope image `sizes=` to the cell track while cells render.
    hooks.begin_grid_cells(args.columns, args.width.as_deref());
    for (i, cell_blocks) in args.cells.iter().enumerate() {
        let fragment = args.source.fragments.get(i);
        let mut cell_html = String::new();
        match quote_cell(cell_blocks) {
            Some((quote, source)) => render_quote(hooks, &mut cell_html, quote, source),
            None => super::render::render_blocks_with_meta(
                hooks,
                &mut cell_html,
                cell_blocks,
                fragment.map_or(&[], |f| f.blocks.as_slice()),
            ),
        }
        let collapsed = collapse_tag_adjacent_newlines(&cell_html);
        cells.push(GridCellParts {
            inner: collapsed.trim().to_string(),
            carded: !matches!(cell_blocks.as_slice(), [Block::LinkCard { .. }]),
            // Filled in later by the host, which can read the image off disk.
            cover_color: None,
            source_range: fragment.and_then(|f| Some((f.first_line?, f.last_line?))),
            layout: collage.as_ref().and_then(|spans| {
                let (wide, phone) = spans.get(i.checked_sub(1)?)?;
                Some(format!("--moss-collage-span:{wide};--moss-collage-span-sm:{phone}"))
            }),
            tile: collage.is_some().then(|| TileKind::of(cell_blocks)),
            upright: false,
        });
    }
    hooks.end_grid_cells();
    let mut open_tag = String::new();
    {
        let mut class_attr = String::from("moss-grid");
        if !args.classes.is_empty() {
            class_attr.push(' ');
            class_attr.push_str(&args.classes);
        }
        open_tag.push_str(r#"<div class=""#);
        open_tag.push_str(&escape_attr(&class_attr));
        open_tag.push_str(r#"" data-columns=""#);
        open_tag.push_str(&args.columns.to_string());
        open_tag.push('"');
        if let Some(mid) = args.mid_columns {
            open_tag.push_str(&format!(r#" data-columns-mid="{mid}""#));
        }
        // A ratio rides as a CUSTOM PROPERTY, never as an inline
        // `grid-template-columns`. An inline declaration outranks every
        // stylesheet rule, so a theme's own override (or any future rule
        // reading `grid-template-columns`) could never reach a `:::grid 2
        // 1:2` cell if the ratio arrived inline. Handing the value over as
        // `--moss-grid-ratio` puts it back in the cascade: the column rules
        // read `grid-template-columns: var(--moss-grid-ratio, <default>)`,
        // an ordinary value an ordinary rule can still outrank. Same shape as
        // `--moss-gallery-columns` next door.
        //
        // `minmax(0, Nfr)` rather than a bare `Nfr` for the reason spelled out
        // over `.moss-grid[data-columns="1"]` in site.css: a bare `fr` track
        // floors at the item's min-content width, so one wide image in a
        // ratio grid overflows the page sideways.
        let mut style: Vec<String> = Vec::new();
        if let Some(r) = &args.ratio {
            let cols = r
                .split(':')
                .map(|n| format!("minmax(0, {}fr)", n.trim()))
                .collect::<Vec<_>>()
                .join(" ");
            style.push(format!("--moss-grid-ratio:{cols}"));
        }
        if !style.is_empty() {
            open_tag.push_str(&format!(r#" style="{}""#, style.join(";")));
        }
        if args.has_equal_rows() {
            open_tag.push_str(r#" data-rows="equal""#);
        }
        // `scroll` keeps the row on one line and lets the reader drag it
        // sideways; `tabindex="0"` makes an overflowing row
        // keyboard-scrollable, the same affordance `.moss-table-scroll` gives
        // a wide table. `label` only means something once the row IS a
        // scroll region, so `role`/`aria-label` are emitted only alongside
        // `scroll` — alone they would name a landmark that was never created.
        //
        // `args.is_scroll_row()`, not the bare `scroll` flag: a one-cell
        // `{scroll}` grid has nothing to drag past, so it gets none of this.
        //
        // A row whose cells already fit in `columns` (`fits_without_scrolling`)
        // still gets the full scroll-region treatment but carries `data-fits`
        // too, so site.css/vertical.css can key the wide-screen plain-grid
        // layout off it. `tabindex="0"` is always emitted so a no-JS narrow
        // view stays keyboard-scrollable; the runtime script (`scroll-row.ts`)
        // removes it once it can see the row isn't scrollable at the current
        // width.
        //
        // `label` is the author's `{label="…"}` text or, when none, the
        // nearest preceding heading's text, filled in by the caller that sees
        // the top-level blocks (`grid_with_heading_fallback` in moss-build's
        // `body_plan.rs`). This function only reads `args.label`; an unnamed
        // focusable element is an accessibility gap, so either name is valid.
        if args.is_scroll_row() {
            open_tag.push_str(r#" data-scroll"#);
            if args.fits_without_scrolling() {
                open_tag.push_str(r#" data-fits"#);
            }
            open_tag.push_str(r#" tabindex="0""#);
            if let Some(label) = &args.label {
                open_tag.push_str(r#" role="region" aria-label=""#);
                open_tag.push_str(&escape_attr(label));
                open_tag.push('"');
            }
        }
        // `data-count` lets the stylesheet pick the arrangement for this many
        // tiles without a per-site `:nth-child` chain.
        if args.is_collage() {
            open_tag.push_str(&format!(r#" data-collage data-count="{}""#, args.cells.len()));
        }
        if let Some(w) = &args.width {
            open_tag.push_str(r#" data-width=""#);
            open_tag.push_str(w);
            open_tag.push('"');
        }
        // Click-to-source: a point source range on the Grid's outermost
        // element routes a click back to the `:::grid` line via the bridge's
        // `resolveSourceTarget`.
        if let Some(n) = source_line {
            open_tag.push_str(r#" data-source-range=""#);
            open_tag.push_str(&args.source.range_from(n));
            open_tag.push('"');
        }
        open_tag.push('>');
    }


    GridParts { open_tag, cells, equal_rows: args.has_equal_rows() }
}

// ── Parts of a card ─────────────────────────────────────────────────────
//
// `{rows=equal}` gives every card one grid track per part, and a part is a
// child of the card in the page, not a block of the author's source: a raw
// HTML wrapper turns a dozen source blocks into one child. So the count is
// read off the markup the card ends up as, after every pass that rewrote it.

/// Elements whose content is text, so a `<` inside is not a tag.
const RAW_TEXT: &[&str] = &["script", "style", "textarea", "title"];

/// The children of the first element in `html` (text runs count as one child
/// each, as grid makes an item of them), or `None` when the markup is not one
/// element with balanced tags, so the count would be a guess.
pub fn card_parts(html: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut parts = 0usize;
    let mut closed_root = false;
    let mut rest = html.trim_start();
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            // Trailing text: a child of the card, or stray text after it.
            return text_run(rest, depth, &mut parts).then_some(parts);
        };
        let (text, tail) = rest.split_at_checked(lt)?;
        if lt > 0 && !text_run(text, depth, &mut parts) {
            return None;
        }
        rest = tail;
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.split_once("-->")?.1;
            continue;
        }
        let end = tag_end(rest)?;
        let (head, tail) = rest.split_at_checked(end)?;
        let tag = head.get(1..end - 1)?;
        rest = tail;
        if tag.starts_with('!') || tag.starts_with('?') {
            continue;
        }
        if tag.starts_with('/') {
            depth = depth.checked_sub(1)?;
            closed_root |= depth == 0;
            continue;
        }
        if closed_root {
            return None;
        }
        let name = tag_name(tag);
        if depth == 1 {
            parts += 1;
        }
        let void = VOID_ELEMENTS.contains(&name.as_str()) || tag.ends_with('/');
        if !void {
            depth += 1;
            if RAW_TEXT.contains(&name.as_str()) {
                let close = format!("</{name}");
                let at = rest.to_ascii_lowercase().find(&close)?;
                rest = rest.get(at..)?;
            }
        } else if depth == 0 {
            return None;
        }
    }
    (closed_root && depth == 0).then_some(parts)
}

/// Account for text between tags; false when it sits outside the card.
fn text_run(text: &str, depth: usize, parts: &mut usize) -> bool {
    if text.trim().is_empty() {
        return true;
    }
    if depth == 0 {
        return false;
    }
    if depth == 1 {
        *parts += 1;
    }
    true
}

fn tag_name(tag: &str) -> String {
    tag.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase()
}

// ── Collage ─────────────────────────────────────────────────────────────
//
// CSS can count a grid's children but not which of them are pictures, so a
// collage's spans and each tile's kind are computed here from the typed cells
// and handed to the stylesheet as custom properties and `data-tile`.

/// A quote longer than this, in characters, is set smaller (`data-length="long"`).
const LONG_QUOTE_CHARS: usize = 140;

/// A grid cell that is a quote: a blockquote, then one italic line naming its
/// source. Returns the blockquote and the source line's inlines.
pub(crate) fn quote_cell(blocks: &[Block]) -> Option<(&Block, &[Inline])> {
    let [quote @ Block::BlockQuote(_), Block::Paragraph(line)] = blocks else {
        return None;
    };
    let blank = |i: &Inline| matches!(i, Inline::Text(s) if s.trim().is_empty());
    let mut parts = line.iter().filter(|i| !blank(i));
    match (parts.next(), parts.next()) {
        (Some(Inline::Emphasis(source)), None) if !source.is_empty() => Some((quote, source)),
        _ => None,
    }
}

/// `<figure class="moss-quote">`: the quote, then its source line as the
/// caption. Only the source line can link, so no link nests in another.
fn render_quote<H: RenderHooks + ?Sized>(hooks: &H, out: &mut String, quote: &Block, source: &[Inline]) {
    out.push_str(r#"<figure class="moss-quote""#);
    let script = cjk_script(quote);
    if script.is_some() {
        out.push_str(r#" data-script="cjk""#);
    }
    if is_long_quote(quote) {
        out.push_str(r#" data-length="long""#);
    }
    out.push('>');
    let from = out.len();
    super::render::render_blocks_with_meta(hooks, out, std::slice::from_ref(quote), &[]);
    if matches!(script, Some(Cjk::Japanese)) {
        // On the quote alone, so the source line after it is not read as Japanese.
        if let Some(at) = out.get(from..).and_then(|quote| quote.find("<blockquote")) {
            out.insert_str(from + at + "<blockquote".len(), r#" lang="ja""#);
        }
    }
    out.push_str("<figcaption>");
    out.push_str(&super::render::render_inlines_html(hooks, source));
    out.push_str("</figcaption></figure>");
}

enum Cjk {
    Chinese,
    Japanese,
}

/// The CJK script a quote is written in, when most of its letters are Han or
/// kana. A tenth of the Han and kana being kana means Japanese; fewer is a
/// Chinese quote that happens to hold a の.
fn cjk_script(quote: &Block) -> Option<Cjk> {
    let Block::BlockQuote(children) = quote else {
        return None;
    };
    let (mut han, mut kana, mut other) = (0usize, 0usize, 0usize);
    for block in children {
        let Block::Paragraph(inlines) = block else { continue };
        for c in super::plain_text::inlines_to_plain_text(inlines).chars() {
            match c as u32 {
                0x3040..=0x30FF => kana += 1,
                0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2A6DF => han += 1,
                _ if c.is_alphabetic() => other += 1,
                _ => {}
            }
        }
    }
    if han + kana <= other {
        None
    } else if kana * 10 >= han + kana {
        Some(Cjk::Japanese)
    } else {
        Some(Cjk::Chinese)
    }
}

/// Whether a quote's words run past the length that gets the smaller size.
pub(crate) fn is_long_quote(quote: &Block) -> bool {
    let Block::BlockQuote(children) = quote else {
        return false;
    };
    let chars: usize = children
        .iter()
        .map(|b| match b {
            Block::Paragraph(inlines) => super::plain_text::inlines_to_plain_text(inlines).chars().count(),
            _ => 0,
        })
        .sum();
    chars > LONG_QUOTE_CHARS
}

/// What a collage tile holds, published as `data-tile` for the stylesheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    /// One picture and nothing else (its credit is the figure's caption).
    Picture,
    /// [`quote_cell`]'s shape.
    Quote,
    /// Anything else, a picture further down included.
    Text,
}

impl TileKind {
    pub fn of(cell: &[Block]) -> Self {
        if quote_cell(cell).is_some() {
            TileKind::Quote
        } else if matches!(cell, [Block::LinkCard { .. }]) || cell_picture(cell).is_some() {
            TileKind::Picture
        } else {
            TileKind::Text
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            TileKind::Picture => "picture",
            TileKind::Quote => "quote",
            TileKind::Text => "text",
        }
    }
}

/// The image of a cell that is one picture: a figure, or a lone image, linked
/// or not, alone in its paragraph.
pub fn cell_picture(cell: &[Block]) -> Option<&Inline> {
    let image = match cell {
        [Block::Figure { image, .. }] => image,
        [Block::Paragraph(inlines)] => match inlines.as_slice() {
            [Inline::Link { children, .. }] => match children.as_slice() {
                [only] => only,
                _ => return None,
            },
            [only] => only,
            _ => return None,
        },
        _ => return None,
    };
    matches!(image, Inline::Image { .. }).then_some(image)
}

/// For each cell after the lead of a collage of 3 to 9 cells, the columns it
/// takes of the six right of the lead on a wide screen, and of six on a
/// phone. `None` outside 3 to 9 cells.
///
/// On a wide screen the spans depend only on the count, as do the rows the
/// stylesheet reads off `data-count`. On a phone a tile that is not a picture
/// takes a full row at its own height, and the pictures fill rows of three, a
/// short last row widened.
pub(crate) fn spans(cells: &[Vec<Block>]) -> Option<Vec<(u8, u8)>> {
    let rest = cells.get(1..).filter(|r| (2..=8).contains(&r.len()))?;
    let wide: &[u8] = match rest.len() {
        2 => &[3, 3],
        3 => &[2, 2, 2],
        4 => &[3, 3, 3, 3],
        5 => &[2, 2, 2, 3, 3],
        6 => &[2, 2, 2, 2, 2, 2],
        7 => &[2, 2, 2, 2, 2, 2, 6],
        _ => &[2, 2, 2, 2, 2, 2, 3, 3],
    };
    let is_picture = |c: &Vec<Block>| TileKind::of(c) == TileKind::Picture;
    let pictures = rest.iter().filter(|c| is_picture(c)).count();
    let short = pictures % 3;
    let mut seen = 0;
    Some(
        rest.iter()
            .zip(wide)
            .map(|(cell, &w)| {
                if !is_picture(cell) {
                    return (w, 6);
                }
                seen += 1;
                let in_short_row = seen > pictures - short;
                (w, if !in_short_row { 2 } else if short == 2 { 3 } else { 6 })
            })
            .collect(),
    )
}

#[cfg(test)]
#[path = "grid_parts_tests.rs"]
mod tests;
