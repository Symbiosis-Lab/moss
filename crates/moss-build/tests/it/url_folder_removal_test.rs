//! The publish record holds the address each folder file was published at, so
//! a later build can tell what the author removed: an HTML page deleted at its
//! own path or from a folder whose page sets `url:`, a deleted notebook, and
//! a WebP picture whose old sizes keep answering while it stays. A notebook is
//! served under its folder's address, and one published elsewhere before
//! keeps its old addresses.

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
    assert!(last.files.contains_key("lab-notes/analysis.html"), "{:?}", last.files.keys());
    published_record::save(&MossPaths::new(&site.root), &last).unwrap();

    std::fs::remove_file(site.root.join("Lab Notes/analysis.ipynb")).unwrap();
    site.rebuild();

    assert_eq!(reasons_for(&site, "lab-notes/analysis.html"), [RemovalReason::AuthorRemoved]);
}

#[test]
fn a_notebook_is_served_under_its_folders_address_where_the_page_links_it() {
    for (folder_page, folder) in [("---\ntitle: Lab Notes\n---\n", "lab-notes"), ("---\ntitle: Lab Notes\nurl: lab\n---\n", "lab")] {
        let site = Site::new(&[
            ("index.md", "---\ntitle: Example Hall\n---\n\n[Analysis](Lab%20Notes/analysis.ipynb)\n"),
            ("Lab Notes/index.md", folder_page),
            ("Lab Notes/analysis.ipynb", NOTEBOOK),
        ]);
        let last = last_publish(&site);
        for file in ["index.html", "analysis.ipynb", "analysis.html"] {
            let address = format!("{folder}/{file}");
            assert!(last.files.contains_key(&address), "{address} in {:?}", last.files.keys());
        }
        let home = std::fs::read_to_string(MossPaths::new(&site.root).current_ptr().join("index.html")).unwrap();
        let href = home.split("href=\"").filter_map(|h| h.split('"').next()).find(|h| h.contains("analysis")).expect("the home page links the notebook");
        let target = href.trim_start_matches('/');
        assert!(last.files.contains_key(target), "the link {href} reaches nothing served: {:?}", last.files.keys());
    }
}

#[test]
fn a_notebook_published_at_its_old_folder_address_keeps_both_old_addresses() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\n[Analysis](Lab%20Notes/analysis.ipynb)\n"),
        ("Lab Notes/analysis.ipynb", NOTEBOOK),
    ]);
    let mut last = last_publish(&site);
    let assets = last.asset_source_to_output.as_mut().unwrap();
    assert_eq!(assets.get("Lab Notes/analysis.ipynb").map(String::as_str), Some("lab-notes/analysis.ipynb"));
    assets.insert("Lab Notes/analysis.ipynb".into(), "lab notes/analysis.ipynb".into());
    for (old, new) in [("lab notes/analysis.ipynb", "lab-notes/analysis.ipynb"), ("lab notes/analysis.html", "lab-notes/analysis.html")] {
        let entry = last.files.remove(new).unwrap();
        last.files.insert(old.into(), entry);
    }
    published_record::save(&MossPaths::new(&site.root), &last).unwrap();

    site.rebuild();

    assert_eq!(reasons_for(&site, "lab notes/analysis.html"), []);
    let live = MossPaths::new(&site.root).current_ptr();
    assert_eq!(std::fs::read_to_string(live.join("lab notes/analysis.ipynb")).unwrap(), NOTEBOOK);
    let forward = std::fs::read_to_string(live.join("lab notes/analysis.html")).unwrap();
    assert!(forward.contains("lab-notes/analysis.html"), "{forward}");
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
    assert!(MossPaths::new(&site.root).current_ptr().join("shows/day.w9999.webp").is_file());
}
