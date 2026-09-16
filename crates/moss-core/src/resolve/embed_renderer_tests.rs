use super::*;

#[derive(Debug)]
struct DummyRenderer;
impl EmbedRenderer for DummyRenderer {
    fn extensions(&self) -> &[&'static str] {
        &["xyz"]
    }
    fn render(&self, embed: &ParsedEmbed<'_>) -> RenderedEmbed {
        RenderedEmbed::Inline(format!("<dummy src={}>", embed.resolved_path))
    }
}

#[test]
fn test_dummy_renderer_trait_surface() {
    let r = DummyRenderer;
    assert_eq!(r.extensions(), &["xyz"]);
    let embed = ParsedEmbed {
        resolved_path: "a.xyz",
        from_path: "post.md",
        pinned_url: "a.xyz",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    };
    assert_eq!(
        r.render(&embed),
        RenderedEmbed::Inline("<dummy src=a.xyz>".to_string())
    );
}

// --- MarkdownEmbedRenderer ---

#[test]
fn test_markdown_embed_renderer_no_section() {
    let r = MarkdownEmbedRenderer;
    let embed = ParsedEmbed {
        resolved_path: "posts/intro.md",
        from_path: "index.md",
        pinned_url: "posts/intro.md",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    };
    assert_eq!(
        r.render(&embed),
        RenderedEmbed::Deferred {
            marker: "<!-- moss-embed:posts/intro.md -->".to_string()
        }
    );
}

#[test]
fn test_markdown_embed_renderer_heading_section() {
    let r = MarkdownEmbedRenderer;
    let embed = ParsedEmbed {
        resolved_path: "guide.md",
        from_path: "index.md",
        pinned_url: "guide.md",
        query: None,
        section: Some("Getting Started"),
        alias: None,
        width: None,
        attrs: None,
    };
    assert_eq!(
        r.render(&embed),
        RenderedEmbed::Deferred {
            marker: "<!-- moss-embed:guide.md#getting-started -->".to_string()
        }
    );
}

#[test]
fn test_markdown_embed_renderer_block_ref_section() {
    let r = MarkdownEmbedRenderer;
    let embed = ParsedEmbed {
        resolved_path: "guide.md",
        from_path: "index.md",
        pinned_url: "guide.md",
        query: None,
        section: Some("^block-xyz"),
        alias: None,
        width: None,
        attrs: None,
    };
    assert_eq!(
        r.render(&embed),
        RenderedEmbed::Deferred {
            marker: "<!-- moss-embed:guide.md#^block-xyz -->".to_string()
        }
    );
}

#[test]
fn test_markdown_embed_renderer_extensions() {
    assert_eq!(MarkdownEmbedRenderer.extensions(), &["md"]);
}

// -- markdown escape helpers (covered above; spec from plan §D1) -----

#[test]
fn markdown_escape_alt_brackets() {
    assert_eq!(markdown_escape_alt("plain"), "plain");
    assert_eq!(markdown_escape_alt("has [brackets]"), r"has \[brackets\]");
    assert_eq!(
        markdown_escape_alt(r"with \ backslash"),
        r"with \\ backslash"
    );
}

// --- Dim parser ---

#[test]
fn test_dim_css_px() {
    assert_eq!(Dim::Px(200).to_css(), "200px");
}

#[test]
fn test_dim_css_percent() {
    assert_eq!(Dim::Percent(100.0).to_css(), "100%");
    assert_eq!(Dim::Percent(50.5).to_css(), "50.5%");
}

#[test]
fn test_dim_css_vh() {
    assert_eq!(Dim::Vh(100.0).to_css(), "100vh");
}

// --- Sizing parser ---

#[test]
fn test_sizing_parse_width_only_px() {
    assert_eq!(Sizing::parse("200"), Some(Sizing::Width(Dim::Px(200))));
}

#[test]
fn test_sizing_parse_width_only_percent() {
    assert_eq!(
        Sizing::parse("100%"),
        Some(Sizing::Width(Dim::Percent(100.0)))
    );
}

#[test]
fn test_sizing_parse_box_px() {
    assert_eq!(
        Sizing::parse("200x150"),
        Some(Sizing::Box(Dim::Px(200), Dim::Px(150)))
    );
}

#[test]
fn test_sizing_parse_box_percent_by_px() {
    assert_eq!(
        Sizing::parse("100%x600"),
        Some(Sizing::Box(Dim::Percent(100.0), Dim::Px(600)))
    );
}

#[test]
fn test_sizing_parse_box_vh_height() {
    assert_eq!(
        Sizing::parse("100%x100vh"),
        Some(Sizing::Box(Dim::Percent(100.0), Dim::Vh(100.0)))
    );
}

#[test]
fn test_sizing_parse_rejects_display_keywords() {
    assert_eq!(Sizing::parse("contain"), None);
    assert_eq!(Sizing::parse("left top"), None);
}

#[test]
fn test_sizing_parse_empty_returns_none() {
    assert_eq!(Sizing::parse(""), None);
    assert_eq!(Sizing::parse("   "), None);
}

// --- Reserved classnames ---

#[test]
fn test_embed_class_constants_stable() {
    // These strings are part of moss's HTML/CSS contract (#508).
    // Changing them is a breaking change for theme authors; this test
    // exists to force an explicit decision if anyone tries.
    assert_eq!(CLASS_EMBED, "moss-embed");
    assert_eq!(CLASS_EMBED_IFRAME, "moss-embed-iframe");
    assert_eq!(CLASS_EMBED_PDF, "moss-embed-pdf");
    assert_eq!(CLASS_EMBED_AUDIO, "moss-embed-audio");
    assert_eq!(CLASS_EMBED_VIDEO, "moss-embed-video");
    assert_eq!(CLASS_EMBED_NOTEBOOK, "moss-embed-notebook");
    assert_eq!(CLASS_EMBED_3D, "moss-embed-3d");
    assert_eq!(CLASS_EMBED_TABLE, "moss-embed-table");
}

#[test]
fn test_embed_marker_prefixes_stable() {
    // Marker prefixes are a contract between moss-core (emit) and
    // src-tauri (resolve). Changing them breaks the resolver.
    assert_eq!(MARKER_MARKDOWN, "moss-embed");
    assert_eq!(MARKER_IPYNB, "moss-embed-ipynb");
    assert_eq!(MARKER_TABLE, "moss-embed-table");
}

// --- RenderedEmbed variants ---

#[test]
fn test_rendered_embed_html_variant() {
    let h = RenderedEmbed::Html("<iframe src=\"x\"></iframe>".to_string());
    match h {
        RenderedEmbed::Html(s) => assert!(s.contains("iframe")),
        _ => panic!("expected Html variant"),
    }
}

#[test]
fn test_rendered_embed_deferred_variant() {
    let d = RenderedEmbed::Deferred {
        marker: "<!-- moss-embed-ipynb:nb.ipynb -->".to_string(),
    };
    match d {
        RenderedEmbed::Deferred { marker } => assert!(marker.contains("ipynb")),
        _ => panic!("expected Deferred variant"),
    }
}

// --- Registry lookup ---

#[test]
fn test_lookup_renderer_by_extension() {
    // Image extensions no longer resolve through the registry — the
    // image-embed synth-collapse routes them to the dispatcher's
    // Block::Figure arm (via IMAGE_EXTENSIONS), not an EmbedRenderer.
    assert!(lookup_renderer("jpg").is_none());
    assert!(lookup_renderer("JPG").is_none()); // case-insensitive
    assert!(lookup_renderer("md").is_some());
    assert!(lookup_renderer("MD").is_some()); // case-insensitive
    assert!(lookup_renderer("xyz").is_none());
    assert!(lookup_renderer("").is_none());
}

// --- IframeRenderer ---

#[test]
fn test_iframe_renderer_extensions() {
    let r = IframeRenderer;
    let exts: Vec<&&str> = r.extensions().iter().collect();
    assert!(exts.iter().any(|&&x| x == "html"));
    assert!(exts.iter().any(|&&x| x == "htm"));
}

/// Helper: render iframe embed to Stage 1 markdown (Inline string).
fn iframe_md(e: &ParsedEmbed) -> String {
    match IframeRenderer.render(e) {
        RenderedEmbed::Inline(s) => s,
        _ => panic!("expected Inline (Stage 1 markdown)"),
    }
}

// Phase 3 PR4 (2026-05-27): the `moss:kind=…` title channel retired —
// `render_link_markdown` now emits bare `[name](url)`. The accumulated
// typed params (query / fragment / sizing / data-width / title-alias)
// are discarded at the markdown boundary; future PRs will thread them
// through wikilink dispatch's `EmitKind` instead of round-tripping.
// Until then these renderers smoke-test only the alt / url shape.

#[test]
fn stage1_iframe_basic_is_bare_link() {
    let out = iframe_md(&ParsedEmbed {
        resolved_path: "widget.html",
        from_path: "post.md",
        pinned_url: "widget.html",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[widget](widget.html)");
}

#[test]
fn stage1_iframe_with_query_emits_bare_link() {
    // `?query` is no longer round-tripped through the markdown title
    // attribute.
    let out = iframe_md(&ParsedEmbed {
        resolved_path: "scale.html",
        from_path: "post.md",
        pinned_url: "scale.html",
        query: Some("a=major,minor&r=D"),
        section: None,
        alias: None,
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[scale](scale.html)");
}

#[test]
fn stage1_iframe_with_sizing_alias_emits_bare_link() {
    let out = iframe_md(&ParsedEmbed {
        resolved_path: "widget.html",
        from_path: "post.md",
        pinned_url: "widget.html",
        query: None,
        section: None,
        alias: Some("100%x600"),
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[widget](widget.html)");
}

#[test]
fn stage1_iframe_text_alias_emits_bare_link() {
    let out = iframe_md(&ParsedEmbed {
        resolved_path: "widget.html",
        from_path: "post.md",
        pinned_url: "widget.html",
        query: None,
        section: None,
        alias: Some("My cool widget"),
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[widget](widget.html)");
}

#[test]
fn stage1_iframe_with_fragment_emits_bare_link() {
    let out = iframe_md(&ParsedEmbed {
        resolved_path: "doc.html",
        from_path: "post.md",
        pinned_url: "doc.html",
        query: Some("x=1"),
        section: Some("section2"),
        alias: None,
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[doc](doc.html)");
}

#[test]
fn stage1_iframe_with_canonical_width_emits_bare_link() {
    let out = iframe_md(&ParsedEmbed {
        resolved_path: "widget.html",
        from_path: "post.md",
        pinned_url: "widget.html",
        query: None,
        section: None,
        alias: None,
        width: Some("wide"),
        attrs: None,
    });
    assert_eq!(out, "[widget](widget.html)");
}

// --- Sizing malformed-input coverage ---

#[test]
fn test_sizing_parse_malformed_box_is_none() {
    assert_eq!(Sizing::parse("100xbad"), None);
    assert_eq!(Sizing::parse("100x"), None);
    assert_eq!(Sizing::parse("-100"), None);
}

#[test]
fn stage1_iframe_malformed_sizing_emits_bare_link() {
    // PR4: title-attribute fallback retired. Malformed sizing aliases
    // simply drop alongside other typed params.
    let out = iframe_md(&ParsedEmbed {
        resolved_path: "widget.html",
        from_path: "post.md",
        pinned_url: "widget.html",
        query: None,
        section: None,
        alias: Some("100xbad"),
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[widget](widget.html)");
}

// --- PdfRenderer ---

fn pdf_md(e: &ParsedEmbed) -> String {
    match PdfRenderer.render(e) {
        RenderedEmbed::Inline(s) => s,
        _ => panic!("expected Inline (Stage 1 markdown)"),
    }
}

#[test]
fn test_pdf_renderer_extensions() {
    assert_eq!(PdfRenderer.extensions(), &["pdf"]);
}

#[test]
fn stage1_pdf_basic_is_bare_link() {
    let out = pdf_md(&ParsedEmbed {
        resolved_path: "report.pdf",
        from_path: "post.md",
        pinned_url: "report.pdf",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[report](report.pdf)");
}

#[test]
fn stage1_pdf_with_page_fragment_emits_bare_link() {
    let out = pdf_md(&ParsedEmbed {
        resolved_path: "doc.pdf",
        from_path: "post.md",
        pinned_url: "doc.pdf",
        query: None,
        section: Some("page=5"),
        alias: None,
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[doc](doc.pdf)");
}

#[test]
fn stage1_pdf_with_sizing_emits_bare_link() {
    let out = pdf_md(&ParsedEmbed {
        resolved_path: "doc.pdf",
        from_path: "post.md",
        pinned_url: "doc.pdf",
        query: None,
        section: None,
        alias: Some("100%x800"),
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[doc](doc.pdf)");
}

// --- AudioRenderer ---

fn audio_md(e: &ParsedEmbed) -> String {
    match AudioRenderer.render(e) {
        RenderedEmbed::Inline(s) => s,
        _ => panic!("expected Inline (Stage 1 markdown)"),
    }
}

#[test]
fn test_audio_renderer_extensions() {
    let r = AudioRenderer;
    let exts: Vec<&&str> = r.extensions().iter().collect();
    for e in &["mp3", "wav", "ogg", "flac", "m4a", "opus", "aac"] {
        assert!(exts.iter().any(|&&x| x == *e), "missing: {}", e);
    }
}

#[test]
fn stage1_audio_basic_is_bare_link() {
    let out = audio_md(&ParsedEmbed {
        resolved_path: "song.mp3",
        from_path: "post.md",
        pinned_url: "song.mp3",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[song](song.mp3)");
}

#[test]
fn stage1_audio_each_extension_emits_bare_link() {
    // Phase 3 PR4: the `ext=` param is dropped at the markdown
    // boundary. Per-extension MIME routing now happens inside the
    // Stage 2 synthesizer via the wikilink dispatcher's typed path.
    for ext in ["mp3", "wav", "ogg", "flac", "m4a", "opus", "aac"] {
        let path = format!("a.{}", ext);
        let out = audio_md(&ParsedEmbed {
            resolved_path: &path,
            from_path: "post.md",
            pinned_url: &path,
            query: None,
            section: None,
            alias: None,
            width: None,
            attrs: None,
        });
        assert_eq!(out, format!("[a](a.{})", ext), "ext={}", ext);
    }
}

// --- VideoRenderer ---

fn video_md(e: &ParsedEmbed) -> String {
    match VideoRenderer.render(e) {
        RenderedEmbed::Inline(s) => s,
        _ => panic!("expected Inline (Stage 1 markdown)"),
    }
}

#[test]
fn test_video_renderer_extensions() {
    let r = VideoRenderer;
    let exts: Vec<&&str> = r.extensions().iter().collect();
    for e in &["mp4", "webm", "mov", "m4v"] {
        assert!(exts.iter().any(|&&x| x == *e), "missing: {}", e);
    }
}

#[test]
fn stage1_video_basic_is_bare_link() {
    let out = video_md(&ParsedEmbed {
        resolved_path: "trailer.mp4",
        from_path: "post.md",
        pinned_url: "trailer.mp4",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[trailer](trailer.mp4)");
}

#[test]
fn stage1_video_emits_original_extension_in_url() {
    // The URL slot carries the original extension (e.g. `.mov`) so the
    // downstream `add_video_placeholder_attributes` rewriter can perform
    // `.mov→.mp4` swap on the served URL. The renderer never modifies it.
    for ext in ["mp4", "webm", "mov", "m4v"] {
        let path = format!("clip.{}", ext);
        let out = video_md(&ParsedEmbed {
            resolved_path: &path,
            from_path: "post.md",
            pinned_url: &path,
            query: None,
            section: None,
            alias: None,
            width: None,
            attrs: None,
        });
        assert_eq!(out, format!("[clip](clip.{})", ext), "ext={}", ext);
    }
}

#[test]
fn stage1_video_with_sizing_emits_bare_link() {
    let out = video_md(&ParsedEmbed {
        resolved_path: "clip.mp4",
        from_path: "post.md",
        pinned_url: "clip.mp4",
        query: None,
        section: None,
        alias: Some("640x360"),
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[clip](clip.mp4)");
}

// --- NotebookRenderer ---

#[test]
fn test_notebook_renderer_extensions() {
    assert_eq!(NotebookRenderer.extensions(), &["ipynb"]);
}

#[test]
fn test_notebook_renderer_basic() {
    let embed = ParsedEmbed {
        resolved_path: "resources/habitable-zone.ipynb",
        from_path: "posts/hello.md",
        pinned_url: "resources/habitable-zone.ipynb",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    };
    match NotebookRenderer.render(&embed) {
        RenderedEmbed::Deferred { marker } => assert_eq!(
            marker,
            "<!-- moss-embed-ipynb:resources/habitable-zone.ipynb -->"
        ),
        _ => panic!("expected Deferred"),
    }
}

#[test]
fn test_notebook_renderer_with_query() {
    let embed = ParsedEmbed {
        resolved_path: "nb.ipynb",
        from_path: "post.md",
        pinned_url: "nb.ipynb",
        query: Some("cells=1-5"),
        section: None,
        alias: None,
        width: None,
        attrs: None,
    };
    match NotebookRenderer.render(&embed) {
        RenderedEmbed::Deferred { marker } => {
            assert!(marker.contains("nb.ipynb?cells=1-5"), "got: {}", marker)
        }
        _ => panic!("expected Deferred"),
    }
}

#[test]
fn test_notebook_renderer_no_head_assets() {
    // nbconvert embeds its own styles inline; no page-level assets needed.
    assert!(NotebookRenderer.head_assets().is_empty());
}

// --- ModelViewerRenderer ---

fn mv_md(e: &ParsedEmbed) -> String {
    match ModelViewerRenderer.render(e) {
        RenderedEmbed::Inline(s) => s,
        _ => panic!("expected Inline (Stage 1 markdown)"),
    }
}

#[test]
fn test_model_viewer_extensions() {
    let exts = ModelViewerRenderer.extensions();
    assert!(exts.iter().any(|&x| x == "glb"));
    assert!(exts.iter().any(|&x| x == "gltf"));
}

#[test]
fn stage1_model_viewer_basic_is_bare_link() {
    let out = mv_md(&ParsedEmbed {
        resolved_path: "teapot.glb",
        from_path: "post.md",
        pinned_url: "teapot.glb",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[teapot](teapot.glb)");
}

#[test]
fn stage1_model_viewer_with_sizing_emits_bare_link() {
    let out = mv_md(&ParsedEmbed {
        resolved_path: "m.glb",
        from_path: "post.md",
        pinned_url: "m.glb",
        query: None,
        section: None,
        alias: Some("400x400"),
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[m](m.glb)");
}

#[test]
fn test_model_viewer_head_assets() {
    let assets = ModelViewerRenderer.head_assets();
    assert_eq!(assets.len(), 1);
    assert!(assets[0].contains("model-viewer"), "got: {}", assets[0]);
    assert!(assets[0].contains("<script"), "got: {}", assets[0]);
}

// --- TableRenderer ---

#[test]
fn test_table_renderer_extensions() {
    let exts = TableRenderer.extensions();
    assert!(exts.iter().any(|&x| x == "csv"));
    assert!(exts.iter().any(|&x| x == "tsv"));
}

#[test]
fn test_table_renderer_emits_deferred() {
    let embed = ParsedEmbed {
        resolved_path: "data/stars.csv",
        from_path: "post.md",
        pinned_url: "data/stars.csv",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    };
    match TableRenderer.render(&embed) {
        RenderedEmbed::Deferred { marker } => {
            assert_eq!(marker, "<!-- moss-embed-table:data/stars.csv -->")
        }
        _ => panic!("expected Deferred"),
    }
}

// -- spec § P9 width: PR4 retires the title-attribute round-trip ----

/// Build a width-only `ParsedEmbed` mirroring the wikilink resolver's
/// pre-pass output for `![[file|full]]`-style aliases.
fn embed_with_width<'a>(resolved_path: &'a str, width: &'static str) -> ParsedEmbed<'a> {
    ParsedEmbed {
        resolved_path,
        from_path: "post.md",
        pinned_url: resolved_path,
        query: None,
        section: None,
        alias: None,
        width: Some(width),
        attrs: None,
    }
}

// Phase 3 PR4: width now drops at the markdown boundary. Each renderer
// still constructs the typed param (so `extra` / `fold_attrs_into_params`
// plumbing stays exercised), but the resulting markdown is bare.

#[test]
fn stage1_iframe_width_emits_bare_link() {
    let out = iframe_md(&embed_with_width("widget.html", "screen"));
    assert_eq!(out, "[widget](widget.html)");
}

#[test]
fn stage1_iframe_no_width_emits_bare_link() {
    let out = iframe_md(&ParsedEmbed {
        resolved_path: "widget.html",
        from_path: "post.md",
        pinned_url: "widget.html",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[widget](widget.html)");
}

#[test]
fn stage1_pdf_width_emits_bare_link() {
    let out = pdf_md(&embed_with_width("doc.pdf", "wide"));
    assert_eq!(out, "[doc](doc.pdf)");
}

#[test]
fn stage1_audio_width_emits_bare_link() {
    let out = audio_md(&embed_with_width("song.mp3", "body"));
    assert_eq!(out, "[song](song.mp3)");
}

#[test]
fn stage1_video_width_emits_bare_link() {
    let out = video_md(&embed_with_width("clip.mp4", "screen"));
    // URL slot still carries the original extension — the rewriter
    // contract is preserved at the markdown level.
    assert_eq!(out, "[clip](clip.mp4)");
}

#[test]
fn stage1_video_no_width_emits_bare_link() {
    let out = video_md(&ParsedEmbed {
        resolved_path: "clip.mp4",
        from_path: "post.md",
        pinned_url: "clip.mp4",
        query: None,
        section: None,
        alias: None,
        width: None,
        attrs: None,
    });
    assert_eq!(out, "[clip](clip.mp4)");
}

#[test]
fn stage1_model_viewer_width_emits_bare_link() {
    let out = mv_md(&embed_with_width("model.glb", "page"));
    assert_eq!(out, "[model](model.glb)");
}

#[test]
fn renderer_and_figure_extensions_are_in_registry() {
    use crate::resolve::asset_registry::{all_assets, asset_info};
    use crate::resolve::ext_kind::ExtKind;
    for r in registry() {
        for ext in r.extensions() {
            assert!(
                asset_info(ext).is_some(),
                "renderer ext {ext} not in registry"
            );
        }
    }
    for ext in IMAGE_EXTENSIONS {
        // the figure-arm image list at embed_renderer.rs:314
        assert!(
            asset_info(ext).is_some(),
            "figure image ext {ext} not in registry"
        );
    }
    // Reverse: every registry Image ext with can_embed:true must be in IMAGE_EXTENSIONS,
    // so ![[photo.avif]] routes to Block::Figure (wikilink_dispatch.rs). Guards against
    // registry additions that silently skip the figure arm.
    for a in all_assets() {
        if a.kind == ExtKind::Image && a.can_embed {
            assert!(
                IMAGE_EXTENSIONS.contains(&a.ext),
                "registry image ext {} with can_embed:true is missing from IMAGE_EXTENSIONS",
                a.ext
            );
        }
    }
    // Reverse: every registry Video ext with can_embed:true must be in VIDEO_EXTENSIONS,
    // so ![[clip.mp4]] routes to VideoRenderer (wikilink_dispatch.rs). Guards against
    // registry additions that silently promise an embeddable video the renderer doesn't list.
    for a in all_assets() {
        if a.kind == ExtKind::Video && a.can_embed {
            assert!(
                VIDEO_EXTENSIONS.contains(&a.ext),
                "registry video ext {} (can_embed) missing from VIDEO_EXTENSIONS",
                a.ext
            );
        }
    }
}

#[test]
fn avif_in_figure_images_and_aac_in_audio() {
    assert!(IMAGE_EXTENSIONS.contains(&"avif"));
    assert!(AUDIO_EXTENSIONS.contains(&"aac"));
}
