//! A markdown page owns the `.js` and `.css` beside it with the same stem: it
//! links them after the theme's own, no other page does, and the files are
//! still published at their own address.

#[path = "support/site.rs"]
mod site;

use moss_build::build::BuildTrigger;
use site::Site;

const THEME_CSS: &str = "body { margin: 0 }";
const THEME_JS: &str = "window.themeRan = true;";

fn site() -> Site {
    let page = |title: &str| format!("---\ntitle: {title}\n---\n\nBody of {title}.\n");
    Site::new(&[
        (".moss/theme/style.css", THEME_CSS),
        (".moss/theme/script.js", THEME_JS),
        ("index.md", &page("Home")),
        ("index.css", ".home-only { color: red }"),
        ("notes/essay.md", &page("Essay")),
        ("notes/essay.js", "console.log('essay')"),
        ("notes/essay.css", ".essay { color: blue }"),
        ("notes/plain.md", &page("Plain")),
        ("notes/styled.md", &page("Styled")),
        ("notes/styled.css", ".styled { color: green }"),
        ("notes/index.md", &page("Notes")),
        ("notes/index.js", "console.log('notes home')"),
        ("zh-hans/essay.md", &page("Essay zh")),
        ("zh-hans/essay.js", "console.log('zh')"),
        ("Field Notes/Café Log.md", &page("Cafe")),
        ("Field Notes/Café Log.js", "console.log('cafe')"),
        ("demo.html", "<!doctype html><title>Demo</title><p>raw page</p>"),
        ("demo.js", "console.log('demo')"),
    ])
}

fn module_scripts(html: &str) -> Vec<&str> {
    html.split("<script type=\"module\" src=\"").skip(1).map(|r| r.split('"').next().unwrap()).collect()
}

#[test]
fn a_page_links_its_own_script_and_stylesheet_after_the_themes() {
    let html = site().html("notes/essay/index.html");
    let theme_css = html.find("/_moss/theme/style.").expect("theme stylesheet");
    let own_css = html.find("/notes/essay.css?v=").expect("page stylesheet");
    assert!(theme_css < own_css, "page stylesheet follows the theme's: {html}");
    assert!(html.contains(r#"/notes/essay.css?v="#) && html.contains(r#"" layer="themes">"#), "{html}");
    let theme_js = html.find("/_moss/theme/script.").expect("theme script");
    let own_js = html.find(r#"<script type="module" src="/notes/essay.js?v="#).expect("page script");
    assert!(theme_js < own_js, "page script follows the theme's: {html}");
}

#[test]
fn a_page_with_neither_file_links_neither() {
    let html = site().html("notes/plain/index.html");
    assert!(!html.contains("notes/plain.") && module_scripts(&html).is_empty(), "{html}");
    assert!(!html.contains("essay.css") && !html.contains("essay.js"), "no other page's files: {html}");
}

#[test]
fn a_page_with_only_a_stylesheet_links_only_that() {
    let html = site().html("notes/styled/index.html");
    assert!(html.contains("/notes/styled.css?v="), "{html}");
    assert!(module_scripts(&html).is_empty(), "{html}");
}

#[test]
fn a_folders_home_page_owns_the_index_files_beside_it() {
    let site = site();
    let notes = site.html("notes/index.html");
    assert!(notes.contains(r#"<script type="module" src="/notes/index.js?v="#), "{notes}");
    let root = site.html("index.html");
    assert!(root.contains("/index.css?v=") && !root.contains("/notes/index.js"), "{root}");
    assert!(!site.html("notes/essay/index.html").contains("notes/index.js"));
}

#[test]
fn a_language_edition_page_owns_the_files_in_its_own_folder() {
    let html = site().html("zh-hans/essay/index.html");
    assert!(html.contains(r#"<script type="module" src="/zh-hans/essay.js?v="#), "{html}");
    assert!(!html.contains("/notes/essay"), "{html}");
}

#[test]
fn the_href_is_the_files_own_served_address() {
    let site = site();
    let page = std::fs::read_dir(site.output("field-notes")).unwrap().flatten().find(|e| e.path().is_dir()).expect("the page's folder");
    let html = std::fs::read_to_string(page.path().join("index.html")).unwrap();
    assert!(html.contains(r#"src="/field-notes/Caf%C3%A9%20Log.js?v="#), "{html}");
}

#[test]
fn a_raw_html_page_is_untouched_and_its_script_is_still_served() {
    let site = site();
    assert_eq!(site.html("demo.html"), "<!doctype html><title>Demo</title><p>raw page</p>");
    assert_eq!(site.html("demo.js"), "console.log('demo')");
}

#[test]
fn the_files_are_served_byte_for_byte_at_their_own_address() {
    let site = site();
    assert_eq!(site.html("notes/essay.js"), "console.log('essay')");
    assert_eq!(site.html("notes/essay.css"), ".essay { color: blue }");
}

#[test]
fn a_sibling_that_appears_or_changes_is_picked_up_by_the_next_build() {
    let site = site();
    let version = |html: &str| html.split("/notes/essay.js?v=").nth(1).map(|r| r.split('"').next().unwrap().to_string());
    let before = version(&site.html("notes/essay/index.html")).expect("linked");

    site.write("notes/essay.js", "console.log('edited')");
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::ContentOnly(vec![site.root.join("notes/essay.js")])).unwrap();
    let after = version(&site.html("notes/essay/index.html")).expect("still linked");
    assert_ne!(before, after, "an edit moves the cache-busting version");

    site.write("notes/plain.css", ".plain { color: black }");
    site::build_sync(&site.root.to_string_lossy(), BuildTrigger::Structural(vec![site.root.join("notes/plain.css")])).unwrap();
    assert!(site.html("notes/plain/index.html").contains("/notes/plain.css?v="));
}
