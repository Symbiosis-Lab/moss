//! A term kind's root embeds as its index: `![[/districts/|style:grid]]` draws
//! the same cards the generated `/districts/` page draws.

#[path = "support/site.rs"]
mod site;

use site::Site;

const DISTRICTS: &str = "\n[terms.districts]\nfields = [\"district\"]\ncount = \"{n} photograph|{n} photographs\"\n";

fn photo(stem: &str, district: &str, day: u8) -> (String, String) {
    (
        format!("photographs/{stem}.md"),
        format!("---\ntitle: Photo {stem}\ndistrict: [{district}]\ndate: 2026-01-0{day}\n---\n\nText {stem}.\n"),
    )
}

fn harbour(home: &str, extra: &[(&str, &str)]) -> Site {
    let photos = [
        photo("quay", "Dockside", 1),
        photo("crane", "Dockside", 2),
        photo("mast", "Old Harbour", 3),
        photo("net", "Saltmarsh", 4),
    ];
    let mut files: Vec<(&str, &str)> = vec![("index.md", home), ("de/index.md", "---\ntitle: Start\n---\n\nWillkommen.\n")];
    files.extend(photos.iter().map(|(p, b)| (p.as_str(), b.as_str())));
    files.extend_from_slice(extra);
    Site::with_config(DISTRICTS, &files)
}

fn cards(html: &str) -> Vec<(String, String)> {
    let doc = scraper::Html::parse_document(html);
    let card = scraper::Selector::parse("a.moss-card").unwrap();
    let title = scraper::Selector::parse(".moss-card-title").unwrap();
    doc.select(&card)
        .map(|c| {
            let href = c.value().attr("href").unwrap_or("").to_string();
            (href, c.select(&title).next().map(|t| t.text().collect::<String>()).unwrap_or_default())
        })
        .collect()
}

#[test]
fn the_root_embed_draws_the_index_cards_with_their_counts() {
    let site = harbour(
        "---\ntitle: Home\nchildren: false\n---\n\nIntro.\n\n![[/districts/|style:grid]]\n",
        &[
            ("photographs/mast.md", "---\ntitle: Photo mast\ndistrict: [Old Harbour]\ndate: 2026-01-03\ncover: ../images/mast.svg\n---\n\nText mast.\n"),
            ("images/mast.svg", "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"30\"><rect width=\"40\" height=\"30\" fill=\"#c00\"/></svg>"),
        ],
    );
    let home = site.html("index.html");
    assert!(home.contains("mast.svg"), "the member's cover is the card's: {home}");
    let embedded = cards(&home);
    let hrefs: Vec<&str> = embedded.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(hrefs, ["/districts/dockside/", "/districts/old-harbour/", "/districts/saltmarsh/"], "{home}");
    assert!(home.contains("2 photographs") && home.contains("1 photograph<"), "count wording of the kind: {home}");
    assert!(!site.logged.iter().any(|m| m.contains("unresolved folder embed")), "{:?}", site.logged);
    // The same cards, in the same order, as the generated index page.
    assert_eq!(card_html(&home), card_html(&site.html("districts/index.html")));
}

/// Each card as its elements in document order, attributes sorted: scraper keeps
/// them in a hash map, so its serialized order varies between parses.
fn card_html(html: &str) -> Vec<Vec<String>> {
    let doc = scraper::Html::parse_document(html);
    let card = scraper::Selector::parse("a.moss-card").unwrap();
    doc.select(&card)
        .map(|c| {
            c.descendants()
                .map(|node| match node.value() {
                    scraper::Node::Element(el) => {
                        let mut attrs: Vec<String> = el.attrs().map(|(k, v)| format!("{k}={v}")).collect();
                        attrs.sort();
                        format!("<{} {}>", el.name(), attrs.join(" "))
                    }
                    scraper::Node::Text(text) => text.to_string(),
                    _ => String::new(),
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_hero_draws_from_the_root_with_no_embed_on_the_site() {
    let home = "---\ntitle: Home\nchildren: false\n---\n\n:::hero {from=\"districts/\"}\nFallback words.\n:::\n\nIntro.\n";
    let site = harbour(home, &[]);
    assert!(!site.logged.iter().any(|m| m.contains("unresolved hero listing")), "{:?}", site.logged);
    let html = site.html("index.html");
    assert!(html.contains("href=\"/districts/"), "{html}");
}

#[test]
fn the_root_embed_takes_limit() {
    let site = harbour("---\ntitle: Home\nchildren: false\n---\n\n![[/districts/|style:grid,limit:2]]\n", &[]);
    assert_eq!(cards(&site.html("index.html")).len(), 2);
}

#[test]
fn an_edition_root_embeds_that_editions_terms() {
    let de_home = "---\ntitle: Start\nchildren: false\n---\n\n![[/de/districts/|style:grid]]\n";
    let site = harbour(
        "---\ntitle: Home\nchildren: false\n---\n\nText.\n",
        &[
            ("de/index.md", de_home),
            ("de/Hafen.md", "---\ntitle: Hafen\ndistrict: [Kai]\ndate: 2026-01-05\n---\n\nText.\n"),
        ],
    );
    let hrefs: Vec<String> = cards(&site.html("de/index.html")).into_iter().map(|(h, _)| h).collect();
    assert_eq!(hrefs, ["/de/districts/kai/"], "{hrefs:?}");
}

#[test]
fn a_kind_without_terms_still_warns() {
    let site = Site::with_config(
        DISTRICTS,
        &[("index.md", "---\ntitle: Home\nchildren: false\n---\n\n![[/districts/|style:grid]]\n")],
    );
    assert!(site.logged.iter().any(|m| m.contains("unresolved folder embed")), "{:?}", site.logged);
}

#[test]
fn a_claimed_term_page_in_a_folder_embed_shows_its_count() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Home\nchildren: false\n---\n\n![[/people/|style:grid]]\n"),
        ("people/alice.md", "---\ntitle: Alice\nauthor_page: true\n---\n\nBio.\n"),
        ("notes/one.md", "---\ntitle: One\nauthor: Alice\ndate: 2026-01-01\n---\n\nText.\n"),
        ("notes/two.md", "---\ntitle: Two\nauthor: Alice\ndate: 2026-01-02\n---\n\nText.\n"),
    ]);
    let meta = |html: &str| {
        let doc = scraper::Html::parse_document(html);
        let sel = scraper::Selector::parse("a.moss-card[href='/people/alice/'] .moss-card-meta").unwrap();
        doc.select(&sel).map(|m| m.text().collect::<String>()).collect::<Vec<_>>()
    };
    assert_eq!(meta(&site.html("index.html")), ["2 articles"]);
    assert_eq!(meta(&site.html("index.html")), meta(&site.html("people/index.html")));
}
