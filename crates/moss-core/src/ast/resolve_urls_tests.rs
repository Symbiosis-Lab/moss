use super::*;
use crate::ast::parser::parse;
use crate::content_graph::ContentGraphBuilder;

fn graph_with(paths: &[&str]) -> crate::content_graph::ContentGraph {
    let mut b = ContentGraphBuilder::new();
    for p in paths {
        b.add_file(p, p);
    }
    b.build()
}

// -----------------------------------------------------------------
// Single-shot resolve_urls behavior
// -----------------------------------------------------------------

#[test]
fn markdown_link_fragment_preserved_raw_not_slugged() {
    // Unit test of `split_path_suffix` PURITY: it splits path from
    // suffix but never slugs — the returned suffix is byte-identical to
    // the source. (Slugging, when it happens, is layered on top by
    // `slug_wikilink_suffix`, exercised separately.)
    //
    // Design split (corrected): a MARKDOWN link (`[t](page#Heading)`) is
    // a literal URL — its `#fragment` stays RAW by design, so authored
    // `#L42` / hand-authored ids / external anchors survive untouched.
    // A WIKILINK (`[[page#Heading]]`) is NOT a literal URL: its fragment
    // IS slugged to match the rendered heading id — `resolve_link_urls`
    // routes wikilinks through `slug_wikilink_suffix` (see the
    // end-to-end guard `markdown_link_fragment_stays_raw_not_slugged`
    // and the `wikilink_*_fragment_is_slugged` tests below). The earlier
    // claim that "authoring correctness comes from editor autocomplete"
    // was the flawed premise behind the link-path bug; wikilink slugging
    // now happens in resolve_urls itself.
    let (path, suffix) = split_path_suffix("page#My Heading");
    assert_eq!(path, "page");
    assert_eq!(suffix, Some("#My Heading")); // raw, spaces + case intact
}

#[test]
fn resolves_standard_markdown_link_to_internal() {
    let mut doc = parse("[文字](文字.md)");
    let graph = graph_with(&["index.md", "文字/文字.md"]);
    let outgoing = resolve_urls(&mut doc, &graph, "index.md").outgoing;

    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].target_path, "文字/文字.md");
    assert_eq!(outgoing[0].display_text, "文字");
    assert_eq!(outgoing[0].link_type, LinkType::Standard);

    // Phase 4 PR7a-stage1b (2026-05-28): the visitor emits a
    // `moss-resolved:` sentinel for internal links (Url::Unresolved)
    // so src-tauri's host classifier can decode it via page_map.
    // The renderer doesn't see this state — the host's
    // `classify_url_prod` pass replaces Unresolved before render.
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link { url, .. } => {
                assert!(url.is_unresolved(), "expected sentinel, got: {url:?}");
                match url {
                    Url::Unresolved(s) => assert_eq!(s, "moss-resolved:文字/文字.md"),
                    Url::Resolved(_) => unreachable!(),
                }
            }
            _ => panic!("expected Link"),
        },
        _ => panic!("expected Paragraph"),
    }
}

#[test]
fn passes_through_external_link() {
    let mut doc = parse("[ex](https://example.com)");
    let graph = graph_with(&["index.md"]);
    let outgoing = resolve_urls(&mut doc, &graph, "index.md").outgoing;

    assert!(outgoing.is_empty());
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link { url, .. } => {
                let Url::Resolved(r) = url else {
                    panic!("expected Resolved, got {url:?}")
                };
                assert_eq!(r.kind, UrlKind::External);
                assert_eq!(r.href, "https://example.com");
            }
            _ => panic!("expected Link"),
        },
        _ => panic!("expected Paragraph"),
    }
}

#[test]
fn classifies_anchor_link() {
    let mut doc = parse("[top](#top)");
    let graph = graph_with(&["index.md"]);
    let outgoing = resolve_urls(&mut doc, &graph, "index.md").outgoing;
    assert!(outgoing.is_empty());
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link { url, .. } => {
                let Url::Resolved(r) = url else {
                    panic!("expected Resolved, got {url:?}")
                };
                assert_eq!(r.kind, UrlKind::Anchor);
                assert_eq!(r.href, "#top");
            }
            _ => panic!("expected Link"),
        },
        _ => panic!("expected Paragraph"),
    }
}

#[test]
fn classifies_mailto() {
    let mut doc = parse("[Mail](mailto:test@example.com)");
    let graph = graph_with(&["index.md"]);
    let _ = resolve_urls(&mut doc, &graph, "index.md").outgoing;
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link { url, .. } => {
                let Url::Resolved(r) = url else {
                    panic!("expected Resolved, got {url:?}")
                };
                assert_eq!(r.kind, UrlKind::Mailto);
                assert_eq!(r.href, "mailto:test@example.com");
            }
            _ => panic!("expected Link"),
        },
        _ => panic!("expected Paragraph"),
    }
}

#[test]
fn resolves_bare_filename_image_against_graph() {
    let mut doc = parse("![My Photo](photo.jpg)");
    let mut b = ContentGraphBuilder::new();
    b.add_file("assets/photo.jpg", "photo");
    let graph = b.build();
    let outgoing = resolve_urls(&mut doc, &graph, "articles/post.md").outgoing;

    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].target_path, "assets/photo.jpg");
    assert_eq!(outgoing[0].display_text, "My Photo");
    assert_eq!(outgoing[0].link_type, LinkType::Standard);

    // Image src rewritten to relative asset path.
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Image { src, .. } => {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "/assets/photo.jpg");
                assert_eq!(r.kind, UrlKind::Asset);
            }
            Inline::Link {
                children: link_kids,
                ..
            } => {
                // pulldown-cmark may wrap an image-only paragraph in a
                // figure or other structure depending on detection;
                // accept either the direct image or one-level
                // deeper.
                if let Some(Inline::Image { src, .. }) = link_kids.first() {
                    let Url::Resolved(r) = src else {
                        panic!("expected Resolved, got {src:?}")
                    };
                    assert_eq!(r.href, "/assets/photo.jpg");
                }
            }
            _ => panic!("expected Image, got {children:?}"),
        },
        Block::Figure { image, .. } => {
            // PR3's Block::Figure: image-only paragraph may parse as
            // Figure directly.
            if let Inline::Image { src, .. } = image {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "/assets/photo.jpg");
            }
        }
        _ => panic!("expected Paragraph or Figure, got {:?}", doc.blocks[0]),
    }
}

#[test]
fn unresolved_bare_filename_passes_through() {
    let mut doc = parse("![](nonexistent.jpg)");
    let graph = graph_with(&["index.md"]);
    let outgoing = resolve_urls(&mut doc, &graph, "articles/post.md").outgoing;

    assert!(outgoing.is_empty());
    // URL stays as raw "nonexistent.jpg" but becomes Resolved (Asset
    // kind) so the renderer's invariant holds.
    let mut found_image = false;
    for block in &doc.blocks {
        if let Block::Paragraph(children) = block {
            for inline in children {
                if let Inline::Image { src, .. } = inline {
                    let Url::Resolved(r) = src else {
                        panic!("expected Resolved, got {src:?}")
                    };
                    assert_eq!(r.href, "nonexistent.jpg");
                    assert_eq!(r.kind, UrlKind::Asset);
                    found_image = true;
                }
            }
        }
        if let Block::Figure { image, .. } = block {
            if let Inline::Image { src, .. } = image {
                let Url::Resolved(r) = src else {
                    panic!("expected Resolved, got {src:?}")
                };
                assert_eq!(r.href, "nonexistent.jpg");
                found_image = true;
            }
        }
    }
    assert!(found_image, "expected an Inline::Image in the parsed doc");
}

#[test]
fn does_not_resolve_url_inside_code_block() {
    // visit_urls_mut never descends into Block::CodeBlock, so the
    // visitor never sees URLs in code fences. This matches Stage 1's
    // fence-aware behavior structurally.
    let mut doc = parse("```\n[link](inside.md)\n```\n");
    let graph = graph_with(&["index.md", "inside.md"]);
    let outgoing = resolve_urls(&mut doc, &graph, "index.md").outgoing;
    assert!(
        outgoing.is_empty(),
        "code block content must not produce OutgoingLink"
    );
}

#[test]
fn fragment_preserved_on_internal_link() {
    let mut doc = parse("[x](文字/文字.md#sec)");
    let graph = graph_with(&["index.md", "文字/文字.md"]);
    let outgoing = resolve_urls(&mut doc, &graph, "index.md").outgoing;

    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].target_path, "文字/文字.md");

    // Sentinel emit: suffix concatenated verbatim after the resolved path.
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link { url, .. } => match url {
                Url::Unresolved(s) => assert_eq!(s, "moss-resolved:文字/文字.md#sec"),
                Url::Resolved(r) => panic!("expected sentinel, got Resolved({r:?})"),
            },
            _ => panic!("expected Link"),
        },
        _ => panic!("expected Paragraph"),
    }
}

#[test]
fn query_string_preserved_on_internal_link() {
    let mut b = ContentGraphBuilder::new();
    b.add_file("index.md", "x");
    b.add_file("assets/scale-compare.html", "h");
    let graph = b.build();

    let mut doc = parse("[demo](scale-compare.html?a=major_pent&r=major_pent%3AD)");
    let outgoing = resolve_urls(&mut doc, &graph, "index.md").outgoing;

    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].target_path, "assets/scale-compare.html");

    // Sentinel emit: suffix concatenated verbatim after the resolved path.
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link { url, .. } => match url {
                Url::Unresolved(s) => assert_eq!(
                    s,
                    "moss-resolved:assets/scale-compare.html?a=major_pent&r=major_pent%3AD"
                ),
                Url::Resolved(r) => panic!("expected sentinel, got Resolved({r:?})"),
            },
            _ => panic!("expected Link"),
        },
        _ => panic!("expected Paragraph"),
    }
}

// -----------------------------------------------------------------
// OutgoingLink + sentinel-shape coverage
// -----------------------------------------------------------------
//
// Phase 4 PR7a-stage1b (2026-05-28): the Stage 1 pass
// `markdown_links::resolve_markdown_links` was deleted in this PR
// alongside the matching `byte_equivalence_*` baseline helpers. The
// visitor now emits the same `moss-resolved:<path>` sentinel Stage 1
// emitted, byte-for-byte — proven by the per-test sentinel
// assertions below. The companion Stage 1 pass
// `markdown_refs::resolve_markdown_refs` was already deleted in the
// prior PR; its parity is covered by
// `resolves_bare_filename_image_against_graph` above.

#[test]
fn standard_markdown_link_emits_sentinel() {
    let source = "index.md";
    let content = "[文字](文字.md)";
    let graph = graph_with(&["index.md", "文字/文字.md"]);

    let mut doc = parse(content);
    let visitor = resolve_urls(&mut doc, &graph, source).outgoing;

    assert_eq!(visitor.len(), 1);
    assert_eq!(visitor[0].target_path, "文字/文字.md");
    assert_eq!(visitor[0].display_text, "文字");
    assert_eq!(visitor[0].link_type, LinkType::Standard);
    // The sentinel shape is what `classify_url_prod` in src-tauri
    // expects to decode via `page_map` / `external_url_map`.
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link {
                url: Url::Unresolved(s),
                ..
            } => {
                assert_eq!(s, "moss-resolved:文字/文字.md");
            }
            _ => panic!("expected Url::Unresolved sentinel, got {:?}", children[0]),
        },
        _ => panic!("expected Paragraph"),
    }
}

#[test]
fn multiple_links_one_line_emit_sentinels() {
    let source = "index.md";
    let content = "[a](foo.md) and [b](bar.md)";
    let graph = graph_with(&["index.md", "foo.md", "bar.md"]);

    let mut doc = parse(content);
    let visitor = resolve_urls(&mut doc, &graph, source).outgoing;

    assert_eq!(visitor.len(), 2);
    assert_eq!(visitor[0].target_path, "foo.md");
    assert_eq!(visitor[1].target_path, "bar.md");
}

#[test]
fn external_links_no_outgoing() {
    let source = "index.md";
    let content = "[ext](https://example.com) [anchor](#top) [mail](mailto:a@b)";
    let graph = graph_with(&["index.md"]);

    let mut doc = parse(content);
    let visitor = resolve_urls(&mut doc, &graph, source).outgoing;

    assert!(visitor.is_empty());
}

#[test]
fn unresolved_link_no_outgoing() {
    let source = "index.md";
    let content = "[missing](missing.md)";
    let graph = graph_with(&["index.md"]);

    let mut doc = parse(content);
    let visitor = resolve_urls(&mut doc, &graph, source).outgoing;

    assert!(visitor.is_empty());
    // The unresolved URL stays as-is (no sentinel) but is marked
    // Url::Resolved so the renderer's invariant holds.
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link { url, .. } => {
                let Url::Resolved(r) = url else {
                    panic!("expected Resolved, got {url:?}")
                };
                assert_eq!(r.href, "missing.md");
                assert_eq!(r.kind, UrlKind::Internal);
            }
            _ => panic!("expected Link"),
        },
        _ => panic!("expected Paragraph"),
    }
}

#[test]
fn code_block_urls_not_visited() {
    let source = "index.md";
    let content =
        "Before\n\n```\n[link](inside.md)\n![](photo.jpg)\n```\n\nAfter [link](inside.md).";
    let mut b = ContentGraphBuilder::new();
    b.add_file("index.md", "x");
    b.add_file("inside.md", "i");
    b.add_file("assets/photo.jpg", "p");
    let graph = b.build();

    let mut doc = parse(content);
    let visitor = resolve_urls(&mut doc, &graph, source).outgoing;

    // Only the trailing `[link](inside.md)` (outside the fence) emits
    // an OutgoingLink. URLs inside `Block::CodeBlock` are not visited.
    assert_eq!(visitor.len(), 1);
    assert_eq!(visitor[0].target_path, "inside.md");
}

#[test]
fn query_and_fragment_sentinel_shape() {
    let source = "index.md";
    let content = "[d](app.html?x=1#sec)";
    let mut b = ContentGraphBuilder::new();
    b.add_file("index.md", "x");
    b.add_file("assets/app.html", "h");
    let graph = b.build();

    let mut doc = parse(content);
    let visitor = resolve_urls(&mut doc, &graph, source).outgoing;

    assert_eq!(visitor.len(), 1);
    assert_eq!(visitor[0].target_path, "assets/app.html");
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link {
                url: Url::Unresolved(s),
                ..
            } => {
                assert_eq!(s, "moss-resolved:assets/app.html?x=1#sec");
            }
            _ => panic!("expected sentinel, got {:?}", children[0]),
        },
        _ => panic!("expected Paragraph"),
    }
}

#[test]
fn link_wrapping_image_target_path() {
    // Shape produced by `[![[image.png]]](target.html?q)` after the
    // wikilinks pass rewrites the embed to `![alt](path)`.
    // Pre-PR7a-stage1b this test compared to a Stage 1 baseline that
    // used the raw markdown source between `[` and `]` for
    // display_text; the visitor uses parsed plain text (alt text).
    // That divergence was non-breaking (display_text has no
    // production consumer). With Stage 1 deleted we assert on the
    // visitor's behavior directly: load-bearing fields (target_path,
    // link_type) plus the documented display_text.
    //
    // Task 6 (asset-engine routing, 2026-06-03): the engine now also
    // resolves separator-path image references through the graph and
    // emits an OutgoingLink for the discovered dependency edge. So
    // this test now expects TWO OutgoingLink entries:
    //   [0] — image: assets/scale-compare.png (phase 1, image resolver)
    //   [1] — link: assets/scale-compare.html (phase 2, link resolver)
    // Previously [0] was absent because separator-path images were
    // passed through verbatim (the 404 bug). The href for the image
    // is unchanged ("assets/scale-compare.png" from index.md root).
    let source = "index.md";
    let content = "[![scale-compare](assets/scale-compare.png)](scale-compare.html?a=major_pent&r=major_pent%3AD)";
    let mut b = ContentGraphBuilder::new();
    b.add_file("index.md", "x");
    b.add_file("assets/scale-compare.html", "h");
    b.add_file("assets/scale-compare.png", "p");
    let graph = b.build();

    let mut doc = parse(content);
    let visitor = resolve_urls(&mut doc, &graph, source).outgoing;

    // Phase 1 emits the image dependency edge; phase 2 emits the link.
    assert_eq!(
        visitor.len(),
        2,
        "expected image + link OutgoingLinks, got: {visitor:?}"
    );
    // Find the link entry by target (order: phase 1 image first, then phase 2 link).
    let link_entry = visitor
        .iter()
        .find(|o| o.target_path == "assets/scale-compare.html")
        .expect("OutgoingLink for scale-compare.html not found");
    assert_eq!(link_entry.link_type, LinkType::Standard);
    assert_eq!(link_entry.display_text, "scale-compare");
    // Image dependency edge also present.
    assert!(
        visitor
            .iter()
            .any(|o| o.target_path == "assets/scale-compare.png"),
        "OutgoingLink for scale-compare.png not found"
    );
}

// -----------------------------------------------------------------
// Edge cases
// -----------------------------------------------------------------

#[test]
fn pipe_bearing_image_url_unchanged() {
    let mut doc = parse("![alt](photo.jpg|contain)");
    let mut b = ContentGraphBuilder::new();
    b.add_file("assets/photo.jpg", "p");
    let graph = b.build();
    let outgoing = resolve_urls(&mut doc, &graph, "articles/post.md").outgoing;

    // Phase 3 PR3 contract: pipe-bearing URLs pass through verbatim,
    // no OutgoingLink emitted.
    assert!(outgoing.is_empty());
}

#[test]
fn idempotent_on_already_resolved_url() {
    // If the document already carries Resolved URLs (e.g. a previous
    // pass ran), the visitor should not double-process. Single
    // invocation should produce the SAME state.
    let mut doc = parse("[文字](文字.md)");
    let graph = graph_with(&["index.md", "文字/文字.md"]);
    let outgoing1 = resolve_urls(&mut doc, &graph, "index.md").outgoing;

    let outgoing2 = resolve_urls(&mut doc, &graph, "index.md").outgoing;
    // After the first pass everything is Resolved; the second pass
    // produces no new OutgoingLink entries.
    assert!(
        outgoing2.is_empty(),
        "idempotency violated: {:?}",
        outgoing2
    );
    assert_eq!(outgoing1.len(), 1);
}

#[test]
fn absolute_path_passes_through() {
    let mut doc = parse("[abs](/about.html)");
    let graph = graph_with(&["index.md", "about.html"]);
    let outgoing = resolve_urls(&mut doc, &graph, "index.md").outgoing;
    // Absolute paths bypass the graph (mirrors markdown_links).
    assert!(outgoing.is_empty());
    match &doc.blocks[0] {
        Block::Paragraph(children) => match &children[0] {
            Inline::Link { url, .. } => {
                let Url::Resolved(r) = url else {
                    panic!("expected Resolved, got {url:?}")
                };
                assert_eq!(r.href, "/about.html");
            }
            _ => panic!("expected Link"),
        },
        _ => panic!("expected Paragraph"),
    }
}

// -----------------------------------------------------------------
// Hero / Gallery shortcode image resolution
// -----------------------------------------------------------------
//
// Regression coverage for the chps-site home hero regression
// (2026-05-29): `:::hero` with a body-image fallback `![[hero.jpg]]`
// (or `image=hero.jpg` attribute) stores the wikilink target as a
// `Url::Unresolved("hero.jpg")` on `HeroShortcode::image`. Before the
// fix, `walk_images_in_shortcode`'s Hero arm explicitly skipped that
// field, deferring to `classify_remaining_urls` — but the fallback
// classifier only assigns a `UrlKind`, never consulting the
// ContentGraph. Result: the renderer emitted `<img src="hero.jpg">`
// instead of the depth-correct `assets/hero.jpg`. The fix routes
// `args.image` (Hero) and `item.src` (Gallery) through the same
// bare-filename graph lookup that `Inline::Image` already uses.

fn extract_hero_image_href(doc: &Document) -> Option<String> {
    for block in &doc.blocks {
        if let Block::Shortcode(Shortcode::Hero(args)) = block {
            if let Some(Url::Resolved(r)) = &args.image {
                return Some(r.href.clone());
            }
            return None;
        }
    }
    None
}

#[test]
fn hero_body_wikilink_resolves_against_graph_at_depth_0() {
    // chps-site home page shape: `:::hero` with `![[hero.jpg]]`
    // wikilink as the body-image fallback. The asset lives at
    // `assets/hero.jpg` on disk, so the emitted href is that file's pinned
    // URL — not the bare wikilink target.
    let mut doc = parse(":::hero\n![[hero.jpg]]\n# Welcome\n:::\n");
    let mut b = ContentGraphBuilder::new();
    b.add_file("index.md", "home");
    b.add_file("assets/hero.jpg", "hero");
    let graph = b.build();
    let outgoing = resolve_urls(&mut doc, &graph, "index.md").outgoing;

    let href = extract_hero_image_href(&doc).expect("hero image must be Resolved");
    assert_eq!(
        href, "/assets/hero.jpg",
        "hero body-wikilink must resolve to the asset's pinned URL, got {href:?}"
    );
    // OutgoingLink registers the discovered dependency edge.
    assert!(
        outgoing.iter().any(|o| o.target_path == "assets/hero.jpg"),
        "expected OutgoingLink to assets/hero.jpg, got {outgoing:?}"
    );
}

#[test]
fn hero_extra_images_resolve_against_graph_like_the_primary() {
    // Multi-image hero: every extra slide resolves through the content
    // graph exactly like the primary and registers a dependency edge —
    // review finding on 06585a7cd (the chps-site regression class,
    // re-introduced per slide).
    let mut doc = parse(":::hero\n![[hero.jpg]]\n![[second.jpg]]\n# Welcome\n:::\n");
    let mut b = ContentGraphBuilder::new();
    b.add_file("index.md", "home");
    b.add_file("assets/hero.jpg", "hero");
    b.add_file("assets/second.jpg", "second");
    let graph = b.build();
    let outgoing = resolve_urls(&mut doc, &graph, "index.md").outgoing;

    let extras: Vec<String> = doc
        .blocks
        .iter()
        .find_map(|blk| match blk {
            Block::Shortcode(Shortcode::Hero(h)) => Some(
                h.extra_images
                    .iter()
                    .map(|u| match u {
                        Url::Resolved(r) => r.href.clone(),
                        Url::Unresolved(s) => format!("UNRESOLVED:{s}"),
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("hero present");
    assert_eq!(extras, vec!["/assets/second.jpg".to_string()], "{extras:?}");
    assert!(
        outgoing
            .iter()
            .any(|o| o.target_path == "assets/second.jpg"),
        "expected OutgoingLink to assets/second.jpg, got {outgoing:?}"
    );
}

#[test]
fn hero_body_wikilink_href_does_not_depend_on_referencing_depth() {
    // The same asset, referenced from a note one directory deep, emits the
    // SAME href as from the home page. Before the pinned-URL rule this test
    // asserted `../assets/hero.jpg` — a source-relative path that a later
    // string pass had to patch again (the referencing page is served at
    // `/articles/post/`, one level deeper than `articles/post.md`, so `../`
    // pointed at `/articles/assets/`). Depth is now structurally out of the
    // answer instead of being compensated for. moss#903 bug 3.
    let mut doc = parse(":::hero\n![[hero.jpg]]\n:::\n");
    let mut b = ContentGraphBuilder::new();
    b.add_file("articles/post.md", "post");
    b.add_file("index.md", "home");
    b.add_file("assets/hero.jpg", "hero");
    let graph = b.build();
    let _ = resolve_urls(&mut doc, &graph, "articles/post.md").outgoing;
    let deep = extract_hero_image_href(&doc).expect("hero image must be Resolved");

    let mut doc_root = parse(":::hero\n![[hero.jpg]]\n:::\n");
    let _ = resolve_urls(&mut doc_root, &graph, "index.md").outgoing;
    let root = extract_hero_image_href(&doc_root).expect("hero image must be Resolved");

    assert_eq!(deep, "/assets/hero.jpg", "got {deep:?}");
    assert_eq!(deep, root, "depth must not change the emitted href");
}

#[test]
fn hero_unresolved_wikilink_passes_through() {
    // If the wikilink target isn't in the graph, leave the URL as
    // the author wrote it (Resolved Asset kind, so the renderer
    // invariant holds). Mirrors `unresolved_bare_filename_passes_through`.
    let mut doc = parse(":::hero\n![[missing.jpg]]\n:::\n");
    let graph = graph_with(&["index.md"]);
    let _ = resolve_urls(&mut doc, &graph, "index.md").outgoing;

    let href = extract_hero_image_href(&doc).expect("hero image must be Resolved");
    assert_eq!(href, "missing.jpg");
}

// -----------------------------------------------------------------
// Wikilink #fragment slugging (keystone bug fix)
//
// Authored `[[Page#Heading]]` wikilinks must resolve to a SLUGGED
// fragment so the emitted href matches the rendered heading id
// (`<h2 id="getting-started">`). Regular markdown links `[x](page#frag)`
// stay RAW (a markdown link is a literal URL). The discriminator is
// `Inline::Link::is_wikilink`, which `parse()` sets for `[[…]]` syntax
// (ENABLE_WIKILINKS). Block refs (`#^id`) keep the id raw minus the
// caret, mirroring `wikilink_dispatch::build_anchor`.
// -----------------------------------------------------------------

/// Pull the resolved Url string out of the first Link inline in the
/// first paragraph. Works for both `Url::Unresolved` (sentinel) and
/// `Url::Resolved` (anchor / external) variants.
fn first_link_href(doc: &Document) -> String {
    match &doc.blocks[0] {
        Block::Paragraph(children) => {
            let link = children
                .iter()
                .find(|i| matches!(i, Inline::Link { .. }))
                .expect("expected an Inline::Link");
            match link {
                Inline::Link { url, .. } => match url {
                    Url::Unresolved(s) => s.clone(),
                    Url::Resolved(r) => r.href.clone(),
                },
                _ => unreachable!(),
            }
        }
        other => panic!("expected Paragraph, got {other:?}"),
    }
}

#[test]
fn wikilink_cross_page_fragment_is_slugged() {
    // `[[other#Getting Started]]` → sentinel `moss-resolved:other.md#getting-started`.
    let mut doc = parse("[[other#Getting Started]]");
    let graph = graph_with(&["index.md", "other.md"]);
    let _ = resolve_urls(&mut doc, &graph, "index.md").outgoing;
    assert_eq!(
        first_link_href(&doc),
        "moss-resolved:other.md#getting-started"
    );
}

#[test]
fn wikilink_same_page_fragment_is_slugged() {
    // Same-page `[[#Local Section]]` → bare anchor `#local-section`,
    // no `moss-resolved:` prefix (the path part is empty).
    let mut doc = parse("[[#Local Section]]");
    let graph = graph_with(&["index.md"]);
    let _ = resolve_urls(&mut doc, &graph, "index.md").outgoing;
    assert_eq!(first_link_href(&doc), "#local-section");
}

#[test]
fn markdown_link_fragment_stays_raw_not_slugged() {
    // Regression guard for the design decision: a NON-wikilink markdown
    // link keeps its fragment RAW (case intact, no slugging). This MUST
    // still hold after the wikilink fix. (CommonMark forbids spaces in a
    // bare link destination, so we use a case-bearing fragment to make
    // the raw-vs-slug distinction observable: raw `#GettingStarted`
    // would slug to `#gettingstarted`.)
    let mut doc = parse("[x](other#GettingStarted)");
    let graph = graph_with(&["index.md", "other.md"]);
    let _ = resolve_urls(&mut doc, &graph, "index.md").outgoing;
    assert_eq!(
        first_link_href(&doc),
        "moss-resolved:other.md#GettingStarted"
    );
}

#[test]
fn wikilink_block_ref_keeps_id_raw() {
    // Block refs (`#^id`) strip the caret but keep the id RAW (no slug),
    // mirroring `wikilink_dispatch::build_anchor`. Use space + uppercase
    // so slugging would be observably different.
    let mut doc = parse("[[other#^Block Id]]");
    let graph = graph_with(&["index.md", "other.md"]);
    let _ = resolve_urls(&mut doc, &graph, "index.md").outgoing;
    let href = first_link_href(&doc);
    assert!(
        href.contains("#Block Id"),
        "expected raw block-ref, got: {href}"
    );
    assert!(!href.contains("#block-id"), "block-ref was slugged: {href}");
}

#[test]
fn wikilink_cjk_fragment_preserved() {
    // CJK characters are preserved by obsidian_heading_anchor.
    let mut doc = parse("[[other#中文标题]]");
    let graph = graph_with(&["index.md", "other.md"]);
    let _ = resolve_urls(&mut doc, &graph, "index.md").outgoing;
    assert_eq!(first_link_href(&doc), "moss-resolved:other.md#中文标题");
}

#[test]
fn slug_wikilink_suffix_preserves_query() {
    // A `?query#frag` suffix: only the `#frag` is slugged; the query
    // passes through untouched.
    assert_eq!(slug_wikilink_suffix("?a=1#My Heading"), "?a=1#my-heading");
    // Query-only suffix is untouched.
    assert_eq!(slug_wikilink_suffix("?a=1"), "?a=1");
    // Fragment-only suffix is slugged.
    assert_eq!(slug_wikilink_suffix("#My Heading"), "#my-heading");
    // Block ref keeps id raw (caret stripped).
    assert_eq!(slug_wikilink_suffix("#^Block Id"), "#Block Id");
}

// -----------------------------------------------------------------
// Task 6: engine routing tests for resolve_asset_url
//
// These tests exercise the unified asset engine (resolve_asset_ref)
// through the resolve_asset_url path. They cover:
//   - Separator-bearing paths that the old code passed through verbatim
//     (the 404 bug), now rebased via SeparatorFallback.
//   - Absolute `/`-prefixed paths that must stay absolute (R3).
//   - Case-mismatched paths that the engine canonicalises.
//   - Bare filenames that must behave identically to the old
//     resolve_reference path (the `image_bare_unchanged_from_today` gate).
// -----------------------------------------------------------------

/// Test seam: build a `Url::Unresolved(raw)`, run it through `resolve_asset_url`,
/// and return the resolved `href` string. The `graph` is built with
/// `ContentGraph::from_paths`.
fn resolve_image_src(
    raw: &str,
    source_path: &str,
    graph: &crate::content_graph::ContentGraph,
) -> String {
    let mut url = Url::Unresolved(raw.to_string());
    let mut found = UrlResolution::default();
    resolve_asset_url(&mut url, "", graph, source_path, &mut found);
    match url {
        Url::Resolved(r) => r.href,
        Url::Unresolved(s) => s,
    }
}

#[test]
fn image_separator_fallback_rebases_to_root() {
    // The 404 bug: `./assets/AGU2025.jpg` authored in `News/post.md` is
    // not adjacent (no `News/assets/` dir). Old code passed it verbatim →
    // 404. The engine rebases to the real file (SeparatorFallback → root
    // `assets/AGU2025.jpg`) and that file's pinned URL is emitted. The two
    // downstream `../` compensations this used to need — one from the source
    // directory, one more for pretty-URL nesting, added by a later string pass
    // — have nothing left to compensate for.
    let graph = graph_with(&["assets/AGU2025.jpg", "News/post.md"]);
    assert_eq!(
        resolve_image_src("./assets/AGU2025.jpg", "News/post.md", &graph),
        "/assets/AGU2025.jpg"
    );
}

#[test]
fn image_absolute_stays_absolute() {
    // R3: an absolute `/`-prefixed asset reference keeps its leading `/`.
    // Now the general case rather than a special one — every resolved asset
    // reference is emitted root-absolute.
    let graph = graph_with(&["assets/x.jpg"]);
    assert_eq!(
        resolve_image_src("/assets/x.jpg", "News/post.md", &graph),
        "/assets/x.jpg"
    );
}

#[test]
fn image_case_mismatch_emits_canonical() {
    // `./assets/Hoon.jpg` authored in `Team.md`; disk is `Hoon.JPG`. The
    // engine finds the real file (CaseMismatch → `assets/Hoon.JPG`) and the
    // emitted URL is that file's pinned URL: the LEAF keeps the disk's case
    // (the bytes are served under it) while directory segments take the slug
    // the output tree uses.
    let graph = graph_with(&["assets/Hoon.JPG"]);
    assert_eq!(
        resolve_image_src("./assets/Hoon.jpg", "Team.md", &graph),
        "/assets/Hoon.JPG"
    );
}

#[test]
fn image_href_is_identical_from_root_and_from_a_nested_note() {
    // moss#903 bug 3, at the unit level: one asset under a MIXED-CASE folder,
    // referenced from the vault root and from a note two folders deep. Both
    // emit the same pinned URL, and its directory segment is the slug the
    // asset copier writes (`MIRROR/` → `/mirror/`) — not the source spelling,
    // and not a case-folded guess.
    let graph = graph_with(&[
        "MIRROR/在場/cover-IMG.png",
        "index.md",
        "MIRROR/在場/note.md",
    ]);
    let from_root = resolve_image_src("cover-IMG.png", "index.md", &graph);
    let from_deep = resolve_image_src("cover-IMG.png", "MIRROR/在場/note.md", &graph);
    assert_eq!(from_root, "/mirror/%E5%9C%A8%E5%A0%B4/cover-IMG.png");
    assert_eq!(
        from_root, from_deep,
        "the referencing note's depth must not change the emitted URL"
    );
}

#[test]
fn unresolvable_image_ref_reports_instead_of_guessing() {
    // A reference the graph cannot resolve keeps the author's bytes and
    // produces a Diagnostic. The failure mode this replaces is a synthesized
    // path that looks like a working link and 404s at deploy.
    let graph = graph_with(&["assets/photo.jpg", "post.md"]);
    let mut doc = parse("![](nope.jpg)");
    let found = resolve_urls(&mut doc, &graph, "post.md");
    assert!(found.outgoing.is_empty(), "{:?}", found.outgoing);
    assert_eq!(found.diagnostics.len(), 1, "{:?}", found.diagnostics);
    assert_eq!(found.diagnostics[0].reference, "nope.jpg");
    assert_eq!(found.diagnostics[0].source_path, "post.md");
}

#[test]
fn every_kind_of_missing_media_is_reported_as_blocking() {
    // "Media" is not just images. A video, an audio file, a PDF and a bare
    // wikilink embed all reach the reader as a hole in the page, so all four
    // produce the one diagnostic kind the publish gate refuses on
    // (`missing_media::refuse_publish`). Marking only images would ship a site
    // with a dead <video> and call it clean.
    let graph = graph_with(&["assets/photo.jpg", "post.md"]);
    for markdown in [
        "![](clip.mp4)",
        "![](take.mov)",
        "![](song.m4a)",
        "![](paper.pdf)",
        "![[clip.mp4]]",
    ] {
        let mut doc = parse(markdown);
        let found = resolve_urls(&mut doc, &graph, "post.md");
        let blocking: Vec<_> = found
            .diagnostics
            .iter()
            .filter(|d| d.kind == DiagnosticKind::MissingAsset)
            .collect();
        assert_eq!(
            blocking.len(),
            1,
            "{markdown} must report exactly one missing reference, got {:?}",
            found.diagnostics
        );
    }
}

#[test]
fn a_resolvable_reference_reports_nothing_to_block_on() {
    // The other half of the gate: a site whose media all exist must publish.
    // A false positive here has no override — it would trap the author.
    let graph = graph_with(&["assets/photo.jpg", "assets/clip.mp4", "post.md"]);
    for markdown in ["![](photo.jpg)", "![[clip.mp4]]"] {
        let mut doc = parse(markdown);
        let found = resolve_urls(&mut doc, &graph, "post.md");
        assert!(
            !found
                .diagnostics
                .iter()
                .any(|d| d.kind == DiagnosticKind::MissingAsset),
            "{markdown} resolves, so nothing may block the publish: {:?}",
            found.diagnostics
        );
    }
}

#[test]
fn image_bare_unchanged_from_today() {
    // Gate: bare-filename resolution must find the SAME file via the engine as
    // the old resolve_reference path did. `photo.jpg` from `post.md` →
    // BareFuzzy → `assets/photo.jpg` → that file's pinned URL.
    let graph = graph_with(&["assets/photo.jpg", "post.md"]);
    assert_eq!(
        resolve_image_src("photo.jpg", "post.md", &graph),
        "/assets/photo.jpg"
    );
}
