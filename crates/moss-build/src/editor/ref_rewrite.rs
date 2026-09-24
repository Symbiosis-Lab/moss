//! Pure text-rewriting layer behind rename-with-refs and delete-with-refs.
//!
//! Everything here is a function of `(source, old, new)` — no filesystem, no
//! Tauri state, no walking. `ref_scan.rs` owns the I/O side (walk the project,
//! read, write, trash) and calls into this module for the bytes.
//!
//! Split out of `ref_scan.rs` when that file crossed the 800-line ratchet: the
//! seam was already there, since these are exactly the functions the unit
//! tests drive directly.

use moss_core::resolve::md_extract::{
    extract_md_references, extract_structural_asset_refs, PathContainer,
};
use moss_core::resolve::reference::{classify_reference, ReferenceContext};
use std::path::Path;

/// Return true when `resolved_target_path` (root-relative) matches the scan target.
///
/// - For a FILE target (`target_root_rel` = e.g. `"posts/note.md"`):
///   exact string equality.
/// - For a FOLDER target (`target_root_rel` = e.g. `"posts"`, no trailing `/`):
///   resolved path equals the folder OR starts with `folder + "/"`.
///   This catches `[[sub/note]]` and `![[sub/img.png]]` refs whose
///   resolved `target_path` is inside the folder.
pub(crate) fn matches_target(resolved: &str, target_root_rel: &str, target_is_dir: bool) -> bool {
    if target_is_dir {
        resolved == target_root_rel
            || resolved.starts_with(&format!("{}/", target_root_rel))
    } else {
        resolved == target_root_rel
    }
}

/// One pending source edit.
#[derive(Debug, Clone)]
pub(crate) struct Edit {
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) text: String,
}

/// Apply `edits` to `source`.
///
/// **Earlier edits in the list win on overlap** — a later edit intersecting a
/// range already kept is dropped. That single rule is how the two callers
/// express opposite preferences with the same function: rename lists the
/// narrow token edits first (syntax preserved), removal lists the wide
/// structural `outer` edits first (the whole line disappears).
///
/// Total: an edit with `from > to`, out of bounds, or bounds that are not
/// char boundaries is dropped rather than panicking. The hand-rolled
/// `sort + replace_range` loops this replaces could be handed two identical
/// ranges and panic mid-delete, leaving the file half-written.
pub(crate) fn apply_edits(source: &str, edits: Vec<Edit>) -> String {
    let mut kept: Vec<Edit> = Vec::new();
    for e in edits {
        if e.from > e.to
            || e.to > source.len()
            || !source.is_char_boundary(e.from)
            || !source.is_char_boundary(e.to)
        {
            continue;
        }
        // Zero-width edits at the same point don't conflict; anything with
        // overlapping interior does.
        if kept
            .iter()
            .any(|k| e.from < k.to && k.from < e.to || (e.from == e.to && e.from > k.from && e.from < k.to))
        {
            continue;
        }
        kept.push(e);
    }
    if kept.is_empty() {
        return source.to_string();
    }
    kept.sort_by(|a, b| b.from.cmp(&a.from));
    let mut result = source.to_string();
    for e in kept {
        result.replace_range(e.from..e.to, &e.text);
    }
    result
}

/// Rebuild a structural (syntax-free) value for writing back into the source.
pub(crate) fn render_bare_value(
    container: &PathContainer,
    quote: Option<char>,
    path: &str,
    attrs: &str,
) -> String {
    let body = if attrs.is_empty() {
        path.to_string()
    } else {
        format!("{path}|{attrs}")
    };
    match quote.or_else(|| required_quote(container, &body)) {
        None => body,
        Some('\'') => format!("'{}'", body.replace('\'', "''")),
        Some(q) => format!("{q}{}{q}", body.replace('\\', "\\\\").replace('"', "\\\"")),
    }
}

/// Does `body` need quoting in `container`, given it was unquoted before?
pub(crate) fn required_quote(container: &PathContainer, body: &str) -> Option<char> {
    match container {
        // Deliberately calls `moss_core::ast::attrs::is_bareword` rather than
        // keeping a second copy of the bareword char class here — the two
        // drifting apart is exactly what let an unquoted non-ASCII
        // `image=頭像.png` silently vanish from rename tracking while the
        // shortcode grammar's own parser (this same function's source of
        // truth) still rejected it too. It has NO single-quote form, so `"`
        // is the only option here.
        PathContainer::ShortcodeAttr { .. } | PathContainer::HeroDirective => body
            .chars()
            .any(|c| !moss_core::ast::attrs::is_bareword(c))
            .then_some('"'),
        // A filename with interior spaces needs no quoting in YAML, so the
        // common case stays unquoted and the diff stays minimal.
        PathContainer::FrontmatterField { .. } => (body.is_empty()
            || body.trim() != body
            || body.contains(": ")
            || body.contains(" #")
            || body
                .chars()
                .next()
                .is_some_and(|c| "#-?:,[]{}&*!|>'\"%@`".contains(c)))
        .then_some('\''),
        // A gallery / hero body line is raw text.
        PathContainer::GalleryBody | PathContainer::HeroBodyMedia => None,
    }
}


/// Rewrite source by removing (emptying) all reference tokens that resolve to `old_target_root_rel`.
pub(crate) fn rewrite_for_removal(
    source: &str,
    from_source: &str,
    old_target_root_rel: &str,
    ctx: &ReferenceContext<'_>,
    target_is_dir: bool,
) -> String {
    let mut edits: Vec<Edit> = Vec::new();
    let hit = |text: &str| -> bool {
        classify_reference(text, from_source, true, ctx)
            .target_path
            .as_deref()
            .is_some_and(|p| matches_target(p, old_target_root_rel, target_is_dir))
    };

    // STRUCTURAL FIRST: on overlap the wide `outer` edit wins, so a gallery
    // line disappears whole instead of leaving a blank line or an orphan
    // `|attrs` behind.
    for span in extract_structural_asset_refs(source) {
        if hit(&span.path) {
            edits.push(Edit { from: span.outer.start, to: span.outer.end, text: String::new() });
        }
    }
    let md_refs = extract_md_references(source);
    for rr in &md_refs {
        if !hit(&rr.text) {
            continue;
        }
        // A nested reference (`[![[gone.png]]](/album/)`) is removed together
        // with the token that encloses it — deleting only the inner embed
        // would leave the author an empty `[](/album/)`. Same rule the
        // structural-first ordering above expresses: on a delete, the widest
        // construct wins.
        let (from, to) = md_refs
            .iter()
            .filter(|o| o.byte_from <= rr.byte_from && rr.byte_to <= o.byte_to)
            .map(|o| (o.byte_from, o.byte_to))
            .max_by_key(|(f, t)| t - f)
            .unwrap_or((rr.byte_from, rr.byte_to));
        edits.push(Edit { from, to, text: String::new() });
    }

    apply_edits(source, edits)
}


/// Does `text` point at the renamed entry? Returns the replacement ref text.
///
/// One matcher for BOTH passes — generic markdown tokens and structural
/// asset spans — which is why `AssetPathSpan` deliberately has no matching
/// rules of its own.
///
/// `from_dir` is the root-relative directory of the referencing file (`""`
/// at the project root).
pub(crate) fn match_and_retarget(
    text: &str,
    from_dir: &str,
    old_root_rel: &str,
    new_root_rel: &str,
    target_is_dir: bool,
) -> Option<String> {
    let text_no_anchor = text.split_once('#').map_or(text, |(head, _)| head);
    let text_ref = text_no_anchor
        .split_once('|')
        .map_or(text_no_anchor, |(head, _)| head);

    // ── Attempt 1: root-relative (today's rules, byte for byte) ──────────
    if let Some(new_text) = retarget_root_relative(text_ref, old_root_rel, new_root_rel, target_is_dir) {
        return Some(new_text);
    }

    // ── Attempt 2: root-anchored (`/assets/hero.png`) ────────────────────
    // This is the form `link_completions::insert_for` writes whenever the
    // accepted candidate lives outside the source file's own subtree, so it is
    // the shape the completion popup puts in a user's file most often. Before
    // this arm, every one of those refs was invisible to rename and left
    // dangling with no report — while DELETE already handled them, because
    // `classify_reference` strips the leading `/` (resolve/reference.rs).
    //
    // The authored form is preserved: a ref written `/assets/hero.png` comes
    // back `/assets/banner.png`, never silently re-spelled document-relative.
    // A trailing slash (a folder embed, `![[/awards/|style:grid]]`) survives
    // too — dropping it would turn a folder listing into a file lookup.
    if let Some(rest) = text_ref.strip_prefix('/') {
        let trailing_slash = rest.ends_with('/');
        let abs = normalize_rel(rest)?;
        let new_abs =
            retarget_root_relative(&abs, old_root_rel, new_root_rel, target_is_dir)?;
        let suffix = if trailing_slash { "/" } else { "" };
        return Some(format!("/{new_abs}{suffix}"));
    }

    // ── Attempt 3: document-relative ─────────────────────────────────────
    // Only fires where the arms above returned None, so no match that works
    // today can regress. Without it, the harbor corpus's actual shape — a
    // gallery in a subfolder referencing `關於/x.png` — keeps breaking
    // silently, which is the reported bug wearing a different label.
    if from_dir.is_empty() {
        return None;
    }
    let abs = normalize_rel(&format!("{from_dir}/{text_ref}"))?;
    let new_abs = retarget_root_relative(&abs, old_root_rel, new_root_rel, target_is_dir)?;
    // Re-emit document-relative when the new location is still under
    // `from_dir`; otherwise fall back to the root-relative form.
    Some(
        new_abs
            .strip_prefix(&format!("{from_dir}/"))
            .unwrap_or(&new_abs)
            .to_string(),
    )
}

/// Produce a same-shaped replacement for `text_ref`, given it is already
/// known (by the caller, via the resolver) to name `old_root_rel`. A FORMATTER,
/// not a matcher: it never decides WHETHER a reference points at the renamed
/// entry — the caller resolves that with `classify_reference` first — only
/// HOW to spell the new target in the same shape the author wrote. `None`
/// means this shape can't express `old_root_rel` at all (the caller escalates
/// to an explicit document-relative or root-absolute form instead).
pub(crate) fn retarget_root_relative(
    text_ref: &str,
    old_root_rel: &str,
    new_root_rel: &str,
    target_is_dir: bool,
) -> Option<String> {
    if target_is_dir {
        // Folder rename: any ref whose path starts with the old folder.
        let old_folder = old_root_rel.trim_end_matches('/');
        let new_folder = new_root_rel.trim_end_matches('/');
        if text_ref == old_folder || text_ref.starts_with(&format!("{old_folder}/")) {
            let suffix = text_ref.strip_prefix(old_folder).unwrap_or_default();
            return Some(format!("{new_folder}{suffix}"));
        }
        return None;
    }

    if text_ref.contains('/') {
        // Path-qualified ref: exact path match, with or without extension.
        // Preserve the original ref's extension-presence.
        if text_ref == old_root_rel {
            return Some(new_root_rel.to_string());
        }
        let old_no_ext = Path::new(old_root_rel)
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        if text_ref == old_no_ext {
            return Some(
                Path::new(new_root_rel)
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
        return None;
    }

    // Bare ref, no path component. Whether it names a FILE or a markdown
    // stem is decided by whether it carries an extension.
    let old_name = Path::new(old_root_rel)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(old_root_rel);
    if Path::new(text_ref).extension().is_some() {
        // Carries an extension → it names a file. Require FULL-NAME equality
        // and emit the new full name. Matching on stem here would repoint a
        // `photo.png` reference at `new.jpg` when `photo.jpg` was renamed —
        // a DIFFERENT file, silently.
        let new_name = Path::new(new_root_rel)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(new_root_rel);
        (text_ref == old_name).then(|| new_name.to_string())
    } else {
        // Extensionless bare ref — a wikilink into the markdown name space.
        let old_stem = Path::new(old_root_rel)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(old_root_rel);
        let new_stem = Path::new(new_root_rel)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(new_root_rel);
        (text_ref == old_stem).then(|| new_stem.to_string())
    }
}

/// Resolve `.`/`..` segments in a root-relative path. `None` if it escapes
/// the project root.
pub(crate) fn normalize_rel(path: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            s => out.push(s),
        }
    }
    Some(out.join("/"))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "ref_rewrite_tests.rs"]
mod tests;
