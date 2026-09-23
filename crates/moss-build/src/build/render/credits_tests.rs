//! Byline / colophon markup tests. Parse-side shapes (string / list / block
//! scalar / absent / empty) are covered where the shapes are defined —
//! `moss_core::frontmatter_union::normalize_credit_rows` — so these cover only
//! what the emitter adds: one row per entry, inline markdown preserved,
//! everything else escaped, and the two blocks differing only in their classes.

use super::*;

fn rows(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn absent_rows_emit_nothing() {
    assert_eq!(render_byline_html(&[], false, None), None);
    assert_eq!(render_colophon_html(&[], false), None);
}

#[test]
fn each_entry_is_one_row() {
    // Full-width space between role and name survives verbatim — the shape
    // content is being migrated to.
    let html = render_byline_html(&rows(&["作者　糜緒洋", "編輯　謝丁"]), false, None).unwrap();
    assert_eq!(
        html,
        r#"<div class="moss-byline"><div class="moss-byline-row">作者　糜緒洋</div><div class="moss-byline-row">編輯　謝丁</div></div>"#
    );
}

#[test]
fn colophon_is_the_same_rows_under_its_own_classes() {
    let html = render_colophon_html(&rows(&["首發媒體　端傳媒"]), false).unwrap();
    assert_eq!(
        html,
        r#"<div class="moss-article-colophon"><div class="moss-article-colophon-row">首發媒體　端傳媒</div></div>"#
    );
}

#[test]
fn inline_markdown_is_rendered_not_escaped() {
    // The whole point of the field: a first-publication row carries links.
    let html = render_colophon_html(
        &rows(&["首發媒體　[端傳媒](https://theinitium.com/a)、[單讀](https://example.com/b)"]),
        false,
    )
    .unwrap();
    // External links get the same new-tab treatment as anywhere else in the body.
    assert!(
        html.contains(r#"href="https://theinitium.com/a">端傳媒</a>"#),
        "markdown link must survive as an anchor: {html}"
    );
    assert!(
        html.contains(r#"href="https://example.com/b">單讀</a>"#),
        "both links, not just the first: {html}"
    );
    assert!(!html.contains("<p>"), "the paragraph wrapper is unwrapped: {html}");
}

#[test]
fn plain_text_is_html_escaped() {
    let html = render_byline_html(&rows(&["Q&A 統籌"]), false, None).unwrap();
    assert!(html.contains("Q&amp;A"), "ampersand escaped: {html}");
}

#[test]
fn raw_html_passes_through_exactly_as_it_does_in_the_body() {
    // Not a security boundary and not trying to be one: a credit row is the
    // site author's own file, read by the same markdown path as the body,
    // where inline HTML has always passed through. Pinned so a future change
    // to that shared path is a visible decision rather than a surprise here.
    let html = render_byline_html(&rows(&["作者 <span>X</span>"]), false, None).unwrap();
    assert!(html.contains("<span>X</span>"), "got: {html}");
}

#[test]
fn a_row_that_is_not_one_paragraph_falls_back_to_literal_text() {
    // A stray `#` must render as itself, never as a heading in the masthead.
    let html = render_byline_html(&rows(&["# 作者 X"]), false, None).unwrap();
    assert!(
        html.contains("# 作者 X"),
        "heading syntax renders as literal text: {html}"
    );
    assert!(!html.contains("<h1"), "no heading in the byline: {html}");
}

#[test]
fn source_mapping_attribute_is_opt_in_and_names_its_field() {
    let on = render_byline_html(&rows(&["作者 X"]), true, None).unwrap();
    assert!(on.contains(r#"data-source-fm="byline""#), "got: {on}");
    let on = render_colophon_html(&rows(&["首發媒體 X"]), true).unwrap();
    assert!(on.contains(r#"data-source-fm="colophon""#), "got: {on}");
    let off = render_byline_html(&rows(&["作者 X"]), false, None).unwrap();
    assert!(!off.contains("data-source-fm"), "got: {off}");
}

#[test]
fn place_line_renders_on_a_page_with_no_byline() {
    // The bug this round caught: keying the combined result on `rows`
    // alone silently dropped the place line on the common case — a page
    // with `location:` set and no `byline:` at all.
    let html = render_byline_html(&[], false, Some("Location: [Kyoto](/about/kyoto/)")).unwrap();
    assert!(html.contains(r#"<div class="moss-place-line">Location: <a href="/about/kyoto/">Kyoto</a></div>"#), "got: {html}");
}

#[test]
fn place_line_and_byline_both_render_independently() {
    let html = render_byline_html(&rows(&["作者　糜緒洋"]), false, Some("Location: [Kyoto](/about/kyoto/)")).unwrap();
    assert!(html.contains("moss-byline"), "got: {html}");
    assert!(html.contains("moss-place-line"), "got: {html}");
}
