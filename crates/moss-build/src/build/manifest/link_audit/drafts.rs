//! Links from a published page to a `draft: true` page.
//!
//! A draft is built for preview and never published, so such a link is a
//! certain 404 on the live site, not a maybe. Read from the parsed documents
//! rather than the sealed output, because it must be known when the build's
//! publish evidence is installed.

use std::collections::HashMap;

use super::{candidate_keys, url_attr_values};
use crate::build::render::DraftLinkOccurrence;
use crate::build::types::ParsedDocument;

/// Root-relative candidates for `href` as written in a page at `page_key`
/// (`notes/about/index.html`), resolving a relative href against the page's
/// directory.
fn candidates_from(page_key: &str, href: &str) -> Option<Vec<String>> {
    if href.starts_with('/') {
        return candidate_keys(href);
    }
    let path = href.split(['?', '#']).next().unwrap_or("");
    let first = path.split('/').next().unwrap_or("");
    if path.is_empty() || first.contains(':') {
        return None;
    }
    let mut segments: Vec<&str> = page_key.split('/').collect();
    segments.pop();
    for seg in path.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                segments.pop()?;
            }
            seg => segments.push(seg),
        }
    }
    let trailing = if path.ends_with('/') { "/" } else { "" };
    candidate_keys(&format!("/{}{trailing}", segments.join("/")))
}

/// The first line of `source` that writes a link to `draft_source`, or that
/// carries `reference` verbatim.
fn link_line(source: &str, draft_source: &str, reference: &str) -> Option<u32> {
    let path = std::path::Path::new(draft_source);
    let mut stem = path.file_stem()?.to_string_lossy().to_lowercase();
    if stem == "index" {
        stem = path.parent()?.file_name()?.to_string_lossy().to_lowercase();
    }
    let line = source.lines().position(|line| {
        let lower = line.to_lowercase();
        (lower.contains("[[") || lower.contains("](")) && lower.contains(&stem)
    });
    let line = line.or_else(|| source.lines().position(|line| line.contains(reference)))?;
    u32::try_from(line + 1).ok()
}

/// Every link from a published document to a draft, sorted by source and
/// reference. A draft linking to a draft is ignored: both are withheld.
/// `read_source` returns a document's source text, for the line number.
pub fn drafts_linked(
    documents: &[ParsedDocument],
    read_source: impl Fn(&str) -> Option<String>,
) -> Vec<DraftLinkOccurrence> {
    let drafts: HashMap<&str, &str> = documents
        .iter()
        .filter(|doc| doc.is_draft() && !doc.slot_only)
        .filter_map(|doc| Some((doc.url_path.as_str(), doc.source_path.as_deref()?)))
        .collect();
    if drafts.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for doc in documents.iter().filter(|doc| !doc.is_draft() && !doc.slot_only) {
        let Some(source_path) = doc.source_path.as_deref() else { continue };
        let mut seen = std::collections::HashSet::new();
        for href in url_attr_values(&doc.html_content) {
            let Some(keys) = candidates_from(&doc.url_path, &href) else { continue };
            let Some(draft_source) = keys.iter().find_map(|key| drafts.get(key.as_str())) else { continue };
            if !seen.insert(href.clone()) {
                continue;
            }
            let line = read_source(source_path).and_then(|text| link_line(&text, draft_source, &href));
            found.push(DraftLinkOccurrence {
                source_path: source_path.to_string(),
                reference: href,
                draft_source: draft_source.to_string(),
                line,
            });
        }
    }
    found.sort_by(|a, b| (&a.source_path, &a.reference).cmp(&(&b.source_path, &b.reference)));
    found
}
