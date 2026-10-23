//! The note a home page gets when it embeds the map of a place root that is also a nav item.

use super::{marker_decode, parse_marker_body, MARKER_END, MARKER_FOLDER_LIST};
use crate::build::types::ParsedDocument;

/// A place root (`key`) the nav links to, and what puts it there.
pub(crate) struct NavRoot {
    pub key: String,
    pub via: String,
}

/// Roots in the nav: kinds with `nav = true` (`term_nav`), and real place-root pages with `nav: true`.
fn nav_roots(term_nav: &[crate::build::terms::TermNavEntry], documents: &[ParsedDocument]) -> Vec<NavRoot> {
    let kinds = term_nav
        .iter()
        .filter(|entry| entry.edition.is_empty())
        .map(|entry| NavRoot { key: entry.root.clone(), via: format!("`nav = true` in [terms.{}]", entry.root) });
    let pages = documents.iter().filter(|d| d.is_place_namespace_root && d.nav == Some(true)).filter_map(|d| {
        let key = d.url_path.strip_suffix("/index.html")?.to_string();
        let page = d.source_path.as_deref().unwrap_or(d.url_path.as_str()).to_string();
        Some(NavRoot { key, via: format!("`nav: true` in {page}") })
    });
    kinds.chain(pages).collect()
}

/// A destination shows once: the home page's embedded map of a root in the nav is a second way to it.
pub(crate) fn note_home_map_of_nav_root(documents: &[ParsedDocument], term_nav: &[crate::build::terms::TermNavEntry]) {
    let roots = nav_roots(term_nav, documents);
    let Some(home) = documents.iter().find(|d| d.url_path == "index.html") else { return };
    let page = home.source_path.as_deref().unwrap_or("index.md");
    let mut named: Vec<&str> = Vec::new();
    for rest in home.html_content.split(MARKER_FOLDER_LIST).skip(1) {
        let Some(parsed) = rest.split_once(MARKER_END).and_then(|(body, _)| parse_marker_body(body)) else { continue };
        if parsed.style.as_deref() != Some("map") {
            continue;
        }
        let target = marker_decode(parsed.path);
        let Some(root) = roots.iter().find(|r| r.key == target.trim_matches('/')) else { continue };
        if named.contains(&root.key.as_str()) {
            continue;
        }
        named.push(&root.key);
        crate::build::cli_output::log_note!(
            "[{page}] the home page embeds the map of `{key}` and `{key}` is also a nav item ({via}); a destination shows once, so keep one of the two.",
            key = root.key,
            via = root.via,
        );
    }
}
