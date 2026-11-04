//! `children_style: lead` draws a folder like `summary` and marks the listing
//! `data-lead`, so the stylesheet can make the first card lead and the rest a
//! register.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn listing(folder_front: &str) -> String {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        ("notes/index.md", &format!("---\ntitle: Notes\n{folder_front}---\n\nAll notes.\n")),
        ("notes/one.md", "---\ntitle: First Note\ndate: 2026-04-01\n---\n\nThe opening line of the first note.\n"),
        ("notes/two.md", "---\ntitle: Second Note\ndate: 2026-03-01\n---\n\nThe opening line of the second note.\n"),
        ("notes/three.md", "---\ntitle: Third Note\ndate: 2025-02-01\n---\n\nThe opening line of the third note.\n"),
        ("notes/four.md", "---\ntitle: Fourth Note\ndate: 2025-01-01\n---\n\nThe opening line of the fourth note.\n"),
    ]);
    site.html("notes/index.html")
}

#[test]
fn lead_style_marks_the_listing_and_draws_summary_cards() {
    let html = listing("children_style: lead\nchildren_group: none\n");
    assert!(html.contains(r#"<div class="moss-cards" data-layout="list" data-lead>"#), "{html}");
    assert!(html.contains("moss-card-description"), "cards carry the opening line: {html}");
    assert!(html.find("First Note").unwrap() < html.find("Second Note").unwrap());
}

#[test]
fn lead_style_honours_the_limit_and_the_more_link() {
    let html = listing("children_style: lead\nchildren_group: none\nchildren_limit: 3\nchildren_more: \"[[All notes]]\"\n");
    assert!(html.contains("data-lead"), "{html}");
    assert!(html.contains("Third Note"), "{html}");
    assert!(!html.contains("Fourth Note"), "the limit is not honoured: {html}");
    assert!(html.contains("All notes"), "{html}");
}

#[test]
fn lead_style_is_not_grouped_by_year_unless_asked() {
    let html = listing("children_style: lead\n");
    assert!(html.contains("data-lead"), "{html}");
    assert!(!html.contains("year-group"), "a front page gets no year headings by default: {html}");
}

#[test]
fn summary_style_is_not_marked_lead() {
    let html = listing("children_style: summary\n");
    assert!(!html.contains("data-lead"), "{html}");
}

#[test]
fn lead_style_works_as_an_embed_option() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\n![[notes/|style:lead,group:none]]\n"),
        ("notes/one.md", "---\ntitle: First Note\ndate: 2026-04-01\n---\n\nThe opening line.\n"),
        ("notes/two.md", "---\ntitle: Second Note\ndate: 2026-03-01\n---\n\nAnother line.\n"),
    ]);
    let html = site.html("index.html");
    assert!(html.contains(r#"data-layout="list" data-lead"#), "{html}");
}
