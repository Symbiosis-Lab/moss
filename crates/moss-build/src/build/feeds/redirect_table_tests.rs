//! The redirect table end to end: every source, every static form, and the
//! `_moss/redirects.json` a capable host reads. Each case builds a real
//! folder through the blocking render, the same entry the preview and the
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
    fn table(&self) -> String {
        self.read("_moss/redirects.json")
    }
}

const HOME: &str = "---\ntitle: Home\n---\n\n# Home\n";

/// Build `files` (path, contents) as a deployed site, plus a home page.
fn build(files: &[(&str, &str)]) -> Built {
    // Not `tempdir()`: its `.tmpXXXX` name is hidden, and the scan skips hidden folders.
    let dir = tempfile::Builder::new().prefix("moss_redirect_table_").tempdir().unwrap();
    fs::write(dir.path().join("index.md"), HOME).unwrap();
    for (rel, contents) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    let out = dir.path().join(".moss").join("build.nosync").join("site");
    fs::create_dir_all(&out).unwrap();
    let structure = crate::build::scan_folder(dir.path().to_str().unwrap()).unwrap();
    let mut pending = crate::build::manifest::PendingManifest::new(crate::types::content::SiteHashes::default());
    pending.set_input_evidence(crate::build::cloud_ledger::InputEvidence::new(dir.path()));
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
    )
    .unwrap();
    crate::build::emit::slots::write_as_rendered(&out, pending.take_unwritten_pages()).unwrap();
    let problems = take_cli_problems();
    Built { _dir: dir, out, problems }
}

fn config(redirects: &str) -> String {
    format!("schema_version = 6\n\n[redirects]\n{redirects}\n")
}

const ABOUT: (&str, &str) = ("about.md", "---\ntitle: About\n---\n\n# About\n");

#[test]
fn a_recorded_rename_still_gets_its_stub() {
    let b = build(&[ABOUT, (".moss/data/redirects.json", r#"{"old-about/": "about/"}"#)]);
    assert!(b.read("old-about/index.html").contains("url=/about/"));
    assert_eq!(b.problems, 0);
}

/// The recorded history may aim at a section of the new page.
#[test]
fn a_target_fragment_survives_into_the_stub() {
    let b = build(&[ABOUT, (".moss/data/redirects.json", r##"{"old-team/": "about/#team"}"##)]);
    assert!(b.read("old-team/index.html").contains("url=/about/#team"));
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
    assert!(!b.table().contains("/about/") && !b.table().contains("keep.html"));
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
    assert_eq!(b.table(), r#"{"version":1,"redirects":[{"from":"/feed.xml","to":"/rss.xml","status":301}]}"#);
}

#[test]
fn a_file_address_with_no_file_target_has_no_static_form_but_is_listed() {
    let cfg = config(
        r#""/a.pdf" = "https://example.com/a.pdf"
"/b.csv" = "/about/""#,
    );
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert!(!b.has("a.pdf") && !b.has("b.csv"));
    assert_eq!(b.problems, 2);
    let table = b.table();
    assert!(table.contains(r#"{"from":"/a.pdf","to":"https://example.com/a.pdf","status":301}"#), "{table}");
    assert!(table.contains(r#"{"from":"/b.csv","to":"/about/","status":301}"#), "{table}");
}

#[test]
fn the_shipped_table_lists_every_source_in_order() {
    let cfg = config(
        r#""/old-post/" = "/about/"
"/data.csv" = "/assets/data.csv""#,
    );
    let b = build(&[
        ABOUT,
        ("assets/data.csv", "x"),
        (".moss/data/redirects.json", r#"{"old-about/": "about/"}"#),
        (".moss/config.toml", &cfg),
    ]);
    assert_eq!(
        b.table(),
        concat!(
            r#"{"version":1,"redirects":["#,
            r#"{"from":"/data.csv","to":"/assets/data.csv","status":301},"#,
            r#"{"from":"/feed.xml","to":"/rss.xml","status":301},"#,
            r#"{"from":"/old-about/","to":"/about/","status":301},"#,
            r#"{"from":"/old-post/","to":"/about/","status":301}"#,
            r#"]}"#
        )
    );
}

#[test]
fn a_declared_feed_xml_replaces_the_alias() {
    let cfg = config(r#""/feed.xml" = "/about/""#);
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert!(b.table().contains(r#"{"from":"/feed.xml","to":"/about/","status":301}"#));
}

#[test]
fn a_target_cannot_inject_markup_into_a_stub() {
    let cfg = config(
        r##""/a/" = "/about/#x\"><script>alert(1)</script>&y"
"/b/" = "https://example.com/?p=1&q=2""##,
    );
    let b = build(&[
        ABOUT,
        (".moss/data/redirects.json", r##"{"old-c/": "about/#\"><script>alert(2)</script>"}"##),
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
fn an_old_address_is_listed_exactly_as_declared() {
    let cfg = config(
        r#""/舊文章/" = "/about/"
"/Caf%C3%A9.html" = "/about/""#,
    );
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    let table = b.table();
    for from in ["/舊文章/", "/Caf%C3%A9.html"] {
        assert!(table.contains(&format!(r#"{{"from":"{from}","#)), "{from}: {table}");
    }
    // A percent-escape names the file by its decoded name.
    assert!(b.read("Café.html").contains("url=/about/"));
    assert_eq!(b.problems, 0);
}

/// A published file path cannot carry `Old Post`, so no stub may stand in at a
/// rewritten address: the entry is listed and reported, and writes nothing.
#[test]
fn an_address_a_file_path_cannot_carry_is_listed_but_writes_nothing() {
    let cfg = config(
        r#""/Old Post/" = "/about/"
"/old-post/" = "/about/""#,
    );
    let b = build(&[ABOUT, (".moss/config.toml", &cfg)]);
    assert!(b.read("old-post/index.html").contains("url=/about/"), "the plain spelling keeps its stub");
    assert!(!b.has("Old Post"));
    assert_eq!(b.problems, 1);
    assert!(b.table().contains(r#"{"from":"/Old Post/","to":"/about/","status":301}"#), "{}", b.table());
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
    assert_eq!(b.table(), r#"{"version":1,"redirects":[{"from":"/feed.xml","to":"/rss.xml","status":301}]}"#);
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
    for (rel, body) in [
        ("index.md", HOME),
        ("about.md", "---\ntitle: About\n---\n\n# About\n"),
        (".moss/config.toml", &config(r#""/old/" = "/about/""#)),
    ] {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
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
