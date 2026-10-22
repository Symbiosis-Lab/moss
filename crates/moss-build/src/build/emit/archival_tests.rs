use super::*;

fn edition(site_url: Option<&str>, author: Option<&str>) -> Edition {
    Edition {
        date: "2026-10-21".into(),
        id: "a1b2c3d".into(),
        site_url: site_url.map(str::to_string),
        license: None,
        style_link: "<link rel=\"stylesheet\" href=\"/_moss/css/archival.x.css\">".into(),
        site_title: "Garden Notes".into(),
        pages: ["essays/tea/index.html", "index.html"]
            .into_iter()
            .map(|path| (path.to_string(), CitedPage { title: "Tea &amp; Ferns".into(), lang: Language::En, author: author.map(str::to_string) }))
            .collect(),
    }
}

const PAGE: &str = "<!DOCTYPE html>\n<html lang=\"en\"><head></head><body><!-- moss:footer --><footer class=\"container\">Links</footer><!-- /moss:footer --></body></html>";

#[test]
fn the_date_is_the_epoch_when_one_is_given_and_the_build_day_otherwise() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap().and_hms_opt(23, 30, 0).unwrap();
    assert_eq!(build_date(None, now), "2026-10-21");
    assert_eq!(build_date(Some("1700000000"), now), "2023-11-14");
    assert_eq!(build_date(Some("not a number"), now), "2026-10-21");
}

#[test]
fn the_citation_names_author_page_site_edition_and_absolute_address() {
    let rendered = edition(Some("https://garden.example"), Some("Ada Example")).render(PAGE, "essays/tea/index.html").unwrap();
    assert!(
        rendered.block.contains("Ada Example. \u{201c}Tea &amp; Ferns.\u{201d} Garden Notes. Edition 2026-10-21 (a1b2c3d). https://garden.example/essays/tea/"),
        "{}",
        rendered.block
    );
    assert!(rendered.block.contains("Edition 2026-10-21 · a1b2c3d"));
    assert!(rendered.head.contains(r#"<meta name="moss:edition" content="2026-10-21 a1b2c3d">"#));
}

#[test]
fn without_a_site_url_the_citation_ends_at_the_edition() {
    let rendered = edition(None, None).render(PAGE, "essays/tea/index.html").unwrap();
    assert!(rendered.block.contains("(a1b2c3d).</p>"), "{}", rendered.block);
}

#[test]
fn a_citation_leads_with_the_pages_authors_then_the_sites() {
    let page = |author: &[&str], lang| ParsedDocument {
        title: "Tea".into(),
        lang,
        author: author.iter().map(|name| name.to_string()).collect(),
        ..Default::default()
    };
    assert_eq!(CitedPage::of(&page(&["Ada", "Bo"], Language::En), Some("Site")).author.as_deref(), Some("Ada, Bo"));
    assert_eq!(CitedPage::of(&page(&["林", "王"], Language::ZhHans), None).author.as_deref(), Some("林、王"));
    assert_eq!(CitedPage::of(&page(&[], Language::En), Some("Site & Co")).author.as_deref(), Some("Site &amp; Co"));
    assert_eq!(CitedPage::of(&page(&[], Language::En), None).author, None);
}

#[test]
fn a_page_without_moss_chrome_is_left_alone() {
    let stub = "<html><head><meta http-equiv=\"refresh\" content=\"0; url=/x/\"></head><body></body></html>";
    assert!(edition(None, None).render(stub, "old/index.html").is_none());
}

#[test]
fn a_page_the_build_has_no_document_for_is_left_alone() {
    assert!(edition(None, None).render(PAGE, "somewhere/else/index.html").is_none());
}

#[test]
fn a_page_takes_its_language_from_the_table() {
    let mut german = edition(None, None);
    german.pages.insert("de/index.html".into(), CitedPage { title: "Start".into(), lang: Language::De, author: None });
    let rendered = german.render(PAGE, "de/index.html").unwrap();
    assert!(rendered.block.contains("Diese Seite zitieren") && rendered.block.contains("Ausgabe 2026-10-21"), "{}", rendered.block);
}

#[test]
fn the_licence_is_stated_on_the_page_only_when_the_site_declares_one() {
    assert!(!edition(None, None).render(PAGE, "index.html").unwrap().block.contains("moss-edition-rights"));
    let mut licensed = edition(None, None);
    licensed.license = Some("No known copyright &amp; free to use".into());
    let block = licensed.render(PAGE, "index.html").unwrap().block;
    assert!(block.contains("<p class=\"moss-edition-rights\">No known copyright &amp; free to use</p>\n</div>"), "{block}");
}

#[test]
fn the_block_lands_at_the_end_of_a_closing_picture_past_a_nested_section() {
    let page = "<!DOCTYPE html>\n<html><head></head><body><footer><section class=\"moss-hero moss-hero-closing\"><section class=\"moss-hero-content\"><p>Words</p></section><p>Credit</p></section><div class=\"moss-colophon\"></div></footer><!-- /moss:footer --></body></html>";
    let placed = edition(None, None).place(page, "index.html").unwrap();
    let block = placed.find("<div class=\"moss-edition\">").unwrap_or_else(|| panic!("{placed}"));
    assert_eq!(placed.matches("<div class=\"moss-edition\">").count(), 1, "{placed}");
    assert!(placed.find("Credit").unwrap() < block && block < placed.find("</section><div class=\"moss-colophon\">").unwrap(), "{placed}");
}

#[test]
fn without_a_closing_picture_the_block_follows_the_footer_end_marker() {
    let placed = edition(None, None).place(PAGE, "index.html").unwrap();
    assert!(placed.contains("Links<div class=\"moss-edition\">"), "{placed}");
    assert!(placed.contains("</head>") && placed.contains("moss:edition"), "{placed}");
}
