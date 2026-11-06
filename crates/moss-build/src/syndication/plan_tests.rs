use super::*;
use std::collections::HashMap;

fn post(source: &str, url: &str, uid: &str, syndicate: &[&str], fm: serde_json::Value) -> ArticleInfo {
    ArticleInfo {
        source_path: source.into(),
        title: String::new(),
        content: "Hello.\n".into(),
        html_content: None,
        frontmatter: fm.as_object().cloned().unwrap_or_default().into_iter().collect::<HashMap<_, _>>(),
        url_path: url.into(),
        date: None,
        tags: vec![],
        uid: Some(uid.into()),
        syndicate: Some(syndicate.iter().map(|s| s.to_string()).collect()),
        untitled: false,
    }
}

fn map(posts: Vec<ArticleInfo>) -> ArticleMap {
    let mut m = ArticleMap::default();
    for p in posts {
        m.articles.insert(p.url_path.clone(), p);
    }
    m
}

fn channels() -> BTreeMap<String, ChannelInfo> {
    BTreeMap::from([
        ("bluesky".to_string(), ChannelInfo { domains: vec!["bsky.app".into()], ..Default::default() }),
        ("mastodon".to_string(), ChannelInfo { domains: vec!["example.social".into()], ..Default::default() }),
    ])
}

fn live(uid: &str, url: &str) -> LiveEntry {
    LiveEntry { uid: uid.into(), url: url.into(), source_path: String::new(), title: String::new() }
}

fn actions(plans: &[PostPlan]) -> Vec<(String, String, Action)> {
    plans.iter().flat_map(|p| p.entries.iter().map(|e| (e.url_path.clone(), e.channel.clone(), e.action.clone()))).collect()
}

#[test]
fn a_new_post_is_due_on_every_channel_it_names() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["bluesky", "mastodon"], serde_json::json!({}))]);
    let plans = decide(&PlanInputs { articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    assert_eq!(
        actions(&plans),
        [("notes/a/".into(), "bluesky".into(), Action::Due), ("notes/a/".into(), "mastodon".into(), Action::Due)]
    );
    assert!(plans[0].stamp_uid, "a page with no stored uid gets one before any copy is due");
}

#[test]
fn a_post_live_before_its_folder_named_the_channel_waits_unless_it_names_it_itself() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let m = map(vec![
        post("notes/old.md", "notes/old/", "0a0a0a0a", &["bluesky"], serde_json::json!({"uid": "0a0a0a0a"})),
        post("notes/own.md", "notes/own/", "0b0b0b0b", &["bluesky"], serde_json::json!({"uid": "0b0b0b0b", "syndicate": ["bluesky"]})),
        // A page with no stored uid has a new one each build, so its address says it was live.
        post("notes/bare.md", "notes/bare/", "0c0c0c0c", &["bluesky"], serde_json::json!({})),
    ]);
    let before = [live("0a0a0a0a", "notes/old/"), live("0b0b0b0b", "notes/own/"), live("99999999", "notes/bare/")];
    let plans = decide(&PlanInputs { articles: &m, previously_live: Some(&before), edited_sources: &HashSet::new(), channels: &channels() }, &records);
    let got = actions(&plans);
    assert!(matches!(got[0].2, Action::Skip { .. }), "{got:?}");
    assert!(matches!(got[1].2, Action::Skip { .. }), "{got:?}");
    assert_eq!(got[2].2, Action::Due, "naming the channel in the page itself posts an old page");
}

#[test]
fn a_recorded_copy_is_not_planned_again_and_an_edit_marks_it_for_update() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let mut posted = CopyRecord::due("aa11aa11", "bluesky", "notes/a.md", "notes/a/");
    posted.state = State::Posted;
    records.save(&posted).unwrap();
    records.save(&CopyRecord::due("aa11aa11", "mastodon", "notes/a.md", "notes/a/")).unwrap();
    std::fs::write(dir.path().join("aa11aa11.mastodon.json"), "garbage").unwrap();

    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["bluesky", "mastodon"], serde_json::json!({"uid": "aa11aa11"}))]);
    let unchanged = decide(&PlanInputs { articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    assert!(actions(&unchanged).is_empty(), "an unreadable record still blocks a repost: {unchanged:?}");

    let edited = HashSet::from(["notes/a.md".to_string()]);
    let changed = decide(&PlanInputs { articles: &m, previously_live: None, edited_sources: &edited, channels: &channels() }, &records);
    assert_eq!(actions(&changed), [("notes/a/".into(), "bluesky".into(), Action::Update)]);
}

#[test]
fn a_copy_already_listed_in_syndicated_is_adopted_not_posted() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let fm = serde_json::json!({"uid": "aa11aa11", "syndicated": ["https://bsky.app/profile/a/post/1"]});
    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["bluesky"], fm)]);
    let plans = decide(&PlanInputs { articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    assert_eq!(actions(&plans)[0].2, Action::Adopt { url: "https://bsky.app/profile/a/post/1".into() });

    let applied = apply(dir.path(), plans, &records, None);
    assert_eq!(applied.len(), 1);
    assert_eq!(records.get("aa11aa11", "bluesky").unwrap().state, State::Posted);
}

/// The same address spelled another way is the same copy: the host's case,
/// its default port and a trailing dot do not make it a stranger.
#[test]
fn a_copy_listed_with_another_spelling_of_its_host_is_adopted() {
    for listed in ["https://BSKY.APP/profile/a/post/1", "https://bsky.app:443/profile/a/post/1", "https://bsky.app./profile/a/post/1"] {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path().into());
        let fm = serde_json::json!({"uid": "aa11aa11", "syndicated": [listed]});
        let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["bluesky"], fm)]);
        let plans = decide(&PlanInputs { articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
        assert_eq!(actions(&plans)[0].2, Action::Adopt { url: listed.into() }, "{listed}");
    }
}

#[test]
fn a_copy_listed_as_one_address_is_adopted_too() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let fm = serde_json::json!({"uid": "aa11aa11", "syndicated": "https://bsky.app/profile/a/post/1"});
    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["bluesky"], fm)]);
    let plans = decide(&PlanInputs { articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    assert_eq!(actions(&plans)[0].2, Action::Adopt { url: "https://bsky.app/profile/a/post/1".into() });
}

#[test]
fn a_channel_nobody_set_up_is_reported_and_false_plans_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let m = map(vec![
        post("notes/a.md", "notes/a/", "aa11aa11", &["threads"], serde_json::json!({"uid": "aa11aa11"})),
        post("notes/b.md", "notes/b/", "bb22bb22", &[], serde_json::json!({"uid": "bb22bb22"})),
    ]);
    let plans = decide(&PlanInputs { articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    let got = actions(&plans);
    assert_eq!(got.len(), 1);
    assert!(matches!(&got[0].2, Action::Skip { reason } if reason.contains("threads")));
}

#[test]
fn a_handed_off_copy_is_adopted_once_the_author_lists_its_address() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let mut handed = CopyRecord::due("aa11aa11", "mastodon", "notes/a.md", "notes/a/");
    handed.state = State::HandedOff;
    records.save(&handed).unwrap();
    let before = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["mastodon"], serde_json::json!({"uid": "aa11aa11"}))]);
    let (none, chans) = (HashSet::new(), channels());
    let inputs = |m| PlanInputs { articles: m, previously_live: None, edited_sources: &none, channels: &chans };
    assert!(actions(&decide(&inputs(&before), &records)).is_empty(), "nothing to do until the address is listed");

    let fm = serde_json::json!({"uid": "aa11aa11", "syndicated": ["https://example.social/@a/9"]});
    let after = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["mastodon"], fm)]);
    let plans = decide(&inputs(&after), &records);
    assert_eq!(actions(&plans)[0].2, Action::Adopt { url: "https://example.social/@a/9".into() });
    apply(dir.path(), plans, &records, None);
    assert_eq!(records.get("aa11aa11", "mastodon").unwrap().state, State::Posted);
}

#[test]
fn email_in_the_list_is_the_newsletter_and_plans_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["email", "bluesky"], serde_json::json!({"uid": "aa11aa11"}))]);
    let plans = decide(&PlanInputs { articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    assert_eq!(actions(&plans), [("notes/a/".into(), "bluesky".into(), Action::Due)]);
}

#[test]
fn apply_writes_the_uid_into_a_bare_page_before_its_record() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("notes")).unwrap();
    std::fs::write(dir.path().join("notes/a.md"), "Hello.\n").unwrap();
    let records = Records::at(dir.path().join(".moss/deploy/copies"));
    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["bluesky"], serde_json::json!({}))]);
    let plans = decide(&PlanInputs { articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    apply(dir.path(), plans, &records, None);
    let page = std::fs::read_to_string(dir.path().join("notes/a.md")).unwrap();
    assert!(page.starts_with("---\nuid: "), "{page}");
    assert!(page.ends_with("Hello.\n"));
    assert_eq!(records.get("aa11aa11", "bluesky").unwrap().state, State::Due);
}

#[test]
fn a_page_whose_uid_cannot_be_written_gets_no_record() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().join("copies"));
    let m = map(vec![post("notes/missing.md", "notes/missing/", "aa11aa11", &["bluesky"], serde_json::json!({}))]);
    let plans = decide(&PlanInputs { articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    let applied = apply(dir.path(), plans, &records, None);
    assert!(matches!(&applied[0].action, Action::Skip { reason } if reason.contains("note id")));
    assert!(!records.exists("aa11aa11", "bluesky"));
}
