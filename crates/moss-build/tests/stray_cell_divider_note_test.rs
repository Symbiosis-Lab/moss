//! A `+++` line only divides cells inside `:::grid` and `:::buttons`; anywhere
//! else it renders as the literal text `+++`, so the build says so.

#[path = "support/site.rs"]
mod site;

use site::Site;

const HOME: (&str, &str) = ("index.md", "---\ntitle: Example Lab\n---\n\nWelcome.\n");

fn notes(site: &Site) -> Vec<&String> {
    site.logged.iter().filter(|l| l.contains("+++")).collect()
}

#[test]
fn a_divider_outside_grid_and_buttons_is_named() {
    let site = Site::new(&[HOME, ("work.md", "---\ntitle: Work\n---\n\nFirst.\n\n+++\n\nSecond.\n\nTight\n+++\nafter.\n")]);
    let found = notes(&site);
    assert_eq!(found.len(), 1, "{:?}", site.logged);
    assert!(found[0].contains("work.md") && found[0].contains(":::grid"), "{}", found[0]);
    assert!(site::problems().iter().all(|p| !p.contains("+++")), "a note is not a problem");
}

#[test]
fn dividers_inside_grid_and_buttons_and_code_are_not_named() {
    let body = "---\ntitle: Work\n---\n\n:::grid 2\nleft\n+++\nright\n:::\n\n:::buttons\n[A](/a)\n+++\n[B](/b)\n:::\n\n```\n+++\n```\n";
    let site = Site::new(&[HOME, ("work.md", body)]);
    assert!(notes(&site).is_empty(), "{:?}", site.logged);
}

#[test]
fn a_partial_embedded_by_two_pages_is_named_once_as_itself() {
    let site = Site::new(&[
        HOME,
        ("shared.md", "---\ntitle: Shared\n---\n\nOne.\n\n+++\n\nTwo.\n"),
        ("a.md", "---\ntitle: A\n---\n\n![[shared]]\n"),
        ("b.md", "---\ntitle: B\n---\n\n![[shared]]\n"),
    ]);
    let found = notes(&site);
    assert_eq!(found.len(), 1, "{:?}", site.logged);
    assert!(found[0].contains("shared.md"), "{}", found[0]);
}
