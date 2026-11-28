//! A folder lists its children in the same order whichever `children_style`
//! draws them: subfolders lead the pages in rows and in cards alike.

use crate::site;

use site::{hrefs_in, Site};

const SLUGS: [&str; 6] = ["alder", "birch", "cedar", "dogwood", "elm", "fir"];

/// The children named in `slugs`, in the order the folder's listing links to
/// them, and the listing's markup.
fn order_of(site: &Site, slugs: &[&'static str]) -> (Vec<&'static str>, String) {
    let html = site.html("trees/index.html");
    let listing = ["class=\"moss-cards", "class=\"moss-stream"]
        .iter()
        .find_map(|wrapper| html.split_once(wrapper))
        .unwrap_or_else(|| panic!("a listing:\n{html}"))
        .1
        .to_string();
    let mut seen: Vec<&'static str> = Vec::new();
    for href in hrefs_in(&listing) {
        if let Some(slug) = slugs.iter().find(|s| href.contains(*s)) {
            if !seen.contains(slug) {
                seen.push(slug);
            }
        }
    }
    assert_eq!(seen.len(), slugs.len(), "every child listed:\n{listing}");
    (seen, listing)
}

fn order(site: &Site) -> (Vec<&'static str>, String) {
    order_of(site, &SLUGS)
}

/// Two dated pages, an undated page, two undated subfolders (one with a dated
/// page inside) and a subfolder whose own page has a date, listed in `style`.
fn listing(style: &str, extra: &str) -> (Vec<&'static str>, String) {
    let index = format!("---\ntitle: Trees\nchildren_style: {style}\n{extra}---\n\nTrees.\n");
    let dated = |title: &str, date: &str| format!("---\ntitle: {title}\ndate: {date}\n---\n\nBody.\n");
    let plain = |title: &str| format!("---\ntitle: {title}\n---\n\nBody.\n");
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("trees/index.md", &index),
        ("trees/alder.md", &dated("Alder", "2026-03-01")),
        ("trees/birch/index.md", &plain("Birch")),
        ("trees/birch/leaf.md", &dated("Birch leaf", "2026-04-01")),
        ("trees/cedar.md", &dated("Cedar", "2026-05-01")),
        ("trees/dogwood/index.md", &dated("Dogwood", "2026-02-01")),
        ("trees/dogwood/leaf.md", &plain("Dogwood leaf")),
        ("trees/elm/index.md", &plain("Elm")),
        ("trees/elm/leaf.md", &plain("Elm leaf")),
        ("trees/fir.md", &plain("Fir")),
    ]);
    order(&site)
}

/// Newest first, folders by their newest page, then the pages; `summary` and
/// `stream` draw no year sections by default, so they are in the next test.
#[test]
fn cards_keep_the_order_of_the_rows_they_replace() {
    let rows = ["birch", "dogwood", "elm", "cedar", "alder", "fir"];
    for style in ["list", "grid"] {
        assert_eq!(listing(style, "").0, rows, "`children_style: {style}`");
    }
}

/// Without year sections a folder with a date of its own takes its place by it.
#[test]
fn a_folder_without_year_sections_lists_in_one_order_whatever_its_style() {
    let rows = ["birch", "elm", "cedar", "alder", "dogwood", "fir"];
    for style in ["list", "summary", "grid", "stream"] {
        assert_eq!(listing(style, "children_group: none\n").0, rows, "`children_style: {style}`");
    }
}

const TYPED: &str = "sort: [fir, elm, cedar, dogwood, birch, alder]\n";

#[test]
fn a_typed_order_is_drawn_as_typed_whatever_the_style() {
    let typed = ["fir", "elm", "cedar", "dogwood", "birch", "alder"];
    for style in ["list", "summary", "grid", "stream"] {
        assert_eq!(listing(style, TYPED).0, typed, "`children_style: {style}`");
    }
}

/// Year sections hold pages only: asked for on a typed order, they keep the
/// folders above them, in the typed order.
#[test]
fn a_typed_order_with_year_sections_keeps_its_folders_above_them() {
    for style in ["list", "summary"] {
        let (order, listing) = listing(style, &format!("{TYPED}children_group: year\n"));
        assert_eq!(order[..3], ["elm", "dogwood", "birch"], "`children_style: {style}`");
        let first_section = listing.find("year-group").unwrap_or_else(|| panic!("year sections:\n{listing}"));
        let in_sections = hrefs_in(&listing[first_section..]);
        assert!(
            !in_sections.iter().any(|h| ["elm", "dogwood", "birch"].iter().any(|f| h.contains(f))),
            "a folder inside a year section:\n{listing}"
        );
    }
}

/// A folder whose subfolders hold no pages of their own is still a folder:
/// it keeps its weight among the other folders instead of sinking below them.
#[test]
fn a_folder_with_nothing_to_count_keeps_its_weight() {
    let page = |title: &str, weight: u8| format!("---\ntitle: {title}\nweight: {weight}\n---\n\nBody.\n");
    for style in ["list", "grid"] {
        let index = format!("---\ntitle: Trees\nchildren_style: {style}\n---\n\nTrees.\n");
        let site = Site::new(&[
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("trees/index.md", &index),
            ("trees/alder/index.md", &page("Alder", 1)),
            ("trees/alder/leaf.md", &page("Alder leaf", 1)),
            ("trees/birch/index.md", &page("Birch", 2)),
            ("trees/birch/bud/index.md", &page("Birch bud", 1)),
            ("trees/cedar/index.md", &page("Cedar", 3)),
            ("trees/cedar/leaf.md", &page("Cedar leaf", 1)),
            ("trees/dogwood/index.md", &page("Dogwood", 4)),
            ("trees/dogwood/leaf.md", &page("Dogwood leaf", 1)),
            ("trees/elm.md", &page("Elm", 5)),
            ("trees/fir.md", &page("Fir", 6)),
        ]);
        assert_eq!(order(&site).0, SLUGS, "`children_style: {style}`");
    }
}

/// On a date order, undated folders lead by title, whether they hold pages
/// to count or only folders.
#[test]
fn an_undated_folder_of_folders_sorts_by_title_among_the_folders() {
    let plain = |title: &str| format!("---\ntitle: {title}\n---\n\nBody.\n");
    for style in ["list", "grid"] {
        let index = format!("---\ntitle: Trees\nchildren_style: {style}\n---\n\nTrees.\n");
        let site = Site::new(&[
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("trees/index.md", &index),
            ("trees/aspen/index.md", &plain("Aspen")),
            ("trees/aspen/grove/index.md", &plain("Aspen grove")),
            ("trees/birch/index.md", &plain("Birch")),
            ("trees/birch/leaf.md", &plain("Birch leaf")),
            ("trees/cedar.md", "---\ntitle: Cedar\ndate: 2026-05-01\n---\n\nBody.\n"),
            ("trees/fir.md", &plain("Fir")),
        ]);
        assert_eq!(order_of(&site, &["aspen", "birch", "cedar", "fir"]).0, ["aspen", "birch", "cedar", "fir"], "`children_style: {style}`");
    }
}

/// A folder with no pages and no folder inside (a folder of pictures, say)
/// lists as a page: it does not lead the pages.
#[test]
fn a_folder_with_nothing_inside_to_list_lists_as_a_page() {
    let plain = |title: &str| format!("---\ntitle: {title}\n---\n\nBody.\n");
    for style in ["list", "grid"] {
        let index = format!("---\ntitle: Trees\nchildren_style: {style}\n---\n\nTrees.\n");
        let site = Site::new(&[
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("trees/index.md", &index),
            ("trees/birch/index.md", &plain("Birch")),
            ("trees/birch/leaf.md", &plain("Birch leaf")),
            ("trees/cedar.md", "---\ntitle: Cedar\ndate: 2026-05-01\n---\n\nBody.\n"),
            ("trees/fir.md", &plain("Fir")),
            ("trees/gallery/index.md", &plain("Gallery")),
        ]);
        assert_eq!(order_of(&site, &["birch", "cedar", "fir", "gallery"]).0, ["birch", "cedar", "fir", "gallery"], "`children_style: {style}`");
    }
}

/// A folder whose only folder inside is left out of listings has nothing to
/// list either: it lists as a page.
#[test]
fn a_folder_whose_only_folder_inside_is_unlisted_lists_as_a_page() {
    let plain = |title: &str| format!("---\ntitle: {title}\n---\n\nBody.\n");
    for style in ["list", "grid"] {
        let index = format!("---\ntitle: Trees\nchildren_style: {style}\n---\n\nTrees.\n");
        let site = Site::new(&[
            ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
            ("trees/index.md", &index),
            ("trees/birch/index.md", &plain("Birch")),
            ("trees/birch/leaf.md", &plain("Birch leaf")),
            ("trees/cedar.md", "---\ntitle: Cedar\ndate: 2026-05-01\n---\n\nBody.\n"),
            ("trees/fir.md", &plain("Fir")),
            ("trees/gallery/index.md", &plain("Gallery")),
            ("trees/gallery/sketches/index.md", "---\ntitle: Sketches\nlisted: false\n---\n\nBody.\n"),
        ]);
        assert_eq!(order_of(&site, &["birch", "cedar", "fir", "gallery"]).0, ["birch", "cedar", "fir", "gallery"], "`children_style: {style}`");
    }
}
