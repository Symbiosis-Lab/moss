use super::*;
use crate::build::manifest::change_set::LiveEntry;
use crate::syndication::plan::{decide, Action, PlanInputs};
use crate::syndication::record::CopyRecord;
use std::collections::{HashMap, HashSet};

const UID: &str = "aa11aa11";
const SOURCE: &str = "notes/a.md";
const URL: &str = "notes/a/";

fn channels() -> BTreeMap<String, ChannelInfo> {
    BTreeMap::from([("bluesky".to_string(), ChannelInfo { domains: vec!["bsky.app".into()], ..Default::default() })])
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
    }
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
                            let dir = tempfile::tempdir().unwrap();
                            let records = Records::at(dir.path().join("copies"));
                            record(&records, state);
                            let post = article(own.map(Vec::as_slice), folders, listed);
                            let mut map = ArticleMap::default();
                            map.articles.insert(URL.into(), post);
                            let before = [LiveEntry { uid: UID.into(), url: URL.into(), source_path: String::new(), title: String::new() }];
                            let sources = HashSet::from([SOURCE.to_string()]);
                            let inputs = PlanInputs {
                                articles: &map,
                                previously_live: live.then_some(&before[..]),
                                edited_sources: if edited { &sources } else { &HashSet::new() },
                                channels: &channels,
                            };
                            let plans = decide(&inputs, &records);
                            let acts = plans.iter().flat_map(|p| &p.entries).any(|e| matches!(e.action, Action::Due | Action::Update | Action::Adopt { .. }));

                            let list = own.map(Vec::as_slice).unwrap_or(folders);
                            let view = PostView {
                                facts: PostFacts { uid: UID, own: own.map(Vec::as_slice).unwrap_or_default(), listed, already_live: live },
                                list,
                                folders,
                                has_own: own.is_some(),
                                live: if live { Live::Yes } else { Live::No },
                                records: Records::at(dir.path().join("copies")),
                            };
                            let named = list.iter().any(|c| c == "bluesky");
                            let got = view.choice("bluesky", info);
                            assert_eq!(got.on, acts || (named && state.is_some()), "own {own:?} folders {folders:?} live {live} state {state:?} listed {listed:?} edited {edited} -> {got:?}");
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
        has_own: true,
        live: Live::Yes,
        records,
    };
    assert_eq!(view.choice("bluesky", &channels()["bluesky"]), ChannelChoice { channel: "bluesky".into(), on: false, origin: ChoiceOrigin::LeftOut });
}

#[test]
fn an_unreadable_baseline_says_unknown_unless_a_copy_stands() {
    let dir = tempfile::tempdir().unwrap();
    let list = strings(&["bluesky"]);
    let view = |records| PostView {
        facts: PostFacts { uid: UID, own: &list, listed: &[], already_live: true },
        list: &list,
        folders: &[],
        has_own: true,
        live: Live::Unknown,
        records,
    };
    let info = &channels()["bluesky"];
    assert_eq!(view(Records::at(dir.path().join("copies"))).choice("bluesky", info).origin, ChoiceOrigin::Unknown);
    let records = Records::at(dir.path().join("copies"));
    record(&records, Some(State::Posted));
    assert_eq!(view(records).choice("bluesky", info).origin, ChoiceOrigin::Copied);
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
    std::fs::create_dir_all(root.join(".moss/build.nosync")).unwrap();
    std::fs::write(root.join("notes/index.md"), "---\ntitle: Notes\n# why\ncascade:\n  syndicate: [bluesky]\n  listing: cards\n---\n").unwrap();
    std::fs::write(root.join(SOURCE), post).unwrap();
    let mut map = ArticleMap::default();
    map.pages.insert("notes/".into(), "notes/index.md".into());
    map.articles.insert(URL.into(), article(None, &strings(&["bluesky"]), &[]));
    map.save(&root.join(".moss")).unwrap();
    dir
}

fn page_of(dir: &tempfile::TempDir, rel: &str) -> String {
    std::fs::read_to_string(dir.path().join(rel)).unwrap()
}

#[test]
fn leaving_a_new_post_out_writes_false_and_taking_it_back_removes_the_key() {
    let dir = site("---\ntitle: A\n---\n\nBody stays.\n");
    set_post(dir.path(), SOURCE, "bluesky", false).unwrap();
    assert_eq!(page_of(&dir, SOURCE), "---\ntitle: A\nsyndicate: false\n---\n\nBody stays.\n");
    set_post(dir.path(), SOURCE, "bluesky", true).unwrap();
    assert_eq!(page_of(&dir, SOURCE), "---\ntitle: A\n---\n\nBody stays.\n");
}

#[test]
fn a_folder_page_writes_its_own_cascade_key_and_keeps_the_others() {
    let dir = site("---\ntitle: A\n---\n");
    let after = set_folder(dir.path(), "notes/", "notes/index.md", "mastodon", true).unwrap();
    assert_eq!(after, strings(&["bluesky", "mastodon"]));
    assert_eq!(page_of(&dir, "notes/index.md"), "---\ntitle: Notes\n# why\ncascade:\n  syndicate:\n    - bluesky\n    - mastodon\n  listing: cards\n---\n");
    assert_eq!(read_folder(dir.path(), "notes/", "notes/index.md"), Some(after));
}

#[test]
fn a_folder_page_is_never_its_own_ancestor() {
    let dir = site("---\ntitle: A\n---\n");
    std::fs::write(dir.path().join("notes/index.md"), "---\ntitle: Notes\n---\n").unwrap();
    let mut map = ArticleMap::load(&dir.path().join(".moss")).unwrap();
    map.pages.insert("notes/".into(), "notes/index.md".into());
    map.save(&dir.path().join(".moss")).unwrap();
    std::fs::write(dir.path().join("notes/index.md"), "---\ntitle: Notes\ncascade:\n  syndicate: false\n---\n").unwrap();
    assert_eq!(set_folder(dir.path(), "notes/index.html", "notes/index.md", "bluesky", true).unwrap(), strings(&["bluesky"]));
    assert_eq!(read_folder(dir.path(), "notes/index.html", "notes/index.md"), Some(strings(&["bluesky"])));
}

#[test]
fn clearing_a_folder_under_one_that_names_channels_writes_false() {
    let dir = site("---\ntitle: A\n---\n");
    std::fs::write(dir.path().join("index.md"), "---\ntitle: Home\ncascade:\n  syndicate: [mastodon]\n---\n").unwrap();
    let mut map = ArticleMap::load(&dir.path().join(".moss")).unwrap();
    map.pages.insert(String::new(), "index.md".into());
    map.save(&dir.path().join(".moss")).unwrap();
    assert_eq!(set_folder(dir.path(), "notes/", "notes/index.md", "bluesky", false).unwrap(), Vec::<String>::new());
    assert!(page_of(&dir, "notes/index.md").contains("syndicate: false"), "removing the key would hand the folder the list above");
    assert_eq!(read_folder(dir.path(), "notes/", "notes/index.md"), Some(vec![]));
}
