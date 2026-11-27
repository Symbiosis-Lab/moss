//! A page's terms drawn as links: the place line, and a line for every kind
//! that sets `line = true`.

use super::index::TermIndex;
use super::kinds::TermKind;
use crate::i18n::Language;
use crate::page::site_page::SitePage;

/// One field's names on one page, ready for the credit area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermLine {
    pub term: String,
    pub field: String,
    pub label: String,
    pub separator: String,
    /// The linked names as inline markdown.
    pub links: String,
    pub is_place: bool,
}

impl TermLine {
    pub fn class(&self) -> &'static str {
        if self.is_place { "moss-term-line moss-place-line" } else { "moss-term-line" }
    }

    /// The line as HTML: its label, then its names as links drawn as the body draws a link.
    pub fn html(&self, emit_source_fm: bool) -> String {
        use crate::media::html_escape;
        let (term, field) = (html_escape(&self.term), html_escape(&self.field));
        let source = if emit_source_fm { format!(r#" data-source-fm="{field}""#) } else { String::new() };
        format!(
            r#"<div class="{}" data-term="{term}" data-field="{field}"{source}><span class="moss-term-line-label">{}</span>{}{}</div>"#,
            self.class(),
            html_escape(&self.label),
            html_escape(&self.separator),
            links_html(&self.links),
        )
    }
}

/// `links` as inline HTML; escaped text should it ever parse as more than one paragraph.
fn links_html(links: &str) -> String {
    use crate::ast::url::{Url, UrlKind};
    let mut doc = crate::ast::parser::parse(links);
    // Every address is already a term page's final one.
    crate::ast::visit::visit_urls_mut(&mut doc, |url| *url = Url::resolved(url.href().to_string(), UrlKind::Internal));
    match doc.blocks.as_slice() {
        [crate::ast::Block::Paragraph(inlines)] => crate::ast::render::render_inlines_html(&crate::ast::hooks::DefaultHooks::default(), inlines),
        _ => crate::media::html_escape(links),
    }
}

/// The plain label a field's role carries: the translated copy for a
/// built-in, the field's name as a label for a declared one.
pub fn term_role_label(lang: Language, field: &str) -> String {
    match field {
        "author" | "tags" | "editor" | "jury" | "location" => crate::i18n::t(lang, &format!("term_role_{field}")).to_string(),
        declared => crate::fields::name_label(declared),
    }
}

/// The page's lines, in kind then field order.
pub fn term_lines<P: SitePage>(page: &P, index: &TermIndex, site_lang: Language) -> Vec<TermLine> {
    let place_key = place_kind(index).map(|k| k.key.as_str());
    index
        .kinds()
        .iter()
        // Every place-typed kind reads `location`, so only the first draws a line.
        .filter(|k| k.draws_line() && (!k.is_place || place_key == Some(k.key.as_str())))
        .flat_map(|kind| kind.line_fields().into_iter().filter_map(move |field| term_line(page, index, kind, field, site_lang)))
        .collect()
}

/// The page's places that have a term page, as a card shows them.
pub fn place_names<P: SitePage>(page: &P, index: &TermIndex) -> Option<String> {
    let names: Vec<String> = linked_names(page, index, place_kind(index)?, "location").into_iter().map(|(name, _)| name).collect();
    (!names.is_empty()).then(|| names.join(", "))
}

fn place_kind(index: &TermIndex) -> Option<&TermKind> {
    index.kinds().iter().find(|k| k.is_place)
}

/// The field's names that have a term page, each with that page's address.
/// [`TermIndex::term_url_in`] keeps a place-typed kind's links site-wide.
fn linked_names<P: SitePage>(page: &P, index: &TermIndex, kind: &TermKind, field: &str) -> Vec<(String, String)> {
    page.term_values(field)
        .iter()
        .filter_map(|name| {
            let name = name.trim();
            Some((name.to_string(), index.term_url_in(page.edition(), &kind.key, name)?))
        })
        .collect()
}

fn term_line<P: SitePage>(page: &P, index: &TermIndex, kind: &TermKind, field: &str, site_lang: Language) -> Option<TermLine> {
    let title = page.title().trim().to_lowercase();
    let links: Vec<String> = linked_names(page, index, kind, field)
        .into_iter()
        // A page titled with a name already says it; the line would only repeat the title.
        .filter(|(name, _)| name.to_lowercase() != title)
        .map(|(name, url)| format!("[{}]({url})", link_text(&name)))
        .collect();
    if links.is_empty() {
        return None;
    }
    // A place-typed kind has one site-wide namespace, labelled in the site's language.
    let lang = if kind.is_place { site_lang } else { page.lang() };
    Some(TermLine {
        term: kind.key.clone(),
        field: field.to_string(),
        label: term_role_label(lang, field),
        separator: crate::i18n::t(lang, "term_line_separator").to_string(),
        links: links.join(", "),
        is_place: kind.is_place,
    })
}

/// A name as link text: brackets would otherwise end the link early.
fn link_text(name: &str) -> String {
    name.replace('\\', r"\\").replace('[', r"\[").replace(']', r"\]")
}
