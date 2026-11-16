//! Every listing wrapper says what its cards stand for (`data-list-holds`)
//! and in what order they come (`data-list-axis`), so a theme can tell a
//! listing of folders from a listing of pages.

use crate::site;

use site::Site;

/// The `.moss-cards` opening tag on `rel`.
fn wrapper(site: &Site, rel: &str) -> String {
    let html = site.html(rel);
    let from = html.find("<div class=\"moss-cards\"").unwrap_or_else(|| panic!("a listing on {rel}:\n{html}"));
    html[from..].split('>').next().unwrap().to_string()
}

fn page(title: &str) -> String {
    format!("---\ntitle: {title}\ndate: 2026-03-01\n---\n\nBody.\n")
}

fn site(style: &str) -> Site {
    let folder = |title: &str| format!("---\ntitle: {title}\nchildren_style: {style}\n---\n\n{title}.\n");
    Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        // Pages only.
        ("notes/index.md", &folder("Notes")),
        ("notes/one.md", &page("One")),
        ("notes/two.md", &page("Two")),
        // Folders with pages to count.
        ("prizes/index.md", &folder("Prizes")),
        ("prizes/film/index.md", &folder("Film")),
        ("prizes/film/winner.md", &page("Winner")),
        ("prizes/poetry/index.md", &folder("Poetry")),
        ("prizes/poetry/winner.md", &page("Poem")),
        // A counted folder beside a page.
        ("events/index.md", &folder("Events")),
        ("events/talks/index.md", &folder("Talks")),
        ("events/talks/first.md", &page("First talk")),
        ("events/fair.md", &page("Fair")),
    ])
}

/// The wrapper on `rel` says `holds`, and names its order, in every style.
fn assert_holds(rel: &str, holds: &str) {
    for style in ["list", "summary", "grid"] {
        let tag = wrapper(&site(style), rel);
        assert!(tag.contains(&format!(r#"data-list-holds="{holds}""#)), "`children_style: {style}` on {rel}: {tag}");
        assert!(tag.contains("data-list-axis="), "`children_style: {style}` on {rel}: {tag}");
    }
}

#[test]
fn a_listing_of_pages_holds_entries() {
    assert_holds("notes/index.html", "entries");
}

#[test]
fn a_listing_of_counted_folders_holds_collections() {
    assert_holds("prizes/index.html", "collections");
}

#[test]
fn a_listing_of_a_folder_and_a_page_is_mixed() {
    assert_holds("events/index.html", "mixed");
}
