//! Folder-listing embed: ![[/folder/|limit:N,sort:axis]]
//!
//! Pure-Rust path parsing + marker emission. The actual children
//! lookup + sort + HTML render happens in src-tauri (which has I/O).
//!
//! See docs/archive/2026-05-17-listing-sort-and-embeds-design.md.

use crate::resolve::embed_renderer::Sizing;
use crate::sort::SortAxis;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FolderEmbedParams {
    pub limit: Option<usize>,
    pub sort: Option<SortAxis>,
    pub style: Option<String>,   // "list" | "summary" | "grid"
    pub depth: Option<String>,   // "direct" | "all"
    pub group: Option<String>,   // "year" | "none"
    /// Listing filter: "only" keeps pages that have a cover. Applied after
    /// flattening (`depth:all`) and before `limit`, so a capped listing
    /// counts the limit off the covered set rather than the full set.
    pub covers: Option<String>,
    /// Raw sizing token (e.g. `"80%"`, `"800x600"`). Parsed to a `Sizing`
    /// at render time and applied ONLY to the static-index iframe branch
    /// (the card-grid listing branch ignores it). Stored raw so the
    /// pothole→marker→render round-trip stays a plain string.
    pub size: Option<String>,
    /// `more:<target>` — a wikilink-or-plain-name reference to the page the
    /// truncated "More →" link should point at, resolved in moss-build
    /// against `all_docs` the same way `children_more` frontmatter is
    /// (moss-core has no document set to resolve against). Lets a body
    /// embed (`![[/|more:Archive]]`) name a target the way `children_more`
    /// frontmatter already can; `synthesize_children_marker` sets this
    /// field from `children_more` too, so both sources share one field.
    pub more: Option<String>,
    /// Internal: this is the root homepage's own depth=all self-listing, so scope it
    /// to the default language tree — on a multilingual site (gated at render time by
    /// `ProjectStructure.has_language_trees`) drop docs under a language-prefix folder
    /// (`en/`, …), leaving only the default tree. Set by `synthesize_children_marker`
    /// for homepage default-mode; not user-facing. Co-set with `exclude_nav` (same
    /// condition) but kept separate as a distinct concern.
    pub scope_default_tree: bool,
    /// Internal: exclude folder pages that act as top-level nav items.
    /// Set by synthesize_children_marker for homepage default-mode; not user-facing.
    pub exclude_nav: bool,
}

/// Parse pipe-encoded params from the portion after `|`.
///
/// Format: `key:value,key:value` (e.g. `limit:5,sort:date`).
/// Unknown keys are silently ignored.
pub fn parse_params(raw: &str) -> FolderEmbedParams {
    let mut out = FolderEmbedParams::default();
    for tok in raw.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        if let Some((k, v)) = tok.split_once(':') {
            match k.trim() {
                "limit" => out.limit = v.trim().parse().ok(),
                "sort" => {
                    out.sort = match v.trim() {
                        "date" => Some(SortAxis::Date),
                        "weight" => Some(SortAxis::Weight),
                        "title" => Some(SortAxis::Title),
                        _ => None,
                    }
                }
                "style" => out.style = Some(v.trim().to_string()),
                "depth" => out.depth = Some(v.trim().to_string()),
                "group" => out.group = Some(v.trim().to_string()),
                "covers" => out.covers = Some(v.trim().to_string()),
                "more" => {
                    let v = v.trim();
                    if !v.is_empty() {
                        out.more = Some(v.to_string());
                    }
                }
                _ => {}
            }
        } else if is_size_token(tok) {
            // A bare token that is unambiguously a sizing hint (ends in `%`,
            // `px`, `vh`, or is `<dim>x<dim>`) — but NOT a bare integer, which
            // stays a no-op bare flag so it never shadows `limit:N`. This is
            // the only place size enters the pothole grammar; all the keyed
            // params above carry a `:` and never reach this branch.
            out.size = Some(tok.to_string());
        }
        // unknown bare flags (e.g. a bare "more" with no `:target`) silently ignored
    }
    out
}

/// Whether a bare pothole token is unambiguously a sizing hint.
///
/// True only when the token is NOT an all-ASCII-digit integer AND
/// `Sizing::parse` accepts it. The digit guard is what keeps a bare `5`
/// (which `Sizing::parse` would read as `5px`) from being mistaken for a
/// size — bare integers stay no-op flags, leaving `limit:N` the sole way
/// to set a limit.
fn is_size_token(tok: &str) -> bool {
    if tok.is_empty() {
        return false;
    }
    let all_digits = tok.bytes().all(|b| b.is_ascii_digit());
    !all_digits && Sizing::parse(tok).is_some()
}

/// Marker prefix for folder-list embeds emitted by moss-core.
/// The src-tauri marker resolver (Task 16) reads everything between the prefix
/// and the terminator as `path=...|from=...|limit=N|more=target|sort=axis`. The
/// `path` is the user-written target (which may carry a leading `/`); `from` is
/// the source markdown file path, used for resolving relative paths against the
/// current document's location.
pub const MARKER_FOLDER_LIST: &str = "<!--MOSS_MARKER_FOLDER_LIST:";
pub const MARKER_END: &str = "-->";

pub fn emit_marker(path: &str, from: &str, params: &FolderEmbedParams) -> String {
    let mut parts = vec![format!("path={}", path), format!("from={}", from)];
    if let Some(ref s) = params.style {
        parts.push(format!("style={}", s));
    }
    if let Some(ref d) = params.depth {
        parts.push(format!("depth={}", d));
    }
    if let Some(ref g) = params.group {
        parts.push(format!("group={}", g));
    }
    if let Some(ref c) = params.covers {
        parts.push(format!("covers={}", c));
    }
    if let Some(ref m) = params.more {
        parts.push(format!("more={}", m));
    }
    if let Some(ref sz) = params.size {
        parts.push(format!("size={}", sz));
    }
    if let Some(n) = params.limit {
        parts.push(format!("limit={}", n));
    }
    if params.scope_default_tree {
        parts.push("scope_default_tree".to_string());
    }
    if params.exclude_nav {
        parts.push("exclude_nav".to_string());
    }
    if let Some(s) = params.sort {
        parts.push(format!(
            "sort={}",
            match s {
                SortAxis::Date => "date",
                SortAxis::Weight => "weight",
                SortAxis::Title => "title",
            }
        ));
    }
    format!("{}{}{}", MARKER_FOLDER_LIST, parts.join("|"), MARKER_END)
}

#[cfg(test)]
#[path = "folder_list_tests.rs"]
mod tests;
