//! A folder's `color:` as the accent of its section.
//!
//! The home page of a folder may declare `color:`. Every page in that folder,
//! its own home page included, then carries the colour (`ParsedDocument::section`),
//! and the nearest coloured ancestor wins, so a nested folder can recolour its
//! own subtree. An invalid value is reported once, against the page that wrote
//! it, and ignored.

use crate::build::scan::section_text;
use crate::build::types::{ParsedDocument, SectionAccent};
use moss_core::PageKind;

/// Resolve `ParsedDocument::section` for every document from its folder
/// homes' `color:`. Runs once per build, over the whole document set.
pub(crate) fn apply_section_colors(docs: &mut [ParsedDocument]) {
    // (folder prefix such as "work/" or "" for the root, accent), deepest last.
    let mut folders: Vec<(String, SectionAccent)> = Vec::new();
    for doc in docs.iter() {
        let Some(raw) = doc.color.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
            continue;
        };
        if doc.kind != PageKind::Folder {
            continue;
        }
        let Some(prefix) = doc.url_path.strip_suffix("index.html") else {
            continue;
        };
        let Some(accent) = section_text::accent(raw, String::new()) else {
            let page = doc.source_path.as_deref().unwrap_or(doc.url_path.as_str());
            log::warn!("[{page}] `color: {raw}` is not a hex colour (#rgb or #rrggbb), so it is ignored");
            continue;
        };
        let segment = prefix.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        folders.push((prefix.to_string(), SectionAccent { segment, ..accent }));
    }
    if folders.is_empty() {
        return;
    }
    folders.sort_by_key(|(prefix, _)| prefix.len());
    for doc in docs.iter_mut() {
        doc.section = folders
            .iter()
            .rev()
            .find(|(prefix, _)| doc.url_path.starts_with(prefix.as_str()))
            .map(|(_, accent)| accent.clone());
    }
}
