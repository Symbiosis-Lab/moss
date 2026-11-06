//! `color:` as the accent of a section.
//!
//! The home page of a folder may declare `color:`. Every page in that folder,
//! its own home page included, then carries the colour (`ParsedDocument::section`),
//! and the nearest coloured ancestor wins, so a nested folder can recolour its
//! own subtree. An ordinary page that declares `color:` carries it itself, over
//! any colour the folder gives, and no other page does. An invalid value is
//! reported once, against the page that wrote it, and ignored.

use crate::build::scan::article_map::to_pretty_url;
use crate::build::scan::section_text;
use crate::build::types::{ParsedDocument, SectionAccent};
use moss_core::PageKind;

/// Resolve `ParsedDocument::section` for every document from its folder
/// homes' `color:` and its own. Runs once per build, over the whole document set.
pub(crate) fn apply_section_colors(docs: &mut [ParsedDocument]) {
    // (folder prefix such as "work/" or "" for the root, accent), deepest last.
    let mut folders: Vec<(String, SectionAccent)> = Vec::new();
    // An ordinary page's own accent, by index into `docs`.
    let mut own: Vec<(usize, SectionAccent)> = Vec::new();
    for (index, doc) in docs.iter().enumerate() {
        let Some(raw) = doc.color.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
            continue;
        };
        let is_folder = doc.kind == PageKind::Folder;
        let prefix = doc.url_path.strip_suffix("index.html").filter(|_| is_folder);
        if is_folder && prefix.is_none() {
            continue;
        }
        let Some(accent) = section_text::accent(raw, String::new()) else {
            let page = doc.source_path.as_deref().unwrap_or(doc.url_path.as_str());
            log::warn!("[{page}] `color: {raw}` is not a hex colour (#rgb or #rrggbb), so it is ignored");
            continue;
        };
        let path = prefix.map_or_else(|| to_pretty_url(&doc.url_path), str::to_string);
        let segment = path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        let accent = SectionAccent { segment, ..accent };
        match prefix {
            Some(prefix) => folders.push((prefix.to_string(), accent)),
            None => own.push((index, accent)),
        }
    }
    folders.sort_by_key(|(prefix, _)| prefix.len());
    for doc in docs.iter_mut() {
        doc.section = folders
            .iter()
            .rev()
            .find(|(prefix, _)| doc.url_path.starts_with(prefix.as_str()))
            .map(|(_, accent)| accent.clone());
    }
    // A page's own colour replaces only the colour: `data-section` keeps naming
    // the coloured folder the page sits in, and the page's own name stands in
    // only where no folder is coloured.
    for (index, accent) in own {
        let segment = docs[index].section.as_ref().map_or(accent.segment.clone(), |folder| folder.segment.clone());
        docs[index].section = Some(SectionAccent { segment, ..accent });
    }
}
