//! Pins an invariant: the build never takes the editor's no-map answer.
//!
//! The editor resolves a link on a site with no URL map to the page's source
//! file, so it can open it before any build has run. The build resolves links
//! on its own, so on a site's first build a link to a page that has never been
//! built still gets the page's URL, never a path to its `.md` source.

use moss_build::moss_paths::MossPaths;
use std::fs;
use std::path::{Path, PathBuf};

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_first_build_links_fresh_pages_by_url_not_by_source_file() {
    let tmp = std::env::temp_dir().join(format!("moss_unbuilt_link_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&tmp, "index.md", "---\ntitle: Harbor Notes\n---\n\nSee [[Fresh]], [[Fresh#Tides]], [[notes/Deep]] and [the log](notes/Deep.md).\n");
    write(&tmp, "Fresh.md", "---\ntitle: Fresh\n---\n\n## Tides\n\nHigh at noon.\n");
    write(&tmp, "notes/Deep.md", "---\ntitle: Deep\n---\n\nBelow.\n");
    assert!(!MossPaths::new(&tmp).build_dir().exists(), "the site has never been built");
    let result = crate::site::build_full(&tmp.to_string_lossy());
    assert!(result.is_ok(), "build failed: {result:?}");

    let html = fs::read_to_string(MossPaths::new(&tmp).staging_dir().join("index.html")).unwrap();
    let doc = scraper::Html::parse_document(&html);
    let sel = scraper::Selector::parse("main a[href]").unwrap();
    let hrefs: Vec<&str> = doc.select(&sel).filter_map(|a| a.value().attr("href")).collect();
    assert!(hrefs.len() >= 4, "every link is drawn: {hrefs:?}\n{html}");
    for href in &hrefs {
        let path = href.split('#').next().unwrap();
        assert!(!path.to_lowercase().ends_with(".md"), "a link names a source file: {href} in {hrefs:?}");
    }
    assert!(hrefs.iter().any(|h| h.ends_with("#tides")), "the anchor survives: {hrefs:?}");
}
