//! A term index draws each term as a folder card: the picture of the first page
//! carrying the term (in the term page's order) and how many pages carry it.

use std::fs;

#[path = "support/site.rs"]
mod site;

fn picture(site: &site::Site, rel: &str, shade: u8) {
    let path = site.root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([shade, (x % 200) as u8, (y % 200) as u8]))
        .save(path)
        .unwrap();
}

/// Writes notes as `(stem, borough, extra front matter, picture name)`; the
/// note's day is its position in the list. A picture name is a file under
/// `notes/Photos/`, so renaming it changes the note's cover.
fn write_notes(site: &site::Site, notes: &[(&str, &str, &str, Option<&str>)]) {
    for (i, (stem, borough, extra, pic)) in notes.iter().enumerate() {
        let body = match pic {
            Some(name) => {
                let shade = name.bytes().map(u32::from).sum::<u32>() as u8;
                picture(site, &format!("notes/Photos/{name}.jpg"), shade);
                format!("![{name}](Photos/{name}.jpg)\n\nText {stem}.\n")
            }
            None => format!("Text {stem}.\n"),
        };
        site.write(
            &format!("notes/{stem}.md"),
            &format!("---\ntitle: Note {stem}\nborough: [{borough}]\ndate: 2026-01-0{}\n{extra}---\n\n{body}", i + 1),
        );
    }
}

fn config(site: &site::Site) {
    site.write(
        ".moss/config.toml",
        "schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.boroughs]\nfields = [\"borough\"]\nnav = true\n",
    );
    site.write("index.md", "---\ntitle: Home\n---\n\nHello.\n");
}

fn borough_site(notes: &[(&str, &str, &str, Option<&str>)]) -> site::Site {
    let site = site::Site::new(&[]);
    config(&site);
    write_notes(&site, notes);
    site.rebuild();
    site
}

/// `(href, cover src, title)` for each card of the page's grid.
fn cards(html: &str) -> Vec<(String, Option<String>, String)> {
    let doc = scraper::Html::parse_document(html);
    let card = scraper::Selector::parse("a.moss-card").unwrap();
    let img = scraper::Selector::parse(".moss-card-cover img").unwrap();
    let title = scraper::Selector::parse(".moss-card-title").unwrap();
    doc.select(&card)
        .map(|c| {
            let href = c.value().attr("href").unwrap_or("").to_string();
            let src = c.select(&img).next().and_then(|i| i.value().attr("src")).map(str::to_string);
            let text = c.select(&title).next().map(|t| t.text().collect::<String>()).unwrap_or_default();
            (href, src, text)
        })
        .collect()
}

fn card<'a>(cards: &'a [(String, Option<String>, String)], name: &str) -> &'a (String, Option<String>, String) {
    cards.iter().find(|(_, _, text)| text == name).unwrap_or_else(|| panic!("no card for {name}: {cards:?}"))
}

/// The card's picture is whatever the term page shows first.
fn assert_cover_is_the_term_pages_first(site: &site::Site, index: &[(String, Option<String>, String)], name: &str) {
    let term_page = cards(&site.html(&format!("boroughs/{}/index.html", name.to_lowercase())));
    let first = term_page.first().and_then(|(_, src, _)| src.clone());
    assert!(first.is_some(), "{name} term page shows a picture first");
    assert_eq!(card(index, name).1, first, "{name} card takes the term page's first picture");
}

#[test]
fn each_term_is_one_card_with_the_first_pages_picture_and_the_count_of_its_pages() {
    let site = borough_site(&[
        ("a", "Bronx", "", Some("one")),
        ("b", "Brooklyn", "", Some("two")),
        ("c", "Bronx", "", Some("three")),
        ("d", "Queens", "", Some("four")),
        ("e", "Brooklyn", "", Some("five")),
        ("f", "Queens", "", Some("six")),
        ("g", "Bronx", "draft: true\n", Some("seven")),
        ("h", "Bronx", "listed: false\n", Some("eight")),
    ]);
    let html = site.html("boroughs/index.html");
    let index = cards(&html);
    assert_eq!(index.len(), 3, "one card per term: {index:?}");
    for name in ["Bronx", "Brooklyn", "Queens"] {
        assert_cover_is_the_term_pages_first(&site, &index, name);
    }
    // The draft and the unlisted page are not counted; Bronx has two.
    assert_eq!(card(&index, "Bronx").0, "/boroughs/bronx/");
    assert!(html.contains("2 articles"), "Bronx card shows its count: {html}");
    assert_eq!(html.matches("articles").count(), 3, "every card shows its count: {html}");
}

#[test]
fn a_term_none_of_whose_pages_has_a_picture_is_a_card_without_one() {
    let site = borough_site(&[("a", "Bronx", "", Some("one")), ("b", "Queens", "", None), ("c", "Queens", "", None)]);
    let html = site.html("boroughs/index.html");
    let index = cards(&html);
    assert_eq!(card(&index, "Queens").1, None, "no picture for Queens: {index:?}");
    assert!(card(&index, "Bronx").1.is_some(), "Bronx keeps its picture");
    assert!(html.contains("2 articles"), "Queens still counts its pages: {html}");
}

#[test]
fn a_cover_follows_the_term_pages_date_order_not_weight() {
    let site = borough_site(&[
        ("a", "Bronx", "weight: 9\n", Some("one")),
        ("b", "Bronx", "weight: 1\n", Some("two")),
        ("c", "Bronx", "", Some("three")),
    ]);
    let index = cards(&site.html("boroughs/index.html"));
    assert_cover_is_the_term_pages_first(&site, &index, "Bronx");
}

#[test]
fn a_claimed_term_keeps_its_page_card_and_gains_the_count() {
    let site = site::Site::new(&[]);
    config(&site);
    site.write("bronx.md", "---\ntitle: Bronx\nborough_page: true\n---\n\nThe borough.\n\n![Own](Photos/own.jpg)\n");
    picture(&site, "Photos/own.jpg", 240);
    write_notes(&site, &[("a", "Bronx", "", Some("one")), ("b", "Queens", "", Some("two")), ("c", "Bronx", "", Some("three"))]);
    site.rebuild();
    let html = site.html("boroughs/index.html");
    let index = cards(&html);
    let bronx = card(&index, "Bronx");
    assert_eq!(bronx.0, "/bronx/", "the card is the claiming page's own");
    assert!(bronx.1.as_deref().is_some_and(|src| src.contains("own")), "the claiming page keeps its own picture: {bronx:?}");
    assert!(html.contains("2 articles"), "the claimed card gains the count: {html}");
    assert!(!html.contains("moss-child-section-divider"), "one group, no divider: {html}");
}

#[test]
fn an_incremental_rebuild_moves_a_card_when_a_member_changes() {
    let before = [("a", "Bronx", "", Some("first")), ("b", "Queens", "", Some("second"))];
    let after = [("a", "Bronx", "", Some("third")), ("b", "Bronx", "", Some("second"))];
    let site = borough_site(&before);
    let stale = cards(&site.html("boroughs/index.html"));
    write_notes(&site, &after);
    site.rebuild();
    let fresh = borough_site(&after);
    let rebuilt = cards(&site.html("boroughs/index.html"));
    assert_eq!(rebuilt, cards(&fresh.html("boroughs/index.html")), "the rebuild matches a fresh build");
    assert_ne!(card(&rebuilt, "Bronx").1, card(&stale, "Bronx").1, "the picture moved");
    assert_eq!(rebuilt.len(), 1, "Queens lost its only page: {rebuilt:?}");
}

/// A card gains a count and a cover, never a place in the order: the cards of
/// a term index read alphabetically by label in one group, whatever mix of
/// claimed and unclaimed, pictured and bare terms the index holds.
fn assert_alphabetical_in_one_group(site: &site::Site, index: &str, expect: &[&str]) {
    let html = site.html(index);
    let titles: Vec<String> = cards(&html).into_iter().map(|(_, _, title)| title).collect();
    assert_eq!(titles, expect, "{index} reads alphabetically: {html}");
    assert!(!html.contains("moss-child-section-divider"), "{index} is one group: {html}");
}

#[test]
fn cards_keep_a_term_indexs_alphabetical_order_whatever_the_mix_of_terms() {
    let site = site::Site::new(&[]);
    config(&site);
    // Claimed with its own picture, claimed without one, and claimed with no
    // page carrying the term.
    site.write("bronx.md", "---\ntitle: Bronx\nborough_page: true\n---\n\n![Own](Photos/own.jpg)\n");
    picture(&site, "Photos/own.jpg", 240);
    site.write("brooklyn.md", "---\ntitle: Brooklyn\nborough_page: true\n---\n\nThe borough.\n");
    site.write("harlem.md", "---\ntitle: Harlem\nborough_page: true\n---\n\nThe district.\n");
    write_notes(
        &site,
        &[
            ("a", "Astoria", "", Some("one")),
            ("b", "Bronx", "", Some("two")),
            ("c", "Brooklyn", "", Some("three")),
            ("d", "Chelsea", "", None),
            ("e", "Queens", "", Some("four")),
        ],
    );
    site.write("de/index.md", "---\ntitle: Start\n---\n\nHallo.\n");
    site.write("de/mitte.md", "---\ntitle: Mitte\nborough_page: true\n---\n\nText.\n");
    site.write("de/n1.md", "---\ntitle: Neukoelln\nborough: [Neukoelln]\ndate: 2026-02-01\n---\n\nText.\n");
    site.write("de/n2.md", "---\ntitle: Dritte\nborough: [Pankow]\ndate: 2026-02-02\n---\n\nText.\n");
    site.rebuild();
    assert_alphabetical_in_one_group(&site, "boroughs/index.html", &["Astoria", "Bronx", "Brooklyn", "Chelsea", "Harlem", "Queens"]);
    assert_alphabetical_in_one_group(&site, "de/boroughs/index.html", &["Mitte", "Neukoelln", "Pankow"]);
}

/// A borough site whose kind declares `count_line` (a TOML line under `[terms.boroughs]`).
fn borough_site_with_count(count_line: &str) -> site::Site {
    let site = site::Site::new(&[]);
    site.write(
        ".moss/config.toml",
        &format!("schema_version = 6\n\n[site]\nlang = \"en\"\n\n[terms.boroughs]\nfields = [\"borough\"]\nnav = true\n{count_line}"),
    );
    site.write("index.md", "---\ntitle: Home\n---\n\nHello.\n");
    write_notes(
        &site,
        &[("a", "Manhattan", "", None), ("b", "Manhattan", "", None), ("c", "Manhattan", "", None), ("d", "Brooklyn", "", None)],
    );
    site.rebuild();
    site
}

/// All the text of the index card whose title is `name`, count included.
fn card_text(html: &str, name: &str) -> String {
    let doc = scraper::Html::parse_document(html);
    let card = scraper::Selector::parse("a.moss-card").unwrap();
    doc.select(&card)
        .map(|c| c.text().collect::<String>())
        .find(|text| text.contains(name))
        .unwrap_or_else(|| panic!("no card for {name}"))
}

#[test]
fn a_term_kind_count_sets_the_cards_wording_singular_and_plural() {
    for count_line in [
        "count = \"{n} photograph|{n} photographs\"\n",
        "count = { en = \"{n} photograph|{n} photographs\" }\n",
    ] {
        let site = borough_site_with_count(count_line);
        let index = site.html("boroughs/index.html");
        let many = card_text(&index, "Manhattan");
        assert!(many.contains("3 photographs"), "{count_line}: {many}");
        let one = card_text(&index, "Brooklyn");
        assert!(one.contains("1 photograph") && !one.contains("photographs"), "{count_line}: {one}");
    }
}

#[test]
fn a_term_kind_count_of_false_hides_the_count() {
    let site = borough_site_with_count("count = false\n");
    let index = site.html("boroughs/index.html");
    for name in ["Manhattan", "Brooklyn"] {
        let text = card_text(&index, name);
        assert!(!text.contains("article") && !text.contains("photograph"), "{text}");
    }
}

#[test]
fn a_term_kind_without_a_count_keeps_the_default_wording() {
    let site = borough_site_with_count("");
    let index = site.html("boroughs/index.html");
    assert!(card_text(&index, "Manhattan").contains("3 articles"));
    assert!(card_text(&index, "Brooklyn").contains("1 article"));
}

#[test]
fn a_term_kind_count_is_worded_per_language_and_falls_back_to_english() {
    let site = borough_site_with_count(
        "count = { en = \"{n} photograph|{n} photographs\", de = \"{n} Foto|{n} Fotos\" }\n",
    );
    site.write("de/notes/eins.md", "---\ntitle: Eins\nborough: [Manhattan]\ndate: 2026-01-01\n---\n\nText.\n");
    site.write("fr/notes/un.md", "---\ntitle: Un\nborough: [Manhattan]\ndate: 2026-01-01\n---\n\nTexte.\n");
    site.write("fr/notes/deux.md", "---\ntitle: Deux\nborough: [Manhattan]\ndate: 2026-01-02\n---\n\nTexte.\n");
    site.rebuild();
    let german = card_text(&site.html("de/boroughs/index.html"), "Manhattan");
    assert!(german.contains("1 Foto") && !german.contains("Fotos"), "{german}");
    let french = card_text(&site.html("fr/boroughs/index.html"), "Manhattan");
    assert!(french.contains("2 photographs"), "{french}");
    let english = card_text(&site.html("boroughs/index.html"), "Manhattan");
    assert!(english.contains("3 photographs"), "{english}");
}
