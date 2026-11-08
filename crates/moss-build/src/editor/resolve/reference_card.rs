//! `editor_reference_card` — the reference card's one backend need: a page
//! link's title, description, source path, generated flag, and (for an
//! anchor) the matching heading's text. Everything else the card shows
//! (asset preview, footnote note, table render, external URL, unresolved/
//! moved/ambiguous diagnostics) is already in the frontend's resolved-
//! reference cache and needs no round trip.

use crate::build::page::meta::resolve_page_description;
use crate::build::scan::path_collision::{logical_path, on_disk};
use crate::editor::content::{compute_heading_state_inner, parse_frontmatter_of, read_vault_text};
use crate::editor::resolve::asset_resolver::EditorScope;
use crate::editor::resolve::reference_resolver::{resolve_references_batch, RefTarget};
use moss_core::heading::extract_headings;
use std::path::Path;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, specta::Type)]
pub struct ReferenceCardInfo {
    pub title: String,
    pub description: Option<String>,
    pub source_path: Option<String>,
    pub generated: bool,
    pub heading: Option<String>,
}

/// Resolve the reference-card content for `target`, authored in `from_file`.
///
/// Two shapes:
///   - a bare `#anchor` (no page part) — the SAME-DOCUMENT case. Never asks
///     `classify_reference`: a leading `#` classifies to the empty
///     `ReferenceKind::Anchor` with nothing to look up, because the
///     classifier has no notion of "the file this reference lives in".
///     Read `from_file` directly instead.
///   - anything else — a page link, answered by `resolve_references_batch`
///     itself, so a card can never show a page the editor's own lint calls
///     broken, and on a site never built it opens the same source the lint
///     follows.
pub fn reference_card_for(
    target: &str,
    from_file: &str,
    project_root: &Path,
) -> Result<ReferenceCardInfo, String> {
    let canonical_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());

    if let Some(anchor) = target.strip_prefix('#') {
        let heading = heading_text_for_anchor(&canonical_root.join(from_file), anchor)?;
        return Ok(ReferenceCardInfo {
            title: heading.clone().unwrap_or_else(|| target.to_string()),
            description: None,
            source_path: Some(from_file.to_string()),
            generated: false,
            heading,
        });
    }

    let link = RefTarget { text: target.to_string(), is_embed: false };
    let resolved = resolve_references_batch(&[link], from_file, &canonical_root, EditorScope::Site).remove(0);
    let source_path = resolved.resolved_path.as_deref().map(|abs| {
        Path::new(abs).strip_prefix(&canonical_root).map_or(abs.to_string(), logical_path)
    });
    let anchor = resolved.anchor;
    // A resolved URL with no known source is exactly what the frontend's own
    // `followTarget` already treats as "goes to the preview, not a file" —
    // the build synthesized this page (an index-less folder listing, a
    // generated author/tag page).
    let generated = resolved.url.is_some() && source_path.is_none();

    let Some(rel) = source_path.clone() else {
        return Ok(ReferenceCardInfo {
            title: target.to_string(),
            description: None,
            source_path: None,
            generated,
            heading: None,
        });
    };

    let content = read_vault_text(&on_disk(&canonical_root, &rel).to_string_lossy())?
        .ok_or_else(|| format!("could not read {rel}"))?;
    let parsed = parse_frontmatter_of(content)?;
    let title_str = json_str(&parsed.frontmatter, "title");
    let description_fm = json_str(&parsed.frontmatter, "description");
    let heading_state = compute_heading_state_inner(&rel, &parsed.frontmatter, &parsed.body);
    let heading = anchor.as_deref().and_then(|slug| heading_text_for_slug(&parsed.body, slug));
    // moss's own `[site].math` setting changes only whether `$…$` is read as
    // math in the excerpt; a card is not worth a full config load for that
    // one bit, so it defaults false (the excerpt falls back to plain text).
    let description = resolve_page_description(description_fm.as_deref(), &parsed.body, false);

    Ok(ReferenceCardInfo {
        title: if heading_state.visible { heading_state.text } else { title_str.unwrap_or(rel.clone()) },
        description,
        source_path: Some(rel),
        generated,
        heading,
    })
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|s| s.as_str()).map(str::to_string)
}

fn heading_text_for_slug(body: &str, slug: &str) -> Option<String> {
    extract_headings(body).into_iter().find(|h| h.slug == slug).map(|h| h.text)
}

fn heading_text_for_anchor(file_abs: &Path, anchor: &str) -> Result<Option<String>, String> {
    match read_vault_text(&file_abs.to_string_lossy())? {
        Some(content) => Ok(heading_text_for_slug(&parse_frontmatter_of(content)?.body, anchor)),
        None => Ok(None),
    }
}

#[cfg(test)]
#[path = "reference_card_tests.rs"]
mod tests;
