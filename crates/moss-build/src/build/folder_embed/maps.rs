//! Explicit map projections of a document's location metadata.

use super::{html_escape, parse_marker_body, marker_decode, ParsedMarker, MARKER_END, MARKER_FOLDER_LIST};
use crate::build::place_map::PlaceMapRenderContext;
use crate::build::types::ParsedDocument;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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

/// How a page places a country map's list apart from the map with `style:levels`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ListPlacement {
    /// Beside the page's own embed of that map, which then leaves its list out.
    Apart,
    /// With no embed of the map on the page: the list stands on its own.
    Alone,
    /// Placed twice, or on the map's own page: not drawn, and the map keeps its list.
    Refused,
}

/// Reads a page's maps once, from its body and its hero: whether it embeds its
/// own map, and how it places each country map's list, with a problem for a
/// placement the page cannot draw.
pub(super) fn record_placements(doc: &mut ParsedDocument, from: &str) {
    let mut maps = BTreeSet::new();
    let mut lists: BTreeMap<String, usize> = BTreeMap::new();
    for html in [Some(doc.html_content.as_str()), doc.hero_html.as_deref()].into_iter().flatten() {
        let markers = html.split(MARKER_FOLDER_LIST).skip(1).filter_map(|rest| rest.split_once(MARKER_END).and_then(|(body, _)| parse_marker_body(body)));
        for marker in markers {
            match marker.style.as_deref() {
                Some("map") => { maps.insert(marker_decode(marker.path)); }
                Some("levels") => *lists.entry(marker_decode(marker.path)).or_default() += 1,
                _ => {}
            }
        }
    }
    doc.has_own_map_embed = maps.contains(from);
    doc.country_lists = lists
        .into_iter()
        .map(|(source, count)| {
            let placement = if count > 1 {
                crate::build::cli_output::log_warn_problem!("[{from}] `style:levels` places the list of '{source}' {count} times; it can be placed once, so none is drawn and the map keeps its own.");
                ListPlacement::Refused
            } else if source == from {
                crate::build::cli_output::log_warn_problem!("[{from}] `style:levels` names this page's own map, which shows its table already; the list is not drawn.");
                ListPlacement::Refused
            } else if !maps.contains(&source) {
                crate::build::cli_output::log_warn_problem!("[{from}] `style:levels` places the list of '{source}' with no map of it on the page, so its rows open nothing.");
                ListPlacement::Alone
            } else {
                ListPlacement::Apart
            };
            (source, placement)
        })
        .collect();
}

/// `![[/places/|style:map]]`: the whole site's map. Nothing on a root page
/// that already draws that map itself, and `None` when the target is not the
/// place root.
pub(super) fn render_place_root(
    parsed: &ParsedMarker<'_>,
    ordinal: usize,
    from: &str,
    docs: &[ParsedDocument],
    maps: &PlaceMapRenderContext,
) -> Option<String> {
    let target = marker_decode(parsed.path);
    if target.trim_matches('/') != maps.namespace() || !target.ends_with('/') {
        return None;
    }
    let host_draws_it = docs
        .iter()
        .find(|d| d.source_path.as_deref() == Some(from))
        .is_some_and(|d| maps.draws_own_root_map_for_doc(d));
    if host_draws_it {
        return Some(String::new());
    }
    let svg = maps.render_root_map_embed(docs.iter(), from, ordinal)?;
    Some(place_map_with_placement(svg, &parsed.placement, parsed.caption.as_deref()))
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
    if let Some((doc, model)) = target.and_then(|d| d.country_map.as_ref().map(|m| (d, m))) {
        use crate::build::place_map::country_map::render::{render, Place};
        if model.rows.is_empty() {
            return String::new();
        }
        let host = docs.iter().find(|d| d.source_path.as_deref() == Some(from.as_str()));
        let place = if path == from {
            Place::OwnPage
        } else if host.and_then(|h| h.country_lists.get(&path)) == Some(&ListPlacement::Apart) {
            Place::EmbeddedListApart(&doc.url_path)
        } else {
            Place::Embedded
        };
        return render(model, &doc.title, doc.date.as_deref(), host.map_or(doc.lang, |d| d.lang), place);
    }
    if let Some((doc, maps)) = target.zip(maps) {
        if let Some(svg) = maps.render_article_map(&doc.location, doc.route, &doc.url_path, &from, ordinal) {
            return place_map_with_placement(svg, &parsed.placement, parsed.caption.as_deref());
        }
    }
    crate::build::cli_output::log_warn_problem!("map embed '{}' has no resolved document with coordinate-bearing locations", path);
    format!(r#"<div class="moss-embed-missing">Map unavailable: {}</div>"#, html_escape(&path))
}

/// `![[Risk levels|style:levels]]`: a country map's list, where the page puts it.
pub(super) fn render_levels(parsed: &ParsedMarker<'_>, docs: &[ParsedDocument]) -> String {
    let path = marker_decode(parsed.path);
    let from = marker_decode(parsed.from);
    let missing = || format!(r#"<div class="moss-embed-missing">Levels unavailable: {}</div>"#, html_escape(&path));
    let map = docs.iter().find(|d| d.source_path.as_deref() == Some(path.as_str())).and_then(|d| d.country_map.as_ref().map(|m| (d, m)));
    let Some((doc, model)) = map.filter(|_| !path.ends_with('/')) else {
        crate::build::cli_output::log_warn_problem!("[{from}] `style:levels` names '{path}', which is not a country map page");
        return missing();
    };
    let host = docs.iter().find(|d| d.source_path.as_deref() == Some(from.as_str()));
    match host.and_then(|h| h.country_lists.get(&path)) {
        Some(ListPlacement::Apart | ListPlacement::Alone) => crate::build::place_map::country_map::render::list(model, host.map_or(doc.lang, |h| h.lang), &doc.url_path),
        _ => missing(),
    }
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
            &crate::build::media::dimensions::MediaDimensionLookup::new(&[], &[], &Default::default(), None), None, Some(maps), None,
            crate::build::inputs::now(),
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
            cards: false,
            key: "places".into(), fields: vec!["location".into()], title: "Places".into(),
            titles: Vec::new(), counts: Vec::new(), is_place: true, parents: Default::default(), explorer: None, line: None, slugs: Default::default(),
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

    fn marker(style: &str, target: &str, from: &str) -> String {
        use moss_core::resolve::embed_renderer::folder_list::{classify_folder_segments, emit_marker, marker_encode};
        emit_marker(&marker_encode(target), &marker_encode(from), &classify_folder_segments(&format!("style:{style}")))
    }

    fn placed(from: &str, body: &[(&str, &str)], hero: &[(&str, &str)]) -> ParsedDocument {
        let html = |markers: &[(&str, &str)]| markers.iter().map(|(style, target)| marker(style, target, from)).collect::<Vec<_>>().join("\n<p>Between.</p>\n");
        let mut doc = ParsedDocument { html_content: html(body), hero_html: (!hero.is_empty()).then(|| html(hero)), ..Default::default() };
        record_placements(&mut doc, from);
        doc
    }

    #[test]
    fn a_list_beside_its_map_is_placed_apart_wherever_the_two_are_written() {
        let body = placed("home.md", &[("map", "risk.md"), ("levels", "risk.md")], &[]);
        assert_eq!(body.country_lists.get("risk.md"), Some(&ListPlacement::Apart));
        assert!(!body.has_own_map_embed);
        let hero = placed("home.md", &[("levels", "risk.md")], &[("map", "risk.md")]);
        assert_eq!(hero.country_lists.get("risk.md"), Some(&ListPlacement::Apart), "a map in the hero counts as on the page");
    }

    #[test]
    fn a_list_with_no_map_stands_alone_and_a_misplaced_one_is_refused() {
        assert_eq!(placed("home.md", &[("levels", "risk.md")], &[]).country_lists.get("risk.md"), Some(&ListPlacement::Alone));
        let twice = placed("home.md", &[("map", "risk.md"), ("levels", "risk.md"), ("levels", "risk.md")], &[]);
        assert_eq!(twice.country_lists.get("risk.md"), Some(&ListPlacement::Refused), "two lists would share one id");
        let own = placed("risk.md", &[("map", "risk.md"), ("levels", "risk.md")], &[]);
        assert_eq!(own.country_lists.get("risk.md"), Some(&ListPlacement::Refused), "its own page shows the table");
        assert!(own.has_own_map_embed);
    }

}
