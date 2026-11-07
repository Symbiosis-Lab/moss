use super::*;
use super::super::types::EditorScope;
use super::super::urls::{Article, UrlMap};

/// The folder index a resolve builds over this listing: only what a build
/// walk of the site reads.
fn index(files: &[&str], map: Option<UrlMap>) -> EditorFolderIndex {
    let input = ResolveInput {
        files: files.iter().map(|f| f.to_string()).collect(), directories: vec![],
        scope: EditorScope::Site, from_source: "index.md".into(), targets: vec![],
        url_index: map, root_name: "Site".into(), passthrough: vec![],
    };
    EditorFolderIndex::new(&input.members())
}

/// One listing, no URL map — the "never built" state. Table-driven so every
/// branch of the predicate is asked of the same index.
#[test]
fn folder_predicates_over_one_fixture_listing() {
    let idx = index(&[
        "index.md",
        // A pre-built app dropped in — passthrough subtree, iframe.
        "Resources/app/index.html",
        "Resources/app/sub/page.html",
        // An explicit markdown index.
        "news/index.md",
        // Index-less folder with children — the build synthesizes an index here.
        "writings/post.md",
        // Self-named folder note (the content-folder form).
        "評選/評選.md",
        // Home stems beyond `index.md`.
        "Docs/README.MD",
        "Notes/main.md",
        // `assets/photo/` is a real directory and `assets/photo.png` a real
        // file: `is_dir` must not conflate them.
        "assets/photo.png",
        "assets/photo/raw.tif",
        // A nested site is not a folder of this one, as in the build's scan.
        "shop/.moss/config.toml",
        "shop/index.md",
    ], None);

    // (query, markdown index?, static index, is_dir?)
    let cases: &[(&str, bool, Option<&str>, bool)] = &[
        ("Resources/app", false, Some("index.html"), true),
        // Subtree exclusion: the app owns every page beneath its root.
        ("Resources/app/sub", false, None, true),
        // The passthrough root's PARENT is an ordinary content folder.
        ("Resources", true, None, true),
        ("news", true, None, true),
        ("writings", true, None, true),
        ("評選", true, None, true),
        ("Docs", true, None, true),
        ("Notes", true, None, true),
        // Root home file.
        ("", true, None, true),
        ("nope", false, None, false),
        // A file query must never answer as its same-stem sibling directory.
        ("assets/photo.png", false, None, false),
        ("assets/photo", true, None, true),
    ];
    for (q, md, static_idx, is_dir) in cases {
        assert_eq!(idx.dir_has_markdown_index(q), *md, "dir_has_markdown_index({q:?})");
        assert_eq!(idx.dir_has_static_index(q).as_deref(), *static_idx, "dir_has_static_index({q:?})");
        assert_eq!(idx.is_dir(q), *is_dir, "is_dir({q:?})");
    }
    assert!(!idx.dir_has_markdown_index("shop"), "a nested site's home page is not a listing here");
}

/// `評選/評選.md` carries `url: awards`, so the folder lives at `awards/` in
/// URL space and nowhere on disk.
#[test]
fn url_override_folder_resolves_under_its_url_not_its_source_dir() {
    let mut map = UrlMap::default();
    map.pages.insert("awards/".into(), "評選/評選.md".into());
    map.dir_overrides.insert("評選".into(), "awards".into());
    let idx = index(&["評選/評選.md"], Some(map));

    assert!(idx.dir_has_markdown_index("awards"), "the URL the build serves");
    assert!(idx.is_dir("awards"), "and it must read as a directory");
    // The source dir must not also go green: the build emits nothing at
    // `評選/index.html`.
    assert!(!idx.dir_has_markdown_index("評選"));
}

/// A map entry whose source file was deleted or renamed is dead: it must not
/// resolve, and it must not suppress anything.
#[test]
fn stale_map_entry_whose_source_vanished_falls_back_to_the_listing() {
    let mut map = UrlMap::default();
    map.pages.insert("awards/".into(), "評選/評選.md".into());
    let idx = index(&["news/post.md"], Some(map));

    assert!(!idx.dir_has_markdown_index("awards"), "source is gone — dead entry");
    assert!(idx.dir_has_markdown_index("news"), "the listing half still answers");
}

/// Only `pages` (folder-index documents) may seed the URL half. `articles`
/// carries ordinary notes' pretty URLs — `posts/hello/` is a page, not a
/// folder listing, and classifying it as one would turn every
/// `![[/posts/hello/]]` into a folder card.
#[test]
fn article_urls_are_not_folders() {
    let mut map = UrlMap::default();
    // The trailing slash: a non-index page's URL is `{parent}/{slug}/` too.
    map.articles.insert("posts/hello/".into(), Article { source_path: "posts/hello.md".into(), ..Default::default() });
    let idx = index(&["posts/hello.md"], Some(map));

    assert!(!idx.dir_has_markdown_index("posts/hello"));
    assert!(!idx.is_dir("posts/hello"));
    // The containing folder is still an ordinary synthesized listing.
    assert!(idx.dir_has_markdown_index("posts"));
}

/// A passthrough subtree is an iframe, never a listing — and the exclusion
/// covers the whole subtree, not just its root.
#[test]
fn passthrough_subtree_is_an_iframe_not_a_listing() {
    let idx = index(&["app/index.html", "app/sub/thing.js"], None);

    assert!(!idx.dir_has_markdown_index("app"));
    assert_eq!(idx.dir_has_static_index("app").as_deref(), Some("index.html"));
    assert!(!idx.dir_has_markdown_index("app/sub"), "subtree exclusion");
}
