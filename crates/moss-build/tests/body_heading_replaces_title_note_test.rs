//! A nav page whose body has its own `# Heading` loses the injected page title:
//! the build says so, naming the page, because the page's heading is then not
//! the title the author wrote in frontmatter.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn notes(site: &Site) -> Vec<&String> {
    site.logged.iter().filter(|l| l.contains("replaces the page title")).collect()
}

#[test]
fn a_body_heading_that_replaces_the_page_title_is_named() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Lab\n---\n\nWelcome.\n"),
        ("research.md", "---\ntitle: Research\nnav: true\n---\n\nIntro.\n\n# Theme 1\n\nText.\n"),
    ]);
    let found = notes(&site);
    assert_eq!(found.len(), 1, "{:?}", site.logged);
    assert!(found[0].contains("research.md") && found[0].contains("Theme 1") && found[0].contains("Research"), "{}", found[0]);
    assert!(!site.html("research/index.html").contains("moss-article-title"), "the output is unchanged");
}

#[test]
fn a_body_that_opens_with_the_title_gets_no_note() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Lab\n---\n\nWelcome.\n"),
        ("research.md", "---\ntitle: Research\nnav: true\n---\n\n# Research\n\nText.\n"),
    ]);
    assert!(notes(&site).is_empty(), "{:?}", site.logged);
}

#[test]
fn an_article_outside_the_nav_keeps_its_title_and_gets_no_note() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Lab\n---\n\nWelcome.\n"),
        ("notes/trip.md", "---\ntitle: Trip\n---\n\n# Day 1\n\nText.\n"),
    ]);
    assert!(notes(&site).is_empty(), "{:?}", site.logged);
}

#[test]
fn a_hero_heading_on_a_page_outside_the_nav_gets_no_nav_advice() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Lab\n---\n\nWelcome.\n"),
        ("notes/trip.md", "---\ntitle: Trip\n---\n\n:::hero {image=day.png}\n# Day 1\n:::\n"),
    ]);
    assert!(notes(&site).is_empty(), "{:?}", site.logged);
}
