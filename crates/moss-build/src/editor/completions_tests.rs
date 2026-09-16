use super::*;
use std::fs;
use tempfile::TempDir;

/// Repo-local temp dir (`target/test-tmp/` is gitignored).
fn repo_temp() -> TempDir {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    fs::create_dir_all(&dir).unwrap();
    // Canonical, so the `..` in the manifest-relative base never trips the
    // path guard the functions under test apply.
    TempDir::new_in(dir.canonicalize().unwrap()).unwrap()
}

fn root_str(root: &Path) -> String {
    root.to_string_lossy().to_string()
}

fn complete(root: &Path, syntax: LinkSyntax, prefix: &str, from: &str) -> Vec<WikilinkCompletion> {
    link_completions(&root_str(root), from, syntax, prefix, None)
}

fn inserts(rows: &[WikilinkCompletion]) -> Vec<&str> {
    rows.iter().map(|r| r.insert.as_str()).collect()
}

fn find<'a>(rows: &'a [WikilinkCompletion], insert: &str) -> &'a WikilinkCompletion {
    rows.iter().find(|r| r.insert == insert).unwrap_or_else(|| panic!("no row inserts {insert:?}: {rows:?}"))
}

/// A map as the build would write it for `about.md` (titled) and a tag page.
fn write_map(root: &Path, json: &str) {
    fs::create_dir_all(root.join(".moss/build")).unwrap();
    fs::write(root.join(".moss/build/article-map.json"), json).unwrap();
}

#[test]
fn walks_the_source_tree_and_classifies_each_row() {
    let project = repo_temp();
    let root = project.path();
    fs::write(root.join("about.md"), b"# About").unwrap();
    fs::write(root.join("photo.png"), b"\x89PNG").unwrap();
    fs::create_dir(root.join("notes")).unwrap();
    fs::write(root.join("notes/ideas.md"), b"# Ideas").unwrap();

    let rows = complete(root, LinkSyntax::Wikilink, "", "index.md");

    let about = find(&rows, "about");
    assert_eq!((about.label.as_str(), about.detail.as_deref(), about.kind), ("about", None, TargetKind::Page));
    assert_eq!(find(&rows, "ideas").kind, TargetKind::Page, "recursive walk finds a subdir page");
    let photo = find(&rows, "photo.png");
    assert_eq!((photo.label.as_str(), photo.detail.as_deref(), photo.kind), ("photo.png", None, TargetKind::Asset));
    let notes = find(&rows, "notes/");
    assert_eq!((notes.label.as_str(), notes.kind), ("notes/", TargetKind::Folder));

    // Link syntax: pages before assets before folders.
    let pos = |i: &str| rows.iter().position(|r| r.insert == i).unwrap();
    assert!(pos("about") < pos("photo.png"));
    assert!(pos("photo.png") < pos("notes/"));
}

#[test]
fn a_page_row_shows_its_title_and_the_map_supplies_it() {
    let project = repo_temp();
    let root = project.path();
    fs::write(root.join("about.md"), b"x").unwrap();
    write_map(root, r#"{"articles":{},"pages":{"about/":"about.md"},"page_titles":{"about/":"關於我們"}}"#);

    let rows = complete(root, LinkSyntax::Wikilink, "關於", "index.md");
    let about = find(&rows, "about");
    assert_eq!(about.label, "關於我們", "the title is the name; the stem is the address");
    assert_eq!(about.detail.as_deref(), Some("about"), "detail shows the insert when it differs");
}

#[test]
fn url_space_offers_deployed_urls_and_generated_pages_only_there() {
    let project = repo_temp();
    let root = project.path();
    fs::write(root.join("about.md"), b"x").unwrap();
    fs::write(root.join("hero.png"), b"x").unwrap();
    fs::create_dir(root.join("notes")).unwrap();
    fs::write(root.join("notes/ideas.md"), b"x").unwrap();
    write_map(
        root,
        r#"{"articles":{},"pages":{"about/":"about.md"},"generated":["tags/","tags/design/"],
            "terms":{"tags/design":{"display":"Design","claimed_by":null},
                     "authors/ma":{"display":"Ma","claimed_by":"about/"}}}"#,
    );

    let urls = complete(root, LinkSyntax::Inline, "/", "index.md");
    let got = inserts(&urls);
    assert!(got.contains(&"/about/"), "page by deployed URL: {got:?}");
    assert!(got.contains(&"/hero.png"), "asset by rooted source path: {got:?}");
    assert!(got.contains(&"/tags/design/"), "unclaimed term page: {got:?}");
    assert!(got.contains(&"/tags/"), "namespace root: {got:?}");
    assert!(!got.contains(&"/authors/ma/"), "a claimed term lives at its claim: {got:?}");
    assert!(!got.iter().any(|i| i.ends_with("ideas") || i.ends_with("ideas/")), "unbuilt page has no URL: {got:?}");
    assert!(!got.contains(&"notes/"), "folders are a source-space offer: {got:?}");
    let design = find(&urls, "/tags/design/");
    assert_eq!((design.label.as_str(), design.kind), ("Design", TargetKind::Generated));

    let source = complete(root, LinkSyntax::Inline, "", "index.md");
    let got = inserts(&source);
    assert!(got.contains(&"about"), "source space, exact form: {got:?}");
    assert!(got.contains(&"notes/"), "folders offered in source space: {got:?}");
    assert!(!got.iter().any(|i| i.starts_with("/tags")), "generated pages need URL space: {got:?}");
}

#[test]
fn prefers_the_same_language_tree_from_an_absolute_from_file() {
    // The editor sends an ABSOLUTE from_file; it must be relativized before
    // ranking or the language-tree bias sees `/Users/...` and inverts.
    let project = repo_temp();
    let root = project.path();
    fs::create_dir(root.join("en")).unwrap();
    fs::create_dir(root.join("zh-hans")).unwrap();
    fs::write(root.join("en/report-en.md"), b"x").unwrap();
    fs::write(root.join("zh-hans/report-zh.md"), b"x").unwrap();

    let from_abs = root.join("zh-hans/about.md").to_string_lossy().to_string();
    let rows = complete(root, LinkSyntax::Wikilink, "report", &from_abs);
    assert_eq!(rows[0].insert, "report-zh");
}

#[test]
fn allowed_kinds_keeps_only_those_assets_and_empty_matches_nothing() {
    let project = repo_temp();
    let root = project.path();
    fs::write(root.join("about.md"), b"x").unwrap();
    fs::write(root.join("photo.png"), b"x").unwrap();
    fs::write(root.join("clip.mp4"), b"x").unwrap();
    let r = root_str(root);

    let all = link_completions(&r, "index.md", LinkSyntax::Wikilink, "", None);
    let got = inserts(&all);
    assert!(got.contains(&"about") && got.contains(&"photo.png") && got.contains(&"clip.mp4"), "{got:?}");

    let images = link_completions(&r, "index.md", LinkSyntax::Wikilink, "", Some(&[ExtKind::Image]));
    assert_eq!(inserts(&images), vec!["photo.png"]);

    let none = link_completions(&r, "index.md", LinkSyntax::Wikilink, "", Some(&[]));
    assert!(none.is_empty(), "got: {none:?}");
}

#[test]
fn path_query_writes_the_exact_form_the_resolver_reproduces() {
    let project = repo_temp();
    let root = project.path();
    fs::create_dir_all(root.join("關於/assets")).unwrap();
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::write(root.join("關於/y.md"), b"x").unwrap();
    fs::write(root.join("關於/assets/x.png"), b"x").unwrap();
    fs::write(root.join("assets/hero.png"), b"x").unwrap();
    fs::write(root.join("notes/ideas.md"), b"x").unwrap();

    // Inside the source's subtree: source-relative, what authors hand-write.
    let rows = complete(root, LinkSyntax::Embed, "關於/x", "關於/y.md");
    let x = find(&rows, "assets/x.png");
    assert_eq!((x.label.as_str(), x.detail.as_deref()), ("x.png", Some("assets/x.png")));
    // Outside it: rooted, so a same-named sibling can never capture it.
    let rows = complete(root, LinkSyntax::Embed, "assets/h", "關於/y.md");
    assert_eq!(find(&rows, "/assets/hero.png").label, "hero.png");
    // A page: root-relative without `.md`.
    let rows = complete(root, LinkSyntax::Wikilink, "notes/id", "index.md");
    assert_eq!(find(&rows, "notes/ideas").label, "ideas");
    // A bare wikilink query keeps the Obsidian filename form.
    let rows = complete(root, LinkSyntax::Embed, "x", "關於/y.md");
    assert_eq!(find(&rows, "x.png").detail, None);
    // An inline link is always exact, even for a bare query.
    let rows = complete(root, LinkSyntax::Inline, "x", "關於/y.md");
    assert_eq!(find(&rows, "assets/x.png").label, "x.png");
}

// ── headings ─────────────────────────────────────────────────────────

fn headings(root: &Path, syntax: LinkSyntax, prefix: &str, page: &str) -> Vec<WikilinkCompletion> {
    heading_completions(&root_str(root), page, syntax, prefix).unwrap()
}

#[test]
fn heading_rows_carry_text_for_wikilinks_and_slug_for_inline_links() {
    let project = repo_temp();
    let root = project.path();
    fs::write(root.join("page.md"), "# Title\n\nbody\n\n## Background Notes\n\n## Details\n").unwrap();

    let rows = headings(root, LinkSyntax::Wikilink, "", "page.md");
    let bg = find(&rows, "Background Notes");
    assert_eq!((bg.detail.as_deref(), bg.kind), (Some("H2"), TargetKind::Heading));
    assert_eq!(find(&rows, "Title").detail.as_deref(), Some("H1"));

    let rows = headings(root, LinkSyntax::Inline, "back", "page.md");
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].label.as_str(), rows[0].detail.as_deref()), ("Background Notes", Some("background-notes")));
    assert_eq!(rows[0].insert, "background-notes");
}

#[test]
fn heading_missing_file_is_empty_and_traversal_is_refused() {
    let project = repo_temp();
    let root = project.path();
    assert!(headings(root, LinkSyntax::Wikilink, "", "nope.md").is_empty());
    let err = heading_completions(&root_str(root), "../escape.md", LinkSyntax::Wikilink, "").unwrap_err();
    assert!(err.contains("traversal"), "got: {err}");
}

/// The completion must insert text that resolves to the anchor the PUBLISHED
/// page actually has. `[site].math` defaults on; parsed with math OFF,
/// intraword `*` is eaten as emphasis before the `$…$` is seen, so
/// `# Dual $V^*$ end` extracts as `Dual $V^$ end` — corruption twice over.
/// Mutation check: swap `extract_headings_with_config` for the bare
/// `extract_headings` in `heading_targets` and this goes red.
#[test]
fn heading_text_resolves_to_the_published_anchor() {
    use moss_core::heading::anchor::obsidian_heading_anchor;
    let project = repo_temp();
    let root = project.path();
    let md = "# Dual $V^*$ and $W^*$ end\n\n## Conv $f*g$ here\n\n### Euler $e^{i\\pi}=-1$ id\n";
    fs::write(root.join("page.md"), md).unwrap();

    let published = moss_core::heading::extract::extract_headings_with_config(
        md,
        &moss_core::ast::ParseConfig { math: true, ..Default::default() },
    );
    let rows = headings(root, LinkSyntax::Wikilink, "", "page.md");
    assert_eq!(rows.len(), published.len(), "one completion per heading");
    for h in &published {
        let row = find(&rows, &h.text);
        assert_eq!(obsidian_heading_anchor(&row.insert), h.slug, "{:?} resolves to an anchor the page lacks", row.insert);
    }
    assert!(rows.iter().any(|r| r.insert == "Dual $V^*$ and $W^*$ end"), "the TeX asterisks must survive: {rows:?}");
}

// ── asset_ref_in: a file picked in the system dialog ─────────────────

fn picked(root: &Path, rel: &str, from_page: &str) -> Result<WikilinkCompletion, String> {
    asset_ref_in(root, &root.join(rel).to_string_lossy(), &root.join(from_page).to_string_lossy())
}

#[test]
fn a_picked_file_is_always_the_exact_form() {
    let tmp = repo_temp();
    let root = tmp.path();
    fs::create_dir_all(root.join("img")).unwrap();
    fs::create_dir_all(root.join("photos")).unwrap();
    fs::create_dir_all(root.join("關於")).unwrap();
    fs::write(root.join("img/cover.png"), b"x").unwrap();
    fs::write(root.join("photos/cover.png"), b"x").unwrap();

    let r = picked(root, "img/cover.png", "index.md").unwrap();
    assert_eq!((r.insert.as_str(), r.label.as_str(), r.kind), ("img/cover.png", "cover.png", TargetKind::Asset));
    assert_eq!(r.detail.as_deref(), Some("img/cover.png"));
    // Outside the page's subtree: rooted.
    assert_eq!(picked(root, "photos/cover.png", "關於/x.md").unwrap().insert, "/photos/cover.png");
}

#[test]
fn a_pick_outside_the_vault_is_refused() {
    let tmp = repo_temp();
    let outside = tempfile::TempDir::new().unwrap();
    fs::write(outside.path().join("cover.png"), b"x").unwrap();
    let r = asset_ref_in(
        tmp.path(),
        &outside.path().join("cover.png").to_string_lossy(),
        &tmp.path().join("index.md").to_string_lossy(),
    );
    assert!(r.is_err(), "a path outside the project must not resolve");
}

#[cfg(unix)]
#[test]
fn a_symlink_escaping_the_vault_is_refused() {
    // The string guard passes (the link itself is inside the project); only
    // the canonical re-check catches this. That is why the command does both.
    let tmp = repo_temp();
    let outside = tempfile::TempDir::new().unwrap();
    let target = outside.path().join("secret.png");
    fs::write(&target, b"x").unwrap();
    let link = tmp.path().join("inside.png");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let r = asset_ref_in(tmp.path(), &link.to_string_lossy(), &tmp.path().join("index.md").to_string_lossy());
    assert!(r.is_err(), "a symlink out of the project must not resolve");
}

#[test]
fn a_folder_and_an_unclassifiable_extension_are_refused() {
    let tmp = repo_temp();
    fs::create_dir_all(tmp.path().join("img")).unwrap();
    fs::write(tmp.path().join("notes.xyz"), b"x").unwrap();
    assert!(picked(tmp.path(), "img", "index.md").is_err());
    assert!(picked(tmp.path(), "notes.xyz", "index.md").is_err());
}
