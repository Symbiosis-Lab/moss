//! An event page, card and row say whether the event is upcoming or past
//! (`data-when`), and an upcoming card or row carries a translated label.

#[path = "support/site.rs"]
mod site;

use site::Site;

const FILES: [(&str, &str); 4] = [
    ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
    ("shows/index.md", "---\ntitle: Shows\nchildren_style: STYLE\n---\n\nAll shows.\n"),
    ("shows/soon.md", "---\ntitle: Example Show\nstart: 2099-03-01 19:30\n---\n\nBody.\n"),
    ("shows/gone.md", "---\ntitle: Old Show\nstart: 2001-05-05 10:00\n---\n\nBody.\n"),
];

fn build(lang: &str, style: &str) -> Site {
    let files: Vec<(&str, String)> = FILES.iter().map(|(p, b)| (*p, b.replace("STYLE", style))).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, b)| (*p, b.as_str())).collect();
    Site::in_language(lang, "", &refs)
}

fn label(lang: &str) -> &'static str {
    match lang {
        "de" => "Demnächst",
        "zh-hans" => "即将上演",
        "zh-hant" => "即將上演",
        _ => "Upcoming",
    }
}

#[test]
fn the_event_page_meta_says_upcoming_or_past() {
    let site = build("en", "grid");
    assert!(site.html("shows/soon/index.html").contains(r#"class="date moss-event-meta" data-when="upcoming""#));
    assert!(site.html("shows/gone/index.html").contains(r#"class="date moss-event-meta" data-when="past""#));
}

#[test]
fn every_listing_form_marks_each_event_and_labels_the_upcoming_one() {
    for style in ["grid", "summary", "list"] {
        let html = build("en", style).html("shows/index.html");
        assert_eq!(html.matches(r#"data-when="upcoming""#).count(), 1, "{style}: {html}");
        assert_eq!(html.matches(r#"data-when="past""#).count(), 1, "{style}: {html}");
        assert_eq!(html.matches(r#"<span class="moss-event-upcoming">Upcoming</span>"#).count(), 1, "{style}: {html}");
    }
}

#[test]
fn the_label_is_translated() {
    for lang in ["de", "zh-hans", "zh-hant"] {
        let html = build(lang, "grid").html("shows/index.html");
        let want = format!(r#"<span class="moss-event-upcoming">{}</span>"#, label(lang));
        assert_eq!(html.matches(&want).count(), 1, "{lang}: {html}");
    }
}
