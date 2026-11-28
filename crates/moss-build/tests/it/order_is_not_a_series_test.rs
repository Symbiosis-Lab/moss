//! Ordering a folder arranges its listing. Previous and next links between
//! its pages come only from `series:` on the folder's home page.

use crate::site::Site;

/// Whether the middle page of a three-page folder links to its neighbours.
/// `index` is the folder home page's frontmatter after its title; `weighted`
/// gives each page a `weight:`.
fn has_series_nav(index: &str, weighted: bool) -> bool {
    let page = |title: &str, n: u8| {
        let weight = if weighted { format!("weight: {n}\n") } else { String::new() };
        format!("---\ntitle: {title}\n{weight}---\n\nBody.\n")
    };
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\n---\n\nHi.\n"),
        ("course/index.md", &format!("---\ntitle: Course\n{index}---\n\nCourse.\n")),
        ("course/one.md", &page("One", 1)),
        ("course/two.md", &page("Two", 2)),
        ("course/three.md", &page("Three", 3)),
    ]);
    site.html("course/two/index.html").contains("moss-series-nav")
}

#[test]
fn a_typed_order_draws_no_previous_and_next() {
    assert!(!has_series_nav("sort: [one, two, three]\n", false));
}

#[test]
fn a_weight_order_draws_no_previous_and_next() {
    assert!(!has_series_nav("", true));
    assert!(!has_series_nav("sort: weight\n", true));
}

#[test]
fn series_true_draws_previous_and_next_on_an_ordered_folder() {
    assert!(has_series_nav("sort: [one, two, three]\nseries: true\n", false));
    assert!(has_series_nav("series: true\n", true));
}
