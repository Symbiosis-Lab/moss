//! Folder-listing embed: ![[/folder/|limit:N,sort:axis]]
//!
//! Pure-Rust path parsing + marker emission. The actual children
//! lookup + sort + HTML render happens in the desktop app (which has I/O).

use crate::media::{extract_placement_from_alias, AlignSide, Placement};
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
    /// How wide the listing is and which way it floats — the same vocabulary
    /// every other embed kind reads, written in its own pipe segment
    /// (`![[/journal/|wide]]`). Distinct from [`Self::size`], which is the
    /// static-index iframe's own dimension token and predates this.
    pub placement: Placement,
    /// Caption for the listing, rendered as a `<figcaption>` on the figure
    /// that wraps it. Any segment that is neither a keyed param nor
    /// placement.
    pub caption: Option<String>,
}

/// Read a whole folder-embed pothole: keyed params, placement and caption,
/// in any order and any number of `|` segments.
///
/// A segment goes to the keyed grammar when it carries a `:`, is a bare
/// sizing token, or is entirely `key=value` pairs — `sort:date` and
/// `sort=date` mean the same thing. Otherwise it is placement if it reads as
/// placement, and the caption if it does not.
///
/// Splitting on `|` FIRST is the fix: the whole text after the first pipe
/// used to go to the comma grammar intact, so `style:grid|wide|A caption`
/// set `style` to the string `"grid|wide|A caption"`.
pub fn classify_folder_segments(raw: &str) -> FolderEmbedParams {
    let mut out = FolderEmbedParams::default();
    let mut caption: Vec<String> = Vec::new();
    for segment in raw.split('|') {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_keyed_segment(trimmed) {
            merge_keyed_params(&mut out, trimmed);
            continue;
        }
        let (placement, rest) = extract_placement_from_alias(trimmed);
        if !placement.is_empty() {
            out.placement.fill_from(placement);
            if !rest.trim().is_empty() {
                caption.push(rest);
            }
            continue;
        }
        caption.push(segment.to_string());
    }
    if !caption.is_empty() {
        out.caption = Some(caption.join("|").trim().to_string());
    }
    out
}

/// Recognized keys for the keyed (`key:value` / `key=value`) grammar.
const KNOWN_KEYS: &[&str] = &["limit", "sort", "style", "depth", "group", "covers", "more", "size"];

/// Whether a segment belongs to the keyed grammar rather than to placement
/// or the caption.
///
/// A `key:value`/`key=value` token must name a recognized key — otherwise a
/// colon in prose (`Note: 2024`) misfires as the keyed grammar and silently
/// eats the caption. A token with NEITHER `:` nor `=` is tolerated rather
/// than disqualifying: `merge_keyed_params` already treats a colon-less
/// token as a bare flag (a sizing hint, or silently ignored, per its own
/// comment), so `limit:3,more` must stay keyed with `more` a no-op, the same
/// as `merge_keyed_params` parses it standalone. At least one recognized
/// key is still required, or a plain caption ("wide cover", "This is nice")
/// would misfire as keyed since every one of its tokens is colon-less.
fn is_keyed_segment(segment: &str) -> bool {
    if is_size_token(segment) {
        return true;
    }
    let tokens: Vec<&str> = segment
        .split([',', ' '])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return false;
    }
    let mut has_known_key = false;
    for t in &tokens {
        match t.split_once([':', '=']) {
            Some((k, _)) if KNOWN_KEYS.contains(&k.trim()) => has_known_key = true,
            Some(_) => return false,
            None => {} // colon-less bare token — tolerated
        }
    }
    has_known_key
}

/// Parse pipe-encoded params from the portion after `|`.
///
/// Format: `key:value,key:value` (e.g. `limit:5,sort:date`).
/// Unknown keys are silently ignored.
pub fn parse_params(raw: &str) -> FolderEmbedParams {
    let mut out = FolderEmbedParams::default();
    merge_keyed_params(&mut out, raw);
    out
}

/// Fold one comma-separated run of `key:value` / `key=value` pairs into
/// `out`. Shared by the comma grammar and the per-segment classifier so the
/// two spellings cannot drift.
fn merge_keyed_params(out: &mut FolderEmbedParams, raw: &str) {
    for tok in raw.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        let split = tok.split_once(':').or_else(|| tok.split_once('='));
        if let Some((k, v)) = split {
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
/// The desktop app's marker resolver (Task 16) reads everything between the prefix
/// and the terminator as `path=...|from=...|limit=N|more=target|sort=axis`. The
/// `path` is the user-written target (which may carry a leading `/`); `from` is
/// the source markdown file path, used for resolving relative paths against the
/// current document's location.
pub const MARKER_FOLDER_LIST: &str = "<!--MOSS_MARKER_FOLDER_LIST:";
pub const MARKER_END: &str = "-->";

/// Percent-encode the characters that would otherwise end a marker field or
/// the marker itself.
///
/// `|` and `=` separate fields and keys; `,` separates the comma grammar;
/// `<` and `>` are encoded because a value containing `-->` would truncate
/// the HTML comment the marker lives in, dropping everything after it. `%`
/// goes first so decoding is unambiguous.
pub fn marker_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '|' => out.push_str("%7C"),
            '=' => out.push_str("%3D"),
            ',' => out.push_str("%2C"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            _ => out.push(ch),
        }
    }
    out
}

/// Inverse of [`marker_encode`]. An unrecognised or truncated `%` escape is
/// left as written rather than dropped.
pub fn marker_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        // `i` indexes a byte; push the whole char starting here.
        let ch = value[i..].chars().next().unwrap_or('%');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

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
        parts.push(format!("more={}", marker_encode(m)));
    }
    if let Some(w) = params.placement.width {
        parts.push(format!("width={}", w));
    }
    if let Some(a) = params.placement.align {
        parts.push(format!(
            "align={}",
            match a {
                AlignSide::Left => "left",
                AlignSide::Right => "right",
            }
        ));
    }
    // `pct`, not `size`: `size=` is already taken by the static-index
    // iframe's own dimension token, which means something else.
    if let Some(ref pct) = params.placement.size {
        parts.push(format!("pct={}", pct));
    }
    if let Some(ref c) = params.caption {
        parts.push(format!("caption={}", marker_encode(c)));
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
