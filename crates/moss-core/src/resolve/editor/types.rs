//! Portable editor reference inputs and the shared editor wire types.
//! Path fields in core results are relative to the site root. A desktop
//! host's adapter joins them to the root, so its editor reads absolute paths;
//! a host that resolves over its own listing may keep them relative.

use crate::resolve::asset_class::AssetProvenance;
use crate::resolve::reference::ReferenceKind;

/// One reference to resolve: the raw target text plus whether it appeared in an
/// embed context (`![[...]]` / `![](...)`) or a plain link (`[[...]]` / `[](...)`).
/// `is_embed` is what drives the classifier's Link-vs-embed fork.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct RefTarget {
    pub text: String,
    pub is_embed: bool,
}

/// What the LAST BUILD listed under a folder, read from the build's URL map.
/// Attached to `FolderListing` embeds so the editor's card can state what will
/// be listed and how, without reimplementing the build's selection.
///
/// The fidelity limits of the underlying derivation are documented on
/// [`super::urls::FolderFacts`]; the editor renders these
/// counts as a bare number and makes no prose claim about build output.
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct FolderEmbedInfo {
    /// Title of the folder's index page. None when there is no index source or
    /// it cannot be read.
    pub title: Option<String>,
    /// Pages listed at `depth:direct` (articles + sub-folder index pages).
    pub direct_child_count: u32,
    /// Pages listed at `depth:all` (articles only, at any depth).
    pub descendant_count: u32,
}

/// Always-filled envelope for one resolved reference: never a bare Option, so
/// the editor can show lint diagnostics for unresolved/ambiguous references too.
/// This is the SINGLE envelope every editor consumer reads (image render,
/// link/asset lint, hover, cmd-click nav).
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct EditorReferenceResolution {
    /// The original reference text that was classified.
    pub target: String,
    /// Whether the reference was resolved in embed context.
    pub is_embed: bool,
    /// The classified kind (Image, Iframe, Folder kinds, Link, External, …).
    pub kind: ReferenceKind,
    /// For file/asset embed kinds: the enriched asset (path, URL, mime, LQIP).
    /// None for non-asset kinds and for a TOCTOU miss.
    pub resolved: Option<ResolvedAsset>,
    /// For Link kinds: the resolved deployed URL (carried from the classifier).
    /// None on a site with no URL map, whose links answer `resolved_path`.
    pub url: Option<String>,
    /// For Link kinds with a `#fragment`: the anchor.
    pub anchor: Option<String>,
    /// The source FILE this reference opens when followed: root-relative from
    /// moss-core, absolute once a desktop host's adapter has joined it to the
    /// site root. Set for:
    ///  * `Link` and `Moved`: the page's source, from the URL map, or the page
    ///    file itself on a site with no map;
    ///  * `FolderListing`: the folder's index source, NOT the folder directory;
    ///    with no index source in the map, the folder's home file;
    ///  * `FolderIndexIframe`: the folder's static `index.html`;
    ///  * `Transclusion`, `Notebook` and `Table`: the embedded file.
    ///
    /// Always a listed file, never a directory — every consumer opens it. A
    /// folder with neither an index source nor a home file leaves this `None`,
    /// so the editor shows no follow affordance rather than one that lands
    /// nowhere.
    pub resolved_path: Option<String>,
    /// Human-readable diagnostic note (separator-fallback / case-mismatch / …).
    pub message: Option<String>,
    /// For Ambiguous: all candidate paths.
    pub candidates: Vec<String>,
    /// For FolderListing embeds: what the last build listed. None for every
    /// other kind, and for a folder the article map does not know.
    pub folder: Option<FolderEmbedInfo>,
}

#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolvedAsset {
    /// The asset's file: root-relative from moss-core, absolute once a desktop
    /// host's adapter has joined it to the site root (and filled in its size
    /// and dimensions).
    pub absolute_path: String,
    pub request_url: String,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: u64,
    /// How the path was resolved: literal, bare-fuzzy, separator-fallback, or
    /// case-mismatch. Carried from the shared engine.
    ///
    /// Read by the editor off the unified reference envelope
    /// (`EditorReferenceResolution`), which marks a separator-fallback or
    /// case-mismatch path as a problem and offers the file's real path.
    /// The single-asset lookup (`resolve_asset`) returns it too.
    pub provenance: AssetProvenance,
    /// The ladder widths the current build holds for this image and can still
    /// vouch for, narrowest first: each is what `request_url?w=<width>` serves.
    /// Empty when there is no fresh rung or the host has no build; the original
    /// is never listed.
    pub rung_widths: Vec<u32>,
    /// The ladder stopped short of this image: a rung is built, and the source
    /// is no wider than the ladder's widest step, so no rung reaches its full
    /// width. The editor may then offer the original as its widest `srcset`
    /// candidate. A camera-sized original never qualifies.
    pub offers_original: bool,
}

/// Which files a reference may search. Loose folders only search the named folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EditorScope { Site, Folder }

/// A host's snapshot of the site. Paths use forward slashes and are root-relative.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolveInput {
    pub files: Vec<String>,
    /// Include empty directories; ancestors of files are inferred automatically.
    #[serde(default)]
    pub directories: Vec<String>,
    pub scope: EditorScope,
    pub from_source: String,
    pub targets: Vec<RefTarget>,
    #[serde(default)]
    pub url_index: Option<super::urls::UrlMap>,
    /// The folder's name, used to recognize its self-named home page.
    #[serde(default)]
    pub root_name: String,
    /// The site's [build].passthrough entries, including ! exclusions.
    #[serde(default)]
    pub passthrough: Vec<String>,
}
