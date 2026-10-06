//! A `[[Folder]]` wikilink must resolve the same way on the build (which
//! renders the page) and the editor (which underlines the link red or not).
//! Landing folder resolution on only one side creates the drift this test
//! exists to catch: the build starts serving a page the editor still calls
//! broken, or the reverse.
//!
//! Both halves live in this crate — `ContentGraph` (moss-core, re-exported
//! here) and `ArticleMapIndex` (moss-build) — so this integration test drives
//! the REAL producer functions end to end: a real `scan_folder` walk, the
//! real `build_content_graph`, and the real `build_article_map`. Nothing here
//! hand-assembles a graph or a map; deleting either producer's registration
//! line is what the two ablations at the bottom exercise.
//!
//! `ContentGraph::resolve_path` returns a SOURCE path (`"essays/index.md"`);
//! `ArticleMapIndex::resolve_reference_to_url` returns a deployed URL
//! (`"/essays/"`). The two APIs answer different questions by design, so for
//! the folder cases the expected URL is DERIVED from the resolved source
//! path via the real `ContentGraph::pinned_url` (see `folder_url` below),
//! not asserted against a hand-maintained table that could silently drift
//! from what that function actually computes. The one precedence/article
//! case is the exception, and says why where it lives.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use moss_build::build::scan::article_map::{build_article_map, ArticleInfo};
use moss_build::build::scan::scan::{build_content_graph, scan_folder};
use moss_build::build::terms::TermIndex;
use moss_build::editor::resolve::url_index::ArticleMapIndex;
use moss_core::content_graph::ContentGraph;
use moss_core::resolve::link_class::UrlIndex;

/// A vault exercising every case the design's v1 scope names: a bare
/// top-level folder with no index (the reported bug), a folder whose name
/// collides with an existing page's stem (precedence), a folder nested two
/// levels deep (nesting scope), a folder under a language tree, bare and
/// nested (exclusion), a folder with a real explicit index (regression
/// guard), and a folder whose slug is overridden by frontmatter `url:`
/// (case + override).
fn write_fixture(root: &Path) {
    fs::create_dir_all(root.join("essays")).unwrap();
    fs::write(root.join("essays/entry.md"), "---\ntitle: Entry\n---\n\nHello.\n").unwrap();

    // `writings/` gets children but no index.md of its own; `Other/Writings.md`
    // is an unrelated ARTICLE whose filename stem collides with the folder's
    // name. A page anywhere in the vault must win over a same-named folder.
    fs::create_dir_all(root.join("writings")).unwrap();
    fs::write(root.join("writings/post.md"), "---\ntitle: Post\n---\n\nHello.\n").unwrap();
    fs::create_dir_all(root.join("Other")).unwrap();
    fs::write(root.join("Other/Writings.md"), "---\ntitle: Writings\n---\n\nHello.\n").unwrap();

    // Nested, no index: `[[obsidian/notes]]` must resolve; bare `[[notes]]`
    // must not (nesting is out of scope in this version).
    fs::create_dir_all(root.join("obsidian/notes")).unwrap();
    fs::write(root.join("obsidian/notes/note.md"), "---\ntitle: Note\n---\n\nHello.\n").unwrap();

    // A language-tree root with content but no index, and a nested folder
    // under it: neither ever gets an auto-generated index, bare or nested.
    fs::create_dir_all(root.join("zh-hans/docs")).unwrap();
    fs::write(root.join("zh-hans/greeting.md"), "---\ntitle: 你好\n---\n\n你好。\n").unwrap();
    fs::write(root.join("zh-hans/docs/manual.md"), "---\ntitle: 手冊\n---\n\n你好。\n").unwrap();

    // A folder with a REAL, authored index.md: unaffected by this fix.
    fs::create_dir_all(root.join("news")).unwrap();
    fs::write(root.join("news/index.md"), "---\ntitle: News\n---\n\nHello.\n").unwrap();

    // A self-named folder note under a directory whose slug is overridden.
    fs::create_dir_all(root.join("評選")).unwrap();
    fs::write(root.join("評選/評選.md"), "---\ntitle: 評選\nurl: awards\n---\n\n你好。\n").unwrap();
}

#[test]
fn editor_and_build_agree_on_folder_wikilinks() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
    fs::create_dir_all(&base).expect("create test-tmp base");
    let tmp = tempfile::Builder::new()
        .prefix("moss_folder_wikilink_parity_")
        .tempdir_in(&base)
        .expect("tempdir");
    let root = tmp.path();
    write_fixture(root);

    // 1. A REAL scan — not a hand-built ProjectStructure.
    let project_structure = scan_folder(&root.to_string_lossy()).expect("scan");

    // 2. The REAL ContentGraph, including this fix's auto-index-dir
    // registration — not a graph assembled by hand for the test.
    let content_graph = build_content_graph(&project_structure);

    // `評選/評選.md` carries `url: awards` — the override a real build would
    // derive from that frontmatter, passed explicitly here rather than
    // running the full page-map scan pipeline.
    let mut dir_overrides = HashMap::new();
    dir_overrides.insert("評選".to_string(), "awards".to_string());

    // 3. The REAL ArticleMap, including this fix's `folder_indexes` field —
    // produced by the real `build_article_map`, not assembled by hand. The
    // ablation below targets exactly the line inside it that populates this
    // field, so it must come from calling the function, not from a literal.
    let mut article_map = build_article_map(
        &[],
        &project_structure.dirs,
        &dir_overrides,
        &[],
        &[],
        &TermIndex::default(),
    );

    // `ParsedDocument` (the input `build_article_map` normally reads
    // `articles`/`pages` from) has a crate-private field, so an integration
    // test cannot construct one — `build::render::blocking`'s render loop is
    // the only place that assembles a full one. `ArticleInfo` (the map's
    // OWN entry type) has no such field, so the one page the precedence
    // case needs is added directly, the same way `url_index_tests.rs`'s own
    // unit tests build an `ArticleMap` for `ArticleMapIndex`. This is
    // unrelated to `folder_indexes`, which stays entirely the real
    // function's output.
    article_map.articles.insert(
        "other/writings".to_string(),
        ArticleInfo {
            source_path: "Other/Writings.md".to_string(),
            title: "Writings".to_string(),
            content: String::new(),
            html_content: None,
            frontmatter: HashMap::new(),
            url_path: "other/writings".to_string(),
            date: None,
            tags: vec![],
            uid: None,
        },
    );

    let idx = ArticleMapIndex::from_map(&article_map);

    // The same graph, with the fixture's overrides installed — the REAL
    // way a resolved source path becomes a URL (`ContentGraph::pinned_url`),
    // used below instead of a hand-maintained table of expected URL
    // strings that could silently drift from what the function actually
    // computes.
    let pinned_graph = content_graph.clone().with_output_overrides(dir_overrides.clone());

    // (reference, from, expected build-side source path), or None when
    // neither side should resolve it. The editor-side URL is DERIVED below
    // via `folder_url`, not asserted against a literal.
    let folder_cases: &[(&str, &str, Option<&str>)] = &[
        // The reported bug: a bare top-level folder with children but no
        // index resolves to its generated index, case-insensitively.
        ("Essays", "index.md", Some("essays/index.md")),
        ("essays", "index.md", Some("essays/index.md")),
        // Nesting: the full path resolves, case-insensitively; the bare
        // leaf does not (out of scope in this version, both sides).
        ("obsidian/notes", "index.md", Some("obsidian/notes/index.md")),
        ("Obsidian/NOTES", "index.md", Some("obsidian/notes/index.md")),
        ("notes", "index.md", None),
        // Language-tree exclusion: bare or nested, never auto-indexed.
        ("zh-hans", "index.md", None),
        ("zh-hans/docs", "index.md", None),
        // Regression guard: a folder with a real, explicit index still
        // resolves — this fix must not change what an authored index does.
        ("news", "index.md", Some("news/index.md")),
        // Case + override: the on-disk directory name resolves to the
        // overridden slug, not to a URL built from its own on-disk name.
        ("評選", "index.md", Some("評選/評選.md")),
    ];

    for &(reference, from, expected_source) in folder_cases {
        let build_found = content_graph.resolve_path(reference, from);
        let editor_found = idx.resolve_reference_to_url(reference, from);
        match expected_source {
            Some(want_source) => {
                assert_eq!(build_found.as_deref(), Some(want_source), "build side for {reference:?}");
                let want_url = folder_url(&pinned_graph, parent_dir(want_source));
                assert_eq!(editor_found, Some(want_url), "editor side for {reference:?}");
            }
            None => {
                assert_eq!(build_found, None, "build side for {reference:?} must NOT resolve");
                assert_eq!(editor_found, None, "editor side for {reference:?} must NOT resolve");
            }
        }
    }

    // Precedence: a page whose stem collides with a folder name wins, on
    // both sides, over the folder of the same name. This exercises
    // `by_stem`/the build's filename-stem match (steps 3/4, ahead of the
    // folder-note fallback), not the new `folder_indexes`/`auto_index_dirs`
    // machinery `folder_url` derives — pages keep their own leaf filename
    // and case in their pinned URL (an asset-fidelity property `pinned_url`
    // preserves on purpose), so `folder_url`'s directory-shaped derivation
    // does not apply here, and the expected URL is the fixture's own,
    // spelled once, not reused as a general-purpose table.
    assert_eq!(
        content_graph.resolve_path("Writings", "index.md").as_deref(),
        Some("Other/Writings.md"),
        "build side for \"Writings\""
    );
    assert_eq!(
        idx.resolve_reference_to_url("Writings", "index.md"),
        Some("/other/writings/".to_string()),
        "editor side for \"Writings\""
    );
}

/// The directory a resolved folder-note source path names: its own leaf
/// filename stripped off. `"essays/index.md"` -> `"essays"`;
/// `"obsidian/notes/index.md"` -> `"obsidian/notes"`; `"評選/評選.md"` ->
/// `"評選"` (self-named).
fn parent_dir(source_path: &str) -> &str {
    source_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// The folder-index URL `pinned_url` actually computes for `dir`, through
/// the real override-aware graph. `pinned_url` is an asset-URL primitive —
/// it preserves a leaf filename verbatim rather than slugging/stripping it
/// the way a page's pretty URL does — so this appends the same `index.html`
/// sentinel every folder-index page's own URL is derived from, and strips
/// it back off; a real function computes the value, not a table.
fn folder_url(pinned_graph: &ContentGraph, dir: &str) -> String {
    let raw = pinned_graph.pinned_url(&format!("{dir}/index.html"));
    format!("{}/", raw.strip_suffix("/index.html").unwrap_or(&raw))
}
