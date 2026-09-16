//! Unit tests for `EditorFolderIndex`.
//!
//! Two tempdir vaults total, not one per assertion: under the new design every
//! index construction pays for a vault walk, so the fixture-per-test shape the
//! old `FsFolderIndex` tests used is now the expensive one.

use super::*;
use crate::build::scan::article_map::ArticleInfo;
use moss_core::resolve::folder_class::FolderIndex;

fn tmp() -> tempfile::TempDir {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).expect("create test-tmp base");
    tempfile::TempDir::new_in(&base).unwrap()
}

fn write(root: &std::path::Path, rel: &str, body: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn article(source_path: &str, url_path: &str) -> ArticleInfo {
    ArticleInfo {
        source_path: source_path.into(),
        title: String::new(),
        content: String::new(),
        html_content: None,
        frontmatter: HashMap::new(),
        url_path: url_path.into(),
        date: None,
        tags: vec![],
        uid: None,
    }
}

/// One vault, no article map — the "never built" state. Table-driven so the
/// walk runs once for every branch of the predicate.
#[test]
fn folder_predicates_over_one_fixture_vault() {
    let t = tmp();
    let root = t.path();
    write(root, "index.md", b"# home");
    // Branch 2: a pre-built app dropped in — passthrough subtree, iframe.
    write(root, "Resources/app/index.html", b"<html></html>");
    write(root, "Resources/app/sub/page.html", b"<html></html>");
    // Branch 1: an explicit markdown index.
    write(root, "news/index.md", b"# News");
    // Index-less folder with children — the build synthesizes an index here.
    write(root, "writings/post.md", b"# Post");
    // Self-named folder note (the content-folder form).
    write(root, "獎項/獎項.md", b"# Awards");
    // Home stems the old hardcoded ["index.md","README.md","_index.md"] missed.
    write(root, "Docs/README.MD", b"# Docs");
    write(root, "Notes/main.md", b"# Notes");
    // Extension-collision guard: `assets/photo/` is a real directory, and
    // `assets/photo.png` is a real file. `is_dir` must not conflate them.
    write(root, "assets/photo.png", b"\x89PNG");
    write(root, "assets/photo/raw.tif", b"II*");

    let idx = EditorFolderIndex::new(root, &ArticleMap::default());

    // (query, markdown index?, static index, is_dir?)
    let cases: &[(&str, bool, Option<&str>, bool)] = &[
        ("Resources/app", false, Some("index.html"), true),
        // Subtree exclusion: the app owns every page beneath its root.
        ("Resources/app/sub", false, None, true),
        // The passthrough root's PARENT is an ordinary content folder.
        ("Resources", true, None, true),
        ("news", true, None, true),
        ("writings", true, None, true),
        ("獎項", true, None, true),
        ("Docs", true, None, true),
        ("Notes", true, None, true),
        // Root home file.
        ("", true, None, true),
        ("nope", false, None, false),
        // The collision guard: a file query must never answer as its
        // same-stem sibling directory.
        ("assets/photo.png", false, None, false),
        ("assets/photo", true, None, true),
    ];
    for (q, md, static_idx, is_dir) in cases {
        assert_eq!(idx.dir_has_markdown_index(q), *md, "dir_has_markdown_index({q:?})");
        assert_eq!(
            idx.dir_has_static_index(q).as_deref(),
            *static_idx,
            "dir_has_static_index({q:?})"
        );
        assert_eq!(idx.is_dir(q), *is_dir, "is_dir({q:?})");
    }
}

/// The reported bug: `獎項/獎項.md` carries `url: awards`, so the folder lives
/// at `awards/` in URL space and NOWHERE on disk.
#[test]
fn url_override_folder_resolves_under_its_url_not_its_source_dir() {
    let t = tmp();
    let root = t.path();
    write(root, "獎項/獎項.md", b"# Awards");

    let mut map = ArticleMap::new();
    map.pages.insert("awards/".into(), "獎項/獎項.md".into());
    map.dir_overrides.insert("獎項".into(), "awards".into());
    let idx = EditorFolderIndex::new(root, &map);

    assert!(idx.dir_has_markdown_index("awards"), "the URL the build serves");
    assert!(idx.is_dir("awards"), "and it must read as a directory");
    // The SOURCE dir must NOT also go green: the build emits nothing at
    // `獎項/index.html`, so a green here would be a false green.
    assert!(!idx.dir_has_markdown_index("獎項"));
}

/// A map entry whose source file was deleted or renamed is dead: it must not
/// resolve, and it must not suppress anything.
#[test]
fn stale_map_entry_whose_source_vanished_falls_back_to_the_filesystem() {
    let t = tmp();
    let root = t.path();
    write(root, "news/post.md", b"# Post");

    let mut map = ArticleMap::new();
    map.pages.insert("awards/".into(), "獎項/獎項.md".into());
    let idx = EditorFolderIndex::new(root, &map);

    assert!(!idx.dir_has_markdown_index("awards"), "source is gone — dead entry");
    assert!(idx.dir_has_markdown_index("news"), "the filesystem half still answers");
}

/// Only `pages` (folder-index documents) may seed the URL half. `articles`
/// carries ordinary notes' pretty URLs — `posts/hello/` is a page, not a
/// folder listing, and classifying it as one would turn every `![[/posts/hello/]]`
/// into a folder card.
#[test]
fn article_urls_are_not_folders() {
    let t = tmp();
    let root = t.path();
    write(root, "posts/hello.md", b"# Hello");

    let mut map = ArticleMap::new();
    // NOTE the trailing slash: `compute_url_path` emits `{parent}/{slug}/index.html`
    // for non-index docs too, and `to_pretty_url` strips only `index.html`.
    map.articles
        .insert("posts/hello/".into(), article("posts/hello.md", "posts/hello/"));
    let idx = EditorFolderIndex::new(root, &map);

    assert!(!idx.dir_has_markdown_index("posts/hello"));
    assert!(!idx.is_dir("posts/hello"));
    // The containing folder is still an ordinary synthesized listing.
    assert!(idx.dir_has_markdown_index("posts"));
}

/// A passthrough subtree is an iframe, never a listing — and the exclusion
/// covers the whole subtree, not just its root.
#[test]
fn passthrough_subtree_is_an_iframe_not_a_listing() {
    let t = tmp();
    let root = t.path();
    write(root, "app/index.html", b"<html></html>");
    write(root, "app/sub/thing.js", b"1");

    let idx = EditorFolderIndex::new(root, &ArticleMap::default());

    assert!(!idx.dir_has_markdown_index("app"));
    assert_eq!(idx.dir_has_static_index("app").as_deref(), Some("index.html"));
    assert!(!idx.dir_has_markdown_index("app/sub"), "subtree exclusion");
}
