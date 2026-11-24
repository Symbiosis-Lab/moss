//! The site's own icon, passed to copy channels, and the verification links a
//! channel's copy record asks the site to carry.

use moss_build::moss_paths::MossPaths;
use moss_build::syndication::job::site_icon;
use moss_build::syndication::record::{CopyRecord, Records, Verification, VerifyFile, VerifyLink};

use crate::site::Site;

const SITE: &str = "https://example.org";
const CONFIG: &str = "url = \"https://example.org\"\n";
const DOC: &str = "at://did:plc:abcdefghijklmnopqrstuvwx/site.standard.document/3kabc";
const PUBLICATION: &str = "at://did:plc:abcdefghijklmnopqrstuvwx/site.standard.publication/3kxyz";
const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><circle cx=\"5\" cy=\"5\" r=\"4\" fill=\"red\"/></svg>";
const PAGES: [(&str, &str); 2] = [
    ("index.md", "---\ntitle: Harbour\n---\n\nWelcome.\n"),
    ("notes.md", "---\ntitle: Notes\n---\n\nA note.\n"),
];

fn pages_with(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut files = PAGES.to_vec();
    files.extend_from_slice(extra);
    files
}

#[test]
fn an_svg_icon_gives_the_512_raster_as_the_site_icon() {
    let site = Site::with_config(CONFIG, &pages_with(&[("assets/favicon.svg", SVG)]));
    assert_eq!(site_icon(&site.root, SITE).as_deref(), Some("https://example.org/assets/favicon-512.png"));
}

#[test]
fn no_icon_or_an_ico_gives_no_site_icon() {
    let none = Site::with_config(CONFIG, &PAGES);
    assert_eq!(site_icon(&none.root, SITE), None, "moss's own mark is not the site's icon");
    let ico = Site::with_config(CONFIG, &pages_with(&[("assets/favicon.ico", "not an image")]));
    assert_eq!(site_icon(&ico.root, SITE), None);
}

fn record(url_path: &str, site_url: &str, verify: Verification) -> CopyRecord {
    let mut r = CopyRecord::due("aa11aa11", "bsky", "notes.md", url_path);
    r.site_url = Some(site_url.into());
    r.verify = Some(verify);
    r
}

fn save(site: &Site, record: &CopyRecord) {
    Records::in_vault(&MossPaths::new(&site.root)).save(record).unwrap();
}

fn full(rel: &str, href: &str, path: &str, body: &str) -> Verification {
    Verification {
        page_link: Some(VerifyLink { rel: rel.into(), href: href.into() }),
        site_file: Some(VerifyFile { path: path.into(), body: body.into() }),
    }
}

fn good() -> Verification {
    full("site.standard.document", DOC, ".well-known/site.standard.publication", PUBLICATION)
}

fn head(html: &str) -> &str {
    &html[..html.find("</head>").expect("a head")]
}

fn link() -> String {
    format!("<link rel=\"site.standard.document\" href=\"{DOC}\">")
}

#[test]
fn a_record_with_verify_puts_the_link_on_its_page_and_writes_the_file() {
    let site = Site::with_config(CONFIG, &PAGES);
    save(&site, &record("notes/", SITE, good()));
    site.rebuild();
    assert!(head(&site.html("notes/index.html")).contains(&link()));
    assert!(!head(&site.html("index.html")).contains("site.standard.document"), "only the record's page");
    let file = std::fs::read_to_string(site.output(".well-known/site.standard.publication")).expect("the file is written");
    assert_eq!(file.trim_end(), PUBLICATION);
}

#[test]
fn a_record_for_another_site_does_neither() {
    let site = Site::with_config(CONFIG, &PAGES);
    save(&site, &record("notes/", "https://elsewhere.example", good()));
    site.rebuild();
    assert!(!head(&site.html("notes/index.html")).contains("site.standard.document"));
    assert!(!site.output(".well-known/site.standard.publication").exists());
}

#[test]
fn a_disallowed_rel_href_or_path_is_dropped_with_a_warning() {
    for (what, verify) in [
        ("rel", full("stylesheet", DOC, ".well-known/site.standard.publication", PUBLICATION)),
        ("href", full("site.standard.document", "https://evil.example/", ".well-known/site.standard.publication", PUBLICATION)),
        ("path", full("site.standard.document", DOC, ".well-known/webauthn", PUBLICATION)),
    ] {
        let site = Site::with_config(CONFIG, &PAGES);
        save(&site, &record("notes/", SITE, verify));
        let built = site.rebuild_logged();
        let html = site.html("notes/index.html");
        let linked = head(&html).contains(&format!("href=\"{DOC}\"")) || head(&html).contains("evil.example");
        let filed = site.output(".well-known/site.standard.publication").exists() || site.output(".well-known/webauthn").exists();
        // Each bad half is dropped; the other half is still honoured.
        assert_eq!(linked, what == "path", "{what}: link");
        assert_eq!(filed, what != "path", "{what}: file");
        assert!(built.logged.iter().any(|l| l.contains("copy verification")), "{what}: {:?}", built.logged);
        assert!(!head(&html).contains("rel=\"stylesheet\" href=\"at:"), "{what}");
    }
}

/// An incremental rebuild after a change to one page's text only.
fn rebuild_after_editing_other(site: &Site, text: &str) {
    site.write("other.md", &format!("---\ntitle: Other\n---\n\n{text}\n"));
    let changed = vec![site.root.join("other.md")];
    crate::site::build_sync(&site.root.to_string_lossy(), moss_build::build::BuildTrigger::ContentOnly(changed)).unwrap();
}

#[test]
fn a_record_added_after_the_build_reaches_a_page_the_rebuild_would_carry() {
    let site = Site::with_config(CONFIG, &pages_with(&[("other.md", "---\ntitle: Other\n---\n\nFirst.\n")]));
    let notes = site.output("notes/index.html");
    std::fs::write(&notes, format!("{}<!--carried-->", site.html("notes/index.html"))).unwrap();
    rebuild_after_editing_other(&site, "Welcome back.");
    assert!(site.html("notes/index.html").contains("<!--carried-->"), "without a record the page is carried");
    save(&site, &record("notes/", SITE, good()));
    rebuild_after_editing_other(&site, "Welcome again.");
    assert!(head(&site.html("notes/index.html")).contains(&link()), "the record re-renders the page it names");
}
