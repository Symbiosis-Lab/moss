use std::path::PathBuf;

use super::*;
use crate::build::manifest::PendingManifest;
use crate::build::types::ParsedDocument;

const FLOW: &str = "flowchart LR\n  A[Write markdown] --> B[Build the site] --> C[Publish it]\n";

fn hash_of(source: &str) -> String {
    picture::picture(source, Language::En).unwrap().hash
}

fn doc_with(content: &str) -> ParsedDocument {
    ParsedDocument { content: content.to_string(), ..Default::default() }
}

fn valid_corpus() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mermaid-corpus/valid");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    files
        .into_iter()
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap()))
        .collect()
}

#[test]
fn every_corpus_picture_carries_its_colours_and_fonts_written_in() {
    for (name, source) in valid_corpus() {
        let picture = picture::picture(&source, Language::En).unwrap_or_else(|| panic!("{name}: no picture"));
        assert!(!picture.svg.contains("var("), "{name}: a var() survived");
        assert!(!picture.svg.contains("--_"), "{name}: a custom property survived");
    }
    let page = picture::palette().iter().find(|(n, _)| n == "--pele-bg").unwrap();
    assert!(page.1.starts_with('#'), "the page colour resolves to a token value: {page:?}");
}

#[test]
fn the_labels_are_drawn() {
    // Text is what a rasterizer most easily drops without a word: a family it
    // cannot find draws nothing.
    let picture = picture::picture(FLOW, Language::En).unwrap();
    let ink = |svg: &str| {
        let png = rasterize(&Picture { svg: svg.to_string(), width: picture.width, height: picture.height, alt: String::new(), fonts: picture.fonts, hash: String::new() }).unwrap();
        let pixmap = tiny_skia::Pixmap::decode_png(&png).unwrap();
        // Labels sit on node fills, so count ink as dark as the text colour
        // (#2c2825), darker than every fill and line.
        pixmap.pixels().iter().filter(|p| p.red() < 80 && p.green() < 80 && p.blue() < 80).count()
    };
    let mut without_text = picture.svg.clone();
    while let Some(at) = without_text.find("<text") {
        let end = at + without_text[at..].find("</text>").unwrap() + "</text>".len();
        without_text.replace_range(at..end, "");
    }
    let (with, without) = (ink(&picture.svg), ink(&without_text));
    assert!(with > without + 500, "labels added only {} inked pixels", with.saturating_sub(without));
}

#[test]
fn the_address_is_pure_and_follows_the_source() {
    let hash = hash_of(FLOW);
    assert_eq!(hash, hash_of(FLOW));
    assert_eq!(hash.len(), 16);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_ne!(hash, hash_of("flowchart LR\n  A --> B\n"));
}

#[test]
fn the_build_draws_each_diagram_once_and_keeps_every_earlier_picture() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("_moss/diagrams/aaaaaaaaaaaaaaaa.png");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, b"\x89PNG earlier").unwrap();

    let mut doc = doc_with(&format!("Before.\n\n```mermaid\n{FLOW}```\n\n```mermaid\nflowchart TD\n  A -->\n```\n"));
    doc.features.diagrams = true;
    let mut pending = PendingManifest::new(crate::types::content::SiteHashes::default());
    emit_diagram_pngs(&[doc], dir.path(), &mut pending).unwrap();

    let key = format!("_moss/diagrams/{}.png", hash_of(FLOW));
    let png = std::fs::read(dir.path().join(&key)).unwrap();
    assert!(png.starts_with(b"\x89PNG"), "the drawn diagram has its picture");
    assert!(pending.files().contains_key(&key));
    assert!(pending.files().contains_key("_moss/diagrams/aaaaaaaaaaaaaaaa.png"), "an earlier picture still ships");
    assert_eq!(std::fs::read_dir(old.parent().unwrap()).unwrap().count(), 2, "the broken diagram has none");
}

#[test]
fn a_feed_gets_the_hosted_picture_and_a_broken_diagram_keeps_its_source() {
    let figure = crate::build::markdown::diagram::render(FLOW, "d1", None, Language::De, None).unwrap().html;
    let broken = "<figure class=\"moss-diagram moss-diagram-error\"><figcaption class=\"moss-diagram-message\">x</figcaption><pre><code class=\"language-mermaid\">a</code></pre></figure>";
    let feed = swap_diagram_figures_for_feed_imgs(&format!("<p>a</p>{figure}{broken}"), "https://example.com/");
    let picture = picture::picture(FLOW, Language::De).unwrap();
    let (w, h) = picture::size_1x(picture.width, picture.height);
    assert_eq!(
        feed,
        format!(
            "<p>a</p><p><img src=\"https://example.com/_moss/diagrams/{}.png\" width=\"{w}\" height=\"{h}\" alt=\"Flussdiagramm\" style=\"max-width:100%;height:auto\"></p>\n{broken}",
            hash_of(FLOW)
        )
    );
}

#[test]
fn a_newsletter_gets_the_same_picture_named_in_its_language() {
    let html = email_diagram_html(FLOW, "https://example.com", Language::ZhHans).unwrap();
    assert!(html.contains(&format!("/_moss/diagrams/{}.png", hash_of(FLOW))), "{html}");
    assert!(html.contains("alt=\"流程图\""), "{html}");
    assert_eq!(email_diagram_html("flowchart TD\n  A -->\n", "https://example.com", Language::En), None);
    assert_eq!(diagram_png_hashes(&format!("```mermaid\n{FLOW}```\n\n```mermaid\nnope\n```\n")), [hash_of(FLOW)]);
}

#[test]
fn a_character_no_carried_font_has_means_no_picture_and_no_address() {
    // U+1F600 is in neither Inter nor any CJK face: a picture would be drawn
    // with a hole in it and cached by mail proxies forever. A character
    // reference reaches the drawing as the character itself.
    for source in ["flowchart LR\n  A[\u{1F600} happy] --> B\n", "flowchart LR\n  A[&#128512; happy] --> B\n", "flowchart LR\n  A[#128512; happy] --> B\n"] {
        assert!(picture::picture(source, Language::En).is_none(), "{source}");
        let figure = crate::build::markdown::diagram::render(source, "d1", None, Language::En, None).unwrap().html;
        assert!(figure.contains("<svg") && !figure.contains("data-diagram"), "the page still draws it: {figure}");
    }
    assert_eq!(picture::picture(FLOW, Language::En).unwrap().fonts, picture::Fonts::Inter);
}

#[test]
fn a_feed_gets_one_layout_of_a_diagram_without_a_picture() {
    let figure = crate::build::markdown::diagram::render(
        "flowchart LR\n  A[\u{1F600} Write markdown] --> B[Build the site] --> C[Publish it] --> D[Read it]\n",
        "d1",
        None,
        Language::En,
        None,
    )
    .unwrap()
    .html;
    assert!(figure.contains("moss-diagram-narrow"), "the page has both layouts");
    let feed = swap_diagram_figures_for_feed_imgs(&figure, "https://example.com");
    assert!(feed.contains("moss-diagram-wide") && !feed.contains("moss-diagram-narrow"), "{feed}");
}

#[test]
fn flattening_touches_only_complete_var_references_inside_tags() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" style="--_fg:var(--pele-fg,#123456);color:var(--_fg)" width="10" height="10"><g aria-label="cost = var(x" fill="var(--_fg)"><text>var(--_fg) stays text</text></g><a href="https://x/var(y">z</a><g aria-label="a var(--x"><text>1) b</text></g></svg>"#;
    let flat = picture::flatten(svg);
    let fg = &picture::palette().iter().find(|(n, _)| n == "--pele-fg").unwrap().1;
    assert!(flat.contains(&format!("fill=\"{fg}\"")), "{flat}");
    assert!(flat.contains("aria-label=\"cost = var(x\""), "an incomplete var( is copied: {flat}");
    assert!(flat.contains("<text>var(--_fg) stays text</text>"), "text is never rewritten: {flat}");
    assert!(flat.contains("href=\"https://x/var(y\">z</a>"), "nothing after it is lost: {flat}");
    assert!(flat.contains("aria-label=\"a var(--x\"><text>1) b</text></g></svg>"), "a reference ends with its value: {flat}");
    assert!(!flat.contains("--_fg:"), "the root's declarations go: {flat}");
}
