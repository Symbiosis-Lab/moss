//! `[site].me` writes `rel="me"` links into the head of the site's pages, and
//! `[site].atproto_did` writes the file a Bluesky domain handle is checked against.

use moss_build::moss_paths::MossPaths;
use crate::site;

use site::{LoggedSite, Site};

const PAGES: [(&str, &str); 2] = [
    ("index.md", "---\ntitle: Harbour\n---\n\nWelcome.\n"),
    ("notes.md", "---\ntitle: Notes\ntags: [harbour]\n---\n\nA note.\n"),
];

const ME: &str = "me = [\"https://social.example/@ana\", \"https://bsky.example/profile/ana.example\"]\n";

#[test]
fn a_folder_holding_only_a_file_serves_the_file_and_no_page() {
    let mut files = PAGES.to_vec();
    files.push(("oauth/client.json", "{}"));
    let site = Site::with_config("", &files);
    assert!(site.output("oauth/client.json").exists(), "the folder's file is served");
    assert!(!site.output("oauth/index.html").exists(), "a folder with no page beneath it gets no page");
}

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
    let site = LoggedSite::with_config("me = [\"https://social.example/@ana\", \"mailto:ana@example.org\"]\n", &PAGES);
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
    let served = MossPaths::new(&site.root).current_ptr().join(".well-known/atproto-did");
    assert_eq!(std::fs::read_to_string(served).expect("sealed copy").trim_end(), "did:plc:abcdefghijklmnopqrstuvwx");
    let hashes = std::fs::read_to_string(MossPaths::new(&site.root).hashes()).unwrap();
    assert!(hashes.contains(".well-known/atproto-did"), "the manifest names the file");
}

#[test]
fn an_atproto_did_that_is_not_a_did_warns_and_writes_nothing() {
    let site = LoggedSite::with_config("atproto_did = \"@ana.example\"\n", &PAGES);
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

// A plugin's `contributes.identity`, filled from its `[services.<id>]` table.
// This harness builds with plugin hooks skipped, so each test below also
// proves the proofs survive a build that runs no plugin code.

const HANDLE_PLUGIN: &str = r#"{"name":"skyhandle","version":"1.0.0","contributes":{
  "site_settings":{"settings":[{"key":"did","type":"string","label":"DID"}]},
  "identity":[{"file":".well-known/atproto-did","body":"{did}"}]}}"#;

const SOCIAL_PLUGIN: &str = r#"{"name":"tootle","version":"1.0.0","contributes":{
  "site_settings":{"settings":[{"key":"account","type":"acct","label":"Account"}]},
  "identity":[
    {"link":"me","href":"https://{account.host}/@{account.user}"},
    {"meta":"fediverse:creator","content":"@{account.user}@{account.host}"}]}}"#;

const SIGNED_IN_PLUGIN: &str = r#"{"name":"tootle","version":"1.0.0","contributes":{
  "site_settings":{"settings":[{"key":"account","type":"acct","label":"Account","from":"account"}]},
  "identity":[
    {"link":"me","href":"https://{account.host}/@{account.user}"},
    {"meta":"fediverse:creator","content":"@{account.user}@{account.host}"}]}}"#;

const NAMES_PLUGIN: &str = r#"{"name":"relaynames","version":"1.0.0","contributes":{
  "site_settings":{"settings":[{"key":"pubkey","type":"string","label":"Public key"}]},
  "identity":[{"file":".well-known/nostr.json","body":"{\"names\":{\"_\":\"{pubkey}\"}}"}]}}"#;

fn with_plugin<'a>(manifest: &'a str, name: &str) -> Vec<(String, &'a str)> {
    vec![(format!(".moss/plugins/{name}/manifest.json"), manifest)]
}

fn site_with(config: &str, plugins: &[(&str, &str)]) -> LoggedSite {
    let mut files: Vec<(String, &str)> = PAGES.iter().map(|(p, b)| (p.to_string(), *b)).collect();
    for (name, manifest) in plugins {
        files.extend(with_plugin(manifest, name));
    }
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, b)| (p.as_str(), *b)).collect();
    LoggedSite::with_config(config, &borrowed)
}

fn head_of(html: &str) -> &str {
    &html[..html.find("</head>").expect("a head")]
}

#[test]
fn a_plugin_file_proof_is_written_from_its_table_and_not_without_it() {
    let site = site_with("\n[services.skyhandle]\ndid = \"did:plc:abcdefghijklmnopqrstuvwx\"\n", &[("skyhandle", HANDLE_PLUGIN)]);
    let text = std::fs::read_to_string(site.output(".well-known/atproto-did")).expect("the plugin's proof is written");
    assert_eq!(text, "did:plc:abcdefghijklmnopqrstuvwx");

    let unset = site_with("", &[("skyhandle", HANDLE_PLUGIN)]);
    assert!(!unset.output(".well-known/atproto-did").exists(), "no value, no proof");
    assert!(!unset.logged.iter().any(|l| l.contains("identity")), "an unset network is not a problem: {:?}", unset.logged);
}

#[test]
fn link_and_meta_proofs_go_into_every_head_from_an_account_address() {
    let site = site_with("\n[services.tootle]\naccount = \"@ana@social.example\"\n", &[("tootle", SOCIAL_PLUGIN)]);
    for page in ["index.html", "notes/index.html", "tags/harbour/index.html"] {
        let html = site.html(page);
        let head = head_of(&html);
        assert!(head.contains("<link rel=\"me\" href=\"https://social.example/@ana\">"), "{page}: {head}");
        assert!(head.contains("<meta name=\"fediverse:creator\" content=\"@ana@social.example\">"), "{page}: {head}");
    }
}

#[test]
fn a_hand_written_account_that_is_not_name_at_host_is_named_and_left_out() {
    let site = site_with("\n[services.tootle]\naccount = \"ana\"\n", &[("tootle", SOCIAL_PLUGIN)]);
    assert!(!site.html("index.html").contains("fediverse:creator"));
    assert!(site.logged.iter().any(|l| l.contains("[services.tootle]") && l.contains("account")), "{:?}", site.logged);
}

#[test]
fn a_json_proof_escapes_its_value_and_stays_json() {
    let site = site_with("\n[services.relaynames]\npubkey = 'ab\"cd'\n", &[("relaynames", NAMES_PLUGIN)]);
    let text = std::fs::read_to_string(site.output(".well-known/nostr.json")).expect("written");
    let json: serde_json::Value = serde_json::from_str(&text).expect("still JSON");
    assert_eq!(json["names"]["_"], "ab\"cd");
}

#[test]
fn the_site_shorthand_wins_a_path_two_suppliers_claim_and_the_build_names_both() {
    let config = "atproto_did = \"did:plc:fromconfigfromconfigfromco\"\n\n[services.skyhandle]\ndid = \"did:plc:frompluginfrompluginfrompl\"\n";
    let site = site_with(config, &[("skyhandle", HANDLE_PLUGIN)]);
    let text = std::fs::read_to_string(site.output(".well-known/atproto-did")).unwrap();
    assert_eq!(text.trim_end(), "did:plc:fromconfigfromconfigfromco");
    assert!(
        site.logged.iter().any(|l| l.contains("[site].atproto_did") && l.contains("skyhandle")),
        "{:?}",
        site.logged
    );
}

#[test]
fn a_profile_in_site_me_and_a_plugin_link_to_it_give_one_link() {
    let config = "me = \"https://social.example/@ana\"\n\n[services.tootle]\naccount = \"@ana@social.example\"\n";
    let site = site_with(config, &[("tootle", SOCIAL_PLUGIN)]);
    assert_eq!(me_links(&site.html("index.html")), ["https://social.example/@ana"]);
}

#[test]
fn a_proof_that_goes_away_is_named_by_the_next_build() {
    let site = site_with("\n[services.skyhandle]\ndid = \"did:plc:abcdefghijklmnopqrstuvwx\"\n", &[("skyhandle", HANDLE_PLUGIN)]);
    site.write(".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    let logged = site.rebuild_logged().logged;
    assert!(!site.output(".well-known/atproto-did").exists());
    assert!(logged.iter().any(|l| l.contains("identity: .well-known/atproto-did removed")), "{logged:?}");
}

const SIGNED_IN: &str = "[accounts.tootle]\nid = \"7\"\nname = \"@ana@social.example\"\nhost = \"social.example\"\n";
const SIGNED_OUT: &str = "[accounts]\ntootle = \"signed-out\"\n";

fn creator(html: &str) -> bool {
    head_of(html).contains("<meta name=\"fediverse:creator\" content=\"@ana@social.example\">")
}

#[test]
fn a_signed_in_account_writes_the_identity_tags_and_signing_out_takes_them_away() {
    let site = site_with("", &[("tootle", SIGNED_IN_PLUGIN)]);
    assert!(!creator(&site.html("index.html")), "no account, no tag");

    site.write(".moss/state.toml", SIGNED_IN);
    site.rebuild();
    assert_eq!(me_links(&site.html("index.html")), ["https://social.example/@ana"]);
    assert!(creator(&site.html("index.html")));
    assert!(creator(&site.html("notes/index.html")));

    site.write(".moss/state.toml", SIGNED_OUT);
    site.rebuild();
    assert!(me_links(&site.html("index.html")).is_empty());
    assert!(!creator(&site.html("index.html")));
}

#[test]
fn a_clean_build_of_a_signed_in_folder_writes_the_tags() {
    let mut files: Vec<(String, &str)> = PAGES.iter().map(|(p, b)| (p.to_string(), *b)).collect();
    files.extend(with_plugin(SIGNED_IN_PLUGIN, "tootle"));
    files.push((".moss/state.toml".into(), SIGNED_IN));
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, b)| (p.as_str(), *b)).collect();
    let site = Site::unbuilt("en", "", &borrowed);
    site.rebuild();
    assert!(creator(&site.html("index.html")));
}

#[test]
fn an_account_written_in_the_services_table_wins_over_the_signed_in_one() {
    let site = site_with("\n[services.tootle]\naccount = \"@bo@other.example\"\n", &[("tootle", SIGNED_IN_PLUGIN)]);
    site.write(".moss/state.toml", SIGNED_IN);
    site.rebuild();
    assert_eq!(me_links(&site.html("index.html")), ["https://other.example/@bo"]);
    assert!(!creator(&site.html("index.html")));
}

#[test]
fn an_acct_setting_sign_in_does_not_fill_is_left_to_its_table() {
    let site = site_with("", &[("tootle", SOCIAL_PLUGIN)]);
    site.write(".moss/state.toml", SIGNED_IN);
    site.rebuild();
    assert!(me_links(&site.html("index.html")).is_empty());
    assert!(!creator(&site.html("index.html")));
}

#[test]
fn an_account_name_on_another_server_than_its_host_is_skipped_with_a_sentence() {
    let state = "[accounts.tootle]\nid = \"7\"\nname = \"@ana@elsewhere.example\"\nhost = \"social.example\"\n";
    let site = site_with("", &[("tootle", SIGNED_IN_PLUGIN)]);
    site.write(".moss/state.toml", state);
    let logged = site.rebuild_logged().logged;
    assert!(me_links(&site.html("index.html")).is_empty());
    assert!(logged.iter().any(|l| l.contains("[services.tootle] `account`")), "{logged:?}");
}
