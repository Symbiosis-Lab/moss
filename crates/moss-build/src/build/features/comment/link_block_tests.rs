use super::super::moderation::{append_owner_event, sign_mod_event, ModKind};
use super::super::reduce::resolve;
use super::super::{CommentAuthor, NormalizedComment};
use super::*;
use k256::elliptic_curve::Generate;
use k256::schnorr::SigningKey;

fn comment(id: &str, source: &str, website: Option<&str>, content: &str) -> NormalizedComment {
    NormalizedComment {
        id: id.into(),
        source: source.into(),
        content: content.into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        author: CommentAuthor { display_name: Some("Ana".into()), name: None, url: website.map(Into::into) },
        reply_to_id: None,
        state: None,
    }
}

struct Owner {
    sk: SigningKey,
    pubkey: String,
}

fn owner() -> Owner {
    let sk = SigningKey::generate();
    let pubkey = hex::encode(sk.verifying_key().to_bytes());
    Owner { sk, pubkey }
}

impl Owner {
    fn event(&self, kind: &str, source: &str, target: &str, seq: u64) -> ModEvent {
        sign_mod_event(&self.sk, kind, source, target, "site", "", seq, seq)
    }
}

fn project() -> tempfile::TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().join("target").join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let d = tempfile::TempDir::new_in(&base).unwrap();
    std::fs::create_dir_all(d.path().join(".moss")).unwrap();
    d
}

/// The comment server asserts the same vectors.
#[test]
fn digests_match_the_comment_servers_vectors() {
    assert_eq!(digest("spam.example"), "8bb6634ad532175e1c706063fd69b452bf51917f61150b14da6500c018cf8df4");
    assert_eq!(digest("resobang.example"), "93e292de116a637515877afab9b783fd388c9eaa0da0fc3a9968f5650fc1f82b");
}

#[test]
fn a_host_is_matched_exactly() {
    for (raw, host) in [
        ("https://WWW.Spam.Example:8443/x?y", "spam.example"),
        ("http://user@spam.example./", "spam.example"),
        ("https://bücher.example/", "xn--bcher-kva.example"),
        ("spam.example/register?aff=1", "spam.example"),
        ("https://forum.free-host.example/", "forum.free-host.example"),
    ] {
        assert_eq!(host_of(raw).as_deref(), Some(host), "{raw}");
    }
    for raw in ["", "mailto:a@spam.example", "javascript:alert(1)", "ftp://spam.example/"] {
        assert_eq!(host_of(raw), None, "{raw}");
    }
}

#[test]
fn hosts_come_from_the_website_field_links_and_bare_urls() {
    let c = comment(
        "1",
        "artalk",
        Some("www.site-field.example"),
        r#"see [here](https://md-link.example/a) and <a href="http://href.example/b">x</a>, or https://bare.example."#,
    );
    let hosts: Vec<String> = comment_hosts(&c).into_iter().collect();
    assert_eq!(hosts, ["bare.example", "href.example", "md-link.example", "site-field.example"]);
}

#[test]
fn the_latest_block_or_unblock_of_a_host_decides() {
    let o = owner();
    let stranger = owner();
    let events = vec![
        o.event("block", LINK_SOURCE, "spam.example", 1),
        o.event("block", LINK_SOURCE, "gone.example", 2),
        o.event("unblock", LINK_SOURCE, "gone.example", 3),
        stranger.event("block", LINK_SOURCE, "forged.example", 4),
        o.event("hide", "artalk", "spam.example", 5),
    ];
    let blocks = active_blocks(&events, std::slice::from_ref(&o.pubkey));
    assert_eq!(blocks.into_iter().collect::<Vec<_>>(), [("spam.example", 1)]);
}

#[test]
fn a_blocked_host_hides_comments_from_every_source_until_a_later_unhide() {
    let o = owner();
    let owners = std::slice::from_ref(&o.pubkey);
    let comments = vec![
        comment("1", "artalk", Some("https://spam.example"), "hi"),
        comment("2", "matters", None, "<p>visit https://www.spam.example/x</p>"),
        comment("3", "artalk", Some("https://friend.example"), "real"),
        comment("4", "artalk", None, "https://spam.example again"),
    ];
    let ids = |events: &[ModEvent]| resolve(&comments, events, owners).iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&[o.event("block", LINK_SOURCE, "spam.example", 2)]), ["3"]);
    // An unhide after the block shows that one comment; one before it does not.
    assert_eq!(
        ids(&[o.event("unhide", "artalk", "4", 1), o.event("block", LINK_SOURCE, "spam.example", 2), o.event("unhide", "artalk", "1", 3)]),
        ["1", "3"]
    );
}

#[test]
fn the_published_file_carries_host_digests_and_an_unverifiable_log_says_so() {
    let m = CommentModeration {
        events: 3,
        verified: 3,
        untrusted: vec![],
        blocked: ["spam.example".to_string()].into(),
    };
    let doc: serde_json::Value = serde_json::from_str(&m.published_json()).unwrap();
    assert_eq!(
        doc,
        serde_json::json!({ "version": 2, "blocked_hosts": ["8bb6634ad532175e1c706063fd69b452bf51917f61150b14da6500c018cf8df4"] })
    );
    let unverified = CommentModeration { events: 2, ..Default::default() };
    let doc: serde_json::Value = serde_json::from_str(&unverified.published_json()).unwrap();
    assert_eq!(doc["moderation"], "unverified");
    assert_eq!(doc["blocked_hosts"], serde_json::json!([]));
}

#[test]
fn moderation_reads_the_folders_blocks_and_never_its_own_host() {
    crate::infra::home::with_moss_home(|_home| {
        let d = project();
        let p = d.path();
        std::fs::write(p.join(".moss/config.toml"), "schema_version = 6\n\n[site]\nurl = \"https://www.own.example/\"\n").unwrap();
        let id = crate::identity::keypair::Identity::generate().unwrap();
        id.save(p).unwrap();
        let ps = p.to_str().unwrap();
        append_owner_event(ps, ModKind::Block, LINK_SOURCE, "spam.example", "", "localhost").unwrap();
        append_owner_event(ps, ModKind::Block, LINK_SOURCE, "own.example", "", "localhost").unwrap();
        let m = CommentModeration::load(ps);
        assert_eq!((m.events, m.verified, m.unverifiable()), (2, 2, false));
        assert_eq!(m.blocked.into_iter().collect::<Vec<_>>(), ["spam.example"]);
    });
}

/// A copy of the folder without `.moss/identity` has the log and no key.
#[test]
fn a_log_no_key_verifies_refuses_a_prebuilt_deploy() {
    crate::infra::home::with_moss_home(|_home| {
        let site = project();
        let ps = site.path().to_str().unwrap();
        let id = crate::identity::keypair::Identity::generate().unwrap();
        id.save(site.path()).unwrap();
        append_owner_event(ps, ModKind::Hide, "artalk", "7", "page", "localhost").unwrap();
        let out = project();
        assert_eq!(refuse_prebuilt(site.path(), out.path()), Ok(()), "another generator's tree has no comments.json");

        std::fs::create_dir_all(out.path().join("_moss")).unwrap();
        std::fs::write(out.path().join(COMMENTS_PATH), CommentModeration { events: 1, ..Default::default() }.published_json()).unwrap();
        assert!(refuse_prebuilt(site.path(), out.path()).unwrap_err().contains(".moss/identity/public.json"));

        std::fs::remove_dir_all(site.path().join(".moss/identity")).unwrap();
        assert!(CommentModeration::load(ps).unverifiable());
        assert!(refuse_prebuilt(site.path(), project().path()).unwrap_err().contains(".moss/identity/public.json"));
    });
}
