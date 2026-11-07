//! Planning at a landed publish: what was live before is read first, then the
//! publish record moves, then the plan is made against the before.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use super::plan::{self, ChannelInfo, PlanInputs, Planned};
use super::record::Records;
use crate::build::manifest::live_baseline::{self, Baseline};
use crate::build::manifest::{change_set, published_record, SealedManifest};
use crate::build::scan::article_map::ArticleMap;
use crate::moss_paths::MossPaths;

/// What the plan is judged against, captured before the publish record moves.
/// `None` when the site has no channel that takes copies: nothing else is read.
pub struct Before {
    channels: BTreeMap<String, ChannelInfo>,
    site_url: Option<String>,
    baseline: Baseline,
    edited: HashSet<String>,
}

impl Before {
    pub fn capture(mp: &MossPaths, sealed: &SealedManifest, target: Option<&str>) -> Option<Self> {
        let channels = channels(mp.project_root());
        if channels.is_empty() {
            return None;
        }
        let previous = published_record::load_for(mp, target);
        let edited = change_set::classify(previous.as_ref(), sealed)
            .pages
            .into_iter()
            .filter(|p| p.verb == change_set::PageVerb::Edited)
            .map(|p| p.source_path)
            .collect();
        Some(Self { channels, site_url: canonical(sealed).map(str::to_string), baseline: live_baseline::load(mp), edited })
    }

    /// Decide without writing anything: what a publish of `live` would plan.
    pub fn decide(&self, folder: &Path, live: &ArticleMap) -> Vec<plan::PostPlan> {
        let previously_live = match &self.baseline {
            Baseline::Present(b) => Some(b.entries.as_slice()),
            Baseline::Absent => None,
            Baseline::Unreadable(why) => {
                log::warn!(target: "syndication", "no copies planned: what was live before this publish could not be read ({why:?})");
                return Vec::new();
            }
        };
        let inputs = PlanInputs { articles: live, previously_live, edited_sources: &self.edited, channels: &self.channels };
        plan::decide(&inputs, &Records::in_vault(&MossPaths::new(folder)))
    }

    pub fn channels(&self) -> &BTreeMap<String, ChannelInfo> {
        &self.channels
    }

    /// Plan against what just went live, and write the records.
    pub fn plan(self, folder: &Path, live: &ArticleMap) -> Vec<Planned> {
        let plans = self.decide(folder, live);
        let records = Records::in_vault(&MossPaths::new(folder));
        let planned = plan::apply(plans, &records, self.site_url.as_deref());
        for p in &planned {
            if let plan::Action::Skip { reason } = &p.action {
                log::info!(target: "syndication", "{} — no copy on {}: {reason}", p.source_path, p.channel);
            }
        }
        planned
    }
}

/// The address the build gave the site, when it is one a reader can open. A
/// site with no address of its own builds against a loopback placeholder; its
/// copies link to wherever the publish reports instead.
pub fn canonical(sealed: &SealedManifest) -> Option<&str> {
    sealed.site_url().filter(|u| crate::build::site_url::SiteUrl::parse(u).is_ok_and(|s| s.is_deployed()))
}

/// The site's channels that take copies, read from their manifests.
pub fn channels(folder: &Path) -> BTreeMap<String, ChannelInfo> {
    let folder = folder.to_string_lossy();
    let Ok(installed) = crate::plugins::discovery::scan_installed_plugins(&folder) else { return BTreeMap::new() };
    crate::plugins::manager::copies::copy_channels_of(&folder, &installed.loaded)
}
