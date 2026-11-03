//! `date:` accepts a bare year written unquoted, which YAML reads as an
//! integer, as the guide says it does.

#[path = "support/site.rs"]
mod site;

use site::Site;

#[test]
fn a_page_dated_with_an_unquoted_year_builds() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        ("notes/index.md", "---\ntitle: Notes\n---\n\nAll notes.\n"),
        ("notes/old.md", "---\ntitle: Old Note\ndate: 2023\n---\n\nBody.\n"),
    ]);
    let html = site.html("notes/old/index.html");
    assert!(html.contains("Old Note"), "{html}");
    assert!(html.contains("2023"), "the year is shown: {html}");
}
