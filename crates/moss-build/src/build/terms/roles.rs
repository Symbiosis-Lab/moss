//! The role a name holds in a term field: the heading a term page gives each
//! field's section, and the role a listing card shows beside a credited name.
//! The card credits (`set_card_credits`) read the same labels, so they live
//! with the labels rather than beside the term model in `terms.rs`.

use super::{field_names, TermKind};
use crate::build::media::cover::html_escape;
use crate::build::types::ParsedDocument;

/// The `<h2>` above one field's group on a term page — "Editor" over the
/// pages that named this person through `editor:`. The class is in
/// moss-core's component table; the copy is `term_role_<field>` in
/// `i18n::strings`.
pub fn term_role_heading(lang: crate::i18n::Language, field: &str) -> String {
    format!(
        "<h2 class=\"moss-term-role\">{}</h2>",
        html_escape(&term_role_label(lang, field))
    )
}

/// The plain label a field's role carries: the translated copy for a
/// built-in, the field's name capitalized for a declared one. The heading
/// above a term page's section and the role beside a card's name both read it.
pub fn term_role_label(lang: crate::i18n::Language, field: &str) -> String {
    match field {
        "author" | "tags" | "editor" | "jury" | "location" => {
            crate::i18n::t(lang, &format!("term_role_{}", field)).to_string()
        }
        // A declared field has no translated label: its name, capitalized.
        declared => {
            let mut chars = declared.chars();
            chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
    }
}

/// Set [`ParsedDocument::card_credits`] on every page from the kinds that
/// declare `cards = true`. A site with no such kind sets `None` everywhere.
pub fn set_card_credits(documents: &mut [ParsedDocument], kinds: &[TermKind]) {
    for doc in documents.iter_mut() {
        doc.card_credits = card_credits_html(doc, kinds);
    }
}

fn card_credits_html(doc: &ParsedDocument, kinds: &[TermKind]) -> Option<String> {
    let mut credits: Vec<String> = Vec::new();
    for kind in kinds.iter().filter(|k| k.cards) {
        for field in &kind.fields {
            let names = field_names(doc, field);
            let names: Vec<&str> = names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).collect();
            if names.is_empty() {
                continue;
            }
            let role = html_escape(&term_role_label(doc.lang, field).to_lowercase());
            for name in names {
                credits.push(format!(
                    r#"<span class="moss-credit">{}<span class="moss-credit-role">, {}</span></span>"#,
                    html_escape(name),
                    role
                ));
            }
        }
    }
    if credits.is_empty() {
        return None;
    }
    Some(format!(
        r#"<span class="moss-card-credits">{}</span>"#,
        credits.join(r#"<span class="moss-event-sep"> · </span>"#)
    ))
}
