//! Build-side `FolderIndex` over the content graph (`all_docs`) + static HTML
//! files, consulted by `classify_reference` to route `folder_embed::render_one`'s
//! three-branch decision. `dir_has_markdown_index` answers the vault-root
//! query off the doc's own `PageKind`/source location rather than
//! reconstructing "index.html" and comparing it to `url_path` — see that
//! method's comment for why the root case needs this and every other
//! directory doesn't.

use moss_core::content_graph::generate_slug;
use moss_core::resolve::asset_class::AssetIndex;
use moss_core::resolve::folder_class::FolderIndex;
use moss_core::resolve::link_class::UrlIndex;
use moss_core::PageKind;

use crate::build::types::ParsedDocument;
use crate::types::content::FileInfo;

pub struct BuildFolderIndex<'a> {
    pub docs: &'a [ParsedDocument],
    pub html_files: &'a [FileInfo],
}

impl<'a> FolderIndex for BuildFolderIndex<'a> {
    fn is_dir(&self, root_rel: &str) -> bool {
        // Only reached for non-trailing-slash paths (folder markers always have
        // a trailing slash → never hit this). Best-effort: a dir exists if any
        // html file or doc lives under it.
        if root_rel.is_empty() {
            return true;
        }
        let raw_prefix = format!("{}/", root_rel);
        let slug_prefix = format!("{}/", generate_slug(root_rel));
        self.html_files
            .iter()
            .any(|f| f.path.starts_with(&raw_prefix))
            || self.docs.iter().any(|d| d.url_path.starts_with(&slug_prefix))
    }

    fn dir_has_markdown_index(&self, root_rel: &str) -> bool {
        // Branch 1: a doc that IS `root_rel`'s folder index.
        //
        // For the vault ROOT specifically, ask the doc's own identity
        // (`PageKind::Folder` + its source file sitting directly in the
        // vault root) instead of reconstructing "index.html" and comparing
        // it against `url_path`. `url_path` is a derived string with its own
        // rules (home-file election, dedup, language prefixes) — a second
        // predicate that re-derives "index.html" independently can drift
        // from whatever decided `url_path` in the first place and silently
        // stop matching, exactly what happened when a root folder-note's
        // home election missed and its `url_path` became an ordinary slug
        // (moss#1101).
        //
        // Every OTHER directory still compares against `url_path`: that is
        // where `url:` overrides live (a source directory can be renamed for
        // the URL, e.g. `獎項/` → `awards/`), and `url_path` is the only
        // place that renaming is recorded — re-deriving identity from the
        // source path there would silently stop matching an overridden
        // folder (see `reference_parity.rs`'s `build_and_editor_folder_index_agree`).
        let target_slug = generate_slug(root_rel);
        if target_slug.is_empty() {
            return self.docs.iter().any(|d| d.kind == PageKind::Folder && is_root_source(d));
        }
        let target = format!("{}/index.html", target_slug);
        self.docs.iter().any(|d| d.url_path == target)
    }

    fn dir_has_static_index(&self, root_rel: &str) -> Option<String> {
        // Mirror Branch 2: a raw-path index.html/.htm in project.html_files.
        for name in ["index.html", "index.htm"] {
            let p = if root_rel.is_empty() {
                name.to_string()
            } else {
                format!("{}/{}", root_rel, name)
            };
            if self.html_files.iter().any(|f| f.path == p) {
                return Some(name.to_string());
            }
        }
        None
    }
}

/// Whether a `PageKind::Folder` doc is the vault root's index. Used only for
/// the root query in `dir_has_markdown_index` — see its comment for why the
/// root case reads identity instead of `url_path`.
///
/// A doc with a source file answers from that file's own location (no parent
/// directory = the vault root), which is what stays right when the doc's
/// `url_path` doesn't (moss#1101). A doc with no source file at all — a
/// synthesized folder index; term/language namespace roots have no backing
/// markdown — has no location to read, so it falls back to the one
/// unambiguous `url_path` check: a synthesized page is only ever the root's
/// index if its `url_path` is the bare `"index.html"`, never a re-derivation
/// of one.
pub(crate) fn is_root_source(doc: &ParsedDocument) -> bool {
    match doc.source_path.as_deref() {
        Some(sp) => match std::path::Path::new(sp).parent() {
            Some(p) => p.as_os_str().is_empty(),
            None => true,
        },
        None => doc.url_path == "index.html",
    }
}

/// Stub asset/url indexes for `classify_reference` calls that only need the
/// folder arm. Never consulted — folder markers short-circuit in
/// classify_reference's folder arm (their `parsed.path` always ends in `/`).
pub struct NoAssetIndex;

impl AssetIndex for NoAssetIndex {
    fn contains(&self, _root_rel: &str) -> bool {
        false
    }
    fn contains_ci(&self, _root_rel: &str) -> Option<String> {
        None
    }
    fn find_by_suffix(&self, _suffix: &str) -> Vec<String> {
        Vec::new()
    }
}

/// See `NoAssetIndex` — never consulted for folder markers.
pub struct NoUrlIndex;

impl UrlIndex for NoUrlIndex {
    fn lookup_exact(&self, _url_path: &str) -> bool {
        false
    }
    fn lookup_normalized(&self, _url_path: &str) -> Option<String> {
        None
    }
    fn resolve_reference_to_url(&self, _reference: &str, _from_source: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
#[path = "folder_index_tests.rs"]
mod tests;
