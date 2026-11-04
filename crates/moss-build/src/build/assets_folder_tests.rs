//! The site-root `assets/` folder stores files: it publishes no folder page,
//! and its old listing addresses forward home so a published site keeps them.

use super::*;
use crate::build::manifest::PendingManifest;
use crate::build::served_path::ServedPath;
use crate::types::content::SiteHashes;

fn site(files: &[(&str, &str)]) -> (std::path::PathBuf, impl Drop) {
    let (dir, cleanup) = create_test_dir();
    fs::write(dir.join("index.md"), "---\ntitle: Example Garden\n---\n\n# Example Garden\n").unwrap();
    for (path, body) in files {
        let at = dir.join(path);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, body).unwrap();
    }
    (dir, cleanup)
}

fn staged(dir: &std::path::Path, rel: &str) -> String {
    fs::read_to_string(crate::moss_paths::MossPaths::new(dir).staging_dir().join(rel)).unwrap_or_default()
}

const STORAGE: [(&str, &str); 4] = [
    ("assets/favicon.svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
    ("assets/imported/a.jpg", "jpg"),
    ("assets/imported/b.pdf", "pdf"),
    ("notes/hello.md", "---\ntitle: Hello\n---\n\nHi.\n"),
];

#[test]
fn assets_folders_publish_forwarding_pages_not_listings() {
    let (dir, _cleanup) = site(&STORAGE);
    let keys = build_test_sealed_at(dir.to_str().unwrap(), Some("https://example.test")).unwrap();
    for folder in ["assets/index.html", "assets/imported/index.html"] {
        let html = staged(&dir, folder);
        assert!(html.contains("http-equiv=\"refresh\"") && html.contains("url=/\""), "{folder} forwards home: {html}");
        assert!(!html.contains("assets –"), "{folder} is no listing: {html}");
    }
    assert!(keys.iter().any(|k| k == "assets/favicon.svg"), "{keys:?}");
    for served in ["assets/favicon.svg", "assets/imported/b.pdf"] {
        assert!(crate::moss_paths::MossPaths::new(&dir).staging_dir().join(served).is_file(), "{served} still served");
    }
    for feed in ["sitemap.xml", "llms.txt"] {
        let body = staged(&dir, feed);
        assert!(body.contains("Hello") || body.contains("notes/hello"), "premise: {feed} lists pages: {body}");
        assert!(!body.contains("/assets/") && !body.contains("## assets"), "{feed} lists no storage folder: {body}");
    }
    assert!(staged(&dir, "notes/index.html").contains("Hello"), "an ordinary folder keeps its listing");
}

#[test]
fn a_page_under_assets_keeps_the_folder_page() {
    let (dir, _cleanup) = site(&[("assets/favicon.svg", "<svg/>"), ("assets/imported/a.jpg", "jpg"), ("assets/about.md", "---\ntitle: About\n---\n\nHi.\n")]);
    build_test_sealed_at(dir.to_str().unwrap(), Some("https://example.test")).unwrap();
    let listing = staged(&dir, "assets/index.html");
    assert!(!listing.contains("http-equiv=\"refresh\"") && listing.contains("About"), "{listing}");
    let imported = staged(&dir, "assets/imported/index.html");
    assert!(imported.contains("http-equiv=\"refresh\""), "no page beneath imported/: {imported}");
    assert!(staged(&dir, "sitemap.xml").contains("/assets/"), "the kept folder page is listed");
}

#[test]
fn an_address_the_last_publish_served_under_assets_is_not_a_pending_removal() {
    use crate::build::manifest::change_set::{pending_removals, removed_addresses, PublishedSnapshot};
    let (dir, _cleanup) = site(&STORAGE);
    let (_, _, _, now) = build_test_promoted_or_withheld(dir.to_str().unwrap()).unwrap();
    let mut last = PendingManifest::new(SiteHashes::default());
    for out in ["index.html", "assets/index.html", "assets/imported/index.html"] {
        last.register(&ServedPath::from_source(out).unwrap(), b"listing", crate::build::manifest::HashBucket::Files);
    }
    let baseline = PublishedSnapshot::from_sealed(&last.seal(), "test", "2026-01-01T00:00:00Z".into());
    let removed = removed_addresses(&baseline, &now, |src| dir.join(src).exists());
    assert!(pending_removals(&removed, None).is_empty(), "{removed:?}");
}

#[test]
fn an_attachment_folder_named_assets_gets_no_forwarding_pages() {
    for setting in ["./assets", "assets"] {
        let (dir, _cleanup) = site(&STORAGE);
        fs::create_dir_all(dir.join(".moss")).unwrap();
        fs::write(dir.join(".moss/config.toml"), format!("[editor]\nattachment_folder = \"{setting}\"\n")).unwrap();
        build_test_sealed_at(dir.to_str().unwrap(), Some("https://example.test")).unwrap();
        for folder in ["assets/index.html", "assets/imported/index.html"] {
            assert!(!staged(&dir, folder).contains("http-equiv=\"refresh\""), "{setting}: {folder} had no page before and gets no stub");
        }
    }
}

#[test]
fn a_hand_made_assets_index_is_served_as_written() {
    let (dir, _cleanup) = site(&[("assets/index.html", "<html><body>Hand made</body></html>"), ("assets/a.jpg", "jpg")]);
    build_test_sealed_at(dir.to_str().unwrap(), Some("https://example.test")).unwrap();
    let served = staged(&dir, "assets/index.html");
    assert!(served.contains("Hand made") && !served.contains("http-equiv=\"refresh\""), "{served}");
}
