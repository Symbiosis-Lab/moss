//! Which copies a landed publish makes due. Local files only: no network call
//! happens here, so planning costs a publish nothing it has to wait for.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;

use super::record::{CopyRecord, PublishedPost, Records, RemoteRef, State};
use crate::build::manifest::change_set::LiveEntry;
use crate::build::scan::article_map::{ArticleInfo, ArticleMap};

/// A `syndicate:` id that names the site's newsletter, not a channel plugin.
pub const RESERVED_EMAIL: &str = "email";

pub use crate::plugins::manager::copies::CopyChannel as ChannelInfo;

pub struct PlanInputs<'a> {
    /// The pages that just went live.
    pub articles: &'a ArticleMap,
    /// What was live on any target before this publish; `None` when nothing
    /// ever was. An unreadable baseline is not `None`: the caller does not plan.
    pub previously_live: Option<&'a [LiveEntry]>,
    /// Sources whose content changed in this publish.
    pub edited_sources: &'a HashSet<String>,
    pub channels: &'a BTreeMap<String, ChannelInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// A new copy to post.
    Due,
    /// The post changed since its copy was sent.
    Update,
    /// The post already lists a copy on this channel's host.
    Adopt { url: String },
    Skip { reason: String },
}

impl Action {
    /// Whether carrying this out writes a copy record that did not exist,
    /// keyed by the post's uid.
    fn starts_a_record(&self) -> bool {
        matches!(self, Action::Due | Action::Adopt { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Planned {
    pub source_path: String,
    pub url_path: String,
    pub channel: String,
    #[serde(flatten)]
    pub action: Action,
}

/// One post's part of the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostPlan {
    pub uid: String,
    pub entries: Vec<Planned>,
}

impl PostPlan {
    /// The post's source file, which every entry shares.
    pub fn source_path(&self) -> Option<&str> {
        self.entries.first().map(|e| e.source_path.as_str())
    }
}

/// What a publish makes of one channel a post's resolved list names. The
/// channel chip reads the same verdict, so what it shows is what goes out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Verdict {
    Due,
    Update,
    Adopt { url: String },
    NotSetUp,
    /// Live before only its folder named the channel.
    LiveBeforeNamed,
    /// A record stands and nothing is owed now; `None` when it cannot be read.
    Settled(Option<State>),
}

impl Verdict {
    /// The plan's entry for this verdict, when it has one.
    fn into_action(self, channel: &str) -> Option<Action> {
        match self {
            Verdict::Due => Some(Action::Due),
            Verdict::Update => Some(Action::Update),
            Verdict::Adopt { url } => Some(Action::Adopt { url }),
            Verdict::NotSetUp => Some(Action::Skip { reason: format!("no channel named {channel} is set up for this site") }),
            Verdict::LiveBeforeNamed => Some(Action::Skip { reason: "it was live before the folder named this channel; name the channel in the page itself to post it".into() }),
            Verdict::Settled(_) => None,
        }
    }
}

/// The parts of a post a verdict reads.
pub(super) struct PostFacts<'a> {
    /// The uid the page was built under, which keys its copies.
    pub uid: &'a str,
    /// The channels the page itself names.
    pub own: &'a [String],
    /// Its `syndicated:` addresses.
    pub listed: &'a [String],
    pub already_live: bool,
}

/// One channel's verdict for a post. Only meaningful for a channel the post's
/// resolved list names.
pub(super) fn verdict(channel: &str, info: Option<&ChannelInfo>, post: &PostFacts<'_>, records: &Records, edited: bool) -> Verdict {
    let Some(info) = info else { return Verdict::NotSetUp };
    let adopt = || post.listed.iter().find(|u| on_host(u, &info.domains)).map(|url| Verdict::Adopt { url: url.clone() });
    if records.exists(post.uid, channel) {
        return match records.get(post.uid, channel) {
            Some(r) if r.state == State::Posted && edited => Verdict::Update,
            // A copy the author posted by hand, or found after an uncertain
            // send, is adopted once its address is listed.
            Some(r) if matches!(r.state, State::HandedOff | State::Unconfirmed | State::Due | State::Refused) => {
                adopt().unwrap_or(Verdict::Settled(Some(r.state)))
            }
            other => Verdict::Settled(other.map(|r| r.state)),
        };
    }
    adopt().unwrap_or(if post.already_live && !post.own.iter().any(|c| c == channel) { Verdict::LiveBeforeNamed } else { Verdict::Due })
}

/// Decide, reading nothing but `inputs` and the existing records.
pub fn decide(inputs: &PlanInputs<'_>, records: &Records) -> Vec<PostPlan> {
    let was_live: HashSet<&str> = inputs
        .previously_live
        .unwrap_or_default()
        .iter()
        .flat_map(|e| [e.uid.as_str(), e.url.as_str()])
        .collect();

    let mut articles: Vec<&ArticleInfo> = inputs.articles.articles.values().collect();
    articles.sort_by(|a, b| a.url_path.cmp(&b.url_path));

    let mut plans = Vec::new();
    for post in articles {
        let Some(channels) = post.syndicate.as_ref().filter(|c| !c.is_empty()) else { continue };
        let Some(uid) = post.uid.clone() else { continue };
        let entry = |channel: &str, action: Action| Planned {
            source_path: post.source_path.clone(),
            url_path: post.url_path.clone(),
            channel: channel.to_string(),
            action,
        };
        let own: Vec<String> =
            post.frontmatter.get("syndicate").and_then(|v| super::parse_choice(v).ok()).unwrap_or_default();
        let listed = crate::build::types::syndicated_urls(post.frontmatter.get("syndicated"));
        let already_live = was_live.contains(uid.as_str()) || was_live.contains(post.url_path.as_str());
        let facts = PostFacts { uid: &uid, own: &own, listed: &listed, already_live };

        let mut entries = Vec::new();
        for channel in channels {
            // The newsletter, which the app sends itself.
            if channel == RESERVED_EMAIL {
                continue;
            }
            let edited = inputs.edited_sources.contains(&post.source_path);
            let Some(action) = verdict(channel, inputs.channels.get(channel), &facts, records, edited).into_action(channel) else { continue };
            entries.push(entry(channel, action));
        }
        if !entries.is_empty() {
            plans.push(PostPlan { uid, entries });
        }
    }
    plans
}

/// Whether `url` is on one of a channel's domains, which are already in
/// [`host_of`](crate::plugins::manager::copies::host_of)'s form.
pub(super) fn on_host(url: &str, domains: &[String]) -> bool {
    if !url.contains("://") {
        return false;
    }
    let Some(host) = crate::plugins::manager::copies::host_of(url) else { return false };
    domains.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// Carry the plan out: write the records. A post whose uid the publish could
/// not write into the page (`unstamped`, source path → why) gets no new record,
/// and says why: its next build could give it another uid. A copy made due
/// or due for an update keeps its post as `published`, the build that went
/// live, holds it.
pub fn apply(plans: Vec<PostPlan>, records: &Records, site_url: Option<&str>, published: Option<&ArticleMap>, unstamped: &HashMap<String, String>) -> Vec<Planned> {
    let by_uid: HashMap<String, &ArticleInfo> =
        published.into_iter().flat_map(|map| map.articles.values()).filter_map(|a| Some((a.uid.clone()?, a))).collect();
    let post_of = |uid: &str| Some(Box::new(PublishedPost::new(by_uid.get(uid)?, published?)));
    let mut out = Vec::new();
    for mut plan in plans {
        if let Some(e) = plan.source_path().and_then(|path| unstamped.get(path)) {
            for entry in &mut plan.entries {
                if entry.action.starts_a_record() {
                    entry.action = Action::Skip { reason: format!("its note id could not be written into the page ({e})") };
                }
            }
        }
        for entry in plan.entries {
            let saved = match &entry.action {
                Action::Due => {
                    let mut r = CopyRecord::due(&plan.uid, &entry.channel, &entry.source_path, &entry.url_path);
                    r.site_url = site_url.map(str::to_string);
                    r.post = post_of(&plan.uid);
                    records.save(&r)
                }
                Action::Adopt { url } => {
                    let mut r = CopyRecord::due(&plan.uid, &entry.channel, &entry.source_path, &entry.url_path);
                    r.state = State::Posted;
                    r.remote = Some(RemoteRef { id: url.clone(), url: url.clone() });
                    records.save(&r)
                }
                Action::Update => match records.get(&plan.uid, &entry.channel) {
                    Some(mut r) => {
                        r.update_due = true;
                        r.site_url = site_url.map(str::to_string).or(r.site_url);
                        r.post = post_of(&plan.uid);
                        r.touch();
                        records.save(&r)
                    }
                    None => Ok(()),
                },
                Action::Skip { .. } => Ok(()),
            };
            match saved {
                Ok(()) => out.push(entry),
                Err(e) => out.push(Planned { action: Action::Skip { reason: format!("its copy record could not be written ({e})") }, ..entry }),
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
