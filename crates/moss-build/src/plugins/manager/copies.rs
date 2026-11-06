//! The manager's half of syndication: which installed channels take copies,
//! and calling one channel's copy hook with one job.

use std::collections::BTreeMap;

use super::{context_for_plugin, PluginManager};
use crate::plugins::types::{HookResult, Plugin, PluginManifest};
use crate::syndication::channel::ChannelSpec;
use crate::syndication::job::{CopyJob, Op};

/// What a channel that takes copies declares.
#[derive(Debug, Clone, Default)]
pub struct CopyChannel {
    pub spec: ChannelSpec,
    /// Hosts its copies live on, so a `syndicated:` address there is taken as
    /// this channel's copy rather than posted again: the manifest's `domains`
    /// (else `domain`), and the host of the setting `host_setting` names.
    pub domains: Vec<String>,
}

/// How a copy hook call ended.
#[derive(Debug)]
pub enum CopyCall {
    Answered(HookResult),
    /// The channel does not export this hook.
    NoExport,
    /// It threw, timed out, or returned nothing: whether a copy exists is unknown.
    Unknown(String),
}

/// A copy hook that returns `success: false` is answering, not failing: the
/// network refused, and the refusal's cause is what the sender acts on. Every
/// other hook's `success: false` stays an error.
pub(super) fn refusal_is_an_answer(hook: &str) -> bool {
    Op::ALL.iter().any(|op| op.hook() == hook)
}

/// The channel's `syndication` block, when it posts through the copy contract.
pub fn copy_spec(manifest: &PluginManifest) -> Option<&ChannelSpec> {
    manifest.contributes.as_ref()?.channel.as_ref()?.syndication.as_ref()
}

/// The channels among `plugins` that the site has set up and that take
/// copies. Reads manifests only, so a publish can plan without an engine.
pub fn copy_channels_of(project_path: &str, plugins: &[Plugin]) -> BTreeMap<String, CopyChannel> {
    let installed = crate::plugins::discovery::get_channels_config(project_path).unwrap_or_default();
    plugins
        .iter()
        .filter(|p| installed.is_installed(&p.manifest.name))
        .filter_map(|p| {
            let spec = copy_spec(&p.manifest)?.clone();
            let mut domains = p.manifest.domains.clone().unwrap_or_else(|| p.manifest.domain.iter().cloned().collect());
            if let Some(key) = &spec.host_setting {
                let config = super::hook_context::hook_config_for_plugin(project_path, p);
                domains.extend(config.get(key).and_then(|v| v.as_str()).and_then(host_of));
            }
            Some((p.manifest.name.clone(), CopyChannel { spec, domains }))
        })
        .collect()
}

/// The host of an address an author typed, with or without its scheme.
fn host_of(address: &str) -> Option<String> {
    let rest = address.trim().split("://").last()?;
    rest.split(['/', '?', '#']).next().filter(|h| !h.is_empty()).map(str::to_string)
}

impl PluginManager {
    /// Installed channels whose manifest declares a `syndication` block,
    /// keyed by channel id.
    pub fn copy_channels(&self) -> BTreeMap<String, CopyChannel> {
        copy_channels_of(&self.project_path, &self.plugins.read().unwrap_or_else(|e| e.into_inner()))
    }

    /// Call `job.channel`'s hook for `job.op`, with the channel's own config.
    pub async fn send_copy(&self, job: &CopyJob) -> CopyCall {
        let plugin = {
            let plugins = self.plugins.read().unwrap_or_else(|e| e.into_inner());
            plugins.iter().find(|p| p.manifest.name == job.channel).cloned()
        };
        let Some(plugin) = plugin else {
            return CopyCall::Unknown(format!("the {} plugin is not loaded", job.channel));
        };
        let job = context_for_plugin(&self.project_path, job, &plugin);
        match self.execute_hook_with_result(&plugin, job.op.hook(), &job).await {
            Ok(Some(result)) => CopyCall::Answered(result),
            Ok(None) => CopyCall::Unknown(format!("{} returned no result", job.op.hook())),
            // The engine reports a missing export by this exact substring.
            Err(e) if e.contains("Hook function not found") => CopyCall::NoExport,
            Err(e) => CopyCall::Unknown(e),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_channel_on_the_authors_own_server_owns_that_servers_addresses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let plugin = root.join(".moss/plugins/social");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("manifest.json"), serde_json::json!({
            "name": "social", "version": "0.0.1", "display_name": "Social", "description": "", "author": "",
            "entry": "main.bundle.js", "global_name": "SocialPlugin",
            "contributes": {"channel": {"syndication": {"forms": ["short"], "host_setting": "server"}}}
        }).to_string()).unwrap();
        std::fs::write(plugin.join("main.bundle.js"), "window.SocialPlugin = {};").unwrap();
        std::fs::write(plugin.join("config.json"), r#"{"server": "https://social.example.net/"}"#).unwrap();
        std::fs::write(root.join(".moss/config.toml"), "[channels.social]\n").unwrap();
        let channels = crate::syndication::landing::channels(root);
        assert_eq!(channels["social"].domains, ["social.example.net"]);
    }
}
