//! Pure ranker for link-target autocomplete.
//!
//! moss-core is zero-I/O: the editor backend (moss-build) walks the source
//! tree and the article map and builds the [`Target`] list; this module ranks
//! it against a typed prefix and decides the text an accepted row inserts.
//! Ranking mirrors the resolver's NFC-normalize + lowercase comparison so the
//! suggested target is the one the link resolver will actually resolve.
//!
//! The mental model this encodes (docs/archive/2026-09-05-link-target-
//! completion-audit-and-design.md): the author links to a THING, moss writes
//! the address. Which address is a function of the syntax around the caret
//! and of whether the author opened the target with `/`, never a per-row
//! choice — see [`insert_for`].

use std::cmp::Reverse;

use unicode_normalization::UnicodeNormalization;

/// One completable thing. Every name the author might type for it and every
/// address it can be written as are derivations of these fields, so a reader
/// never checks which optional field happens to be filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A markdown page with a source file. `url` is its published pretty URL
    /// (`/about/`), or empty when no build has recorded one yet.
    Page { source: String, title: String, url: String },
    /// A non-page file, project-relative.
    Asset { source: String },
    /// A directory, project-relative, no trailing slash. Accepting one
    /// descends: the insert ends in `/` so the list reopens inside it.
    Folder { source: String },
    /// A page the build synthesizes (`/tags/design/`, `/authors/馬欣宜/`). It
    /// has no source, so the URL is its only address.
    Generated { url: String, display: String },
    /// A heading in the target page. `slug` is the anchor the build emits.
    Heading { text: String, slug: String, level: u8 },
}

/// The link syntax the caret is inside. Decides how a target is written and
/// which kinds rank first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum LinkSyntax {
    /// `[[…]]` — Obsidian form: pages by stem, assets by filename.
    Wikilink,
    /// `![[…]]` — same forms, assets first.
    Embed,
    /// `[text](…)` — the one syntax with two address spaces: a leading `/`
    /// means the published site, anything else means the source tree.
    Inline,
    /// `![alt](…)`, `image=…`, a gallery body line — an asset path, never a page.
    AssetPath,
}

/// What the completion was asked from: the syntax, the typed prefix, and the
/// project-relative path of the file being edited.
#[derive(Debug, Clone, Copy)]
pub struct InsertCtx<'a> {
    pub syntax: LinkSyntax,
    pub prefix: &'a str,
    pub from_rel: &'a str,
}

impl InsertCtx<'_> {
    /// URL space is the ONE case where the author is naming a place on the
    /// published site: an inline link opened with `/`. Every other context is
    /// a source path — the same reading `resolve_asset_ref` step 1 and the
    /// editor lint's `classify_link` already give a leading slash.
    pub fn url_space(&self) -> bool {
        self.syntax == LinkSyntax::Inline && self.prefix.starts_with('/')
    }

}

/// A target's kind, as the dropdown paints it (`.cm-completionIcon-<kind>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "kebab-case")]
pub enum TargetKind {
    Page,
    Asset,
    Folder,
    Generated,
    Heading,
}

impl Target {
    pub fn kind(&self) -> TargetKind {
        match self {
            Target::Page { .. } => TargetKind::Page,
            Target::Asset { .. } => TargetKind::Asset,
            Target::Folder { .. } => TargetKind::Folder,
            Target::Generated { .. } => TargetKind::Generated,
            Target::Heading { .. } => TargetKind::Heading,
        }
    }

    /// The name the dropdown shows: title for a page, filename for an asset,
    /// `dir/` for a folder, display text for a generated page, heading text.
    pub fn label(&self) -> String {
        match self {
            Target::Page { source, title, .. } => {
                if title.is_empty() { stem_of(source).to_string() } else { title.clone() }
            }
            Target::Asset { source } => file_name(source).to_string(),
            Target::Folder { source } => format!("{}/", file_name(source)),
            Target::Generated { display, .. } => display.clone(),
            Target::Heading { text, .. } => text.clone(),
        }
    }

    /// The project-relative source path, for language-tree and proximity
    /// ranking. A generated page ranks by its URL; a heading has none.
    fn source_path(&self) -> Option<&str> {
        match self {
            Target::Page { source, .. } | Target::Asset { source } | Target::Folder { source } => Some(source),
            Target::Generated { url, .. } => Some(url),
            Target::Heading { .. } => None,
        }
    }

    /// Whether this target can be written at all in `ctx`. A generated page
    /// exists only in URL space; a folder or a heading never does; an asset
    /// context takes no pages.
    fn offered_in(&self, ctx: &InsertCtx<'_>) -> bool {
        let url_space = ctx.url_space();
        match self {
            Target::Page { url, .. } => {
                ctx.syntax != LinkSyntax::AssetPath && (!url_space || !url.is_empty())
            }
            Target::Asset { .. } => true,
            Target::Folder { .. } => !url_space,
            Target::Generated { .. } => url_space,
            Target::Heading { .. } => true,
        }
    }

    /// Lower ranks first. The trigger decides: asset syntaxes put files first,
    /// link syntaxes put pages first, and URL space puts the site's pages
    /// (authored or generated) before files. Folders always trail the files
    /// that matched the same segment — a folder row is an offer to descend,
    /// never the default accept.
    fn kind_rank(&self, ctx: &InsertCtx<'_>) -> u8 {
        use LinkSyntax::*;
        match (ctx.syntax, ctx.url_space(), self) {
            (Embed | AssetPath, _, Target::Asset { .. }) => 0,
            (Embed | AssetPath, _, Target::Folder { .. }) => 1,
            (Embed | AssetPath, _, _) => 2,
            (_, true, Target::Page { .. }) => 0,
            (_, true, Target::Generated { .. }) => 1,
            (_, true, _) => 2,
            (_, false, Target::Page { .. }) => 0,
            (_, false, Target::Asset { .. }) => 1,
            (_, false, _) => 2,
        }
    }

    /// Every name the author might have in mind, NFC-folded. The first is the
    /// primary (what `starts_with` ranking reads).
    fn names(&self, url_space: bool) -> Vec<String> {
        match self {
            Target::Page { source, title, url } => {
                let mut v = vec![norm(stem_of(source))];
                if !title.is_empty() { v.push(norm(title)); }
                if let Some(last) = last_segment(url) { v.push(norm(last)); }
                if url_space { v.rotate_right(1); } // url slug becomes primary
                v
            }
            Target::Asset { source } | Target::Folder { source } => vec![norm(file_name(source))],
            Target::Generated { url, display } => {
                let mut v = vec![norm(display)];
                if let Some(last) = last_segment(url) { v.push(norm(last)); }
                v
            }
            Target::Heading { text, .. } => vec![norm(text)],
        }
    }

    /// The path a path-qualified query is matched against: the source path in
    /// source space, the published URL in URL space.
    fn match_path(&self, url_space: bool) -> String {
        match self {
            Target::Page { source, url, .. } => {
                if url_space { url.clone() } else { crate::content_graph::normalize_path(source) }
            }
            Target::Asset { source } | Target::Folder { source } => crate::content_graph::normalize_path(source),
            Target::Generated { url, .. } => url.clone(),
            Target::Heading { .. } => String::new(),
        }
    }
}

/// The text an accepted row inserts. THE INVARIANT: the emitted form is the
/// one whose FIRST applicable resolver step reproduces the target exactly.
///
/// - Wikilink / embed / asset path: Obsidian forms — a page by stem, an asset
///   by filename — unless the author typed a path, in which case the exact
///   path form (`ContentGraph::resolve_path` steps 1/2 pin a root-relative
///   page path; `resolve_asset_ref` step 2 reproduces a source-relative asset
///   path, step 1 a `/`-rooted one).
/// - Inline link, source space: always the exact form. A relative markdown
///   link is validated by the lint and rewritten to the pretty URL by the
///   build, and survives a `url:` change, which is why it is the default.
/// - Inline link, URL space: the published URL for a page or a generated page;
///   `/` + source for an asset, which step 1 pins.
/// - Folder: the path so far plus `/`, so the list reopens inside.
/// - Heading: the text for `[[…#`, the anchor slug for `[…](…#`.
///
/// The bare root-relative asset form is never emitted: from `關於/x.md` it
/// would hit step 2 first and silently resolve `assets/hero.png` to an
/// entirely different, existing `關於/assets/hero.png`. `..` is never emitted
/// either — step-1 anchoring is strictly simpler and equally exact.
pub fn insert_for(t: &Target, ctx: &InsertCtx<'_>) -> String {
    use LinkSyntax::*;
    let exact = ctx.syntax == Inline || parse_query(ctx.prefix).path_qualified;
    match t {
        Target::Page { source, url, .. } => {
            if ctx.url_space() {
                url.clone()
            } else if exact {
                let rel = source.replace('\\', "/");
                rel.strip_suffix(".md").unwrap_or(&rel).to_string()
            } else {
                stem_of(source).to_string()
            }
        }
        Target::Asset { source } => {
            if ctx.url_space() {
                format!("/{}", source.replace('\\', "/"))
            } else if exact {
                asset_ref_relative(ctx.from_rel, source)
            } else {
                file_name(source).to_string()
            }
        }
        Target::Folder { source } => format!("{}/", source.replace('\\', "/")),
        Target::Generated { url, .. } => url.clone(),
        Target::Heading { text, slug, .. } => {
            if ctx.syntax == Inline { slug.clone() } else { text.clone() }
        }
    }
}

/// Rank `targets` against `ctx.prefix`, returning indices into `targets`
/// ordered best-first. Targets the context cannot write are dropped (see
/// [`Target::offered_in`]). An empty prefix returns every offered target,
/// kind-ordered.
///
/// `ctx.from_rel` biases ties toward the source's own context — candidates in
/// the same language tree (e.g. both under `zh-hans/`), and then candidates
/// closer in the directory tree, rank higher. This mirrors
/// [`crate::content_graph`]'s resolver so the dropdown order matches how a
/// link would actually resolve.
///
/// A prefix containing a `/` (or `\`) is PATH-QUALIFIED: its segments are
/// matched, in order, against the target's path components rather than
/// against its names alone. That is what makes `關於/頭像-李` find
/// `關於/assets/頭像-李柏萱.png` even though the author omits the `assets/`
/// segment they never type. In URL space the components are the published
/// URL's.
pub fn rank_completions(targets: &[Target], ctx: &InsertCtx<'_>) -> Vec<usize> {
    let from_norm = crate::content_graph::normalize_path(ctx.from_rel);
    let from_lang = crate::home::lang_tree_prefix(&from_norm);
    let from_dirs = crate::content_graph::dir_components(&from_norm);
    let url_space = ctx.url_space();
    let query = parse_query(ctx.prefix);

    // ONE normalization pass per target. Both the filter (`match_query`) and
    // the sort key (`score`) read the same `Prepared`.
    let mut hits: Vec<(Prepared<'_>, Hit)> = targets
        .iter()
        .enumerate()
        .filter(|(_, t)| t.offered_in(ctx))
        .filter_map(|(i, t)| {
            let p = Prepared {
                idx: i,
                t,
                names: t.names(url_space),
                path_norm: t.match_path(url_space),
                kind_rank: t.kind_rank(ctx),
                label: t.label(),
            };
            let hit = match_query(&query, &p)?;
            Some((p, hit))
        })
        .collect();

    // `sort_by_cached_key`, not `sort_by_key`: `score` allocates, so caching one
    // key per element avoids re-running it on every comparison.
    hits.sort_by_cached_key(|(p, hit)| score(&query, p, *hit, from_lang, &from_dirs));
    hits.into_iter().map(|(p, _)| p.idx).collect()
}

/// A target with its normalized forms computed once.
struct Prepared<'a> {
    idx: usize,
    t: &'a Target,
    /// Every NFC-folded name; `names[0]` is the primary.
    names: Vec<String>,
    /// Resolver-normalized path the segments of a path query match against.
    path_norm: String,
    kind_rank: u8,
    label: String,
}

/// The typed prefix, split into path segments.
///
/// `path_qualified` is what switches the matcher from "a name contains" to
/// "ordered subsequence over path components". Empty segments are dropped, so
/// a leading `/` is harmless (paths are root-relative already) and `a//b` is
/// `a`,`b`. `dir_only` (a trailing `/`) restricts matching to directories.
struct Query {
    segs: Vec<String>,
    path_qualified: bool,
    dir_only: bool,
}

fn parse_query(prefix: &str) -> Query {
    let raw = prefix.replace('\\', "/");
    Query {
        path_qualified: raw.contains('/'),
        dir_only: raw.ends_with('/'),
        segs: raw.split('/').filter(|s| !s.is_empty()).map(norm).collect(),
    }
}

/// How well a target matched — two sort keys, both constant `0` for a
/// non-path-qualified query, which is what keeps bare-query ordering
/// independent of the path keys.
#[derive(Debug, Clone, Copy)]
struct Hit {
    /// 0 = the LAST query segment matched the final component; 1 = it only
    /// matched a directory above it (so `關於/ass` still lists the subtree
    /// while any true filename match outranks it).
    seg_hit: u8,
    /// 0 = the matched components are contiguous AND end at the final
    /// component (a true path-suffix match); 1 = gapped. Mirrors
    /// `resolve_asset_ref` step 4, which tries `find_by_suffix(target)` before
    /// `find_by_suffix(basename)`.
    dir_tight: u8,
}

/// Filter + match-quality, in one pass. `None` drops the target.
fn match_query(q: &Query, p: &Prepared<'_>) -> Option<Hit> {
    if q.segs.is_empty() {
        return Some(Hit { seg_hit: 0, dir_tight: 0 });
    }
    // Headings are excluded from path logic on purpose: heading TEXT may
    // contain `/` (`[[Page#A/B]]`). A non-path-qualified query takes the same
    // branch: any name containing the query matches.
    if !q.path_qualified || matches!(p.t, Target::Heading { .. }) {
        let whole = norm(&q.segs.join("/"));
        return if p.names.iter().any(|n| n.contains(&whole)) {
            Some(Hit { seg_hit: 0, dir_tight: 0 })
        } else {
            None
        };
    }
    let comps: Vec<&str> = p.path_norm.split('/').filter(|s| !s.is_empty()).collect();
    if comps.is_empty() {
        return None;
    }
    let file_i = comps.len() - 1;
    // A trailing `/` means "inside this directory": the segments must all be
    // consumed by directory components, never by the final one. For a folder
    // target the final component is the folder's own name, so `關於/` lists
    // `關於/assets` but never `關於` itself.
    let limit = if q.dir_only { file_i } else { comps.len() };
    let mut positions: Vec<usize> = Vec::with_capacity(q.segs.len());
    let mut next = 0usize;
    for seg in &q.segs {
        let mut found = None;
        while next < limit {
            let at = next;
            next += 1;
            if comps[at].contains(seg.as_str()) {
                found = Some(at);
                break;
            }
        }
        positions.push(found?);
    }
    // `?` rather than `expect`: the crate denies `clippy::expect_used`, and the
    // empty case is already handled by the early return above.
    let last = *positions.last()?;
    let contiguous = positions.windows(2).all(|w| w[1] == w[0] + 1);
    Some(Hit {
        seg_hit: u8::from(last != file_i),
        dir_tight: u8::from(!(contiguous && last == file_i)),
    })
}

/// The reference form for an asset at `rel_path`, written from the page at
/// `from_rel` — the asset arm of [`insert_for`].
///
/// Same invariant: the emitted form is the one whose first applicable resolver
/// step reproduces `rel_path` exactly — source-relative when the asset lives in
/// the page's own subtree (`resolve_asset_ref` step 2 reproduces it by
/// construction), `/`-rooted otherwise (pinned by step 1).
fn asset_ref_relative(from_rel: &str, rel_path: &str) -> String {
    let rel = rel_path.replace('\\', "/");
    let from = from_rel.replace('\\', "/");
    let from_dir = crate::resolve::parent_dir(&from);
    source_relative(from_dir, &rel).unwrap_or_else(|| format!("/{rel}"))
}

/// `rel_path` expressed relative to `from_dir`, or `None` when it is not inside
/// that directory. Compares COMPONENTS, never the raw string — `關於2/x.png`
/// must not count as living inside `關於`. Never produces a `..` segment.
fn source_relative(from_dir: &str, rel_path: &str) -> Option<String> {
    let base: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let target: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    if target.len() <= base.len() {
        return None;
    }
    if base.iter().enumerate().any(|(i, b)| target[i] != *b) {
        return None;
    }
    Some(target[base.len()..].join("/"))
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn stem_of(path: &str) -> &str {
    let name = file_name(path);
    name.strip_suffix(".md").unwrap_or(name)
}

/// Last non-empty segment of a URL path (`/tags/design/` → `design`).
fn last_segment(url: &str) -> Option<&str> {
    url.split('/').filter(|s| !s.is_empty()).next_back()
}

/// NFC-normalize then lowercase — identical to
/// [`crate::content_graph`]'s `normalize_component`, so a completion suggestion
/// folds the same way the link resolver will fold it at build time. Without the
/// NFC step a decomposed-form filename (e.g. NFD CJK/accented codepoints, which
/// HFS+ historically wrote and APFS preserves) could rank as a match here but
/// resolve differently in `ContentGraph`, suggesting a target that doesn't
/// round-trip.
fn norm(s: &str) -> String {
    s.nfc().collect::<String>().to_lowercase()
}

/// Lower score sorts first. Ordering, in priority:
/// 1. kind matches the trigger ([`Target::kind_rank`])
/// 2. prefix-at-start beats prefix-in-middle (match quality, on the primary name)
/// 3. the last query segment matched the final component, not just a directory
/// 4. the matched components are a contiguous path suffix, not a gapped one
/// 5. same language tree as the source (or both tree-less) beats a different one
/// 6. closer in the directory tree (longer shared dir prefix) beats farther
/// 7. shorter label (closer match) beats longer
/// 8. lexicographic primary name
/// 9. lexicographic normalized path (fully deterministic, independent of the
///    filesystem walk order)
///
/// Keys 5, 6 and 9 mirror the resolver's tiebreak chain in
/// [`crate::content_graph`] (`tree_match`, `common_prefix_len`,
/// `Reverse(normalized path)`), so when two candidates share a stem the
/// dropdown surfaces the same one the link would resolve to. Keys 1–4 are
/// completion-specific — the resolver matches exact stems and has no notion of
/// trigger kind or partial-match quality.
///
/// Keys 3 and 4 are the constant `0` for every candidate when the query has no
/// separator, so they cannot perturb bare-query order. Key 2 is computed
/// against the query's LAST segment, which for a bare query is the whole prefix.
fn score(
    q: &Query,
    p: &Prepared<'_>,
    hit: Hit,
    from_lang: Option<&str>,
    from_dirs: &[&str],
) -> (u8, u8, u8, u8, u8, Reverse<usize>, usize, String, String) {
    let primary = p.names.first().map(String::as_str).unwrap_or("");
    let starts = match q.segs.last() {
        Some(last) if primary.starts_with(last.as_str()) => 0u8,
        _ => 1u8,
    };

    // Language tree + directory proximity, both relative to the source file and
    // computed on the resolver-normalized candidate path.
    let cand_path = p.t.source_path().map(crate::content_graph::normalize_path).unwrap_or_default();
    let cand_lang = crate::home::lang_tree_prefix(&cand_path);
    let lang_rank = match (from_lang, cand_lang) {
        (Some(f), Some(cc)) if f.eq_ignore_ascii_case(cc) => 0u8,
        (None, None) => 0u8,
        _ => 1u8,
    };
    let proximity = crate::content_graph::common_prefix_len(
        &crate::content_graph::dir_components(&cand_path),
        from_dirs,
    );

    (
        p.kind_rank,
        starts,
        hit.seg_hit,
        hit.dir_tight,
        lang_rank,
        Reverse(proximity), // more shared dirs sorts first under ascending order
        p.label.chars().count(), // scalar count, not byte len — CJK filenames sort correctly
        primary.to_string(),
        p.path_norm.clone(), // terminal: alphabetical-by-normalized-path (smallest wins, mirroring the resolver's Reverse(path))
    )
}

#[cfg(test)]
#[path = "link_completions_tests.rs"]
mod tests;
