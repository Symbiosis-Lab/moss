//! Moderation decides what a build shows only when a key of the folder's own
//! verifies it. A copy of the folder without `.moss/identity` builds (the
//! preview still renders, with a problem) and is refused at publish, by every
//! route, so it can never ship hidden comments or an empty block list.

use crate::site;

use moss_build::build::features::comment::link_block::{refuse_prebuilt, COMMENTS_PATH};
use moss_build::build::features::comment::moderation::sign_mod_event;
use moss_build::deploy::{publish_refusal, RefusalKind};
use moss_build::identity::keypair::Identity;
use site::Site;

const PAGE: &str = "---\ntitle: Harbour log\ndate: 2026-01-01\nuid: ab12cd34\ncomments: true\n---\n\nThe boats came in.\n";
const COMMENTS: &str = r#"{"schemaVersion":"1.1.0","articles":{"ab12cd34":{"comments":[
{"id":"1","source":"artalk","content":"Cheap pills","createdAt":"2026-01-02","author":{"displayName":"S","url":"https://spam.example/"}},
{"id":"2","source":"artalk","content":"More at https://spam.example/x","createdAt":"2026-01-03","author":{"displayName":"S2"}},
{"id":"3","source":"artalk","content":"Lovely harbour","createdAt":"2026-01-04","author":{"displayName":"Reader","url":"https://reader.example/"}}]}}}"#;

fn moderated_site() -> Site {
    let site = Site::with_config("[services.comments]\nenabled = true\n", &[
        ("index.md", "# Home\n"),
        ("log.md", PAGE),
        (".moss/data/social/comment.json", COMMENTS),
    ]);
    let id = Identity::generate().unwrap();
    id.save(&site.root).unwrap();
    let key = id.signing_key_loaded().unwrap();
    let events = [
        sign_mod_event(key, "hide", "artalk", "1", "localhost", "ab12cd34", 1, 1),
        sign_mod_event(key, "block", "link", "spam.example", "localhost", "", 2, 2),
    ];
    let log: String = events.iter().map(|e| serde_json::to_string(e).unwrap() + "\n").collect();
    site.write(".moss/data/social/moderation.jsonl", &log);
    site.rebuild();
    site
}

#[test]
fn a_verified_log_hides_blocked_hosts_and_publishes_their_digests() {
    let site = moderated_site();
    let html = site.html("log/index.html");
    assert!(html.contains("Lovely harbour"), "{html}");
    assert!(!html.contains("Cheap pills") && !html.contains("More at"), "hidden and blocked comments are not built");
    let published: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(site.output(COMMENTS_PATH)).unwrap()).unwrap();
    assert_eq!(published["blocked_hosts"].as_array().map(Vec::len), Some(1));
    assert!(!published.to_string().contains("spam.example"), "the published list is not readable");
    let refusal = publish_refusal(&site.root.to_string_lossy());
    assert!(refusal.as_ref().is_none_or(|r| r.kind != RefusalKind::ModerationUnverified), "{refusal:?}");
}

#[test]
fn a_copy_without_its_identity_builds_but_never_publishes() {
    let site = moderated_site();
    std::fs::remove_dir_all(site.root.join(".moss/identity")).unwrap();
    site.rebuild();
    assert!(site.html("log/index.html").contains("Cheap pills"), "the preview still renders");
    let refusal = publish_refusal(&site.root.to_string_lossy()).expect("publish is refused");
    assert_eq!(refusal.kind, RefusalKind::ModerationUnverified);
    assert!(refusal.text.contains(".moss/identity/public.json"), "{}", refusal.text);

    // The tree that copy built cannot be carried by a `--prebuilt` deploy
    // from the real folder, which has its key again.
    let real = moderated_site();
    let err = refuse_prebuilt(&real.root, &site.output("")).unwrap_err();
    assert!(err.contains(".moss/identity/public.json"), "{err}");
    assert_eq!(refuse_prebuilt(&real.root, &real.output("")), Ok(()));
}

#[test]
fn a_prebuilt_dry_run_reports_the_refusal_the_upload_meets() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let dry_run = |folder: &Site, tree: &Site| {
        rt.block_on(moss_build::deploy::dry_run::run_prebuilt_dry_run(&folder.root, &tree.output(""), None, false, false)).unwrap()
    };
    // A first publish needs a hosting environment; local keeps the server unasked.
    let publishable = |site: Site| {
        let config = site.root.join(".moss/config.toml");
        let text = std::fs::read_to_string(&config).unwrap();
        std::fs::write(&config, format!("environment = \"local\"\n{text}")).unwrap();
        site
    };
    let copy = publishable(moderated_site());
    std::fs::remove_dir_all(copy.root.join(".moss/identity")).unwrap();
    copy.rebuild();
    let real = publishable(moderated_site());

    let dry = dry_run(&real, &copy);
    assert_eq!(dry.refusal.map(|r| r.kind), Some(RefusalKind::ModerationUnverified));
    let dry = dry_run(&copy, &real);
    assert_eq!(dry.refusal.map(|r| r.kind), Some(RefusalKind::ModerationUnverified));
    let dry = dry_run(&real, &real);
    assert!(dry.refusal.as_ref().is_none_or(|r| r.kind != RefusalKind::ModerationUnverified), "{:?}", dry.refusal);
}
