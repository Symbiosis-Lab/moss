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
    assert_eq!(render_byline_html(&[], false, &[]), None);
    assert_eq!(render_colophon_html(&[], false), None);
}

#[test]
fn each_entry_is_one_row() {
    // Full-width space between role and name survives verbatim — the shape
    // content is being migrated to.
    let html = render_byline_html(&rows(&["作者　陳遠山", "編輯　周一"]), false, &[]).unwrap();
    assert_eq!(
        html,
        r#"<div class="moss-byline"><div class="moss-byline-row">作者　陳遠山</div><div class="moss-byline-row">編輯　周一</div></div>"#
    );
}

#[test]
fn colophon_is_the_same_rows_under_its_own_classes() {
    let html = render_colophon_html(&rows(&["首發媒體　遠聲媒體"]), false).unwrap();
    assert_eq!(
        html,
        r#"<div class="moss-article-colophon"><div class="moss-article-colophon-row">首發媒體　遠聲媒體</div></div>"#
    );
}

#[test]
fn inline_markdown_is_rendered_not_escaped() {
    // The whole point of the field: a first-publication row carries links.
    let html = render_colophon_html(
        &rows(&["首發媒體　[遠聲媒體](https://example.org/a)、[夜讀](https://example.com/b)"]),
        false,
    )
    .unwrap();
    // External links get the same new-tab treatment as anywhere else in the body.
    assert!(
        html.contains(r#"href="https://example.org/a">遠聲媒體</a>"#),
        "markdown link must survive as an anchor: {html}"
    );
    assert!(
        html.contains(r#"href="https://example.com/b">夜讀</a>"#),
        "both links, not just the first: {html}"
    );
    assert!(!html.contains("<p>"), "the paragraph wrapper is unwrapped: {html}");
}

#[test]
fn plain_text_is_html_escaped() {
    let html = render_byline_html(&rows(&["Q&A 統籌"]), false, &[]).unwrap();
    assert!(html.contains("Q&amp;A"), "ampersand escaped: {html}");
}

#[test]
fn raw_html_passes_through_exactly_as_it_does_in_the_body() {
    // Not a security boundary and not trying to be one: a credit row is the
    // site author's own file, read by the same markdown path as the body,
    // where inline HTML has always passed through. Pinned so a future change
    // to that shared path is a visible decision rather than a surprise here.
    let html = render_byline_html(&rows(&["作者 <span>X</span>"]), false, &[]).unwrap();
    assert!(html.contains("<span>X</span>"), "got: {html}");
}

#[test]
fn a_row_that_is_not_one_paragraph_falls_back_to_literal_text() {
    // A stray `#` must render as itself, never as a heading in the masthead.
    let html = render_byline_html(&rows(&["# 作者 X"]), false, &[]).unwrap();
    assert!(
        html.contains("# 作者 X"),
        "heading syntax renders as literal text: {html}"
    );
    assert!(!html.contains("<h1"), "no heading in the byline: {html}");
}

#[test]
fn source_mapping_attribute_is_opt_in_and_names_its_field() {
    let on = render_byline_html(&rows(&["作者 X"]), true, &[]).unwrap();
    assert!(on.contains(r#"data-source-fm="byline""#), "got: {on}");
    let on = render_colophon_html(&rows(&["首發媒體 X"]), true).unwrap();
    assert!(on.contains(r#"data-source-fm="colophon""#), "got: {on}");
    let off = render_byline_html(&rows(&["作者 X"]), false, &[]).unwrap();
    assert!(!off.contains("data-source-fm"), "got: {off}");
}

fn place_line() -> TermLine {
    TermLine {
        term: "places".into(),
        field: "location".into(),
        label: "Location".into(),
        separator: ": ".into(),
        links: "[Kyoto](/about/kyoto/)".into(),
        is_place: true,
    }
}

#[test]
fn the_place_line_keeps_its_class_and_text_on_a_page_with_no_byline() {
    let html = render_byline_html(&[], false, &[place_line()]).unwrap();
    assert_eq!(html, r#"<div class="moss-term-line moss-place-line" data-term="places" data-field="location"><span class="moss-term-line-label">Location</span>: <a href="/about/kyoto/">Kyoto</a></div>"#);
}

#[test]
fn a_term_line_names_its_kind_and_field_and_links_each_name() {
    let tags = TermLine {
        term: "tags".into(),
        field: "tags".into(),
        label: "Tags".into(),
        separator: ": ".into(),
        links: "[Field Notes](/tags/field-notes/), [Slow Reading](/tags/slow-reading/)".into(),
        is_place: false,
    };
    let html = render_byline_html(&[], false, &[tags]).unwrap();
    assert_eq!(html, r#"<div class="moss-term-line" data-term="tags" data-field="tags"><span class="moss-term-line-label">Tags</span>: <a href="/tags/field-notes/">Field Notes</a>, <a href="/tags/slow-reading/">Slow Reading</a></div>"#);
}

#[test]
fn a_term_line_maps_back_to_its_field_in_the_editor() {
    let html = render_byline_html(&[], true, &[place_line()]).unwrap();
    assert!(html.contains(r#"data-source-fm="location""#), "got: {html}");
}

#[test]
fn term_lines_follow_the_byline() {
    let html = render_byline_html(&rows(&["作者　陳遠山"]), false, &[place_line()]).unwrap();
    let (byline, line) = (html.find("moss-byline").unwrap(), html.find("moss-place-line").unwrap());
    assert!(byline < line, "got: {html}");
}

mod site_credit {
    use super::super::ColophonCredit;

    fn parse(raw: &str) -> Result<Option<(ColophonCredit, Vec<String>)>, String> {
        ColophonCredit::parse(raw)
    }

    #[test]
    fn a_link_stays_a_link_and_text_is_escaped() {
        let (credit, dropped) = parse("Designed by [Example Studio](https://example.org) & friends").unwrap().unwrap();
        assert!(credit.html.contains(r#"href="https://example.org">Example Studio</a>"#), "{}", credit.html);
        assert!(credit.html.contains("&amp; friends"), "{}", credit.html);
        assert!(!credit.html.contains("<p>"), "{}", credit.html);
        assert!(dropped.is_empty());
    }

    #[test]
    fn block_markdown_and_extra_lines_are_refused_naming_the_key() {
        for raw in ["# Title", "- one\n- two", "first\n\nsecond", "one\ntwo", "> quoted", "![x](a.png)"] {
            let problem = parse(raw).expect_err(raw);
            assert!(problem.contains("[site].credit"), "{problem}");
        }
    }

    #[test]
    fn an_empty_setting_is_no_credit() {
        assert_eq!(parse("  ").unwrap(), None);
    }

    #[test]
    fn raw_html_is_shown_as_text() {
        let (credit, _) = parse("Hi <script>alert(1)</script> <b>x</b>").unwrap().unwrap();
        assert!(!credit.html.contains("<script") && !credit.html.contains("<b>"), "{}", credit.html);
        assert!(credit.html.contains("&lt;script&gt;"), "{}", credit.html);
    }

    #[test]
    fn only_http_https_mailto_and_relative_links_survive() {
        for ok in ["https://example.org", "http://example.org/a?b=c&d=e", "mailto:a@example.org", "/about", "about.html", "#top", "../x"] {
            let (credit, dropped) = parse(&format!("[x]({ok})")).unwrap().unwrap();
            assert!(credit.html.contains("<a "), "{ok}: {}", credit.html);
            assert!(dropped.is_empty(), "{ok}");
        }
        for bad in ["javascript:alert(1)", "JaVaScRiPt:alert(1)", "data:text/html,x", "vbscript:x", "//evil.example", "jav&#x61;script:alert(1)"] {
            let (credit, dropped) = parse(&format!("see [x]({bad}) now")).unwrap().unwrap();
            assert!(!credit.html.contains("<a ") && !credit.html.to_lowercase().contains("javascript"), "{bad}: {}", credit.html);
            assert!(credit.html.contains("see x now"), "the text stays: {}", credit.html);
            assert_eq!(dropped.len(), 1, "{bad}");
        }
    }
}
