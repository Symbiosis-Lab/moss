//! File-list lookups for editor references; no filesystem access.

use crate::content_graph::{normalize_path, ContentGraph, ContentGraphBuilder, PathMatch};
use crate::resolve::asset_class::{resolve_file_target, AssetProvenance, AssetResolution};
use std::path::Path;

pub fn spelled_out_file(target: &str, from_source: &str, all_files: &[String]) -> Option<AssetResolution> {
    if target.contains('%') {
        return None;
    }
    let mut files = std::collections::BTreeSet::new();
    for folder in &named_folders(target, from_source) {
        files.extend(site_files_in(all_files, folder)?);
    }
    let keys: std::collections::HashSet<String> = files.iter().map(|f| normalize_path(f)).collect();
    if keys.len() != files.len() {
        return None;
    }
    let mut builder = ContentGraphBuilder::new();
    for f in &files {
        builder.add_file(f, "");
    }
    let graph = builder.build();
    let found = graph.resolve_path_with_ties(target, from_source)?;
    (found.matched != PathMatch::Searched).then(|| resolve_file_target(target, from_source, &graph))
}

/// The folders a spelled-out `target` can sit in: from the site root, and,
/// unless it starts with `/`, from the page's folder.
pub fn named_folders(target: &str, from_source: &str) -> Vec<String> {
    let folder_of = |path: &str| path.rsplit_once('/').map_or(String::new(), |(dir, _)| dir.to_string());
    let rel = target.trim_start_matches('/');
    let mut folders = vec![folder_of(rel)];
    if !target.starts_with('/') {
        folders.push(format!("{}/{}", folder_of(from_source), folder_of(rel)));
    }
    folders
}

/// Files directly in the written folder, retaining real case. Ambiguous
/// case-folded directory names cannot be answered by the narrow lookup.
pub fn site_files_in(files: &[String], folder: &str) -> Option<Vec<String>> {
    let Some(joined) = crate::content_graph::join_written("", folder) else { return Some(Vec::new()) };
    let mut real = String::new();
    for seg in joined.split('/').filter(|s| !s.is_empty()) {
        let key = normalize_path(seg);
        let candidates: std::collections::BTreeSet<&str> = files.iter()
            .filter_map(|f| f.strip_prefix(&real)?.split_once('/').map(|(dir, _)| dir))
            .filter(|dir| normalize_path(dir) == key).collect();
        if candidates.len() > 1 { return None; }
        let Some(dir) = candidates.first() else { return Some(Vec::new()) };
        real.push_str(dir);
        real.push('/');
    }
    Some(files.iter().filter(|f| f.strip_prefix(&real).is_some_and(|name| !name.contains('/'))).cloned().collect())
}

pub fn graph(files: &[String]) -> ContentGraph {
    let mut builder = ContentGraphBuilder::new();
    for file in files { builder.add_file(file, ""); }
    builder.build()
}

pub fn scope_graph(files: &[String], scope: super::types::EditorScope, from: &str, target: &str) -> ContentGraph {
    if scope == super::types::EditorScope::Site { return graph(files); }
    let mut folders = std::collections::BTreeSet::from([String::new()]);
    folders.extend(named_folders(target, from));
    let selected: Vec<String> = folders.iter().filter_map(|folder| site_files_in(files, folder)).flatten().collect();
    graph(&selected)
}

/// Asset metadata available without reading bytes. Hosts fill dimensions and size.
pub fn asset(root_rel: &str, provenance: AssetProvenance) -> super::types::ResolvedAsset {
    super::types::ResolvedAsset {
        absolute_path: root_rel.to_string(), request_url: format!("/{root_rel}"),
        mime_type: mime_for_path(Path::new(root_rel)), width: None, height: None,
        size_bytes: 0, provenance, rung_widths: Vec::new(), offers_original: false,
    }
}

pub fn mime_for_path(p: &Path) -> String {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        Some("tiff") | Some("tif") => "image/tiff",
        Some("heic") | Some("heif") => "image/heic",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogg") => "audio/ogg",
        Some("mov") => "video/quicktime",
        Some("m4v") => "video/x-m4v",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("flac") => "audio/flac",
        Some("aac") => "audio/aac",
        Some("m4a") => "audio/mp4",
        Some("opus") => "audio/opus",
        Some("pdf") => "application/pdf",
        Some("gltf") => "model/gltf+json",
        Some("glb") => "model/gltf-binary",
        Some("usdz") => "model/vnd.usdz+zip",
        Some("ipynb") => "application/x-ipynb+json",
        _ => "application/octet-stream",
    }
    .to_string()
}


/// Whether a build walk of the site reads `path` (site-relative): the walk's
/// rule applied to each folder on the way down and to the entry itself.
/// `nested` holds the folders that contain a `.moss/` of their own.
pub fn site_member(path: &str, is_dir: bool, nested: &std::collections::HashSet<&str>) -> bool {
    use crate::source_edit::policy::left_out;
    let count = path.split('/').count();
    let mut prefix = String::new();
    for (i, segment) in path.split('/').enumerate() {
        if i > 0 { prefix.push('/'); }
        prefix.push_str(segment);
        let folder = i + 1 < count || is_dir;
        if left_out(segment, folder, i + 1, || nested.contains(prefix.as_str())).is_some() { return false; }
    }
    true
}

/// Folders below the root that hold their own `.moss/`, read from the paths.
pub fn nested_sites<'a>(paths: impl Iterator<Item = &'a String>) -> std::collections::HashSet<&'a str> {
    paths.filter_map(|p| p.split_once("/.moss/").map(|(site, _)| site).or_else(|| p.strip_suffix("/.moss"))).collect()
}
