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
    let removed = removed_addresses(&baseline, now.site_hashes_view(), |src| dir.join(src).exists());
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

/// The folder as an app build sees it after `attachment_folder = "assets"`:
/// moss writes no forwarding pages under assets/ any more. `docs/` and `hi/`
/// hold only files.
fn attachment_site(extra: &[(&'static str, &'static str)]) -> (std::path::PathBuf, impl Drop, crate::build::manifest::SealedManifest) {
    let mut files = STORAGE.to_vec();
    files.extend([("docs/guide.pdf", "pdf"), ("hi/card.pdf", "pdf")]);
    files.extend_from_slice(extra);
    let (dir, cleanup) = site(&files);
    fs::create_dir_all(dir.join(".moss")).unwrap();
    fs::write(dir.join(".moss/config.toml"), "[editor]\nattachment_folder = \"assets\"\n").unwrap();
    let (_, _, _, now) = build_test_promoted_or_withheld(dir.to_str().unwrap()).unwrap();
    assert!(!now.files().contains_key("assets/imported/index.html"), "premise: no forwarding page is built");
    (dir, cleanup, now)
}

fn removed_paths(baseline: &crate::build::manifest::change_set::PublishedSnapshot, dir: &std::path::Path, now: &crate::build::manifest::SealedManifest) -> Vec<String> {
    use crate::build::manifest::change_set::removed_addresses;
    removed_addresses(baseline, now.site_hashes_view(), |src| dir.join(src).exists()).into_iter().map(|r| r.path).collect()
}

#[test]
fn after_a_prebuilt_publish_only_pages_are_named_not_the_assets_forwards() {
    use crate::build::manifest::change_set::{LiveEntry, PublishedSnapshot};
    let (dir, _cleanup, now) = attachment_site(&[("assets/drafts/plan.md", "---\ntitle: Plan\ndraft: true\n---\n\nSoon.\n")]);
    // A prebuilt upload's record: the tree it sent, no sources.
    let files = ["index.html", "assets/index.html", "assets/imported/index.html", "assets/drafts/plan/index.html", "docs/index.html", "gone/index.html", "old.html"];
    let prebuilt = |triples: Vec<LiveEntry>| PublishedSnapshot {
        target: "test".into(),
        files: files.iter().map(|f| (f.to_string(), format!("100644:{f}"))).collect(),
        triples: Some(triples),
        ..Default::default()
    };
    assert!(now.files().contains_key("docs/guide.pdf") && !now.files().contains_key("docs/index.html"), "premise: docs/ is served with no page");
    let named = ["assets/drafts/plan/index.html", "docs/index.html", "gone/index.html", "old.html"];
    assert_eq!(removed_paths(&prebuilt(Vec::new()), &dir, &now), named);
    // A page the record names under assets/ is still a page.
    let imported = LiveEntry { uid: "a1".into(), url: "assets/imported/".into(), source_path: "assets/imported/imported.md".into(), title: "Imported".into() };
    assert!(removed_paths(&prebuilt(vec![imported]), &dir, &now).contains(&"assets/imported/index.html".to_string()));
}

#[test]
fn a_recorded_forward_from_an_assets_folder_is_not_a_page_going_offline() {
    use crate::build::manifest::change_set::PublishedSnapshot;
    let (dir, _cleanup, now) = attachment_site(&[]);
    assert!(!now.files().contains_key("assets/index.html"), "premise: no forwarding page is built");
    let mut last = PendingManifest::new(SiteHashes::default());
    for out in ["index.html", "notes/hello/index.html", "assets/index.html", "assets/about/index.html", "hi/index.html"] {
        last.register(&ServedPath::from_source(out).unwrap(), b"forward", crate::build::manifest::HashBucket::Files);
    }
    last.register_page_stub("assets/index.html", "index.html");
    last.register_page_stub("hi/index.html", "notes/hello/index.html");
    let mut baseline = PublishedSnapshot::from_sealed(&last.seal(), "test", "2026-01-01T00:00:00Z".into());
    // A page the author wrote under assets/ and has since deleted.
    baseline.source_to_output.insert("assets/about.md".into(), "assets/about/index.html".into());
    assert!(!now.files().contains_key("hi/index.html"), "premise: hi/ is served with no page");
    assert_eq!(removed_paths(&baseline, &dir, &now), ["assets/about/index.html", "hi/index.html"], "pages and their old addresses still count");
}

#[test]
fn an_old_address_of_a_page_under_assets_still_counts() {
    use crate::build::manifest::change_set::PublishedSnapshot;
    let (dir, _cleanup, now) = attachment_site(&[("assets/notes/x.md", "---\ntitle: X\n---\n\nHi.\n")]);
    assert!(now.files().contains_key("assets/notes/x/index.html"), "premise: the page is served");
    let mut last = PendingManifest::new(SiteHashes::default());
    for out in ["index.html", "assets/notes/x/index.html", "assets/notes/old-x/index.html"] {
        last.register(&ServedPath::from_source(out).unwrap(), b"page", crate::build::manifest::HashBucket::Files);
    }
    last.register_page_stub("assets/notes/old-x/index.html", "assets/notes/x/index.html");
    let mut baseline = PublishedSnapshot::from_sealed(&last.seal(), "test", "2026-01-01T00:00:00Z".into());
    baseline.source_to_output.insert("assets/notes/x.md".into(), "assets/notes/x/index.html".into());
    assert_eq!(removed_paths(&baseline, &dir, &now), ["assets/notes/old-x/index.html"]);
}
