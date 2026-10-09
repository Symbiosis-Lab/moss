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
    extract_md_references, extract_structural_asset_refs, PathContainer, RawRef, RefSyntax,
};
use moss_core::resolve::fuzzy_path::{escape_md_destination, percent_encode_path_segments};
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

/// Which Markdown construct a standard-form destination sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MdKind {
    Link,
    Image,
    Definition,
}

/// How a reference's destination is spelled in the source, which decides what
/// a rewritten destination has to escape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DestForm {
    /// `[t](dest)`, `![t](dest)` or `[id]: dest`; `angle` when the author
    /// wrote `<dest>`.
    Markdown { kind: MdKind, angle: bool },
    /// `[[target]]` / `![[target]]`: no escaping exists, so a name the form
    /// cannot carry is refused.
    Wiki,
    /// A structural value (frontmatter, shortcode attribute, gallery line);
    /// quoting is `render_bare_value`'s job.
    Bare,
}

/// How `rr`'s destination is spelled in `source` (see [`DestForm`]).
pub(crate) fn dest_form(source: &str, rr: &RawRef) -> DestForm {
    let angle = rr.ref_from > 0
        && source.as_bytes()[rr.ref_from - 1] == b'<'
        && source.as_bytes().get(rr.ref_to) == Some(&b'>');
    match rr.syntax {
        RefSyntax::MarkdownLink { .. } => DestForm::Markdown { kind: MdKind::Link, angle },
        RefSyntax::MarkdownImage { .. } => DestForm::Markdown { kind: MdKind::Image, angle },
        RefSyntax::Definition { .. } => DestForm::Markdown { kind: MdKind::Definition, angle },
        RefSyntax::StructuralAsset => DestForm::Bare,
        _ => DestForm::Wiki,
    }
}

/// Characters a wikilink target cannot carry: `|` starts an alias, `#` a
/// heading, `?` a query, `]` closes the link. A new name holding one cannot be
/// written in a `[[…]]` link at all.
pub(crate) const WIKI_UNWRITABLE: [char; 4] = ['|', '#', '?', ']'];

/// Spell `base` (the path part of a destination) plus its verbatim `?query` /
/// `#fragment` `suffix` as source text for `form`, or `None` when no spelling
/// of this form parses back as the same destination.
///
/// `percent_style` keeps an authored percent-encoded destination
/// (`my%20note.md`) percent-encoded. For the standard form the result is
/// checked by feeding it back through a real CommonMark parser AND this
/// crate's scanner: a rewrite that stops being a link, or is read as a
/// different destination, is refused here instead of reaching the file.
pub(crate) fn render_destination(form: DestForm, base: &str, suffix: &str, percent_style: bool) -> Option<String> {
    let encoded = |b: &str| if percent_style { percent_encode_path_segments(b) } else { b.to_string() };
    match form {
        DestForm::Bare => Some(format!("{}{suffix}", encoded(base))),
        DestForm::Wiki => {
            let text = format!("{}{suffix}", encoded(base));
            let carries = !encoded(base).contains(WIKI_UNWRITABLE);
            (carries && wikilink_round_trips(&text)).then_some(text)
        }
        DestForm::Markdown { kind, angle } => {
            let path = if percent_style { percent_encode_path_segments(base) } else { escape_md_destination(base, angle) };
            let text = format!("{path}{suffix}");
            markdown_round_trips(kind, angle, &text).then_some(text)
        }
    }
}

fn wikilink_round_trips(text: &str) -> bool {
    let src = format!("[[{text}]]");
    matches!(extract_md_references(&src).as_slice(), [r] if r.text == text)
}

fn markdown_round_trips(kind: MdKind, angle: bool, dest: &str) -> bool {
    use pulldown_cmark::{Event, Parser, Tag};
    let wrapped = if angle { format!("<{dest}>") } else { dest.to_string() };
    let src = match kind {
        MdKind::Link => format!("[x]({wrapped})"),
        MdKind::Image => format!("![x]({wrapped})"),
        MdKind::Definition => format!("[x]: {wrapped}\n\n[x]\n"),
    };
    let mut parsed: Vec<(bool, String)> = Vec::new();
    for ev in Parser::new(&src) {
        if let Event::Start(Tag::Link { dest_url, .. }) = &ev {
            parsed.push((false, dest_url.to_string()));
        } else if let Event::Start(Tag::Image { dest_url, .. }) = &ev {
            parsed.push((true, dest_url.to_string()));
        }
    }
    let parser_agrees = matches!(parsed.as_slice(), [(is_img, d)] if *is_img == (kind == MdKind::Image) && d == dest);
    let scanned = extract_md_references(&src);
    let scanner_agrees = matches!(scanned.as_slice(), [r] if r.text == dest);
    parser_agrees && scanner_agrees
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
/// Only the authored shapes that name the entry by its root-relative path or
/// by a bare name are matched here. A path written relative to the page is
/// spelled by [`exact_style`] / [`exact_spelling`], which also know how to
/// climb out of a folder with `../`.
pub(crate) fn match_and_retarget(
    text: &str,
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

    None
}

// ── Exact paths ─────────────────────────────────────────────────────────────
//
// A destination that names its file EXACTLY (relative to the page, relative to
// the root, or as a published address) must still name it exactly after the
// operation, in the same style. Other tools follow only such paths; the
// resolver's name search would hide a dead one. A reference that never named
// its file exactly (a bare name, a path that only matches by suffix) is not
// described by any of this and keeps the resolver-driven rule.

/// What a destination is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// Relative to the referencing page (`../x.md`, `x.md`, `./x.md`).
    Page,
    /// Leading `/`.
    Root,
    /// Root-relative without the slash: how a path-shaped wikilink is written.
    RootBare,
}

/// How the file is named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathForm {
    /// The file's own path, extension included (or the folder itself).
    Full,
    /// The file's path without its Markdown extension.
    NoExt,
    /// `/docs/`: the address of a folder's home page.
    AddrHome,
    /// `/blog/post/`: the address of an ordinary page.
    AddrPage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExactStyle {
    anchor: Anchor,
    form: PathForm,
    /// Authored with a trailing `/`.
    slash: bool,
    /// Authored with a leading `./`.
    dot: bool,
}

impl ExactStyle {
    /// Same way of naming the file, ignoring cosmetic `./` and `/`. The two
    /// address forms are one way: which of them fits depends on the target.
    pub(crate) fn same_kind(&self, other: &ExactStyle) -> bool {
        let is_address = |f: PathForm| matches!(f, PathForm::AddrHome | PathForm::AddrPage);
        self.anchor == other.anchor
            && (self.form == other.form || (is_address(self.form) && is_address(other.form)))
    }

    /// A root-anchored published address, written with or without its
    /// trailing `/`.
    pub(crate) fn root_address(slash: bool) -> ExactStyle {
        ExactStyle { anchor: Anchor::Root, form: PathForm::AddrPage, slash, dot: false }
    }

    /// A root-anchored published address (`/notes/beta/`), which the build
    /// keeps verbatim rather than resolving.
    pub(crate) fn is_root_address(&self) -> bool {
        self.anchor == Anchor::Root && matches!(self.form, PathForm::AddrHome | PathForm::AddrPage)
    }

    /// `address`, as served for `new_target`, in this style's trailing-slash
    /// habit: a folder's home written without the slash (`/docs`) stays
    /// slashless for as long as the target is still a home page.
    pub(crate) fn respell_address<'a>(&self, address: &'a str, new_target: &str) -> &'a str {
        if !self.slash && is_home_path(new_target) && address.len() > 1 {
            address.trim_end_matches('/')
        } else {
            address
        }
    }

    /// Root-relative without a leading slash, as a path-shaped wikilink is.
    pub(crate) fn is_root_bare(&self) -> bool {
        self.anchor == Anchor::RootBare
    }

    /// The same anchor naming the file by its full path, for a target that
    /// can no longer be named in this style. Only a path without its
    /// extension has one: an address never turns into a path, because the
    /// build keeps a leading-`/` destination verbatim and a `.md` path there
    /// is not served.
    pub(crate) fn full_path(&self) -> Option<ExactStyle> {
        (self.form == PathForm::NoExt).then_some(ExactStyle { form: PathForm::Full, slash: false, ..*self })
    }
}

pub(crate) fn dirname(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn strip_md_ext(path: &str) -> Option<&str> {
    path.strip_suffix(".md").or_else(|| path.strip_suffix(".markdown"))
}

/// Is `path` the page a folder address (`/dir/`) resolves to?
fn is_home_path(path: &str) -> bool {
    let Some(stem) = strip_md_ext(path) else { return false };
    let (dir, stem) = stem.rsplit_once('/').map_or(("", stem), |(d, s)| (d, s));
    moss_core::home::is_home_file(stem, dir.rsplit('/').next().unwrap_or(""))
}

/// Minimal `../`-relative spelling of `to_path` from `from_dir` (both
/// root-relative, filesystem shape). Deliberately NOT `fuzzy_path`'s
/// `relative_asset_path`: that percent-encodes segments for an HTML `href`,
/// and this text is written back into markdown SOURCE, where percent-encoding
/// would be a regression an author never asked for.
pub(crate) fn relative_root_path(from_dir: &str, to_path: &str) -> String {
    let from_parts: Vec<&str> = if from_dir.is_empty() { vec![] } else { from_dir.split('/').collect() };
    let to_parts: Vec<&str> = to_path.split('/').collect();
    let common = from_parts.iter().zip(to_parts.iter()).take_while(|(a, b)| a == b).count();
    let ups = from_parts.len() - common;
    let mut segs: Vec<&str> = std::iter::repeat("..").take(ups).collect();
    segs.extend_from_slice(&to_parts[common..]);
    segs.join("/")
}

/// Does the (percent-decoded, `?`/`#`-stripped) destination `base`, written in
/// a page whose directory is `from_dir`, name `target` exactly? `target` is a
/// file, or a folder when `is_dir`. `wiki` forms count only when written with
/// a `/`: a bare `[[stem]]` is a name, not a path.
pub(crate) fn exact_style(base: &str, from_dir: &str, target: &str, is_dir: bool, wiki: bool) -> Option<ExactStyle> {
    if base.is_empty() || (wiki && !base.contains('/')) {
        return None;
    }
    let (anchor, joined) = if let Some(rest) = base.strip_prefix('/') {
        (Anchor::Root, rest.to_string())
    } else if wiki && !(base.starts_with("./") || base.starts_with("../")) {
        (Anchor::RootBare, base.to_string())
    } else if from_dir.is_empty() {
        (Anchor::Page, base.to_string())
    } else {
        (Anchor::Page, format!("{from_dir}/{base}"))
    };
    let slash = base.ends_with('/');
    let n = moss_core::content_graph::join_written("", &joined)?;
    let form = if n == target {
        PathForm::Full
    } else if is_dir {
        return None;
    } else if !slash && strip_md_ext(target) == Some(n.as_str()) {
        PathForm::NoExt
    } else if wiki {
        return None;
    } else if is_home_path(target) && dirname(target) == n {
        PathForm::AddrHome
    } else if slash && strip_md_ext(target) == Some(n.as_str()) {
        PathForm::AddrPage
    } else {
        return None;
    };
    Some(ExactStyle { anchor, form, slash, dot: base.starts_with("./") })
}

/// Spell `new_target` in `style` from a page in `from_dir`: the same anchor
/// and form the author used, with whatever `./` or `../` segments are needed.
/// `None` when the new target can no longer be named that way (a path
/// without its extension for a file that has none).
pub(crate) fn exact_spelling(style: &ExactStyle, new_target: &str, from_dir: &str) -> Option<String> {
    let mut slash = style.slash;
    let path = match style.form {
        PathForm::Full => new_target,
        PathForm::NoExt => strip_md_ext(new_target)?,
        // An address names whatever page is served there: a folder's home page
        // is its folder's address, any other page is its own path plus `/`.
        PathForm::AddrHome | PathForm::AddrPage if is_home_path(new_target) => dirname(new_target),
        PathForm::AddrHome | PathForm::AddrPage => {
            slash = true;
            strip_md_ext(new_target)?
        }
    };
    let mut out = match style.anchor {
        Anchor::Root => format!("/{path}"),
        Anchor::RootBare => path.to_string(),
        Anchor::Page => {
            let mut rel = relative_root_path(from_dir, path);
            if rel.is_empty() {
                rel.push('.');
            }
            if style.dot && !rel.starts_with("..") && rel != "." {
                rel.insert_str(0, "./");
            }
            rel
        }
    };
    if slash && !out.ends_with('/') {
        out.push('/');
    }
    Some(out)
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

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "ref_rewrite_tests.rs"]
mod tests;
