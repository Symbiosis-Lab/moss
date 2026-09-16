//! A `:::grid` serialized into its structural pieces.
//!
//! The flat `Grid` arm of [`RenderHooks::render_shortcode`] is
//! [`GridParts::to_html`] over this value, so there is exactly ONE grid byte
//! shape and the split form can never drift from the flat one.
//!
//! The split form exists because a structural decision about a grid *cell* —
//! "is this cell a link to a collection, and if so what is that collection's
//! title, cover and child count?" — depends on facts no single page can see.
//! Before ADR-034 the host answered it by regex-scraping its own emitted
//! markup; that scraping is what aborted the build when a CJK character sat in
//! front of a `:::grid` (moss#903 bug 1). With the pieces kept separate the
//! host pairs each cell's HTML with the typed [`Block`] cell it came from, so
//! recognition reads exactly what emission read.

use super::hooks::{collapse_tag_adjacent_newlines, escape_attr, RenderHooks};
use super::node::Block;
use super::shortcode::GridShortcode;

/// A `:::grid` block's serialized pieces: the opening `<div class="moss-grid"
/// …>` tag and one [`GridCellParts`] per cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridParts {
    /// Carries the author's classes, the ratio `style=`, `data-width`, and
    /// `data-source-range`.
    pub open_tag: String,
    /// One entry per [`GridShortcode::cells`] entry, in the same order.
    pub cells: Vec<GridCellParts>,
}

/// One grid cell's content and the card-chrome decision made about it.
///
/// Kept apart rather than pre-joined because a host pass that re-wraps a cell
/// (an internal-link cell becomes one big `<a>`) needs the content WITHOUT the
/// chrome, and the alternative — handing it the joined string to cut the
/// wrapper back off — is the string surgery ADR-034 exists to delete.
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
    /// colour. Published as `--moss-cover-color` on the wrapper so a theme can
    /// paint a band behind the cell's text — the same variable, on the same
    /// class, that a collection card already carries.
    ///
    /// moss-core never computes it: extraction reads pixels off disk, so the
    /// host fills this in (`grid_cells::apply_cell_cover_colors`) and this
    /// module only decides where the bytes land.
    pub cover_color: Option<String>,
}

impl GridCellParts {
    /// Content plus chrome. The ONE place the card wrapper's bytes are written.
    pub fn to_html(&self) -> String {
        if self.carded {
            let color = self
                .cover_color
                .as_deref()
                .map(|c| {
                    format!(
                        r#" data-cover-color style="--moss-cover-color: {}""#,
                        escape_attr(c)
                    )
                })
                .unwrap_or_default();
            format!(
                r#"<div class="moss-grid-card"{}>{}</div>"#,
                color, self.inner
            )
        } else {
            self.inner.clone()
        }
    }

    /// Final markup that brings its own chrome (a rendered collection card, a
    /// link preview).
    pub fn final_markup(html: String) -> Self {
        Self {
            inner: html,
            carded: false,
            cover_color: None,
        }
    }
}

impl GridParts {
    /// Re-assemble the flat grid HTML — byte-identical to what the `Grid` arm
    /// of [`RenderHooks::render_shortcode`] emits.
    pub fn to_html(&self) -> String {
        let cells: Vec<String> = self.cells.iter().map(GridCellParts::to_html).collect();
        Self::assemble(&self.open_tag, &cells)
    }

    /// Re-assemble with `replacement` cells (same order) inside this grid's own
    /// opening tag, so `data-columns`, the ratio `style=`, `data-width` and
    /// `data-source-range` survive any cell substitution.
    pub fn assemble(open_tag: &str, cells: &[String]) -> String {
        let mut out = String::with_capacity(open_tag.len() + 8);
        out.push_str(open_tag);
        out.push_str(&cells.join("\n"));
        out.push_str("</div>");
        out
    }
}

/// The canonical grid serialization. Called by
/// [`RenderHooks::render_grid_parts`]'s default body; overriding impls that
/// delegate the `Grid` arm elsewhere call this with the hooks they delegate to.
pub fn render_grid_parts<H: RenderHooks + ?Sized>(
    hooks: &H,
    args: &GridShortcode,
    source_line: Option<usize>,
) -> GridParts {
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
        // A ratio rides as a CUSTOM PROPERTY, never as an inline
        // `grid-template-columns`. An inline declaration outranks every
        // stylesheet rule, so `:::grid 2 1:2` used to stay two columns at
        // every viewport — the mobile collapse (`@media (max-width: 768px)
        // .moss-grid[data-columns] { grid-template-columns: 1fr }`) could
        // never reach it. Handing the value over as `--moss-grid-ratio` puts
        // it back in the cascade: the column rules read
        // `grid-template-columns: var(--moss-grid-ratio, <default>)`, and the
        // mobile rule simply doesn't read the variable. Same shape as
        // `--moss-gallery-columns` next door.
        //
        // `minmax(0, Nfr)` rather than a bare `Nfr` for the reason spelled out
        // over `.moss-grid[data-columns="1"]` in site.css: a bare `fr` track
        // floors at the item's min-content width, so one wide image in a
        // ratio grid overflows the page sideways.
        if let Some(r) = &args.ratio {
            let cols = r
                .split(':')
                .map(|n| format!("minmax(0, {}fr)", n.trim()))
                .collect::<Vec<_>>()
                .join(" ");
            open_tag.push_str(r#" style="--moss-grid-ratio:"#);
            open_tag.push_str(&cols);
            open_tag.push('"');
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
            open_tag.push_str(&n.to_string());
            open_tag.push('-');
            open_tag.push_str(&n.to_string());
            open_tag.push('"');
        }
        open_tag.push('>');
    }

    // Phase 4 PR4.5 (2026-05-28): cells are typed `Vec<Block>`. Each cell
    // renders into its own scratch buffer, gets tag-adjacent newlines
    // collapsed (so the byte shape matches pulldown-cmark's `push_html`), then
    // gets wrapped — or not, for `Block::LinkCard`, which renders its own
    // wrapping `<a>`. `to_html` joins with `\n` to match the byte shape of the
    // long-deleted `render_grid_html_typed`.
    let mut cells: Vec<GridCellParts> = Vec::with_capacity(args.cells.len());
    // Scope image `sizes=` to the cell track while cells render.
    hooks.begin_grid_cells(args.columns, args.width.as_deref());
    for cell_blocks in &args.cells {
        let mut cell_html = String::new();
        super::render::render_blocks(hooks, &mut cell_html, cell_blocks);
        let collapsed = collapse_tag_adjacent_newlines(&cell_html);
        cells.push(GridCellParts {
            inner: collapsed.trim().to_string(),
            carded: !matches!(cell_blocks.as_slice(), [Block::LinkCard { .. }]),
            // Filled in later by the host, which can read the image off disk.
            cover_color: None,
        });
    }
    hooks.end_grid_cells();

    GridParts { open_tag, cells }
}
