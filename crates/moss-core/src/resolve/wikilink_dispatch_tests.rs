use super::*;
use crate::content_graph::{ContentGraph, ContentGraphBuilder};

// --- parse_pothole_params edge cases ----------------------------------

#[test]
fn pothole_empty_string_is_empty() {
    assert_eq!(parse_pothole_params(""), PotholeContent::Empty);
    assert_eq!(parse_pothole_params("   "), PotholeContent::Empty);
}

#[test]
fn pothole_pure_digit_is_alias_not_width_token() {
    // `[[img.jpg|400]]` — `400` is NOT a spec § P9 width keyword
    // (only `body|wide|page|screen|full` match). Pure-pixel widths
    // are handled downstream by the relevant renderer's `Sizing::parse`
    // on the alias. parse_pothole_params therefore classifies `400`
    // as a plain alias here; the image / video renderer's existing
    // alias-based sizing logic (carry through to ParsedEmbed.alias)
    // does the rest.
    match parse_pothole_params("400") {
        PotholeContent::Alias(s) => assert_eq!(s, "400"),
        other => panic!("expected Alias, got {:?}", other),
    }
}

#[test]
fn pothole_plain_alias() {
    // `[[file|My alias]]`
    match parse_pothole_params("My alias") {
        PotholeContent::Alias(s) => assert_eq!(s, "My alias"),
        other => panic!("expected Alias, got {:?}", other),
    }
}

#[test]
fn pothole_kv_pair_is_params() {
    // `[[file|width=400 align=left]]`
    match parse_pothole_params("width=400 align=left") {
        PotholeContent::Params(p) => {
            assert_eq!(p.get("width"), Some("400"));
            assert_eq!(p.get("align"), Some("left"));
        }
        other => panic!("expected Params, got {:?}", other),
    }
}

#[test]
fn pothole_single_kv_is_params() {
    // `[[file|width=400]]`
    match parse_pothole_params("width=400") {
        PotholeContent::Params(p) => {
            assert_eq!(p.get("width"), Some("400"));
        }
        other => panic!("expected Params, got {:?}", other),
    }
}

#[test]
fn pothole_bare_alt_blocks_kv_parse() {
    // CRITICAL: `[[file|alt text=cover]]` — `alt` is bare (no `=`),
    // so the whole thing must be classified as alias text, NOT as
    // a `text=cover` param.
    match parse_pothole_params("alt text=cover") {
        PotholeContent::Alias(s) => assert_eq!(s, "alt text=cover"),
        other => panic!("expected Alias, got {:?}", other),
    }
}

#[test]
fn pothole_uppercase_key_blocks_kv_parse() {
    // `[[file|My Notes=Important]]` — `My` doesn't start with
    // lowercase letter; whole thing falls through to alias.
    match parse_pothole_params("My Notes=Important") {
        PotholeContent::Alias(s) => assert_eq!(s, "My Notes=Important"),
        other => panic!("expected Alias, got {:?}", other),
    }
}

#[test]
fn pothole_no_equals_is_alias() {
    // `[[file|width 400]]` — no `=` on `width` token; alias.
    match parse_pothole_params("width 400") {
        PotholeContent::Alias(s) => assert_eq!(s, "width 400"),
        other => panic!("expected Alias, got {:?}", other),
    }
}

#[test]
fn pothole_partial_kv_falls_through_to_alias() {
    // `[[file|width=400 caption text]]` — first token is K=V but
    // `caption` and `text` aren't. Every-token rule fails → alias.
    match parse_pothole_params("width=400 caption text") {
        PotholeContent::Alias(s) => assert_eq!(s, "width=400 caption text"),
        other => panic!("expected Alias, got {:?}", other),
    }
}

#[test]
fn pothole_kv_with_hyphenated_key() {
    // Hyphen and underscore allowed in keys.
    match parse_pothole_params("aria-label=primary data_id=42") {
        PotholeContent::Params(p) => {
            assert_eq!(p.get("aria-label"), Some("primary"));
            assert_eq!(p.get("data_id"), Some("42"));
        }
        other => panic!("expected Params, got {:?}", other),
    }
}

#[test]
fn pothole_obsidian_width_keyword() {
    // `[[img.jpg|wide]]` — `wide` is a known width keyword.
    match parse_pothole_params("wide") {
        PotholeContent::WidthToken { width, rest_alias } => {
            assert_eq!(width, "wide");
            assert!(rest_alias.is_empty());
        }
        other => panic!("expected WidthToken, got {:?}", other),
    }
}

// --- split_dest_url cases --------------------------------------------

#[test]
fn split_dest_url_plain_file() {
    let s = split_dest_url("notes");
    assert_eq!(s.file, "notes");
    assert_eq!(s.section, None);
    assert_eq!(s.query, None);
}

#[test]
fn split_dest_url_with_anchor() {
    let s = split_dest_url("notes#section");
    assert_eq!(s.file, "notes");
    assert_eq!(s.section, Some("section"));
    assert_eq!(s.query, None);
}

#[test]
fn split_dest_url_with_query() {
    let s = split_dest_url("page.html?x=1");
    assert_eq!(s.file, "page.html");
    assert_eq!(s.query, Some("x=1"));
}

#[test]
fn split_dest_url_anchor_then_query() {
    let s = split_dest_url("page.html#frag?x=1");
    assert_eq!(s.file, "page.html");
    assert_eq!(s.section, Some("frag"));
    assert_eq!(s.query, Some("x=1"));
}

#[test]
fn split_dest_url_query_then_anchor() {
    // Both '?' and '#' present, '?' first — query owns its tail; '#' splits out.
    let s = split_dest_url("page.html?x=1#frag");
    assert_eq!(s.file, "page.html");
    // query is [q+1..h] => "x=1"
    assert_eq!(s.query, Some("x=1"));
    // section is [h+1..] => "frag"
    assert_eq!(s.section, Some("frag"));
}

// --- dispatch_wikilink_embed integration -----------------------------
//
// Use a minimal ContentGraph that registers a few paths. We rely on
// ContentGraph::resolve_path() to map bare names back to filesystem-
// looking paths (the same surface Stage 1 uses).

fn build_graph(paths: &[&str]) -> ContentGraph {
    let mut b = ContentGraphBuilder::new();
    for p in paths {
        // Derive a simple slug from the filename stem; the slug is
        // only relevant for slug-based resolution which our tests
        // don't exercise (they use bare filenames matching `path`).
        let slug = std::path::Path::new(p)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(p);
        b.add_file(p, slug);
    }
    b.build()
}

/// Helper: empty AssetSnapshot. Phase 3 PR4.5 (2026-05-27) added the
/// `assets` parameter to dispatch_wikilink_embed so non-image embed
/// kinds can route directly to their HTML synthesizers.
fn empty_snapshot() -> AssetSnapshot {
    AssetSnapshot::new()
}

#[test]
fn dispatch_bare_wikilink_is_link() {
    let graph = build_graph(&["notes.md"]);
    let emit = dispatch_wikilink_embed(
        "notes",
        None,
        /* is_embed */ false,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Link(link) => {
            assert!(link.contains("notes"));
            assert!(link.contains("moss-resolved:"));
        }
        other => panic!("expected Link, got {:?}", other),
    }
    assert!(emit.outgoing_link.is_some());
    assert!(emit.diagnostics.is_empty());
}

#[test]
fn dispatch_wikilink_with_alias_uses_alias_text() {
    let graph = build_graph(&["notes.md"]);
    let emit = dispatch_wikilink_embed(
        "notes",
        Some("My alias"),
        false,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Link(link) => {
            assert!(link.starts_with("[My alias]"));
        }
        other => panic!("expected Link, got {:?}", other),
    }
}

#[test]
fn dispatch_unresolved_wikilink_emits_diagnostic() {
    let graph = build_graph(&[]);
    let emit = dispatch_wikilink_embed(
        "missing",
        None,
        false,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    assert_eq!(emit.diagnostics.len(), 1);
    match emit.output {
        EmitKind::Link(link) => assert!(link.contains("moss-unresolved:")),
        other => panic!("expected Link, got {:?}", other),
    }
}

#[test]
fn dispatch_anchor_wikilink_preserves_section_in_href() {
    let graph = build_graph(&["notes.md"]);
    let emit = dispatch_wikilink_embed(
        "notes#section",
        None,
        false,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Link(link) => {
            assert!(link.contains("moss-resolved:"));
            // Anchor preserved (Obsidian-style heading-anchor slug).
            assert!(link.contains("#section"), "got: {}", link);
        }
        other => panic!("expected Link, got {:?}", other),
    }
}

// --- build_anchor / dispatch_wikilink_form (`[[…]]` text-link) -------
//
// SCOPE WARNING — read before trusting these as link-path coverage.
//
// The three tests below drive `dispatch_wikilink_embed(..., is_embed:
// false, ..)`, i.e. the `dispatch_wikilink_form` branch and its
// `build_anchor` helper. That branch is the ONLY caller of `build_anchor`,
// and in the LIVE build it is DORMANT: the sole production caller of this
// dispatcher — the AST visitor `crate::ast::dispatch_wikilink_embeds`
// (`ast/dispatch_wikilink_embeds.rs`) — hard-codes `is_embed: true`
// (it only walks `![[…]]` image-embed `Inline::Image` nodes). Plain
// `[[Page#Heading]]` TEXT links never reach this function in production;
// they arrive as `Inline::Link { is_wikilink: true }` and are resolved
// by `crate::ast::resolve_urls`, whose `slug_wikilink_suffix` performs
// the user-facing `#Heading → #heading` slugging.
//
// ==> The REAL guards for `[[Page#Heading]]` text-link slugging live in
//     `crates/moss-core/src/ast/resolve_urls.rs`:
//       - wikilink_cross_page_fragment_is_slugged
//       - wikilink_same_page_fragment_is_slugged
//       - markdown_link_fragment_stays_raw_not_slugged
//       - wikilink_block_ref_keeps_id_raw
//       - wikilink_cjk_fragment_preserved
//       - slug_wikilink_suffix_preserves_query
//
// These three tests are kept because `build_anchor` is real code worth
// locking (it mirrors `slug_wikilink_suffix`, and a plugin/CLI caller
// could pass `is_embed: false`), NOT because they cover the live link
// path. Their names are deliberately `build_anchor_*` so a future reader
// is not misled into thinking text-link resolution is guarded here.

#[test]
fn build_anchor_slugs_section_fragment() {
    // Helper-path test (NOT the live `[[…]]` link path — see SCOPE
    // WARNING above; the live guard is
    // `resolve_urls::wikilink_cross_page_fragment_is_slugged`).
    //
    // `dispatch_wikilink_form("notes#My Heading")` slugs the section
    // fragment via `build_anchor` → `obsidian_heading_anchor` →
    // `#my-heading`.
    // Emitted output: `[notes > My Heading](moss-resolved:notes.md#my-heading)`.
    let graph = build_graph(&["notes.md"]);
    let emit = dispatch_wikilink_embed(
        "notes#My Heading",
        None,
        false,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Link(link) => {
            assert!(link.contains("#my-heading"), "got: {}", link);
            assert!(link.contains("moss-resolved:"), "got: {}", link);
        }
        other => panic!("expected Link, got {:?}", other),
    }
}

#[test]
fn build_anchor_same_page_emits_bare_anchor() {
    // Helper-path test (NOT the live `[[…]]` link path — see SCOPE
    // WARNING above; the live guard is
    // `resolve_urls::wikilink_same_page_fragment_is_slugged`).
    //
    // `dispatch_wikilink_form("#My Heading")` (empty file part) resolves
    // to a bare slugged anchor with no `moss-resolved:` prefix.
    // Emitted output: `[My Heading](#my-heading)`.
    let graph = build_graph(&["notes.md"]);
    let emit = dispatch_wikilink_embed(
        "#My Heading",
        None,
        false,
        &graph,
        "notes.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Link(link) => {
            assert!(link.contains("(#my-heading)"), "got: {}", link);
            assert!(!link.contains("moss-resolved:"), "got: {}", link);
        }
        other => panic!("expected Link, got {:?}", other),
    }
}

#[test]
fn build_anchor_block_ref_is_not_slugged() {
    // Helper-path test (NOT the live `[[…]]` link path — see SCOPE
    // WARNING above; the live guard is
    // `resolve_urls::wikilink_block_ref_keeps_id_raw`).
    //
    // Block refs (^id) are emitted RAW — NOT run through
    // obsidian_heading_anchor. Use a block-id with a space + uppercase
    // so slugging (which would yield "#block-id") is observably
    // different from the raw form ("#Block Id"). This fails loudly if
    // the `^` short-circuit in build_anchor regresses.
    // Emitted output: `[notes > ^Block Id](moss-resolved:notes.md#Block Id)`.
    let graph = build_graph(&["notes.md"]);
    let emit = dispatch_wikilink_embed(
        "notes#^Block Id",
        None,
        false,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Link(link) => {
            assert!(
                link.contains("#Block Id"),
                "expected raw block-ref, got: {}",
                link
            );
            assert!(
                !link.contains("#block-id"),
                "block-ref was slugged: {}",
                link
            );
        }
        other => panic!("expected Link, got {:?}", other),
    }
}

#[test]
fn dispatch_video_extension_routes_to_synth() {
    // Phase 3 PR4.5 (2026-05-27): non-image wikilinks now route
    // DIRECTLY to the per-kind synthesizer — the markdown round-trip
    // is gone (it was dropping the `moss:kind=…` title since PR2 and
    // entirely silent after PR4 deleted `parse_title`). The dispatcher
    // returns `EmitKind::Html` carrying the `<video>` byte shape; we
    // pin only the structural identity (element + src) so byte-shape
    // changes are owned by the synth tests in `render/video.rs`.
    let graph = build_graph(&["clip.mp4"]);
    let emit = dispatch_wikilink_embed(
        "clip.mp4",
        None,
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains("<video"), "expected <video>, got: {}", s);
            assert!(s.contains(r#"src="/clip.mp4""#), "expected src=, got: {}", s);
            assert!(s.contains("moss-embed-video"), "expected class, got: {}", s);
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

#[test]
fn dispatch_pdf_extension_routes_to_synth() {
    // See `dispatch_video_extension_routes_to_synth` for the PR4.5
    // routing rationale. PdfRenderer emits an `<object type="application/pdf">`.
    let graph = build_graph(&["report.pdf"]);
    let emit = dispatch_wikilink_embed(
        "report.pdf",
        None,
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains("<object"), "expected <object>, got: {}", s);
            assert!(
                s.contains(r#"data="/report.pdf""#),
                "expected data=, got: {}",
                s
            );
            assert!(
                s.contains(r#"type="application/pdf""#),
                "expected type=, got: {}",
                s
            );
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

#[test]
fn dispatch_audio_extension_routes_to_synth() {
    let graph = build_graph(&["song.mp3"]);
    let emit = dispatch_wikilink_embed(
        "song.mp3",
        None,
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains("<audio"), "expected <audio>, got: {}", s);
            assert!(s.contains(r#"src="/song.mp3""#), "expected src=, got: {}", s);
            assert!(
                s.contains(r#"type="audio/mpeg""#),
                "expected MIME, got: {}",
                s
            );
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

#[test]
fn dispatch_iframe_extension_routes_to_synth() {
    let graph = build_graph(&["widget.html"]);
    let emit = dispatch_wikilink_embed(
        "widget.html",
        None,
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains("<iframe"), "expected <iframe>, got: {}", s);
            assert!(
                s.contains(r#"src="/widget.html""#),
                "expected src=, got: {}",
                s
            );
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

#[test]
fn dispatch_model_extension_routes_to_synth() {
    let graph = build_graph(&["scene.glb"]);
    let emit = dispatch_wikilink_embed(
        "scene.glb",
        None,
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(
                s.contains("<model-viewer"),
                "expected <model-viewer>, got: {}",
                s
            );
            assert!(
                s.contains(r#"src="/scene.glb""#),
                "expected src=, got: {}",
                s
            );
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

#[test]
fn dispatch_iframe_alias_carries_title() {
    // `![[widget.html|Embedded Widget]]` — non-sizing alias text
    // surfaces on the iframe as the `title=` accessible name. The
    // synth function reads `params.get("title")`; `build_synth_params`
    // routes the alias there for iframe-kind.
    let graph = build_graph(&["widget.html"]);
    let emit = dispatch_wikilink_embed(
        "widget.html",
        Some("Embedded Widget"),
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains(r#"title="Embedded Widget""#), "got: {}", s);
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

#[test]
fn dispatch_video_sizing_alias_propagates_dims() {
    // `![[clip.mp4|640x360]]` — sizing alias becomes width/height
    // CSS-formatted on the <video>.
    let graph = build_graph(&["clip.mp4"]);
    let emit = dispatch_wikilink_embed(
        "clip.mp4",
        Some("640x360"),
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains(r#"width="640px""#), "got: {}", s);
            assert!(s.contains(r#"height="360px""#), "got: {}", s);
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

// --- Image display-attr dispatch (fit / position threading) ----------
//
// The polish-pass plan (docs/archive/2026-05-27-polish-passes-followups.md
// Item B) flagged that `![[hero.jpg|cover]]` and
// `![[hero.jpg|fit=cover position=left]]` were silently dropping
// fit/position. `ImageRenderer::render_to_markdown` builds `TitleParams`
// from the alias / pothole, then explicitly discards them with
// `let _ = params;` — the emitted markdown is bare `![](url)`. The
// dispatcher now intercepts these cases ahead of the renderer registry
// and emits a final `<img>` with the appropriate `style=`.

#[test]
fn dispatch_only_fires_for_wikilink_caller() {
    // This test documents the safety rule from v2 revision notes:
    // dispatch_wikilink_embed is the ONLY public entry point for
    // wikilink-form events. There is no parallel function for
    // LinkType::Inline. Plain `[link](file.pdf)` events stay as
    // markdown links via pulldown-cmark's default emission.
    //
    // We can't directly test what the caller does (that's in pipeline.rs),
    // but we can pin the invariant by asserting the API surface:
    // the public function takes `is_embed: bool` for `![[…]]` vs
    // `[[…]]`, not a `LinkType` enum that could be confused with Inline.

    // No assertion needed — the type signature itself is the check.
}
// ---- Image embed dispatch (image-embed synth-collapse) ---------------
//
// ALL `![[photo.jpg]]` forms now emit `EmitKind::Block(Block::Figure)`
// with full param threading (width / caption / fit / position / align).
// The OLD tests asserted `EmitKind::Inline("![](url)")` round-trips and
// a fit/position fast-path that DROPPED width — they encoded the bug
// this change fixes and were removed.

fn figure_of(emit: &WikilinkEmit) -> &crate::ast::node::Block {
    match &emit.output {
        EmitKind::Block(b) => b.as_ref(),
        other => panic!("expected EmitKind::Block(Figure), got {other:?}"),
    }
}

fn render_figure(emit: &WikilinkEmit) -> String {
    let block = match emit.output.clone() {
        EmitKind::Block(b) => *b,
        other => panic!("expected EmitKind::Block(Figure), got {other:?}"),
    };
    let doc = crate::ast::Document::from_blocks(vec![block]);
    crate::ast::render_document(&doc, &crate::ast::DefaultHooks::new())
}

fn dispatch_img(alias: Option<&str>) -> WikilinkEmit {
    let graph = build_graph(&["photo.jpg", "hero.jpg"]);
    dispatch_wikilink_embed(
        "photo.jpg",
        alias,
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    )
}

#[test]
fn dispatch_image_plain_emits_figure_block() {
    use crate::ast::node::{Block, Inline};
    let emit = dispatch_img(None);
    match figure_of(&emit) {
        Block::Figure {
            image,
            caption,
            width,
            align,
            class_names,
            img_style,
        } => {
            assert!(caption.is_none(), "plain embed: no caption");
            assert!(width.is_none());
            assert!(align.is_none());
            assert!(class_names.is_empty());
            assert!(img_style.is_none());
            match image {
                Inline::Image {
                    src,
                    alt,
                    is_wikilink,
                    ..
                } => {
                    assert!(src.is_resolved());
                    assert_eq!(alt, "");
                    assert!(*is_wikilink);
                }
                other => panic!("expected Image, got {other:?}"),
            }
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn dispatch_image_caption_text_keeps_alt_in_the_node_but_not_in_the_html() {
    use crate::ast::node::{Block, Inline};
    let emit = dispatch_img(Some("My caption"));
    match figure_of(&emit) {
        Block::Figure { image, caption, .. } => {
            let cap = caption.as_ref().expect("caption present");
            assert_eq!(cap.len(), 1);
            match &cap[0] {
                Inline::Text(t) => assert_eq!(t, "My caption"),
                other => panic!("expected caption Text, got {other:?}"),
            }
            // The NODE keeps the alias in `alt` — it is the alias's only
            // home, and consumers that never see a caption (plain-text
            // extraction, an unwound figure) read it from here.
            match image {
                Inline::Image { alt, .. } => assert_eq!(alt, "My caption"),
                other => panic!("expected Image, got {other:?}"),
            }
        }
        other => panic!("expected Figure, got {other:?}"),
    }
    // The HTML does not: the `<figcaption>` is already announced as part
    // of the figure, so repeating it in `alt` makes a screen reader say
    // the same sentence twice. The renderer drops it — see
    // `ast::render`'s Figure arm.
    let html = render_figure(&emit);
    assert!(html.contains(r#"alt="""#), "got: {html}");
    assert!(
        html.contains("<figcaption>My caption</figcaption>"),
        "got: {html}"
    );
}

#[test]
fn dispatch_image_width_token_preserved_as_data_width() {
    use crate::ast::node::Block;
    // FIX: width is no longer dropped — it lands as figure data-width=.
    let emit = dispatch_img(Some("wide"));
    match figure_of(&emit) {
        Block::Figure { width, caption, .. } => {
            assert_eq!(width.as_deref(), Some("wide"));
            assert!(caption.is_none(), "width token is not a caption");
        }
        other => panic!("expected Figure, got {other:?}"),
    }
    let html = render_figure(&emit);
    assert!(html.contains(r#"data-width="wide""#), "got: {html}");
}

#[test]
fn dispatch_image_cover_emits_object_fit_on_inner_img() {
    use crate::ast::node::Block;
    let emit = dispatch_img(Some("cover"));
    match figure_of(&emit) {
        Block::Figure {
            img_style, caption, ..
        } => {
            assert_eq!(img_style.as_deref(), Some("object-fit:cover"));
            assert!(caption.is_none(), "structural alias is not a caption");
        }
        other => panic!("expected Figure, got {other:?}"),
    }
    let html = render_figure(&emit);
    assert!(html.contains("object-fit:cover"), "got: {html}");
    assert!(
        html.contains(r#"<figure class="moss-image""#),
        "got: {html}"
    );
}

#[test]
fn dispatch_image_cover_left_emits_fit_and_position() {
    use crate::ast::node::Block;
    let emit = dispatch_img(Some("cover left"));
    match figure_of(&emit) {
        Block::Figure { img_style, .. } => {
            let style = img_style.as_deref().expect("style present");
            assert!(style.contains("object-fit:cover"), "got: {style}");
            assert!(style.contains("object-position:left"), "got: {style}");
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn dispatch_image_params_form_emits_object_fit() {
    use crate::ast::node::Block;
    let emit = dispatch_img(Some("fit=cover"));
    match figure_of(&emit) {
        Block::Figure { img_style, .. } => {
            assert_eq!(img_style.as_deref(), Some("object-fit:cover"));
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn dispatch_image_two_word_position_combines() {
    use crate::ast::node::Block;
    let emit = dispatch_img(Some("cover top left"));
    match figure_of(&emit) {
        Block::Figure { img_style, .. } => {
            let style = img_style.as_deref().expect("style present");
            assert!(style.contains("object-fit:cover"), "got: {style}");
            assert!(style.contains("object-position:top left"), "got: {style}");
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn dispatch_image_wide_cover_combines_width_and_fit() {
    use crate::ast::node::Block;
    // width → figure data-width; fit → inner <img> style. Both survive
    // (the pre-collapse fast-path DROPPED width when fit was present).
    let emit = dispatch_img(Some("wide cover"));
    match figure_of(&emit) {
        Block::Figure {
            width, img_style, ..
        } => {
            assert_eq!(width.as_deref(), Some("wide"));
            assert_eq!(img_style.as_deref(), Some("object-fit:cover"));
        }
        other => panic!("expected Figure, got {other:?}"),
    }
    let html = render_figure(&emit);
    assert!(html.contains(r#"data-width="wide""#), "got: {html}");
    assert!(html.contains("object-fit:cover"), "got: {html}");
}

#[test]
fn dispatch_image_inner_img_has_single_style_attr() {
    // Inner <img> carries exactly one style= (object-fit), no LQIP dup
    // (no snapshot here).
    let emit = dispatch_img(Some("fit=cover"));
    let html = render_figure(&emit);
    let n = html.matches("style=").count();
    assert_eq!(n, 1, "exactly one style= attr, got {n}: {html}");
}

// Editor Image UX (2026-06-04): wikilink `|NN%` percent width carries
// into Block::Figure.width instead of leaking into the caption.
// -------------------------------------------------------------------

#[test]
fn wikilink_image_percent_carries_width() {
    use crate::ast::node::Block;
    // ![[pic.jpg|55%]] → Figure { width: Some("55%") }, no bogus caption
    let emit = dispatch_img(Some("55%"));
    match figure_of(&emit) {
        Block::Figure { width, caption, .. } => {
            assert_eq!(width.as_deref(), Some("55%"));
            assert!(caption.is_none(), "percent must not become a caption");
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

#[test]
fn wikilink_image_percent_with_caption() {
    use crate::ast::node::{Block, Inline};
    // ![[pic.jpg|My cap|55%]] → width Some("55%"), caption "My cap"
    let emit = dispatch_img(Some("My cap|55%"));
    match figure_of(&emit) {
        Block::Figure { width, caption, .. } => {
            assert_eq!(width.as_deref(), Some("55%"));
            let cap = caption.as_ref().expect("caption present");
            assert!(
                matches!(cap.as_slice(), [Inline::Text(t)] if t == "My cap"),
                "caption should be the non-width segment, got {cap:?}"
            );
        }
        other => panic!("expected Figure, got {other:?}"),
    }
}

// --- External URL dispatch ---

#[test]
fn dispatch_external_url_youtube_emits_html() {
    let graph = build_graph(&[]);
    let emit = dispatch_wikilink_embed(
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        None,
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains("<iframe"), "got: {s}");
            assert!(s.contains("youtube.com/embed"), "got: {s}");
            assert!(s.contains(r#"data-provider="youtube""#), "got: {s}");
        }
        other => panic!("expected Html, got: {other:?}"),
    }
    assert!(
        emit.outgoing_link.is_none(),
        "external URLs must not register in ContentGraph"
    );
    assert!(emit.diagnostics.is_empty());
}

#[test]
fn dispatch_external_url_generic_emits_html() {
    let graph = build_graph(&[]);
    let emit = dispatch_wikilink_embed(
        "https://example.com/embed",
        None,
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains("<iframe"), "got: {s}");
            assert!(s.contains(r#"src="https://example.com/embed""#), "got: {s}");
            assert!(
                !s.contains("data-provider="),
                "generic must not have data-provider, got: {s}"
            );
        }
        other => panic!("expected Html, got: {other:?}"),
    }
    assert!(emit.outgoing_link.is_none());
}

#[test]
fn dispatch_external_url_http_also_works() {
    let graph = build_graph(&[]);
    let emit = dispatch_wikilink_embed(
        "http://example.com/embed",
        None,
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => assert!(s.contains("<iframe"), "got: {s}"),
        other => panic!("expected Html, got: {other:?}"),
    }
}

#[test]
fn dispatch_external_url_with_width_pothole() {
    let graph = build_graph(&[]);
    let emit = dispatch_wikilink_embed(
        "https://vimeo.com/123456789",
        Some("wide"),
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => assert!(s.contains(r#"data-width="wide""#), "got: {s}"),
        other => panic!("expected Html, got: {other:?}"),
    }
}

// --- |loop ambient-video parser arm (spec §3.3a) ----------------------

#[test]
fn dispatch_video_loop_alias_emits_ambient_set() {
    // `![[clip.mp4|loop]]` — bare `loop` token sets the ambient playback set
    // and suppresses controls.
    let graph = build_graph(&["clip.mp4"]);
    let emit = dispatch_wikilink_embed(
        "clip.mp4",
        Some("loop"),
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains(" autoplay"), "missing autoplay, got: {}", s);
            assert!(s.contains(" muted"), "missing muted, got: {}", s);
            assert!(s.contains(" loop"), "missing loop, got: {}", s);
            assert!(
                s.contains(" playsinline"),
                "missing playsinline, got: {}",
                s
            );
            assert!(
                !s.contains(" controls"),
                "controls must be absent on loop branch, got: {}",
                s
            );
            assert!(s.contains(" data-loop"), "missing data-loop, got: {}", s);
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

#[test]
fn dispatch_video_loop_alias_case_insensitive() {
    // `![[clip.mp4|LOOP]]` — loop keyword must be case-insensitive.
    let graph = build_graph(&["clip.mp4"]);
    let emit = dispatch_wikilink_embed(
        "clip.mp4",
        Some("LOOP"),
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(
                s.contains(" autoplay"),
                "missing autoplay on LOOP alias, got: {}",
                s
            );
            assert!(
                !s.contains(" controls"),
                "controls must be absent on LOOP alias, got: {}",
                s
            );
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

#[test]
fn dispatch_video_size_and_loop_alias_propagates_all() {
    // `![[clip.mp4|640x360 loop]]` — sizing AND loop must both be set;
    // order within the alias is irrelevant to the output.
    let graph = build_graph(&["clip.mp4"]);
    let emit = dispatch_wikilink_embed(
        "clip.mp4",
        Some("640x360 loop"),
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains(r#"width="640px""#), "missing width, got: {}", s);
            assert!(
                s.contains(r#"height="360px""#),
                "missing height, got: {}",
                s
            );
            assert!(s.contains(" autoplay"), "missing autoplay, got: {}", s);
            assert!(s.contains(" data-loop"), "missing data-loop, got: {}", s);
            assert!(
                !s.contains(" controls"),
                "controls must be absent, got: {}",
                s
            );
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

#[test]
fn dispatch_video_loop_first_then_size() {
    // `![[clip.mp4|loop 640x360]]` — loop before size is also valid
    // (order-independent within the alias).
    let graph = build_graph(&["clip.mp4"]);
    let emit = dispatch_wikilink_embed(
        "clip.mp4",
        Some("loop 640x360"),
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains(r#"width="640px""#), "missing width, got: {}", s);
            assert!(
                s.contains(r#"height="360px""#),
                "missing height, got: {}",
                s
            );
            assert!(s.contains(" autoplay"), "missing autoplay, got: {}", s);
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}

#[test]
fn dispatch_video_sizing_alias_still_works_without_loop() {
    // `![[clip.mp4|640x360]]` — sizing without loop must NOT emit autoplay
    // (backward-compat; non-loop path unchanged).
    let graph = build_graph(&["clip.mp4"]);
    let emit = dispatch_wikilink_embed(
        "clip.mp4",
        Some("640x360"),
        true,
        &graph,
        "index.md",
        &empty_snapshot(),
    );
    match emit.output {
        EmitKind::Html(s) => {
            assert!(s.contains(r#"width="640px""#), "missing width, got: {}", s);
            assert!(
                !s.contains(" autoplay"),
                "autoplay must NOT be emitted without loop, got: {}",
                s
            );
            assert!(
                s.contains(" controls"),
                "controls must be emitted on non-loop path, got: {}",
                s
            );
        }
        other => panic!("expected Html, got: {:?}", other),
    }
}
