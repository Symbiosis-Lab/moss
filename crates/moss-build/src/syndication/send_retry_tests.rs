//! A retry overwrites: one copy per post per channel, rewritten in place.

use super::*;
use crate::syndication::record::{Handoff, RemoteRef, Stays};

pub(super) const SOURCE: &str = "a.md";
pub(super) const UID: &str = "aaaa0001";
pub(super) const URL: &str = "https://social.example.net/1";
const DRAFT: &str = "{ success: true, copy: { handoff: { text: '', open_url: 'https://social.example.net/draft', id: 'd-2' } } }";
pub(super) const NEVER: &str = "async function () { throw new Error('must not be called'); }";

pub(super) fn channel(root: &Path, ops: &str) {
    let js = format!("{{ {ops} }}");
    if root.join(".moss/plugins/social").exists() {
        std::fs::write(root.join(".moss/plugins/social/main.bundle.js"), format!("window.SOCIALPlugin = {js};")).unwrap();
    } else {
        super::super::test_channel(root, "social", &js);
    }
}

/// A site with one post and a record of it in `state`, shaped by `tweak`.
pub(super) fn site(root: &Path, state: State, tweak: impl FnOnce(&mut CopyRecord)) -> (Records, ArticleMap) {
    std::fs::write(root.join(SOURCE), "---\ntitle: A\n---\nBody.\n").unwrap();
    let records = Records::in_vault(&crate::moss_paths::MossPaths::new(root));
    let mut r = CopyRecord::due(UID, "social", SOURCE, "a/");
    r.state = state;
    r.form = Some(super::super::channel::Form::Short);
    tweak(&mut r);
    records.save(&r).unwrap();
    let post: ArticleInfo = serde_json::from_value(serde_json::json!({
        "source_path": SOURCE, "title": "A", "content": "Body.\n", "url_path": "a/", "date": null, "uid": UID, "syndicate": ["social"]
    })).unwrap();
    let mut articles = ArticleMap::default();
    articles.articles.insert(post.url_path.clone(), post);
    (records, articles)
}

pub(super) fn posted(r: &mut CopyRecord) {
    r.remote = Some(RemoteRef { id: "1".into(), url: URL.into() });
    r.update_due = true;
}

fn drafted(r: &mut CopyRecord) {
    r.handoff = Some(Handoff { text: String::new(), open_url: Some("https://social.example.net/draft".into()), id: Some("d-1".into()) });
}

/// A run; `press` is the retry of this one post's copy on the channel.
pub(super) async fn run(root: &Path, articles: &ArticleMap, retry: bool, only_source: Option<&str>) -> Vec<Outcome> {
    let manager = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned()).get_or_create(&root.to_string_lossy()).unwrap();
    let emit = |_: &Sent| {};
    let channels = ["social".to_string()];
    let opts = SendOptions { retry: if retry { &channels } else { &[] }, only_source, ..SendOptions::test("https://example.org", &emit) };
    send_due(root, &manager, articles, &opts).await.into_iter().map(|s| s.outcome).filter(|o| *o != Outcome::Sending).collect()
}

pub(super) fn refusal(cause: &str) -> String {
    format!("update_copy: async function () {{ return {{ success: false, copy: {{ refused: {{ cause: '{cause}' }} }} }}; }}")
}

pub(super) fn publishes(id: &str) -> String {
    format!("publish_copy: async function () {{ return {{ success: true, copy: {{ id: '{id}', url: 'https://social.example.net/{id}' }} }}; }}")
}

#[tokio::test]
async fn a_locked_refusal_keeps_the_copy_and_is_said_once_per_page_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Posted, posted);
    channel(root, &refusal("locked"));
    assert_eq!(run(root, &articles, false, None).await, [Outcome::AlreadyPosted { url: URL.into() }]);
    let r = records.get(UID, "social").unwrap();
    assert!(r.stays == Some(Stays::Locked) && !r.update_due && r.remote.as_ref().unwrap().url == URL && r.state == State::Posted);
    assert!(run(root, &articles, false, None).await.is_empty(), "no page change, nothing said");

    channel(root, &format!("update_copy: {NEVER}"));
    let mut again = records.get(UID, "social").unwrap();
    again.update_due = true;
    records.save(&again).unwrap();
    assert_eq!(run(root, &articles, false, None).await, [Outcome::AlreadyPosted { url: URL.into() }], "a new page change meets the locked copy");
    assert!(!records.get(UID, "social").unwrap().update_due);
}

#[tokio::test]
async fn a_channel_without_update_copy_says_so_once_and_updates_after_it_gains_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Posted, posted);
    channel(root, &publishes("p"));
    assert_eq!(run(root, &articles, false, None).await, [Outcome::AlreadyPosted { url: URL.into() }]);
    let r = records.get(UID, "social").unwrap();
    assert!(r.update_due && r.stays.is_none(), "the change is kept; only a refusal locks");
    assert!(run(root, &articles, false, None).await.is_empty(), "said on the run that met the change");

    channel(root, "update_copy: async function () { return { success: true, copy: { id: '1', url: 'https://social.example.net/1' } }; }");
    assert_eq!(run(root, &articles, false, None).await, [Outcome::Updated { url: URL.into() }]);
    assert!(!records.get(UID, "social").unwrap().update_due);
}

#[tokio::test]
async fn a_retry_rewrites_the_draft_in_place_and_makes_no_second_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::HandedOff, drafted);
    channel(root, &format!("publish_copy: {NEVER}, find_copy: async function () {{ return {{ success: true, copy: null }}; }}, update_copy: async function () {{ return {DRAFT_SAME}; }}", DRAFT_SAME = DRAFT.replace("d-2", "d-1")));
    let out = run(root, &articles, true, Some(SOURCE)).await;
    assert!(matches!(&out[..], [Outcome::HandedOff { .. }]), "{out:?}");
    assert_eq!(records.get(UID, "social").unwrap().handoff.unwrap().id.as_deref(), Some("d-1"));
}

#[tokio::test]
async fn a_bulk_retry_leaves_a_draft_and_a_locked_copy_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::HandedOff, drafted);
    channel(root, &format!("publish_copy: {NEVER}, find_copy: {NEVER}, update_copy: {NEVER}"));
    let before = records.get(UID, "social").unwrap();
    assert!(run(root, &articles, true, None).await.is_empty());
    assert_eq!(records.get(UID, "social").unwrap(), before);

    let (records, articles) = site(root, State::Posted, |r| { posted(r); r.stays = Some(Stays::Locked); r.update_due = false; });
    let before = records.get(UID, "social").unwrap();
    assert!(run(root, &articles, true, None).await.is_empty());
    assert_eq!(records.get(UID, "social").unwrap(), before);
}

#[tokio::test]
async fn a_draft_deleted_on_the_network_is_made_again() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::HandedOff, drafted);
    channel(root, &format!("find_copy: async function () {{ return {{ success: true, copy: null }}; }}, update_copy: async function () {{ return {{ success: true, copy: {{ absent: true }} }}; }}, publish_copy: async function () {{ return {DRAFT}; }}"));
    let out = run(root, &articles, true, Some(SOURCE)).await;
    assert!(matches!(&out[..], [Outcome::HandedOff { .. }]), "{out:?}");
    let r = records.get(UID, "social").unwrap();
    assert_eq!((r.state, r.handoff.unwrap().id.as_deref().map(str::to_string)), (State::HandedOff, Some("d-2".into())));
}

#[tokio::test]
async fn a_published_copy_deleted_on_the_network_is_not_posted_again_by_itself() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::Posted, posted);
    channel(root, &format!("update_copy: async function () {{ return {{ success: true, copy: {{ absent: true }} }}; }}, publish_copy: {NEVER}"));
    assert_eq!(run(root, &articles, false, None).await, [Outcome::Deleted { url: URL.into() }]);
    let r = records.get(UID, "social").unwrap();
    assert!(r.stays == Some(Stays::Deleted) && r.remote.is_some());
    assert!(run(root, &articles, false, None).await.is_empty());
    assert!(run(root, &articles, true, None).await.is_empty(), "a bulk retry never reposts");
}

#[tokio::test]
async fn the_press_posts_a_locked_or_deleted_copy_again_once() {
    for locked_before in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (records, articles) = site(root, State::Posted, |r| {
            posted(r);
            if locked_before { r.stays = Some(Stays::Deleted); r.update_due = false; }
        });
        channel(root, &format!("{}, {}", publishes("p"), refusal("locked")));
        let out = run(root, &articles, true, Some(SOURCE)).await;
        assert_eq!(out, [Outcome::Posted { url: "https://social.example.net/p".into() }], "locked before: {locked_before}");
        let r = records.get(UID, "social").unwrap();
        assert!(r.stays.is_none() && r.remote.unwrap().id == "p");
        assert!(run(root, &articles, false, None).await.is_empty());
    }
}

/// Publish of one post with a channel on: every copy it will not send says why.
#[tokio::test]
async fn a_one_post_publish_says_why_it_sends_nothing() {
    let absent = "find_copy: async function () { return { success: true, copy: { absent: true } }; }";
    let unknown = "find_copy: async function () { throw new Error('lookup down'); }";
    for (case, state, lookup) in [("hand-off, draft gone", State::HandedOff, absent), ("hand-off, lookup down", State::HandedOff, unknown), ("unconfirmed", State::Unconfirmed, unknown)] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let (_, articles) = site(root, state, |r| if state == State::HandedOff {
            r.handoff = Some(Handoff { text: "Post it".into(), open_url: None, id: None });
            r.redrafted = true;
        });
        channel(root, &format!("publish_copy: {NEVER}, update_copy: {NEVER}, {lookup}"));
        let out = run(root, &articles, false, Some(SOURCE)).await;
        assert!(matches!(&out[..], [Outcome::HandedOff { .. }] | [Outcome::Unconfirmed { .. }]), "{case}: {out:?}");
    }

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, mut articles) = site(root, State::Due, |_| {});
    articles.articles.get_mut("a/").unwrap().syndicate = Some(vec![]);
    channel(root, &format!("publish_copy: {NEVER}"));
    let before = records.get(UID, "social").unwrap();
    assert!(matches!(&run(root, &articles, false, Some(SOURCE)).await[..], [Outcome::Held { why: HeldWhy::NotNamed, .. }]));
    assert!(run(root, &articles, false, None).await.is_empty(), "a deploy says nothing of a channel the page dropped");
    assert_eq!(records.get(UID, "social").unwrap(), before);
}

/// A deploy that changed the page meets a hand-off whose draft is gone: said once, then quiet.
#[tokio::test]
async fn a_changed_page_says_a_gone_draft_waits_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let (records, articles) = site(root, State::HandedOff, |r| {
        r.handoff = Some(Handoff { text: "Post it".into(), open_url: None, id: None });
        r.redrafted = true;
        r.update_due = true;
    });
    channel(root, &format!("publish_copy: {NEVER}, update_copy: {NEVER}, find_copy: async function () {{ return {{ success: true, copy: {{ absent: true }} }}; }}"));
    assert!(matches!(&run(root, &articles, false, None).await[..], [Outcome::HandedOff { .. }]));
    let r = records.get(UID, "social").unwrap();
    assert!(r.state == State::HandedOff && !r.update_due, "{r:?}");
    assert!(run(root, &articles, false, None).await.is_empty());
}
