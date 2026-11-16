//! A site's `404.md` is the page a host shows for an unknown address. Static
//! hosts look for `/404.html` at the root, so the build writes the page there
//! as well as at `/404/`, and neither address is part of the site's content:
//! the sitemap, feed and `llms.txt` leave it out. Built through the real CLI.

use std::path::Path;
use std::process::{Command, Stdio};

const MARK: &str = "fog-gate-marker";

fn write(site: &Path, rel: &str, body: &str) {
    let p = site.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn build(site: &Path, moss_home: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["build", site.to_str().unwrap(), "--no-plugins", "--strict"])
        .env("MOSS_HOME", moss_home)
        .env("MOSS_SITE_URL", "https://example.com")
        .stdin(Stdio::null())
        .output()
        .expect("run moss-cli");
    assert!(out.status.success(), "build must be clean; stderr:\n{}", String::from_utf8_lossy(&out.stderr));
}

fn site_with_404(tmp: &Path) {
    write(tmp, "index.md", "---\ntitle: Home\n---\n\nWelcome.\n");
    write(tmp, "about.md", "---\ntitle: About\n---\n\nAbout us.\n");
    write(tmp, "404.md", &format!("---\ntitle: Not here\ndate: 2026-01-02\n---\n\nNothing past this {MARK}.\n"));
}

#[test]
fn the_not_found_page_is_served_at_404_html_and_stays_out_of_the_listings() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    site_with_404(&site);
    build(&site, moss_home.path());

    let built = site.join(".moss/build.nosync/current");
    let root_copy = std::fs::read_to_string(built.join("404.html")).expect("/404.html is written");
    let page = std::fs::read_to_string(built.join("404/index.html")).expect("/404/ keeps resolving");
    assert!(root_copy.contains(MARK) && page.contains(MARK));
    assert_eq!(root_copy, page, "the two addresses serve the same page");
    let noindex = "<meta name=\"robots\" content=\"noindex\">";
    assert!(page.contains(noindex), "a crawler must not index the not-found page");
    let about = std::fs::read_to_string(built.join("about/index.html")).unwrap();
    assert!(!about.contains(noindex), "real pages stay indexable");

    let sitemap = std::fs::read_to_string(built.join("sitemap.xml")).unwrap();
    assert!(sitemap.contains("https://example.com/about/"), "real pages stay listed: {sitemap}");
    assert!(!sitemap.contains("404"), "the sitemap must not list the not-found page: {sitemap}");
    for listing in ["rss.xml", "llms.txt"] {
        let text = std::fs::read_to_string(built.join(listing)).unwrap();
        assert!(!text.contains(MARK) && !text.contains("Not here"), "{listing} must not carry the not-found page:\n{text}");
    }

    // Removing the source takes both addresses away; neither lingers in the stage.
    std::fs::remove_file(site.join("404.md")).unwrap();
    build(&site, moss_home.path());
    assert!(!built.join("404.html").exists(), "a stale /404.html must not outlive its source");
    assert!(!built.join("404/index.html").exists());
}

#[test]
fn an_authors_own_404_html_is_not_overwritten() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    site_with_404(&site);
    write(&site, "404.html", "<!doctype html><title>mine</title><p>hand-written</p>");
    build(&site, moss_home.path());

    let built = site.join(".moss/build.nosync/current");
    let root_copy = std::fs::read_to_string(built.join("404.html")).unwrap();
    assert!(root_copy.contains("hand-written") && !root_copy.contains(MARK), "{root_copy}");
    assert!(std::fs::read_to_string(built.join("404/index.html")).unwrap().contains(MARK));
}

#[test]
fn an_own_404_html_added_after_a_build_wins_and_removing_it_brings_back_the_copy() {
    let moss_home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(tmp.path()).unwrap();
    site_with_404(&site);
    build(&site, moss_home.path());
    let built = site.join(".moss/build.nosync/current");
    assert!(std::fs::read_to_string(built.join("404.html")).unwrap().contains(MARK));

    write(&site, "404.html", "<!doctype html><title>mine</title><p>hand-written</p>");
    build(&site, moss_home.path());
    let root_copy = std::fs::read_to_string(built.join("404.html")).unwrap();
    assert!(root_copy.contains("hand-written") && !root_copy.contains(MARK), "{root_copy}");

    std::fs::remove_file(site.join("404.html")).unwrap();
    build(&site, moss_home.path());
    assert!(std::fs::read_to_string(built.join("404.html")).unwrap().contains(MARK), "the generated copy comes back");
}
