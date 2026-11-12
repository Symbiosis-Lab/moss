//! Editor reference resolution over a file snapshot. Hosts supply paths and
//! optional build facts, then enrich returned paths with their own metadata.

pub mod files;
pub mod folders;
pub mod types;
pub mod urls;
pub use urls::UrlMap;

pub use types::{EditorReferenceResolution, EditorScope, FolderEmbedInfo, RefTarget, ResolvedAsset, ResolveInput};
use folders::{EditorFolderIndex, home_file_in};
use urls::{ArticleMapIndex, FolderFacts};
use super::folder_class::{FolderIndex, NoFolderIndex};
use super::link_class::UrlIndex;
use super::reference::{classify_reference, ReferenceContext, ReferenceKind};
use super::asset_class::{AssetProvenance, AssetResolution, resolve_file_target};
use crate::content_graph::ContentGraph;

impl ResolveInput {
    /// This snapshot with only what a build walk of the site reads. A phone
    /// lists the whole folder, `.git` and `.moss/` included; resolving over
    /// that would find files the build never sees.
    pub fn members(&self) -> ResolveInput {
        let nested = files::nested_sites(self.files.iter().chain(&self.directories));
        let keep = |paths: &[String], is_dir| paths.iter().filter(|p| files::site_member(p, is_dir, &nested)).cloned().collect();
        ResolveInput { files: keep(&self.files, false), directories: keep(&self.directories, true), ..self.clone() }
    }
}

pub fn resolve_references(input: &ResolveInput) -> Vec<EditorReferenceResolution> {
    let input = &input.members();
    let site = input.scope == EditorScope::Site;
    let graph = site.then(|| files::graph(&input.files));
    let folder_index = site.then(|| EditorFolderIndex::new(input));
    let folders: &dyn FolderIndex = match &folder_index { Some(index) => index, None => &NoFolderIndex };
    let map = input.url_index.as_ref().filter(|_| site);
    let urls = map.map(ArticleMapIndex::from_map);
    let empty_map = UrlMap::default();
    let facts = FolderFacts::from_map(map.unwrap_or(&empty_map));
    let listed: std::collections::HashSet<&str> = input.files.iter().map(String::as_str).collect();
    input.targets.iter().map(|target| {
        let norm = target.text.replace('\\', "/");
        let loose;
        let assets = match &graph {
            Some(graph) => graph,
            None => { loose = files::scope_graph(&input.files, input.scope, &input.from_source, &norm); &loose }
        };
        let pages = FolderPages(assets);
        let urls: &dyn UrlIndex = urls.as_ref().map_or(&pages as &dyn UrlIndex, |idx| idx);
        let resolved = classify_reference(&norm, &input.from_source, target.is_embed,
            &ReferenceContext { assets, folders, urls });
        let anchor = match &resolved.kind { ReferenceKind::Link { anchor } => anchor.clone(), _ => None };
        let mut out = EditorReferenceResolution {
            target: target.text.clone(), is_embed: target.is_embed, kind: resolved.kind.clone(),
            resolved: None, url: resolved.url.clone(), anchor, resolved_path: None,
            message: resolved.message, candidates: resolved.candidates, folder: None,
        };
        if let Some(rel) = resolved.target_path.as_deref() {
            match &resolved.kind {
                ReferenceKind::Image | ReferenceKind::Iframe | ReferenceKind::Pdf | ReferenceKind::Video
                | ReferenceKind::Audio | ReferenceKind::Model if listed.contains(rel) => {
                    out.resolved = Some(files::asset(rel, resolved.provenance.unwrap_or(AssetProvenance::Literal)));
                }
                ReferenceKind::FolderListing => {
                    out.folder = facts.lookup(rel);
                    out.resolved_path = facts.index_source_for(rel)
                        .filter(|src| listed.contains(src)).map(str::to_string)
                        .or_else(|| home_file_in(input, rel));
                }
                ReferenceKind::FolderIndexIframe => {
                    out.resolved_path = folders.dir_has_static_index(rel)
                        .map(|name| if rel.is_empty() { name } else { format!("{rel}/{name}") })
                        .filter(|src| listed.contains(src.as_str()));
                }
                ReferenceKind::Transclusion | ReferenceKind::Notebook | ReferenceKind::Table if listed.contains(rel) => {
                    out.resolved_path = Some(rel.to_string());
                }
                _ => {}
            }
        }
        if matches!(out.kind, ReferenceKind::Link { .. } | ReferenceKind::Moved { .. }) {
            if let Some(map) = map {
                out.resolved_path = out.url.as_ref().and_then(|url| map.source_path_for_url(url))
                    .filter(|src| listed.contains(src.as_str()));
            } else {
                out.resolved_path = out.url.take().map(|source| source.split('#').next().unwrap_or(&source).to_string());
            }
        }
        out
    }).collect()
}

/// A file-backed link answers its source path, not a deployed URL. Used only
/// here, when there is no URL map (a loose folder, or a site never built), and
/// the answer leaves as `resolved_path` with `url` empty; the build resolves
/// links through its own map and never calls this.
struct FolderPages<'a>(&'a ContentGraph);
impl UrlIndex for FolderPages<'_> {
    fn lookup_exact(&self, _: &str) -> bool { false }
    fn lookup_normalized(&self, _: &str) -> Option<String> { None }
    fn resolve_reference_to_url(&self, reference: &str, from_source: &str) -> Option<String> {
        std::iter::once(reference.to_string()).chain(crate::page_kind::PAGE_EXTENSIONS.iter().map(|ext| format!("{reference}.{ext}"))).find_map(|written| {
            match resolve_file_target(&written, from_source, self.0) {
                AssetResolution::Resolved { root_rel, .. } if crate::source_edit::policy::is_page_path(std::path::Path::new(&root_rel)) => Some(root_rel),
                _ => None,
            }
        })
    }
}

/// The singular asset command also accepts documents and picks the build's
/// winner for an ambiguous name; the batch envelope keeps the ambiguity.
pub fn resolve_asset(input: &ResolveInput, target: &str) -> Option<ResolvedAsset> {
    let input = &input.members();
    let target = target.replace('\\', "/");
    let graph = files::scope_graph(&input.files, input.scope, &input.from_source, &target);
    let picked = files::spelled_out_file(&target, &input.from_source, &input.files)
        .unwrap_or_else(|| resolve_file_target(&target, &input.from_source, &graph));
    match picked {
        AssetResolution::Resolved { root_rel, provenance } => Some(files::asset(&root_rel, provenance)),
        AssetResolution::Ambiguous { chosen, .. } => Some(files::asset(&chosen, AssetProvenance::SeparatorFallback)),
        AssetResolution::NotFound => None,
    }
}

#[cfg(test)]
mod tests;
