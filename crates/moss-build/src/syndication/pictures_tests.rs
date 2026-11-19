use super::*;

fn html(s: &str) -> Block { Block::Html { html: s.into() } }
fn picture(url: &str, alt: &str) -> Block { Block::Picture { picture: Picture { url: url.into(), alt: alt.into() } } }

#[test]
fn an_article_is_split_at_each_picture_in_reading_order() {
    let got = blocks(
        "<p>One.</p><figure><img src=\"https://example.org/a.png\" alt=\"A\"><figcaption>Cap</figcaption></figure><p>Two.</p><p><img src=\"https://example.org/b.png\" alt=\"B\"></p><p>Three.</p>",
        "Title",
    );
    assert_eq!(got, vec![
        html("<p>One.</p>"),
        picture("https://example.org/a.png", "A"),
        html("<figure><figcaption>Cap</figcaption></figure><p>Two.</p>"),
        picture("https://example.org/b.png", "B"),
        html("<p>Three.</p>"),
    ]);
}

#[test]
fn a_picture_inside_a_sentence_follows_its_paragraph() {
    let got = blocks("<p>Before <img src=\"https://example.org/a.png\" alt=\"A\"> after.</p>", "T");
    assert_eq!(got, vec![html("<p>Before  after.</p>"), picture("https://example.org/a.png", "A")]);
}

#[test]
fn a_formula_stays_inside_its_text() {
    let src = "https://example.org/_moss/math/abc.png";
    let got = blocks(&format!("<p>So <img src=\"{src}\" alt=\"x^2\"> holds.</p>"), "T");
    assert_eq!(got, vec![html(&format!("<p>So <img src=\"{src}\" alt=\"x^2\"> holds.</p>"))]);
}

#[test]
fn a_leading_heading_that_repeats_the_title_goes_and_any_other_stays() {
    assert_eq!(blocks("<h1>Autumn</h1><p>Leaves.</p>", " Autumn "), vec![html("<p>Leaves.</p>")]);
    assert_eq!(blocks("<h1>Spring</h1><p>Leaves.</p>", "Autumn"), vec![html("<h1>Spring</h1><p>Leaves.</p>")]);
    assert_eq!(blocks("<p>Leaves.</p><h1>Autumn</h1>", "Autumn"), vec![html("<p>Leaves.</p><h1>Autumn</h1>")]);
}

#[test]
fn what_is_not_the_articles_text_is_removed() {
    let got = blocks("<meta charset=\"utf-8\"><link rel=\"stylesheet\" href=\"/s.css\"><style>p{}</style><script>alert(1)</script><base href=\"/\"><p>Text.</p>", "T");
    assert_eq!(got, vec![html("<p>Text.</p>")]);
}

#[test]
fn notes_get_a_heading_and_lose_their_back_links() {
    let got = blocks(
        concat!(
            r##"<p>Body<sup class="moss-footnote-ref" id="fnref-1"><a href="https://example.org/a/#fn-1" role="doc-noteref">1</a></sup>.</p>"##,
            r##"<section class="moss-footnotes" role="doc-endnotes"><ol><li id="fn-1"><p>Note <a class="moss-footnote-backref" href="https://example.org/a/#fnref-1">↩</a></p></li></ol></section>"##,
        ),
        "T",
    );
    let [Block::Html { html }] = got.as_slice() else { panic!("{got:?}") };
    assert!(html.contains("</p><h2>Notes</h2><section class=\"moss-footnotes\""), "{html}");
    assert!(!html.contains("moss-footnote-backref") && !html.contains('↩'), "{html}");
    assert!(html.contains("#fn-1"), "the reference still leads to its note: {html}");
}

#[test]
fn an_embed_becomes_a_link_to_its_address() {
    let got = blocks(
        concat!(
            "<iframe src=\"https://video.example.net/e/1\" title=\"A talk\"></iframe>",
            "<p>See <iframe src=\"https://example.org/jupyter/?path=n.ipynb\"></iframe> here.</p>",
            "<video controls><source src=\"https://example.org/v.mp4\" type=\"video/mp4\"></video>",
            "<audio></audio>",
        ),
        "T",
    );
    assert_eq!(got, vec![html(concat!(
        "<p><a href=\"https://video.example.net/e/1\">A talk</a></p>",
        "<p>See <a href=\"https://example.org/jupyter/?path=n.ipynb\">https://example.org/jupyter/?path=n.ipynb</a> here.</p>",
        "<p><a href=\"https://example.org/v.mp4\">https://example.org/v.mp4</a></p>",
    ))]);
}

#[test]
fn chinese_and_right_to_left_text_pass_through() {
    let got = blocks("<h2>第一章</h2><p>中文段落。</p><p dir=\"rtl\">مرحبا بالعالم</p>", "T");
    assert_eq!(got, vec![html("<h2>第一章</h2><p>中文段落。</p><p dir=\"rtl\">مرحبا بالعالم</p>")]);
}

#[test]
fn blocks_reach_the_plugin_as_html_or_picture_objects() {
    let json = serde_json::to_value(vec![html("<p>a</p>"), picture("https://example.org/a.png", "A")]).unwrap();
    assert_eq!(json, serde_json::json!([{ "html": "<p>a</p>" }, { "picture": { "url": "https://example.org/a.png", "alt": "A" } }]));
    let back: Vec<Block> = serde_json::from_value(json).unwrap();
    assert_eq!(back[1], picture("https://example.org/a.png", "A"));
}
