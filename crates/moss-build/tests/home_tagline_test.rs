//! A home page's `tagline:` follows the site name in its tab title and share
//! titles; every other page keeps the plain site name.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn tag(html: &str, open: &str) -> String {
    let start = html.find(open).unwrap_or_else(|| panic!("no {open} in {html}")) + open.len();
    html[start..].split(['<', '"']).next().unwrap().to_string()
}

fn build(home_front: &str) -> Site {
    Site::new(&[
        ("index.md", &format!("---\ntitle: Harbor Notes\n{home_front}---\n\nWelcome.\n")),
        ("about.md", "---\ntitle: About\ntagline: ignored here\n---\n\nHello.\n"),
    ])
}

#[test]
fn tagline_joins_the_site_name_on_the_home_page_only() {
    let site = build("tagline: Tide tables and tall stories\n");
    let home = site.html("index.html");
    let full = "Harbor Notes - Tide tables and tall stories";
    assert_eq!(tag(&home, "<title>"), full);
    assert_eq!(tag(&home, r#"<meta property="og:title" content=""#), full);
    assert_eq!(tag(&home, r#"<meta name="twitter:title" content=""#), full);
    assert_eq!(tag(&home, r#"<a href="/" class="site-name">"#), "Harbor Notes");
    assert_eq!(tag(&home, r#"<meta property="og:site_name" content=""#), "Harbor Notes");
    assert!(!home.contains(">Tide tables and tall stories<"), "the tagline is not drawn on the page");

    let about = site.html("about/index.html");
    assert_eq!(tag(&about, "<title>"), "About - Harbor Notes");
}

#[test]
fn a_home_without_a_tagline_is_unchanged() {
    let home = build("").html("index.html");
    assert_eq!(tag(&home, "<title>"), "Harbor Notes");
    assert_eq!(tag(&home, r#"<meta property="og:title" content=""#), "Harbor Notes");
    assert_eq!(tag(&home, r#"<meta name="twitter:title" content=""#), "Harbor Notes");
}
