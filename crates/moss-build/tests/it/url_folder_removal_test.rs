//! The publish record holds the address each folder file was published at, so
//! a later build can tell what the author removed: an HTML page deleted at its
//! own path or from a folder whose page sets `url:`, a deleted notebook in a
//! folder whose name is served lowercased, and a WebP picture whose old sizes
//! keep answering while it stays.

use crate::site;

use moss_build::build::manifest::change_set::{PublishedSnapshot, RemovalReason};
use moss_build::build::manifest::published_record;
use moss_build::moss_paths::MossPaths;
use moss_build::system::build_records::records;
use moss_build::vault_root::VaultRoot;
use site::Site;

const NOTEBOOK: &str = r#"{"cells":[{"cell_type":"markdown","metadata":{},"source":["Notebook prose."]}],"metadata":{},"nbformat":4,"nbformat_minor":5}"#;

/// The last publish, recorded as a publish records it, after an unchanged
/// rebuild: every file is reused from the last build, and its address kept.
fn last_publish(site: &Site) -> PublishedSnapshot {
    site.rebuild();
    let sealed = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(moss_build::deploy::one_shot::build_sealed_now(&VaultRoot::resolve(&site.root)))
        .unwrap();
    PublishedSnapshot::from_sealed(&sealed, "moss:example-hall", "2026-11-01T00:00:00+00:00".into())
}

fn reasons_for(site: &Site, path: &str) -> Vec<RemovalReason> {
    let folder = site.root.to_string_lossy();
    let refused: Vec<String> = records().pending_removals(&folder).into_iter().map(|r| r.path).collect();
    assert_eq!(refused, Vec::<String>::new());
    let removed = records().removed_addresses(&folder).expect("the build compared itself with the last publish");
    removed.iter().filter(|r| r.path == path).map(|r| r.reason).collect()
}

#[test]
fn an_html_page_deleted_from_a_folder_with_its_own_address_is_the_authors_removal() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\n[Shows](Shows-src/index.md)\n"),
        ("Shows-src/index.md", "---\ntitle: Shows\nurl: shows\n---\n\nThe season.\n"),
        ("Shows-src/program.html", "<!doctype html><title>Program</title><p>Tonight</p>\n"),
    ]);
    let last = last_publish(&site);
    assert!(last.files.contains_key("shows/program.html"), "the folder's files are served under its address: {:?}", last.files.keys());
    published_record::save(&MossPaths::new(&site.root), &last).unwrap();

    std::fs::remove_file(site.root.join("Shows-src/program.html")).unwrap();
    site.rebuild();

    assert_eq!(reasons_for(&site, "shows/program.html"), [RemovalReason::AuthorRemoved]);
}

#[test]
fn an_html_page_deleted_after_its_folder_drops_its_own_address_is_the_authors_removal() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\n[Shows](shows/index.md)\n"),
        ("shows/index.md", "---\ntitle: Shows\nurl: events\n---\n\nThe season.\n"),
        ("shows/program.html", "<!doctype html><title>Program</title><p>Tonight</p>\n"),
    ]);
    site.write("shows/index.md", "---\ntitle: Shows\n---\n\nThe season.\n");
    let last = last_publish(&site);
    assert!(last.files.contains_key("shows/program.html"), "{:?}", last.files.keys());
    published_record::save(&MossPaths::new(&site.root), &last).unwrap();

    std::fs::remove_file(site.root.join("shows/program.html")).unwrap();
    site.rebuild();

    assert_eq!(reasons_for(&site, "shows/program.html"), [RemovalReason::AuthorRemoved]);
}

#[test]
fn a_deleted_notebook_is_named_by_its_viewer_page() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\n[Analysis](Lab%20Notes/analysis.ipynb)\n"),
        ("Lab Notes/analysis.ipynb", NOTEBOOK),
    ]);
    let last = last_publish(&site);
    assert!(last.files.contains_key("lab notes/analysis.html"), "{:?}", last.files.keys());
    published_record::save(&MossPaths::new(&site.root), &last).unwrap();

    std::fs::remove_file(site.root.join("Lab Notes/analysis.ipynb")).unwrap();
    site.rebuild();

    assert_eq!(reasons_for(&site, "lab notes/analysis.html"), [RemovalReason::AuthorRemoved]);
}

#[test]
fn an_html_page_deleted_from_its_own_path_is_the_authors_removal() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\n[Guide](guide.html)\n"),
        ("guide.html", "<!doctype html><title>Guide</title><p>How to find us</p>\n"),
    ]);
    let last = last_publish(&site);
    assert!(last.files.contains_key("guide.html"), "{:?}", last.files.keys());
    published_record::save(&MossPaths::new(&site.root), &last).unwrap();

    std::fs::remove_file(site.root.join("guide.html")).unwrap();
    site.rebuild();

    assert_eq!(reasons_for(&site, "guide.html"), [RemovalReason::AuthorRemoved]);
}

#[test]
fn a_size_no_longer_made_of_a_webp_picture_in_a_folder_with_its_own_address_keeps_answering() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\n[Shows](Shows-src/index.md)\n"),
        ("Shows-src/index.md", "---\ntitle: Shows\nurl: shows\n---\n\n![Day](day.webp)\n"),
    ]);
    image::RgbImage::from_fn(1200, 800, |x, y| image::Rgb([40, (x % 200) as u8, (y % 200) as u8]))
        .save(site.root.join("Shows-src/day.webp"))
        .unwrap();
    let mut last = last_publish(&site);
    assert!(last.files.contains_key("shows/day.webp"), "{:?}", last.files.keys());
    last.files.insert("shows/day.w9999.webp".into(), "100644:0000000000000000".into());
    published_record::save(&MossPaths::new(&site.root), &last).unwrap();

    site.rebuild();

    assert_eq!(reasons_for(&site, "shows/day.w9999.webp"), []);
    assert!(site.root.join(".moss/build.nosync/current/shows/day.w9999.webp").is_file());
}
