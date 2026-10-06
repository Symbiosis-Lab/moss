//! Explicit map projections of a document's location metadata.

use super::{html_escape, parse_marker_body, marker_decode, ParsedMarker, MARKER_END, MARKER_FOLDER_LIST};
use crate::build::place_map::PlaceMapRenderContext;
use crate::build::types::ParsedDocument;

pub(super) fn place_map_with_placement(
    svg: String,
    placement: &moss_core::media::Placement,
    caption: Option<&str>,
) -> String {
    let attrs = moss_core::render::placement::placement_attrs(placement);
    let class = attrs.class_value("moss-place-map-frame");
    let caption = caption.map(|text| format!(
        "<div class=\"moss-place-map-caption\">{}</div>",
        moss_core::media::html_escape(text)
    )).unwrap_or_default();
    format!(
        "<div class=\"{class}\"{}{size}>{svg}{caption}</div>",
        attrs.data_width_attr,
        size = attrs.size_style_attr,
    )
}

pub(super) fn has_own_embed(html: &str, source: &str) -> bool {
    html.split(MARKER_FOLDER_LIST).skip(1).any(|rest| {
        rest.split_once(MARKER_END)
            .and_then(|(body, _)| parse_marker_body(body))
            .is_some_and(|p| p.style.as_deref() == Some("map") && marker_decode(p.path) == source)
    })
}

pub(super) fn render(
    parsed: &ParsedMarker<'_>,
    ordinal: usize,
    docs: &[ParsedDocument],
    maps: Option<&PlaceMapRenderContext>,
) -> String {
    let path = marker_decode(parsed.path);
    let from = marker_decode(parsed.from);
    let target = docs.iter().find(|d| d.source_path.as_deref() == Some(path.as_str()));
    if let Some((doc, maps)) = target.zip(maps) {
        if let Some(svg) = maps.render_article_map(&doc.location, doc.route, &doc.url_path, &from, ordinal) {
            return place_map_with_placement(svg, &parsed.placement, parsed.caption.as_deref());
        }
    }
    crate::build::cli_output::log_warn_problem!("map embed '{}' has no resolved document with coordinate-bearing locations", path);
    format!(r#"<div class="moss-embed-missing">Map unavailable: {}</div>"#, html_escape(&path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::content_graph::ContentGraphBuilder;

    fn fixture(markdown: &str, from: &str) -> (Vec<ParsedDocument>, PlaceMapRenderContext) {
        let mut graph = ContentGraphBuilder::new();
        graph.add_file("story.md", "Story");
        graph.add_file("other.md", "Other");
        let resolved = moss_core::resolve::resolve_content(from, markdown, &graph.build(), &|_| {
            panic!("an explicit map must not transclude its target body")
        });
        let docs = vec![
            ParsedDocument {
                source_path: Some("story.md".into()), url_path: "story/index.html".into(),
                location: vec!["Kyoto".into(), "Osaka".into()],
                html_content: if from == "story.md" { resolved.content_markdown.clone() } else { String::new() },
                ..Default::default()
            },
            ParsedDocument {
                source_path: Some("other.md".into()), url_path: "other/index.html".into(),
                html_content: if from == "other.md" { resolved.content_markdown } else { String::new() },
                ..Default::default()
            },
        ];
        let table = toml::from_str("[Kyoto]\nlat=35.01\nlng=135.77\nprecision='city'\n[Osaka]\nlat=34.69\nlng=135.50\nprecision='city'\n").unwrap();
        let maps = PlaceMapRenderContext::new(
            crate::build::place_map::PlaceMapContext::embedded().unwrap(),
            crate::vault::places::parse_gazetteer(&table), "places".into(),
            crate::build::place_map::LocatorPlacement::AlignRight, Default::default(),
        );
        (docs, maps)
    }

    fn expand(docs: &mut [ParsedDocument], maps: &PlaceMapRenderContext) {
        super::super::expand_markers_in_documents_with_place_maps(
            docs, &super::super::tests::test_project(), &Default::default(), false,
            &crate::build::media::dimensions::MediaDimensionLookup::new(&[], &[], &Default::default(), None), None, Some(maps),
        );
    }

    #[test]
    fn authored_self_map_stays_between_prose_and_replaces_the_automatic_locator() {
        let (mut docs, maps) = fixture("Before\n\n![[#|style:map|align-right 50%|The places]]\n\nAfter", "story.md");
        expand(&mut docs, &maps);
        let html = &docs[0].html_content;
        assert!(html.find("Before").unwrap() < html.find("moss-place-map-frame").unwrap());
        assert!(html.find("moss-place-map-frame").unwrap() < html.find("After").unwrap());
        assert!(html.contains("moss-align-right\" style=\"width:50%\""), "{html}");
        assert!(html.contains("data-map-marker=\"places/kyoto\""));
        assert!(html.contains("data-map-marker=\"places/osaka\""));
        assert!(html.contains("/places/?article=/story/&embed=1"), "{html}");
        assert!(html.contains("The places</div>"));
        let layout = crate::build::page::layout::LayoutConfig::new("test-site", None).with_place_maps(Some(maps));
        assert!(crate::build::render::credits::render_place_locator(&docs[0], &layout).is_none());
    }

    #[test]
    fn a_named_article_map_uses_the_targets_locations_and_keeps_svg_ids_distinct() {
        let (mut docs, maps) = fixture("![[story|style:map|wide]]\n\n![[story|style:map|align-left 30%]]", "other.md");
        expand(&mut docs, &maps);
        let html = &docs[1].html_content;
        assert_eq!(html.matches("data-moss-place-embed ").count(), 2);
        assert_eq!(html.matches("article=/story/").count(), 2);
        assert!(!docs[1].has_own_map_embed);
        let mut ids = std::collections::HashSet::new();
        for part in html.split(" id=\"").skip(1) {
            assert!(ids.insert(part.split('"').next().unwrap()), "duplicate SVG id");
        }
    }

    #[test]
    fn article_map_ids_are_unique_across_body_segments_and_grid_cells() {
        let markdown = "![[story|style:map]]\n\n:::grid 2\n![[story|style:map]]\n\n---\n\n![[story|style:map]]\n:::\n\n![[story|style:map]]";
        let (mut docs, maps) = fixture(markdown, "other.md");
        let ast = moss_core::ast::parse(&docs[1].html_content);
        let plan = crate::build::markdown::body_plan::render_segmented(&ast, &moss_core::ast::DefaultHooks::default());
        assert!(plan.segments.len() > 1);
        docs[1].body_plan = Some(plan);
        expand(&mut docs, &maps);
        let html = &docs[1].html_content;
        assert_eq!(html.matches("data-moss-place-embed ").count(), 4);
        let mut ids = std::collections::HashSet::new();
        for part in html.split(" id=\"").skip(1) {
            assert!(ids.insert(part.split('"').next().unwrap()), "duplicate SVG id across segments");
        }
    }

    #[test]
    fn article_and_place_maps_share_a_unique_svg_sequence() {
        let (mut docs, maps) = fixture("![[story|style:map]]\n\n![[/places/kyoto/|style:map]]\n\n![[/places/kyoto/|style:map]]", "other.md");
        let kind = crate::build::terms::TermKind {
            key: "places".into(), fields: vec!["location".into()], title: "Places".into(),
            is_place: true, parents: Default::default(), explorer: None, line: None,
        };
        crate::build::terms::derive_terms(&mut docs, vec![kind]);
        expand(&mut docs, &maps);
        let html = &docs[1].html_content;
        assert_eq!(html.matches("data-moss-place-embed ").count(), 3, "{html}");
        let mut ids = std::collections::HashSet::new();
        for part in html.split(" id=\"").skip(1) {
            assert!(ids.insert(part.split('"').next().unwrap()), "duplicate SVG id across map types");
        }
    }

    #[test]
    fn a_map_without_locations_reports_a_missing_embed_without_changing_ordinary_transclusion() {
        let (mut docs, maps) = fixture("![[#|style:map]]", "other.md");
        expand(&mut docs, &maps);
        assert!(docs[1].html_content.contains("moss-embed-missing"));
        assert!(!docs[1].html_content.contains("data-moss-place-embed"));
    }
}
