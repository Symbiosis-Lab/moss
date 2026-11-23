use super::*;
use crate::build::features::comment::moderation::{load_mod_events, moderation_log_path};

fn site() -> tempfile::TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().join("target").join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let d = tempfile::TempDir::new_in(&base).unwrap();
    let social = d.path().join(".moss/data/social");
    std::fs::create_dir_all(&social).unwrap();
    // Invented comments: ids 1 and 2 on page "pg1" (2 replies to 1), id 5 under two sources.
    std::fs::write(
        social.join("comment.json"),
        r#"{"articles":{"pg1":{"comments":[
            {"id":"1","content":"<p>Cheap watches &amp; more at example.com</p>","createdAt":"2026-03-01T10:00:00Z","author":{"displayName":"Spammy Sam"}},
            {"id":"2","content":"<p>Buy now</p>","createdAt":"2026-03-02T10:00:00Z","author":{"displayName":"Spammy Sam"},"replyToId":"1"},
            {"id":"5","content":"<p>Lovely garden</p>","createdAt":"2026-03-03T10:00:00Z","author":{"displayName":"Ana Example"}}]}}}"#,
    )
    .unwrap();
    std::fs::write(
        social.join("matters.json"),
        r#"{"articles":{"pg1":{"comments":[
            {"id":"5","content":"<p>Nice read</p>","createdAt":"2026-03-04T10:00:00Z","author":{"displayName":"Ben Example"}}]}}}"#,
    )
    .unwrap();
    d
}

fn with_key(d: &tempfile::TempDir) -> String {
    let id = crate::identity::keypair::Identity::generate().unwrap();
    id.save(d.path()).unwrap();
    id.pubkey
}

fn run_in(d: &tempfile::TempDir, args: &[&str]) -> i32 {
    let mut all: Vec<String> = vec![args[0].into(), d.path().to_str().unwrap().into()];
    all.extend(args[1..].iter().map(|s| s.to_string()));
    crate::infra::home::with_moss_home(|_| run(&all))
}

fn events(d: &tempfile::TempDir) -> usize {
    load_mod_events(d.path().to_str().unwrap()).len()
}

#[test]
fn an_unknown_id_changes_nothing_even_beside_a_valid_one() {
    let d = site();
    with_key(&d);
    assert_eq!(run_in(&d, &["hide", "99"]), 1);
    assert_eq!(run_in(&d, &["hide", "1", "99"]), 1);
    assert_eq!(events(&d), 0, "no event for any id of a failed call");
    assert!(!moderation_log_path(d.path().to_str().unwrap()).exists());
}

#[test]
fn an_id_under_two_sources_asks_for_source() {
    let d = site();
    with_key(&d);
    assert_eq!(run_in(&d, &["hide", "5"]), 1);
    assert_eq!(events(&d), 0);
    assert_eq!(run_in(&d, &["hide", "5", "--source", "matters"]), 0);
    let ev = load_mod_events(d.path().to_str().unwrap());
    assert_eq!((ev.len(), ev[0].source.as_str(), ev[0].page.as_str()), (1, "matters", "pg1"));
}

#[test]
fn hiding_twice_appends_one_event_and_unhide_restores() {
    let d = site();
    let pk = with_key(&d);
    assert_eq!(run_in(&d, &["hide", "1"]), 0);
    assert_eq!(run_in(&d, &["hide", "1"]), 0);
    assert_eq!(events(&d), 1);
    assert!(load_mod_events(d.path().to_str().unwrap())[0].verify(&pk));
    assert_eq!(run_in(&d, &["unhide", "1"]), 0);
    assert_eq!(run_in(&d, &["unhide", "1"]), 0);
    assert_eq!(events(&d), 2);
    let (rows, _) = collect(d.path().to_str().unwrap());
    assert!(rows.iter().all(|r| !r.hidden));
}

#[test]
fn list_marks_hidden_comments_and_works_before_any_build() {
    let d = site();
    with_key(&d);
    assert_eq!(run_in(&d, &["hide", "1"]), 0);
    let (rows, unresolved) = collect(d.path().to_str().unwrap());
    assert!(unresolved, "no build yet: pages are named by uid");
    let hidden: Vec<(&str, bool)> = rows.iter().filter(|r| r.hidden).map(|r| (r.id.as_str(), r.directly_hidden)).collect();
    // The parent is hidden directly; its reply only through it.
    assert_eq!(hidden, vec![("2", false), ("1", true)]);
    assert_eq!(rows.iter().find(|r| r.id == "1").unwrap().text, "Cheap watches & more at example.com");
    assert_eq!(rows[0].id, "5", "newest first");
    assert_eq!(run_in(&d, &["list"]), 0);
    // A reply hidden only through its parent cannot be unhidden on its own.
    assert_eq!(run_in(&d, &["unhide", "2"]), 1);
}

#[test]
fn hiding_without_a_signing_key_fails_and_creates_none() {
    let d = site();
    assert_eq!(run_in(&d, &["hide", "1"]), 1);
    assert_eq!(events(&d), 0);
    assert!(!d.path().join(".moss/identity").exists());
}

#[test]
fn control_characters_never_reach_the_terminal_but_survive_in_json() {
    let d = site();
    std::fs::write(
        d.path().join(".moss/data/social/comment.json"),
        "{\"articles\":{\"pg1\":{\"comments\":[{\"id\":\"9\",\"content\":\"<p>wipe\\u001b[2J bell\\u0007 c1\\u009b alm\\u061c</p>\",\"createdAt\":\"2026-03-01T10:00:00Z\",\"author\":{\"displayName\":\"Eve\\u001b]0;pwned\\u0007\"}}]}}}",
    )
    .unwrap();
    let (rows, _) = collect(d.path().to_str().unwrap());
    let has_control = |s: &str| s.chars().any(|c| (c.is_control() && c != '\n') || c == '\u{61c}');
    assert!(!has_control(&render_table(&rows)), "list output");
    let eve = rows.iter().find(|r| r.id == "9").unwrap();
    assert!(!has_control(&eve.summary()), "hide summary");
    let json = eve.to_json();
    assert!(json["author"].as_str().unwrap().contains('\u{1b}'), "json keeps the real characters");
    assert!(json["text"].as_str().unwrap().contains('\u{7}'));
    assert!(!json.to_string().contains('\u{1b}'), "and serializes them escaped");
}

#[test]
fn listing_never_rewrites_an_old_format_identity() {
    let d = site();
    let id_dir = d.path().join(".moss/identity");
    std::fs::create_dir_all(&id_dir).unwrap();
    let key = <k256::schnorr::SigningKey as k256::elliptic_curve::Generate>::generate();
    let legacy = serde_json::json!({
        "version": 1,
        "pubkey": hex::encode(key.verifying_key().to_bytes()),
        "privkey": hex::encode(key.to_bytes()),
    });
    std::fs::write(id_dir.join("public.json"), legacy.to_string()).unwrap();
    let snapshot = |d: &Path| -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<_> = std::fs::read_dir(d)
            .unwrap()
            .map(|e| e.unwrap())
            .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap()))
            .collect();
        v.sort();
        v
    };
    let before = snapshot(&id_dir);
    assert_eq!(run_in(&d, &["list"]), 0);
    assert_eq!(snapshot(&id_dir), before, "list wrote to .moss/identity");
}

#[test]
fn adopt_key_brings_back_the_hides_of_a_key_lost_with_its_identity_folder() {
    let d = site();
    let old = with_key(&d);
    assert_eq!(run_in(&d, &["hide", "1"]), 0);
    // The whole of .moss/identity is lost and a new key made, so nothing remembers the old key.
    std::fs::remove_dir_all(d.path().join(".moss/identity")).unwrap();
    with_key(&d);
    let hidden = || collect(d.path().to_str().unwrap()).0.iter().any(|r| r.id == "1" && r.hidden);
    assert!(!hidden(), "the old key's hide no longer applies");

    assert_eq!(run_in(&d, &["adopt-key", &old]), 0);
    assert!(hidden(), "adopted, the hide applies again");
    assert_ne!(run_in(&d, &["adopt-key", &old]), 0, "a trusted key is refused");
    assert_ne!(run_in(&d, &["adopt-key"]), 0, "a key is required");
}

/// Invented spam: 10 and 11 link to spam.example (10 also gives the site's own
/// address), 12 is a real comment, 13 a real comment the owner hid and unhid
/// that also links to shared.example.
fn spam_site() -> tempfile::TempDir {
    let d = site();
    std::fs::write(d.path().join(".moss/config.toml"), "schema_version = 6\n\n[site]\nurl = \"https://own.example/\"\n").unwrap();
    std::fs::write(
        d.path().join(".moss/data/social/douban.json"),
        r#"{"articles":{"pg2":{"comments":[
            {"id":"10","content":"<p>see https://spam.example/x and https://shared.example/</p>","createdAt":"2026-03-05T10:00:00Z","author":{"displayName":"S","url":"https://www.own.example/"}},
            {"id":"11","content":"<p>hi</p>","createdAt":"2026-03-06T10:00:00Z","author":{"displayName":"S2","url":"http://spam.example"}},
            {"id":"12","content":"<p>real</p>","createdAt":"2026-03-07T10:00:00Z","author":{"displayName":"R","url":"https://friend.example"}},
            {"id":"13","content":"<p>me too https://shared.example</p>","createdAt":"2026-03-08T10:00:00Z","author":{"displayName":"R2"}}]}}}"#,
    )
    .unwrap();
    d
}

#[test]
fn spam_hides_blocks_its_hosts_and_undo_reverses_all_of_it() {
    let d = spam_site();
    with_key(&d);
    assert_eq!(run_in(&d, &["hide", "13"]), 0);
    assert_eq!(run_in(&d, &["unhide", "13"]), 0);
    let ps = d.path().to_str().unwrap().to_string();
    let outcome = crate::infra::home::with_moss_home(|_| mark_spam(&ps, &[("douban".into(), "10".into())])).unwrap();
    assert_eq!(outcome.hidden, [("douban".to_string(), "10".to_string())]);
    assert_eq!(outcome.blocked.iter().collect::<Vec<_>>(), ["spam.example"], "never the site's own host");
    assert_eq!(outcome.not_blocked.iter().collect::<Vec<_>>(), ["shared.example"], "a comment the owner unhid links to it");
    assert_eq!(outcome.also_hidden, 1);
    assert_eq!(outcome.line(), "Spam · spam.example blocked · 1 more comment hidden");
    let hidden = |ps: &str| collect(ps).0.into_iter().filter(|r| r.hidden).map(|r| r.id).collect::<BTreeSet<_>>();
    assert_eq!(hidden(&ps), ["10".to_string(), "11".to_string()].into());

    crate::infra::home::with_moss_home(|_| undo_spam(&ps, &outcome)).unwrap();
    assert!(hidden(&ps).is_empty());
    assert!(blocked_now(&ps).is_empty());
}

#[test]
fn spam_on_a_hidden_comment_still_blocks_and_unblock_lifts_it() {
    let d = spam_site();
    with_key(&d);
    assert_eq!(run_in(&d, &["hide", "10", "11"]), 0);
    assert_eq!(run_in(&d, &["spam", "11"]), 0);
    let ps = d.path().to_str().unwrap();
    assert_eq!(blocked_now(ps), ["spam.example"]);
    assert_eq!(run_in(&d, &["unblock", "nothing.example"]), 1);
    assert_eq!(run_in(&d, &["unblock", "https://WWW.spam.example/"]), 0);
    assert!(blocked_now(ps).is_empty());
}

#[tokio::test]
async fn blocked_shows_each_hosts_drop_count_from_the_hosting_service() {
    let d = spam_site();
    with_key(&d);
    assert_eq!(run_in(&d, &["spam", "11"]), 0);
    let hosts = blocked_now(d.path().to_str().unwrap());
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/api/sites/site-x/comments/blocked")
        .with_status(200)
        .with_body(r#"{"hosts":[{"host":"spam.example","dropped":14,"last":1795435200},{"host":"gone.example","dropped":3,"last":1795435200}]}"#)
        .create_async()
        .await;
    let identity = crate::identity::keypair::Identity::load_for_signing(d.path()).unwrap();
    let client = crate::seta::client::MossSetaClient::with_identity_and_url(&identity, &server.url());
    assert_eq!(
        blocked_report(&hosts, &client, "site-x", false).await,
        "New comments dropped in the last 90 days:\nspam.example · 14 dropped · last 2026-11-23\n"
    );
    let json: serde_json::Value = serde_json::from_str(&blocked_report(&hosts, &client, "site-x", true).await).unwrap();
    assert_eq!(json, serde_json::json!({ "hosts": [{ "host": "spam.example", "dropped": 14, "last": "2026-11-23" }], "drops_unavailable": null }));
    mock.expect(2).assert_async().await;
}

#[test]
fn blocked_lists_hosts_when_counts_are_unavailable() {
    let hosts = ["quiet.example".to_string(), "spam.example".to_string()];
    let ok = || Ok(vec![HostDrops { host: "spam.example".into(), dropped: 2, last: 1795435200 }]);
    assert_eq!(
        render_blocked(&hosts, ok(), false),
        "New comments dropped in the last 90 days:\nquiet.example · 0 dropped\nspam.example · 2 dropped · last 2026-11-23\n"
    );
    assert_eq!(
        render_blocked(&hosts, Err("no site_id — deploy this folder first (moss deploy <folder>)".into()), false),
        "quiet.example\nspam.example\nDrop counts unavailable: no site_id — deploy this folder first (moss deploy <folder>)\n"
    );
    assert_eq!(render_blocked(&[], Ok(vec![]), false), "No blocked hosts.\n");
    assert_eq!(render_blocked(&[], Ok(vec![]), true), "{\"drops_unavailable\":null,\"hosts\":[]}\n");
}
