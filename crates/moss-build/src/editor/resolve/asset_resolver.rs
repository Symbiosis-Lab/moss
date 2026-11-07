//! Filesystem snapshot and asset metadata adapter for pure reference resolution.
use crate::build::scan::classify::{left_out, left_out_of_site};
use moss_core::content_graph::{normalize_path, ContentGraph};
use moss_core::resolve::editor::{RefTarget, ResolveInput};
use moss_core::resolve::editor::files::named_folders;
pub use moss_core::resolve::editor::{EditorScope, ResolvedAsset};
pub use moss_core::resolve::editor::files::mime_for_path;
use std::path::{Path, PathBuf};
#[cfg(test)]
use moss_core::resolve::asset_class::{AssetProvenance, resolve_file_target};

/// One site walk, with exactly the build scan's exclusions and symlink policy.
fn site_listing(root: &Path) -> (Vec<String>, Vec<String>) {
    #[cfg(test)]
    tests::SITE_WALKS.with(|n| n.set(n.get() + 1));
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let walk = walkdir::WalkDir::new(root).into_iter().filter_entry(|e| left_out_of_site(e).is_none());
    for entry in walk.flatten() {
        let Ok(rel) = entry.path().strip_prefix(root) else { continue; };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_file() { files.push(rel); }
        else if entry.file_type().is_dir() && !rel.is_empty() { directories.push(rel); }
    }
    (files, directories)
}

pub fn project_graph(root: &Path) -> ContentGraph {
    moss_core::resolve::editor::files::graph(&site_listing(root).0)
}

/// Snapshot every named folder once. A loose folder is never recursively walked.
pub(super) fn reference_input(root: &Path, scope: EditorScope, from: &str, targets: &[RefTarget]) -> ResolveInput {
    let (files, directories) = if scope == EditorScope::Site {
        site_listing(root)
    } else {
        let mut folders = std::collections::BTreeSet::from([String::new()]);
        for target in targets {
            folders.extend(named_folders(&target.text.replace('\\', "/"), from).iter()
                .filter_map(|folder| moss_core::content_graph::join_written("", folder)));
        }
        let files = folders.iter().filter_map(|folder| read_folder_files(root, folder)).flatten()
            .collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        (files, Vec::new())
    };
    ResolveInput {
        files, directories, scope, from_source: from.to_string(), targets: targets.to_vec(), url_index: None,
        root_name: crate::vault_root::VaultRoot::resolve(root).name().to_string(),
        passthrough: if scope == EditorScope::Site {
            crate::build::site_config::get_build_passthrough(&root.to_string_lossy()).unwrap_or_default()
        } else { Vec::new() },
    }
}

pub(super) fn compute_from_source(from_file: &Path, canonical_root: &Path) -> String {
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

/// Add only host facts; classification, request URL and MIME stay core-owned.
pub(super) fn enrich(mut asset: ResolvedAsset, root: &Path) -> Option<ResolvedAsset> {
    let absolute = root.join(&asset.absolute_path);
    let meta = std::fs::metadata(&absolute).ok()?;
    if asset.mime_type.starts_with("image/") {
        if let Some((width, height)) = crate::build::scan::scan::extract_image_dimensions(&absolute) {
            asset.width = Some(width);
            asset.height = Some(height);
        }
    }
    asset.absolute_path = absolute.to_string_lossy().to_string();
    asset.size_bytes = meta.len();
    Some(asset)
}

pub fn resolve_asset(target: &str, from_file: &Path, project_root: &Path, scope: EditorScope) -> Option<ResolvedAsset> {
    let root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let from = compute_from_source(from_file, &root);
    let input = reference_input(&root, scope, &from, &[RefTarget { text: target.to_string(), is_embed: true }]);
    enrich(moss_core::resolve::editor::resolve_asset(&input, target)?, &root)
}

fn read_folder_files(root: &Path, folder: &str) -> Option<Vec<String>> {
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
        let result = resolve_asset("forest.jpg", &from_file, root, EditorScope::Site).expect("resolves");
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

        let resolved = resolve_asset("../img/hero.jpg", &from_file, tmp.path(), EditorScope::Site)
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
        let resolved = resolve_asset("fred-adams.jpg", &from_file, tmp.path(), EditorScope::Site);
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
        let result = resolve_asset("assets/hero.jpg", &from_file, root, EditorScope::Site);
        assert!(result.is_some(), "should resolve via filesystem");
        let asset = result.unwrap();
        assert!(asset.request_url.ends_with("/assets/hero.jpg"));
    }

    #[test]
    fn returns_none_for_nonexistent_file() {
        let dir = scratch_root();
        let from_file = dir.path().join("index.md");
        let result = resolve_asset("assets/missing.jpg", &from_file, dir.path(), EditorScope::Site);
        assert!(result.is_none());
    }

    /// Through the shared resolver: a subfolder `./`-prefix reference that
    /// needs SeparatorFallback (file is at root `assets/`, authored path is
    /// `./assets/Summit2099.jpg` from a `News/` subdirectory).
    #[test]
    fn editor_resolves_subfolder_dotslash_via_engine() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create test-tmp base");
        let dir = tempfile::TempDir::new_in(&base).unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/Summit2099.jpg"), b"x").unwrap();
        std::fs::create_dir_all(root.join("News")).unwrap();
        let r = resolve_asset("./assets/Summit2099.jpg", &root.join("News/post.md"), root, EditorScope::Site).unwrap();
        assert!(r.request_url.ends_with("/assets/Summit2099.jpg"),
            "expected /assets/Summit2099.jpg, got {}", r.request_url);
        assert_eq!(r.provenance, AssetProvenance::SeparatorFallback,
            "expected SeparatorFallback provenance, got {:?}", r.provenance);
    }

    /// Through the shared resolver, with the file's real case: the authored path
    /// `./assets/Fern.jpg` (lowercase ext) must resolve to the real file
    /// `assets/Fern.JPG` (uppercase ext) with CaseMismatch provenance.
    ///
    /// On macOS APFS this proves the picked path carries the file's real
    /// on-disk case, not the authored one (`Path::exists` is case-blind there).
    #[test]
    fn editor_resolves_case_mismatch_fern_jpg() {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create test-tmp base");
        let dir = tempfile::TempDir::new_in(&base).unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        // Real file has uppercase extension: Fern.JPG
        std::fs::write(root.join("assets/Fern.JPG"), b"x").unwrap();
        std::fs::create_dir_all(root.join("team")).unwrap();
        // Authored with lowercase extension: Fern.jpg
        let r = resolve_asset("./assets/Fern.jpg", &root.join("team/Team.md"), root, EditorScope::Site).unwrap();
        // request_url must use the REAL on-disk case (Fern.JPG)
        assert!(r.request_url.ends_with("/assets/Fern.JPG"),
            "expected /assets/Fern.JPG (real case), got {}", r.request_url);
        assert_eq!(r.provenance, AssetProvenance::CaseMismatch,
            "expected CaseMismatch provenance, got {:?}", r.provenance);
    }

    #[test]
    fn editor_bare_case_mismatch_resolves_not_red() {
        // A BARE `fern.jpg` whose real file is assets/Fern.JPG must
        // resolve in the editor (case-insensitive fuzzy walk), matching the
        // build — otherwise the editor shows a spurious red lint on an asset the
        // build ships fine (editor must never be redder than the build).
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
        std::fs::create_dir_all(&base).expect("create test-tmp base");
        let dir = tempfile::TempDir::new_in(&base).unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/Fern.JPG"), b"x").unwrap();
        std::fs::create_dir_all(root.join("team")).unwrap();
        // bare lowercase ref, not adjacent; only assets/Fern.JPG exists on disk
        let r = resolve_asset("fern.jpg", &root.join("team/Team.md"), root, EditorScope::Site)
            .expect("bare case-mismatched ref must resolve (not NotFound)");
        assert!(r.request_url.ends_with("/assets/Fern.JPG"),
            "expected /assets/Fern.JPG (real case), got {}", r.request_url);
    }

    // The editor's file set (read from disk) and the build's (its own scan)
    // must be the same files and give the same answer for every target, so
    // the editor is never greener or redder than the build.
    #[test]
    fn editor_and_build_file_sets_resolve_alike() {
        let dir = scratch_root();
        let root = std::fs::canonicalize(dir.path()).unwrap();

        let corpus_paths: &[&str] = &[
            "assets/Summit2099.jpg",
            "assets/Fern.JPG",
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
            ("./assets/Summit2099.jpg", "News/post.md"),
            ("./assets/Fern.jpg", "team/Team.md"),
            ("fern.jpg", "team/Team.md"),
            ("Summit2099.jpg", "News/post.md"),
            ("/assets/Summit2099.jpg", "News/post.md"),
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
            let fast = moss_core::resolve::editor::files::spelled_out_file(target, from, site.all_files());
            assert_eq!(fast, Some(resolve_file_target(target, from, &site)), "{target:?} from {from:?}");
        }
        // Anything the first two steps do not settle goes to the whole site.
        let rest = [("x.png", "Team/p.md"), ("shop/y.png", "post.md"), ("assets/x", "News/post.md"), ("q.png", "News/post.md")];
        for (target, from) in rest {
            assert_eq!(moss_core::resolve::editor::files::spelled_out_file(target, from, site.all_files()), None, "{target:?} from {from:?}");
        }
    }

    thread_local! {
        /// How many times [`project_graph`] walked a site on this thread.
        pub(super) static SITE_WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[test]
    fn each_asset_call_walks_the_site_once() {
        let dir = scratch_root();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/x.png"), b"x").unwrap();
        let walks = || SITE_WALKS.with(|n| n.get());
        let before = walks();
        assert!(resolve_asset("assets/x.png", &root.join("News/post.md"), &root, EditorScope::Site).is_some());
        assert_eq!(walks(), before + 1, "one snapshot for a spelled-out path");
        assert!(resolve_asset("x.png", &root.join("News/post.md"), &root, EditorScope::Site).is_some());
        assert_eq!(walks(), before + 2, "one snapshot for a bare name");
    }

    #[test]
    fn a_loose_folder_is_read_by_name_and_never_walked() {
        let dir = scratch_root();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("img/deep")).unwrap();
        std::fs::write(root.join("beside.png"), b"x").unwrap();
        std::fs::write(root.join("img/shot.png"), b"x").unwrap();
        std::fs::write(root.join("img/deep/far.png"), b"x").unwrap();
        let walks = || SITE_WALKS.with(|n| n.get());
        let before = walks();
        let found = |target: &str| resolve_asset(target, Path::new("note.md"), &root, EditorScope::Folder).is_some();
        assert!(found("beside.png") && found("img/shot.png") && found("./img/deep/far.png"));
        assert!(!found("far.png"), "a bare name is looked for beside the document only");
        assert_eq!(walks(), before, "a loose folder can be a home directory: it is never walked");
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
        assert_eq!(moss_core::resolve::editor::files::spelled_out_file("a/photo.jpg", "p.md", project_graph(&root).all_files()), None);
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
        std::fs::write(root.join("assets/Summit2099.jpg"), b"x").unwrap();
        std::fs::create_dir_all(root.join("News")).unwrap();
        // RELATIVE from_file, exactly as the editor passes it via getCurrentEntry().path
        let r = resolve_asset(
            "./assets/Summit2099.jpg",
            std::path::Path::new("News/post.md"),
            root,
            EditorScope::Site,
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
