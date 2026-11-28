//! A `[[page]]` line inside `:::buttons` is a button to that page, as a
//! `[text](page.md)` line is.

use crate::site;

use site::LoggedSite;

fn button_hrefs(html: &str) -> Vec<String> {
    html.split("<a ")
        .skip(1)
        .filter(|a| a.contains("class=\"moss-btn\""))
        .filter_map(|a| a.split("href=\"").nth(1)?.split('"').next().map(str::to_string))
        .collect()
}

#[test]
fn a_wikilink_in_buttons_is_a_button_to_its_page() {
    let home = "---\ntitle: Example Lab\n---\n\n:::buttons\n[[訂閱|訂閱]]\n[[about#Our Team|Team]]\n:::\n";
    let site = LoggedSite::new(&[
        ("index.md", home),
        ("訂閱.md", "---\ntitle: 訂閱\n---\n\nForm.\n"),
        ("about.md", "---\ntitle: About\n---\n\n## Our Team\n\nUs.\n"),
    ]);
    let html = site.site.html("index.html");
    let hrefs = button_hrefs(&html);
    assert_eq!(hrefs.len(), 2, "{hrefs:?}\n{html}");
    assert!(hrefs[0].contains("訂閱") || hrefs[0].contains("%E8%A8%82"), "{hrefs:?}");
    assert!(hrefs[1].ends_with("about/#our-team"), "{hrefs:?}");
    assert!(html.contains(">訂閱</a>") && html.contains(">Team</a>"), "{html}");
    assert!(site.logged.iter().all(|l| !l.contains(":::buttons")), "{:?}", site.logged);
}
