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
    let key = k256::schnorr::SigningKey::random(&mut rand::rngs::OsRng);
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
