use super::*;

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
    // These strings are part of moss's HTML/CSS contract.
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
    // moss-build (resolve). Changing them breaks the resolver.
    assert_eq!(MARKER_MARKDOWN, "moss-embed");
    assert_eq!(MARKER_IPYNB, "moss-embed-ipynb");
    assert_eq!(MARKER_TABLE, "moss-embed-table");
}

// --- Sizing malformed-input coverage ---

#[test]
fn test_sizing_parse_malformed_box_is_none() {
    assert_eq!(Sizing::parse("100xbad"), None);
    assert_eq!(Sizing::parse("100x"), None);
    assert_eq!(Sizing::parse("-100"), None);
}

#[test]
fn figure_extensions_are_in_asset_registry() {
    use crate::resolve::asset_registry::{all_assets, asset_info};
    use crate::resolve::ext_kind::ExtKind;
    for ext in IMAGE_EXTENSIONS {
        // the figure-arm image list, `IMAGE_EXTENSIONS` in embed_renderer.rs
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
}

#[test]
fn avif_in_figure_images() {
    assert!(IMAGE_EXTENSIONS.contains(&"avif"));
}
