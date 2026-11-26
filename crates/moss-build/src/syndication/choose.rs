//! What a post's channel chips show, and what a click on one writes. Reads the
//! page's own files, so a read straight after a click is right before the
//! rebuild finishes. A click writes the page only where the author wrote a
//! `syndicate:` line; otherwise it is a [`Choices`] record. Nothing here
//! touches the network.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use moss_core::frontmatter::{self, FieldEdit};
use serde::Serialize;
use specta::Type;

use super::plan::{self, ChannelInfo, PostFacts, Verdict};
use super::Named;
use super::record::{Records, State};
use crate::build::cloud_readiness::{probe_input_to_string, rewrite_input};
use crate::build::manifest::live_baseline::{self, Baseline};
use crate::build::scan::article_map::{ArticleInfo, ArticleMap};
use crate::build::scan::path_collision::on_disk;
use crate::moss_paths::MossPaths;

/// Why a channel reads as it does on a post.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ChoiceOrigin {
    /// On: only the folder's list names it.
    Folder,
    /// On: the post's own list names it.
    Post,
    /// On: a copy stands for it.
    Copied,
    /// Off: the post's own list leaves out what the folder names.
    LeftOut,
    /// Off: live before only its folder named the channel.
    LiveBefore,
    /// Off: neither the post nor a folder names it.
    NotIncluded,
    /// Off: what is live could not be read, so a publish plans nothing.
    Unknown,
    /// The channel cannot take this page in any available form.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ChannelChoice {
    pub channel: String,
    pub on: bool,
    pub origin: ChoiceOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Whether the post was on the site before this publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Live {
    Yes,
    No,
    Unknown,
}

fn live_state(baseline: &Baseline, post: &ArticleInfo, uid: &str) -> Live {
    match baseline {
        Baseline::Present(b) if b.entries.iter().any(|e| e.uid == uid || e.url == post.url_path) => Live::Yes,
        Baseline::Present(_) | Baseline::Absent => Live::No,
        Baseline::Unreadable(_) => Live::Unknown,
    }
}

/// The folders above `url_path`, nearest first, as `ArticleMap.pages` keys them.
fn ancestors(url_path: &str) -> Vec<String> {
    let mut rest = url_path.trim_end_matches('/');
    if rest.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    while let Some((parent, _)) = rest.rsplit_once('/') {
        out.push(format!("{parent}/"));
        rest = parent;
    }
    out.push(String::new());
    out
}

fn read_page(folder: &Path, source: &str) -> Option<String> {
    probe_input_to_string(&on_disk(folder, source)).ok().flatten()
}

type Channels = BTreeMap<String, ChannelInfo>;

const HIDDEN: &str = "the page is reached only by its link (`search_engines: false`)";

/// The chips' clicks on posts with no `syndicate:` line, kept in moss's own
/// records rather than the page: one file per post and channel under
/// `.moss/deploy/choices/`, named `<uid>.<channel>` and holding `on` or `off`.
/// Not a copy record: a choice is made before any copy and outlives one. A
/// choice is kept only while it changes what the post asks for.
#[derive(Debug, Default)]
pub struct Choices(HashMap<String, Vec<(String, bool)>>);

fn choices_dir(mp: &MossPaths) -> PathBuf {
    mp.deploy_dir().join("choices")
}

impl Choices {
    pub fn in_vault(mp: &MossPaths) -> Self {
        let Ok(entries) = std::fs::read_dir(choices_dir(mp)) else { return Self::default() };
        let mut by_uid: HashMap<String, Vec<(String, bool)>> = HashMap::new();
        for entry in entries.filter_map(Result::ok) {
            let Ok(name) = entry.file_name().into_string() else { continue };
            let Some((uid, channel)) = name.split_once('.') else { continue };
            let on = match std::fs::read_to_string(entry.path()).ok().as_deref().map(str::trim) {
                Some("on") => true,
                Some("off") => false,
                _ => continue,
            };
            by_uid.entry(uid.to_string()).or_default().push((channel.to_string(), on));
        }
        by_uid.values_mut().for_each(|v| v.sort());
        Self(by_uid)
    }

    pub(super) fn any(&self, uid: &str) -> bool {
        self.0.contains_key(uid)
    }

    /// The channels the post's choices turn on.
    pub(super) fn turned_on(&self, uid: &str) -> Vec<String> {
        self.0.get(uid).into_iter().flatten().filter(|(_, on)| *on).map(|(c, _)| c.clone()).collect()
    }

    /// `ids` with the post's choices on top: `on` adds a channel, `off` takes one out.
    pub(super) fn apply(&self, uid: &str, ids: &mut Vec<String>) {
        for (channel, on) in self.0.get(uid).into_iter().flatten() {
            ids.retain(|c| c != channel);
            if *on {
                ids.push(channel.clone());
            }
        }
    }
}

/// Keep `on` as the post's choice for `channel`, or remove the choice (`None`).
fn record_choice(mp: &MossPaths, uid: &str, channel: &str, on: Option<bool>) -> Result<(), String> {
    if !super::record::plain_id(uid) || !super::record::plain_id(channel) {
        return Err(format!("a channel choice needs a plain note id and channel id, not {uid:?} / {channel:?}"));
    }
    let dir = choices_dir(mp);
    let path = dir.join(format!("{uid}.{channel}"));
    let Some(on) = on else {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("remove {}: {e}", path.display())),
            _ => Ok(()),
        };
    };
    let text = if on { "on\n" } else { "off\n" };
    if std::fs::read_to_string(&path).is_ok_and(|t| t == text) {
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    crate::infra::atomic_write::write_atomic(&path, text)
}

fn choice_of(value: &serde_yaml::Value, channels: &Channels) -> Result<Named, String> {
    let list = serde_json::to_value(value).map_err(|e| e.to_string()).and_then(|v| super::parse_choice(&v))?;
    Ok(Named::of(&list, channels))
}

/// A folder page's own `cascade: syndicate:`; `None` when absent or invalid,
/// which leaves the value to the folders above, as the cascade does.
fn folder_own(content: &str, channels: &Channels) -> Option<Named> {
    choice_of(frontmatter::parse(content).frontmatter.get("cascade")?.get("syndicate")?, channels).ok()
}

/// The list the nearest folder above `url_path` sets.
fn inherited(folder: &Path, map: &ArticleMap, url_path: &str, channels: &Channels) -> Vec<String> {
    ancestors(url_path)
        .iter()
        .filter_map(|key| map.pages.get(key))
        .find_map(|source| folder_own(&read_page(folder, source)?, channels))
        .map(|named| named.ids().to_vec())
        .unwrap_or_default()
}

fn load_map(folder: &Path) -> Option<ArticleMap> {
    ArticleMap::load(&folder.join(".moss")).ok()
}

/// What a post's chips show now.
pub fn read_post(folder: &Path, source: &str) -> Option<Vec<ChannelChoice>> {
    read_post_with(folder, source, &super::landing::channels(folder))
}

pub fn read_post_with(folder: &Path, source: &str, channels: &BTreeMap<String, ChannelInfo>) -> Option<Vec<ChannelChoice>> {
    let map = load_map(folder)?;
    let post = map.articles.values().find(|a| a.source_path == source)?;
    let uid = post.uid.as_deref()?;
    let content = read_page(folder, source)?;
    let front = frontmatter::parse(&content).frontmatter;
    // An invalid `syndicate:` is the page's say, as in the cascade: it names nothing.
    let own: Option<Named> = front.get("syndicate").map(|v| choice_of(v, channels).unwrap_or_default());
    let listed = serde_json::to_value(front.get("syndicated")).ok();
    let listed = crate::build::types::syndicated_urls(listed.as_ref());
    let folders = inherited(folder, &map, &post.url_path, channels);
    let mp = MossPaths::new(folder);
    let live = live_state(&live_baseline::load(&mp), post, uid);
    let choices = Choices::in_vault(&mp);
    let folders_named = Named { ids: folders.clone(), shared: Vec::new() };
    let asked = super::Asked::new(post.hidden_from_search_engines, own, Some(folders_named), uid, &choices);
    let list = asked.list.unwrap_or_default();
    let view = PostView {
        facts: PostFacts { uid, own: asked.own.ids(), listed: &listed, already_live: live != Live::No },
        list: list.ids(),
        folders: &folders,
        live,
        records: Records::in_vault(&mp),
    };
    let config = crate::build::site_config::read_project_config(&folder.to_string_lossy()).ok()?;
    Some(channels.iter().map(|(id, info)| {
        if post.hidden_from_search_engines {
            return view.choice(id, Verdict::Unavailable { reason: HIDDEN.into() });
        }
        let opts = super::shape::ShapeOptions::new(&config, folder, &map, post, id, None);
        let verdict = plan::verdict(id, Some(info), &view.facts, &view.records, false);
        view.choice(id, plan::assess_verdict(verdict, post, info, &view.records, id, config.site_str("url"), &opts))
    }).collect())
}

/// One post as its chips read it.
struct PostView<'a> {
    facts: PostFacts<'a>,
    /// What the post sends: its own list, or else the nearest folder's with
    /// its choices on top.
    list: &'a [String],
    folders: &'a [String],
    live: Live,
    records: Records,
}

impl PostView<'_> {
    /// A channel that cannot take the page reads so whether or not it is named.
    fn choice(&self, id: &str, verdict: Verdict) -> ChannelChoice {
        let named = |l: &[String]| l.iter().any(|c| c == id);
        let chosen = if named(self.facts.own) { ChoiceOrigin::Post } else { ChoiceOrigin::Folder };
        // A copy marks a channel only while the list names it, or leaving it
        // out could never turn the chip off.
        let (on, origin) = match verdict {
            Verdict::Unavailable { reason } => {
                return ChannelChoice { channel: id.to_string(), on: false, origin: ChoiceOrigin::Unavailable, reason: Some(reason) };
            }
            _ if !named(self.list) => (false, if named(self.folders) { ChoiceOrigin::LeftOut } else { ChoiceOrigin::NotIncluded }),
            Verdict::Settled(Some(State::Posted)) => (true, ChoiceOrigin::Copied),
            Verdict::Settled(_) => (true, chosen),
            _ if self.live == Live::Unknown => (false, ChoiceOrigin::Unknown),
            Verdict::LiveBeforeNamed => (false, ChoiceOrigin::LiveBefore),
            Verdict::NotSetUp => (false, ChoiceOrigin::NotIncluded),
            Verdict::Due | Verdict::Update | Verdict::Adopt { .. } => (true, chosen),
        };
        ChannelChoice { channel: id.to_string(), on, origin, reason: None }
    }
}

/// The names in `own` that more than one channel goes by: a write puts them
/// back as written, since no chip stands for them.
fn kept(own: Option<&Named>) -> impl Iterator<Item = String> + '_ {
    own.into_iter().flat_map(|n| n.shared().iter().map(|(written, _)| written.clone()))
}

/// `current` with `channel` added or taken out, keeping the folder's order.
fn next_list(current: &[String], folders: &[String], channel: &str, on: bool) -> Vec<String> {
    let mut list: Vec<String> = current.iter().filter(|c| *c != channel).cloned().collect();
    if on {
        let rank = |c: &str| folders.iter().position(|f| f == c);
        let at = rank(channel).and_then(|r| list.iter().position(|c| rank(c).is_some_and(|x| x > r))).unwrap_or(list.len());
        list.insert(at, channel.to_string());
    }
    list
}

fn yaml_list(list: &[String]) -> serde_yaml::Value {
    serde_yaml::Value::Sequence(list.iter().map(|c| serde_yaml::Value::String(c.clone())).collect())
}

/// Whether a post's word on its channels can be left to its folders: it says
/// what they say and the post is not live, or it names nothing. A live post
/// keeps its word, since it sends only what it names itself.
fn left_to_folders(follows: bool, names_any: bool, live: Live) -> bool {
    follows && (live == Live::No || !names_any)
}

/// The edit a post's own list makes.
fn key_edit(list: &[String], folders: &[String], live: Live) -> FieldEdit {
    let key = "syndicate".to_string();
    if left_to_folders(list == folders, !list.is_empty(), live) {
        FieldEdit::Remove { key }
    } else if list.is_empty() {
        FieldEdit::Set { key, value: serde_yaml::Value::Bool(false) }
    } else {
        FieldEdit::Set { key, value: yaml_list(list) }
    }
}

/// Set one channel on a post. A post with its own `syndicate:` line has that
/// line edited; any other keeps the click as a [`Choices`] record and its page
/// is not written. Returns what the chips show after.
pub fn set_post(folder: &Path, source: &str, channel: &str, on: bool) -> Result<Vec<ChannelChoice>, String> {
    let map = load_map(folder).ok_or("this site has not been built yet")?;
    let post = map.articles.values().find(|a| a.source_path == source).ok_or("this page is not a post")?;
    let uid = post.uid.as_deref().ok_or("this post has no note ID yet")?;
    if post.hidden_from_search_engines {
        return Err(HIDDEN.into());
    }
    let mp = MossPaths::new(folder);
    let live = live_state(&live_baseline::load(&mp), post, uid);
    let channels = super::landing::channels(folder);
    let folders = inherited(folder, &map, &post.url_path, &channels);
    let mut has_line = false;
    rewrite_input(&on_disk(folder, source), |bytes| {
        let content = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let Some(own) = frontmatter::parse(content).frontmatter.get("syndicate").map(|v| choice_of(v, &channels).unwrap_or_default()) else { return Ok(None) };
        has_line = true;
        let mut list = next_list(own.ids(), &folders, channel, on);
        list.extend(kept(Some(&own)));
        let edit = key_edit(&list, &folders, live);
        frontmatter::edit_fields(content, &[edit]).map(|after| (after != content).then_some(after))
    })?;
    // Where a line exists a choice would never apply, so none is kept.
    let follows = folders.iter().any(|c| c == channel) == on;
    record_choice(&mp, uid, channel, (!has_line && !left_to_folders(follows, on, live)).then_some(on))?;
    read_post_with(folder, source, &channels).ok_or_else(|| "the post could not be read back".to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Adopted {
    /// The address could not be written into the page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_back: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum AdoptRefusal {
    UnknownChannel,
    OffHost { expected: String },
    NotAPost,
    Busy,
    Unreadable,
    AlreadyPosted { url: String },
}

/// Add an existing copy's address to a post, settling its record if it has one.
pub fn adopt(folder: &Path, source: &str, channel: &str, url: &str) -> Result<Adopted, AdoptRefusal> {
    let channels = super::landing::channels(folder);
    let info = channels.get(channel).ok_or(AdoptRefusal::UnknownChannel)?;
    if !plan::on_host(url, &info.domains) {
        return Err(AdoptRefusal::OffHost { expected: info.domains.first().cloned().unwrap_or_default() });
    }
    let map = load_map(folder).ok_or(AdoptRefusal::NotAPost)?;
    let post = map.articles.values().find(|a| a.source_path == source).ok_or(AdoptRefusal::NotAPost)?;
    let uid = post.uid.as_deref().ok_or(AdoptRefusal::NotAPost)?;
    let records = Records::in_vault(&MossPaths::new(folder));
    let write_back = || Adopted { write_back: super::send::write_back(folder, &post.source_path, url).err() };
    if !records.exists(uid, channel) {
        return Ok(write_back());
    }
    let _turn = records.turn().ok_or(AdoptRefusal::Busy)?;
    let mut record = records.get(uid, channel).ok_or(AdoptRefusal::Unreadable)?;
    match record.state {
        State::Posted => {
            let existing = record.remote.as_ref().map(|r| r.url.as_str()).unwrap_or_default();
            if existing == url {
                Ok(write_back())
            } else {
                Err(AdoptRefusal::AlreadyPosted { url: existing.to_string() })
            }
        }
        State::Sending => Err(AdoptRefusal::Busy),
        State::Due | State::Refused | State::Unavailable | State::Unconfirmed | State::HandedOff => {
            record.source_path = post.source_path.clone();
            record.url_path = post.url_path.clone();
            let outcome = super::record::CopyOutcome { id: Some(url.into()), url: Some(url.into()), ..Default::default() };
            let sent = super::send::adopt(folder, &records, record, outcome);
            Ok(Adopted { write_back: sent.write_back })
        }
    }
}

#[cfg(test)]
#[path = "choose_tests.rs"]
mod tests;
