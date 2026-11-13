//! Planning at a landed publish: what was live before is read first, then the
//! publish record moves, then the plan is made against the before.

use std::collections::{BTreeMap, HashMap, HashSet};
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
        Some(Self {
            channels,
            site_url: canonical(sealed).map(str::to_string),
            baseline: live_baseline::load(mp),
            edited,
        })
    }

    /// Decide without writing anything: what a publish of `live` would plan.
    pub fn decide(&self, folder: &Path, live: &ArticleMap) -> Vec<plan::PostPlan> {
        let config = match crate::build::site_config::read_project_config(&folder.to_string_lossy()) {
            Ok(config) => config,
            Err(e) => { log::warn!(target: "syndication", "no copies planned: {e}"); return Vec::new(); }
        };
        self.decide_with(folder, live, &config, &Records::in_vault(&MossPaths::new(folder)))
    }

    /// Decide using the preview's already-loaded configuration and records.
    pub(super) fn decide_with(&self, folder: &Path, live: &ArticleMap, config: &crate::config::ConfigFile, records: &Records) -> Vec<plan::PostPlan> {
        let previously_live = match &self.baseline {
            Baseline::Present(b) => Some(b.entries.as_slice()),
            Baseline::Absent => None,
            Baseline::Unreadable(why) => {
                log::warn!(target: "syndication", "no copies planned: what was live before this publish could not be read ({why:?})");
                return Vec::new();
            }
        };
        let inputs = PlanInputs { config, folder, site_url: self.site_url.as_deref(), articles: live, previously_live, edited_sources: &self.edited, channels: &self.channels };
        plan::decide(&inputs, records)
    }

    pub fn channels(&self) -> &BTreeMap<String, ChannelInfo> {
        &self.channels
    }

    /// Write the records for `plans`, which [`Self::decide`] made against
    /// `live`, what just went live; `unstamped` names the pages whose uid the
    /// publish could not write, source path → why.
    pub fn apply(self, folder: &Path, plans: Vec<plan::PostPlan>, live: Option<&ArticleMap>, unstamped: &HashMap<String, String>) -> Vec<Planned> {
        let records = Records::in_vault(&MossPaths::new(folder));
        let planned = plan::apply(plans, &records, self.site_url.as_deref(), live, unstamped);
        for p in &planned {
            match &p.action {
                plan::Action::Unavailable { reason } => crate::build::cli_output::log_note!("{} — no copy on {}: {reason}", p.source_path, p.channel),
                plan::Action::Skip { reason } => log::info!(target: "syndication", "{} — no copy on {}: {reason}", p.source_path, p.channel),
                _ => {}
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
