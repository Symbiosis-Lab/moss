//! A plugin that contributes a listing style, through the real binary: the
//! plugin is a few lines of JavaScript in `.moss/plugins/boardview/` and the
//! style is `board`. All names and text are invented.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

struct Site {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    root: PathBuf,
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A plugin whose `listing` returns `body`, a JavaScript expression for the result.
fn plugin(root: &Path, name: &str, styles: &[&str], body: &str) {
    let styles: Vec<String> = styles.iter().map(|s| format!(r#""{s}": {{"label": "Board", "description": "Cards"}}"#)).collect();
    write(
        root,
        &format!(".moss/plugins/{name}/manifest.json"),
        &format!(
            r#"{{"name":"{name}","version":"1.0.0","entry":"main.js","contributes":{{"listing_styles":{{{}}}}}}}"#,
            styles.join(",")
        ),
    );
    let global = format!("{}Plugin", name[..1].to_uppercase() + &name[1..]);
    write(root, &format!(".moss/plugins/{name}/main.js"), &format!("var {global} = (() => ({{ async listing(ctx) {{ return {body}; }} }}))();"));
}

fn site(style: &str) -> Site {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("site");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    write(&root, ".moss/config.toml", "# mine\n");
    write(&root, "index.md", "---\ntitle: Home\n---\nWelcome\n");
    write(&root, "board/index.md", &format!("---\ntitle: Board\nchildren_style: {style}\n---\nIntro\n"));
    write(&root, "board/Beta.md", "---\ntitle: Beta\ndate: 2026-01-02\nauthor: Ada Example\n---\nbeta body\n");
    write(&root, "board/sub/Alpha.md", "---\ntitle: Alpha\ndate: 2026-01-01\n---\n![](pic.jpg)\n\nSee [[Beta]]\n");
    write(&root, "board/sub/pic.jpg", "x");
    Site { _tmp: tmp, home, root }
}

impl Site {
    fn build(&self, flags: &[&str]) -> Output {
        Command::new(MOSS)
            .arg("build")
            .arg(&self.root)
            .args(flags)
            .env("MOSS_HOME", &self.home)
            .env_remove("MOSS_ENV")
            .output()
            .expect("spawn moss-cli")
    }

    fn out(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(".moss/build.nosync/current").join(rel)).ok()
    }

    fn manifest_lists(&self, rel: &str) -> bool {
        let hashes = std::fs::read_to_string(self.root.join(".moss/build.nosync/hashes.json")).unwrap();
        hashes.contains(&format!("\"{rel}\""))
    }
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const CARDS: &str = r#"{ html: '<ul class="boardview">' + ctx.pages.map(p => '<li>' + p.title + '</li>').join('') + '</ul>',
  head: [{ stylesheet: 'board.css' }, { script: 'board.js' }],
  files: [{ path: 'board.css', text: '.boardview{}' }, { path: 'board.js', base64: 'dm9pZCAwOw==' }] }"#;

#[test]
fn the_plugins_html_replaces_the_listing_and_its_head_and_files_ship() {
    let s = site("board");
    plugin(&s.root, "boardview", &["board"], CARDS);
    let o = s.build(&["--allow-plugins", "--strict"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let page = s.out("board/index.html").unwrap();
    assert!(page.contains(r#"<ul class="boardview"><li>"#), "{page}");
    assert!(!page.contains("moss-card"), "the default listing is still drawn: {page}");
    assert!(!page.contains("moss:listing"), "a marker is left: {page}");
    assert!(page.contains(r#"<link rel="stylesheet" href="board.css">"#) && page.contains(r#"<script src="board.js" defer>"#), "{page}");
    assert_eq!(s.out("board/board.css").as_deref(), Some(".boardview{}"));
    assert_eq!(s.out("board/board.js").as_deref(), Some("void 0;"));
    assert!(s.manifest_lists("board/board.css") && s.manifest_lists("board/index.html"));
}

const DUMP: &str = r#"{ html: '<script type="application/json" id="ctx">' + JSON.stringify(ctx) + '</script>' }"#;

fn context_of(page: &str) -> serde_json::Value {
    let json = page.split(r#"id="ctx">"#).nth(1).and_then(|rest| rest.split("</script>").next()).expect("no context in the page");
    serde_json::from_str(json).unwrap()
}

#[test]
fn the_plugin_receives_exactly_the_pages_the_listing_shows() {
    let s = site("board");
    write(&s.root, "board/sub/index.md", "---\ntitle: Sub\n---\nsub home\n");
    write(&s.root, "board/Gamma.md", "---\ntitle: Gamma\ndate: 2026-01-03\n---\ngamma\n");
    write(&s.root, "board/Gamma.zh-hans.md", "---\ntitle: Gamma ZH\ndate: 2026-01-03\n---\ngamma zh\n");
    write(&s.root, "board/Hidden.md", "---\ntitle: Hidden\ndate: 2026-01-04\nlisted: false\n---\nhidden\n");
    plugin(&s.root, "boardview", &["board"], DUMP);
    let o = s.build(&["--allow-plugins", "--strict"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let ctx = context_of(&s.out("board/index.html").unwrap());
    assert_eq!(ctx["model"], 1);
    assert_eq!(ctx["style"], "board");
    assert_eq!(ctx["page"]["title"], "Board");
    assert_eq!(ctx["page"]["url"], "./");
    assert_eq!(ctx["page"]["frontmatter"]["children_style"], "board");

    let pages = ctx["pages"].as_array().unwrap();
    let mut urls: Vec<&str> = pages.iter().map(|p| p["url"].as_str().unwrap()).collect();
    urls.sort();
    assert_eq!(urls, ["beta/", "gamma/", "sub/"], "{pages:#?}");
    let beta = pages.iter().find(|p| p["title"] == "Beta").unwrap();
    assert_eq!(beta["url_path"], "board/beta/index.html");
    assert_eq!(beta["folder"], "");
    assert_eq!(beta["frontmatter"]["author"], "Ada Example");
    let gamma = pages.iter().find(|p| p["title"] == "Gamma").unwrap();
    assert_eq!(gamma["translations"][0]["lang"], "zh-Hans", "{gamma}");
    assert!(gamma["translations"][0]["url_path"].as_str().unwrap().contains("gamma"), "{gamma}");
    assert_eq!(ctx["folders"][0]["url"], "sub/");
    assert_eq!(ctx["terms"]["authors"]["Ada Example"], "../authors/ada-example/", "relative to the listing page: {}", ctx["terms"]);
}

fn img_srcs(html: &str) -> Vec<&str> {
    html.split("<img ").skip(1).filter_map(|tag| tag.split("src=\"").nth(1)?.split('"').next()).collect()
}

#[test]
fn a_childs_body_resolves_from_the_listing_page() {
    let s = site("board");
    write(&s.root, "board/sub/index.md", "---\ntitle: Sub\n---\nsub home\n");
    write(&s.root, "board/sub/Deep.md", "---\ntitle: Deep\n---\n![](pic.jpg)\n\n<img class=\"raw\" src=\"pic.jpg\">\n\nSee [[Beta]] and [next to it](Alpha.md)\n");
    write(&s.root, "board/index.md", "---\ntitle: Board\nchildren_style: board\nchildren_depth: all\n---\nIntro\n");
    plugin(&s.root, "boardview", &["board"], DUMP);
    let o = s.build(&["--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let ctx = context_of(&s.out("board/index.html").unwrap());
    let deep = ctx["pages"].as_array().unwrap().iter().find(|p| p["title"] == "Deep").expect("depth all lists the nested page");
    assert_eq!(deep["folder"], "sub");
    let html = deep["html"].as_str().unwrap();
    // Each reference, followed from board/, lands on the file or page beside the child.
    for src in img_srcs(html) {
        assert!(src == "/board/sub/pic.jpg" || src == "sub/pic.jpg", "image does not resolve from board/: {src} in {html}");
    }
    assert!(html.contains("href=\"beta/\""), "link to a page in board/ does not resolve from board/: {html}");
    assert!(html.contains("href=\"sub/alpha/\""), "link to a sibling page does not resolve from board/: {html}");
}

#[test]
fn an_unknown_style_warns_once_and_keeps_the_default_listing() {
    let s = site("nonesuch");
    let o = s.build(&["--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let err = stderr(&o);
    assert_eq!(err.matches("nonesuch").count(), 1, "{err}");
    assert!(err.contains("board/index.md"), "{err}");
    let page = s.out("board/index.html").unwrap();
    assert!(page.contains("moss-card") && !page.contains("moss:listing"), "{page}");
}

#[test]
fn no_plugins_falls_back_with_one_warning() {
    let s = site("board");
    plugin(&s.root, "boardview", &["board"], CARDS);
    let o = s.build(&["--no-plugins", "--strict"]);
    let err = stderr(&o);
    assert_eq!(err.matches("`children_style: board`").count(), 1, "{err}");
    assert!(!o.status.success(), "--strict must fail on the warning: {err}");
    let page = s.out("board/index.html").unwrap();
    assert!(page.contains("moss-card") && !page.contains("boardview"), "{page}");
}

#[test]
fn a_refused_path_warns_and_is_not_written() {
    let s = site("board");
    plugin(
        &s.root,
        "boardview",
        &["board"],
        r#"{ html: '<p>ok</p>', files: [{ path: '../escape.txt', text: 'x' }, { path: '/abs.txt', text: 'x' }, { path: '_moss/x.txt', text: 'x' }, { path: 'index.html', text: 'x' }, { path: 'fine.txt', text: 'x' }] }"#,
    );
    let o = s.build(&["--allow-plugins", "--strict"]);
    let err = stderr(&o);
    assert!(!o.status.success(), "{err}");
    for refused in ["../escape.txt", "/abs.txt", "_moss/x.txt", "`index.html`"] {
        assert!(err.contains(refused) && err.contains("[boardview]"), "{refused}: {err}");
    }
    assert_eq!(s.out("board/fine.txt").as_deref(), Some("x"));
    assert!(s.out("escape.txt").is_none() && s.out("board/_moss/x.txt").is_none());
    assert!(s.out("board/index.html").unwrap().contains("<p>ok</p>"), "a refused file does not cost the listing");
}

#[test]
fn two_plugins_claiming_one_id_are_refused_with_one_warning() {
    let s = site("board");
    plugin(&s.root, "boardview", &["board"], r#"{ html: '<p>first</p>' }"#);
    plugin(&s.root, "cardview", &["board"], r#"{ html: '<p>second</p>' }"#);
    let o = s.build(&["--allow-plugins"]);
    let err = stderr(&o);
    assert_eq!(err.matches("is contributed by both").count(), 1, "{err}");
    assert!(err.contains("`boardview`") && err.contains("`cardview`"), "{err}");
    let page = s.out("board/index.html").unwrap();
    assert!(page.contains("<p>first</p>") && !page.contains("second"), "{page}");
}

#[test]
fn a_plugin_that_throws_or_returns_garbage_leaves_the_default_listing() {
    for body in ["(() => { throw new Error('boom'); })()", "{ nothtml: 1 }", "'text'"] {
        let s = site("board");
        plugin(&s.root, "boardview", &["board"], body);
        let o = s.build(&["--allow-plugins"]);
        assert!(o.status.success(), "{body}: {}", stderr(&o));
        let err = stderr(&o);
        assert_eq!(err.matches("[boardview]").count(), 1, "{body}: {err}");
        let page = s.out("board/index.html").unwrap();
        assert!(page.contains("moss-card") && !page.contains("moss:listing"), "{body}: {page}");
    }
}

#[test]
fn the_styles_width_applies_to_a_page_that_sets_none() {
    let s = site("board");
    plugin(&s.root, "boardview", &["board"], r#"{ html: '<p>w</p>' }"#);
    let manifest = std::fs::read_to_string(s.root.join(".moss/plugins/boardview/manifest.json")).unwrap();
    write(&s.root, ".moss/plugins/boardview/manifest.json", &manifest.replace(r#""description": "Cards""#, r#""description": "Cards", "content_width": "full""#));
    let o = s.build(&["--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(s.out("board/index.html").unwrap().contains(r#"data-content-width="full""#));
}

fn titles_of(ctx: &serde_json::Value, key: &str) -> Vec<String> {
    ctx[key].as_array().unwrap().iter().map(|p| p["title"].as_str().unwrap().to_string()).collect()
}

#[test]
fn the_plugin_gets_the_one_page_a_limited_sorted_listing_shows() {
    let host = "---\ntitle: Board\nchildren_style: board\nchildren_limit: 1\nsort: title\n---\nIntro\n";
    let s = site("board");
    for (file, title) in [("Zed", "Zed"), ("Apple", "Apple"), ("Mango", "Mango")] {
        write(&s.root, &format!("board/{file}.md"), &format!("---\ntitle: {title}\n---\nbody\n"));
    }
    write(&s.root, "board/index.md", host);
    plugin(&s.root, "boardview", &["board"], DUMP);
    let o = s.build(&["--allow-plugins", "--strict"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let ctx = context_of(&s.out("board/index.html").unwrap());
    // What moss's own listing shows for the same folder, limit and sort.
    let control = site("list");
    for file in ["Zed", "Apple", "Mango"] {
        write(&control.root, &format!("board/{file}.md"), &format!("---\ntitle: {file}\n---\nbody\n"));
    }
    write(&control.root, "board/index.md", &host.replace("board", "list").replace("title: Board", "title: Board"));
    assert!(control.build(&[]).status.success());
    let shown = control.out("board/index.html").unwrap();
    let pages = titles_of(&ctx, "pages");
    assert_eq!(pages.len(), 1, "{pages:?}");
    assert!(shown.contains(&format!(">{}<", pages[0])), "the listing shows {shown}, the plugin got {pages:?}");
    assert_eq!(pages[0], "Apple", "sort: title puts Apple first");
    assert!(!shown.contains(">Zed<") && !shown.contains(">Mango<"));
}

#[test]
fn a_folders_record_carries_its_translations_and_the_folder_it_sits_in() {
    let s = site("board");
    write(&s.root, "board/index.md", "---\ntitle: Board\nchildren_style: board\nchildren_depth: all\n---\nIntro\n");
    write(&s.root, "board/sub/index.md", "---\ntitle: Sub\n---\nsub home\n");
    write(&s.root, "board/sub/index.zh-hans.md", "---\ntitle: Sub ZH\n---\nsub zh\n");
    write(&s.root, "board/sub/deep/index.md", "---\ntitle: Deep\n---\ndeep home\n");
    plugin(&s.root, "boardview", &["board"], DUMP);
    let o = s.build(&["--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let ctx = context_of(&s.out("board/index.html").unwrap());
    let folders = ctx["folders"].as_array().unwrap();
    let sub = folders.iter().find(|f| f["title"] == "Sub").expect("sub is a folder with a home page");
    assert_eq!(sub["folder"], "");
    assert_eq!(sub["url"], "sub/");
    assert_eq!(sub["translations"][0]["lang"], "zh-Hans", "{sub}");
    let deep = folders.iter().find(|f| f["title"] == "Deep").expect("a nested folder is listed too");
    assert_eq!(deep["folder"], "sub");
    assert_eq!(deep["url"], "sub/deep/");
}

#[test]
fn a_listing_takes_folders_from_its_own_language_tree_only() {
    let s = site("board");
    write(&s.root, "index.md", "---\ntitle: Home\nchildren_style: board\n---\nWelcome\n");
    write(&s.root, "de/index.md", "---\ntitle: Start\nchildren_style: board\nchildren_depth: all\n---\nWillkommen\n");
    write(&s.root, "de/board/index.md", "---\ntitle: Tafel\n---\nTafel\n");
    write(&s.root, "de/board/Beta.md", "---\ntitle: Beta DE\n---\nbeta de\n");
    plugin(&s.root, "boardview", &["board"], DUMP);
    let o = s.build(&["--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let root = titles_of(&context_of(&s.out("index.html").unwrap()), "folders");
    assert!(root.contains(&"Board".to_string()), "{root:?}");
    assert!(!root.iter().any(|t| t == "Start" || t == "Tafel"), "the root listing took in the German tree: {root:?}");
    let de = context_of(&s.out("de/index.html").unwrap());
    assert_eq!(titles_of(&de, "folders"), ["Tafel"]);
    assert!(titles_of(&de, "pages").contains(&"Beta DE".to_string()), "{}", de["pages"]);
}

#[test]
fn a_childs_body_loses_its_title_and_keeps_links_that_resolve_anywhere() {
    let s = site("board");
    write(
        &s.root,
        "board/Alpha.md",
        "---\ntitle: Alpha\n---\nJump [down](#frag), read [more](https://example.org/x), or [write](mailto:a@example.org).\n\n## Frag\n",
    );
    plugin(&s.root, "boardview", &["board"], DUMP);
    let o = s.build(&["--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let ctx = context_of(&s.out("board/index.html").unwrap());
    let alpha = ctx["pages"].as_array().unwrap().iter().find(|p| p["title"] == "Alpha").expect("alpha is listed");
    let html = alpha["html"].as_str().unwrap();
    assert!(!html.contains("moss-article-title") && !html.contains("data-source-"), "the title heading or an editor marker is still in the body: {html}");
    // An in-page fragment is left as written (it resolves against the listing page).
    for kept in ["href=\"#frag\"", "href=\"https://example.org/x\"", "href=\"mailto:a@example.org\""] {
        assert!(html.contains(kept), "{kept} was rewritten: {html}");
    }
}

#[test]
fn a_file_that_collides_with_other_output_is_refused() {
    let s = site("board");
    write(&s.root, "board/data.csv", "a,b\n");
    plugin(
        &s.root,
        "boardview",
        &["board"],
        r#"{ html: '<p>ok</p>', files: [{ path: 'data.csv', text: 'x' }, { path: 'sub/pic.jpg', text: 'x' }, { path: 'again.txt', text: '1' }, { path: 'again.txt', text: '2' }] }"#,
    );
    let o = s.build(&["--allow-plugins"]);
    let err = stderr(&o);
    for refused in ["`data.csv`", "`sub/pic.jpg`"] {
        assert!(err.contains(refused) && err.contains("[boardview]"), "{refused}: {err}");
    }
    assert_eq!(err.matches("`again.txt`").count(), 1, "the second write of one path is refused: {err}");
    assert_eq!(s.out("board/again.txt").as_deref(), Some("1"));
    assert_eq!(s.out("board/data.csv").as_deref(), Some("a,b\n"), "the author's file is untouched");
}

#[test]
fn a_style_on_a_page_that_draws_no_listing_is_judged_against_the_plugins() {
    let s = site("board");
    plugin(&s.root, "boardview", &["board"], r#"{ html: '<p>ok</p>' }"#);
    // Beta is a leaf: its `children_style` draws nothing, but a typo is still a typo.
    write(&s.root, "board/Beta.md", "---\ntitle: Beta\nchildren_style: nonesuch\n---\nbeta\n");
    write(&s.root, "board/Gamma.md", "---\ntitle: Gamma\nchildren_style: board\n---\ngamma\n");
    let o = s.build(&["--allow-plugins"]);
    let err = stderr(&o);
    assert_eq!(err.matches("`children_style: nonesuch`").count(), 1, "{err}");
    assert!(err.contains("Beta.md"), "{err}");
    assert!(!err.contains("Gamma.md"), "a plugin's own id is fine on any page: {err}");
}

#[test]
fn no_plugins_warns_once_for_a_style_on_a_page_that_draws_no_listing() {
    let s = site("list");
    plugin(&s.root, "boardview", &["board"], r#"{ html: '<p>ok</p>' }"#);
    write(&s.root, "board/Gamma.md", "---\ntitle: Gamma\nchildren_style: board\n---\ngamma\n");
    let o = s.build(&["--no-plugins"]);
    let err = stderr(&o);
    assert_eq!(err.matches("`children_style: board`").count(), 1, "{err}");
    assert!(err.contains("Gamma.md"), "{err}");
}

#[test]
fn a_plugin_that_failed_is_not_asked_again_in_the_same_build() {
    let s = site("board");
    write(&s.root, "other/index.md", "---\ntitle: Other\nchildren_style: board\n---\nIntro\n");
    write(&s.root, "other/Item.md", "---\ntitle: Item\n---\nitem\n");
    plugin(&s.root, "boardview", &["board"], "(() => { throw new Error('boom'); })()");
    let o = s.build(&["--allow-plugins"]);
    let err = stderr(&o);
    assert_eq!(err.matches("boom").count(), 1, "the plugin ran once: {err}");
    assert_eq!(err.matches("[boardview]").count(), 1, "one warning for the plugin: {err}");
    assert!(err.contains("all 2 of its pages"), "{err}");
    for page in ["board/index.html", "other/index.html"] {
        assert!(s.out(page).unwrap().contains("moss-card"), "{page} keeps the default listing");
    }
}

#[test]
fn a_warning_about_a_page_names_its_source_file() {
    let s = site("board");
    plugin(
        &s.root,
        "boardview",
        &["board"],
        r#"{ html: '<p>ok</p>', warnings: ['plain one', { text: 'about beta', page: 'board/beta/index.html' }] }"#,
    );
    let o = s.build(&["--allow-plugins", "--strict"]);
    let err = stderr(&o);
    assert!(!o.status.success(), "{err}");
    assert!(err.contains("[boardview] plain one"), "{err}");
    assert!(err.contains("[boardview] board/Beta.md: about beta"), "{err}");
}

#[test]
fn a_declared_field_reaches_the_plugin_parsed_and_an_invalid_one_does_not() {
    let s = site("board");
    write(&s.root, ".moss/config.toml", "[fields.years]\ntype = \"period\"\n");
    write(&s.root, "board/Beta.md", "---\ntitle: Beta\ndate: 2026-01-02\nyears: 1927–1933\n---\nbeta body\n");
    write(&s.root, "board/Gamma.md", "---\ntitle: Gamma\nyears: the twenties\n---\ngamma\n");
    plugin(&s.root, "boardview", &["board"], DUMP);
    let o = s.build(&["--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stderr(&o).contains("field 'years'"), "the invalid value is warned about: {}", stderr(&o));
    let ctx = context_of(&s.out("board/index.html").unwrap());
    let pages = ctx["pages"].as_array().unwrap();
    let beta = pages.iter().find(|p| p["title"] == "Beta").unwrap();
    assert_eq!(
        beta["fields"]["years"],
        serde_json::json!({
            "start": {"value": "1927", "precision": "year", "qualifier": null},
            "end": {"value": "1933", "precision": "year", "qualifier": null},
            "open": false,
            "text": "1927–1933"
        }),
        "{beta}"
    );
    let gamma = pages.iter().find(|p| p["title"] == "Gamma").unwrap();
    assert!(gamma["fields"].get("years").is_none(), "an invalid value is left out: {gamma}");
    assert_eq!(gamma["frontmatter"]["years"], "the twenties", "the raw value stays in frontmatter");
}

const FIELDS: &str = "[fields.kind]\ntype = \"choice\"\nof = { Building = { en = \"Built\", de = \"Errichtet\" }, Service = \"Founded\" }\nplace = \"eyebrow\"\n\n[fields.stage]\ntype = \"choice\"\nof = [\"Service\", \"Building\"]\n";

#[test]
fn the_plugin_gets_declarations_in_the_listing_language_and_each_pages_field_errors() {
    let s = site("board");
    write(&s.root, ".moss/config.toml", FIELDS);
    write(&s.root, "board/index.md", "---\ntitle: Board\nlang: de\nchildren_style: board\n---\nIntro\n");
    write(&s.root, "board/Beta.md", "---\ntitle: Beta\nlang: en\nkind: Building\nstage: Service\n---\nbeta body\n");
    write(&s.root, "board/Gamma.md", "---\ntitle: Gamma\nkind: Buidling\n---\ngamma\n");
    plugin(&s.root, "boardview", &["board"], DUMP);
    let o = s.build(&["--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let ctx = context_of(&s.out("board/index.html").unwrap());

    assert_eq!(
        ctx["declarations"]["kind"],
        serde_json::json!({
            "type": "choice",
            "choices": [{"value": "Building", "label": "Errichtet"}, {"value": "Service", "label": "Founded"}],
            "place": "eyebrow",
        }),
    );
    assert_eq!(
        ctx["declarations"]["stage"],
        serde_json::json!({
            "type": "choice",
            "choices": [{"value": "Service", "label": "Service"}, {"value": "Building", "label": "Building"}],
            "place": null,
        }),
        "declared order is kept, not sorted"
    );

    let pages = ctx["pages"].as_array().unwrap();
    let beta = pages.iter().find(|p| p["title"] == "Beta").unwrap();
    assert_eq!(beta["fields"]["kind"], serde_json::json!({"value": "Building", "label": "Built"}), "the page's own language");
    assert_eq!(beta["field_errors"], serde_json::json!({}), "always present, empty when every value fits");
    let gamma = pages.iter().find(|p| p["title"] == "Gamma").unwrap();
    assert_eq!(gamma["field_errors"]["kind"], "\"Buidling\" is not one of: Building, Service", "{gamma}");
    assert!(gamma["fields"].get("kind").is_none(), "a value that does not fit is left out: {gamma}");
}

#[test]
fn a_count_on_a_german_page_reads_its_decimal_mark_from_the_page_language() {
    let s = site("board");
    write(&s.root, ".moss/config.toml", "[fields.homes]\ntype = \"count\"\n");
    write(&s.root, "board/Beta.md", "---\ntitle: Beta\nlang: de\nhomes: 1.382 Wohnungen\n---\nbeta body\n");
    write(&s.root, "board/Gamma.md", "---\ntitle: Gamma\nhomes: 1,382 flats\n---\ngamma\n");
    plugin(&s.root, "boardview", &["board"], DUMP);
    let o = s.build(&["--allow-plugins", "--strict"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let ctx = context_of(&s.out("board/index.html").unwrap());
    let pages = ctx["pages"].as_array().unwrap();
    let beta = pages.iter().find(|p| p["title"] == "Beta").unwrap();
    assert_eq!(beta["fields"]["homes"]["number"], 1382, "{beta}");
    let gamma = pages.iter().find(|p| p["title"] == "Gamma").unwrap();
    assert_eq!(gamma["fields"]["homes"]["number"], 1382, "{gamma}");
}
