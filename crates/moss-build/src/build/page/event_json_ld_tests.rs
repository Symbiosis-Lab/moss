use super::*;
use crate::build::page::layout::LayoutConfig;
use crate::build::render::html::generate_html;
use crate::build::types::ParsedDocument;
use crate::i18n::Language;
use crate::types::content::ProjectStructure;
use moss_core::PageKind;

fn site() -> SiteUrl {
    SiteUrl::parse("https://example.test").unwrap()
}

fn project() -> ProjectStructure {
    ProjectStructure {
        root_path: String::new(),
        markdown_files: vec![],
        html_files: vec![],
        image_files: vec![],
        video_files: vec![],
        notebook_files: vec![],
        other_files: vec![],
        total_files: 0,
        homepage_file: Some("index.md".to_string()),
        ffmpeg_bin_path: None,
        evicted_count: 0,
        evicted_paths: Vec::new(),
        has_content_folders: false,
        has_language_trees: false,
        passthrough_roots: std::collections::HashSet::new(),
        dirs: Vec::new(),
    }
}

fn page(title: &str, fm: &[(&str, &str)], places: &[&str]) -> ParsedDocument {
    let get = |k: &str| fm.iter().find(|(key, _)| *key == k).map(|(_, v)| v.to_string());
    let event = get("start").and_then(|s| EventTime::parse(&s).ok()).map(|start| EventFields {
        start,
        when: start.sort_key(),
        end: get("end").and_then(|e| EventTime::parse(&e).ok()),
        timezone: get("timezone"),
        status: get("status"),
        tickets: get("tickets"),
        online: get("online"),
    });
    ParsedDocument {
        event,
        title: title.to_string(),
        label: title.to_string(),
        url_path: "events/spring/index.html".to_string(),
        html_content: "<article><p>Body.</p></article>".to_string(),
        slug: "spring".to_string(),
        permalink: "/events/spring/".to_string(), // allow:served-path-url-construct (test fixture — permalink field, not HTML-emitted URL)
        lang: Language::En,
        kind: PageKind::Article,
        date: Some("2026-01-05".to_string()),
        description: Some("An evening of chamber music.".to_string()),
        location: places.iter().map(|p| p.to_string()).collect(),
        raw_frontmatter: fm
            .iter()
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect(),
        ..Default::default()
    }
}

/// The JSON-LD block of the rendered page, parsed. Fails the test when the
/// emitted text is not valid JSON.
fn rendered_json_ld(doc: &ParsedDocument) -> Option<Value> {
    let home = ParsedDocument {
        title: "Example Hall".into(),
        label: "Example Hall".into(),
        url_path: "index.html".into(),
        lang: Language::En,
        kind: PageKind::Article,
        ..Default::default()
    };
    let all = vec![home, doc.clone()];
    let html = generate_html(
        Some(doc), &all, &project(), &LayoutConfig::new("test-site", Some("Example Hall")),
        false, None, None, Language::En, None, false, false, None, false, None, None,
        &std::collections::HashMap::new(), &site(), false, false, "favicon.svg", None,
        std::path::Path::new(""),
    )
    .expect("render");
    let open = "<script type=\"application/ld+json\">";
    let start = html.find(open)? + open.len();
    let end = start + html[start..].find("</script>")?;
    Some(serde_json::from_str(&html[start..end]).expect("JSON-LD must parse"))
}

#[test]
fn timed_event_with_zone_is_an_event_with_zoned_times() {
    let doc = page(
        "Spring Concert",
        &[("start", "2026-11-01 14:00"), ("end", "2026-11-01 16:30"), ("timezone", "Asia/Taipei")],
        &["Example Hall"],
    );
    let ld = rendered_json_ld(&doc).expect("json-ld");
    assert_eq!(ld["@type"], "Event");
    assert_eq!(ld["name"], "Spring Concert");
    assert_eq!(ld["url"], "https://example.test/events/spring/");
    assert_eq!(ld["description"], "An evening of chamber music.");
    assert_eq!(ld["startDate"], "2026-11-01T14:00+08:00");
    assert_eq!(ld["endDate"], "2026-11-01T16:30+08:00");
    assert_eq!(ld["eventStatus"], "https://schema.org/EventScheduled");
    assert_eq!(ld["eventAttendanceMode"], "https://schema.org/OfflineEventAttendanceMode");
    assert_eq!(ld["location"], json!({"@type": "Place", "name": "Example Hall"}));
    assert_eq!(ld["organizer"], json!({"@type": "Organization", "name": "Example Hall"}));
    assert!(ld.get("datePublished").is_none(), "an event is not also an Article");
}

#[test]
fn zone_offset_follows_daylight_saving_in_the_event_zone() {
    let summer = page("Summer Talk", &[("start", "2026-07-01 14:00"), ("timezone", "America/New_York")], &[]);
    let ld = rendered_json_ld(&summer).expect("json-ld");
    assert_eq!(ld["startDate"], "2026-07-01T14:00-04:00");
    let winter = page("Winter Talk", &[("start", "2026-12-01 14:00"), ("timezone", "America/New_York")], &[]);
    let ld = rendered_json_ld(&winter).expect("json-ld");
    assert_eq!(ld["startDate"], "2026-12-01T14:00-05:00");
}

#[test]
fn all_day_event_has_date_only_values() {
    let doc = page("Festival", &[("start", "2026-11-01"), ("end", "2026-11-03")], &["Example Park"]);
    let ld = rendered_json_ld(&doc).expect("json-ld");
    assert_eq!(ld["@type"], "Event");
    assert_eq!(ld["startDate"], "2026-11-01");
    assert_eq!(ld["endDate"], "2026-11-03");
}

#[test]
fn cancelled_online_event_with_tickets() {
    let doc = page(
        "Webcast",
        &[
            ("start", "2026-11-01 19:00"), ("timezone", "Europe/Berlin"), ("status", "cancelled"),
            ("online", "https://live.example.test/x"), ("tickets", "https://tickets.example.test/x"),
        ],
        &[],
    );
    let ld = rendered_json_ld(&doc).expect("json-ld");
    assert_eq!(ld["eventStatus"], "https://schema.org/EventCancelled");
    assert_eq!(ld["eventAttendanceMode"], "https://schema.org/OnlineEventAttendanceMode");
    assert_eq!(ld["location"], json!({"@type": "VirtualLocation", "url": "https://live.example.test/x"}));
    assert_eq!(ld["offers"], json!({"@type": "Offer", "url": "https://tickets.example.test/x"}));
    assert!(ld.get("endDate").is_none());
}

#[test]
fn online_plus_place_is_mixed() {
    let doc = page("Hybrid", &[("start", "2026-11-01"), ("online", "https://live.example.test/x")], &["Example Hall"]);
    let ld = rendered_json_ld(&doc).expect("json-ld");
    assert_eq!(ld["eventAttendanceMode"], "https://schema.org/MixedEventAttendanceMode");
    assert_eq!(ld["location"].as_array().map(Vec::len), Some(2));
}

#[test]
fn page_without_start_keeps_its_article_json_ld() {
    let doc = page("Notes", &[], &[]);
    let ld = rendered_json_ld(&doc).expect("json-ld");
    assert_eq!(ld["@type"], "Article");
    assert_eq!(ld["headline"], "Notes");
    assert_eq!(ld["datePublished"], "2026-01-05");
}

#[test]
fn title_cannot_close_the_script_block() {
    let doc = page("</script><b>x", &[("start", "2026-11-01")], &[]);
    let ld = rendered_json_ld(&doc).expect("json-ld");
    assert_eq!(ld["name"], "</script><b>x");
}

#[test]
fn event_keeps_tags_as_keywords_in_any_layout() {
    let mut doc = page("Talk", &[("start", "2026-11-01")], &[]);
    doc.tags = Some(vec!["music".into()]);
    doc.kind = PageKind::Folder;
    let ld = rendered_json_ld(&doc).expect("json-ld");
    assert_eq!(ld["@type"], "Event");
    assert_eq!(ld["keywords"], json!(["music"]));
}
