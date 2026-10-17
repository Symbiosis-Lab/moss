//! German is a site edition: a page under a `de/` tree is dressed in German
//! interface strings, German dates and `<html lang="de">`, and the switcher on
//! the other editions reads "DE", as the English one reads "EN". An edition's home and top level
//! behave like the root's.

#[path = "support/site.rs"]
mod site;

use site::Site;

#[test]
fn a_de_tree_is_dressed_in_german_chrome_and_dates() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\nchildren_depth: direct\n---\n\nWillkommen.\n"),
        ("de/blog/index.md", "---\ntitle: Blog\n---\n\nText.\n"),
        ("de/blog/erster.md", "---\ntitle: Erster\ndate: 2026-10-15\n---\n\n## Anfang\n\nText.\n"),
        ("de/blog/zweiter.md", "---\ntitle: Zweiter\ndate: 2026-10-16\n---\n\nText.\n"),
    ]);

    let post = site.html("de/blog/erster/index.html");
    assert!(post.contains(r#"<html data-moss-html-version="1" lang="de">"#), "{post}");
    assert!(post.contains("Zum Inhalt springen"), "skip link");
    assert!(post.contains(r#"aria-label="Sprache""#), "language switcher label");
    assert!(post.contains(r#"aria-label="Leseeinstellungen""#), "reading preferences");
    assert!(post.contains(">15. Oktober 2026<"), "formatted date");
    assert!(post.contains(r#"aria-label="Dauerhafter Link zu diesem Abschnitt""#), "heading anchor");

    let home = site.html("de/index.html");
    assert!(home.contains("2 Artikel"), "folder card count: {home}");
    assert!(home.contains("Veröffentlicht mit moss"), "colophon");
    assert!(!home.contains("Skip to content"), "no English chrome on a German page");

    // The English edition keeps its own chrome and points at the German one.
    let root = site.html("index.html");
    assert!(root.contains("Skip to content") && root.contains(r#"hreflang="de""#), "{root}");
    assert!(root.contains(r#"hreflang="de">DE</a>"#), "the switcher labels the German edition DE, as it labels English EN: {root}");
}

#[test]
fn a_regional_german_tree_keeps_its_region_beside_the_plain_german_one() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
        ("about.md", "---\ntitle: About\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
        ("de/about.md", "---\ntitle: Über\n---\n\nText.\n"),
        ("de-at/index.md", "---\ntitle: Servus\n---\n\nWillkommen.\n"),
        ("de-at/about.md", "---\ntitle: Über uns\n---\n\nText.\n"),
    ]);

    let de = site.html("de/about/index.html");
    assert!(de.contains(r#"<html data-moss-html-version="1" lang="de">"#), "{de}");
    assert!(de.contains("Zum Inhalt springen"), "German chrome in de/");

    let at = site.html("de-at/about/index.html");
    assert!(at.contains(r#"<html data-moss-html-version="1" lang="de-AT">"#), "{at}");
    assert!(at.contains("Zum Inhalt springen"), "German chrome in de-at/");

    let root = site.html("about/index.html");
    assert!(root.contains(r#"hreflang="de""#), "plain German alternate: {root}");
    assert!(root.contains(r#"hreflang="de-AT""#), "regional German alternate: {root}");
}

const EDITION_SITE: &[(&str, &str)] = &[
    ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
    ("About.md", "---\ntitle: About\n---\n\nText.\n"),
    ("Notes/first.md", "---\ntitle: First note\ndate: 2026-01-01\n---\n\nText.\n"),
    ("Notes/2026/deep.md", "---\ntitle: Deep note\ndate: 2026-01-02\n---\n\nText.\n"),
    ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n"),
    ("de/Über.md", "---\ntitle: Über\n---\n\nText.\n"),
    ("de/Notizen/erste.md", "---\ntitle: Erste Notiz\ndate: 2026-01-01\n---\n\nText.\n"),
    ("de/Notizen/2026/tiefe.md", "---\ntitle: Tiefe Notiz\ndate: 2026-01-02\n---\n\nText.\n"),
];

#[test]
fn an_edition_home_lists_its_whole_tree_and_only_its_own() {
    let site = Site::new(EDITION_SITE);

    let (_, de) = site.links("de/index.html");
    assert!(de.contains(&"/de/notizen/erste/".to_string()), "{de:?}");
    assert!(de.contains(&"/de/notizen/2026/tiefe/".to_string()), "subfolder pages: {de:?}");
    assert!(de.iter().all(|h| !h.starts_with("/notes") && h != "/about/"), "no root page: {de:?}");

    let (_, root) = site.links("index.html");
    assert!(root.contains(&"/notes/2026/deep/".to_string()), "{root:?}");
    assert!(root.iter().all(|h| !h.starts_with("/de/")), "no German page: {root:?}");
}

#[test]
fn a_top_level_page_of_an_edition_joins_its_nav_and_leaves_its_listing() {
    let site = Site::new(EDITION_SITE);

    let (de_nav, de_main) = site.links("de/index.html");
    assert!(de_nav.iter().any(|h| h.contains("ber/") && h.starts_with("/de/")), "German nav: {de_nav:?}");
    assert!(de_main.iter().all(|h| !h.contains("ber/")), "not in the German listing: {de_main:?}");
    assert!(de_nav.iter().all(|h| h != "/about/"), "{de_nav:?}");

    let (en_nav, _) = site.links("index.html");
    assert!(en_nav.contains(&"/about/".to_string()), "{en_nav:?}");
    assert!(en_nav.iter().all(|h| !h.starts_with("/de/ber") && !h.starts_with("/de/%C3")), "English nav: {en_nav:?}");

    // The same bar on a deeper German page.
    let (nested_nav, _) = site.links("de/notizen/erste/index.html");
    assert!(nested_nav.iter().any(|h| h.starts_with("/de/") && h.contains("ber/")), "{nested_nav:?}");
}

#[test]
fn children_depth_direct_keeps_the_narrow_edition_listing() {
    let mut files = EDITION_SITE.to_vec();
    files.retain(|(p, _)| *p != "de/index.md");
    files.push(("de/index.md", "---\ntitle: Start\nchildren_depth: direct\n---\n\nWillkommen.\n"));
    let site = Site::new(&files);

    let (_, de) = site.links("de/index.html");
    assert!(de.contains(&"/de/notizen/".to_string()), "the folder card: {de:?}");
    assert!(de.iter().any(|h| h.contains("ber/")), "the top-level page is listed again: {de:?}");
    assert!(!de.contains(&"/de/notizen/erste/".to_string()), "{de:?}");
}

#[test]
fn a_generated_edition_home_lists_like_a_written_one() {
    let files: Vec<_> = EDITION_SITE.iter().copied().filter(|(p, _)| *p != "de/index.md").collect();
    let site = Site::new(&files);

    let (nav, main) = site.links("de/index.html");
    assert!(main.contains(&"/de/notizen/2026/tiefe/".to_string()), "{main:?}");
    assert!(main.iter().all(|h| !h.starts_with("/notes") && !h.contains("ber/")), "{main:?}");
    assert!(nav.iter().any(|h| h.starts_with("/de/") && h.contains("ber/")), "{nav:?}");
}

#[test]
fn a_tree_for_the_site_language_does_not_repeat_the_root_nav() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nText.\n"),
        ("About.md", "---\ntitle: About\n---\n\nText.\n"),
        ("Notes/first.md", "---\ntitle: First\ndate: 2026-01-01\n---\n\nText.\n"),
        ("en/About.md", "---\ntitle: About again\n---\n\nText.\n"),
    ]);
    let (nav, _) = site.links("index.html");
    assert!(nav.iter().all(|h| !h.starts_with("/en/")), "{nav:?}");
}

#[test]
fn on_a_root_that_only_chooses_a_language_the_site_language_tree_is_an_edition_too() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Choose\n---\n\nPick a language.\n"),
        ("en/index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("en/About.md", "---\ntitle: About\n---\n\nText.\n"),
        ("en/Posts/A.md", "---\ntitle: A\ndate: 2026-01-01\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\n---\n\nHallo.\n"),
        ("de/Posts/B.md", "---\ntitle: B\ndate: 2026-01-01\n---\n\nText.\n"),
    ]);

    let (en_nav, en_main) = site.links("en/index.html");
    assert_eq!(en_nav.first().map(String::as_str), Some("/en/"), "the site name leads to the English home: {en_nav:?}");
    assert!(en_nav.contains(&"/en/about/".to_string()), "{en_nav:?}");
    assert!(en_main.contains(&"/en/posts/a/".to_string()), "{en_main:?}");

    let (de_nav, _) = site.links("de/posts/b/index.html");
    assert!(de_nav.contains(&"/en/".to_string()) && !de_nav.contains(&"/".to_string()), "the switch leads to the English home, not the chooser: {de_nav:?}");

    let (_, root) = site.links("index.html");
    assert!(root.iter().all(|h| !h.starts_with("/en/")), "the chooser lists no edition's pages: {root:?}");
}

#[test]
fn a_root_with_a_page_of_its_own_or_a_single_tree_does_not_only_choose_a_language() {
    let chooser = [
        ("index.md", "---\ntitle: Choose\n---\n\nPick a language.\n"),
        ("footer.md", "Footer.\n"),
        ("en/index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("en/About.md", "---\ntitle: About\n---\n\nText.\n"),
        ("de/index.md", "---\ntitle: Start\n---\n\nHallo.\n"),
    ];
    let en_home = |files: &[(&str, &str)]| Site::new(files).links("en/index.html").0.first().cloned();
    assert_eq!(en_home(&chooser).as_deref(), Some("/en/"), "a footer is no page of the root's own");

    let mut with_html = chooser.to_vec();
    with_html.push(("Imprint.html", "<!doctype html><title>Imprint</title><p>Text.</p>\n"));
    assert_eq!(en_home(&with_html).as_deref(), Some("/"), "a written HTML page belongs to the root");

    let single: Vec<_> = chooser.iter().copied().filter(|(p, _)| !p.starts_with("de/")).collect();
    assert_eq!(en_home(&single).as_deref(), Some("/"), "one tree is no choice of language");
}
