//! Editor-facing asset resolution.
//!
//! Resolves asset references for the editor using direct filesystem access —
//! no dependency on the build's AssetRegistry or WebP/LQIP pipeline.
//! The editor renders source images directly via `moss-source://`.

use crate::build::scan::classify::{left_out, left_out_of_site};
use moss_core::content_graph::{normalize_path, ContentGraph, ContentGraphBuilder, PathMatch};
use moss_core::resolve::asset_class::{AssetProvenance, AssetResolution, resolve_file_target};
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
    /// case-mismatch. Carried from the shared engine.
    ///
    /// Read directly by the editor's asset-advisory lint (cm-asset-lint.ts),
    /// which pulls `env.resolved.provenance` off the unified reference envelope
    /// (`EditorReferenceResolution`) to warn on separator-fallback / case-mismatch.
    /// Also exposed by the singular `editor_resolve_asset` (chip-bar) command.
    pub provenance: AssetProvenance,
}

/// Every file of the site as a [`ContentGraph`], read from disk now.
///
/// This is the editor's file set for [`resolve_file_target`]: the same
/// resolver the build runs, over the same files. The walk reads exactly what
/// the build's scan reads ([`left_out_of_site`]: no `.moss/build.nosync`
/// shadow copies, no nested site, no root agent instructions) and, like it,
/// does not follow symlinks, so a symlink cycle cannot recurse or produce
/// duplicate paths. Built once per call or batch.
pub fn project_graph(project_root: &Path) -> ContentGraph {
    #[cfg(test)]
    tests::SITE_WALKS.with(|n| n.set(n.get() + 1));
    let mut builder = ContentGraphBuilder::new();
    let walk = walkdir::WalkDir::new(project_root).into_iter().filter_entry(|e| left_out_of_site(e).is_none());
    for entry in walk.flatten().filter(|e| e.file_type().is_file()) {
        if let Ok(rel) = entry.path().strip_prefix(project_root) {
            builder.add_file(&rel.to_string_lossy().replace('\\', "/"), "");
        }
    }
    builder.build()
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
/// Path resolution is delegated to the one resolver (`resolve_file_target` in
/// moss-core, over [`project_graph`]), so `CaseMismatch` provenance is
/// produced when the authored path differs only in case from the real file,
/// and the file picked is the one the build links.
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
    let target_norm = target.replace('\\', "/");
    let picked = spelled_out_file(&target_norm, &from_source, &canonical_root)
        .unwrap_or_else(|| resolve_file_target(&target_norm, &from_source, &project_graph(&canonical_root)));

    let (root_rel, provenance) = match picked {
        AssetResolution::Resolved { root_rel, provenance } => (root_rel, provenance),
        AssetResolution::Ambiguous { chosen, .. } => {
            // Equally near candidates — use the one the build links. Treat as
            // SeparatorFallback provenance (non-literal resolution).
            // The unified reference resolver models Ambiguous properly in its envelope
            // (kind:ambiguous + candidates); this singular path just picks the winner.
            (chosen, AssetProvenance::SeparatorFallback)
        }
        AssetResolution::NotFound => return None,
    };

    enrich(&root_rel, provenance, &canonical_root)
}

/// What [`resolve_file_target`] answers for a `target` that spells out a path
/// to an existing file, from the page's folder or from the site root, read
/// from those two folders instead of a walk of the whole site (a completion
/// preview asks this once per row). It is exact: the resolver's first two
/// steps look at nothing but those two paths, so when one of them matches, no
/// other file can change the answer. `None` when the target is
/// percent-encoded (a decoded retry could follow a search of the whole site),
/// when a name is ambiguous on disk (two folders, or two files, that differ
/// only in letter case), or when neither step matched; the caller then asks
/// the whole site.
fn spelled_out_file(target: &str, from_source: &str, root: &Path) -> Option<AssetResolution> {
    if target.contains('%') {
        return None;
    }
    let folder_of = |path: &str| path.rsplit_once('/').map_or(String::new(), |(dir, _)| dir.to_string());
    let rel = target.trim_start_matches('/');
    let mut folders = vec![folder_of(rel)];
    if !target.starts_with('/') {
        folders.push(format!("{}/{}", folder_of(from_source), folder_of(rel)));
    }
    let mut files = std::collections::BTreeSet::new();
    for folder in &folders {
        files.extend(site_files_in(root, folder)?);
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

/// The site files directly in `folder` (written relative to the site root,
/// `.` and `..` allowed, any letter case), as root-relative paths in their
/// real case: the files [`project_graph`] would hold there. Empty when no such
/// folder belongs to the site; `None` when the folder cannot be read or its
/// name matches more than one folder on disk.
fn site_files_in(root: &Path, folder: &str) -> Option<Vec<String>> {
    let Some(joined) = moss_core::content_graph::join_written("", folder) else { return Some(Vec::new()) };
    let written: Vec<&str> = joined.split('/').filter(|s| !s.is_empty()).collect();
    let mut dir = root.to_path_buf();
    let mut real: Vec<String> = Vec::new();
    for (depth, seg) in written.iter().enumerate() {
        let key = normalize_path(seg);
        let mut same: Vec<std::fs::DirEntry> = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| normalize_path(&e.file_name().to_string_lossy()) == key)
            .collect();
        let Some(entry) = same.pop() else { return Some(Vec::new()) };
        if !same.is_empty() {
            return None;
        }
        if left_out(&entry.path(), true, depth + 1).is_some() {
            return Some(Vec::new());
        }
        real.push(entry.file_name().to_string_lossy().into_owned());
        dir = entry.path();
    }
    let prefix: String = real.iter().map(|s| format!("{s}/")).collect();
    let files = std::fs::read_dir(&dir).ok()?.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file()));
    Some(
        files
            .filter(|e| left_out(&e.path(), false, real.len() + 1).is_none())
            .map(|e| format!("{prefix}{}", e.file_name().to_string_lossy()))
            .collect(),
    )
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

    fn scratch_root() -> tempfile::TempDir {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create test-tmp base");
        tempfile::TempDir::new_in(&base).expect("tmp dir")
    }

    #[test]
    fn walk_skips_dot_moss_build_staging_shadow() {
        // Exact replica of a real site's resolve-not-found repro: a bare-filename
        // image reference (`![](forest.jpg)`) with the real source under `assets/`
        // and TWO build-output shadows under `.moss/build.nosync/{staging,current}/assets/`.
        // Both shadows live below `.moss`, which the walker now skips, so the real
        // `assets/forest.jpg` is the only candidate returned.
        let dir = scratch_root();
        let root = dir.path();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/forest.jpg"), b"real").unwrap();
        std::fs::create_dir_all(root.join(".moss/build.nosync/staging/assets")).unwrap();
        std::fs::write(root.join(".moss/build.nosync/staging/assets/forest.jpg"), b"shadow").unwrap();
        std::fs::create_dir_all(root.join(".moss/build.nosync/current/assets")).unwrap();
        std::fs::write(root.join(".moss/build.nosync/current/assets/forest.jpg"), b"shadow").unwrap();

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
    /// On macOS APFS this proves the picked path carries the file's real
    /// on-disk case, not the authored one (`Path::exists` is case-blind there).
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

    // The editor's file set (read from disk) and the build's (its own scan)
    // must be the same files and give the same answer for every target, so
    // the editor is never greener or redder than the build.
    #[test]
    fn editor_and_build_file_sets_resolve_alike() {
        let dir = scratch_root();
        let root = std::fs::canonicalize(dir.path()).unwrap();

        let corpus_paths: &[&str] = &[
            "assets/AGU2025.jpg",
            "assets/Hoon.JPG",
            "News/post.md",
            "team/Team.md",
            "a/photo.jpg",
            "deep/dir/photo.jpg",
            // Not part of this site: a nested site, agent instructions at the
            // root, and folders the build never reads.
            "shop/.moss/config.toml",
            "shop/only-here.jpg",
            "AGENTS.md",
            "node_modules/pkg/logo.png",
            ".obsidian/icon.png",
        ];
        for p in corpus_paths {
            let full = root.join(p.replace('/', std::path::MAIN_SEPARATOR_STR));
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, b"x").unwrap();
        }
        let scanned = crate::build::scan::scan::scan_folder(&root.to_string_lossy()).expect("scan");
        let build_graph = crate::build::scan::scan::build_content_graph(&scanned);
        let editor_graph = project_graph(&root);

        let sorted = |g: &ContentGraph| {
            let mut v = g.all_files().to_vec();
            v.sort();
            v
        };
        assert_eq!(sorted(&editor_graph), sorted(&build_graph));

        let cases: &[(&str, &str)] = &[
            ("./assets/AGU2025.jpg", "News/post.md"),
            ("./assets/Hoon.jpg", "team/Team.md"),
            ("hoon.jpg", "team/Team.md"),
            ("AGU2025.jpg", "News/post.md"),
            ("/assets/AGU2025.jpg", "News/post.md"),
            ("photo.jpg", "News/post.md"),
            ("photo.jpg", "deep/page.md"),
            ("../../etc/x.jpg", "News/post.md"),
            ("only-here.jpg", "News/post.md"),
            ("AGENTS", "News/post.md"),
            ("logo.png", "News/post.md"),
        ];
        for (target, from_source) in cases {
            assert_eq!(
                resolve_file_target(target, from_source, &build_graph),
                resolve_file_target(target, from_source, &editor_graph),
                "build and editor file sets disagree on {target:?} from {from_source:?}"
            );
        }
    }

    // A path spelled out from the page or the root is answered from two
    // folder listings, and that answer is the whole site's answer.
    #[test]
    fn a_spelled_out_path_is_answered_from_its_folders_as_the_whole_site_would() {
        let dir = scratch_root();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        for p in [
            "assets/x.png", "News/assets/x.png", "News/post.md", "Team/Photo.JPG", "a/b/c.png",
            "a/b/c.png.md", "shop/.moss/config.toml", "shop/y.png", "zh-hans/p.md", "uk/z.png", "q.png.md",
        ] {
            let full = root.join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, b"x").unwrap();
        }
        let site = project_graph(&root);
        let answered = [
            ("assets/x.png", "News/post.md"),
            ("/assets/x.png", "News/post.md"),
            ("../assets/x.png", "News/post.md"),
            ("team/photo.jpg", "post.md"),
            ("TEAM/Photo.JPG", "News/post.md"),
            ("a/b/c.png", "x/y.md"),
            ("uk/z.png", "zh-hans/p.md"),
        ];
        for (target, from) in answered {
            let fast = spelled_out_file(target, from, &root);
            assert_eq!(fast, Some(resolve_file_target(target, from, &site)), "{target:?} from {from:?}");
        }
        // Anything the first two steps do not settle goes to the whole site.
        let rest = [("x.png", "Team/p.md"), ("shop/y.png", "post.md"), ("assets/x", "News/post.md"), ("q.png", "News/post.md")];
        for (target, from) in rest {
            assert_eq!(spelled_out_file(target, from, &root), None, "{target:?} from {from:?}");
        }
    }

    thread_local! {
        /// How many times [`project_graph`] walked a site on this thread.
        pub(super) static SITE_WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[test]
    fn a_spelled_out_path_is_resolved_without_walking_the_site() {
        let dir = scratch_root();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/x.png"), b"x").unwrap();
        let walks = || SITE_WALKS.with(|n| n.get());
        let before = walks();
        assert!(resolve_asset("assets/x.png", &root.join("News/post.md"), &root).is_some());
        assert_eq!(walks(), before, "a path from the root must not walk the site");
        assert!(resolve_asset("x.png", &root.join("News/post.md"), &root).is_some());
        assert_eq!(walks(), before + 1, "a bare name is found by searching the whole site");
    }

    #[test]
    fn case_twins_in_one_folder_send_the_fast_route_to_the_whole_site() {
        // Only a case-sensitive disk can hold both names.
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(root.join("a/photo.jpg"), b"lower").unwrap();
        std::fs::write(root.join("a/Photo.jpg"), b"upper").unwrap();
        if std::fs::read(root.join("a/photo.jpg")).unwrap() != b"lower" {
            eprintln!("skipped: this disk is case-insensitive, so it cannot hold case twins");
            return;
        }
        assert_eq!(spelled_out_file("a/photo.jpg", "p.md", &root), None);
    }

    #[test]
    fn project_graph_reaches_deep_non_hardcoded_folder() {
        // A source 5 levels deep in an arbitrary folder is in the editor's file
        // set, and an excluded dir (parity with the build scan) is not.
        let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = root.path();
        std::fs::create_dir_all(p.join("docs/a/b/c/d")).unwrap();
        std::fs::write(p.join("docs/a/b/c/d/photo.jpg"), b"x").unwrap();
        std::fs::create_dir_all(p.join(".moss/build.nosync/current")).unwrap();
        std::fs::write(p.join(".moss/build.nosync/current/photo.jpg"), b"shadow").unwrap();

        let graph = project_graph(p);
        assert_eq!(graph.all_files(), ["docs/a/b/c/d/photo.jpg".to_string()],
            "must find the deep source file and skip the .moss shadow");
    }

    #[test]
    fn project_graph_does_not_follow_dir_symlinks() {
        // A directory symlink back at the root forms a cycle. The build's
        // WalkDir does not follow symlinks, so the editor walk must not either:
        // else unbounded recursion and symlinked duplicates the build never sees.
        let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let p = root.path();
        std::fs::create_dir_all(p.join("real")).unwrap();
        std::fs::write(p.join("real/photo.jpg"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(p, p.join("loop")).unwrap();
        assert_eq!(project_graph(p).all_files(), ["real/photo.jpg".to_string()]);
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
