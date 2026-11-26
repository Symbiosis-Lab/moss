use crate::moss_paths::MossPaths;
use super::*;
use crate::build::manifest::change_set::LiveEntry;
use crate::syndication::plan::{decide, Action, PlanInputs};
use crate::syndication::record::CopyRecord;
use std::collections::{HashMap, HashSet};

const UID: &str = "aa11aa11";
const SOURCE: &str = "notes/a.md";
const URL: &str = "notes/a/";

fn channels() -> BTreeMap<String, ChannelInfo> {
    BTreeMap::from([("bluesky".to_string(), ChannelInfo { spec: super::super::channel::ChannelSpec { forms: vec![super::super::channel::Form::Short], ..Default::default() }, domains: vec!["bsky.app".into()], ..Default::default() })])
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn article(own: Option<&[String]>, folders: &[String], listed: &[String]) -> ArticleInfo {
    let mut fm = serde_json::json!({ "uid": UID, "syndicated": listed });
    if let Some(own) = own {
        fm["syndicate"] = serde_json::json!(own);
    }
    ArticleInfo {
        source_path: SOURCE.into(),
        title: String::new(),
        content: String::new(),
        html_content: None,
        frontmatter: fm.as_object().cloned().unwrap_or_default().into_iter().collect::<HashMap<_, _>>(),
        url_path: URL.into(),
        date: None,
        tags: vec![],
        uid: Some(UID.into()),
        syndicate: Some(own.unwrap_or(folders).to_vec()),
        untitled: false,
        hidden_from_search_engines: false,
    }
}

fn chip(view: &PostView<'_>, info: &ChannelInfo) -> ChannelChoice {
    view.choice("bluesky", plan::verdict("bluesky", Some(info), &view.facts, &view.records, false))
}

fn record(records: &Records, state: Option<State>) {
    if let Some(state) = state {
        let mut r = CopyRecord::due(UID, "bluesky", SOURCE, URL);
        r.state = state;
        records.save(&r).unwrap();
    }
}

/// A chip reads on exactly when the plan would act, or a copy stands for a
/// named channel, across the engine's whole table.
#[test]
fn a_chip_reads_on_exactly_when_a_publish_would_act_or_a_copy_stands() {
    let on_bluesky = strings(&["bluesky"]);
    let on_other = strings(&["mastodon"]);
    let none: Vec<String> = vec![];
    let address = strings(&["https://bsky.app/profile/a/post/1"]);
    let channels = channels();
    let info = &channels["bluesky"];
    let states = [None, Some(State::Due), Some(State::Posted), Some(State::Refused), Some(State::HandedOff), Some(State::Unconfirmed), Some(State::Sending)];
    for own in [None, Some(&none), Some(&on_bluesky), Some(&on_other)] {
        for folders in [&none, &on_bluesky] {
            for live in [false, true] {
                for state in states {
                    for listed in [&none, &address] {
                        for edited in [false, true] {
                          for choice in [None, Some(true), Some(false)] {
                            let dir = tempfile::tempdir().unwrap();
                            let mp = MossPaths::new(dir.path());
                            record_choice(&mp, UID, "bluesky", choice).unwrap();
                            let choices = Choices::in_vault(&mp);
                            let records = Records::at(dir.path().join("copies"));
                            record(&records, state);
                            let post = article(own.map(Vec::as_slice), folders, listed);
                            let mut map = ArticleMap::default();
                            map.articles.insert(URL.into(), post);
                            let before = [LiveEntry { uid: UID.into(), url: URL.into(), source_path: String::new(), title: String::new() }];
                            let sources = HashSet::from([SOURCE.to_string()]);
                            let inputs = PlanInputs { folder: std::path::Path::new("."), config: &crate::config::ConfigFile::parse("").unwrap(), site_url: Some("https://example.org"),
                                articles: &map,
                                previously_live: live.then_some(&before[..]),
                                edited_sources: if edited { &sources } else { &HashSet::new() },
                                channels: &channels, choices: &choices,
                            };
                            let plans = decide(&inputs, &records);
                            let acts = plans.iter().flat_map(|p| &p.entries).any(|e| matches!(e.action, Action::Due | Action::Update | Action::Adopt { .. }));

                            let named = |l: &Vec<String>| super::super::Named::of(l, &channels);
                            let asked = super::super::Asked::new(false, own.map(named), Some(named(folders)), UID, &choices);
                            let list = asked.list.unwrap_or_default();
                            let list = list.ids();
                            let view = PostView {
                                facts: PostFacts { uid: UID, own: asked.own.ids(), listed, already_live: live },
                                list,
                                folders,
                                live: if live { Live::Yes } else { Live::No },
                                records: Records::at(dir.path().join("copies")),
                            };
                            let named = list.iter().any(|c| c == "bluesky");
                            let got = chip(&view, info);
                            assert_eq!(got.on, acts || (named && state.is_some()), "own {own:?} folders {folders:?} live {live} state {state:?} listed {listed:?} edited {edited} choice {choice:?} -> {got:?}");
                          }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_copy_of_a_channel_the_list_no_longer_names_reads_off() {
    let dir = tempfile::tempdir().unwrap();
    let records = Records::at(dir.path().join("copies"));
    record(&records, Some(State::Posted));
    let own: Vec<String> = vec![];
    let folders = strings(&["bluesky"]);
    let view = PostView {
        facts: PostFacts { uid: UID, own: &own, listed: &[], already_live: true },
        list: &own,
        folders: &folders,
        live: Live::Yes,
        records,
    };
    assert_eq!(chip(&view, &channels()["bluesky"]), ChannelChoice { channel: "bluesky".into(), on: false, origin: ChoiceOrigin::LeftOut, reason: None });
}

#[test]
fn an_unreadable_baseline_says_unknown_unless_a_copy_stands() {
    let dir = tempfile::tempdir().unwrap();
    let list = strings(&["bluesky"]);
    let view = |records| PostView {
        facts: PostFacts { uid: UID, own: &list, listed: &[], already_live: true },
        list: &list,
        folders: &[],
        live: Live::Unknown,
        records,
    };
    let info = &channels()["bluesky"];
    assert_eq!(chip(&view(Records::at(dir.path().join("copies"))), info).origin, ChoiceOrigin::Unknown);
    let records = Records::at(dir.path().join("copies"));
    record(&records, Some(State::Posted));
    assert_eq!(chip(&view(records), info).origin, ChoiceOrigin::Copied);
}

#[test]
fn the_folders_above_a_page_are_walked_nearest_first() {
    assert_eq!(ancestors("a/b/post/"), ["a/b/", "a/", ""]);
    assert_eq!(ancestors("post"), [""]);
    assert!(ancestors("").is_empty());
}

#[test]
fn a_channel_added_back_takes_the_folders_place_and_other_entries_stay() {
    let folders = strings(&["bluesky", "email", "mastodon"]);
    assert_eq!(next_list(&strings(&["email", "mastodon"]), &folders, "bluesky", true), folders);
    assert_eq!(next_list(&strings(&["bluesky", "mastodon"]), &folders, "email", true), folders);
    assert_eq!(next_list(&folders, &folders, "email", false), strings(&["bluesky", "mastodon"]));
    assert_eq!(next_list(&strings(&["bluesky"]), &folders, "threads", true), strings(&["bluesky", "threads"]));
}

#[test]
fn the_key_follows_the_folder_unless_the_post_is_live() {
    let folders = strings(&["bluesky"]);
    let none: Vec<String> = vec![];
    assert!(matches!(key_edit(&folders, &folders, Live::No), FieldEdit::Remove { .. }));
    assert!(matches!(key_edit(&folders, &folders, Live::Yes), FieldEdit::Set { value, .. } if value.is_sequence()));
    assert!(matches!(key_edit(&folders, &folders, Live::Unknown), FieldEdit::Set { value, .. } if value.is_sequence()));
    assert!(matches!(key_edit(&none, &folders, Live::No), FieldEdit::Set { value: serde_yaml::Value::Bool(false), .. }));
    assert!(matches!(key_edit(&none, &none, Live::Yes), FieldEdit::Remove { .. }), "a live post with an empty folder gets no `syndicate: []`");
}

fn site(post: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).unwrap();
    std::fs::create_dir_all(MossPaths::new(root).build_dir()).unwrap();
    std::fs::write(root.join("notes/index.md"), "---\ntitle: Notes\n# why\ncascade:\n  syndicate: [bluesky]\n  listing: cards\n---\n").unwrap();
    std::fs::write(root.join(SOURCE), post).unwrap();
    let mut map = ArticleMap::default();
    map.pages.insert("notes/".into(), "notes/index.md".into());
    map.articles.insert(URL.into(), article(None, &strings(&["bluesky"]), &[]));
    map.save(&root.join(".moss")).unwrap();
    dir
}

/// Named or not, a channel that cannot take the page reads off, with why.
#[test]
fn a_channel_that_cannot_take_the_page_reads_unavailable_with_the_reason() {
    let dir = site("---\ntitle: A\n---\n\nBody.\n");
    let mut map = ArticleMap::load(&dir.path().join(".moss")).unwrap();
    map.articles.get_mut(URL).unwrap().content = "word ".repeat(100);
    map.save(&dir.path().join(".moss")).unwrap();
    let mut channels = channels();
    channels.get_mut("bluesky").unwrap().spec.max_chars = Some(300);
    for own in ["", "syndicate: false\n"] {
        std::fs::write(dir.path().join(SOURCE), format!("---\ntitle: A\n{own}---\n\nBody.\n")).unwrap();
        let chips = read_post_with(dir.path(), SOURCE, &channels).unwrap();
        assert_eq!((chips[0].on, chips[0].origin), (false, ChoiceOrigin::Unavailable), "{own:?}: {chips:?}");
        assert!(chips[0].reason.is_some(), "{chips:?}");
    }
}

fn page_of(dir: &tempfile::TempDir, rel: &str) -> String {
    std::fs::read_to_string(dir.path().join(rel)).unwrap()
}

const COPY_URL: &str = "https://bsky.app/profile/a/post/1";
const POST: &str = "---\ntitle: A\nuid: bb22bb22\n---\n\nBody stays.\n";

fn adoption_site() -> tempfile::TempDir {
    let dir = site(POST);
    super::super::test_channel(dir.path(), "bluesky", "{}");
    let manifest = dir.path().join(".moss/plugins/bluesky/manifest.json");
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    value["domains"] = serde_json::json!(["bsky.app", "social.example"]);
    std::fs::write(manifest, value.to_string()).unwrap();
    dir
}

fn copies(dir: &tempfile::TempDir) -> Records {
    Records::in_vault(&MossPaths::new(dir.path()))
}

fn record_path(dir: &tempfile::TempDir) -> std::path::PathBuf {
    MossPaths::new(dir.path()).deploy_dir().join("copies").join(format!("{UID}.bluesky.json"))
}

fn seed_copy(dir: &tempfile::TempDir, state: State) -> CopyRecord {
    use super::super::record::{Handoff, RemoteRef};
    let mut r = CopyRecord::due(UID, "bluesky", SOURCE, URL);
    r.state = state;
    r.reason = Some("waiting".into());
    r.handoff = Some(Handoff { text: "copy".into(), open_url: None, id: None });
    r.remote = Some(RemoteRef { id: "original-id".into(), url: COPY_URL.into() });
    copies(dir).save(&r).unwrap();
    r
}

fn assert_listed_once(dir: &tempfile::TempDir, source: &str, url: &str) {
    let content = page_of(dir, source);
    let front = frontmatter::parse(&content).frontmatter;
    let list = front["syndicated"].as_sequence().unwrap();
    assert_eq!(list.iter().filter(|v| v.as_str() == Some(url)).count(), 1);
    assert!(content.contains("uid: bb22bb22"));
    assert!(content.ends_with("\n\nBody stays.\n"));
}

#[test]
fn adopt_checks_channel_then_host_before_the_post_without_mutation() {
    let dir = adoption_site();
    seed_copy(&dir, State::Due);
    let before = std::fs::read(record_path(&dir)).unwrap();
    assert_eq!(adopt(dir.path(), "absent.md", "missing", "invalid"), Err(AdoptRefusal::UnknownChannel));
    for address in ["https://elsewhere.example/post", "https://bsky.app.evil.example/post", "invalid"] {
        assert_eq!(adopt(dir.path(), "absent.md", "bluesky", address), Err(AdoptRefusal::OffHost { expected: "bsky.app".into() }));
    }
    assert_eq!(page_of(&dir, SOURCE), POST);
    assert_eq!(std::fs::read(record_path(&dir)).unwrap(), before);
}

#[test]
fn adopt_requires_a_built_source_and_built_uid() {
    let dir = adoption_site();
    assert_eq!(adopt(dir.path(), "absent.md", "bluesky", COPY_URL), Err(AdoptRefusal::NotAPost));
    let mut map = ArticleMap::load(&dir.path().join(".moss")).unwrap();
    map.articles.get_mut(URL).unwrap().uid = None;
    map.save(&dir.path().join(".moss")).unwrap();
    assert_eq!(adopt(dir.path(), SOURCE, "bluesky", COPY_URL), Err(AdoptRefusal::NotAPost));
    assert_eq!(page_of(&dir, SOURCE), POST);
    assert!(!copies(&dir).exists(UID, "bluesky"));
    assert!(!copies(&dir).exists("bb22bb22", "bluesky"));
}

#[test]
fn adopt_without_a_record_only_writes_the_page_even_while_a_turn_is_held() {
    for held in [false, true] {
        let dir = adoption_site();
        let records = copies(&dir);
        let _turn = held.then(|| records.turn().unwrap());
        assert_eq!(adopt(dir.path(), SOURCE, "bluesky", COPY_URL), Ok(Adopted { write_back: None }));
        assert_eq!(adopt(dir.path(), SOURCE, "bluesky", COPY_URL), Ok(Adopted { write_back: None }));
        assert_listed_once(&dir, SOURCE, COPY_URL);
        assert!(!records.exists(UID, "bluesky"));
        assert!(!records.exists("bb22bb22", "bluesky"));
        if !held {
            assert!(!MossPaths::new(dir.path()).deploy_dir().join("copies.lock").exists());
        }
    }
}

#[test]
fn adopt_accepts_any_declared_domain() {
    let dir = adoption_site();
    let url = "https://social.example/post/1";
    adopt(dir.path(), SOURCE, "bluesky", url).unwrap();
    assert_listed_once(&dir, SOURCE, url);
    assert!(!copies(&dir).exists(UID, "bluesky"));
}

#[test]
fn adopt_existing_record_needs_a_turn_before_reading() {
    let dir = adoption_site();
    seed_copy(&dir, State::Due);
    let records = copies(&dir);
    let _turn = records.turn().unwrap();
    for bytes in [std::fs::read(record_path(&dir)).unwrap(), b"not json".to_vec()] {
        std::fs::write(record_path(&dir), &bytes).unwrap();
        assert_eq!(adopt(dir.path(), SOURCE, "bluesky", COPY_URL), Err(AdoptRefusal::Busy));
        assert_eq!(page_of(&dir, SOURCE), POST);
        assert_eq!(std::fs::read(record_path(&dir)).unwrap(), bytes);
    }
}

#[test]
fn adopt_refuses_an_unreadable_record_without_touching_it_or_the_page() {
    let dir = adoption_site();
    seed_copy(&dir, State::Due);
    std::fs::write(record_path(&dir), "not json").unwrap();
    assert_eq!(adopt(dir.path(), SOURCE, "bluesky", COPY_URL), Err(AdoptRefusal::Unreadable));
    assert_eq!(page_of(&dir, SOURCE), POST);
    assert_eq!(std::fs::read_to_string(record_path(&dir)).unwrap(), "not json");
}

#[test]
fn adopt_posted_same_address_preserves_the_record_and_remote_id() {
    let dir = adoption_site();
    seed_copy(&dir, State::Posted);
    let before = std::fs::read(record_path(&dir)).unwrap();
    for _ in 0..2 {
        assert_eq!(adopt(dir.path(), SOURCE, "bluesky", COPY_URL), Ok(Adopted { write_back: None }));
        assert_listed_once(&dir, SOURCE, COPY_URL);
        assert_eq!(std::fs::read(record_path(&dir)).unwrap(), before);
    }
}

#[test]
fn adopt_posted_different_address_preserves_both_files() {
    let dir = adoption_site();
    seed_copy(&dir, State::Posted);
    let before = std::fs::read(record_path(&dir)).unwrap();
    assert_eq!(adopt(dir.path(), SOURCE, "bluesky", "https://bsky.app/other"), Err(AdoptRefusal::AlreadyPosted { url: COPY_URL.into() }));
    assert_eq!(page_of(&dir, SOURCE), POST);
    assert_eq!(std::fs::read(record_path(&dir)).unwrap(), before);
}

#[test]
fn adopt_settles_each_pending_state_using_the_built_identity() {
    for state in [State::Due, State::Refused, State::Unconfirmed, State::HandedOff] {
        let dir = adoption_site();
        seed_copy(&dir, state);
        assert_eq!(adopt(dir.path(), SOURCE, "bluesky", COPY_URL), Ok(Adopted { write_back: None }));
        let records = copies(&dir);
        let adopted = records.get(UID, "bluesky").unwrap();
        assert_eq!(adopted.state, State::Posted, "{state:?}");
        let remote = adopted.remote.unwrap();
        assert_eq!(remote.id, COPY_URL);
        assert_eq!(remote.url, COPY_URL);
        assert!(adopted.reason.is_none());
        assert!(adopted.handoff.is_none());
        assert!(!records.exists("bb22bb22", "bluesky"));
        adopt(dir.path(), SOURCE, "bluesky", COPY_URL).unwrap();
        assert_listed_once(&dir, SOURCE, COPY_URL);
    }
}

#[test]
fn adopt_sending_is_busy_without_mutation() {
    let dir = adoption_site();
    seed_copy(&dir, State::Sending);
    let before = std::fs::read(record_path(&dir)).unwrap();
    assert_eq!(adopt(dir.path(), SOURCE, "bluesky", COPY_URL), Err(AdoptRefusal::Busy));
    assert_eq!(page_of(&dir, SOURCE), POST);
    assert_eq!(std::fs::read(record_path(&dir)).unwrap(), before);
}

#[test]
fn adopt_writes_a_renamed_post_and_only_refreshes_pending_records() {
    for state in [State::Due, State::Posted] {
        let dir = adoption_site();
        seed_copy(&dir, state);
        let before = std::fs::read(record_path(&dir)).unwrap();
        let renamed = "notes/renamed.md";
        std::fs::rename(dir.path().join(SOURCE), dir.path().join(renamed)).unwrap();
        std::fs::write(dir.path().join(SOURCE), "An unrelated page.\n").unwrap();
        let mut map = ArticleMap::load(&dir.path().join(".moss")).unwrap();
        let post = map.articles.get_mut(URL).unwrap();
        post.source_path = renamed.into();
        post.url_path = "notes/renamed/".into();
        map.save(&dir.path().join(".moss")).unwrap();
        adopt(dir.path(), renamed, "bluesky", COPY_URL).unwrap();
        assert_listed_once(&dir, renamed, COPY_URL);
        assert_eq!(page_of(&dir, SOURCE), "An unrelated page.\n");
        let r = copies(&dir).get(UID, "bluesky").unwrap();
        assert_eq!(r.state, State::Posted);
        if state == State::Posted {
            assert_eq!(std::fs::read(record_path(&dir)).unwrap(), before);
        } else {
            assert_eq!(r.source_path, renamed);
            assert_eq!(r.url_path, "notes/renamed/");
        }
    }
}

#[test]
fn adopt_reports_writeback_failure_and_keeps_the_settled_record() {
    for state in [None, Some(State::Due), Some(State::Posted)] {
        let dir = adoption_site();
        if let Some(state) = state {
            seed_copy(&dir, state);
        }
        std::fs::remove_file(dir.path().join(SOURCE)).unwrap();
        let result = adopt(dir.path(), SOURCE, "bluesky", COPY_URL).unwrap();
        assert!(result.write_back.is_some());
        assert!(!dir.path().join(SOURCE).exists());
        let r = copies(&dir).get(UID, "bluesky");
        if state.is_some() {
            let r = r.unwrap();
            assert_eq!(r.state, State::Posted);
            assert_eq!(r.remote.unwrap().url, COPY_URL);
        } else {
            assert!(r.is_none());
            assert!(!copies(&dir).exists(UID, "bluesky"));
        }
    }
}

#[test]
fn adopt_refusals_serialize_with_kebab_case_names() {
    let cases = [
        (AdoptRefusal::UnknownChannel, serde_json::json!("unknown-channel")),
        (AdoptRefusal::OffHost { expected: "bsky.app".into() }, serde_json::json!({"off-host": {"expected": "bsky.app"}})),
        (AdoptRefusal::NotAPost, serde_json::json!("not-a-post")),
        (AdoptRefusal::Busy, serde_json::json!("busy")),
        (AdoptRefusal::Unreadable, serde_json::json!("unreadable")),
        (AdoptRefusal::AlreadyPosted { url: COPY_URL.into() }, serde_json::json!({"already-posted": {"url": COPY_URL}})),
    ];
    for (refusal, wire) in cases {
        assert_eq!(serde_json::to_value(refusal).unwrap(), wire);
    }
}

/// A site whose X channel is set up, and a second channel its network also calls X.
fn site_with_x(post: &str) -> tempfile::TempDir {
    let dir = site(post);
    super::super::test_channel_named(dir.path(), "xcom", "X", "{}");
    super::super::test_channel(dir.path(), "bluesky", "{}");
    dir
}

#[test]
fn turning_a_channel_off_takes_out_the_name_the_page_wrote_for_it() {
    let dir = site_with_x("---\ntitle: A\nsyndicate: [x, bluesky]\n---\n\nBody stays.\n");
    let on: Vec<_> = read_post(dir.path(), SOURCE).unwrap().into_iter().filter(|c| c.on).map(|c| c.channel).collect();
    assert_eq!(on, strings(&["bluesky", "xcom"]), "the chip of the channel the page names as x is on");
    set_post(dir.path(), SOURCE, "xcom", false).unwrap();
    assert_eq!(page_of(&dir, SOURCE), "---\ntitle: A\n---\n\nBody stays.\n", "the folder's list, bluesky, is what is left");
}

#[test]
fn a_name_two_channels_go_by_turns_no_chip_on_and_a_write_keeps_it() {
    let page = "---\ntitle: A\nsyndicate: [x]\n---\n\nBody stays.\n";
    let dir = site_with_x(page);
    super::super::test_channel_named(dir.path(), "xmirror", "x", "{}");
    let chips = read_post(dir.path(), SOURCE).unwrap();
    assert!(chips.iter().all(|c| !c.on), "{chips:?}");
    assert_eq!(page_of(&dir, SOURCE), page, "reading writes nothing");
    set_post(dir.path(), SOURCE, "bluesky", true).unwrap();
    assert_eq!(page_of(&dir, SOURCE), "---\ntitle: A\nsyndicate:\n  - bluesky\n  - x\n---\n\nBody stays.\n");
}

fn chips_on(dir: &tempfile::TempDir) -> Vec<String> {
    read_post(dir.path(), SOURCE).unwrap().into_iter().filter(|c| c.on).map(|c| c.channel).collect()
}

/// What the plan and the send are told the post asks for.
fn sends(dir: &tempfile::TempDir) -> Vec<String> {
    let map = ArticleMap::load(&dir.path().join(".moss")).unwrap();
    let choices = Choices::in_vault(&MossPaths::new(dir.path()));
    let named = super::super::Named::of_post(&map.articles[URL], &super::super::landing::channels(dir.path()), &choices);
    named.map(|n| n.ids().to_vec()).unwrap_or_default()
}

fn choice_files(dir: &tempfile::TempDir) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(choices_dir(&MossPaths::new(dir.path()))) else { return Vec::new() };
    let mut names: Vec<String> = entries.filter_map(Result::ok).filter_map(|e| e.file_name().into_string().ok()).collect();
    names.sort();
    names
}

#[test]
fn a_click_on_a_post_without_its_own_line_leaves_the_page_byte_identical() {
    let page = "---\ntitle: A\n---\n\nBody stays.\n";
    let dir = site_with_x(page);
    set_post(dir.path(), SOURCE, "xcom", true).unwrap();
    set_post(dir.path(), SOURCE, "bluesky", false).unwrap();
    assert_eq!(page_of(&dir, SOURCE), page);
    assert_eq!(chips_on(&dir), strings(&["xcom"]));
    assert_eq!(sends(&dir), strings(&["xcom"]), "the plan and the send read the same choices as the chips");
    set_post(dir.path(), SOURCE, "xcom", false).unwrap();
    set_post(dir.path(), SOURCE, "bluesky", true).unwrap();
    assert_eq!(page_of(&dir, SOURCE), page);
    assert_eq!(sends(&dir), strings(&["bluesky"]));
    assert!(choice_files(&dir).is_empty(), "a choice equal to the folder's list is removed: {:?}", choice_files(&dir));
}

#[test]
fn a_choice_is_never_applied_to_a_post_with_its_own_line_and_the_next_click_removes_it() {
    let dir = site_with_x("---\ntitle: A\n---\n\nBody stays.\n");
    set_post(dir.path(), SOURCE, "xcom", true).unwrap();
    assert_eq!(choice_files(&dir), vec![format!("{UID}.xcom")]);
    std::fs::write(dir.path().join(SOURCE), "---\ntitle: A\nsyndicate: [bluesky]\n---\n\nBody stays.\n").unwrap();
    let mut map = ArticleMap::load(&dir.path().join(".moss")).unwrap();
    let post = map.articles.get_mut(URL).unwrap();
    post.frontmatter.insert("syndicate".into(), serde_json::json!(["bluesky"]));
    map.save(&dir.path().join(".moss")).unwrap();
    assert_eq!(chips_on(&dir), strings(&["bluesky"]));
    assert_eq!(sends(&dir), strings(&["bluesky"]));
    set_post(dir.path(), SOURCE, "xcom", false).unwrap();
    assert!(choice_files(&dir).is_empty(), "{:?}", choice_files(&dir));
}

#[test]
fn a_click_on_turns_a_waiting_channel_on_for_a_live_post() {
    let dir = site_with_x("---\ntitle: A\nuid: aa11aa11\n---\n\nBody stays.\n");
    let mp = MossPaths::new(dir.path());
    std::fs::create_dir_all(mp.deploy_dir()).unwrap();
    std::fs::copy(mp.article_map(), mp.deployed_article_map()).unwrap();
    let chips = set_post(dir.path(), SOURCE, "bluesky", true).unwrap();
    assert!(chips.iter().any(|c| c.channel == "bluesky" && c.on), "{chips:?}");
    assert_eq!(sends(&dir), strings(&["bluesky"]));
}

#[test]
fn a_choice_never_sends_a_page_reached_only_by_its_link() {
    let dir = site_with_x("---\ntitle: A\n---\n\nBody stays.\n");
    set_post(dir.path(), SOURCE, "xcom", true).unwrap();
    let mut map = ArticleMap::load(&dir.path().join(".moss")).unwrap();
    let post = map.articles.get_mut(URL).unwrap();
    post.hidden_from_search_engines = true;
    post.syndicate = Some(Vec::new());
    map.save(&dir.path().join(".moss")).unwrap();
    assert_eq!(sends(&dir), Vec::<String>::new());
    assert_eq!(chips_on(&dir), Vec::<String>::new());
}
