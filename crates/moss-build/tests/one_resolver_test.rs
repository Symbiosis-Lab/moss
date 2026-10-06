//! Which site file does a reference's target name? One function answers that
//! for every spelling: standard images, wiki embeds, both link forms, the
//! editor's classifier, and the rename, delete and scan tools. These tests run
//! each real entry point on one site with duplicated file names and require
//! the same file from all of them.

use std::fs;
use std::path::Path;

use moss_build::build::scan::scan::{build_content_graph, scan_folder};
use moss_build::editor::ref_scan::{clean_references_to_paths, scan_project_references_to};
use moss_build::editor::rename_plan::plan_moves;
use moss_build::editor::resolve::reference_resolver::{resolve_references_batch, RefTarget};
use moss_core::content_graph::ContentGraph;
use moss_core::resolve::reference::ReferenceKind;

fn touch(root: &Path, rel: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"x").unwrap();
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn tmp(name: &str) -> tempfile::TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
    fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new().prefix(name).tempdir_in(&base).unwrap()
}

fn graph_for(root: &Path) -> ContentGraph {
    build_content_graph(&scan_folder(&root.to_string_lossy()).expect("scan"))
}

/// The file the build links to for `md`, or the diagnostic kind when it does not.
fn build_pick(md: &str, page: &str, g: &ContentGraph) -> Option<String> {
    let mut doc = moss_core::ast::parse(md);
    let snap = moss_core::asset_snapshot::AssetSnapshot::new();
    let d = moss_core::ast::dispatch_wikilink_embeds(&mut doc, &snap, g, page);
    let u = moss_core::ast::resolve_urls(&mut doc, g, page);
    d.outgoing_links
        .iter()
        .chain(u.outgoing.iter())
        .map(|o| o.target_path.clone())
        .find(|p| g.contains_path(p))
}

/// What the editor's classifier makes of `text` as an embed on `page`: its
/// kind, the file it shows, and the candidates it lists when it is ambiguous.
fn editor_full(text: &str, page: &str, root: &Path) -> (ReferenceKind, Option<String>, Vec<String>) {
    let out = resolve_references_batch(&[RefTarget { text: text.to_string(), is_embed: true }], page, root);
    let e = &out[0];
    let rr = root.canonicalize().unwrap();
    let path = e.resolved.as_ref().map(|a| {
        Path::new(&a.absolute_path)
            .strip_prefix(&rr)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| a.absolute_path.clone())
    });
    (e.kind.clone(), path, e.candidates.clone())
}

fn fixture() -> tempfile::TempDir {
    let d = tmp("one_resolver_");
    for f in [
        "a/b/page.md", "a/x/page.md", "a/c/d/page.md", "q/page.md",
        "a/photos/photo.jpg", "a/b/deep/photos/unique.jpg",
        "a/b/dup.jpg", "a/c/dup.jpg", "z/far/deep/dup.jpg",
        "z/far.jpg", "a/x/y/far.jpg",
        "a/b/my photo.jpg", "a/b/pct%20name.jpg", "a/b/pct name.jpg",
    ] {
        touch(d.path(), f);
    }
    d
}

#[test]
fn every_entry_point_names_the_same_file() {
    let d = fixture();
    let r = d.path();
    let g = graph_for(r);
    // (page, target, the file all five must name)
    let rows: [(&str, &str, &str); 10] = [
        ("a/c/d/page.md", "dup.jpg", "a/c/dup.jpg"),
        ("a/x/page.md", "far.jpg", "a/x/y/far.jpg"),
        ("a/b/page.md", "../photos/photo", "a/photos/photo.jpg"),
        ("a/b/page.md", "../../../x/photo.jpg", "a/photos/photo.jpg"),
        ("a/c/d/page.md", "./dup.jpg", "a/c/dup.jpg"),
        ("a/c/d/page.md", "../../b/dup.jpg", "a/b/dup.jpg"),
        ("a/b/page.md", "pct%20name.jpg", "a/b/pct%20name.jpg"),
        ("a/b/page.md", "my%20photo.jpg", "a/b/my photo.jpg"),
        ("a/b/page.md", "unique", "a/b/deep/photos/unique.jpg"),
        ("q/page.md", "photos/unique.jpg", "a/b/deep/photos/unique.jpg"),
    ];
    for (page, t, want) in rows {
        let std_image = build_pick(&format!("![]({t})\n"), page, &g);
        let embed = build_pick(&format!("![[{t}]]\n"), page, &g);
        let link = build_pick(&format!("[x]({t})\n"), page, &g);
        let wiki_link = build_pick(&format!("[[{t}|x]]\n"), page, &g);
        let (kind, ed, _) = editor_full(t, page, r);
        let want = Some(want.to_string());
        let ctx = format!("{t} from {page}");
        assert_eq!(embed, want, "wiki embed, {ctx}");
        assert_eq!(std_image, want, "standard image, {ctx}");
        assert_eq!(link, want, "standard link, {ctx}");
        assert_eq!(wiki_link, want, "wiki link, {ctx}");
        assert_eq!(ed, want, "editor, {ctx} ({kind:?})");
    }
}

/// A leading `/` starts at the site root: that exact path wins when it exists,
/// and otherwise the target is searched for like any other. (A link written
/// with a leading `/` is a published address and is not a file reference.)
#[test]
fn a_leading_slash_names_the_same_file_for_images_embeds_and_the_editor() {
    let d = fixture();
    let r = d.path();
    let g = graph_for(r);
    for (t, want) in [
        ("/a/photos/photo.jpg", "a/photos/photo.jpg"),
        ("/nope/dup.jpg", "a/c/dup.jpg"),
        ("/dup.jpg", "a/c/dup.jpg"),
    ] {
        let page = "a/c/d/page.md";
        let want = Some(want.to_string());
        assert_eq!(build_pick(&format!("![[{t}]]\n"), page, &g), want, "wiki embed {t}");
        assert_eq!(build_pick(&format!("![]({t})\n"), page, &g), want, "standard image {t}");
        assert_eq!(editor_full(t, page, r).1, want, "editor {t}");
    }
}

#[test]
fn the_editor_shows_the_nearest_copy_and_reports_only_a_true_tie() {
    let d = fixture();
    let r = d.path();
    let g = graph_for(r);

    // One copy is strictly nearest: the editor names it, no ambiguity.
    let (kind, path, candidates) = editor_full("dup.jpg", "a/c/d/page.md", r);
    assert_eq!(kind, ReferenceKind::Image);
    assert_eq!(path.as_deref(), Some("a/c/dup.jpg"));
    assert!(candidates.is_empty());

    // From a/x, the copies in a/b and a/c are equally near (z/far is farther):
    // a real tie. The editor says so and lists just the tied copies; the build
    // still links the first of them.
    let (kind, _, candidates) = editor_full("dup.jpg", "a/x/page.md", r);
    assert_eq!(kind, ReferenceKind::Ambiguous);
    assert_eq!(candidates, vec!["a/b/dup.jpg".to_string(), "a/c/dup.jpg".to_string()]);
    assert_eq!(build_pick("![[dup.jpg]]\n", "a/x/page.md", &g).as_deref(), Some("a/b/dup.jpg"));
    assert_eq!(build_pick("![](dup.jpg)\n", "a/x/page.md", &g).as_deref(), Some("a/b/dup.jpg"));
}

#[test]
fn rename_scan_and_delete_see_the_same_references() {
    let d = tmp("one_resolver_refs_");
    let r = d.path();
    write(r, "x/dup.md", "# x\n");
    write(r, "y/dup.md", "# y\n");
    write(r, "top.md", "See [[dup]] and [t](dup.md).\n");
    let root = r.canonicalize().unwrap();
    let abs = |rel: &str| root.join(rel).to_string_lossy().to_string();

    // The build links both to x/dup.md (alphabetical among equally near files).
    let g = graph_for(r);
    assert_eq!(build_pick("[[dup]]\n", "top.md", &g).as_deref(), Some("x/dup.md"));
    assert_eq!(build_pick("[t](dup.md)\n", "top.md", &g).as_deref(), Some("x/dup.md"));

    let plan = plan_moves(&root, &[(abs("x/dup.md"), abs("x/renamed.md"))]).expect("plan");
    let scan = scan_project_references_to(&root.join("x/dup.md"), &root).expect("scan");
    assert_eq!(plan.edits.len(), 2, "rename plan");
    assert_eq!(scan.len(), 2, "reference scan");

    // The other copy is referenced by nothing, by every tool.
    let plan_y = plan_moves(&root, &[(abs("y/dup.md"), abs("y/renamed.md"))]).expect("plan y");
    let scan_y = scan_project_references_to(&root.join("y/dup.md"), &root).expect("scan y");
    assert_eq!(plan_y.edits.len(), 0, "rename plan, other copy");
    assert_eq!(scan_y.len(), 0, "reference scan, other copy");

    // Delete's cleanup removes exactly those two references.
    clean_references_to_paths(&root, &[abs("x/dup.md")]).expect("clean");
    let after = fs::read_to_string(root.join("top.md")).unwrap();
    assert!(!after.contains("[[dup]]") && !after.contains("(dup.md)"), "cleanup left {after:?}");
}

/// A target that, joined to the page's folder, names an existing file means
/// that file, even when a nearer-by-name copy sits beneath the page.
const PATH_PAGE: &str = "![one](../c/p.png)\n![[../c/p.png|two]]\n![three](c/p.png)\n[four](../c/p.png)\n";

fn path_site() -> tempfile::TempDir {
    let d = tmp("one_resolver_paths_");
    touch(d.path(), "a/c/p.png");
    touch(d.path(), "a/b/c/p.png");
    write(d.path(), "a/b/page.md", PATH_PAGE);
    d
}

#[test]
fn a_path_that_names_a_file_means_that_file_not_a_nearer_copy_by_name() {
    let d = path_site();
    let g = graph_for(d.path());
    let page = "a/b/page.md";
    let far = Some("a/c/p.png".to_string());
    let near = Some("a/b/c/p.png".to_string());
    assert_eq!(build_pick("![one](../c/p.png)\n", page, &g), far, "one");
    assert_eq!(build_pick("![[../c/p.png|two]]\n", page, &g), far, "two");
    assert_eq!(build_pick("![three](c/p.png)\n", page, &g), near, "three");
    assert_eq!(build_pick("[four](../c/p.png)\n", page, &g), far, "four");
    assert_eq!(build_pick("[[../c/p.png|four]]\n", page, &g), far, "wiki link");
    assert_eq!(editor_full("../c/p.png", page, d.path()).1, far, "editor, one");
    assert_eq!(editor_full("c/p.png", page, d.path()).1, near, "editor, three");
}

#[test]
fn a_rename_rewrites_the_references_that_name_the_file_and_only_those() {
    let d = path_site();
    let root = d.path().canonicalize().unwrap();
    let abs = |rel: &str| root.join(rel).to_string_lossy().to_string();
    let plan = plan_moves(&root, &[(abs("a/c/p.png"), abs("a/c/q.png"))]).expect("plan");
    let mut text = PATH_PAGE.to_string();
    let mut edits: Vec<_> = plan.edits.iter().filter(|e| e.file.ends_with("a/b/page.md")).collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.byte_from));
    for e in edits {
        text.replace_range(e.byte_from..e.byte_to, &e.new_text);
    }
    assert_eq!(text, "![one](../c/q.png)\n![[../c/q.png|two]]\n![three](c/p.png)\n[four](../c/q.png)\n");
}

/// Two different files would both be named by `notes/a.md`: the one beside the
/// page wins, for a page link and a wiki link alike.
#[test]
fn the_path_from_the_page_beats_the_path_from_the_site_root() {
    let d = tmp("one_resolver_conflict_");
    for f in ["blog/post.md", "blog/notes/a.md", "notes/a.md"] {
        touch(d.path(), f);
    }
    let g = graph_for(d.path());
    for md in ["[x](notes/a.md)\n", "[[notes/a.md|x]]\n", "[[notes/a|x]]\n", "[x](notes/a)\n"] {
        assert_eq!(build_pick(md, "blog/post.md", &g).as_deref(), Some("blog/notes/a.md"), "{md}");
    }
}

#[test]
fn an_extension_less_path_from_the_page_finds_the_file_in_that_folder() {
    let d = tmp("one_resolver_noext_");
    for f in ["a/b/page.md", "a/photos/photo.jpg", "a/b/photos/photo.jpg", "a/notes/alpha.md", "a/b/notes/alpha.md"] {
        touch(d.path(), f);
    }
    let g = graph_for(d.path());
    assert_eq!(build_pick("![[../photos/photo]]\n", "a/b/page.md", &g).as_deref(), Some("a/photos/photo.jpg"));
    assert_eq!(build_pick("[[../notes/alpha]]\n", "a/b/page.md", &g).as_deref(), Some("a/notes/alpha.md"));
}
