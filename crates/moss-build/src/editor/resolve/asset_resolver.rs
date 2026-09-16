//! Editor-facing asset resolution.
//!
//! Resolves asset references for the editor using direct filesystem access —
//! no dependency on the build's AssetRegistry or WebP/LQIP pipeline.
//! The editor renders source images directly via `moss-source://`.

use moss_core::resolve::asset_class::{AssetIndex, AssetProvenance, AssetResolution, resolve_asset_ref};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ResolvedAsset {
    pub absolute_path: String,
    pub request_url: String,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: u64,
    /// How the path was resolved: literal, bare-fuzzy, separator-fallback, or
    /// case-mismatch. Carried from the shared engine (ADR-018).
    ///
    /// Read directly by the editor's asset-advisory lint (cm-asset-lint.ts),
    /// which pulls `env.resolved.provenance` off the unified reference envelope
    /// (`EditorReferenceResolution`) to warn on separator-fallback / case-mismatch.
    /// Also exposed by the singular `editor_resolve_asset` (chip-bar) command.
    pub provenance: AssetProvenance,
}

/// Filesystem-backed `AssetIndex` implementation for the editor resolver.
///
/// All three methods operate on forward-slash, project-root-relative paths.
///
/// CRITICAL macOS / APFS note:
/// `contains` uses `read_dir` + exact byte comparison — NOT `Path::exists()` —
/// because APFS is case-insensitive: `Path::exists("assets/hoon.jpg")` returns
/// true even when the real file is `assets/Hoon.JPG`. Exact-case is necessary
/// so `CaseMismatch` provenance is produced for the Hoon scenario.
pub struct FsAssetIndex {
    project_root: PathBuf,
}

impl FsAssetIndex {
    pub fn new(project_root: &Path) -> Self {
        Self { project_root: project_root.to_path_buf() }
    }

    /// Convert a root-relative forward-slash path to an absolute path.
    fn abs(&self, root_rel: &str) -> PathBuf {
        self.project_root.join(root_rel.replace('/', std::path::MAIN_SEPARATOR_STR))
    }
}

impl AssetIndex for FsAssetIndex {
    /// Exact-case check: split root_rel into parent dir + filename,
    /// read_dir the parent, and compare filename byte-for-byte.
    fn contains(&self, root_rel: &str) -> bool {
        let path = self.abs(root_rel);
        let parent = match path.parent() {
            Some(p) => p,
            None => return false,
        };
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => return false,
        };
        let entries = match std::fs::read_dir(parent) {
            Ok(e) => e,
            Err(_) => return false,
        };
        for entry in entries.flatten() {
            if entry.file_name().to_str() == Some(file_name) {
                return true;
            }
        }
        false
    }

    /// Case-insensitive check: read_dir the parent, find an entry whose name
    /// equals `root_rel`'s filename case-insensitively, and return the
    /// canonical root-relative path using the entry's REAL on-disk name.
    fn contains_ci(&self, root_rel: &str) -> Option<String> {
        let path = self.abs(root_rel);
        let parent = path.parent()?;
        let file_name = path.file_name()?.to_str()?;
        let file_name_lower = file_name.to_lowercase();

        // Determine the parent's root-relative forward-slash prefix.
        let parent_rel = parent
            .strip_prefix(&self.project_root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");

        let entries = std::fs::read_dir(parent).ok()?;
        for entry in entries.flatten() {
            let entry_name = entry.file_name();
            let entry_str = entry_name.to_str()?;
            if entry_str.to_lowercase() == file_name_lower && entry_str != file_name {
                // Found a case-insensitive match with a DIFFERENT real case.
                let canonical = if parent_rel.is_empty() {
                    entry_str.to_string()
                } else {
                    format!("{}/{}", parent_rel, entry_str)
                };
                return Some(canonical);
            }
        }
        None
    }

    /// Walk the WHOLE project tree collecting real-case root-relative paths whose
    /// suffix matches `suffix`. Breadth + exclusions match the BUILD scan
    /// (`crate::build::scan::classify::is_excluded_dir_name`, applied inside `walk_collect`) so the editor
    /// and build resolve the same source-file set — no depth cap, no hardcoded
    /// asset-folder allow-list. (Parity: see plan-b / ADR-020.)
    fn find_by_suffix(&self, suffix: &str) -> Vec<String> {
        let mut results = Vec::new();
        walk_collect(&self.project_root, suffix, &self.project_root, usize::MAX, &mut results);
        results
    }
}

/// Compute the project-root-relative forward-slash source path from `from_file`.
///
/// from_file may not exist (new/unsaved file). We try two strategies:
///   1. Canonicalize from_file's parent directory (which should exist), then
///      reconstruct the path with the original filename. This gives the real-case
///      parent prefix without requiring the file itself to exist.
///   2. Fall back to lexical strip_prefix against canonical_root.
///   3. Last resort: basename only.
fn compute_from_source(from_file: &Path, canonical_root: &Path) -> String {
    // The editor passes a PROJECT-ROOT-RELATIVE path (e.g. "News/post.md", from
    // `getCurrentEntry().path`). A relative path IS already the root-relative
    // source — return it directly. Do NOT fall through to the canonicalize-parent
    // logic below, which would resolve "News" against the process CWD (not the
    // project root), fail, and collapse to the basename — making the engine treat
    // the file as root-level so `./assets/X` resolves as Literal (warning lost).
    if from_file.is_relative() {
        return from_file.to_string_lossy().replace('\\', "/");
    }

    let file_name = from_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Try to canonicalize the parent directory (which exists even when the file doesn't).
    let parent_canon: Option<PathBuf> = from_file
        .parent()
        .and_then(|p| std::fs::canonicalize(p).ok());

    if let Some(parent) = parent_canon {
        match parent.strip_prefix(canonical_root) {
            Ok(rel_parent) => {
                let rel_parent_str = rel_parent.to_string_lossy().replace('\\', "/");
                if rel_parent_str.is_empty() {
                    file_name
                } else {
                    format!("{}/{}", rel_parent_str, file_name)
                }
            }
            Err(_) => {
                // Parent is outside project root — use basename only.
                file_name
            }
        }
    } else {
        // Parent doesn't exist either; try lexical strip_prefix as last resort.
        match from_file.strip_prefix(canonical_root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => file_name,
        }
    }
}

/// Enrich a root-relative path into a full `ResolvedAsset` by reading disk
/// metadata directly. No registry or WebP/LQIP dependency — the editor
/// renders source images directly via `moss-source://`.
///
/// Returns `None` only when the file has vanished between the engine's index
/// lookup and this call (i.e. a TOCTOU miss). Callers degrade to an unresolved
/// reference (kind:not-found) in that case.
fn enrich(
    root_rel: &str,
    provenance: AssetProvenance,
    canonical_root: &Path,
) -> Option<ResolvedAsset> {
    let absolute = canonical_root.join(root_rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !absolute.exists() {
        return None;
    }
    let request_url = absolute_to_request_url(&absolute, canonical_root)?;
    let meta = std::fs::metadata(&absolute).ok()?;
    let mime_type = mime_for_path(&absolute);
    let (width, height) = if mime_type.starts_with("image/") {
        crate::build::scan::scan::extract_image_dimensions(&absolute)
            .map(|(w, h)| (Some(w), Some(h))).unwrap_or((None, None))
    } else { (None, None) };
    Some(ResolvedAsset {
        absolute_path: absolute.to_string_lossy().to_string(),
        request_url, mime_type, width, height, size_bytes: meta.len(), provenance,
    })
}

/// Enrich a root-relative SOURCE path (as produced by `classify_reference`'s
/// `target_path`) into a full `ResolvedAsset`, reusing the single `enrich`
/// implementation so the reference command never duplicates that logic.
///
/// `project_root` is canonicalized here (mirroring `resolve_asset`) so the
/// caller can pass the raw project root. `provenance` carries the engine's
/// classification (defaults to `Literal` when the caller has none). Returns
/// `None` only on a TOCTOU miss (file vanished between classify and enrich) —
/// the caller should degrade to a not-found envelope.
pub fn enrich_root_rel(
    root_rel: &str,
    provenance: AssetProvenance,
    project_root: &Path,
) -> Option<ResolvedAsset> {
    let canonical_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    enrich(root_rel, provenance, &canonical_root)
}

/// Resolve a single reference. Returns None when the path doesn't resolve
/// to a known asset.
///
/// Path resolution is delegated to the shared engine (`resolve_asset_ref` in
/// moss-core, ADR-018). The engine drives an `FsAssetIndex` that does exact-case
/// `read_dir` checks — bypassing `Path::exists()` which is case-blind on macOS
/// APFS — so `CaseMismatch` provenance is correctly produced when the authored
/// path differs only in case from the real file.
///
/// Used by the singular `editor_resolve_asset` Tauri command (chip-bar asset
/// widget) which needs a bare `Option<ResolvedAsset>`. Editor lint diagnostics
/// go through the unified reference resolver (`reference_resolver.rs`), which
/// reuses `enrich_root_rel` for the same enrichment.
pub fn resolve_asset(
    target: &str,
    from_file: &Path,
    project_root: &Path,
) -> Option<ResolvedAsset> {
    // Canonicalize project_root so strip_prefix works correctly after the engine
    // returns a root-relative path (both paths must share the same real-path prefix,
    // not just a lexical one — important on macOS where /tmp is a symlink to
    // /private/var/folders/...).
    let canonical_root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());

    let from_source = compute_from_source(from_file, &canonical_root);
    let index = FsAssetIndex::new(&canonical_root);
    let target_norm = target.replace('\\', "/");

    let (root_rel, provenance) = match resolve_asset_ref(&target_norm, &from_source, &index) {
        AssetResolution::Resolved { root_rel, provenance } => (root_rel, provenance),
        AssetResolution::Ambiguous { chosen, .. } => {
            // Multiple candidates found — use the shortest-path winner chosen by the
            // engine. Treat as SeparatorFallback provenance (non-literal resolution).
            // The unified reference resolver models Ambiguous properly in its envelope
            // (kind:ambiguous + candidates); this singular path just picks the winner.
            (chosen, AssetProvenance::SeparatorFallback)
        }
        AssetResolution::NotFound => return None,
    };

    enrich(&root_rel, provenance, &canonical_root)
}

/// Recursively walk `dir` up to `max_depth` levels deep, collecting ALL files
/// whose project-root-relative forward-slash path ends with `suffix`.
///
/// Results are appended to `out` as real-case root-relative paths (no leading slash).
/// Excluded dirs (`.moss`, `node_modules`, etc.) are skipped — same rule as the build
/// pipeline — so `.moss/build/staging` shadow copies are never returned.
fn walk_collect(dir: &Path, suffix: &str, project_root: &Path, max_depth: usize, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Use the dirent's own type — it does NOT follow symlinks (unlike
        // `path.is_file()` / `path.is_dir()`, which stat the target). A directory
        // symlink is therefore neither file nor dir here and is skipped, matching
        // the build's `walkdir::WalkDir` (no-follow by default). This prevents a
        // dir-symlink cycle from causing unbounded recursion (max_depth = usize::MAX)
        // and keeps editor↔build parity (no symlinked-duplicate paths).
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_file() {
            // Compute real-case root-relative path.
            if let Ok(rel) = path.strip_prefix(project_root) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                // Match case-INSENSITIVELY (mirrors the build's
                // ContentGraph::asset_find_by_suffix) so a bare `hoon.jpg` whose
                // real file is `assets/Hoon.JPG` resolves on BOTH adapters — the
                // editor must never be redder than the build (review S1). The
                // pushed value stays real-case (`rel_str`) for the canonical URL.
                let rel_lower = rel_str.to_lowercase();
                let suffix_lower = suffix.to_lowercase();
                if rel_lower == suffix_lower || rel_lower.ends_with(&format!("/{}", suffix_lower)) {
                    // Avoid duplicates (root walk and asset-dir walk may overlap).
                    if !out.contains(&rel_str) {
                        out.push(rel_str);
                    }
                }
            }
        } else if ft.is_dir() && max_depth > 0 {
            // Mirror the build pipeline's exclusion rule: skip .moss, node_modules,
            // .git, etc. Critically skips `.moss/build/staging` shadow copies.
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if crate::build::scan::classify::is_excluded_dir_name(name) {
                continue;
            }
            walk_collect(&path, suffix, project_root, max_depth - 1, out);
        }
    }
}

fn absolute_to_request_url(absolute: &Path, project_root: &Path) -> Option<String> {
    let rel = absolute.strip_prefix(project_root).ok()?;
    let s = rel.to_string_lossy().to_string();
    Some(format!("/{}", s.replace('\\', "/")))
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

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::ast::GraphAssetIndex;
    use moss_core::content_graph::ContentGraph;
    use moss_core::resolve::asset_class::{resolve_asset_ref, AssetResolution};

    fn scratch_root() -> tempfile::TempDir {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create test-tmp base");
        tempfile::TempDir::new_in(&base).expect("tmp dir")
    }

    #[test]
    fn walk_skips_dot_moss_build_staging_shadow() {
        // Exact replica of the Yi-website resolve-not-found repro: a bare-filename
        // image reference (`![](forest.jpg)`) with the real source under `assets/`
        // and TWO build-output shadows under `.moss/build/{staging,current}/assets/`.
        // Both shadows live below `.moss`, which the walker now skips, so the real
        // `assets/forest.jpg` is the only candidate returned.
        let dir = scratch_root();
        let root = dir.path();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/forest.jpg"), b"real").unwrap();
        std::fs::create_dir_all(root.join(".moss/build/staging/assets")).unwrap();
        std::fs::write(root.join(".moss/build/staging/assets/forest.jpg"), b"shadow").unwrap();
        std::fs::create_dir_all(root.join(".moss/build/current/assets")).unwrap();
        std::fs::write(root.join(".moss/build/current/assets/forest.jpg"), b"shadow").unwrap();

        let from_file = root.join("main.md");
        let result = resolve_asset("forest.jpg", &from_file, root).expect("resolves");
        assert!(result.request_url.ends_with("/assets/forest.jpg"),
            "expected /assets/forest.jpg, got {}", result.request_url);
        assert!(!result.request_url.contains(".moss"),
            "must not resolve into .moss: {}", result.request_url);
    }

    #[test]
    fn resolve_relative_path_to_asset() {
        let tmp = tempfile::tempdir().unwrap();
        let img = tmp.path().join("img").join("hero.jpg");
        std::fs::create_dir_all(img.parent().unwrap()).unwrap();
        std::fs::write(&img, b"fake").unwrap();
        let from_file = tmp.path().join("posts").join("post.md");
        std::fs::create_dir_all(from_file.parent().unwrap()).unwrap();

        let resolved = resolve_asset("../img/hero.jpg", &from_file, tmp.path())
            .expect("should resolve");
        assert_eq!(resolved.request_url, "/img/hero.jpg");
        assert_eq!(resolved.mime_type, "image/jpeg");
        assert_eq!(resolved.size_bytes, 4);
    }

    #[test]
    fn resolve_wikilink_style_searches_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        // Asset lives in assets/faculty/, not next to the source file
        let asset_dir = tmp.path().join("assets").join("faculty");
        std::fs::create_dir_all(&asset_dir).unwrap();
        let img = asset_dir.join("fred-adams.jpg");
        std::fs::write(&img, b"fake-jpg").unwrap();

        let from_file = tmp.path().join("Faculty.md");
        std::fs::write(&from_file, "").unwrap();

        // Wikilink-style reference: just the basename, no path
        let resolved = resolve_asset("fred-adams.jpg", &from_file, tmp.path());
        assert!(resolved.is_some(), "should resolve wikilink-style basename");
        let a = resolved.unwrap();
        assert_eq!(a.mime_type, "image/jpeg");
        assert_eq!(a.request_url, "/assets/faculty/fred-adams.jpg");
    }

    #[test]
    fn resolves_asset_via_filesystem() {
        let dir = scratch_root();
        let root = dir.path();
        let assets_dir = root.join("assets");
        std::fs::create_dir_all(&assets_dir).unwrap();
        let img = assets_dir.join("hero.jpg");
        std::fs::write(&img, b"fake jpeg").unwrap();

        let from_file = root.join("index.md");
        let result = resolve_asset("assets/hero.jpg", &from_file, root);
        assert!(result.is_some(), "should resolve via filesystem");
        let asset = result.unwrap();
        assert!(asset.request_url.ends_with("/assets/hero.jpg"));
    }

    #[test]
    fn returns_none_for_nonexistent_file() {
        let dir = scratch_root();
        let from_file = dir.path().join("index.md");
        let result = resolve_asset("assets/missing.jpg", &from_file, dir.path());
        assert!(result.is_none());
    }

    /// Task 8 — engine integration: subfolder `./`-prefix reference that
    /// needs SeparatorFallback (file is at root `assets/`, authored path is
    /// `./assets/AGU2025.jpg` from a `News/` subdirectory).
    #[test]
    fn editor_resolves_subfolder_dotslash_via_engine() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create test-tmp base");
        let dir = tempfile::TempDir::new_in(&base).unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/AGU2025.jpg"), b"x").unwrap();
        std::fs::create_dir_all(root.join("News")).unwrap();
        let r = resolve_asset("./assets/AGU2025.jpg", &root.join("News/post.md"), root).unwrap();
        assert!(r.request_url.ends_with("/assets/AGU2025.jpg"),
            "expected /assets/AGU2025.jpg, got {}", r.request_url);
        assert_eq!(r.provenance, AssetProvenance::SeparatorFallback,
            "expected SeparatorFallback provenance, got {:?}", r.provenance);
    }

    /// Task 8 — engine integration + APFS exact-case: the authored path
    /// `./assets/Hoon.jpg` (lowercase ext) must resolve to the real file
    /// `assets/Hoon.JPG` (uppercase ext) with CaseMismatch provenance.
    ///
    /// On macOS APFS this proves that `FsAssetIndex::contains` is exact-case
    /// (uses read_dir, not Path::exists which is case-blind) so the engine
    /// falls through to `contains_ci` and returns the canonical real-case path.
    #[test]
    fn editor_resolves_case_mismatch_hoon_jpg() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create test-tmp base");
        let dir = tempfile::TempDir::new_in(&base).unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        // Real file has uppercase extension: Hoon.JPG
        std::fs::write(root.join("assets/Hoon.JPG"), b"x").unwrap();
        std::fs::create_dir_all(root.join("team")).unwrap();
        // Authored with lowercase extension: Hoon.jpg
        let r = resolve_asset("./assets/Hoon.jpg", &root.join("team/Team.md"), root).unwrap();
        // request_url must use the REAL on-disk case (Hoon.JPG)
        assert!(r.request_url.ends_with("/assets/Hoon.JPG"),
            "expected /assets/Hoon.JPG (real case), got {}", r.request_url);
        assert_eq!(r.provenance, AssetProvenance::CaseMismatch,
            "expected CaseMismatch provenance, got {:?}", r.provenance);
    }

    #[test]
    fn editor_bare_case_mismatch_resolves_not_red() {
        // Parity S1: a BARE `hoon.jpg` whose real file is assets/Hoon.JPG must
        // resolve in the editor (case-insensitive fuzzy walk), matching the
        // build — otherwise the editor shows a spurious red lint on an asset the
        // build ships fine (editor must never be redder than the build).
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create test-tmp base");
        let dir = tempfile::TempDir::new_in(&base).unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/Hoon.JPG"), b"x").unwrap();
        std::fs::create_dir_all(root.join("team")).unwrap();
        // bare lowercase ref, not adjacent; only assets/Hoon.JPG exists on disk
        let r = resolve_asset("hoon.jpg", &root.join("team/Team.md"), root)
            .expect("bare case-mismatched ref must resolve (not NotFound)");
        assert!(r.request_url.ends_with("/assets/Hoon.JPG"),
            "expected /assets/Hoon.JPG (real case), got {}", r.request_url);
    }

    // -----------------------------------------------------------------------
    // Task 13 — editor↔build no-false-green parity test (R1)
    //
    // Asserts that `resolve_asset_ref` returns IDENTICAL `AssetResolution` when
    // backed by `GraphAssetIndex` (build) vs `FsAssetIndex` (editor) over the
    // SAME file set. This is the "no-false-green" guarantee: the editor must
    // never report a reference as Resolved when the build would not, and vice
    // versa. Any divergence on the corpus below is a real parity bug.
    //
    // Invariant asserted: FULL EQUALITY — same variant, same canonical path,
    // same candidates list. This is stronger than one-directional (editor ⊆
    // build) and appropriate for a flat corpus that both adapters can fully
    // index. The `deep/dir/photo.jpg` file (depth 2) is within the editor
    // walker's depth-4 limit and within the root walk starting from the project
    // root, so both backends see it.
    // -----------------------------------------------------------------------
    #[test]
    fn no_false_green_parity_graph_vs_fs() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create test-tmp base");
        let dir = tempfile::TempDir::new_in(&base).expect("tmp dir");
        let root = dir.path();

        // ---- Create the shared file corpus on disk -------------------------
        // Every path here is also registered in the ContentGraph so both
        // adapters see exactly the same set.
        let corpus_paths: &[&str] = &[
            "assets/AGU2025.jpg",
            "assets/Hoon.JPG",   // uppercase extension — tests case-insensitive suffix match
            "News/post.md",
            "team/Team.md",
            "a/photo.jpg",
            "deep/dir/photo.jpg",
        ];
        for p in corpus_paths {
            let full = root.join(p.replace('/', std::path::MAIN_SEPARATOR_STR));
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, b"x").unwrap();
        }

        // ---- Build ContentGraph (build adapter) ----------------------------
        let graph = ContentGraph::from_paths(corpus_paths);
        let graph_index = GraphAssetIndex(&graph);

        // ---- Build FsAssetIndex (editor adapter) --------------------------
        // Canonicalize so strip_prefix works correctly on macOS (/tmp symlink).
        let canonical_root = std::fs::canonicalize(root)
            .unwrap_or_else(|_| root.to_path_buf());
        let fs_index = FsAssetIndex::new(&canonical_root);

        // ---- Corpus of (target, from_source) references -------------------
        // from_source is root-relative (as the engine expects).
        let cases: &[(&str, &str, &str)] = &[
            // (description, target, from_source)
            ("relative dotslash to assets from News/",
             "./assets/AGU2025.jpg", "News/post.md"),
            ("relative dotslash with case mismatch (Hoon.jpg vs Hoon.JPG)",
             "./assets/Hoon.jpg", "team/Team.md"),
            ("bare filename case mismatch (hoon.jpg → assets/Hoon.JPG)",
             "hoon.jpg", "team/Team.md"),
            ("bare filename exact (AGU2025.jpg)",
             "AGU2025.jpg", "News/post.md"),
            ("root-absolute path",
             "/assets/AGU2025.jpg", "News/post.md"),
            ("ambiguous bare filename (photo.jpg has two candidates)",
             "photo.jpg", "News/post.md"),
            ("containment escape — must be NotFound",
             "../../etc/x.jpg", "News/post.md"),
        ];

        for (desc, target, from_source) in cases {
            let graph_result = resolve_asset_ref(target, from_source, &graph_index);
            let fs_result    = resolve_asset_ref(target, from_source, &fs_index);

            // Normalize candidate lists: the engine already sorts by depth then
            // lexical order in both paths, but sort again defensively so any
            // ordering difference in the backing adapter doesn't produce a false
            // divergence report.
            let graph_norm = normalize_resolution(graph_result);
            let fs_norm    = normalize_resolution(fs_result);

            assert_eq!(
                graph_norm, fs_norm,
                "parity divergence on [{desc}] target={target:?} from={from_source:?}\n  graph: {graph_norm:?}\n  fs:    {fs_norm:?}"
            );
        }
    }

    /// Normalize an `AssetResolution` for comparison: sort the `candidates`
    /// list inside `Ambiguous` so ordering differences between adapters don't
    /// produce spurious divergences.
    fn normalize_resolution(r: AssetResolution) -> AssetResolution {
        match r {
            AssetResolution::Ambiguous { chosen, mut candidates } => {
                candidates.sort();
                AssetResolution::Ambiguous { chosen, candidates }
            }
            other => other,
        }
    }

    #[test]
    fn find_by_suffix_reaches_deep_non_hardcoded_folder() {
        // A bare-basename reference whose source lives 5 levels deep in a folder
        // NOT in the old hardcoded allow-list (`docs/...`, not `assets/...`).
        // The pre-fix depth-4 + hardcoded-dir walk missed this; the build graph
        // always found it. Parity requires the editor to find it too.
        let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = root.path();
        std::fs::create_dir_all(p.join("docs/a/b/c/d")).unwrap();
        std::fs::write(p.join("docs/a/b/c/d/photo.jpg"), b"x").unwrap();
        // And an excluded dir that must STILL be skipped (parity with build scan).
        std::fs::create_dir_all(p.join(".moss/build/current")).unwrap();
        std::fs::write(p.join(".moss/build/current/photo.jpg"), b"shadow").unwrap();

        let idx = FsAssetIndex::new(p);
        let hits = idx.find_by_suffix("photo.jpg");
        assert_eq!(hits, vec!["docs/a/b/c/d/photo.jpg".to_string()],
            "must find the deep source file and skip the .moss shadow");
    }

    #[test]
    fn find_by_suffix_does_not_follow_dir_symlinks() {
        // A directory symlink that points back at the root forms a cycle. The
        // build's WalkDir does NOT follow symlinks (its default), so the editor
        // walker must not either — else a cycle = unbounded recursion / stack
        // overflow (max_depth = usize::MAX) AND a parity divergence (the editor
        // would surface symlinked duplicates the build never sees).
        let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = root.path();
        std::fs::create_dir_all(p.join("real")).unwrap();
        std::fs::write(p.join("real/photo.jpg"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(p, p.join("loop")).unwrap(); // cycle if followed
        let idx = FsAssetIndex::new(p);
        let hits = idx.find_by_suffix("photo.jpg");
        assert_eq!(hits, vec!["real/photo.jpg".to_string()]); // found once, no hang, no dup
    }

    #[test]
    fn mime_for_path_is_case_insensitive_and_falls_back() {
        // Direct coverage for mime_for_path (it is now the single MIME authority
        // shared by enrich() and the moss-source:// scheme handler). Restores the
        // assertions that lived in source_asset_protocol's deleted ct_tests.
        // Case-insensitive extension match:
        assert_eq!(mime_for_path(Path::new("a/b/x.JPEG")), "image/jpeg");
        assert_eq!(mime_for_path(Path::new("x.PNG")), "image/png");
        assert_eq!(mime_for_path(Path::new("x.webp")), "image/webp");
        // Non-image media the scheme handler also serves must stay honest
        // (not collapse to octet-stream, which breaks <video>/<audio>/pdf):
        assert_eq!(mime_for_path(Path::new("clip.MP4")), "video/mp4");
        assert_eq!(mime_for_path(Path::new("song.mp3")), "audio/mpeg");
        // Unknown / missing extension falls back to octet-stream:
        assert_eq!(mime_for_path(Path::new("x.bin")), "application/octet-stream");
        assert_eq!(mime_for_path(Path::new("noext")), "application/octet-stream");
    }

    #[test]
    fn mime_for_path_matches_asset_registry_for_media_kinds() {
        // Property test (M2/M3): for every entry in the asset registry whose kind
        // is a media kind (Image, Video, Audio, Pdf, Model), mime_for_path must
        // return the same MIME type as AssetInfo.mime.
        //
        // Documented exceptions — extensions in the registry that mime_for_path
        // intentionally does not serve (no browser-protocol handler needed):
        //   md, markdown — text/markdown: editor serves raw source, not via scheme handler
        //   html, htm    — text/html: the browser renders these natively; no scheme handler
        //   csv, tsv     — text/csv / text/tsv: not served as standalone assets
        //   ipynb        — application/x-ipynb+json: served by the notebook renderer path
        //   glb, gltf    — model/*: not currently served via moss-source:// scheme
        //   wma          — audio/x-ms-wma: import-only, not served by scheme handler
        //   bmp, ico, tiff — viewer-only images, not browser-embeddable
        //   avi, mkv     — import-only video, not browser-embeddable
        use moss_core::resolve::asset_registry::all_assets;
        use moss_core::resolve::ext_kind::ExtKind;

        let scheme_handler_exceptions = [
            "md", "markdown",       // text source, not served as media
            "html", "htm",          // native browser render
            "csv", "tsv",           // table renderer path
            "ipynb",                // notebook renderer path
            "glb", "gltf",          // model viewer (not yet in scheme handler)
            "wma",                  // import-only, not web-playable
            "bmp", "ico", "tiff",   // viewer-only images, not browser-embeddable
            "avi", "mkv",           // import-only video, not browser-embeddable
        ];

        for a in all_assets() {
            if a.kind == ExtKind::Other || scheme_handler_exceptions.contains(&a.ext) {
                continue;
            }
            let got = mime_for_path(Path::new(&format!("x.{}", a.ext)));
            assert_eq!(
                got, a.mime,
                "mime_for_path diverges from registry for .{}: got '{}', registry says '{}'",
                a.ext, got, a.mime
            );
        }

        // Spot-check the three that were wrong before the M2/M3 fix:
        assert_eq!(mime_for_path(Path::new("x.ogg")), "audio/ogg",  "ogg must be audio/ogg");
        assert_eq!(mime_for_path(Path::new("x.aac")), "audio/aac",  "aac must be audio/aac");
        assert_eq!(mime_for_path(Path::new("x.m4a")), "audio/mp4",  "m4a must be audio/mp4");
    }

    #[test]
    fn editor_relative_fromfile_subfolder_warns() {
        // The real editor passes a PROJECT-ROOT-RELATIVE fromFile ("News/post.md"),
        // NOT an absolute path. compute_from_source must keep the "News/" subfolder
        // (a relative path IS already the root-relative source). Regression: the
        // canonicalize-parent path resolved "News" against the process CWD, failed,
        // and fell back to the basename "post.md" — making the engine treat the file
        // as root-level so `./assets/X` resolved as Literal and the warning vanished.
        //
        // The unified lint reads provenance off the resolved asset; this asserts it
        // on the surviving singular `resolve_asset` path (same enrich + engine).
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create test-tmp base");
        let dir = tempfile::TempDir::new_in(&base).unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/AGU2025.jpg"), b"x").unwrap();
        std::fs::create_dir_all(root.join("News")).unwrap();
        // RELATIVE from_file, exactly as the editor passes it via getCurrentEntry().path
        let r = resolve_asset(
            "./assets/AGU2025.jpg",
            std::path::Path::new("News/post.md"),
            root,
        )
        .expect("relative fromFile must still resolve via separator fallback");
        assert_eq!(
            r.provenance,
            AssetProvenance::SeparatorFallback,
            "relative fromFile must preserve the News/ subfolder; got {:?}",
            r.provenance
        );
    }

    #[test]
fn image_extensions_align_with_editor_mime_map() {
    // IMAGE_EXTENSIONS (the SourceAssetChanged notify list) is intentionally
    // kept in lock-step with the editor's image MIME map
    // (asset_resolver::mime_for_path). Without this guard the two drift the
    // next time someone adds an image format to only one of them.
    use std::path::Path;
    // Forward: every notified extension must be an image per the MIME map —
    // else we'd notify the editor about a file it can't render inline.
    for ext in crate::build::watch::IMAGE_EXTENSIONS {
        let mime = mime_for_path(Path::new(&format!("x.{ext}")));
        assert!(
            mime.starts_with("image/"),
            "IMAGE_EXTENSIONS has `{ext}` but mime_for_path returns `{mime}` (not image/*)"
        );
    }
    // Boundary: non-image media the moss-source:// handler also serves must
    // NOT be in the notify list, so video/audio/doc edits don't masquerade
    // as image refreshes.
    for non_img in ["mp4", "webm", "mov", "mp3", "wav", "flac", "pdf"] {
        assert!(
            !crate::build::watch::IMAGE_EXTENSIONS.contains(&non_img),
            "`{non_img}` is non-image per mime_for_path but is in IMAGE_EXTENSIONS"
        );
    }
}
}
