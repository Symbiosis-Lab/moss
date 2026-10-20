//! A listing written inside a hero is expanded like one in the body, and a
//! listing marker that survives to a finished page is reported.

#[path = "support/site.rs"]
mod site;

use site::{hero_of, Site};

const SOONER: &str = "---\ntitle: Sooner Show\nstart: 2099-03-01 19:30\n---\n\nBody.\n";

#[test]
fn a_folder_embed_inside_a_hero_is_expanded() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Company\n---\n\n:::hero\n# Example Company\n\n![[shows/|style:list]]\n:::\n\nText below.\n"),
        ("shows/index.md", "---\ntitle: Shows\nchildren: false\n---\n"),
        ("shows/sooner.md", SOONER),
    ]);
    let html = site.html("index.html");
    let hero = hero_of(&html);
    assert!(hero.contains("Sooner Show"), "the listing is drawn inside the hero: {hero}");
    // The hero has no paragraph, so the description falls back to its source.
    for meta in ["<meta name=\"description\"", "<meta property=\"og:description\""] {
        if let Some(at) = html.find(meta) {
            let tag = &html[at..html[at..].find('>').map_or(html.len(), |i| at + i)];
            assert!(!tag.contains("MOSS_MARKER"), "description quotes a marker: {tag}");
        }
    }
    if let Some(at) = html.find("MOSS_MARKER") {
        panic!("raw marker: {}", &html[at.saturating_sub(300)..(at + 200).min(html.len())]);
    }
}

#[test]
fn a_raw_listing_marker_in_a_built_page_warns() {
    // A marker the expander cannot close is left as written.
    let site = Site::new(&[(
        "index.md",
        "---\ntitle: Example Company\n---\n\nIntro.\n\n<!--MOSS_MARKER_FOLDER_LIST:path=shows/ no end\n\nText below.\n",
    )]);
    assert!(
        site.logged.iter().any(|l| l.contains("index.md") && l.contains("listing")),
        "a warning names the page: {:?}",
        site.logged
    );
}
