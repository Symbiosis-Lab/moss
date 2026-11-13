//! A `children:` that names an edition's home page lists what that home's own
//! listing lists: its edition, none of another.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn archive(home: &str) -> String {
    format!("---\ntitle: Archive\nchildren: \"[[{home}]]\"\nchildren_depth: all\n---\n\nAll.\n")
}

fn two_editions() -> Site {
    Site::in_language("zh-hans", "", &[
        ("Field Notes.md", "---\ntitle: Field Notes\nhome: true\ntranslationKey: home\nchildren_limit: 1\n---\n\nHome.\n"),
        ("Archive.md", &archive("Field Notes")),
        ("Essay.md", "---\ntitle: Root Essay\ndate: 2026-01-01\n---\n\nText.\n"),
        ("Walk.md", "---\ntitle: Root Walk\ndate: 2026-01-02\n---\n\nText.\n"),
        ("en/Notes.md", "---\ntitle: Notes\nhome: true\ntranslationKey: home\n---\n\nHome.\n"),
        ("en/Archive.md", &archive("Notes")),
        ("en/Essay.md", "---\ntitle: English Essay\ndate: 2026-01-03\n---\n\nText.\n"),
        ("en/Walk.md", "---\ntitle: English Walk\ndate: 2026-01-04\n---\n\nText.\n"),
    ])
}

#[test]
fn an_archive_naming_the_root_home_lists_the_root_edition_only() {
    let html = two_editions().html("archive/index.html");
    assert!(!html.contains("moss-embed-missing"), "{html}");
    assert!(html.contains("Root Essay") && html.contains("Root Walk"), "{html}");
    assert!(!html.contains("English Essay") && !html.contains("English Walk"), "{html}");
}

#[test]
fn an_archive_naming_an_edition_home_lists_that_edition_only() {
    let html = two_editions().html("en/archive/index.html");
    assert!(!html.contains("moss-embed-missing"), "{html}");
    assert!(html.contains("English Essay") && html.contains("English Walk"), "{html}");
    assert!(!html.contains("Root Essay") && !html.contains("Root Walk"), "{html}");
}

#[test]
fn an_edition_homes_children_source_redirects_its_listing() {
    let site = Site::in_language("zh-hans", "", &[
        ("Field Notes.md", "---\ntitle: Field Notes\nhome: true\ntranslationKey: home\n---\n\nHome.\n"),
        ("en/Notes.md", "---\ntitle: Notes\nhome: true\ntranslationKey: home\nchildren: \"[[log]]\"\n---\n\nHome.\n"),
        ("en/Other.md", "---\ntitle: English Other\ndate: 2026-01-03\n---\n\nText.\n"),
        ("log/index.md", "---\ntitle: Log\n---\n\nLog.\n"),
        ("log/One.md", "---\ntitle: Log One\ndate: 2026-01-04\n---\n\nText.\n"),
    ]);
    let html = site.html("en/index.html");
    let main = &html[html.find("<main").unwrap()..];
    assert!(main.contains("Log One"), "{main}");
    assert!(!main.contains("English Other"), "{main}");
}

#[test]
fn a_title_edit_under_the_edition_home_a_home_names_redraws_that_home() {
    // `notes/` is a folder whose name the reference also matches; the home lists `en`.
    let site = Site::in_language("zh-hans", "", &[
        ("Field Notes.md", "---\ntitle: Field Notes\nhome: true\ntranslationKey: home\nchildren: \"[[Notes]]\"\n---\n\nHome.\n"),
        ("en/Notes.md", "---\ntitle: Notes\nhome: true\ntranslationKey: home\n---\n\nHome.\n"),
        ("notes/index.md", "---\ntitle: Notes\n---\n\nA plain folder.\n"),
        ("en/posts/Walk.md", "---\ntitle: First Title\ndate: 2026-01-02\n---\n\nText.\n"),
    ]);
    assert!(site.html("index.html").contains("First Title"));
    site.write("en/posts/Walk.md", "---\ntitle: Second Title\ndate: 2026-01-02\n---\n\nText.\n");
    let post = site.root.join("en/posts/Walk.md");
    site::build_sync(&site.root.to_string_lossy(), moss_build::build::BuildTrigger::ContentOnly(vec![post])).unwrap();
    assert!(site.html("index.html").contains("Second Title"), "a watch build redraws the home's listing");
}
