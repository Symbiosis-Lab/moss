//! `when:past` / `when:upcoming` on a folder embed, and `children_when:` on a
//! folder's own listing, keep only the events that have (not) ended. A page
//! with no start or date counts as past.

#[path = "support/site.rs"]
mod site;

use site::Site;

fn shows(index: &str, embed_page: &str) -> Site {
    Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        ("shows/index.md", index),
        ("lab.md", embed_page),
        ("shows/soon.md", "---\ntitle: Soon Show\nstart: 2099-03-01 19:30\n---\n\nBody.\n"),
        ("shows/gone-a.md", "---\ntitle: Gone Alpha\nstart: 2001-05-05 10:00\n---\n\nBody.\n"),
        ("shows/gone-b.md", "---\ntitle: Gone Beta\nstart: 2002-05-05 10:00\n---\n\nBody.\n"),
        ("shows/note.md", "---\ntitle: Plain Note\n---\n\nBody.\n"),
    ])
}

fn listed(html: &str) -> Vec<&'static str> {
    ["Soon Show", "Gone Alpha", "Gone Beta", "Plain Note"]
        .into_iter()
        .filter(|t| html.contains(&format!(">{t}<")))
        .collect()
}

fn embed(opts: &str) -> Vec<&'static str> {
    let page = format!("---\ntitle: Lab\n---\n\n![[/shows/|style:grid,{opts}]]\n");
    listed(&shows("---\ntitle: Shows\nchildren: false\n---\n", &page).html("lab/index.html"))
}

fn own_listing(when: &str) -> Vec<&'static str> {
    let site = shows(&format!("---\ntitle: Shows\nchildren_style: grid\nchildren_when: {when}\n---\n\n"), "---\ntitle: Lab\n---\n");
    listed(&site.html("shows/index.html"))
}

#[test]
fn an_embed_lists_only_the_upcoming_events() {
    assert_eq!(embed("when:upcoming"), ["Soon Show"]);
}

#[test]
fn an_embed_lists_only_the_past_pages_and_limit_counts_off_them() {
    assert_eq!(embed("when:past").len(), 3);
    assert_eq!(embed("when:past,limit:2").len(), 2);
    assert!(!embed("when:past").contains(&"Soon Show"));
}

#[test]
fn a_folders_own_listing_takes_children_when() {
    assert_eq!(own_listing("upcoming"), ["Soon Show"]);
    assert!(!own_listing("past").contains(&"Soon Show"));
    assert_eq!(own_listing("past").len(), 3);
}

fn three_upcoming(index: &str, embed_page: &str) -> Site {
    Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\nWelcome.\n"),
        ("shows/index.md", index),
        ("lab.md", embed_page),
        ("shows/c.md", "---\ntitle: Alpha Show\nstart: 2099-09-01 19:30\n---\n\nBody.\n"),
        ("shows/a.md", "---\ntitle: Bravo Show\nstart: 2099-03-01 19:30\n---\n\nBody.\n"),
        ("shows/b.md", "---\ntitle: Charlie Show\nstart: 2099-06-01 19:30\n---\n\nBody.\n"),
    ])
}

/// The three show titles in the order the page lists them.
fn order_of(html: &str) -> Vec<&'static str> {
    let mut found: Vec<(usize, &'static str)> = ["Alpha Show", "Bravo Show", "Charlie Show"]
        .into_iter()
        .filter_map(|t| html.find(&format!(">{t}<")).map(|at| (at, t)))
        .collect();
    found.sort();
    found.into_iter().map(|(_, t)| t).collect()
}

fn embedded_order(opts: &str) -> Vec<&'static str> {
    let page = format!("---\ntitle: Lab\n---\n\n![[/shows/|style:grid,{opts}]]\n");
    order_of(&three_upcoming("---\ntitle: Shows\nchildren: false\n---\n", &page).html("lab/index.html"))
}

#[test]
fn upcoming_with_limit_one_shows_the_soonest_event() {
    assert_eq!(embedded_order("when:upcoming,limit:1"), ["Bravo Show"]);
}

#[test]
fn an_upcoming_listing_reads_soonest_first() {
    assert_eq!(embedded_order("when:upcoming"), ["Bravo Show", "Charlie Show", "Alpha Show"]);
}

#[test]
fn an_explicit_sort_wins_over_soonest_first() {
    assert_eq!(embedded_order("when:upcoming,sort:title"), ["Alpha Show", "Bravo Show", "Charlie Show"]);
}

#[test]
fn a_folders_own_upcoming_listing_reads_soonest_first() {
    let site = three_upcoming("---\ntitle: Shows\nchildren_style: grid\nchildren_when: upcoming\n---\n\n", "---\ntitle: Lab\n---\n");
    assert_eq!(order_of(&site.html("shows/index.html")), ["Bravo Show", "Charlie Show", "Alpha Show"]);
}

#[test]
fn a_folders_own_sort_wins_over_soonest_first() {
    let site = three_upcoming("---\ntitle: Shows\nsort: title\nchildren_style: grid\nchildren_when: upcoming\n---\n\n", "---\ntitle: Lab\n---\n");
    assert_eq!(order_of(&site.html("shows/index.html")), ["Alpha Show", "Bravo Show", "Charlie Show"]);
}
