//! Which copies a landed publish makes due. Local files only: no network call
//! happens here, so planning costs a publish nothing it has to wait for.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use super::record::{CopyRecord, Records, RemoteRef, State};
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
        let entry = |channel: &str, action: Action| Planned {
            source_path: post.source_path.clone(),
            url_path: post.url_path.clone(),
            channel: channel.to_string(),
            action,
        };
        // The uid the page was built under, which its live copy carries.
        let uid = post.uid.clone().unwrap_or_default();
        let own: Vec<String> =
            post.frontmatter.get("syndicate").and_then(|v| super::parse_choice(v).ok()).unwrap_or_default();
        let listed = crate::build::types::syndicated_urls(post.frontmatter.get("syndicated"));
        let already_live = was_live.contains(uid.as_str()) || was_live.contains(post.url_path.as_str());

        let mut entries = Vec::new();
        for channel in channels {
            // The newsletter, which the app sends itself.
            if channel == RESERVED_EMAIL {
                continue;
            }
            let action = if let Some(info) = inputs.channels.get(channel) {
                if records.exists(&uid, channel) {
                    match records.get(&uid, channel) {
                        Some(r) if r.state == State::Posted && inputs.edited_sources.contains(&post.source_path) => Action::Update,
                        // A copy the author posted by hand, or found after an
                        // uncertain send, is adopted once its address is listed.
                        Some(r) if matches!(r.state, State::HandedOff | State::Unconfirmed | State::Due | State::Refused) => {
                            match listed.iter().find(|u| on_host(u, &info.domains)) {
                                Some(url) => Action::Adopt { url: url.to_string() },
                                None => continue,
                            }
                        }
                        _ => continue,
                    }
                } else if let Some(url) = listed.iter().find(|u| on_host(u, &info.domains)) {
                    Action::Adopt { url: url.to_string() }
                } else if already_live && !own.contains(channel) {
                    Action::Skip { reason: "it was live before the folder named this channel; name the channel in the page itself to post it".into() }
                } else {
                    Action::Due
                }
            } else {
                Action::Skip { reason: format!("no channel named {channel} is set up for this site") }
            };
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
fn on_host(url: &str, domains: &[String]) -> bool {
    if !url.contains("://") {
        return false;
    }
    let Some(host) = crate::plugins::manager::copies::host_of(url) else { return false };
    domains.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// Carry the plan out: write the copy records.
pub fn apply(plans: Vec<PostPlan>, records: &Records, site_url: Option<&str>) -> Vec<Planned> {
    let mut out = Vec::new();
    for mut plan in plans {
        if plan.uid.is_empty() {
            for entry in &mut plan.entries {
                entry.action = Action::Skip { reason: "the page has no note id".into() };
            }
        }
        for entry in plan.entries {
            let saved = match &entry.action {
                Action::Due => {
                    let mut r = CopyRecord::due(&plan.uid, &entry.channel, &entry.source_path, &entry.url_path);
                    r.site_url = site_url.map(str::to_string);
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
