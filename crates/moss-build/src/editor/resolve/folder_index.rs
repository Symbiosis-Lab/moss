//! Editor-side [`FolderIndex`] — a reconstruction of the BUILD's folder-index
//! URL set, not an independent filesystem opinion.
//!
//! ## Why this is not a `read_dir` for `index.md`
//!
//! The build decides `![[/X/]]` in URL space: `BuildFolderIndex::
//! dir_has_markdown_index` asks whether any document's `url_path` is
//! `slug(X)/index.html`. That document set has two halves, and the editor's old
//! filesystem answer could see neither:
//!
//! 1. **Source-backed folder-index docs.** `獎項/獎項.md` carrying `url: awards`
//!    in its frontmatter is served at `awards/index.html`. `![[/awards/]]`
//!    renders a listing in the build; a `read_dir` of `<root>/awards` finds
//!    nothing.
//! 2. **Synthesized folder-index docs.** `build/render/blocking.rs` pushes a
//!    source-less `PageKind::Folder` document for EVERY scanned directory that
//!    is not in a passthrough subtree, not under a language prefix, and has no
//!    explicit index document. So `![[/writings/]]` renders a listing even
//!    though `writings/` contains no `index.md` at all.
//!
//! So this index reconstructs both halves from the same three inputs the build
//! uses — the last build's [`ArticleMap`] (half 1, plus its `dir_overrides`),
//! the scanned directory set, and the passthrough/language exclusions (half 2).
//! Every rule is CALLED, never re-implemented: [`crate::build::scan::classify::is_excluded_dir_name`],
//! `classify::compute_passthrough_roots`,
//! `crate::build::site_config::get_build_passthrough`,
//! [`moss_core::home::is_home_file`],
//! `moss_core::resolve::output_url::resolve_path_with_overrides`, and
//! `crate::vault_root::VaultRoot::name`.
//!
//! ## Parity direction
//!
//! With a fresh article map the editor and the build agree exactly. Where they
//! cannot (no build has ever run, or a `url:` override was added since the last
//! build), the editor is deliberately GREENER than the build: a false red
//! blocks an author over a divergence the next build will resolve on its own,
//! which costs more than a false green here.
//!
//! One known hazard from that choice: deleting a real directory named `awards/`
//! while `awards` is ALSO a frontmatter `url:` override for some other folder
//! makes `ref_scan` offer `![[/awards/]]` for rewrite, because `target_path`
//! for a folder reference is the URL-space query string.

use crate::build::scan::article_map::ArticleMap;
use crate::types::content::FileInfo;
use moss_core::resolve::folder_class::FolderIndex;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct EditorFolderIndex {
    root: PathBuf,
    /// `VaultRoot::name()` — the load-bearing folder-name token. Never
    /// re-derived from `root` (see `vault/paths.rs`).
    root_name: String,
    /// `ArticleMap::pages`: pretty URL → source file. The half of the build's
    /// folder-index document set that carries `url:` frontmatter overrides.
    map_pages: HashMap<String, String>,
    /// `ArticleMap::dir_overrides`: source dir prefix → URL segment.
    dir_overrides: HashMap<String, String>,
    /// Built on the first folder-shaped query, never in `new()`: a document
    /// with no folder-shaped reference must pay nothing.
    scan: OnceCell<VaultScan>,
}

struct VaultScan {
    /// The build's folder-index URL set, reconstructed. Keys are URL-space,
    /// no leading or trailing `/`; `""` is the site root.
    folder_urls: HashSet<String>,
    /// Raw dir path → `index.html` | `index.htm` (the iframe branch).
    static_index: HashMap<String, String>,
}

/// URL-space key for a query the author wrote.
///
/// `slug::slugify_path_segments`, NOT `content_graph::generate_slug`: the
/// latter strips the trailing segment's extension, so `assets/photo.png` would
/// key as `assets/photo` and a real `assets/photo/` directory would then make
/// an image embed classify as a folder listing. `classify_reference` calls
/// `is_dir` for EVERY leading-slash reference, absolute file embeds included,
/// so that collision is reachable.
fn url_key(query: &str) -> String {
    moss_core::slug::slugify_path_segments(query.trim_matches('/'))
}

/// The build's key for a scanned source directory: append `/index.html` so
/// every directory segment is slugified/override-mapped and the leaf drops
/// out. Mirrors the synthesis block in `build/render/blocking.rs`.
fn dir_url_key(dir: &str, overrides: &HashMap<String, String>) -> Option<String> {
    let mapped = moss_core::resolve::output_url::resolve_path_with_overrides(
        &format!("{}/index.html", dir),
        overrides,
    );
    mapped
        .strip_suffix("/index.html")
        .filter(|k| !k.is_empty())
        .map(str::to_string)
}

impl EditorFolderIndex {
    /// The ONE constructor. Both production call sites already load the map;
    /// pass `&ArticleMap::default()` where no build has ever run.
    pub fn new(project_root: &Path, map: &ArticleMap) -> Self {
        EditorFolderIndex {
            root: project_root.to_path_buf(),
            root_name: crate::vault_root::VaultRoot::resolve(project_root)
                .name()
                .to_string(),
            map_pages: map.pages.clone(),
            dir_overrides: map.dir_overrides.clone(),
            scan: OnceCell::new(),
        }
    }

    fn scan(&self) -> &VaultScan {
        self.scan.get_or_init(|| self.build_scan())
    }

    /// One vault walk, at most once per index.
    fn build_scan(&self) -> VaultScan {
        let mut raw_dirs: HashSet<String> = HashSet::new();
        let mut static_index: HashMap<String, String> = HashMap::new();
        let mut html_files: Vec<FileInfo> = Vec::new();
        // Directories (root-relative; `""` is the vault root) holding a real
        // home/index markdown file.
        let mut md_home_dirs: HashSet<String> = HashSet::new();

        // Same predicate, same shape as the build scan's WalkDir
        // (`build/scan/scan.rs`), reached through the crate-root re-export so
        // there is ONE exclusion rule, not two.
        let walker = walkdir::WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|e| {
                // depth 0 is the vault root itself: its own name is not subject
                // to the exclusion rule (a vault may legitimately live in a
                // dot-directory, and pruning it would empty the whole walk).
                if e.depth() == 0 || !e.file_type().is_dir() {
                    return true;
                }
                !crate::build::scan::classify::is_excluded_dir_name(&e.file_name().to_string_lossy())
            });

        for entry in walker.flatten() {
            let rel = match entry.path().strip_prefix(&self.root) {
                Ok(r) => moss_core::slug::normalize_separators(&r.to_string_lossy()),
                Err(_) => continue,
            };
            if entry.file_type().is_dir() {
                // Skip the root itself (depth 0) — the build never synthesizes
                // a root folder index; only a real home file counts there.
                if entry.depth() > 0 && !rel.is_empty() {
                    raw_dirs.insert(rel);
                }
                continue;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let parent = rel.rsplit_once('/').map(|(p, _)| p).unwrap_or("").to_string();
            let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
            let (stem, ext) = match name.rsplit_once('.') {
                Some((s, e)) => (s.to_string(), e.to_lowercase()),
                None => (name.clone(), String::new()),
            };
            match ext.as_str() {
                "html" | "htm" => {
                    html_files.push(FileInfo {
                        path: rel.clone(),
                        file_type: ext.clone(),
                        size: 0,
                        modified: None,
                    });
                    if stem.eq_ignore_ascii_case("index") {
                        // The REAL dirent name, not a lowercased reconstruction:
                        // the editor joins this to open the file, and `Index.HTML`
                        // does not exist under that name on a case-sensitive
                        // filesystem.
                        static_index.entry(parent).or_insert_with(|| name.clone());
                    }
                }
                "md" | "markdown" => {
                    let parent_leaf = if parent.is_empty() {
                        self.root_name.as_str()
                    } else {
                        parent.rsplit('/').next().unwrap_or(&parent)
                    };
                    if moss_core::home::is_home_file(&stem, parent_leaf) {
                        md_home_dirs.insert(parent);
                    }
                }
                _ => {}
            }
        }

        // The build's own passthrough computation over the build's own config
        // reader — no second auto-detection rule, no second TOML parse.
        let passthrough_roots = crate::build::scan::classify::compute_passthrough_roots(
            &html_files,
            &crate::build::site_config::get_build_passthrough(&self.root.to_string_lossy())
                .unwrap_or_default(),
        );

        // Half (A): source-backed folder-index documents, straight from the
        // map. A directory whose folder-index document the map already claims
        // contributes ONLY that document's URL key, never its own directory key
        // — otherwise `![[/獎項/]]` goes green in the editor while the build
        // (whose doc lives at `awards/index.html`) emits a missing embed.
        // `is_file` is the staleness guard: a mapped source that no longer
        // exists is a dead entry and must not suppress anything.
        let mut folder_urls: HashSet<String> = HashSet::new();
        let mut mapped_dirs: HashSet<String> = HashSet::new();
        for (pretty_url, src) in &self.map_pages {
            if !self
                .root
                .join(src.replace('/', std::path::MAIN_SEPARATOR_STR))
                .is_file()
            {
                continue;
            }
            folder_urls.insert(pretty_url.trim_end_matches('/').to_string());
            mapped_dirs.insert(
                moss_core::slug::normalize_separators(src)
                    .rsplit_once('/')
                    .map(|(p, _)| p.to_string())
                    .unwrap_or_default(),
            );
        }

        // Half (B): the directories the build synthesizes a folder index for.
        for dir in &raw_dirs {
            if mapped_dirs.contains(dir) {
                continue;
            }
            let Some(key) = dir_url_key(dir, &self.dir_overrides) else {
                continue;
            };
            let top = key.split('/').next().unwrap_or(&key);
            let synthesized = crate::i18n::path::resolve_language_from_folder(top).is_none()
                && !crate::build::scan::classify::is_in_passthrough(
                    &format!("{}/", dir),
                    &passthrough_roots,
                );
            // A real home file makes the folder a listing regardless of
            // synthesis — the build has a real doc there either way.
            if synthesized || md_home_dirs.contains(dir) {
                folder_urls.insert(key);
            }
        }
        if md_home_dirs.contains("") {
            folder_urls.insert(String::new());
        }

        VaultScan {
            folder_urls,
            static_index,
        }
    }
}

impl FolderIndex for EditorFolderIndex {
    fn is_dir(&self, root_rel: &str) -> bool {
        // The vault root always exists — matches `BuildFolderIndex::is_dir`.
        if root_rel.is_empty() {
            return true;
        }
        // Literal on-disk directory next: covers every ordinary reference
        // without forcing the walk.
        if self
            .root
            .join(root_rel.replace('/', std::path::MAIN_SEPARATOR_STR))
            .is_dir()
        {
            return true;
        }
        let s = self.scan();
        s.folder_urls.contains(&url_key(root_rel)) || s.static_index.contains_key(root_rel)
    }

    fn dir_has_markdown_index(&self, root_rel: &str) -> bool {
        self.scan().folder_urls.contains(&url_key(root_rel))
    }

    fn dir_has_static_index(&self, root_rel: &str) -> Option<String> {
        self.scan().static_index.get(root_rel).cloned()
    }
}

#[cfg(test)]
#[path = "folder_index_tests.rs"]
mod tests;
