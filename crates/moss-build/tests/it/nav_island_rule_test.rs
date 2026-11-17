//! A `floating_nav` site gives a page the floating nav island only when its
//! sections hold enough text to be worth listing; a page of two headings over
//! a card grid and a few links gets none, nor does a listing moss generates. A site with no header keeps its
//! controls on such a page, without the sections button.

use crate::site;

use site::Site;

fn words(n: usize) -> String {
    vec!["word"; n].join(" ")
}

fn pages() -> Vec<(&'static str, String)> {
    vec![
        ("index.md", "---\ntitle: Harbour\n---\n\nHome text.\n".into()),
        ("works/index.md", format!("---\ntitle: Works\n---\n\n{}\n\n## All\n\n![[./]]\n\n## Elsewhere\n\n- [One](https://example.com/one)\n- [Two](https://example.com/two)\n", words(200))),
        ("works/a.md", format!("---\ntitle: A long piece\n---\n\n## First\n\n{}\n\n## Second\n\n{}\n", words(120), words(120))),
        ("works/b.md", "---\ntitle: B\n---\n\nShort.\n".into()),
        ("works/notes/c.md", "---\ntitle: C\n---\n\nShort.\n".into()),
    ]
}

fn site(config: &str) -> Site {
    let pages = pages();
    let refs: Vec<(&str, &str)> = pages.iter().map(|(p, b)| (*p, b.as_str())).collect();
    Site::with_config(config, &refs)
}

#[test]
fn a_page_of_headings_over_cards_has_no_island_and_a_long_article_keeps_it() {
    let site = site("floating_nav = true\n");
    let hub = site.html("works/index.html");
    assert!(!hub.contains("moss-nav-island"), "{hub}");
    let article = site.html("works/a/index.html");
    assert!(article.contains(r#"class="moss-nav-island""#), "{article}");
    assert!(article.contains(r#"class="moss-nav-island-sections""#), "{article}");
    // A listing moss generates has no sections of its own.
    let listing = site.html("works/notes/index.html");
    assert!(!listing.contains(r#"class="moss-nav-island""#), "{listing}");
}

#[test]
fn a_site_with_no_header_keeps_the_controls_without_the_sections_button() {
    let site = site("header = \"none\"\n");
    let hub = site.html("works/index.html");
    assert!(hub.contains(r#"data-header="none""#), "{hub}");
    assert!(hub.contains(r#"class="nav-icons""#), "{hub}");
    assert!(!hub.contains(r#"class="moss-nav-island-sections""#), "{hub}");
    let article = site.html("works/a/index.html");
    assert!(article.contains(r#"class="moss-nav-island-sections""#), "{article}");
}

#[test]
fn the_members_version_of_a_page_gets_the_island_its_whole_body_earns() {
    let body = format!("---\ntitle: Members piece\nmembers: subscribers\n---\n\nShort preview.\n<!--members-only-->\n\n## First\n\n{}\n\n## Second\n\n{}\n", words(120), words(120));
    let site = Site::with_config("floating_nav = true\n", &[("index.md", "---\ntitle: Harbour\n---\n\nHome.\n"), ("piece.md", body.as_str())]);
    let public = site.html("piece/index.html");
    assert!(!public.contains(r#"class="moss-nav-island""#), "{public}");
    let whole = site.html("_gated/piece/index.html");
    assert!(whole.contains(r#"class="moss-nav-island-sections""#), "{whole}");
}
