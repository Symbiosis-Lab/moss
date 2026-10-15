//! Where a claimed term page's listing goes in its body.

use crate::i18n::Language;

/// `body` with `listing` inserted before its closing sources section, or
/// `body` followed by `listing` when it has none.
///
/// The closing sources section is the last `<h2>` of the body when its text
/// is one of the language's sources headings (`i18n::sources_headings`), so a
/// page that ends in "Sources" keeps that list at the foot of the page.
pub(crate) fn insert_before_sources(body: &str, listing: &str, lang: Language, lang_tag: &str) -> String {
    let at = body.rfind("<h2").filter(|&start| {
        let rest = &body[start..];
        let Some(end) = rest.find("</h2>") else { return false };
        let inner = rest[..end].split_once('>').map_or("", |(_, inner)| inner);
        let mut text = String::new();
        let mut in_tag = false;
        for c in inner.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => text.push(c),
                _ => {}
            }
        }
        let text = text.trim().to_lowercase();
        crate::i18n::sources_headings(lang, lang_tag).iter().any(|h| h.to_lowercase() == text)
    });
    match at {
        Some(at) => format!("{}{}{}", &body[..at], listing, &body[at..]),
        None => format!("{body}{listing}"),
    }
}
