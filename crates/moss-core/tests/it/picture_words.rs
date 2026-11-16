//! Picture words after a pipe, and a linked picture with a caption line, read
//! the same on both image forms: `![alt|words](x.jpg)` and `![[x.jpg|words]]`.

use moss_core::ast::{parse, Block, Inline, Url};

fn first(md: &str) -> Block {
    parse(md).blocks.into_iter().next().expect("one block")
}

fn caption_text(caption: &Option<Vec<Inline>>) -> String {
    moss_core::ast::inlines_to_plain_text(caption.as_deref().unwrap_or_default())
}

fn figure(md: &str) -> (String, String, Option<String>, Option<String>) {
    match first(md) {
        Block::Figure { image: Inline::Image { alt, .. }, caption, img_style, link, .. } => (
            alt,
            caption_text(&caption),
            img_style,
            link.map(|u| match u {
                Url::Unresolved(s) => s,
                Url::Resolved(r) => r.href,
            }),
        ),
        other => panic!("expected a figure for {md:?}, got {other:?}"),
    }
}

#[test]
fn a_standard_image_takes_display_words_after_its_pipe() {
    let (alt, caption, style, _) = figure("![Newton|focus:60%,45%](newton.jpg)");
    assert_eq!((alt.as_str(), caption.as_str()), ("Newton", "Newton"));
    assert_eq!(style.as_deref(), Some("object-position:60% 45%"));

    let (_, caption, style, _) = figure("![Title page of the first edition|contain](title.jpg)");
    assert_eq!(caption, "Title page of the first edition");
    assert_eq!(style.as_deref(), Some("object-fit:contain"));
}

#[test]
fn a_standard_alt_with_no_pipe_stays_the_alt_even_when_it_is_a_display_word() {
    let (alt, caption, style, _) = figure("![Cover](cover.jpg)");
    assert_eq!((alt.as_str(), caption.as_str(), style), ("Cover", "Cover", None));
}

#[test]
fn a_wikilink_image_sorts_each_segment() {
    let (alt, caption, style, _) = figure("![[x.jpg|focus:20%,62%|A caption]]");
    assert_eq!((alt.as_str(), caption.as_str()), ("A caption", "A caption"));
    assert_eq!(style.as_deref(), Some("object-position:20% 62%"));
}

#[test]
fn unknown_words_stay_caption() {
    assert_eq!(figure("![[x.jpg|contian]]").1, "contian");
    let (_, caption, style, _) = figure("![A lake|contian](x.jpg)");
    assert_eq!((caption.as_str(), style), ("A lake|contian", None));
}

#[test]
fn the_alt_stays_the_alt_under_an_italic_caption() {
    let (alt, caption, style, _) = figure("![Newton|focus:60%,45%](newton.jpg)\n*__Newton__ c. 1800 · Public domain*");
    assert_eq!(alt, "Newton");
    assert_eq!(caption, "Newton c. 1800 · Public domain");
    assert_eq!(style.as_deref(), Some("object-position:60% 45%"));
}

#[test]
fn a_linked_image_with_its_caption_line_is_a_figure_with_a_link() {
    let (alt, caption, style, link) =
        figure("[![Newton|focus:60%,45%](newton.jpg)](/works/newton/)\n*__Newton__ c. 1800*");
    assert_eq!((alt.as_str(), caption.as_str()), ("Newton", "Newton c. 1800"));
    assert_eq!(style.as_deref(), Some("object-position:60% 45%"));
    assert_eq!(link.as_deref(), Some("/works/newton/"));
}

#[test]
fn both_image_forms_give_one_linked_figure() {
    let (_, caption, style, link) = figure("[![[newton.jpg|focus:60%,45%]]](/works/newton/)\n*Newton*");
    assert_eq!((caption.as_str(), style.as_deref(), link.as_deref()), ("Newton", Some("object-position:60% 45%"), Some("/works/newton/")));
}

#[test]
fn a_linked_image_without_a_caption_line_stays_a_paragraph() {
    assert!(matches!(first("[![Logo](logo.png)](https://example.org/)"), Block::Paragraph(_)));
    assert!(matches!(first("[![[logo.png]]](https://example.org/)"), Block::Paragraph(_)));
    // An italic span on the image's own line is prose, not a caption.
    assert!(matches!(first("[![Logo](logo.png)](https://example.org/) *beta*"), Block::Paragraph(_)));
}

#[test]
fn a_linked_figure_renders_the_link_around_the_image_only() {
    let mut doc = parse("[![Newton](newton.jpg)](/works/newton/)\n*Newton, c. 1800*");
    moss_core::ast::visit_urls_mut(&mut doc, |url| {
        if let Url::Unresolved(s) = url {
            *url = Url::resolved(s.clone(), moss_core::ast::UrlKind::Internal);
        }
    });
    let html = moss_core::ast::render_document(&doc, &moss_core::ast::DefaultHooks::new());
    assert!(html.starts_with(r#"<figure class="moss-image"><a href="/works/newton/""#), "got: {html}");
    assert!(html.contains("</a><figcaption>Newton, c. 1800</figcaption></figure>"), "got: {html}");
}
