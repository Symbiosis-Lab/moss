//! A file whose name a sync client stored decomposed (`e` plus a combining
//! accent) is published under the composed name, so every machine serves it
//! at one address.

#[path = "support/site.rs"]
mod site;

use site::Site;

#[test]
fn a_decomposed_file_name_is_published_composed() {
    let site = Site::new(&[
        ("index.md", "---\ntitle: Example Hall\n---\n\n[Prices](files/cr\u{e8}me.csv)\n"),
        ("files/cre\u{300}me.csv", "item,price\ntart,4\n"),
    ]);
    // The published folder, and its listing rather than a lookup: a Mac disk
    // finds either spelling.
    let published = site.root.join(".moss/build.nosync/current/files");
    let mut names: Vec<String> = std::fs::read_dir(&published)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["cr\u{e8}me.csv".to_string(), "index.html".to_string()]);
    assert_eq!(std::fs::read_to_string(published.join("cr\u{e8}me.csv")).unwrap(), "item,price\ntart,4\n");
}

/// A folder, a page and a file inside it, written in the spelling `form` gives.
fn hall(form: fn(&str) -> String) -> Site {
    let files = [
        ("index.md".to_string(), "---\ntitle: Example Hall\n---\n\n[Menu](caf\u{e9}/cr\u{e8}me.md)\n".to_string()),
        (form("caf\u{e9}/cr\u{e8}me.md"), "---\ntitle: Cr\u{e8}me\n---\n\n[Prices](th\u{e9}.csv)\n".to_string()),
        (form("caf\u{e9}/th\u{e9}.csv"), "item,price\ntart,4\n".to_string()),
    ];
    let files: Vec<(&str, &str)> = files.iter().map(|(rel, body)| (rel.as_str(), body.as_str())).collect();
    Site::with_config("title = \"Example Hall\"", &files)
}

/// Every published file by the name its folder lists, with its bytes.
fn published(site: &Site) -> Vec<(String, String)> {
    let current = site.root.join(".moss/build.nosync/current");
    let mut files: Vec<(String, String)> = walkdir::WalkDir::new(&current)
        .follow_links(true)
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().is_file())
        .map(|e| (e.path().strip_prefix(&current).unwrap().to_string_lossy().into_owned(), String::from_utf8_lossy(&std::fs::read(e.path()).unwrap()).into_owned()))
        .collect();
    files.sort();
    files
}

#[test]
fn a_folder_written_decomposed_builds_publishes_and_edits_as_the_composed_folder() {
    use moss_build::build::manifest::{change_set::PublishedSnapshot, published_record};
    use moss_build::moss_paths::MossPaths;
    use unicode_normalization::UnicodeNormalization;

    let composed = hall(|rel| rel.to_string());
    let decomposed = hall(|rel| rel.nfd().collect());
    assert_eq!(published(&decomposed), published(&composed));

    // The last publish served the composed folder's addresses.
    let last = moss_build::build::load_previous_hashes(&composed.root.to_string_lossy());
    let snapshot = PublishedSnapshot {
        generation_id: "last".into(),
        target: "moss:example-hall".into(),
        published_at: "2026-11-01T00:00:00+00:00".into(),
        files: last.files,
        asset_source_to_output: Some(Default::default()),
        ..Default::default()
    };
    let paths = MossPaths::new(&decomposed.root);
    published_record::save(&paths, &snapshot).unwrap();
    decomposed.rebuild();
    let removals = moss_build::system::build_records::records().pending_removals(&decomposed.root.to_string_lossy());
    assert!(removals.is_empty(), "the decomposed build serves every address the composed one did: {removals:?}");

    let page = "caf\u{e9}/cr\u{e8}me.md";
    let url = moss_build::editor::resolve::links::resolve_url_for_file_inner(page, &decomposed.root, &paths.initial_serve_dir())
        .unwrap()
        .expect("the editor finds the page's address");
    let source = moss_build::editor::page_source::resolve_page_source(&url, &decomposed.root).unwrap();
    assert!(source.source_path.is_some(), "the editor finds the source of {url}");

    // A page a cloud folder has not downloaded keeps its last build.
    #[cfg(target_os = "macos")]
    {
        let html = || std::fs::read(decomposed.root.join(".moss/build.nosync/current").join(url.trim_matches('/')).join("index.html")).unwrap();
        let before = html();
        let folder = decomposed.root.join("cafe\u{301}");
        std::fs::remove_file(folder.join("cre\u{300}me.md")).unwrap();
        std::fs::write(folder.join(".cre\u{300}me.md.icloud"), "").unwrap();
        decomposed.rebuild();
        assert_eq!(html(), before);
    }
}
