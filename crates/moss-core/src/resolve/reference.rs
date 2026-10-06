//! The single reference classifier shared by build + editor (asset/file-embed/
//! folder kinds). Pure; indexes injected via ReferenceContext. Page-Link
//! emission is out of scope here (the build keeps relative_pretty_url/page_map);
//! Link is classify-only. Named `classify_reference` to avoid colliding with
//! `fuzzy_path::resolve_reference` (the [[note]]/ContentGraph resolver).

use crate::content_graph::ContentGraph;
use crate::resolve::asset_class::AssetProvenance;
use crate::resolve::embed_renderer::Sizing;
use crate::resolve::folder_class::FolderIndex;
use crate::resolve::link_class::UrlIndex;

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "data")]
pub enum ReferenceKind {
    Link { anchor: Option<String> },
    Image,
    Iframe,
    Pdf,
    Video,
    Audio,
    Model,
    FolderListing,
    FolderIndexIframe,
    Transclusion,
    Notebook,
    Table,
    External { url: String },
    Anchor,
    Ambiguous,
    NotFound,
    /// An internal link whose URL no longer exists: the page it named now
    /// lives at `canonical` (a claimed term's generated URL). `url` on the
    /// resolved reference carries `canonical` so following it lands there.
    Moved { canonical: String },
}

/// Index handles a classify call needs. Bundled so the signature stays small
/// and a future index can be added without re-touching every caller.
pub struct ReferenceContext<'a> {
    /// Every file of the site; the one place a file target is resolved.
    pub assets: &'a ContentGraph,
    pub folders: &'a dyn FolderIndex,
    /// Link arm only; the build supplies a graph-backed impl in sub-project #4.
    /// For unit #1+#2 a Link result is classify-only and this may be a no-op.
    pub urls: &'a dyn UrlIndex,
}

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedReference {
    pub kind: ReferenceKind,
    /// Root-relative SOURCE path (real case) for file/folder kinds, and for
    /// Ambiguous (the file the build links among the tied candidates); None for
    /// Link/External/Anchor/NotFound.
    pub target_path: Option<String>,
    pub size: Option<Sizing>,
    pub provenance: Option<AssetProvenance>,
    /// Human-readable resolution note (separator-fallback / case-mismatch / …).
    pub message: Option<String>,
    /// Populated for Ambiguous (the equally near candidate paths).
    pub candidates: Vec<String>,
    /// Resolved page/asset URL for a non-embed Link (None for embeds — the
    /// build emits embed URLs itself; editor embeds use `target_path`).
    pub url: Option<String>,
}

impl ResolvedReference {
    pub(crate) fn not_found() -> Self {
        ResolvedReference {
            kind: ReferenceKind::NotFound,
            target_path: None,
            size: None,
            provenance: None,
            message: None,
            candidates: Vec::new(),
            url: None,
        }
    }
    /// Invariant: target_path is Some iff kind is a file/folder kind.
    pub(crate) fn debug_check_invariant(&self) {
        let has_path = matches!(
            self.kind,
            ReferenceKind::Image
                | ReferenceKind::Iframe
                | ReferenceKind::Pdf
                | ReferenceKind::Video
                | ReferenceKind::Audio
                | ReferenceKind::Model
                | ReferenceKind::FolderListing
                | ReferenceKind::FolderIndexIframe
                | ReferenceKind::Transclusion
                | ReferenceKind::Notebook
                | ReferenceKind::Table
                | ReferenceKind::Ambiguous
        );
        debug_assert_eq!(
            has_path,
            self.target_path.is_some(),
            "target_path presence must match kind: {:?}",
            self.kind
        );
    }
}

/// One canonical classification result, including the blocking consequence of
/// an unresolved asset-shaped target.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceVerdict {
    pub resolved: ResolvedReference,
    pub missing_kind: crate::resolve::DiagnosticKind,
}

impl ReferenceVerdict {
    /// Keep the resolved record only when this reference is the publish gate's
    /// one blocking missing-asset outcome.
    pub fn into_missing_asset(self) -> Option<ResolvedReference> {
        (self.missing_kind == crate::resolve::DiagnosticKind::MissingAsset).then_some(self.resolved)
    }
}

struct ParsedReference<'a> {
    path: &'a str,
    pothole: Option<&'a str>,
    anchor: Option<String>,
}

fn parse_reference(inner: &str) -> ParsedReference<'_> {
    let (path, pothole) = match inner.split_once('|') {
        Some((path, pothole)) => (path.trim(), Some(pothole)),
        None => (inner, None),
    };
    let path = path.split_once('?').map(|(path, _)| path.trim()).unwrap_or(path);
    let (path, anchor) = match path.split_once('#') {
        Some((path, anchor)) => (path.trim(), Some(anchor.to_string())),
        None => (path, None),
    };
    ParsedReference { path, pothole, anchor }
}

/// Classify a reference's inner text (target + optional |pothole / #anchor /
/// ?query) into a kind + resolved source path. Pure.
pub fn classify_reference(
    inner: &str,
    from_source: &str,
    is_embed: bool,
    ctx: &ReferenceContext,
) -> ResolvedReference {
    classify_reference_verdict(inner, from_source, is_embed, ctx).resolved
}

/// Classify an authored reference and retain the canonical missing-asset
/// consequence alongside the resolved kind.
pub fn classify_reference_verdict(
    inner: &str,
    from_source: &str,
    is_embed: bool,
    ctx: &ReferenceContext,
) -> ReferenceVerdict {
    let inner = inner.trim();
    let parsed = parse_reference(inner);
    let resolved = classify_reference_parts(inner, &parsed, from_source, is_embed, ctx);
    let missing_kind = if matches!(resolved.kind, ReferenceKind::NotFound) {
        crate::resolve::ext_kind::missing_reference_kind(
            crate::path_ext::path_extension(parsed.path).as_deref(),
        )
    } else {
        crate::resolve::DiagnosticKind::Other
    };
    ReferenceVerdict { resolved, missing_kind }
}

fn classify_reference_parts(
    inner: &str,
    parsed: &ParsedReference,
    from_source: &str,
    is_embed: bool,
    ctx: &ReferenceContext,
) -> ResolvedReference {

    // External short-circuits (mirror classify_link's exception list).
    const EXTERNAL_PREFIXES: &[&str] =
        &["http://", "https://", "//", "mailto:", "tel:", "data:"];
    if EXTERNAL_PREFIXES.iter().any(|p| inner.starts_with(p)) {
        let mut r = ResolvedReference::not_found();
        r.kind = ReferenceKind::External { url: inner.to_string() };
        r.debug_check_invariant();
        return r;
    }
    // Pure anchor / query (no path component).
    if inner.starts_with('#') || inner.starts_with('?') {
        let mut r = ResolvedReference::not_found();
        r.kind = ReferenceKind::Anchor;
        return r;
    }

    let path_no_anchor = parsed.path;
    let anchor = parsed.anchor.clone();
    let size = parsed.pothole.and_then(crate::resolve::embed_renderer::Sizing::parse);

    // Non-embed mode: a `[[note]]` / `[](path)` reference is a Link resolved
    // against the deployed URL space (`ctx.urls`), NOT an embed kind. This runs
    // BEFORE the folder/file arms so it cannot mis-route a non-embed reference
    // to Transclusion/Image/Folder. The BUILD always passes `is_embed=true`
    // (folder markers), so this branch is dead for the build — the folder arm
    // below short-circuits there.
    if !is_embed {
        use crate::resolve::link_class::{classify_link, LinkClass};
        let with_anchor = |url: &str| match &anchor {
            Some(a) => format!("{}#{}", url, a),
            None => url.to_string(),
        };
        return match classify_link(path_no_anchor, from_source, ctx.urls) {
            LinkClass::Resolved { url } => {
                let mut r = ResolvedReference::not_found();
                r.kind = ReferenceKind::Link { anchor: anchor.clone() };
                r.url = Some(with_anchor(&url));
                r
            }
            LinkClass::Mismatch { canonical } => {
                // A page exists but the link won't hit its canonical URL
                // (case/slug). Surface it as a Link pointing at the canonical
                // URL, with a note explaining the redirect.
                let mut r = ResolvedReference::not_found();
                r.kind = ReferenceKind::Link { anchor: anchor.clone() };
                r.url = Some(with_anchor(&canonical));
                r.message = Some(format!("resolves to canonical URL {}", canonical));
                r
            }
            LinkClass::External => {
                let mut r = ResolvedReference::not_found();
                r.kind = ReferenceKind::External { url: path_no_anchor.to_string() };
                r
            }
            LinkClass::Anchor => {
                let mut r = ResolvedReference::not_found();
                r.kind = ReferenceKind::Anchor;
                r
            }
            LinkClass::Moved { canonical } => {
                // The URL is gone for good and the page lives elsewhere — a
                // certain 404, unlike Mismatch. Carry the canonical as `url`
                // so the follow path opens the page that replaced it.
                let mut r = ResolvedReference::not_found();
                r.url = Some(with_anchor(&canonical));
                r.message = Some(format!("no page at this URL any more — it lives at {}", canonical));
                r.kind = ReferenceKind::Moved { canonical };
                r
            }
            // Broken: no deployed page for this internal reference.
            LinkClass::Broken => ResolvedReference::not_found(),
        };
    }

    use crate::resolve::asset_class::AssetResolution;
    use crate::resolve::ext_kind::{reference_kind_for_ext, ExtKind};

    // Folder arm: trailing slash, or the target resolves to a directory.
    let looks_like_folder = path_no_anchor.ends_with('/');
    let folder_rel: Option<String> = if let Some(abs) = path_no_anchor.strip_prefix('/') {
        Some(abs.trim_end_matches('/').to_string())
    } else if looks_like_folder {
        // From the page's folder; a path climbing out of the site names none.
        let from_dir = crate::resolve::parent_dir(from_source);
        crate::content_graph::join_written(from_dir, path_no_anchor.trim_end_matches('/'))
    } else {
        None
    };
    if let Some(folder_rel) = folder_rel {
        // Only treat this as a folder reference when it is one: an explicit
        // trailing slash, or a path that the folder index resolves to a real
        // directory. A leading-slash path WITHOUT a trailing slash (e.g. an
        // absolute file embed `/assets/photo.png`) is NOT a folder — it must
        // fall through to the file arm and resolve as the asset it names.
        let is_folder = looks_like_folder || ctx.folders.is_dir(&folder_rel);
        if is_folder {
            if ctx.folders.dir_has_markdown_index(&folder_rel) {
                let mut r = ResolvedReference::not_found();
                r.kind = ReferenceKind::FolderListing;
                r.target_path = Some(folder_rel);
                r.size = size;
                r.debug_check_invariant();
                return r;
            }
            if ctx.folders.dir_has_static_index(&folder_rel).is_some() {
                let mut r = ResolvedReference::not_found();
                r.kind = ReferenceKind::FolderIndexIframe;
                r.target_path = Some(folder_rel);
                r.size = size;
                r.debug_check_invariant();
                return r;
            }
            // A confirmed folder (explicit trailing slash, or a real directory)
            // without an index is NotFound — it must NOT fall through to the
            // file arm (a folder path is never a file asset).
            return ResolvedReference::not_found();
        }
    }

    // File arm.
    //
    // Resolve the reference to a source file, then key the embed kind off the
    // RESOLVED file's extension — NOT the query string's. A bare `![[note]]`
    // carries no extension; the build resolves it to `note.md`
    // (ContentGraph::resolve_path step 1b/2) and renders a Transclusion. Keying
    // off the query instead (extensionless → Other → Link) was the editor-only
    // drift that showed `![[support-band]]` as "not found" while the build
    // transcluded it. `query_ext_kind` is used only to decide the *unresolved*
    // fallback (known-ext miss = broken embed; unknown-ext miss = note Link).
    let query_ext_kind = reference_kind_for_ext(
        crate::path_ext::path_extension(path_no_anchor).as_deref().unwrap_or(""),
    );

    let resolved: Option<(String, AssetProvenance)> =
        match crate::resolve::asset_class::resolve_file_target(path_no_anchor, from_source, ctx.assets) {
            AssetResolution::Resolved { root_rel, provenance } => Some((root_rel, provenance)),
            // Equally near files: the build links `chosen`, so that is the
            // target; the kind tells the editor it was a coin flip.
            AssetResolution::Ambiguous { chosen, candidates } => {
                let mut r = ResolvedReference::not_found();
                r.kind = ReferenceKind::Ambiguous;
                r.target_path = Some(chosen);
                r.candidates = candidates;
                r.debug_check_invariant();
                return r;
            }
            AssetResolution::NotFound => None,
        };

    match resolved {
        Some((root_rel, provenance)) => {
            // Kind keyed off the RESOLVED file's extension (see comment above).
            let kind = match reference_kind_for_ext(
                crate::path_ext::path_extension(&root_rel).as_deref().unwrap_or(""),
            ) {
                ExtKind::Image => ReferenceKind::Image,
                ExtKind::Iframe => ReferenceKind::Iframe,
                ExtKind::Pdf => ReferenceKind::Pdf,
                ExtKind::Video => ReferenceKind::Video,
                ExtKind::Audio => ReferenceKind::Audio,
                ExtKind::Model => ReferenceKind::Model,
                ExtKind::Transclusion => ReferenceKind::Transclusion,
                ExtKind::Notebook => ReferenceKind::Notebook,
                ExtKind::Table => ReferenceKind::Table,
                // A resolved file with an UNKNOWN extension is a Link target (no embed path).
                ExtKind::Other => ReferenceKind::Link { anchor: anchor.clone() },
            };
            let is_link = matches!(kind, ReferenceKind::Link { .. });
            let mut r = ResolvedReference::not_found();
            r.kind = kind;
            r.target_path = if is_link { None } else { Some(root_rel) };
            r.size = size;
            r.provenance = Some(provenance);
            r.debug_check_invariant();
            r
        }
        None => {
            if matches!(query_ext_kind, ExtKind::Other) {
                // An unresolved reference with no known file extension is a note
                // link (classify-only here; link resolution/emission is sub-project #4).
                let mut r = ResolvedReference::not_found();
                r.kind = ReferenceKind::Link { anchor };
                r
            } else {
                // A known-extension asset that didn't resolve is a broken embed.
                ResolvedReference::not_found()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::resolve::folder_class::FakeFolderIndex;
    use crate::resolve::link_class::FakeUrlIndex;

    fn ctx<'a>(
        a: &'a ContentGraph,
        f: &'a FakeFolderIndex,
        u: &'a FakeUrlIndex,
    ) -> ReferenceContext<'a> {
        ReferenceContext { assets: a, folders: f, urls: u }
    }

    #[test]
    fn external_url_is_external() {
        let a = ContentGraph::from_paths(&[]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("https://example.com/x", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::External { url: "https://example.com/x".into() });
        assert!(r.target_path.is_none());
    }

    #[test]
    fn bare_anchor_is_anchor() {
        let a = ContentGraph::from_paths(&[]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("#section", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Anchor);
    }

    #[test]
    fn not_found_has_no_path() {
        let r = ResolvedReference::not_found();
        assert_eq!(r.kind, ReferenceKind::NotFound);
        assert!(r.target_path.is_none());
        r.debug_check_invariant();
    }

    #[test]
    fn image_file_resolves_to_image_kind() {
        let a = ContentGraph::from_paths(&["assets/photo.png"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("photo.png", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Image);
        assert_eq!(r.target_path.as_deref(), Some("assets/photo.png"));
        r.debug_check_invariant();
    }

    #[test]
    fn html_file_resolves_to_iframe_with_size() {
        let a = ContentGraph::from_paths(&["widgets/app.html"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("widgets/app.html|800x600", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Iframe);
        assert!(matches!(r.size, Some(crate::resolve::embed_renderer::Sizing::Box(_, _))));
    }

    #[test]
    fn ambiguous_file_match_sets_candidates() {
        let a = ContentGraph::from_paths(&["a/logo.png", "b/logo.png"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("logo.png", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Ambiguous);
        assert_eq!(r.candidates, vec!["a/logo.png".to_string(), "b/logo.png".to_string()]);
        // The tie still names the file the build links.
        assert_eq!(r.target_path.as_deref(), Some("a/logo.png"));
    }

    #[test]
    fn the_nearest_copy_is_not_ambiguous() {
        let a = ContentGraph::from_paths(&["a/logo.png", "b/logo.png"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("logo.png", "b/page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Image);
        assert_eq!(r.target_path.as_deref(), Some("b/logo.png"));
    }

    #[test]
    fn folder_with_static_index_is_iframe() {
        let a = ContentGraph::from_paths(&[]);
        let mut f = FakeFolderIndex::new();
        f.dirs.insert("Resources/app".into());
        f.static_index.insert("Resources/app".into(), "index.html".into());
        let u = FakeUrlIndex::new();
        let r = classify_reference("/Resources/app/", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::FolderIndexIframe);
        assert_eq!(r.target_path.as_deref(), Some("Resources/app"));
        r.debug_check_invariant();
    }

    #[test]
    fn folder_with_markdown_index_is_listing() {
        let a = ContentGraph::from_paths(&[]);
        let mut f = FakeFolderIndex::new();
        f.dirs.insert("news".into());
        f.md_index.insert("news".into());
        let u = FakeUrlIndex::new();
        let r = classify_reference("/news/", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::FolderListing);
        r.debug_check_invariant();
    }

    #[test]
    fn a_folder_path_climbing_out_of_the_site_names_no_folder() {
        let a = ContentGraph::from_paths(&[]);
        let mut f = FakeFolderIndex::new();
        f.dirs.insert("news".into());
        f.md_index.insert("news".into());
        let u = FakeUrlIndex::new();
        let r = classify_reference("../news/", "a/page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::FolderListing);
        let r = classify_reference("../../news/", "a/page.md", true, &ctx(&a, &f, &u));
        assert_ne!(r.kind, ReferenceKind::FolderListing, "{r:?}");
    }

    #[test]
    fn absolute_file_embed_resolves_to_image() {
        // A leading-slash path with NO trailing slash, naming a real asset, is a
        // file embed — not a folder. The folder arm must let it fall through to
        // the file arm so `![[/assets/photo.png]]` resolves as an Image.
        let a = ContentGraph::from_paths(&["assets/photo.png"]);
        let f = FakeFolderIndex::new(); // NOT a dir, no indexes
        let u = FakeUrlIndex::new();
        let r = classify_reference("/assets/photo.png", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Image);
        assert_eq!(r.target_path.as_deref(), Some("assets/photo.png"));
        r.debug_check_invariant();
    }

    #[test]
    fn trailing_slash_unresolved_folder_is_not_found() {
        let a = ContentGraph::from_paths(&[]);
        let f = FakeFolderIndex::new(); // empty: not a dir, no indexes
        let u = FakeUrlIndex::new();
        let r = classify_reference("/ghost/", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::NotFound);
    }

    #[test]
    fn bare_note_name_is_link() {
        let a = ContentGraph::from_paths(&[]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("some-note", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Link { anchor: None });
        assert!(r.target_path.is_none());
        r.debug_check_invariant();
    }

    #[test]
    fn missing_known_ext_asset_is_not_found() {
        // A known image extension that doesn't resolve stays NotFound (it is a
        // broken asset embed, not a note link).
        let a = ContentGraph::from_paths(&[]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("missing.png", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::NotFound);
    }

    #[test]
    fn non_embed_md_note_resolves_as_link_not_transclusion() {
        let a = ContentGraph::from_paths(&["note.md"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::resolving(&[("note.md", "/note/")]);
        let r = classify_reference("note.md", "page.md", false, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Link { anchor: None });
        assert_eq!(r.url.as_deref(), Some("/note/"));
    }

    #[test]
    fn embed_md_is_still_transclusion() {
        let a = ContentGraph::from_paths(&["note.md"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("note.md", "page.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Transclusion);
    }

    // ── Bare-name embed transclusion parity (Bug 3) ──────────────────────────
    // The build resolves `![[support-band]]` (no extension) to `support-band.md`
    // via ContentGraph::resolve_path (exact-path + `.md`, lang-scoped) and renders
    // a Transclusion. The editor classifier previously keyed the embed kind off
    // the QUERY string's extension (extensionless → Other → Link), so the same
    // reference showed "not found" in the editor. These lock the parity.

    #[test]
    fn bare_embed_resolves_markdown_note_as_transclusion() {
        let a = ContentGraph::from_paths(&["support-band.md"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("support-band", "index.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Transclusion);
        assert_eq!(r.target_path.as_deref(), Some("support-band.md"));
        r.debug_check_invariant();
    }

    #[test]
    fn bare_embed_prefers_source_relative_md_note() {
        // From a language-tree source, the sibling note wins — mirrors
        // ContentGraph::resolve_path step 1b lang-scoping (source-relative
        // resolution in the one resolver gives this for free).
        let a = ContentGraph::from_paths(&["support-band.md", "zh-hans/support-band.md"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("support-band", "zh-hans/index.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Transclusion);
        assert_eq!(r.target_path.as_deref(), Some("zh-hans/support-band.md"));
        r.debug_check_invariant();
    }

    #[test]
    fn bare_embed_resolves_root_note_from_root_source() {
        // Both root and lang-tree notes exist; a root source resolves the root one
        // deterministically (source-relative join), never Ambiguous.
        let a = ContentGraph::from_paths(&["support-band.md", "zh-hans/support-band.md"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("support-band", "index.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Transclusion);
        assert_eq!(r.target_path.as_deref(), Some("support-band.md"));
        r.debug_check_invariant();
    }

    #[test]
    fn bare_embed_path_qualified_note_is_transclusion() {
        // `![[work/orchard]]` (path, no extension) resolves work/orchard.md.
        let a = ContentGraph::from_paths(&["work/orchard.md"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("work/orchard", "index.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Transclusion);
        assert_eq!(r.target_path.as_deref(), Some("work/orchard.md"));
    }

    #[test]
    fn bare_embed_unresolved_stays_link() {
        // No matching note → still a classify-only Link, not Transclusion.
        let a = ContentGraph::from_paths(&["other.md"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("support-band", "index.md", true, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Link { anchor: None });
        assert!(r.target_path.is_none());
    }

    #[test]
    fn bare_link_not_embed_does_not_use_md_fallback() {
        // is_embed=false resolves against the URL space, NOT the note-extension
        // fallback — a non-embed `[[support-band]]` must never become Transclusion.
        let a = ContentGraph::from_paths(&["support-band.md"]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::new();
        let r = classify_reference("support-band", "index.md", false, &ctx(&a, &f, &u));
        assert_ne!(r.kind, ReferenceKind::Transclusion);
    }

    #[test]
    fn non_embed_link_carries_anchor() {
        let a = ContentGraph::from_paths(&[]);
        let f = FakeFolderIndex::new();
        let u = FakeUrlIndex::resolving(&[("note", "/note/")]);
        let r = classify_reference("note#heading", "page.md", false, &ctx(&a, &f, &u));
        assert_eq!(r.kind, ReferenceKind::Link { anchor: Some("heading".into()) });
        assert_eq!(r.url.as_deref(), Some("/note/#heading"));
    }
}
