use std::collections::HashMap;

use super::super::super::scope::UrlScope;
use super::super::facts::{Facts, Icon, IconKind, NavEntry};
use super::super::HomePage;
use super::*;

const BASE: &str = "https://studio.example/";

fn note(title: &str) -> String {
    format!("---\ntitle: \"{title}\"\norigin: \"{BASE}\"\n---\n\nBody\n")
}

fn put(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap()
}

fn entry(label: &str, path: &str) -> NavEntry {
    NavEntry { label: label.to_string(), url: format!("{BASE}{path}") }
}

/// A folder holding the home note, `about`, `work` and a leaf `news`, as a
/// crawl would have left it, and the map the crawl hands the chrome pass.
fn folder() -> (tempfile::TempDir, HashMap<String, String>) {
    let tmp = tempfile::tempdir().unwrap();
    let mut written = HashMap::new();
    for (path, rel, title) in [
        ("", "index.md", "Furniture"),
        ("about", "about.md", "About"),
        ("work", "work.md", "Work"),
        ("news", "news.md", "News"),
    ] {
        put(tmp.path(), rel, &note(title));
        written.insert(norm_url(&format!("{BASE}{path}")).unwrap(), rel.to_string());
    }
    (tmp, written)
}

async fn run(root: &Path, written: &HashMap<String, String>, facts: &Facts) -> ChromeSummary {
    let scope = UrlScope::new(BASE).unwrap();
    let home = HomePage { url: BASE.to_string(), html: String::new(), is_start: true };
    let capture = Capture { out_dir: root, scope: &scope, user_agent: "test", home: &home, written };
    apply(&capture, facts, &mut HostPacer::default()).await.unwrap()
}

fn facts() -> Facts {
    Facts {
        site_name: Some("Studio Example".to_string()),
        nav: vec![entry("Home", ""), entry("About", "about"), entry("Work", "work")],
        footer: Some("(c) Studio Example\n".to_string()),
        ..Default::default()
    }
}

#[tokio::test]
async fn nav_goes_on_the_linked_pages_in_order_and_the_home_note_and_leaves_are_not_nav() {
    let (tmp, written) = folder();
    let summary = run(tmp.path(), &written, &facts()).await;

    assert_eq!((summary.nav, summary.nav_items), (ChromePart::Written, 2));
    assert!(read(tmp.path(), "about.md").contains("origin: \"https://studio.example/\"\nnav: true\nweight: 1\n---\n"));
    assert!(read(tmp.path(), "work.md").contains("nav: true\nweight: 2\n---\n"));
    assert!(!read(tmp.path(), "index.md").contains("nav:"), "the home note is not a nav item");
    assert_eq!(read(tmp.path(), "news.md"), note("News"), "a page the nav does not link is untouched");
}

#[tokio::test]
async fn a_second_pass_changes_nothing_and_an_authors_nav_choice_stands() {
    let (tmp, written) = folder();
    put(tmp.path(), "work.md", "---\ntitle: \"Work\"\nnav: false\n---\n\nBody\n");
    run(tmp.path(), &written, &facts()).await;
    let after_first: Vec<String> =
        ["index.md", "about.md", "work.md", "footer.md"].iter().map(|f| read(tmp.path(), f)).collect();
    assert_eq!(read(tmp.path(), "work.md"), "---\ntitle: \"Work\"\nnav: false\n---\n\nBody\n");

    let again = run(tmp.path(), &written, &facts()).await;
    let after_second: Vec<String> =
        ["index.md", "about.md", "work.md", "footer.md"].iter().map(|f| read(tmp.path(), f)).collect();
    assert_eq!(after_first, after_second);
    assert_eq!(again.nav, ChromePart::LeftUnchanged);
    assert_eq!(again.footer, ChromePart::LeftUnchanged);
}

#[tokio::test]
async fn one_linked_page_is_not_a_nav_bar() {
    let (tmp, written) = folder();
    let mut f = facts();
    f.nav = vec![entry("About", "about"), entry("Elsewhere", "not-written")];
    let summary = run(tmp.path(), &written, &f).await;
    assert_eq!(summary.nav, ChromePart::NotFound);
    assert_eq!(read(tmp.path(), "about.md"), note("About"));
}

#[tokio::test]
async fn the_site_name_becomes_the_title_and_the_old_title_the_description_only_when_absent() {
    let (tmp, written) = folder();
    run(tmp.path(), &written, &facts()).await;
    assert_eq!(
        read(tmp.path(), "index.md"),
        "---\ntitle: \"Studio Example\"\norigin: \"https://studio.example/\"\ndescription: \"Furniture\"\n---\n\nBody\n"
    );

    let (tmp, written) = folder();
    put(tmp.path(), "index.md", "---\ntitle: \"Furniture\"\ndescription: \"Mine\"\n---\n\nBody\n");
    run(tmp.path(), &written, &facts()).await;
    assert_eq!(
        read(tmp.path(), "index.md"),
        "---\ntitle: \"Studio Example\"\ndescription: \"Mine\"\n---\n\nBody\n"
    );
}

#[tokio::test]
async fn a_generic_old_title_is_not_moved_into_the_description() {
    for old in ["Welcome", "Home | Studio Example"] {
        let (tmp, written) = folder();
        put(tmp.path(), "index.md", &note(old));
        run(tmp.path(), &written, &facts()).await;
        let home = read(tmp.path(), "index.md");
        assert!(home.contains("title: \"Studio Example\""), "{home}");
        assert!(!home.contains("description:"), "{old}: {home}");
    }
}

#[tokio::test]
async fn a_page_with_its_own_weight_is_left_byte_identical_and_the_order_stays_dense() {
    let (tmp, written) = folder();
    let mine = "---\ntitle: \"About\"\nweight: 9\n---\n\nBody\n";
    put(tmp.path(), "about.md", mine);
    let mut f = facts();
    f.nav.push(entry("News", "news"));
    run(tmp.path(), &written, &f).await;
    assert_eq!(read(tmp.path(), "about.md"), mine);
    assert!(read(tmp.path(), "work.md").contains("nav: true\nweight: 1\n"));
    assert!(read(tmp.path(), "news.md").contains("nav: true\nweight: 2\n"));
}

#[tokio::test]
async fn an_existing_footer_and_favicon_are_left_alone() {
    let (tmp, written) = folder();
    put(tmp.path(), "footer.md", "mine\n");
    fs::create_dir_all(tmp.path().join("assets")).unwrap();
    fs::write(tmp.path().join("assets/favicon.png"), b"mine").unwrap();
    let mut f = facts();
    f.favicon = Some(Icon { url: format!("{BASE}f.svg"), kind: IconKind::Svg });

    let summary = run(tmp.path(), &written, &f).await;

    assert_eq!(read(tmp.path(), "footer.md"), "mine\n");
    assert_eq!(fs::read(tmp.path().join("assets/favicon.png")).unwrap(), b"mine");
    assert!(!tmp.path().join("assets/favicon.svg").exists());
    assert_eq!((summary.footer, summary.favicon), (ChromePart::LeftUnchanged, ChromePart::LeftUnchanged));
}

#[tokio::test]
async fn footer_links_to_written_pages_become_note_links() {
    let (tmp, written) = folder();
    let mut f = facts();
    f.footer = Some(format!("[About us]({BASE}about)\n"));
    run(tmp.path(), &written, &f).await;
    assert_eq!(read(tmp.path(), "footer.md"), "[About us](./about.md)\n");
}

#[test]
fn the_icon_kind_comes_from_the_bytes_not_the_link() {
    assert_eq!(sniff(b"<?xml version='1.0'?>\n<!-- c --><svg xmlns='x'/>"), Some(IconKind::Svg));
    assert_eq!(sniff("\u{FEFF}  <svg/>".as_bytes()), Some(IconKind::Svg));
    assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 13]), Some(IconKind::Png));
    assert_eq!(sniff(&[0, 0, 1, 0, 1]), Some(IconKind::Ico));
    assert_eq!(sniff(b"<html><body><svg></svg></body></html>"), None);
    assert_eq!(sniff(b"<html>Not found</html>"), None);
    let late = format!("{}<svg/>", " ".repeat(2000));
    assert_eq!(sniff(late.as_bytes()), None, "beyond the first KB is not a document start");
}
