//! Wikilink embed dispatch visitor.
//!
//! Walks a [`Document`] and routes every wikilink embed image
//! (`Inline::Image { is_wikilink: true, .. }`) through the
//! [`crate::resolve::wikilink_dispatch::dispatch_wikilink_embed_with_registry`]
//! dispatcher, replacing the block-level paragraph with the renderer's
//! output (HTML, inline markdown re-parse, deferred plugin marker, or
//! standard link).
//!
//! # Why a separate visitor
//!
//! Pre-Phase-4, `transform_events` ran this dispatch INLINE on each
//! `Event::Start(Tag::Image { link_type: LinkType::WikiLink, .. })`
//! event from pulldown-cmark, swallowing the event range. With the flip to
//! `parse → render_document`, pulldown-cmark only runs once (during
//! `parse`), so the dispatcher must operate on the typed AST instead. This
//! visitor IS the AST equivalent.
//!
//! # Why ![`![[...]]`] needs pothole preservation
//!
//! PR3.5 (2026-05-28) added wikilink-alt classification to the parser, so
//! `![[v.mp4|width=400]]` arrives at the AST with `alt: ""` (params
//! consumed) and the original `width=400` token is gone. The dispatcher
//! needs the original pothole to compose typed params for the video synth.
//! PR7a-flip-core-B added the `wikilink_pothole` field on `Inline::Image`
//! that the parser populates from the raw alt text BEFORE classification
//! runs — this visitor reads it for embed dispatch.
//!
//! # Inline vs block-level
//!
//! Per the SoCiviC + chps fixtures (the 4 client sites at Phase 4 cutover),
//! every wikilink embed in production is a "lone embed paragraph": a
//! paragraph whose only `Inline::Image { is_wikilink: true, .. }` plus
//! whitespace/linebreaks. The visitor detects this shape and replaces the
//! whole paragraph with the dispatch output (so block-level HTML doesn't
//! get `<p>`-wrapped).
//!
//! Inline wikilink images (e.g. `Some text ![[icon.png]] more text` in the
//! same paragraph) stay as `Inline::Image` and route through the normal
//! `render_inline` → `hooks.render_image` synth path. The dispatcher does
//! NOT walk them — the `<picture>` shape produced by `synthesize_image_html`
//! is the right output for inline embeds.

use crate::asset_snapshot::AssetSnapshot;
use crate::content_graph::ContentGraph;
use crate::resolve::registry::RendererRegistry;
use crate::resolve::wikilink_dispatch::{
    dispatch_wikilink_embed_with_registry, EmitKind, WikilinkEmit,
};
use crate::resolve::{Diagnostic, OutgoingLink};

use super::document::Document;
use super::node::{Block, Inline};
use super::parser::parse;
use super::shortcode::Shortcode;
use super::url::Url;

/// Aggregated output of [`dispatch_wikilink_embeds`].
///
/// Returned alongside the in-place document mutation so callers
/// (currently `process_markdown_file`) can fold the discovered outgoing
/// links into the page's `ContentGraph` updates and surface diagnostics.
#[derive(Debug, Default, Clone)]
pub struct WikilinkDispatchResult {
    /// Outgoing links discovered by the dispatcher (target paths +
    /// display text). Caller may extend its own outgoing-link list.
    pub outgoing_links: Vec<OutgoingLink>,
    /// Diagnostics (e.g. unresolved reference). Caller logs via
    /// `log::warn!` per entry.
    pub diagnostics: Vec<Diagnostic>,
}

/// Walk the document's top-level blocks and dispatch every wikilink
/// embed image (`![[…]]` paragraph) through the embed-renderer registry.
///
/// Returns the aggregated outgoing-link + diagnostic data; mutates
/// `doc.blocks` in place to substitute embed paragraphs with their
/// dispatched output.
///
/// # When to call
///
/// Run BEFORE [`crate::ast::resolve_urls::resolve_urls`]. The dispatcher
/// reads `Inline::Image.src` as `Url::Unresolved(raw)` — the parser's
/// pre-resolve form — because `dispatch_wikilink_embed_with_registry` does
/// its own [`crate::resolve::fuzzy_path::resolve_reference`] internally.
/// If `resolve_urls` runs first, the wikilink images' src is already
/// `Url::Resolved(href)` and the dispatcher's internal resolver would
/// double-resolve.
pub fn dispatch_wikilink_embeds(
    doc: &mut Document,
    snapshot: &AssetSnapshot,
    graph: &ContentGraph,
    registry: &RendererRegistry,
    source_path: &str,
) -> WikilinkDispatchResult {
    let mut result = WikilinkDispatchResult::default();
    dispatch_in_block_children(
        &mut doc.blocks,
        snapshot,
        graph,
        registry,
        source_path,
        &mut result,
    );
    result
}

/// Walk a `Vec<Block>` (top-level OR a BlockQuote / Callout / List item)
/// and dispatch any lone-embed paragraphs. Recurses into nested
/// containers (BlockQuote / Callout / List items).
fn dispatch_in_block_children(
    blocks: &mut Vec<Block>,
    snapshot: &AssetSnapshot,
    graph: &ContentGraph,
    registry: &RendererRegistry,
    source_path: &str,
    result: &mut WikilinkDispatchResult,
) {
    let mut i = 0;
    while i < blocks.len() {
        // First, check if this block is a lone wikilink embed paragraph.
        let dispatch_info = match &blocks[i] {
            Block::Paragraph(inlines) => find_lone_wikilink_image(inlines),
            _ => None,
        };

        if let Some((dest_url, pothole)) = dispatch_info {
            let emit = dispatch_wikilink_embed_with_registry(
                &dest_url,
                pothole.as_deref(),
                true, // is_embed: lone-paragraph wikilink image is an embed
                graph,
                source_path,
                snapshot,
                registry,
            );
            apply_emit(blocks, i, emit, result);
            i += 1;
            continue;
        }

        // Not a lone embed — descend into nested containers if any.
        //
        // PR7a-flip-core-C (2026-05-28): the recursion now matches the
        // visitor pattern in `visit.rs` for `Grid.cells` and `Hero.overlay`
        // (visit.rs:140, 145). Pre-flip, this visitor missed shortcode
        // bodies — a wikilink embed inside a `:::grid` cell or `:::hero`
        // overlay would not be dispatched.
        match &mut blocks[i] {
            Block::BlockQuote(children)
            | Block::Callout { children, .. }
            | Block::FootnoteDefinition { children, .. } => {
                dispatch_in_block_children(
                    children,
                    snapshot,
                    graph,
                    registry,
                    source_path,
                    result,
                );
            }
            Block::List { items, .. } => {
                for item in items.iter_mut() {
                    dispatch_in_block_children(
                        item,
                        snapshot,
                        graph,
                        registry,
                        source_path,
                        result,
                    );
                }
            }
            Block::LinkCard { children, .. } => {
                // PR4.5 compound-link cell — descend into its block body so
                // wikilinks inside a grid LinkCard render correctly.
                dispatch_in_block_children(
                    children,
                    snapshot,
                    graph,
                    registry,
                    source_path,
                    result,
                );
            }
            Block::Shortcode(sc) => {
                dispatch_in_shortcode(sc, snapshot, graph, registry, source_path, result);
            }
            _ => {}
        }
        i += 1;
    }
}

/// Recurse into a shortcode's typed block bodies (Grid cells, Hero overlay).
/// Matches `visit.rs::visit_urls_in_shortcode` so wikilink embeds inside
/// shortcode bodies are dispatched alongside top-level ones.
fn dispatch_in_shortcode(
    sc: &mut Shortcode,
    snapshot: &AssetSnapshot,
    graph: &ContentGraph,
    registry: &RendererRegistry,
    source_path: &str,
    result: &mut WikilinkDispatchResult,
) {
    match sc {
        // Variants with no typed block body — nothing to descend into.
        Shortcode::Subscribe(_)
        | Shortcode::Buttons(_)
        | Shortcode::Gallery(_)
        | Shortcode::Recent(_)
        | Shortcode::Apply(_) => {}
        Shortcode::Hero(args) => {
            dispatch_in_block_children(
                &mut args.overlay,
                snapshot,
                graph,
                registry,
                source_path,
                result,
            );
        }
        Shortcode::Grid(args) => {
            for cell in args.cells.iter_mut() {
                dispatch_in_block_children(cell, snapshot, graph, registry, source_path, result);
            }
        }
    }
}

/// Detect a "lone wikilink image" paragraph: exactly one
/// `Inline::Image { is_wikilink: true, .. }` modulo whitespace text and
/// line breaks.
///
/// Returns `Some((dest_url, pothole))` where `dest_url` is the unresolved
/// wikilink target (e.g. `"v.mp4"`) and `pothole` is the original pothole
/// text (e.g. `Some("width=400")`).
fn find_lone_wikilink_image(inlines: &[Inline]) -> Option<(String, Option<String>)> {
    let mut found: Option<(String, Option<String>)> = None;
    for inline in inlines {
        match inline {
            Inline::Image {
                src,
                is_wikilink: true,
                wikilink_pothole,
                ..
            } => {
                if found.is_some() {
                    return None; // Multiple images — not a lone embed.
                }
                let dest = match src {
                    Url::Unresolved(s) => s.clone(),
                    Url::Resolved(r) => r.href.clone(),
                };
                found = Some((dest, wikilink_pothole.clone()));
            }
            // Whitespace / linebreak siblings are tolerated.
            Inline::Text(t) if t.trim().is_empty() => {}
            Inline::LineBreak => {}
            _ => return None, // Any non-whitespace sibling disqualifies.
        }
    }
    found
}

/// Apply the dispatcher's `EmitKind` to `blocks[i]`.
///
/// - `Html` / `Deferred` → replace with `Block::Other(html_or_marker)`
///   (block-level raw HTML, bypassing `<p>` wrap).
/// - `Inline` / `Link` → re-parse via [`parse`]; splice the resulting
///   blocks in at position `i` (so e.g. an image embed that re-parses
///   into a `Block::Paragraph(vec![Inline::Image { … }])` becomes the
///   exact same shape the inline-image renderer expects).
fn apply_emit(
    blocks: &mut Vec<Block>,
    i: usize,
    emit: WikilinkEmit,
    result: &mut WikilinkDispatchResult,
) {
    if let Some(link) = emit.outgoing_link {
        result.outgoing_links.push(link);
    }
    result.diagnostics.extend(emit.diagnostics);

    match emit.output {
        EmitKind::Html(html) | EmitKind::Deferred(html) => {
            blocks[i] = Block::Other(html);
        }
        EmitKind::Block(block) => {
            // Image-embed synth-collapse: a typed `Block::Figure` (or other
            // typed block) substituted 1:1 at `blocks[i]`. This is the only
            // emit shape that preserves the source paragraph's `block_meta`
            // (a `Block::Other` HTML string carries none): replacing in place
            // leaves the parallel `block_meta` vec untouched, so the figure
            // inherits the original paragraph's `data-source-line`. No
            // re-parse, no splice — exactly one block in, one block out.
            blocks[i] = *block;
        }
        EmitKind::Inline(markdown) | EmitKind::Link(markdown) => {
            // Re-parse the emitted markdown and splice in the resulting
            // blocks at position `i`. Typical shape: a single
            // `Block::Paragraph(vec![Inline::Image { … }])` or
            // `Block::Figure { image: Inline::Image { … }, … }`, which
            // routes through the standard image-render path (synth
            // `<picture>` for raster, etc.).
            //
            // The caller's loop advances by 1, so we leave it to step
            // through any inserted blocks. The inserted blocks shouldn't
            // themselves contain wikilink embeds (re-parse of a
            // `![alt](url)` produces a plain markdown image), so a single
            // advance is safe.
            let parsed = parse(&markdown);
            // Latent invariant: caller (apply_emit) holds `&mut Document` and
            // splices block_meta in lockstep if/when parsed.blocks.len() != 1.
            // Today re-parse of an emit-rendered `![alt](url)` or `[text](url)`
            // always yields exactly one block, so the parallel-vec invariant
            // (blocks.len() == block_meta.len()) accidentally holds via this
            // path. If a future EmitKind expansion emits a multi-block
            // markdown fragment, this assert fails fast in debug builds
            // before render_document's own debug_assert panics with a less
            // helpful message.
            debug_assert_eq!(
                parsed.blocks.len(),
                1,
                "wikilink-emit re-parse must yield exactly one block; \
                 block_meta lockstep update needed here if this changes"
            );
            blocks.splice(i..=i, parsed.blocks);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_snapshot::AssetSnapshot;
    use crate::content_graph::ContentGraph;
    use crate::resolve::registry::RendererRegistry;

    fn empty_graph() -> ContentGraph {
        crate::content_graph::ContentGraphBuilder::new().build()
    }

    fn empty_snapshot() -> AssetSnapshot {
        AssetSnapshot::default()
    }

    fn empty_registry() -> RendererRegistry {
        RendererRegistry::builtin().build()
    }

    #[test]
    fn lone_wikilink_embed_image_replaces_paragraph_with_inline_form() {
        // `![[photo.png]]` is a lone wikilink-embed paragraph. The image
        // renderer emits inline-markdown (`![alt](url)`), which re-parses
        // to `Block::Figure { image: Inline::Image { … }, … }` (image-only
        // paragraph promotion). The dispatcher splices in the re-parsed
        // result.
        let mut doc = Document::from_blocks(vec![Block::Paragraph(vec![Inline::Image {
            src: Url::unresolved("photo.png"),
            alt: String::new(),
            title: None,
            is_wikilink: true,
            wikilink_pothole: None,
        }])]);
        let snap = empty_snapshot();
        let graph = empty_graph();
        let reg = empty_registry();
        let result = dispatch_wikilink_embeds(&mut doc, &snap, &graph, &reg, "post.md");
        // The original paragraph is replaced. Either with Block::Figure
        // (after re-parse) or Block::Paragraph (when re-parse doesn't
        // promote). Either way, the resulting blocks should NOT contain
        // a wikilink `Inline::Image`.
        let has_wikilink_image = find_any_wikilink_image(&doc.blocks);
        assert!(
            !has_wikilink_image,
            "dispatch should have removed the wikilink image"
        );
        // photo.png is unresolved in an empty graph; the result records
        // the missing-reference outgoing link (target_path == "photo.png").
        // We don't assert the exact form because the renderer's behavior
        // for unresolved paths is contract-tested in wikilink_dispatch.rs.
        let _ = result;
    }

    #[test]
    fn non_wikilink_image_is_left_alone() {
        // Standard markdown image (not a wikilink). Dispatch should
        // skip it.
        let mut doc = Document::from_blocks(vec![Block::Paragraph(vec![Inline::Image {
            src: Url::unresolved("photo.png"),
            alt: "a".into(),
            title: None,
            is_wikilink: false,
            wikilink_pothole: None,
        }])]);
        let snap = empty_snapshot();
        let graph = empty_graph();
        let reg = empty_registry();
        let _ = dispatch_wikilink_embeds(&mut doc, &snap, &graph, &reg, "post.md");
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => match &inlines[0] {
                Inline::Image { is_wikilink, .. } => assert!(!is_wikilink),
                _ => panic!("expected Image"),
            },
            _ => panic!("expected Paragraph"),
        }
    }

    #[test]
    fn inline_wikilink_image_with_surrounding_text_is_not_dispatched() {
        // `Some text ![[icon.png]] more text` — the wikilink image is
        // inline. Dispatch should NOT touch it (the inline path
        // renders through `render_image`).
        let mut doc = Document::from_blocks(vec![Block::Paragraph(vec![
            Inline::Text("hello ".into()),
            Inline::Image {
                src: Url::unresolved("icon.png"),
                alt: String::new(),
                title: None,
                is_wikilink: true,
                wikilink_pothole: None,
            },
            Inline::Text(" world".into()),
        ])]);
        let snap = empty_snapshot();
        let graph = empty_graph();
        let reg = empty_registry();
        let _ = dispatch_wikilink_embeds(&mut doc, &snap, &graph, &reg, "post.md");
        // The paragraph should still carry the inline wikilink image
        // (text + image + text shape preserved).
        match &doc.blocks[0] {
            Block::Paragraph(inlines) => {
                assert_eq!(inlines.len(), 3);
                assert!(matches!(
                    &inlines[1],
                    Inline::Image {
                        is_wikilink: true,
                        ..
                    }
                ));
            }
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    #[test]
    fn empty_document_is_a_no_op() {
        let mut doc = Document::from_blocks(vec![]);
        let snap = empty_snapshot();
        let graph = empty_graph();
        let reg = empty_registry();
        let result = dispatch_wikilink_embeds(&mut doc, &snap, &graph, &reg, "post.md");
        assert!(doc.blocks.is_empty());
        assert!(result.outgoing_links.is_empty());
        assert!(result.diagnostics.is_empty());
    }

    /// Helper: scan blocks for any wikilink-embed Inline::Image. Used by
    /// the lone-embed test to verify dispatch removed the wikilink.
    fn find_any_wikilink_image(blocks: &[Block]) -> bool {
        for block in blocks {
            if block_has_wikilink_image(block) {
                return true;
            }
        }
        false
    }

    fn block_has_wikilink_image(block: &Block) -> bool {
        match block {
            Block::Paragraph(inlines) => inlines.iter().any(|i| {
                matches!(
                    i,
                    Inline::Image {
                        is_wikilink: true,
                        ..
                    }
                )
            }),
            Block::Figure { image, .. } => matches!(
                image,
                Inline::Image {
                    is_wikilink: true,
                    ..
                }
            ),
            Block::BlockQuote(children) | Block::Callout { children, .. } => {
                children.iter().any(block_has_wikilink_image)
            }
            Block::List { items, .. } => items
                .iter()
                .any(|item| item.iter().any(block_has_wikilink_image)),
            Block::LinkCard { children, .. } => children.iter().any(block_has_wikilink_image),
            Block::Shortcode(sc) => shortcode_has_wikilink_image(sc),
            _ => false,
        }
    }

    fn shortcode_has_wikilink_image(sc: &super::super::shortcode::Shortcode) -> bool {
        use super::super::shortcode::Shortcode;
        match sc {
            // Variants with no typed block body — no wikilink images possible.
            // (`Recent` carries `fallback_markdown: String`, not typed blocks;
            // see `fix(ast): cover Shortcode::Recent in dispatch_wikilink_embeds`
            // commit 747b8f2b0 for the production-side rationale.)
            Shortcode::Subscribe(_)
            | Shortcode::Buttons(_)
            | Shortcode::Gallery(_)
            | Shortcode::Recent(_)
            | Shortcode::Apply(_) => false,
            Shortcode::Hero(args) => args.overlay.iter().any(block_has_wikilink_image),
            Shortcode::Grid(args) => args
                .cells
                .iter()
                .any(|cell| cell.iter().any(block_has_wikilink_image)),
        }
    }

    // -----------------------------------------------------------------
    // PR7a-flip-core-C (2026-05-28): shortcode-body recursion
    // -----------------------------------------------------------------

    #[test]
    fn grid_cell_wikilink_embed_is_dispatched() {
        // A `:::grid` whose cell contains a lone wikilink embed paragraph.
        // The visitor must descend into Grid.cells and dispatch the embed,
        // replacing the paragraph in place. Before flip-core-C, the
        // wikilink Inline::Image would survive in the cell.
        use super::super::shortcode::{GridShortcode, Shortcode};

        let cell = vec![Block::Paragraph(vec![Inline::Image {
            src: Url::unresolved("photo.png"),
            alt: String::new(),
            title: None,
            is_wikilink: true,
            wikilink_pothole: None,
        }])];
        let mut doc =
            Document::from_blocks(vec![Block::Shortcode(Shortcode::Grid(GridShortcode {
                columns: 1,
                ratio: None,
                classes: String::new(),
                cells: vec![cell],
                width: None,
            }))]);
        let snap = empty_snapshot();
        let graph = empty_graph();
        let reg = empty_registry();
        let _ = dispatch_wikilink_embeds(&mut doc, &snap, &graph, &reg, "post.md");
        let has_wikilink_image = find_any_wikilink_image(&doc.blocks);
        assert!(
            !has_wikilink_image,
            "dispatch should descend into Grid cells and remove the wikilink image"
        );
    }

    #[test]
    fn hero_overlay_wikilink_embed_is_dispatched() {
        // A `:::hero` whose overlay contains a lone wikilink embed paragraph.
        // The visitor must descend into Hero.overlay and dispatch the embed.
        // SoCiviC's fixtures rely on this — hero overlays carry markdown
        // that may include `![[...]]` references.
        use super::super::shortcode::{HeroShortcode, Shortcode};

        let overlay = vec![Block::Paragraph(vec![Inline::Image {
            src: Url::unresolved("overlay.png"),
            alt: String::new(),
            title: None,
            is_wikilink: true,
            wikilink_pothole: None,
        }])];
        let mut doc =
            Document::from_blocks(vec![Block::Shortcode(Shortcode::Hero(HeroShortcode {
                image: None,
                extra_images: Vec::new(),
                attrs: String::new(),
                classes: String::new(),
                overlay,
                overlay_text: String::new(),
                width: None,
                mobile: None,
                caption: String::new(),
            }))]);
        let snap = empty_snapshot();
        let graph = empty_graph();
        let reg = empty_registry();
        let _ = dispatch_wikilink_embeds(&mut doc, &snap, &graph, &reg, "post.md");
        let has_wikilink_image = find_any_wikilink_image(&doc.blocks);
        assert!(
            !has_wikilink_image,
            "dispatch should descend into Hero overlay and remove the wikilink image"
        );
    }

    // --- parse → dispatch composition (video sizing truth table) ---------
    //
    // Each stage was unit-tested in isolation while the COMPOSITION broke:
    // the parser promoted `![[clip.mov|77%]]` to Block::Figure (width
    // bypasses the empty-alt guard) and this visitor only dispatches
    // Paragraph-shaped embeds, so the video synthesizer never ran. These
    // tests pin the full parse→dispatch pipe for every video sizing shape.

    fn parse_and_dispatch(md: &str, files: &[&str]) -> Vec<Block> {
        let mut doc = crate::ast::parse(md);
        let mut b = crate::content_graph::ContentGraphBuilder::new();
        for p in files {
            let slug = std::path::Path::new(p)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(p);
            b.add_file(p, slug);
        }
        let graph = b.build();
        let snap = empty_snapshot();
        let reg = empty_registry();
        let _ = dispatch_wikilink_embeds(&mut doc, &snap, &graph, &reg, "post.md");
        doc.blocks
    }

    /// Extract the raw HTML of the single Block::Other the dispatcher
    /// emitted, panicking with the actual shape otherwise.
    fn dispatched_html(blocks: &[Block]) -> &str {
        match blocks {
            [Block::Other(html)] => html,
            other => panic!("expected one dispatched Block::Other, got {other:?}"),
        }
    }

    #[test]
    fn video_plain_dispatches_to_video_synth() {
        let blocks = parse_and_dispatch("![[clip.mov]]\n", &["clip.mov"]);
        let html = dispatched_html(&blocks);
        assert!(html.contains("<video"), "got: {html}");
        assert!(html.contains("clip.mp4"), "mov→mp4 swap missing: {html}");
    }

    #[test]
    fn video_percent_keeps_video_and_width() {
        let blocks = parse_and_dispatch("![[clip.mov|77%]]\n", &["clip.mov"]);
        let html = dispatched_html(&blocks);
        assert!(html.contains("<video"), "got: {html}");
        assert!(
            html.contains(r#"width="77%""#),
            "percent width dropped: {html}"
        );
        assert!(
            !html.contains("<img"),
            "video must not render as <img>: {html}"
        );
    }

    #[test]
    fn video_box_sizing_keeps_video_and_dims() {
        let blocks = parse_and_dispatch("![[clip.mov|640x360]]\n", &["clip.mov"]);
        let html = dispatched_html(&blocks);
        assert!(html.contains("<video"), "got: {html}");
        assert!(html.contains(r#"width="640px""#), "got: {html}");
        assert!(html.contains(r#"height="360px""#), "got: {html}");
        assert!(
            !html.contains("figcaption"),
            "sizing alias must not become a caption: {html}"
        );
    }

    #[test]
    fn image_percent_still_promotes_to_figure() {
        // Images keep the parse-time Figure promotion (dispatch skips the
        // already-promoted block; resolve_urls owns its src downstream).
        let blocks = parse_and_dispatch("![[pic.jpg|55%]]\n", &["pic.jpg"]);
        match &blocks[..] {
            [Block::Figure { width, .. }] => {
                assert_eq!(width.as_deref(), Some("55%"));
            }
            other => panic!("expected Figure for image percent, got {other:?}"),
        }
    }
}
