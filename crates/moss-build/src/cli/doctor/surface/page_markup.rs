//! Markup a page writes that is not plain Markdown: a `{.class}` written after
//! text, a heading or a picture, which moss prints as literal text, and a raw
//! HTML block, which reaches the page untouched by the theme or the editor.
//!
//! A fence line (`:::grid 2 {.tall}`, `::: {.note}`) is moss's own syntax and is
//! left to the classed-div report. Code blocks and code spans are not text
//! events, and an HTML block that holds only a comment draws nothing, so
//! neither counts. Inline HTML (`<br>`, a `<span>` inside a paragraph) is not
//! reported: only blocks.

use std::path::Path;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupKind {
    ClassSpan,
    RawHtml,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMarkup {
    pub kind: MarkupKind,
    /// Site-relative path of the page.
    pub path: String,
    pub line: usize,
    /// The source line, trimmed.
    pub text: String,
}

/// Whether `text` holds a closed `{.class}` attribute block.
fn has_class_block(text: &str) -> bool {
    text.match_indices("{.").any(|(i, _)| {
        text.get(i + 2..).is_some_and(|rest| {
            rest.starts_with(|c: char| c.is_alphabetic() || c == '_') && rest.split_once('}').is_some_and(|(inner, _)| !inner.contains('{'))
        })
    })
}

fn page_markup(content: &str, path: &str) -> Vec<PageMarkup> {
    let (body, offset) = super::super::page_body(content);
    let source_line = |start: usize| {
        let line = super::super::line_of(&body, start);
        (offset + line, body.lines().nth(line - 1).unwrap_or("").trim().to_string())
    };
    let mut out = Vec::new();
    let mut in_code = false;
    let mut html_block: Option<usize> = None;
    for (event, range) in Parser::new_ext(&body, moss_core::ast::parser_options(true)).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => in_code = false,
            Event::Start(Tag::HtmlBlock) => html_block = Some(range.start),
            Event::Html(html) if !html.trim().is_empty() && !html.trim_start().starts_with("<!--") => {
                if let Some(start) = html_block.take() {
                    let (line, text) = source_line(start);
                    out.push(PageMarkup { kind: MarkupKind::RawHtml, path: path.to_string(), line, text });
                }
            }
            Event::End(TagEnd::HtmlBlock) => html_block = None,
            // Math is neither class text nor HTML, and its braces are not attributes.
            Event::InlineMath(_) | Event::DisplayMath(_) => {}
            Event::Text(text) if !in_code && has_class_block(&text) => {
                let (line, source) = source_line(range.start);
                let reported = out.last().is_some_and(|m: &PageMarkup| m.kind == MarkupKind::ClassSpan && m.line == line);
                if !source.starts_with(":::") && !reported {
                    out.push(PageMarkup { kind: MarkupKind::ClassSpan, path: path.to_string(), line, text: source });
                }
            }
            _ => {}
        }
    }
    out
}

/// Every page's class spans and raw HTML blocks, in page order.
pub fn check(folder: &Path) -> Vec<PageMarkup> {
    super::super::markdown_files(folder)
        .into_iter()
        .filter_map(|page| Some((super::site_relative(folder, &page), std::fs::read_to_string(&page).ok()?)))
        .flat_map(|(rel, content)| page_markup(&content, &rel))
        .collect()
}

#[cfg(test)]
#[path = "page_markup_tests.rs"]
mod tests;
