//! `[site].me` writes `rel="me"` links into the head of the site's pages, and
//! `[site].atproto_did` writes the file a Bluesky domain handle is checked against.

#[path = "support/site.rs"]
mod site;

use site::Site;

const PAGES: [(&str, &str); 2] = [
    ("index.md", "---\ntitle: Harbour\n---\n\nWelcome.\n"),
    ("notes.md", "---\ntitle: Notes\ntags: [harbour]\n---\n\nA note.\n"),
];

const ME: &str = "me = [\"https://social.example/@ana\", \"https://bsky.example/profile/ana.example\"]\n";

fn me_links(html: &str) -> Vec<&str> {
    let head = &html[..html.find("</head>").expect("a head")];
    head.split("<link rel=\"me\" href=\"").skip(1).filter_map(|p| p.split('"').next()).collect()
}

#[test]
fn me_writes_one_head_link_per_address_on_a_page_and_on_the_home_page() {
    // A generated term page and the not-found page carry them too, through
    // the same page chrome.
    let pages = [PAGES[0], PAGES[1], ("404.md", "---\ntitle: Not found\n---\n\nNothing here.\n")];
    let site = Site::with_config(ME, &pages);
    for page in ["index.html", "notes/index.html", "tags/harbour/index.html", "404.html"] {
        assert_eq!(
            me_links(&site.html(page)),
            ["https://social.example/@ana", "https://bsky.example/profile/ana.example"],
            "{page}"
        );
    }
}

#[test]
fn one_me_address_written_without_a_list_is_a_list_of_one() {
    let site = Site::with_config("me = \"https://social.example/@ana\"\n", &PAGES);
    assert_eq!(me_links(&site.html("index.html")), ["https://social.example/@ana"]);
}

#[test]
fn a_me_entry_that_is_not_http_warns_by_setting_name_and_is_skipped() {
    let site = Site::with_config("me = [\"https://social.example/@ana\", \"mailto:ana@example.org\"]\n", &PAGES);
    assert_eq!(me_links(&site.html("index.html")), ["https://social.example/@ana"]);
    assert!(
        site.logged.iter().any(|l| l.contains("[site].me") && l.contains("mailto:ana@example.org")),
        "{:?}",
        site.logged
    );
}

#[test]
fn atproto_did_writes_the_did_as_the_well_known_file() {
    let site = Site::with_config("atproto_did = \"did:plc:abcdefghijklmnopqrstuvwx\"\n", &PAGES);
    let text = std::fs::read_to_string(site.output(".well-known/atproto-did")).expect("the file is written");
    assert_eq!(text.trim_end(), "did:plc:abcdefghijklmnopqrstuvwx");
}

#[test]
fn the_sealed_generation_a_publish_uploads_carries_the_did_file() {
    let site = Site::with_config("atproto_did = \"did:plc:abcdefghijklmnopqrstuvwx\"\n", &PAGES);
    let served = site.root.join(".moss/build.nosync/current/.well-known/atproto-did");
    assert_eq!(std::fs::read_to_string(served).expect("sealed copy").trim_end(), "did:plc:abcdefghijklmnopqrstuvwx");
    let hashes = std::fs::read_to_string(site.root.join(".moss/build.nosync/hashes.json")).unwrap();
    assert!(hashes.contains(".well-known/atproto-did"), "the manifest names the file");
}

#[test]
fn an_atproto_did_that_is_not_a_did_warns_and_writes_nothing() {
    let site = Site::with_config("atproto_did = \"@ana.example\"\n", &PAGES);
    assert!(!site.output(".well-known/atproto-did").exists());
    assert!(site.logged.iter().any(|l| l.contains("[site].atproto_did")), "{:?}", site.logged);
}

#[test]
fn without_the_settings_no_link_and_no_file_are_written() {
    let site = Site::new(&PAGES);
    assert!(me_links(&site.html("index.html")).is_empty());
    assert!(!site.html("index.html").contains("rel=\"me\""));
    assert!(!site.output(".well-known").exists());
}
