//! Grid-cell enhancement: what a `:::grid` cell BECOMES once the whole build
//! is known.
//!
//! Three things a cell can turn into, none of them decidable while the page
//! that contains it renders:
//!
//! - a **collection card** — the cell links to a folder or article elsewhere in
//!   the site, so it renders that page's title, cover, date and child count;
//! - a **link preview** — the cell links out, so it renders favicon + domain +
//!   (cached) title;
//! - a **link card** — the cell links internally but resolves to nothing
//!   listable, so its content becomes one clickable card.
//!
//! Everything here reads [`GridCellEmission::blocks`] — the typed cell the
//! serializer rendered from. This module replaced `render/grid_post.rs`, which
//! answered the same questions by re-scanning the page's own emitted HTML with
//! five regexes and a hand-rolled `<div>`-depth byte cursor. That approach
//! failed in both directions:
//!
//! - the byte cursor sliced mid-character and aborted whole builds
//!   (`start byte index 66 is not a char boundary; it is inside
//!   '在'`);
//! - the regexes silently stopped matching when the emitter's attribute order
//!   changed. `<a href="…"` never matches moss-core's external-link output,
//!   which is `<a target="_blank" rel="noopener" href="…">`, so grids of
//!   external links quietly stopped becoming link previews — visible in
//!   `tests/fixtures/link-preview-site`, whose recorded output contradicts its
//!   own prose. Reading the typed cell fixes that by construction: recognition
//!   now looks at what emission looked at.
//!
//! Pass order: `{.summary}` fences first (they leave no grid behind at all),
//! then collection cards, then the URLs whose metadata the preview pass wants,
//! then previews. A cell replaced by an earlier pass has its typed content
//! cleared, so no later pass can re-classify generated markup as if it were the
//! author's.
//!
//! A whole FENCE can also change shape: `:::grid {.summary}` becomes the same
//! `.moss-cards[data-layout="list"]` container a folder listing emits, its
//! cells rendered by the same summary-card emitter — see
//! [`apply_summary_grids`].

use std::collections::HashMap;

use moss_core::ast::{Block, Inline, ResolvedUrl, Url, UrlKind};

use crate::build::components::child_list::ChildItemProps;
use crate::build::components::grid_card::{render_external_card, render_item_with_typesetting};
use crate::build::markdown::body_plan::{BodyPlan, BodySegment, GridCellEmission, GridEmission};
use crate::build::media::cover::detect_cover_type;
use crate::build::page::link_meta::LinkMeta;
use crate::i18n::Language;
use crate::build::types::ParsedDocument;

/// Whole-build facts a grid cell needs, gathered once per page render.
pub(crate) struct BuildIndex<'a> {
    /// Every page in the build — the lookup target for a cell's link.
    pub documents: &'a [ParsedDocument],
    /// Partial-path resolution (`游记/` → `文字/游记/`) for links that name a
    /// folder without its full path.
    pub graph: Option<&'a moss_core::content_graph::ContentGraph>,
    pub dir_overrides: &'a HashMap<String, String>,
    pub root_path: Option<&'a str>,
    pub media_lookup: Option<&'a crate::build::media::dimensions::MediaDimensionLookup>,
    pub lang: Language,
    /// The page being rendered. Cell hrefs are relative to its pretty URL, so
    /// they anchor here before being matched against absolute `url_path`s.
    pub page_url_path: &'a str,
    /// The rendering page's resolved typesetting, for the card's CJK date and count.
    pub typesetting: Option<&'a str>,
}

/// A grid cell's structure, read from its typed blocks.
pub(crate) enum GridCell<'a> {
    /// The cell's substantive content is exactly one link.
    Link(CellLink<'a>),
    /// Anything else — renders as the author wrote it.
    Opaque,
}

/// The single link a [`GridCell::Link`] cell is built around.
pub(crate) struct CellLink<'a> {
    pub href: &'a str,
    pub kind: UrlKind,
    /// The link's text with markup dropped — the card's title when the linked
    /// page doesn't supply a better one.
    pub text: String,
    /// The entire cell is the link (a [`Block::LinkCard`]). An INTERNAL
    /// whole-cell link already IS the serializer's final
    /// `<a class="moss-grid-card">` chrome, so a pass that would re-wrap it
    /// has to leave it alone. An EXTERNAL one is not left alone the same
    /// way — see [`render_external_card`], which overrides it with the
    /// unified `.moss-card` shell regardless of `whole_cell`.
    pub whole_cell: bool,
    /// The href + alt of an authored cover image, when the link's own
    /// content opens with one — a `Block::LinkCard` built around a photo,
    /// or a leading link followed by a separate caption paragraph (see
    /// [`leading_link`]). No pass may replace such a cell with a generated
    /// page/collection card, or drop the image in favor of fetched
    /// metadata: author content always wins.
    pub cover_image: Option<(&'a str, &'a str)>,
}

impl CellLink<'_> {
    /// Links that leave the site. `AssetNewtab` counts: it is an internal file
    /// opened in a new tab, and the old scanner treated it the same way.
    fn external(&self) -> bool {
        matches!(self.kind, UrlKind::External | UrlKind::AssetNewtab)
    }

    /// True when the author wrote a bare URL (or no text at all), which is the
    /// signal that the preview title should come from fetched metadata rather
    /// than from the link text.
    fn wants_fetched_title(&self) -> bool {
        self.text.is_empty() || self.text.starts_with("http://") || self.text.starts_with("https://")
    }

    /// True when a generated page or collection card must not replace this
    /// cell: it either leaves the site (never a candidate — it names nothing
    /// in this build), or it already carries its own authored content that
    /// such a card would throw away. The one check both card-generating
    /// passes ([`card_markup`], [`summary_card_markup`]) share.
    fn ineligible_for_a_generated_card(&self) -> bool {
        self.external() || self.cover_image.is_some()
    }
}

/// Read a grid cell's structure off its typed blocks.
///
/// The accepted shapes, and nothing else:
///
/// 1. `[LinkCard]` — the whole cell is one markdown link spanning block
///    content (`[![poster](p.png) ### Title](/show/)`).
/// 2. `[Paragraph]` opening with a link, with nothing after it, or a
///    soft-wrapped plain-text description.
/// 3. `[Paragraph, Paragraph]` — as (2), with the description as its own
///    paragraph.
/// 4. `[List]` with exactly ONE item, unwrapped to (2) or (3). Grids predate
///    `+++` cell dividers and were authored as markdown lists
///    (`:::grid 3` / `- [My Folder](articles/my-folder)`), which the old
///    scanner accepted because it matched the first `<a href=` anywhere in the
///    cell. A list with two or more items is NOT a link cell: the old scanner
///    replaced such a cell with a card for its first link and silently dropped
///    the rest, which is a bug rather than a shape worth reproducing.
///
/// A cell with two links, a heading, a nested container, or any styled
/// description is [`GridCell::Opaque`]. The old scanner's equivalent test was
/// "exactly one `<a href=` in the cell, and the cell opens with `<p><a`" — the
/// same rule, asked of the data instead of the markup.
pub(crate) fn classify_cell(blocks: &[Block]) -> GridCell<'_> {
    match blocks {
        [Block::LinkCard { url, children }] => match resolved(url) {
            Some(r) => GridCell::Link(CellLink {
                href: &r.href,
                kind: r.kind,
                text: blocks_text(children),
                whole_cell: true,
                cover_image: moss_core::ast::link_card::cover_image_href(children),
            }),
            None => GridCell::Opaque,
        },
        [Block::Paragraph(inlines)] => leading_link(inlines),
        [Block::Paragraph(inlines), Block::Paragraph(desc)] if is_plain_prose(desc) => {
            leading_link(inlines)
        }
        [Block::List { items, .. }] => match items.as_slice() {
            [only] => classify_cell(only),
            _ => GridCell::Opaque,
        },
        _ => GridCell::Opaque,
    }
}

/// A paragraph is a link cell when it OPENS with a link and carries nothing
/// after it but a soft-wrapped plain-text description.
fn leading_link(inlines: &[Inline]) -> GridCell<'_> {
    let [Inline::Link { url, children, .. }, tail @ ..] = inlines else {
        return GridCell::Opaque;
    };
    // A soft break renders as a literal `\n`, which is what made the old
    // scanner's "newline right after `</a>`" test work. `[A](a/) desc` (same
    // line) is deliberately NOT a link cell — the text reads as prose beside a
    // link, not as a card subtitle.
    let described = is_plain_prose(tail)
        && matches!(tail.first(), Some(Inline::Text(t)) if t.starts_with('\n'));
    if !(tail.is_empty() || described) {
        return GridCell::Opaque;
    }
    match resolved(url) {
        Some(r) => GridCell::Link(CellLink {
            href: &r.href,
            kind: r.kind,
            text: inlines_text(children),
            whole_cell: false,
            // An ordinary (non-wikilink) image-plus-caption cell reaches this
            // shape rather than `Block::LinkCard` (see
            // `moss_core::ast::shortcode_extract::detect_compound_link`), so
            // the link's own children can still open with an image.
            cover_image: match children.first() {
                Some(Inline::Image { src: Url::Resolved(r), alt, .. }) => {
                    Some((r.href.as_str(), alt.as_str()))
                }
                _ => None,
            },
        }),
        None => GridCell::Opaque,
    }
}

/// Unstyled running text — the only thing allowed to follow a cell's link.
fn is_plain_prose(inlines: &[Inline]) -> bool {
    inlines.iter().all(|i| matches!(i, Inline::Text(_)))
}

/// Only a resolved URL can be looked up. An unresolved one means the resolve
/// phase missed this link, which is a bug elsewhere — the cell renders as-is
/// rather than being silently matched against a raw author string.
fn resolved(url: &Url) -> Option<&ResolvedUrl> {
    match url {
        Url::Resolved(r) => Some(r),
        Url::Unresolved(_) => None,
    }
}

/// A link's text with markup dropped.
///
/// The old scanner recovered this by stripping tags out of the rendered anchor
/// and then HTML-decoding what was left, which also picked up rendered chrome —
/// a heading inside a compound cell contributed its permalink anchor's `#`.
/// Reading the inlines yields the author's text and only that.
fn inlines_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    push_inlines_text(&mut out, inlines);
    out.trim().to_string()
}

fn blocks_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Paragraph(inlines) => push_inlines_text(&mut out, inlines),
            Block::Heading { children, .. } => push_inlines_text(&mut out, children),
            _ => {}
        }
        out.push(' ');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn push_inlines_text(out: &mut String, inlines: &[Inline]) {
    for inline in inlines {
        match inline {
            Inline::Text(t) | Inline::Code(t) => out.push_str(t),
            Inline::Emphasis(children) | Inline::Strong(children) | Inline::Strikethrough(children) => {
                push_inlines_text(out, children)
            }
            Inline::Link { children, .. } => push_inlines_text(out, children),
            Inline::LineBreak => out.push(' '),
            Inline::Image { .. } | Inline::Other(_) | Inline::FootnoteRef(_) | Inline::TaskMarker(_) => {}
        }
    }
}

// ── the render-phase entry point ────────────────────────────────────────

/// Resolve one page's `:::grid` cells against the whole build, returning its
/// body with every cell that names something the build knows about replaced.
///
/// The steps in order — `{.summary}` fences (which leave no grid behind),
/// collection cards, a cache-only metadata read for the outbound links that
/// want a fetched title, then previews. Every decision is made on the typed
/// cells the serializer recorded in `ParsedDocument::body_plan`, never on the
/// emitted markup.
///
/// A document without a plan — synthesized outside the markdown pipeline —
/// keeps its `html_content` as one opaque segment and picks up no enhancements,
/// which is what those pages did before the typed grid cells too.
pub(crate) fn resolve_page_body(
    doc: &ParsedDocument,
    all_docs: &[ParsedDocument],
    graph: Option<&moss_core::content_graph::ContentGraph>,
    dir_overrides: &HashMap<String, String>,
    root_path: &str,
    media_lookup: &crate::build::media::dimensions::MediaDimensionLookup,
    typesetting: Option<&str>,
) -> BodyPlan {
    let mut plan = match &doc.body_plan {
        Some(p) => p.clone(),
        None => BodyPlan::opaque(doc.html_content.clone()),
    };
    let index = BuildIndex {
        documents: all_docs,
        graph,
        dir_overrides,
        root_path: Some(root_path),
        media_lookup: Some(media_lookup),
        lang: doc.lang,
        page_url_path: &doc.url_path,
        typesetting,
    };
    apply_summary_grids(&mut plan, &index);
    apply_collection_cards(&mut plan, &index);
    apply_cell_cover_colors(&mut plan, &index);

    let urls_to_fetch = external_urls_needing_fetch(&plan);
    let link_meta = if urls_to_fetch.is_empty() {
        None
    } else {
        let moss_dir = std::path::Path::new(root_path).join(".moss");
        let url_refs: Vec<&str> = urls_to_fetch.iter().map(|s| s.as_str()).collect();
        // Critical-path I/O policy: render reads cache only and explicitly
        // records URLs for the next build's background prewarm. The first build
        // may render empty cards; the next sees populated metadata after sync
        // has run.
        crate::build::page::link_meta::record_urls_for_prewarm(&url_refs, &moss_dir);
        Some(crate::build::page::link_meta::read_link_meta_from_cache(
            &url_refs, &moss_dir,
        ))
    };
    apply_link_previews(&mut plan, &index, link_meta.as_ref());
    plan
}

// ── the passes ─────────────────────────────────────────────────────────

/// Replace a whole `:::grid {.summary}` fence with the listing container, its
/// cells rendered as SUMMARY cards.
///
/// `{.summary}` REPLACES the grid's container rather than decorating it. Every
/// summary rule in `site.css` rides on `.moss-cards[data-layout="list"]`, so a
/// `.moss-grid` merely *containing* `.moss-card` elements would need a second
/// stylesheet — the parallel renderer this variant exists to avoid. The
/// segment therefore becomes `BodySegment::Html`, which also takes the grid
/// out of `grids_mut`: the cover-colour and link-preview passes that follow
/// cannot then fight the summary cards for the same cell.
///
/// A column count (`:::grid 3 {.summary}`) is dropped by construction — the
/// opener carrying `data-columns` is simply not emitted. A summary card is a
/// full-measure row; two side by side is a grid card, which is what the author
/// gets by removing `{.summary}`.
///
/// `{.no-cards}` wins over `{.summary}`: "do not convert my cells" is the
/// stronger statement, and an author who writes both has contradicted
/// themselves in a direction where doing nothing is safe.
///
/// Runs FIRST, before [`apply_collection_cards`].
pub(crate) fn apply_summary_grids(plan: &mut BodyPlan, index: &BuildIndex<'_>) {
    for segment in &mut plan.segments {
        let BodySegment::Grid(grid) = segment else {
            continue;
        };
        if !grid.summary() || grid.no_cards() {
            continue;
        }
        let mut cards: Vec<String> = Vec::with_capacity(grid.cells.len());
        let mut resolved_any = false;
        for cell in &grid.cells {
            match summary_card_markup(cell, index) {
                Some(card) => {
                    resolved_any = true;
                    cards.push(card);
                }
                // The author's own markup, unwrapped, as a flex item in the
                // list. NOT `parts.to_html()`: that wraps it in
                // `.moss-grid-card` chrome whose CSS is scoped to `.moss-grid`
                // and would render unstyled here.
                None => cards.push(cell.inner().to_string()),
            }
        }
        // Nothing resolved — a wrong link path shows the author their own
        // content rather than an empty styled block, the same courtesy
        // `apply_collection_cards` extends to a grid that resolves to nothing.
        if !resolved_any {
            continue;
        }
        *segment = BodySegment::Html(format!(
            "{}\n",
            crate::build::components::cards_container(
                "list",
                true,
                &Default::default(),
                &cards.join("\n"),
            )
        ));
    }
}

/// One summary card for a cell that names a page in this build.
fn summary_card_markup(cell: &GridCellEmission, index: &BuildIndex<'_>) -> Option<String> {
    let GridCell::Link(link) = classify_cell(&cell.blocks) else {
        return None;
    };
    if link.ineligible_for_a_generated_card() {
        return None;
    }
    let props = picked_card_props(&link, index)?;
    Some(crate::build::components::child_summary::render_with_sort(
        &props,
        index.lang,
        index.typesetting,
        index.media_lookup,
        // A hand-picked block has no sort. The axis names what the META SLOT
        // says, not how the list was ordered: `Date` surfaces the page's date,
        // which is the fact a reader picks a piece by; `Weight`/`Title` would
        // silently delete it from every card.
        moss_core::sort::SortAxis::Date,
    ))
}

/// Replace cells that link to a page in this build with that page's card.
///
/// Skips grids the author marked `{.no-cards}` — navigation grids and
/// mixed-content splits whose cells happen to contain internal links.
pub(crate) fn apply_collection_cards(plan: &mut BodyPlan, index: &BuildIndex<'_>) {
    for grid in grids_mut(plan) {
        if grid.no_cards() {
            continue;
        }
        // The first card with a cover carries the LCP preload hint, matching
        // the bookkeeping the listing-grid renderer does.
        let mut eager_spent = false;
        for cell in &mut grid.cells {
            if let Some(html) = card_markup(cell, index, &mut eager_spent) {
                cell.replace(html);
            }
        }
    }
}

/// Publish the dominant colour of a cell's cover image as `--moss-cover-color`
/// on the cell's own `.moss-grid-card`.
///
/// A collection card gets this already; a hand-built cell — image on top, then
/// the author's own text — got nothing, so a theme that wanted the same
/// coloured band behind a bespoke layout had no colour to paint with. The
/// variable is only published, never painted: moss's own CSS ignores it here,
/// so an existing site's grids look exactly as they did.
///
/// Run after [`apply_collection_cards`], whose cells bring their own chrome and
/// their own colour; `carded` is what tells the two apart.
pub(crate) fn apply_cell_cover_colors(plan: &mut BodyPlan, index: &BuildIndex<'_>) {
    for grid in grids_mut(plan) {
        for cell in &mut grid.cells {
            if !cell.parts.carded {
                continue;
            }
            let Some(src) = cover_image_href(&cell.blocks) else {
                continue;
            };
            // The one shared ladder — cache first, then extraction — so a cell
            // image and a card cover of the same file agree on their colour.
            cell.parts.cover_color = crate::build::components::color_extract::resolve_card_color(
                Some(src),
                Some(detect_cover_type(src, None)),
                index.root_path.map(std::path::Path::new),
                index.media_lookup,
            );
        }
    }
}

/// The href of the image in a cell's cover position — its FIRST block, alone in
/// a paragraph or wrapped in a figure. An image further down the cell is an
/// illustration inside the author's prose, not the thing the cell is a card for.
fn cover_image_href(blocks: &[Block]) -> Option<&str> {
    let image = match blocks.first()? {
        Block::Figure { image, .. } => image,
        Block::Paragraph(inlines) => match inlines.as_slice() {
            [only] => only,
            _ => return None,
        },
        _ => return None,
    };
    match image {
        Inline::Image { src, .. } => resolved(src).map(|r| r.href.as_str()),
        _ => None,
    }
}

fn card_markup(
    cell: &GridCellEmission,
    index: &BuildIndex<'_>,
    eager_spent: &mut bool,
) -> Option<String> {
    let GridCell::Link(link) = classify_cell(&cell.blocks) else {
        return None;
    };
    if link.ineligible_for_a_generated_card() {
        return None;
    }
    let props = picked_card_props(&link, index)?;
    let eager = !*eager_spent && props.cover.is_some();
    *eager_spent |= eager;
    Some(render_item_with_typesetting(
        &props,
        index.root_path.map(std::path::Path::new),
        index.lang,
        // The cell content is a title plus a count, and the count IS
        // typesetting-dependent: `article_count_label` writes 四篇 rather than
        // `4 篇` under vertical CJK. Passing `None` here left an Arabic digit
        // lying on its side in every `:::grid` folder card (2026-09-11).
        index.typesetting,
        index.media_lookup,
        eager,
    ))
}

/// External URLs whose cached metadata the card pass wants — every
/// external cell, whole-cell or not, gets the same fetch. Owner decision
/// (2026-09): author-written link text wins the TITLE slot only; it must
/// never suppress the fetch, the favicon, or the og:image cover — the
/// owner's case is a row of podcast episodes written as
/// `[episode title](https://…)` that still need covers like every other
/// card. An authored image in the cell still wins as the cover regardless
/// (see `external_card_markup`'s `cover_image` match), so a whole-cell
/// image card's fetch here is purely for its favicon/cover — its title
/// never depended on it.
///
/// Run after [`apply_collection_cards`]: a cell that became a card is no
/// longer a candidate, and its cleared blocks say so.
pub(crate) fn external_urls_needing_fetch(plan: &BodyPlan) -> Vec<String> {
    let mut urls = Vec::new();
    for grid in grids(plan) {
        for cell in &grid.cells {
            if let GridCell::Link(link) = classify_cell(&cell.blocks) {
                if link.external() {
                    urls.push(link.href.to_string());
                }
            }
        }
    }
    urls
}

/// Every external grid-cell URL across a whole build's already-parsed
/// documents, deduplicated — the candidate set for
/// `link_meta::fetch_new_link_meta_for_build`'s once-per-build,
/// short-budget fetch (`build::render::blocking`'s pre-render step).
///
/// Safe to read a document's RAW `body_plan`, before any per-page pass has
/// run on it: [`apply_collection_cards`] and [`apply_summary_grids`] only
/// ever replace INTERNAL cells, so which cells are external is fixed at
/// parse time regardless of pipeline order — reading it early costs
/// nothing and avoids threading per-page `BuildIndex`es through a
/// whole-build pre-pass that doesn't otherwise need one.
pub(crate) fn external_urls_across_build<'a>(
    documents: impl IntoIterator<Item = &'a ParsedDocument>,
) -> Vec<String> {
    let mut urls = std::collections::BTreeSet::new();
    for doc in documents {
        if let Some(plan) = &doc.body_plan {
            urls.extend(external_urls_needing_fetch(plan));
        }
    }
    urls.into_iter().collect()
}

/// Turn single-link cells that leave the site into `.moss-card`s — the same
/// card kind [`apply_collection_cards`] gives an internal page (the owner's
/// "one card kind" decision). Internal cells are untouched here: a
/// `leading_link` one already rendered its own `<a>` (wrapping it again
/// would nest anchors), and a whole-cell one already has the serializer's
/// `data-kind="link"` chrome.
pub(crate) fn apply_link_previews(
    plan: &mut BodyPlan,
    index: &BuildIndex<'_>,
    link_meta: Option<&HashMap<String, LinkMeta>>,
) {
    for grid in grids_mut(plan) {
        for cell in &mut grid.cells {
            if let Some(html) = external_card_markup(cell, index, link_meta) {
                cell.replace(html);
            }
        }
    }
}

fn external_card_markup(
    cell: &GridCellEmission,
    index: &BuildIndex<'_>,
    link_meta: Option<&HashMap<String, LinkMeta>>,
) -> Option<String> {
    let GridCell::Link(link) = classify_cell(&cell.blocks) else {
        return None;
    };
    if !link.external() {
        // Every non-external cell that reaches here is non-whole-cell (a
        // `leading_link` paragraph, never a `Block::LinkCard`), so its own
        // `Inline::Link` already rendered an `<a>` around the cell's
        // content. Wrapping that in a second one nests anchors. A whole-cell
        // internal link keeps the serializer's own `data-kind="link"` shape
        // (see `moss_core::ast::render`'s `Block::LinkCard` arm) — this
        // pass is scoped to links that leave the site.
        return None;
    }
    let (fetched_title, favicon) = {
        let meta = link_meta.and_then(|m| m.get(link.href));
        (
            meta.and_then(|m| m.title.clone()),
            meta.and_then(|m| m.favicon.clone()),
        )
    };
    // Author content wins the TITLE slot only (owner decision, 2026-09):
    // real link text is never replaced by a fetched title, but — unlike an
    // earlier "manual mode" that also withheld the fetch itself — the
    // favicon and cover below are never gated on this. Only a titleless
    // cell falls back further, to the URL itself.
    let title = if !link.wants_fetched_title() {
        link.text.clone()
    } else {
        fetched_title.unwrap_or_else(|| moss_core::ast::link_card::domain_and_path(link.href))
    };
    let cover_html = match link.cover_image {
        Some((href, alt)) => {
            let (cover_path, attrs_str) = moss_core::media::split_pipe(href);
            let cover_attrs = moss_core::media::parse_media_attrs(attrs_str);
            let cover_type = detect_cover_type(cover_path, None);
            crate::build::media::cover::render_cover_html(
                cover_path,
                cover_type,
                alt,
                "moss-card-cover",
                &cover_attrs,
                true,
                index.media_lookup,
                false,
            )
        }
        None => r#"<div class="moss-card-cover moss-card-no-cover"></div>"#.to_string(),
    };
    Some(render_external_card(
        link.href,
        &title,
        &moss_core::ast::link_card::extract_domain(link.href),
        favicon.as_deref(),
        &cover_html,
    ))
}

fn grids(plan: &BodyPlan) -> impl Iterator<Item = &GridEmission> {
    plan.segments.iter().filter_map(|s| match s {
        BodySegment::Grid(g) => Some(g),
        BodySegment::Html(_) | BodySegment::Subscribe(_) => None,
    })
}

fn grids_mut(plan: &mut BodyPlan) -> impl Iterator<Item = &mut GridEmission> {
    plan.segments.iter_mut().filter_map(|s| match s {
        BodySegment::Grid(g) => Some(g),
        BodySegment::Html(_) | BodySegment::Subscribe(_) => None,
    })
}

// ── link → card resolution ─────────────────────────────────────────────

/// Anchor a cell's link to an absolute site path, the way a browser would.
///
/// Grid hrefs are relative to the page's pretty URL (a `/en/` page links its
/// sibling collection as `video/`, not `/en/video/`). Cards are matched against
/// absolute `url_path`s, so an unanchored `video/` from `/en/` would wrongly
/// match a root `video/` collection of the same name — the multilingual
/// `url:`-override collision.
fn resolve_cell_href(page_url_path: &str, href: &str) -> String {
    // Root-relative (`/foo/`): page-independent.
    if let Some(abs) = href.strip_prefix('/') {
        return abs.to_string();
    }
    // Relative: drop the page's filename (`en/index.html` → `en`), then join
    // the href's segments, normalizing `.` and `..`.
    let base_dir = page_url_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut segments: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    let mut out = segments.join("/");
    // Keep a trailing slash so the lookup below reads it as a folder.
    if href.ends_with('/') && !out.is_empty() {
        out.push('/');
    }
    out
}

/// The card props for the page a cell's link names, or `None` when it points
/// outside the build.
///
/// The same reading of the page a folder listing makes — count or date, cover,
/// `external_url:`, kicker — so a hand-picked card and the listing card for one
/// page agree by construction. Until 2026-09-12 the plain `:::grid` card built
/// its own from `doc.date` alone, and a page whose date came from any other
/// rung showed one in the listing and none here.
fn picked_card_props(link: &CellLink<'_>, index: &BuildIndex<'_>) -> Option<ChildItemProps> {
    let doc = find_linked_document(link, index)?;
    let mut props = crate::build::components::child_list::props_for_document(
        doc,
        index.documents,
        index.root_path.unwrap_or(""),
        index.dir_overrides,
        index.typesetting,
    );
    // An explicit link text wins; otherwise the linked page's own label, which
    // `props_for_document` already filled in.
    if !link.text.is_empty() {
        props.title = link.text.clone();
    }
    Some(props)
}

/// The page a cell's link names, or `None` when it points outside the build.
///
/// Decode, anchor against the hosting page's pretty URL, then look up — the
/// shared half of resolving a cell, used by both card shapes.
fn find_linked_document<'a>(
    link: &CellLink<'_>,
    index: &BuildIndex<'a>,
) -> Option<&'a ParsedDocument> {
    let decoded = urlencoding::decode(link.href)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| link.href.to_string());
    let anchored = resolve_cell_href(index.page_url_path, &decoded);
    let folder_url = if anchored.ends_with('/') || anchored.is_empty() {
        anchored
    } else {
        format!("{}/", anchored)
    };
    find_document(&folder_url, index)
}

/// Find the document a folder-shaped URL points at.
fn find_document<'a>(folder_url: &str, index: &BuildIndex<'a>) -> Option<&'a ParsedDocument> {
    let index_url = format!("{}index.html", folder_url);
    let exact = index
        .documents
        .iter()
        .find(|d| d.url_path == folder_url || d.url_path == index_url);
    if exact.is_some() {
        return exact;
    }

    // Partial-path fallback: `游记/` names a folder without its parents, and
    // `Mirror/` may not be the folder's on-disk case, so ask the content graph
    // where that folder note actually lives. Try `{folder}/index.md` first
    // (suffix matching reaches nested paths), then the bare name (stem matching
    // reaches self-named notes).
    let graph = index.graph?;
    let folder_name = folder_url.trim_end_matches('/');
    let resolved = graph
        .resolve_path(&format!("{folder_name}/index.md"), "")
        .or_else(|| graph.resolve_path(folder_name, ""))?;

    // Match on the SOURCE path the graph just returned. Deriving a url_path
    // from it instead (drop `.md`, collapse a home file to its folder, append
    // `index.html`) re-implemented three rules page_map already applied — and
    // spelled the folder in the source's case, so a mixed-case folder matched
    // nothing and the card silently vanished.
    index
        .documents
        .iter()
        .find(|d| d.source_path.as_deref() == Some(resolved.as_str()))
}

#[cfg(test)]
#[path = "grid_cells_tests.rs"]
mod tests;
