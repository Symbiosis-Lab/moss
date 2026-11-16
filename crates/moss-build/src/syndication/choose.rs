//! What a post's channel chips show, and what a click on one writes. Reads and
//! writes the page's own files, so a read straight after a click is right
//! before the rebuild finishes. Nothing here touches the network.

use std::collections::BTreeMap;
use std::path::Path;

use moss_core::frontmatter::{self, FieldEdit};
use serde::Serialize;
use specta::Type;

use super::plan::{self, ChannelInfo, PostFacts, Verdict};
use super::Named;
use super::record::{Records, State};
use crate::build::cloud_readiness::{probe_input_to_string, rewrite_input};
use crate::build::manifest::live_baseline::{self, Baseline};
use crate::build::scan::article_map::{to_pretty_url, ArticleInfo, ArticleMap};
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
    let own = own.as_ref().map(Named::ids);
    let listed = serde_json::to_value(front.get("syndicated")).ok();
    let listed = crate::build::types::syndicated_urls(listed.as_ref());
    let folders = inherited(folder, &map, &post.url_path, channels);
    let mp = MossPaths::new(folder);
    let live = live_state(&live_baseline::load(&mp), post, uid);
    let view = PostView {
        facts: PostFacts { uid, own: own.unwrap_or_default(), listed: &listed, already_live: live != Live::No },
        list: own.unwrap_or(&folders),
        folders: &folders,
        has_own: own.is_some(),
        live,
        records: Records::in_vault(&mp),
    };
    let config = crate::build::site_config::read_project_config(&folder.to_string_lossy()).ok()?;
    Some(channels.iter().map(|(id, info)| {
        let opts = super::shape::ShapeOptions::new(&config, folder, &map, post, id, None);
        let verdict = plan::verdict(id, Some(info), &view.facts, &view.records, false);
        view.choice(id, plan::assess_verdict(verdict, post, info, &view.records, id, config.site_str("url"), &opts))
    }).collect())
}

/// One post as its chips read it.
struct PostView<'a> {
    facts: PostFacts<'a>,
    /// The resolved list: the post's own, or else the nearest folder's.
    list: &'a [String],
    folders: &'a [String],
    has_own: bool,
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
            _ if !named(self.list) => (false, if self.has_own && named(self.folders) { ChoiceOrigin::LeftOut } else { ChoiceOrigin::NotIncluded }),
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

/// The edit a post's list makes: a post following its folder has no key, but a
/// live post keeps its list, since removing the key would turn its channels off.
fn key_edit(list: &[String], folders: &[String], live: Live) -> FieldEdit {
    let key = "syndicate".to_string();
    if list == folders && (live == Live::No || list.is_empty()) {
        FieldEdit::Remove { key }
    } else if list.is_empty() {
        FieldEdit::Set { key, value: serde_yaml::Value::Bool(false) }
    } else {
        FieldEdit::Set { key, value: yaml_list(list) }
    }
}

/// Set one channel on a post, writing only its `syndicate:` line. Returns what
/// the chips show after the write.
pub fn set_post(folder: &Path, source: &str, channel: &str, on: bool) -> Result<Vec<ChannelChoice>, String> {
    let map = load_map(folder).ok_or("this site has not been built yet")?;
    let post = map.articles.values().find(|a| a.source_path == source).ok_or("this page is not a post")?;
    let uid = post.uid.as_deref().ok_or("this post has no note ID yet")?;
    let mp = MossPaths::new(folder);
    let live = live_state(&live_baseline::load(&mp), post, uid);
    let channels = super::landing::channels(folder);
    let folders = inherited(folder, &map, &post.url_path, &channels);
    rewrite_input(&on_disk(folder, source), |bytes| {
        let content = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let own: Option<Named> = frontmatter::parse(content).frontmatter.get("syndicate").map(|v| choice_of(v, &channels).unwrap_or_default());
        let mut list = next_list(own.as_ref().map_or(&folders, |n| n.ids()), &folders, channel, on);
        list.extend(kept(own.as_ref()));
        let edit = key_edit(&list, &folders, live);
        frontmatter::edit_fields(content, &[edit]).map(|after| (after != content).then_some(after))
    })?;
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

/// The channels a folder's own page sets for new posts: its own list, or the
/// nearest folder's above it.
pub fn read_folder(folder: &Path, url_path: &str, index_source: &str) -> Option<Vec<String>> {
    let url_path = to_pretty_url(url_path);
    let content = read_page(folder, index_source)?;
    let channels = super::landing::channels(folder);
    Some(match folder_own(&content, &channels) {
        Some(own) => own.ids().to_vec(),
        None => inherited(folder, &load_map(folder)?, &url_path, &channels),
    })
}

/// Set one channel in a folder page's `cascade: syndicate:`. Returns the
/// folder's list after the write.
pub fn set_folder(folder: &Path, url_path: &str, index_source: &str, channel: &str, on: bool) -> Result<Vec<String>, String> {
    let channels = super::landing::channels(folder);
    let above = inherited(folder, &load_map(folder).ok_or("this site has not been built yet")?, &to_pretty_url(url_path), &channels);
    let mut result = Vec::new();
    rewrite_input(&on_disk(folder, index_source), |bytes| {
        let content = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let own = folder_own(content, &channels);
        let list = next_list(own.as_ref().map_or(&above, |n| n.ids()), &above, channel, on);
        let written: Vec<String> = list.iter().cloned().chain(kept(own.as_ref())).collect();
        let value = if written == above {
            None
        } else if list.is_empty() {
            Some(serde_yaml::Value::Bool(false))
        } else {
            Some(yaml_list(&written))
        };
        result = list;
        let edit = FieldEdit::SetChild { parent: "cascade".into(), key: "syndicate".into(), value };
        frontmatter::edit_fields(content, &[edit]).map(|after| (after != content).then_some(after))
    })?;
    Ok(result)
}

#[cfg(test)]
#[path = "choose_tests.rs"]
mod tests;
