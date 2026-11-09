use crate::ast::{classify_remaining_urls, parse, render_document, DefaultHooks};

fn render(markdown: &str) -> String {
    let mut doc = parse(markdown);
    classify_remaining_urls(&mut doc);
    render_document(&doc, &DefaultHooks::new())
}

#[test]
fn pair_becomes_mark() {
    assert_eq!(render("a ==x== b"), "<p>a <mark>x</mark> b</p>\n");
}

#[test]
fn pair_may_contain_other_inlines() {
    let html = render("==**bold** text==");
    assert!(html.contains("<mark><strong>bold</strong> text</mark>"), "{html}");
}

#[test]
fn spaced_or_longer_runs_stay_text() {
    for src in ["a == b", "a === b", "a ==== b ==== c", "== a == b"] {
        let html = render(src);
        assert!(!html.contains("<mark"), "{src}: {html}");
        assert!(html.contains("=="), "{src}: {html}");
    }
}

#[test]
fn setext_underline_is_a_heading_not_a_mark() {
    let html = render("Title\n===\n");
    assert!(html.contains("<h1"), "{html}");
    assert!(!html.contains("<mark"), "{html}");
}

#[test]
fn inline_code_is_untouched() {
    let html = render("`==code==` and ==real==");
    assert!(html.contains("<code>==code==</code>"), "{html}");
    assert!(html.contains("<mark>real</mark>"), "{html}");
}

#[test]
fn pair_crossing_a_link_stays_literal() {
    let html = render("==a [b==](https://example.com/)");
    assert!(!html.contains("<mark"), "{html}");
    assert!(html.contains("==a "), "{html}");
}

#[test]
fn code_inside_a_pair_is_marked_with_the_code_kept() {
    let html = render("==a `c` b==");
    assert!(html.contains("<mark>a <code>c</code> b</mark>"), "{html}");
}

#[test]
fn delimiters_inside_code_never_pair_with_outside_ones() {
    let html = render("==a `b==` c");
    assert!(!html.contains("<mark"), "{html}");
}

#[test]
fn heading_marks_and_keeps_the_markers_out_of_the_id() {
    let html = render("## a ==b== c\n");
    assert!(html.contains("<mark>b</mark>"), "{html}");
    assert!(html.contains(r#"id="a-b-c""#), "{html}");
}

#[test]
fn list_item_and_table_cell_mark() {
    let html = render("- ==x==\n\n| h |\n|---|\n| ==y== |\n");
    assert!(html.contains("<mark>x</mark>"), "{html}");
    assert!(html.contains("<mark>y</mark>"), "{html}");
}

#[test]
fn a_backslash_does_not_escape_the_delimiter() {
    assert!(render(r"\==x==").contains("<mark>x</mark>"));
}
