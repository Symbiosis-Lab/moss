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
        ("bluesky".to_string(), ChannelInfo { spec: super::super::channel::ChannelSpec { forms: vec![super::super::channel::Form::Short], ..Default::default() }, domains: vec!["bsky.app".into()], ..Default::default() }),
        ("mastodon".to_string(), ChannelInfo { spec: super::super::channel::ChannelSpec { forms: vec![super::super::channel::Form::Short], ..Default::default() }, domains: vec!["example.social".into()], ..Default::default() }),
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
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    assert_eq!(
        actions(&plans),
        [("notes/a/".into(), "bluesky".into(), Action::Due), ("notes/a/".into(), "mastodon".into(), Action::Due)]
    );
}

#[test]
fn a_post_live_before_its_folder_named_the_channel_waits_unless_it_names_it_itself() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let m = map(vec![
        post("notes/old.md", "notes/old/", "0a0a0a0a", &["bluesky"], serde_json::json!({"uid": "0a0a0a0a"})),
        post("notes/own.md", "notes/own/", "0b0b0b0b", &["bluesky"], serde_json::json!({"uid": "0b0b0b0b", "syndicate": ["bluesky"]})),
        // Live before under another uid: its address says it was live.
        post("notes/bare.md", "notes/bare/", "0c0c0c0c", &["bluesky"], serde_json::json!({})),
    ]);
    let before = [live("0a0a0a0a", "notes/old/"), live("0b0b0b0b", "notes/own/"), live("99999999", "notes/bare/")];
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: Some(&before), edited_sources: &HashSet::new(), channels: &channels() }, &records);
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
    let unchanged = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    assert!(actions(&unchanged).is_empty(), "an unreadable record still blocks a repost: {unchanged:?}");

    let edited = HashSet::from(["notes/a.md".to_string()]);
    let changed = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &edited, channels: &channels() }, &records);
    assert_eq!(actions(&changed), [("notes/a/".into(), "bluesky".into(), Action::Update)]);
}

#[test]
fn a_copy_already_listed_in_syndicated_is_adopted_not_posted() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let fm = serde_json::json!({"uid": "aa11aa11", "syndicated": ["https://bsky.app/profile/a/post/1"]});
    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["bluesky"], fm)]);
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    assert_eq!(actions(&plans)[0].2, Action::Adopt { url: "https://bsky.app/profile/a/post/1".into() });

    let applied = apply(plans, &records, None, None, &HashMap::new());
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
        let plans = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
        assert_eq!(actions(&plans)[0].2, Action::Adopt { url: listed.into() }, "{listed}");
    }
}

#[test]
fn a_copy_listed_as_one_address_is_adopted_too() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let fm = serde_json::json!({"uid": "aa11aa11", "syndicated": "https://bsky.app/profile/a/post/1"});
    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["bluesky"], fm)]);
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
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
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
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
    let config = crate::config::ConfigFile::parse("").unwrap();
    let inputs = |m| PlanInputs { folder: std::path::Path::new("."), config: &config, site_url: Some("https://example.org"), articles: m, previously_live: None, edited_sources: &none, channels: &chans };
    assert!(actions(&decide(&inputs(&before), &records)).is_empty(), "nothing to do until the address is listed");

    let fm = serde_json::json!({"uid": "aa11aa11", "syndicated": ["https://example.social/@a/9"]});
    let after = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["mastodon"], fm)]);
    let plans = decide(&inputs(&after), &records);
    assert_eq!(actions(&plans)[0].2, Action::Adopt { url: "https://example.social/@a/9".into() });
    apply(plans, &records, None, None, &HashMap::new());
    assert_eq!(records.get("aa11aa11", "mastodon").unwrap().state, State::Posted);
}

#[test]
fn email_in_the_list_is_the_newsletter_and_plans_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["email", "bluesky"], serde_json::json!({"uid": "aa11aa11"}))]);
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    assert_eq!(actions(&plans), [("notes/a/".into(), "bluesky".into(), Action::Due)]);
}

/// A copied note still carries the original's uid in its file until it is
/// published, but is built under its own: its copy is keyed by that one, which
/// is the uid the publish writes into the file.
#[test]
fn a_copy_is_keyed_by_the_uid_its_page_was_built_under() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().join("copies"));
    let m = map(vec![post("notes/copy.md", "notes/copy/", "bb22bb22", &["bluesky"], serde_json::json!({"uid": "aa11aa11"}))]);
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    apply(plans, &records, None, None, &HashMap::new());
    assert!(records.exists("bb22bb22", "bluesky"));
    assert!(!records.exists("aa11aa11", "bluesky"), "the original's uid belongs to the original's copies");
}

#[test]
fn a_page_whose_uid_cannot_be_written_gets_no_record() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().join("copies"));
    let m = map(vec![post("notes/missing.md", "notes/missing/", "aa11aa11", &["bluesky"], serde_json::json!({}))]);
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &HashSet::new(), channels: &channels() }, &records);
    let unstamped = HashMap::from([("notes/missing.md".to_string(), "it is still in the cloud".to_string())]);
    let applied = apply(plans, &records, None, None, &unstamped);
    assert!(matches!(&applied[0].action, Action::Skip { reason } if reason.contains("note id") && reason.contains("cloud")));
    assert!(!records.exists("aa11aa11", "bluesky"));
}

#[test]
fn edited_refusals_refresh_the_snapshot_while_uncertain_copies_keep_it() {
    use super::super::record::PublishedPost;
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let mut original = post("a.md", "a/", "u", &["bluesky"], serde_json::json!({}));
    original.content = "Original".into();
    let mut edited = original.clone();
    edited.content = "Edited".into();
    let m = map(vec![edited]);
    let edits = HashSet::from(["a.md".into()]);
    let chans = channels();
    let inputs = PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), articles: &m, previously_live: None, edited_sources: &edits, channels: &chans };
    for state in [State::Refused, State::Due, State::Sending, State::Unconfirmed, State::HandedOff] {
        let mut r = CopyRecord::due("u", "bluesky", "a.md", "a/");
        r.state = state;
        r.form = Some(super::super::channel::Form::Short);
        r.part_count = Some(1);
        if state == State::Refused { r.discard_attempt(); }
        r.post = Some(Box::new(PublishedPost { article: original.clone(), lang: None }));
        records.save(&r).unwrap();
        let plans = decide(&inputs, &records);
        if state == State::Refused {
            assert_eq!(actions(&plans)[0].2, Action::Due);
            apply(plans, &records, Some("https://example.org"), Some(&m), &HashMap::new());
            let saved = records.get("u", "bluesky").unwrap();
            assert_eq!(saved.state, State::Due);
            assert!(saved.form.is_none());
            assert_eq!(saved.post.unwrap().article.content, "Edited");
        } else {
            assert!(plans.is_empty(), "{state:?}");
            assert_eq!(records.get("u", "bluesky").unwrap().form, r.form);
        }
    }
}

#[test]
fn unavailable_pages_are_skipped_before_any_new_record_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let mut article = post("a.md", "a/", "u", &["bluesky"], serde_json::json!({}));
    article.content = "too long ".repeat(100);
    let mut chans = channels();
    chans.get_mut("bluesky").unwrap().spec.max_chars = Some(20);
    let config = crate::config::ConfigFile::parse("").unwrap();
    for site_url in [None, Some("https://example.org")] {
        let m = map(vec![article.clone()]);
        let inputs = PlanInputs { folder: std::path::Path::new("."), articles: &m, channels: &chans, config: &config, site_url, previously_live: None, edited_sources: &HashSet::new() };
        let plans = decide(&inputs, &records);
        assert!(matches!(&actions(&plans)[0].2, Action::Unavailable { reason } if reason.contains("fits none")));
        apply(plans, &records, site_url, Some(&m), &HashMap::new());
        assert!(!records.exists("u", "bluesky"));
    }
    article.frontmatter.insert("syndicated".into(), serde_json::json!(["https://bsky.app/existing"]));
    let m = map(vec![article]);
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), articles: &m, channels: &chans, config: &config, site_url: None, previously_live: None, edited_sources: &HashSet::new() }, &records);
    assert!(matches!(actions(&plans)[0].2, Action::Adopt { .. }), "identity wins over fit");
}

#[test]
fn old_posted_copies_are_not_marked_for_update() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let mut r = CopyRecord::due("u", "bluesky", "a.md", "a/");
    r.state = State::Posted;
    r.remote = Some(RemoteRef { id: "1".into(), url: "https://bsky.app/1".into() });
    records.save(&r).unwrap();
    let m = map(vec![post("a.md", "a/", "u", &["bluesky"], serde_json::json!({}))]);
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), articles: &m, channels: &channels(), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), previously_live: None, edited_sources: &HashSet::from(["a.md".into()]) }, &records);
    assert!(matches!(&actions(&plans)[0].2, Action::Unavailable { reason } if reason.contains("retaining")));
    apply(plans, &records, None, Some(&m), &HashMap::new());
    assert_eq!(records.get("u", "bluesky").unwrap(), r);
}

fn replan_edit(records: &Records, article: ArticleInfo) -> (Vec<Planned>, ArticleMap) {
    let edits = HashSet::from([article.source_path.clone()]);
    let m = map(vec![article]);
    let mut chans = channels();
    chans.get_mut("bluesky").unwrap().spec.max_chars = Some(80);
    let plans = decide(&PlanInputs { folder: std::path::Path::new("."), articles: &m, channels: &chans, config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"), previously_live: None, edited_sources: &edits }, records);
    (apply(plans, records, Some("https://example.org"), Some(&m), &HashMap::new()), m)
}

#[test]
fn unavailable_first_work_cancels_and_resumes_on_a_fitting_edit() {
    for state in [State::Refused, State::Due] {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path().into());
        let article = post("a.md", "a/", "u", &["bluesky"], serde_json::json!({}));
        let mut r = CopyRecord::due("u", "bluesky", "a.md", "a/");
        r.state = state;
        r.post = Some(Box::new(PublishedPost { article: article.clone(), lang: None }));
        records.save(&r).unwrap();
        let mut impossible = article.clone();
        impossible.content = "too long ".repeat(80);
        let (plan, _) = replan_edit(&records, impossible);
        assert!(matches!(&plan[0].action, Action::Unavailable { reason } if reason.contains("fits none")));
        let blocked = records.get("u", "bluesky").unwrap();
        assert_eq!(blocked.next_op(false), None, "stale first publication remains sendable: {state:?}");
        assert_eq!(blocked.next_op(true), None);
        assert_eq!(blocked.state, State::Unavailable);
        assert!(blocked.post.is_none());
        assert!(blocked.reason.as_deref().unwrap().contains("fits none"));
        let mut fitting = article;
        fitting.content = "Fits again.".into();
        replan_edit(&records, fitting);
        let resumed = records.get("u", "bluesky").unwrap();
        assert_eq!(resumed.next_op(false), Some(super::super::job::Op::Publish));
        assert_eq!(resumed.post.unwrap().article.content, "Fits again.");
        assert_eq!(resumed.uid, r.uid);
    }
}

#[test]
fn unavailable_posted_edits_cancel_queued_updates_and_resume_later() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let article = post("a.md", "a/", "u", &["bluesky"], serde_json::json!({}));
    let mut r = CopyRecord::due("u", "bluesky", "a.md", "a/");
    r.state = State::Posted;
    r.form = Some(super::super::channel::Form::Short);
    r.remote = Some(RemoteRef { id: "1".into(), url: "https://bsky.app/1".into() });
    r.hash = Some("posted-hash".into());
    r.update_due = true;
    r.post = Some(Box::new(PublishedPost { article: article.clone(), lang: None }));
    records.save(&r).unwrap();
    let mut impossible = article.clone();
    impossible.content = "too long ".repeat(80);
    replan_edit(&records, impossible);
    let blocked = records.get("u", "bluesky").unwrap();
    assert_eq!(blocked.next_op(false), None, "the older queued update must not send");
    assert_eq!(blocked.state, State::Posted);
    assert!(blocked.post.is_none() && !blocked.update_due);
    assert_eq!((&blocked.remote, &blocked.form, &blocked.hash, &blocked.parts), (&r.remote, &r.form, &r.hash, &r.parts));
    let mut fitting = article;
    fitting.content = "Fits again.".into();
    replan_edit(&records, fitting);
    let resumed = records.get("u", "bluesky").unwrap();
    assert_eq!(resumed.next_op(false), Some(super::super::job::Op::Update));
    assert_eq!(resumed.post.unwrap().article.content, "Fits again.");
    assert_eq!(resumed.remote, r.remote);
}

#[test]
fn unavailable_edits_preserve_uncertain_attempts_and_drafts() {
    for state in [State::Sending, State::Unconfirmed, State::HandedOff, State::Due] {
        let dir = tempfile::tempdir().unwrap();
        let records = Records::at(dir.path().into());
        let mut article = post("a.md", "a/", "u", &["bluesky"], serde_json::json!({}));
        let mut r = CopyRecord::due("u", "bluesky", "a.md", "a/");
        r.state = state;
        if state == State::Due { r.form = Some(super::super::channel::Form::Short); }
        r.post = Some(Box::new(PublishedPost { article: article.clone(), lang: None }));
        if state == State::HandedOff {
            r.handoff = Some(super::super::record::Handoff { text: String::new(), open_url: None, id: Some("draft".into()) });
        }
        records.save(&r).unwrap();
        let before = records.get("u", "bluesky").unwrap();
        article.content = "too long ".repeat(80);
        replan_edit(&records, article);
        assert_eq!(records.get("u", "bluesky").unwrap(), before);
        assert!(!r.make_unavailable("too long"));
        assert_eq!(r, before);
    }
}

fn with_x() -> BTreeMap<String, ChannelInfo> {
    let mut channels = channels();
    let short = super::super::channel::ChannelSpec { forms: vec![super::super::channel::Form::Short], ..Default::default() };
    channels.insert("xcom".into(), ChannelInfo { spec: short, domains: vec!["x.com".into()], name: Some("X".into()) });
    channels
}

fn decide_with(channels: &BTreeMap<String, ChannelInfo>, m: &ArticleMap, before: Option<&[LiveEntry]>, records: &Records) -> Vec<(String, String, Action)> {
    let config = crate::config::ConfigFile::parse("").unwrap();
    actions(&decide(&PlanInputs { folder: std::path::Path::new("."), config: &config, site_url: Some("https://example.org"), articles: m, previously_live: before, edited_sources: &HashSet::new(), channels }, records))
}

#[test]
fn a_page_that_names_a_channel_by_its_networks_name_is_planned_under_the_channels_id() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["x"], serde_json::json!({}))]);
    assert_eq!(decide_with(&with_x(), &m, None, &records), [("notes/a/".into(), "xcom".into(), Action::Due)]);

    // The copy it then has is the one the page names: nothing is planned again.
    let mut posted = CopyRecord::due("aa11aa11", "xcom", "notes/a.md", "notes/a/");
    posted.state = State::Posted;
    records.save(&posted).unwrap();
    assert_eq!(decide_with(&with_x(), &m, None, &records), []);
}

#[test]
fn a_page_live_before_that_names_the_channel_itself_by_name_is_posted() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let m = map(vec![post("notes/own.md", "notes/own/", "0b0b0b0b", &["X"], serde_json::json!({"uid": "0b0b0b0b", "syndicate": ["X"]}))]);
    let before = [live("0b0b0b0b", "notes/own/")];
    assert_eq!(decide_with(&with_x(), &m, Some(&before), &records), [("notes/own/".into(), "xcom".into(), Action::Due)]);
}

#[test]
fn a_name_two_channels_go_by_is_unavailable_and_says_both() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().into());
    let mut channels = with_x();
    channels.insert("xmirror".into(), ChannelInfo { name: Some("x".into()), ..channels["xcom"].clone() });
    let m = map(vec![post("notes/a.md", "notes/a/", "aa11aa11", &["x", "bluesky"], serde_json::json!({}))]);
    let got = decide_with(&channels, &m, None, &records);
    assert_eq!(got[0], ("notes/a/".into(), "bluesky".into(), Action::Due));
    let Action::Unavailable { reason } = &got[1].2 else { panic!("{got:?}") };
    assert!(got[1].1 == "x" && reason.contains("xcom") && reason.contains("xmirror"), "{got:?}");
    assert_eq!(got.len(), 2);
}
