//! The redirect table end to end: every source and every static form. Each
//! case builds a real folder through the blocking render, the same entry the preview and the
//! snapshot suite use, so what is asserted is what a site would serve.

use crate::build::cli_output::take_cli_problems;
use crate::build::render::blocking::SiteConfig;
use std::fs;
use std::path::PathBuf;

struct Built {
    _dir: tempfile::TempDir,
    out: PathBuf,
    /// `--strict`-visible advisories this build raised.
    problems: usize,
}

impl Built {
    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.out.join(rel)).unwrap_or_else(|_| panic!("{rel} should exist"))
    }
    fn has(&self, rel: &str) -> bool {
        self.out.join(rel).exists()
    }
}

const HOME: &str = "---\ntitle: Home\n---\n\n# Home\n";

fn write_all(dir: &std::path::Path, files: &[(&str, &str)]) {
    for (rel, body) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
}

/// Build `files` (path, contents) as a deployed site, plus a home page.
fn build(files: &[(&str, &str)]) -> Built {
    // Not `tempdir()`: its `.tmpXXXX` name is hidden, and the scan skips hidden folders.
    let dir = tempfile::Builder::new().prefix("moss_redirect_table_").tempdir().unwrap();
    fs::write(dir.path().join("index.md"), HOME).unwrap();
    write_all(dir.path(), files);
    let out = dir.path().join(".moss").join("build.nosync").join("site");
    fs::create_dir_all(&out).unwrap();
    let structure = crate::build::scan_folder(dir.path().to_str().unwrap()).unwrap();
    let mut pending = crate::build::manifest::PendingManifest::for_build(crate::types::content::SiteHashes::default(), crate::build::cloud_ledger::InputEvidence::new(dir.path()));
    let _ = take_cli_problems();
    // What the pipeline does with its one parse of the config.
    let declared_redirects = fs::read_to_string(dir.path().join(".moss/config.toml"))
        .ok()
        .map(|text| crate::config::ConfigFile::parse(&text).unwrap().declared_redirects())
        .unwrap_or_default();
    crate::build::render::generate_blocking_content_for_build(
        &crate::vault::paths::VaultRoot::resolve(dir.path()),
        &structure,
        &out,
        None,
        None,
        true,
        SiteConfig {
            site_url_override: Some("https://example.com".to_string()),
            declared_redirects,
            ..SiteConfig::default()
        },
        &mut pending,
        true,
        false,
        crate::build::folder_embed::upcoming::now(),
    )
    .unwrap();
    crate::build::emit::slots::write_as_rendered(&out, pending.take_unwritten_pages()).unwrap();
    // The seal writes the file copies and folder moves, from what is served.
    let mut sealed = pending.seal();
    let late = crate::system::build_records::records().late_redirects(dir.path().to_str().unwrap());
    crate::build::feeds::kept_addresses::keep(&crate::moss_paths::MossPaths::new(dir.path()), &out, &mut sealed, &late);
    let problems = take_cli_problems();
    Built { _dir: dir, out, problems }
}

fn config(redirects: &str) -> String {
    format!("schema_version = 6\n\n[redirects]\n{redirects}\n")
}

const ABOUT: (&str, &str) = ("about.md", "---\ntitle: About\n---\n\n# About\n");

fn page(title: &str) -> String {
    format!("---\ntitle: {title}\n---\n\n# {title}\n")
}

#[test]
fn a_recorded_rename_still_gets_its_stub() {
    let b = build(&[ABOUT, (".moss/redirects.json", r#"{"old-about/": "about/"}"#)]);
    assert!(b.read("old-about/index.html").contains("url=/about/"));
    assert_eq!(b.problems, 0);
}

/// The recorded history may aim at a section of the new page.
#[test]
fn a_target_fragment_survives_into_the_stub() {
    let b = build(&[ABOUT, (".moss/redirects.json", r##"{"old-team/": "about/#team"}"##)]);
    assert!(b.read("old-team/index.html").contains("url=/about/#team"));
    assert_eq!(b.problems, 0);
}

/// A rename recorded inside a language edition outlives its target when the
/// translated page is later deleted. The old address keeps answering: at the
/// translation while it exists, else at the default-language page in the same
/// place, never at the deleted address.
#[test]
fn a_recorded_rename_whose_translation_is_gone_lands_on_the_default_language_page() {
    let history = r#"{"zh-hans/old-kept/": "zh-hans/kept/", "zh-hans/old-gone/": "zh-hans/gone/?from=old"}"#;
    let b = build(&[
        ("kept.md", &page("Kept")),
        ("zh-hans/kept.md", &page("保留")),
        ("gone.md", &page("Gone")),
        (".moss/redirects.json", history),
    ]);
    assert!(b.read("zh-hans/old-kept/index.html").contains("url=/zh-hans/kept/"));
    assert!(b.read("zh-hans/old-gone/index.html").contains("url=/gone/?from=old"));
}

/// A language's home is its folder, written with or without the slash; when
/// that edition is gone, the default language's home is the same place.
#[test]
fn a_gone_language_home_lands_on_the_site_home_in_either_spelling() {
    let history = r#"{"zh-hans/start/": "zh-hans/", "zh-hant/start/": "zh-hant"}"#;
    let b = build(&[ABOUT, (".moss/redirects.json", history)]);
    assert!(b.read("zh-hans/start/index.html").contains("url=/\""));
    assert!(b.read("zh-hant/start/index.html").contains("url=/\""));
}

/// The default-language page must be one this build serves itself. Another
/// entry's stub at that place is not followed: the old address is left as
/// recorded and the dead-link audit names it.
#[test]
fn a_fallback_that_is_itself_only_a_redirect_is_not_followed() {
    let history = r#"{"zh-hans/old/": "zh-hans/mid/", "mid/": "new/"}"#;
    let b = build(&[("new.md", &page("New")), (".moss/redirects.json", history)]);
    assert!(b.read("mid/index.html").contains("url=/new/"));
    assert!(b.read("zh-hans/old/index.html").contains("url=/zh-hans/mid/"));
}

/// Only a language edition falls back. On a single-language site a missing
/// target is left as recorded, and an ordinary first folder is not stripped.
#[test]
fn a_missing_target_outside_a_language_edition_is_left_as_recorded() {
    let history = r#"{"old-guide/": "guides/gone/"}"#;
    let b = build(&[ABOUT, ("gone.md", &page("Gone")), (".moss/redirects.json", history)]);
    assert!(b.read("old-guide/index.html").contains("url=/guides/gone/"));
}

/// A notebook's viewer page is written after the table is planned, beside its
/// `.ipynb`; only a notebook the scan publishes gets one.
#[test]
fn a_notebook_viewer_is_served_only_for_a_published_notebook() {
    let notebook = r#"{"cells":[],"metadata":{},"nbformat":4,"nbformat_minor":5}"#;
    let cfg = config(
        r#""/old-notebook.html" = "/analysis.html"
"/old-hidden.html" = "/node_modules/hidden.html""#,
    );
    let b = build(&[
        ABOUT,
        ("analysis.ipynb", notebook),
        ("node_modules/hidden.ipynb", notebook),
        (".moss/config.toml", &cfg),
    ]);
    assert!(b.read("old-notebook.html").contains("url=/analysis.html"));
    assert!(!b.has("old-hidden.html"), "the scan skips node_modules, so no viewer is written there");
    assert_eq!(b.problems, 1);
}

/// A generated page registered late in the blocking phase, like a media
/// collection page, is an address the site serves.
#[test]
fn a_declared_redirect_to_a_media_collection_page_is_kept() {
    let cfg = config(r#""/old-photos/" = "/photography/""#);
    let b = build(&[
        ("essay.md", "---\ntitle: Essay\n---\n\n![A photo](photo.png)\n<!-- photography -->\n"),
        (".moss/config.toml", &cfg),
    ]);
    assert!(b.has("photography/index.html"), "premise: the collection page is built");
    assert!(b.read("old-photos/index.html").contains("url=/photography/"));
    assert_eq!(b.problems, 0);
}

#[test]
fn a_declared_page_address_gets_a_stub_in_each_spelling() {
    let cfg = config(
        r#""/old-a/" = "/about/"
"/old-b" = "/about/"
"old-c.html" = "/about/"
"/old-d.html" = "/about/""#,
    );
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    for stub in ["old-a/index.html", "old-b/index.html", "old-c.html", "old-d.html"] {
        assert!(b.read(stub).contains("url=/about/"), "{stub}");
    }
    assert_eq!(b.problems, 0);
}

#[test]
fn a_declared_file_address_is_a_byte_copy_of_its_target() {
    let cfg = config(r#""/data.csv" = "/assets/data.csv""#);
    let b = build(&[ABOUT, ("assets/data.csv", "a,b\n列,2\n"), (".moss/config.toml", &cfg)]);
    assert_eq!(b.read("data.csv"), "a,b\n列,2\n");
    assert_eq!(b.problems, 0);
}

/// A target in a folder named with capitals is served under its lowercased
/// path; its copy still finds the file as the folder names it. (A case-blind
/// file system finds it either way, so only a case-sensitive one can fail.)
#[test]
fn a_file_redirect_finds_a_target_whose_folder_name_is_slugged() {
    let cfg = config(r#""/old.csv" = "/My Docs/data.csv""#);
    let b = build(&[ABOUT, ("My Docs/data.csv", "a,b\n"), (".moss/config.toml", &cfg)]);
    assert_eq!(b.read("old.csv"), "a,b\n");
    assert_eq!(b.problems, 0);
}

#[test]
fn a_declared_external_target_gets_a_stub_pointing_at_the_url() {
    let cfg = config(r#""/shop/" = "https://example.com/shop""#);
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    let stub = b.read("shop/index.html");
    assert!(stub.contains(r#"url=https://example.com/shop""#), "{stub}");
    assert_eq!(b.problems, 0);
}

#[test]
fn an_address_the_site_really_serves_keeps_its_content() {
    let cfg = config(
        r#""/about/" = "/contact/"
"/keep.html" = "/contact/""#,
    );
    let b = build(&[ABOUT, ("contact.md", "# Contact\n"), ("keep.html", "<p>mine</p>"), (".moss/config.toml", &cfg)]);
    assert!(!b.read("about/index.html").contains("Redirecting"));
    assert!(!b.has("keep.html"), "the folder's own file is copied in later; no stub may sit in its place");
    assert_eq!(b.problems, 2);
}

#[test]
fn a_self_redirect_and_an_unknown_target_are_advisories_not_stubs() {
    let cfg = config(
        r#""/gone/" = "/gone/"
"/old/" = "/nowhere/"
"/plain/" = "http://example.com/""#,
    );
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert_eq!(b.problems, 3);
    for rel in ["gone/index.html", "old/index.html", "plain/index.html"] {
        assert!(!b.has(rel), "{rel}");
    }
}

#[test]
fn a_file_address_with_no_file_target_has_no_static_form_and_is_reported() {
    let cfg = config(
        r#""/a.pdf" = "https://example.com/a.pdf"
"/b.csv" = "/about/""#,
    );
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert!(!b.has("a.pdf") && !b.has("b.csv"));
    assert_eq!(b.problems, 2);
}

/// No table of redirects is published; each is served by its stub or copy.
#[test]
fn every_source_is_served_without_publishing_a_table() {
    let cfg = config(
        r#""/old-post/" = "/about/"
"/data.csv" = "/assets/data.csv""#,
    );
    let b = build(&[
        ABOUT,
        ("assets/data.csv", "x"),
        (".moss/redirects.json", r#"{"old-about/": "about/"}"#),
        (".moss/config.toml", &cfg),
    ]);
    assert_eq!(b.read("data.csv"), "x");
    assert_eq!(b.read("feed.xml"), b.read("rss.xml"));
    assert!(b.read("old-about/index.html").contains("url=/about/"));
    assert!(b.read("old-post/index.html").contains("url=/about/"));
    assert!(!b.has("_moss/redirects.json"));
    assert_eq!(b.problems, 0);
}

/// An unlisted page's address is its secret: after a move, the old address
/// may appear in its own forwarding page and nowhere else the site publishes.
#[test]
fn a_moved_unlisted_pages_old_address_is_published_only_in_its_own_stub() {
    let cfg = config(r#""/review/k7x9q2/" = "/review/client/k7x9q2/""#);
    let b = build(&[
        ABOUT,
        (
            "review/client/k7x9q2/index.html",
            "<!doctype html><html><head><meta name=\"robots\" content=\"noindex\"><title>Round</title></head><body>Round</body></html>",
        ),
        (".moss/config.toml", &cfg),
    ]);
    let stub = b.read("review/k7x9q2/index.html");
    assert!(stub.contains("url=/review/client/k7x9q2/"));
    assert!(stub.contains(r#"<meta name="robots" content="noindex">"#), "a stub must not be indexed: {stub}");
    let mut leaks = Vec::new();
    for entry in walkdir::WalkDir::new(&b.out).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file()) {
        let rel = entry.path().strip_prefix(&b.out).unwrap().to_string_lossy().replace('\\', "/");
        let text = String::from_utf8_lossy(&fs::read(entry.path()).unwrap()).into_owned();
        if rel != "review/k7x9q2/index.html" && text.contains("/review/k7x9q2/") {
            leaks.push(rel);
        }
    }
    assert!(leaks.is_empty(), "the old address is published in {leaks:?}");
}

/// A folder line naming the site root would copy every address of the site,
/// so it moves no folder; the page itself is still redirected.
#[test]
fn a_folder_line_naming_the_root_moves_no_folder() {
    let cfg = config(r#""/blog/" = "/""#);
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert!(b.read("blog/index.html").contains("url=/\""));
    assert!(!b.has("blog/about/index.html"));
    assert_eq!(b.problems, 1);
}

/// A file can only be kept by a copy, so a declared line sending the old feed
/// to a page is reported and the generator's own alias stays.
#[test]
fn a_declared_feed_xml_with_no_static_form_leaves_the_alias() {
    let cfg = config(r#""/feed.xml" = "/about/""#);
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert_eq!(b.read("feed.xml"), b.read("rss.xml"));
    assert_eq!(b.problems, 1);
}

#[test]
fn a_target_cannot_inject_markup_into_a_stub() {
    let cfg = config(
        r##""/a/" = "/about/#x\"><script>alert(1)</script>&y"
"/b/" = "https://example.com/?p=1&q=2""##,
    );
    let b = build(&[
        ABOUT,
        (".moss/redirects.json", r##"{"old-c/": "about/#\"><script>alert(2)</script>"}"##),
        (".moss/config.toml", &cfg),
    ]);
    for stub in ["a/index.html", "b/index.html", "old-c/index.html"] {
        let html = b.read(stub);
        assert!(!html.contains("<script>"), "{stub}: {html}");
    }
    assert!(b.read("a/index.html").contains("url=/about/#x&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;&amp;y"));
    assert!(b.read("b/index.html").contains(r#"href="https://example.com/?p=1&amp;q=2""#));
}

#[test]
fn an_old_address_is_served_exactly_as_declared() {
    let cfg = config(
        r#""/舊文章/" = "/about/"
"/Caf%C3%A9.html" = "/about/""#,
    );
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert!(b.read("舊文章/index.html").contains("url=/about/"));
    // A percent-escape names the file by its decoded name.
    assert!(b.read("Café.html").contains("url=/about/"));
    assert_eq!(b.problems, 0);
}

/// A published file path cannot carry `Old Post`, so no stub may stand in at a
/// rewritten address: the entry is reported and writes nothing.
#[test]
fn an_address_a_file_path_cannot_carry_is_reported_and_writes_nothing() {
    let cfg = config(
        r#""/Old Post/" = "/about/"
"/old-post/" = "/about/""#,
    );
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert!(b.read("old-post/index.html").contains("url=/about/"), "the plain spelling keeps its stub");
    assert!(!b.has("Old Post"));
    assert_eq!(b.problems, 1);
}

/// Same, with nothing else to confuse the stub check.
#[test]
fn a_cased_address_alone_leaves_no_file_at_either_spelling() {
    let cfg = config(r#""/Old Post/" = "/about/""#);
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert!(!b.has("old-post/index.html") && !b.has("Old Post/index.html"));
    assert_eq!(b.problems, 1);
}

#[test]
fn an_ambiguous_or_escaping_address_is_refused_not_guessed() {
    let cfg = config(
        r#""/a?x=1/" = "/about/"
"/b#c/" = "/about/"
"/../d/" = "/about/"
"/e/" = "/../about/"
"/f/" = "//example.com/x"
"/g/" = "javascript:alert(1)"
"/h/" = "http://example.com/""#,
    );
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert_eq!(b.problems, 7);
    for rel in ["e/index.html", "f/index.html", "g/index.html", "h/index.html"] {
        assert!(!b.has(rel), "{rel}");
    }
}

/// Declared entries arrive through the build's one parse of the config, not a
/// second read of the file.
#[test]
fn declared_entries_come_from_the_inputs_not_from_the_file() {
    use crate::build::feeds::redirects::{emit_redirect_table, TableInputs};
    let dir = tempfile::Builder::new().prefix("moss_redirect_inputs_").tempdir().unwrap();
    let out = dir.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let paths = crate::moss_paths::MossPaths::new(dir.path());
    let mut pending = crate::build::manifest::PendingManifest::new(crate::types::content::SiteHashes::default());
    let about = crate::build::served_path::ServedPath::from_source("about/index.html").unwrap();
    crate::build::context::BuildContext::for_render(&out, &mut pending)
        .emit(&about, b"<html></html>", crate::build::manifest::HashBucket::Files)
        .unwrap();
    let inputs = TableInputs { declared: vec![("/old/".into(), "/about/".into())], ..Default::default() };
    emit_redirect_table(&paths, &crate::build::scan::article_map::ArticleMap::new(), &inputs, &out, &mut pending)
        .unwrap();
    assert!(fs::read_to_string(out.join("old/index.html")).unwrap().contains("url=/about/"));
}

/// The pipeline is what hands the parsed config's entries to the build.
#[test]
fn the_pipeline_passes_the_configs_redirects_to_the_build() {
    use crate::build::pipeline::{run, PipelineRunOutput};
    let dir = tempfile::Builder::new().prefix("moss_redirect_pipeline_").tempdir().unwrap();
    write_all(dir.path(), &[ABOUT, ("index.md", HOME), (".moss/config.toml", &config(r#""/old/" = "/about/""#))]);
    let folder = dir.path().to_str().unwrap();
    let ps = crate::build::scan::scan::scan_folder(folder).unwrap();
    let root = crate::vault::paths::VaultRoot::resolve(folder);
    let port = crate::build::ports::port_of_this_build(None, None);
    let keys = crate::build::ports::CacheKeyInputs { builder: "test-builder-fingerprint".to_string() };
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    rt.block_on(async {
        let PipelineRunOutput { bg_handle, .. } = run(
            &root,
            None,
            None,
            &port,
            None,
            Some(Box::new(|_, _, _| Ok(crate::build::enhance::ResolvedSlots::empty()))),
            &ps,
            Some("https://example.com".to_string()),
            crate::build::render::IncrementalGates::default(),
            crate::build::feeds::search_lane::Freshness::Now,
            &keys,
        )
        .map_err(crate::build::outcome::BuildStopped::into_message)
        .unwrap();
        bg_handle.unwrap().await_completion().await.unwrap();
    });
    let stub = crate::moss_paths::MossPaths::new(dir.path()).staging_dir().join("old/index.html");
    assert!(fs::read_to_string(stub).unwrap().contains("url=/about/"));
}
