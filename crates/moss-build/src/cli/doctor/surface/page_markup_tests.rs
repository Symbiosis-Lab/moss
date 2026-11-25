use super::*;

fn kinds(md: &str) -> Vec<(MarkupKind, usize)> {
    page_markup(md, "p.md").into_iter().map(|m| (m.kind, m.line)).collect()
}

#[test]
fn class_spans_after_text_headings_and_pictures_are_found() {
    let md = "---\ntitle: T\n---\n\nA [word]{.hl} here.\n\n## Heading {.kicker}\n\n![pic](x.png){.wide}\n";
    assert_eq!(kinds(md), [(MarkupKind::ClassSpan, 5), (MarkupKind::ClassSpan, 7), (MarkupKind::ClassSpan, 9)]);
}

#[test]
fn a_raw_html_block_is_found_once_at_its_first_line() {
    let md = "Intro.\n\n<div class=\"box\">\n<p>raw</p>\n</div>\n\n<!-- a note -->\n";
    assert_eq!(kinds(md), [(MarkupKind::RawHtml, 3)]);
}

#[test]
fn fences_code_and_braces_that_are_not_classes_are_left_alone() {
    let md = ":::grid 2 {.tall}\nleft\n+++\nright\n:::\n\n::: {.note}\nHi.\n:::\n\n```\n[x]{.hl}\n<div>code</div>\n```\n\nUse `{.hl}` here; a set {.5, 1} is fine.\n";
    assert_eq!(kinds(md), []);
}

#[test]
fn class_spans_in_lists_and_tables_and_html_in_a_quote_are_found() {
    let md = "- a [x]{.hl} item\n\n| a | b |\n|---|---|\n| [y]{.hl} | z |\n\n> <div>quoted</div>\n";
    assert_eq!(kinds(md), [(MarkupKind::ClassSpan, 1), (MarkupKind::ClassSpan, 5), (MarkupKind::RawHtml, 7)]);
}

#[test]
fn an_unclosed_brace_is_not_a_class() {
    assert_eq!(kinds("Write {.hl and stop.\n"), []);
}

#[test]
fn braces_inside_math_are_not_classes() {
    assert_eq!(kinds("Energy $a{.b}$ here.\n\n$$\nx{.y}\n$$\n"), []);
}
