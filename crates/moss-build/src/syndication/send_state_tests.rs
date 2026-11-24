//! What a send does with a copy in each state, whatever its channel answers.

use super::retry_tests::{channel, posted, publishes, run, site, NEVER, SOURCE, UID, URL};
use super::*;

const FOUND: &str = "https://social.example.net/found";

fn finds(answer: &str) -> String {
    format!("find_copy: async function () {{ return {answer}; }}")
}

fn finds_a_copy() -> String {
    finds(&format!("{{ success: true, copy: {{ id: 'f', url: '{FOUND}' }} }}"))
}

/// A channel that numbers its posts, so a second publish shows as `/2`.
fn counting_publisher() -> String {
    "publish_copy: (() => { let n = 0; return async function () { n += 1; return { success: true, copy: { id: String(n), url: 'https://social.example.net/' + n } }; }; })()".into()
}

#[tokio::test]
async fn an_interrupted_send_on_a_channel_without_a_key_is_adopted_when_the_network_holds_the_copy() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Sending, |_| {});
    channel(root, &format!("{}, {}", publishes("p"), finds_a_copy()));
    assert_eq!(run(root, &articles, false, None).await, [Outcome::Posted { url: FOUND.into() }]);
    let r = records.get(UID, "social").unwrap();
    assert_eq!((r.state, r.remote.map(|c| c.url)), (State::Posted, Some(FOUND.into())));
    assert!(std::fs::read_to_string(root.join(SOURCE)).unwrap().contains(FOUND), "syndicated: names the adopted copy");
}

#[tokio::test]
async fn an_unconfirmed_copy_is_adopted_when_the_network_holds_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Unconfirmed, |r| r.reason = Some("lost answer".into()));
    channel(root, &format!("{}, {}", publishes("p"), finds_a_copy()));
    assert_eq!(run(root, &articles, false, None).await, [Outcome::Posted { url: FOUND.into() }]);
    let r = records.get(UID, "social").unwrap();
    assert_eq!((r.state, r.reason, r.remote.map(|c| c.url)), (State::Posted, None, Some(FOUND.into())));
}

#[tokio::test]
async fn an_interrupted_send_the_network_has_no_copy_of_is_published_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Sending, |_| {});
    channel(root, &format!("{}, {}", counting_publisher(), finds("{ success: true, copy: { absent: true } }")));
    let first = Outcome::Posted { url: "https://social.example.net/1".into() };
    assert_eq!(run(root, &articles, false, None).await, [first]);
    assert_eq!(records.get(UID, "social").unwrap().state, State::Posted);
    assert!(run(root, &articles, false, None).await.is_empty(), "nothing is sent a second time");
    assert_eq!(records.get(UID, "social").unwrap().remote.unwrap().id, "1");
}

#[tokio::test]
async fn an_interrupted_send_whose_lookup_fails_stays_unconfirmed_and_sends_nothing() {
    for lookup in [finds("{ success: false, message: 'down' }"), "find_copy: async function () { throw new Error('offline'); }".into(), "find_copy: undefined".to_string()] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (records, articles) = site(root, State::Sending, |_| {});
        // A publish that succeeded would show as Posted, so a held copy proves it was never called.
        channel(root, &format!("{}, {lookup}", publishes("p")));
        let out = run(root, &articles, false, None).await;
        assert!(matches!(&out[..], [Outcome::Unconfirmed { .. }]), "{lookup}: {out:?}");
        let r = records.get(UID, "social").unwrap();
        assert!(r.state == State::Unconfirmed && r.remote.is_none(), "{lookup}: {r:?}");
    }
}

#[tokio::test]
async fn an_unconfirmed_copy_found_waiting_as_a_draft_is_handed_off_and_not_published() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Unconfirmed, |_| {});
    let draft = "{ success: true, copy: { handoff: { text: 'Body', open_url: 'https://social.example.net/draft', id: 'd-1' } } }";
    channel(root, &format!("{}, {}", publishes("p"), finds(draft)));
    let out = run(root, &articles, false, None).await;
    assert_eq!(out, [Outcome::HandedOff { text: "Body".into(), open_url: Some("https://social.example.net/draft".into()) }]);
    let r = records.get(UID, "social").unwrap();
    assert_eq!((r.state, r.remote, r.handoff.unwrap().id), (State::HandedOff, None, Some("d-1".into())));
}

#[tokio::test]
async fn an_edit_that_leaves_the_copy_unchanged_sends_nothing_and_clears_the_pending_update() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Due, |_| {});
    channel(root, &publishes("1"));
    run(root, &articles, false, None).await;
    let mut r = records.get(UID, "social").unwrap();
    assert_eq!(r.state, State::Posted);
    r.update_due = true;
    records.save(&r).unwrap();
    channel(root, &format!("update_copy: {NEVER}"));
    assert_eq!(run(root, &articles, false, None).await, [Outcome::Unchanged]);
    let r = records.get(UID, "social").unwrap();
    assert!(r.state == State::Posted && !r.update_due && r.remote.unwrap().url == URL);
}

#[tokio::test]
async fn an_update_the_network_never_answers_is_refused_and_tried_again_next_run() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Posted, posted);
    channel(root, "update_copy: async function () { throw new Error('offline'); }");
    let out = run(root, &articles, false, None).await;
    assert!(matches!(&out[..], [Outcome::Refused { cause: Cause::Unknown, reason }] if reason.contains("offline")), "{out:?}");
    let r = records.get(UID, "social").unwrap();
    assert!(r.state == State::Posted && r.update_due && r.remote.unwrap().url == URL);
    channel(root, "update_copy: async function () { return { success: true, copy: { id: '1', url: 'https://social.example.net/1' } }; }");
    assert_eq!(run(root, &articles, false, None).await, [Outcome::Updated { url: URL.into() }]);
}

#[tokio::test]
async fn a_credential_that_expired_at_publish_is_refused_for_auth_and_publishes_next_run() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Due, |r| r.form = None);
    let denied = "publish_copy: async function () { return { success: false, copy: { refused: { cause: 'auth', message: 'token expired' } } }; }";
    channel(root, denied);
    assert_eq!(run(root, &articles, false, None).await, [Outcome::Refused { cause: Cause::Auth, reason: "token expired".into() }]);
    let r = records.get(UID, "social").unwrap();
    assert!(r.state == State::Refused && r.remote.is_none(), "{r:?}");
    channel(root, &publishes("p"));
    assert_eq!(run(root, &articles, false, None).await, [Outcome::Posted { url: "https://social.example.net/p".into() }]);
    assert_eq!(records.get(UID, "social").unwrap().state, State::Posted);
}

#[tokio::test]
async fn a_page_that_does_not_answer_holds_its_copy_and_nothing_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Due, |_| {});
    channel(root, &publishes("p"));
    let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
    let emit = |_: &Sent| {};
    // Nothing listens on port 1, so the page cannot answer.
    let opts = SendOptions { check_pages: true, ..SendOptions::test("http://127.0.0.1:1", &emit) };
    let out: Vec<Outcome> = send_due(root, &manager, &articles, &opts).await.into_iter().map(|s| s.outcome).collect();
    assert!(matches!(&out[..], [Outcome::Held { why: HeldWhy::PageNotAnswering, .. }]), "{out:?}");
    let r = records.get(UID, "social").unwrap();
    assert!(r.state == State::Due && r.remote.is_none(), "{r:?}");
}

#[tokio::test]
async fn a_post_missing_from_the_latest_build_is_held_when_due_and_unconfirmed_when_interrupted() {
    for (state, held) in [(State::Due, true), (State::Sending, false)] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (records, _) = site(root, state, |_| {});
        channel(root, &format!("{}, {}", publishes("p"), finds_a_copy()));
        let out = run(root, &ArticleMap::default(), false, None).await;
        let r = records.get(UID, "social").unwrap();
        if held {
            assert!(matches!(&out[..], [Outcome::Held { why: HeldWhy::NotPublished, .. }]), "{out:?}");
            assert_eq!(r.state, State::Due);
        } else {
            assert!(matches!(&out[..], [Outcome::Unconfirmed { .. }]), "{out:?}");
            assert_eq!(r.state, State::Unconfirmed);
        }
        assert!(r.remote.is_none());
    }
}
