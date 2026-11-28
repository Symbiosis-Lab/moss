use super::*;

fn resolved(markdown: &str) -> Document {
    let mut doc = ast::parse(markdown);
    ast::visit_urls_mut(&mut doc, |url| {
        if let Url::Unresolved(s) = url {
            *url = Url::Resolved(ast::ResolvedUrl { href: s.clone(), kind: ast::UrlKind::Asset });
        }
    });
    doc
}

#[test]
fn a_page_without_a_hero_takes_its_first_raster_picture() {
    let doc = resolved("Text.\n\n![a](/notes/a%20b.jpg)\n\n![b](/notes/drawing.svg)\n\n![c](/notes/c.png)\n");
    let pictures = PagePictures::of(&doc, None);
    assert_eq!(pictures.first.as_deref(), Some("/notes/a%20b.jpg"));
    assert_eq!(pictures.body.iter().map(|p| p.href.as_str()).collect::<Vec<_>>(), ["/notes/a%20b.jpg", "/notes/c.png"]);
    assert_eq!(pictures.card_cover(None).as_deref(), Some("notes/a b.jpg"), "a card path is decoded and site-relative");
}

#[test]
fn the_hero_s_picture_wins_and_a_remote_one_is_passed_over() {
    let doc = resolved("![a](/notes/a.jpg)\n");
    assert_eq!(PagePictures::of(&doc, Some("/notes/hero.webp".into())).card_cover(None).as_deref(), Some("notes/hero.webp"));
    assert_eq!(PagePictures::of(&doc, Some("https://example.com/x.jpg".into())).card_cover(None).as_deref(), Some("notes/a.jpg"));
    assert_eq!(PagePictures::of(&doc, None).card_cover(Some(" false")), None, "`cover: false` opts out");
}

#[test]
fn a_strip_follows_the_cover_without_repeating_it() {
    let body = [BodyPicture { href: "/a.jpg".into(), alt: String::new() }, BodyPicture { href: "/b.jpg".into(), alt: String::new() }];
    assert_eq!(card_pictures_after_cover(Some("a.jpg"), None, &body, 3), ["b.jpg"]);
    assert!(card_pictures_after_cover(None, None, &body, 3).is_empty(), "no cover, no strip");
}
