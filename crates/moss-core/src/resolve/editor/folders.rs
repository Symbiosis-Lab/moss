//! Folder predicates reconstructed from a host's file snapshot and build map.

use std::collections::{HashMap, HashSet};
use super::types::ResolveInput;
use crate::resolve::folder_class::FolderIndex;
use crate::source_edit::policy::{compute_passthrough_roots, is_in_passthrough};

pub struct EditorFolderIndex {
    dirs: HashSet<String>,
    folder_urls: HashSet<String>,
    static_index: HashMap<String, String>,
}

fn url_key(query: &str) -> String {
    crate::slug::slugify_path_segments(query.trim_matches('/'))
}

impl EditorFolderIndex {
    pub fn new(input: &ResolveInput) -> Self {
        let mut dirs: HashSet<String> = input.directories.iter().cloned().collect();
        let mut static_index = HashMap::new();
        let mut md_home_dirs = HashSet::new();
        let mut html = Vec::new();
        for rel in &input.files {
            let mut ancestor = rel.as_str();
            while let Some((parent, _)) = ancestor.rsplit_once('/') {
                dirs.insert(parent.to_string());
                ancestor = parent;
            }
            let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
            let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
            match ext.to_lowercase().as_str() {
                "html" | "htm" => {
                    html.push(rel.as_str());
                    if stem.eq_ignore_ascii_case("index") {
                        static_index.entry(parent.to_string()).or_insert_with(|| name.to_string());
                    }
                }
                "md" => {
                    let leaf = if parent.is_empty() { &input.root_name } else { parent.rsplit('/').next().unwrap_or(parent) };
                    if crate::home::is_home_file(stem, leaf) { md_home_dirs.insert(parent.to_string()); }
                }
                _ => {}
            }
        }
        let passthrough = compute_passthrough_roots(&html, &input.passthrough);
        let mut folder_urls = HashSet::new();
        let mut mapped_dirs = HashSet::new();
        let empty = super::urls::UrlMap::default();
        let map = input.url_index.as_ref().unwrap_or(&empty);
        let files: HashSet<&str> = input.files.iter().map(String::as_str).collect();
        for (url, src) in &map.pages {
            if !files.contains(src.as_str()) { continue; }
            folder_urls.insert(url.trim_end_matches('/').to_string());
            mapped_dirs.insert(src.rsplit_once('/').map_or("", |(parent, _)| parent).to_string());
        }
        for dir in &dirs {
            if dir.is_empty() || mapped_dirs.contains(dir) { continue; }
            let mapped = crate::resolve::output_url::resolve_path_with_overrides(&format!("{dir}/index.html"), &map.dir_overrides);
            let Some(key) = mapped.strip_suffix("/index.html").filter(|k| !k.is_empty()) else { continue; };
            let top = key.split('/').next().unwrap_or(key);
            let synthesized = crate::home::folder_language_code(top).is_none()
                && !is_in_passthrough(&format!("{dir}/"), &passthrough);
            if synthesized || md_home_dirs.contains(dir) { folder_urls.insert(key.to_string()); }
        }
        if md_home_dirs.contains("") { folder_urls.insert(String::new()); }
        Self { dirs, folder_urls, static_index }
    }
}

impl FolderIndex for EditorFolderIndex {
    fn is_dir(&self, root_rel: &str) -> bool {
        root_rel.is_empty() || self.dirs.contains(root_rel)
            || self.folder_urls.contains(&url_key(root_rel)) || self.static_index.contains_key(root_rel)
    }
    fn dir_has_markdown_index(&self, root_rel: &str) -> bool {
        self.folder_urls.contains(&url_key(root_rel))
    }
    fn dir_has_static_index(&self, root_rel: &str) -> Option<String> {
        self.static_index.get(root_rel).cloned()
    }
}

/// Only named home tiers are usable before a build elects a page or a listing.
pub fn home_file_in(input: &ResolveInput, folder: &str) -> Option<String> {
    let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
    let names: Vec<&str> = input.files.iter().chain(&input.directories)
        .filter_map(|file| file.strip_prefix(&prefix))
        .filter_map(|name| name.split('/').next()).filter(|name| !name.is_empty()).collect();
    let folder_name = if folder.is_empty() { &input.root_name } else { folder.rsplit('/').next()? };
    let pick = crate::home::detect_home_file_in_folder(&names, folder_name)?;
    let stem = std::path::Path::new(pick).file_stem()?.to_str()?;
    let path = format!("{prefix}{pick}");
    (crate::home::is_home_file(stem, folder_name) && input.files.contains(&path)).then_some(path)
}

#[cfg(test)]
#[path = "folders_tests.rs"]
mod tests;
