use super::*;
use crate::ast::{parse, render_document, DefaultHooks, Url, UrlKind};

fn resolved(md: &str) -> crate::ast::Document {
    let mut doc = parse(md);
    crate::ast::visit_urls_mut(&mut doc, |url| {
        if let Url::Unresolved(s) = url {
            let kind = if s.starts_with("http") { UrlKind::External } else { UrlKind::Internal };
            *url = Url::resolved(s.clone(), kind);
        }
    });
    doc
}

fn render(md: &str) -> String {
    render_document(&resolved(md), &DefaultHooks::new())
}

const WORDS: &str = "> The annual fiesta has become the day of days.\n>\n";

#[test]
fn a_dash_line_with_an_absolute_link_sets_cite_and_changes_nothing_else() {
    let html = render(&format!("{WORDS}> — Jane Doe, [\"Care of Migrants\"](https://archive.example.org/record/123), 1958"));
    assert!(html.contains(r#"<blockquote cite="https://archive.example.org/record/123">"#), "got: {html}");
    assert!(html.contains(r#"<p>— Jane Doe, <a target="_blank" rel="noopener" href="https://archive.example.org/record/123">"#), "got: {html}");
    assert!(!html.contains("<figure"), "got: {html}");
}

#[test]
fn the_horizontal_bar_and_leading_space_count_as_a_dash() {
    let html = render(&format!("{WORDS}>   ― [Doe](https://example.org/a)"));
    assert!(html.contains(r#"cite="https://example.org/a""#), "got: {html}");
}

#[test]
fn the_first_absolute_link_wins() {
    let html = render(&format!("{WORDS}> — [A](/local/) and [B](https://example.org/b) and [C](https://example.org/c)"));
    assert!(html.contains(r#"cite="https://example.org/b""#), "got: {html}");
}

#[test]
fn a_relative_link_leaves_the_blockquote_alone() {
    let html = render(&format!("{WORDS}> — [Doe](/notes/doe/)"));
    assert!(!html.contains("cite="), "got: {html}");
}

#[test]
fn a_last_paragraph_without_the_dash_leaves_the_blockquote_alone() {
    let html = render(&format!("{WORDS}> Jane Doe, [record](https://example.org/a)"));
    assert!(!html.contains("cite="), "got: {html}");
}

#[test]
fn a_dash_line_that_is_not_last_leaves_the_blockquote_alone() {
    let html = render("> — [Doe](https://example.org/a)\n>\n> More words follow.");
    assert!(!html.contains("cite="), "got: {html}");
}

#[test]
fn a_grid_quote_cell_takes_its_cite_from_the_source_line() {
    let html = render(":::grid 2\n> Communication is health.\n\n*[Montaigne, 1924](https://example.org/essays/)*\n+++\n> Other.\n\n*Plain source*\n:::");
    assert!(html.contains(r#"<figure class="moss-quote"><blockquote cite="https://example.org/essays/">"#), "got: {html}");
    assert_eq!(html.matches("cite=").count(), 1, "only the linked source: {html}");
}

#[test]
fn a_grid_quote_with_its_own_dash_line_has_one_cite() {
    let md = ":::grid 2\n> Words.\n>\n> — [A](https://example.org/a)\n\n*[B](https://example.org/b)*\n+++\n> x\n\n*y*\n:::";
    let html = render(md);
    assert_eq!(html.matches("cite=").count(), 1, "got: {html}");
    assert_eq!(citations(&resolved(md).blocks)[0].href, "https://example.org/a");
}

#[test]
fn citations_list_each_address_once_in_page_order() {
    let md = format!(
        "{WORDS}> — [First \"one\"](https://example.org/a)\n\nBetween.\n\n{WORDS}> — [Again](https://example.org/a)\n\n> x\n>\n> — [Second](https://example.org/b)\n\n:::grid 2\n> Q.\n\n*[Third](https://example.org/c)*\n+++\n> R.\n\n*Plain*\n:::"
    );
    let found = citations(&resolved(&md).blocks);
    let got: Vec<(&str, &str)> = found.iter().map(|c| (c.href.as_str(), c.text.as_str())).collect();
    assert_eq!(
        got,
        [("https://example.org/a", "First \"one\""), ("https://example.org/b", "Second"), ("https://example.org/c", "Third")]
    );
}

#[test]
fn no_cited_quote_means_no_citations() {
    assert!(citations(&resolved(&format!("{WORDS}> — [Doe](/x/)\n\nText.")).blocks).is_empty());
}
