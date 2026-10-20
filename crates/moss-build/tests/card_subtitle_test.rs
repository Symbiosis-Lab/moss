//! A page's `subtitle:` is drawn under its title on every card and row a
//! folder listing makes.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn listing(folder_front: &str) -> String {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        ("shows/index.md", &format!("---\ntitle: Shows\n{folder_front}---\n\nAll shows.\n")),
        (
            "shows/opening.md",
            "---\ntitle: Opening Night\nsubtitle: 示例\ndate: 2026-03-01\nweight: 1\n---\n\nBody.\n",
        ),
        (
            "shows/closing.md",
            "---\ntitle: Closing Night\ndate: 2026-04-01\nweight: 2\n---\n\nBody.\n",
        ),
    ]);
    site.html("shows/index.html")
}

fn assert_subtitle_under_title(html: &str, what: &str) {
    assert_eq!(html.matches(r#"class="moss-card-subtitle">示例</span>"#).count(), 1, "{what}: {html}");
    let title = html.find("Opening Night").unwrap_or_else(|| panic!("{what}: no title"));
    let subtitle = html.find("示例").unwrap();
    assert!(title < subtitle, "{what}: the subtitle comes after the title");
    assert!(!html.contains(r#"moss-card-subtitle"></span>"#), "{what}: an empty subtitle was drawn");
}

#[test]
fn grid_cards_draw_the_subtitle() {
    assert_subtitle_under_title(&listing("children_style: grid\n"), "grid");
}

#[test]
fn summary_cards_draw_the_subtitle() {
    assert_subtitle_under_title(&listing("children_style: summary\n"), "summary");
}

#[test]
fn year_grouped_list_rows_draw_the_subtitle() {
    assert_subtitle_under_title(&listing("children_style: list\n"), "list by date");
}

#[test]
fn list_rows_in_a_hand_set_order_draw_the_subtitle() {
    assert_subtitle_under_title(&listing("children_style: list\nsort: weight\n"), "list by weight");
}

#[test]
fn ungrouped_list_rows_draw_the_subtitle() {
    assert_subtitle_under_title(&listing("children_style: list\nchildren_group: none\n"), "list ungrouped");
}
