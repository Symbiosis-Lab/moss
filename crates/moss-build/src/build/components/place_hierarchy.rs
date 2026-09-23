//! A place page's breadcrumb (up its parent chain) and children-with-counts
//! (its direct descendants) — pure rendering over data [`crate::build::terms`]
//! has already resolved. Mirrors `term_member_groups`'s own "takes no
//! `TermIndex`" design: both functions here take only the already-resolved
//! pairs, never the index itself, so the claiming page (reading its own
//! `ParsedDocument` fields) and the generated page (reading `TermIndex`)
//! can share one renderer without either holding a reference the other
//! lacks.
//!
//! No gate on `kind.is_place` anywhere in this file: a non-place kind's
//! terms simply never have breadcrumb pairs or children recorded (`terms.rs`
//! only ever fills `TermSite::parent` for a place-typed kind), so the
//! emptiness check below is the only gate needed.

use crate::build::media::cover::html_escape;

/// `<nav class="moss-place-breadcrumb">`, one `.breadcrumb-segment` per
/// ancestor with a `.breadcrumb-separator` between them — the same classes
/// the masthead trail already ships, reused here rather than duplicated.
/// `None` for an empty chain (a root place, or any non-place term).
pub fn render_breadcrumb(pairs: &[(String, String)]) -> Option<String> {
    if pairs.is_empty() {
        return None;
    }
    let separator = r#"<span class="breadcrumb-separator">/</span>"#;
    let segments: Vec<String> = pairs
        .iter()
        .map(|(display, url)| {
            format!(
                r#"<a href="{}" class="breadcrumb-segment">{}</a>"#,
                html_escape(url),
                html_escape(display),
            )
        })
        .collect();
    Some(format!(r#"<nav class="moss-place-breadcrumb">{}</nav>"#, segments.join(separator)))
}

/// `<ul class="moss-place-children">`, one `<li>` per direct child with its
/// roll-up-inclusive member count. `None` for no children (a leaf place, or
/// any non-place term).
pub fn render_children(children: &[(String, String, usize)]) -> Option<String> {
    if children.is_empty() {
        return None;
    }
    let items: String = children
        .iter()
        .map(|(display, url, count)| {
            format!(
                r#"<li><a href="{}">{} ({})</a></li>"#,
                html_escape(url),
                html_escape(display),
                count,
            )
        })
        .collect();
    Some(format!(r#"<ul class="moss-place-children">{}</ul>"#, items))
}

#[cfg(test)]
#[path = "place_hierarchy_tests.rs"]
mod tests;
