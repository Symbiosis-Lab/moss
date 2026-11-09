use super::*;
use super::urls::{UrlMap, Article};

fn input(files: &[&str], from: &str, targets: &[(&str, bool)]) -> ResolveInput {
    ResolveInput {
        files: files.iter().map(|s| s.to_string()).collect(), directories: vec![],
        scope: EditorScope::Site, from_source: from.into(),
        targets: targets.iter().map(|(text, is_embed)| RefTarget { text: text.to_string(), is_embed: *is_embed }).collect(),
        url_index: None, root_name: "Site".into(), passthrough: vec![],
    }
}

#[test]
fn file_paths_and_provenance_are_host_independent() {
    let cases = [
        ("local.png", "News/post.md", "News/local.png", AssetProvenance::Literal),
        ("/assets/Summit.jpg", "News/post.md", "assets/Summit.jpg", AssetProvenance::Literal),
        ("Summit.jpg", "News/post.md", "assets/Summit.jpg", AssetProvenance::BareFuzzy),
        ("./assets/Summit.jpg", "News/post.md", "assets/Summit.jpg", AssetProvenance::SeparatorFallback),
        ("./assets/Fern.jpg", "team/Team.md", "assets/Fern.JPG", AssetProvenance::CaseMismatch),
    ];
    for (target, from, path, provenance) in cases {
        let out = resolve_references(&input(&["News/local.png", "assets/Summit.jpg", "assets/Fern.JPG"], from, &[(target, true)]));
        let asset = out[0].resolved.as_ref().unwrap();
        assert_eq!(asset.absolute_path, path, "{target}");
        assert_eq!(asset.request_url, format!("/{path}"));
        assert_eq!(asset.provenance, provenance, "{target}");
        assert_eq!((asset.width, asset.height, asset.size_bytes), (None, None, 0));
    }
}

#[test]
fn bare_case_mismatch_and_ambiguous_names() {
    let out = resolve_references(&input(&["assets/Fern.JPG", "a/photo.jpg", "b/photo.jpg"], "post.md", &[("fern.jpg", true), ("photo.jpg", true)]));
    assert_eq!(out[0].resolved.as_ref().unwrap().absolute_path, "assets/Fern.JPG");
    assert_eq!(out[1].kind, ReferenceKind::Ambiguous);
    assert_eq!(out[1].candidates, ["a/photo.jpg", "b/photo.jpg"]);
    assert!(out[1].resolved.is_none());
}

#[test]
fn a_whole_folder_listing_resolves_only_over_what_the_build_reads() {
    let listed = [
        "pics/photo.png", ".git/objects/photo.png", ".moss/build.nosync/staging/photo.png", "node_modules/x/photo.png",
        "other/.moss/config.toml", "other/photo.png", "AGENTS.md", "notes/.draft.md", "notes/keep.md",
    ];
    let targets = [("photo.png", true), ("AGENTS", false), (".draft", false), ("keep", false), (".git/", true), ("node_modules/", true), ("other/", true)];
    for scope in [EditorScope::Site, EditorScope::Folder] {
        let mut i = input(&listed, "index.md", &targets);
        i.scope = scope;
        i.directories = [".git", ".moss", "node_modules", "other", "pics", "notes"].map(String::from).to_vec();
        let out = resolve_references(&i);
        if scope == EditorScope::Site {
            let photo = out[0].resolved.as_ref().unwrap_or_else(|| panic!("{:?}", out[0]));
            assert_eq!((photo.absolute_path.as_str(), photo.provenance), ("pics/photo.png", AssetProvenance::BareFuzzy));
            assert!(out[0].candidates.is_empty());
            assert_eq!(out[3].resolved_path.as_deref(), Some("notes/keep.md"));
        }
        assert_eq!(out[1].resolved_path, None, "{scope:?}: a root agent file is not a page");
        assert_eq!(out[2].resolved_path, None, "{scope:?}: a dot-named page is the author's own draft");
        for r in &out[4..] {
            assert_eq!(r.kind, ReferenceKind::NotFound, "{scope:?}: {} is not part of the site", r.target);
        }
    }
    let i = input(&listed, "index.md", &[]);
    assert_eq!(resolve_asset(&i, "photo.png").map(|a| a.absolute_path), Some("pics/photo.png".into()));
}

#[test]
fn a_map_cannot_open_a_file_that_is_not_in_the_site() {
    let mut i = input(&["notes/Page.md"], "index.md", &[("Page", false), ("Outside", false), ("Renamed", false)]);
    i.url_index = Some(UrlMap { articles: [
        ("page/".into(), Article { source_path: "notes/Page.md".into(), ..Default::default() }),
        ("outside/".into(), Article { source_path: "../Outside.md".into(), ..Default::default() }),
        ("renamed/".into(), Article { source_path: "notes/Renamed.md".into(), ..Default::default() }),
    ].into(), ..Default::default() });
    let out = resolve_references(&i);
    assert_eq!(out[0].resolved_path.as_deref(), Some("notes/Page.md"));
    for r in &out[1..] {
        assert!(r.url.is_some(), "the map knows the page: {r:?}");
        assert_eq!(r.resolved_path, None, "a path outside the listing is never opened: {r:?}");
    }
}

#[test]
fn links_without_builds_open_sources_and_keep_anchors() {
    let out = resolve_references(&input(&["notes/Page.md"], "index.md", &[("Page#heading", false), ("https://example.org/a#b", false)]));
    assert_eq!(out[0].kind, ReferenceKind::Link { anchor: Some("heading".into()) });
    assert_eq!(out[0].anchor.as_deref(), Some("heading"));
    assert_eq!(out[0].resolved_path.as_deref(), Some("notes/Page.md"));
    assert!(out[0].url.is_none());
    assert!(matches!(out[1].kind, ReferenceKind::External { .. }));
}

#[test]
fn links_with_builds_answer_the_deployed_url() {
    let mut i = input(&["notes/Page.md"], "index.md", &[("Page#heading", false)]);
    i.url_index = Some(UrlMap { articles: [("custom/".into(), Article { source_path: "notes/Page.md".into(), ..Default::default() })].into(), ..Default::default() });
    let out = resolve_references(&i);
    assert_eq!(out[0].url.as_deref(), Some("/custom/#heading"));
    assert_eq!(out[0].resolved_path.as_deref(), Some("notes/Page.md"));
    assert_eq!(out[0].anchor.as_deref(), Some("heading"));
}

#[test]
fn folder_home_priority_staleness_and_empty_directories() {
    for (files, expected) in [
        (vec!["notes/index.md", "notes/README.md"], Some("notes/index.md")),
        (vec!["notes/README.md", "notes/notes.md"], Some("notes/README.md")),
        (vec!["notes/notes.md"], Some("notes/notes.md")),
        (vec!["notes/article.md"], None),
        (vec![], None),
    ] {
        let mut i = input(&files, "index.md", &[("/notes/", true)]);
        i.directories.push("notes".into());
        i.url_index = Some(UrlMap { pages: [("notes/".into(), "notes/gone.md".into())].into(), ..Default::default() });
        let out = resolve_references(&i);
        assert_eq!(out[0].kind, ReferenceKind::FolderListing);
        assert_eq!(out[0].resolved_path.as_deref(), expected);
    }
}

#[test]
fn folder_overrides_static_indexes_and_passthrough() {
    let mut i = input(&["評選/評選.md", "app/Index.HTM", "app/sub/page.html"], "index.md", &[("/awards/", true), ("/app/", true), ("/app/sub/", true)]);
    i.url_index = Some(UrlMap { pages: [("awards/".into(), "評選/評選.md".into())].into(), ..Default::default() });
    let out = resolve_references(&i);
    assert_eq!(out[0].resolved_path.as_deref(), Some("評選/評選.md"));
    assert_eq!(out[1].kind, ReferenceKind::FolderIndexIframe);
    assert_eq!(out[1].resolved_path.as_deref(), Some("app/Index.HTM"));
    assert_eq!(out[2].kind, ReferenceKind::NotFound);
    i.passthrough.push("!app".into());
    assert_eq!(resolve_references(&i)[1].kind, ReferenceKind::FolderListing);
}

#[test]
fn loose_targets_do_not_expand_each_others_search_scope() {
    let mut i = input(&["beside.png", "img/shot.png", "img/deep/far.png", "notes/Page.md"], "note.md", &[("beside.png", true), ("img/shot.png", true), ("./img/deep/far.png", true), ("far.png", true), ("notes/Page", false), ("Page", false), ("/img/", true)]);
    i.scope = EditorScope::Folder;
    let out = resolve_references(&i);
    assert!(out[..3].iter().all(|o| o.resolved.is_some()));
    assert_eq!(out[3].kind, ReferenceKind::NotFound);
    assert_eq!(out[4].resolved_path.as_deref(), Some("notes/Page.md"));
    assert_eq!(out[5].kind, ReferenceKind::NotFound);
    assert_eq!(out[6].kind, ReferenceKind::NotFound);
}

#[test]
fn spelled_out_lookup_agrees_with_the_complete_snapshot() {
    let i = input(&["assets/x.png", "News/assets/x.png", "News/post.md", "Team/Photo.JPG", "a/b/c.png", "a/b/c.png.md", "zh-hans/p.md", "uk/z.png", "q.png.md"], "", &[]);
    let graph = files::graph(&i.files);
    for (target, from) in [("assets/x.png", "News/post.md"), ("/assets/x.png", "News/post.md"), ("../assets/x.png", "News/post.md"), ("team/photo.jpg", "post.md"), ("TEAM/Photo.JPG", "News/post.md"), ("a/b/c.png", "x/y.md"), ("uk/z.png", "zh-hans/p.md")] {
        assert_eq!(files::spelled_out_file(target, from, &i.files), Some(resolve_file_target(target, from, &graph)), "{target}");
    }
    for (target, from) in [("x.png", "Team/p.md"), ("shop/y.png", "post.md"), ("assets/x", "News/post.md"), ("q.png", "News/post.md"), ("assets/%78.png", "post.md")] {
        assert_eq!(files::spelled_out_file(target, from, &i.files), None, "{target}");
    }
}

#[test]
fn case_twins_are_testable_without_a_case_sensitive_disk() {
    for files in [vec!["a/photo.jpg", "a/Photo.jpg"], vec!["a/photo.jpg", "A/other.png"]] {
        let i = input(&files, "p.md", &[]);
        assert_eq!(files::spelled_out_file("a/photo.jpg", "p.md", &i.files), None);
    }
}

#[test]
fn a_directory_named_as_a_home_file_is_not_opened() {
    let i = input(&["notes/index.md/child.md", "notes/README.md"], "index.md", &[("/notes/", true)]);
    let out = resolve_references(&i);
    assert_eq!(out[0].kind, ReferenceKind::FolderListing);
    assert!(out[0].resolved_path.is_none());
}

#[test]
fn the_single_asset_lookup_opens_the_file_the_build_links_for_an_ambiguous_name() {
    let i = input(&["a/photo.jpg", "b/photo.jpg"], "post.md", &[("photo.jpg", true)]);
    assert_eq!(resolve_references(&i)[0].kind, ReferenceKind::Ambiguous, "the batch keeps the ambiguity for the lint");
    let built = files::graph(&i.files).resolve_path("photo.jpg", "post.md").expect("the build links one of them");
    let asset = resolve_asset(&i, "photo.jpg").expect("the single lookup opens a file");
    assert_eq!(asset.absolute_path, built);
    assert_eq!(asset.provenance, AssetProvenance::SeparatorFallback);
}
