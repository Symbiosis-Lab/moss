//! The manager's half of syndication: which installed channels take copies,
//! and calling one channel's copy hook with one job.

use std::collections::BTreeMap;

use super::{context_for_plugin, PluginManager};
use crate::plugins::types::{HookResult, Plugin, PluginManifest};
use crate::syndication::channel::{ChannelSpec, Form};
use crate::syndication::job::{AccountQuery, CopyJob, Op};

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
            let mut spec = copy_spec(&p.manifest)?.clone();
            let declared = p.manifest.domains.clone().unwrap_or_else(|| p.manifest.domain.iter().cloned().collect());
            let mut domains: Vec<String> = declared.iter().filter_map(|d| host_of(d)).collect();
            if spec.host_setting.is_some() || spec.long_setting.is_some() {
                let config = super::hook_context::hook_config_for_plugin(project_path, p);
                if let Some(key) = &spec.host_setting {
                    domains.extend(config.get(key).and_then(|v| v.as_str()).and_then(host_of));
                }
                if spec.long_setting.as_ref().is_some_and(|key| config.get(key).and_then(|v| v.as_bool()) != Some(true)) {
                    spec.forms.retain(|form| *form != Form::Long);
                }
            }
            Some((p.manifest.name.clone(), CopyChannel { spec, domains }))
        })
        .collect()
}

/// The host of an address or bare domain, with or without its scheme, in
/// the one form two spellings of it share: lowercase, with no port and no
/// trailing dot. `BSKY.APP`, `bsky.app.` and `https://bsky.app:443/x` are all
/// `bsky.app`.
pub(crate) fn host_of(address: &str) -> Option<String> {
    let address = address.trim();
    let parsed = url::Url::parse(address).ok().filter(|u| u.has_host()).or_else(|| url::Url::parse(&format!("https://{address}")).ok())?;
    let host = parsed.host_str()?.trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The account a channel is signed in to now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Account {
    /// The channel names no account (no `copy_account`, or an answer without
    /// one, as when the author posts by hand); its copies are not tied to one.
    Unsaid,
    Known(String),
    /// It exports `copy_account`, which failed.
    Unknown(String),
}

impl PluginManager {
    /// Installed channels whose manifest declares a `syndication` block,
    /// keyed by channel id.
    pub fn copy_channels(&self) -> BTreeMap<String, CopyChannel> {
        copy_channels_of(&self.project_path, &self.plugins.read().unwrap_or_else(|e| e.into_inner()))
    }

    /// Call `job.channel`'s hook for `job.op`, with the channel's own config.
    pub async fn send_copy(&self, job: &CopyJob) -> CopyCall {
        self.call_channel(&job.channel, job.op.hook(), job).await
    }

    /// Ask `channel`'s `copy_account` which account it is signed in to.
    pub async fn copy_account(&self, channel: &str) -> Account {
        let query = AccountQuery { channel: channel.to_string(), config: Default::default() };
        match self.call_channel(channel, "copy_account", &query).await {
            CopyCall::NoExport => Account::Unsaid,
            CopyCall::Answered(r) if !r.success => Account::Unknown(r.message.unwrap_or_else(|| "copy_account failed".into())),
            CopyCall::Answered(r) => match r.copy.and_then(|c| c.account).filter(|a| !a.is_empty()) {
                Some(account) => Account::Known(account),
                None => Account::Unsaid,
            },
            CopyCall::Unknown(e) => Account::Unknown(e),
        }
    }

    async fn call_channel<C: super::hook_context::HookContext + serde::Serialize>(&self, channel: &str, hook: &str, context: &C) -> CopyCall {
        let plugin = {
            let plugins = self.plugins.read().unwrap_or_else(|e| e.into_inner());
            plugins.iter().find(|p| p.manifest.name == channel).cloned()
        };
        let Some(plugin) = plugin else {
            return CopyCall::Unknown(format!("the {channel} plugin is not loaded"));
        };
        let context = context_for_plugin(&self.project_path, context, &plugin);
        match self.execute_hook_with_result(&plugin, hook, &context).await {
            Ok(Some(result)) => CopyCall::Answered(result),
            Ok(None) => CopyCall::Unknown(format!("{hook} returned no result")),
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

    #[test]
    fn a_channel_takes_articles_only_while_its_setting_is_on() {
        use crate::syndication::channel::Form;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let plugin = root.join(".moss/plugins/social");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("manifest.json"), serde_json::json!({
            "name": "social", "version": "0.0.1", "display_name": "Social", "description": "", "author": "",
            "entry": "main.bundle.js", "global_name": "SocialPlugin",
            "contributes": {"channel": {"syndication": {"forms": ["short", "long", "teaser"], "long_setting": "articles"}}}
        }).to_string()).unwrap();
        std::fs::write(plugin.join("main.bundle.js"), "window.SocialPlugin = {};").unwrap();
        std::fs::write(root.join(".moss/config.toml"), "[channels.social]\n").unwrap();
        let forms = || crate::syndication::landing::channels(root)["social"].spec.forms.clone();
        assert_eq!(forms(), [Form::Short, Form::Teaser], "unset is off");
        std::fs::write(plugin.join("config.json"), r#"{"articles": false}"#).unwrap();
        assert_eq!(forms(), [Form::Short, Form::Teaser]);
        std::fs::write(plugin.join("config.json"), r#"{"articles": true}"#).unwrap();
        assert_eq!(forms(), [Form::Short, Form::Long, Form::Teaser]);

        // An article posted while the setting was on is left as posted once it is off.
        use crate::syndication::{record::{CopyRecord, RemoteRef}, shape};
        let mut posted = CopyRecord::due("u", "social", "page.md", "page/");
        posted.form = Some(Form::Long);
        posted.remote = Some(RemoteRef { id: "1".into(), url: "https://social.example.net/1".into() });
        let post: crate::build::scan::article_map::ArticleInfo = serde_json::from_value(serde_json::json!({ "source_path": "page.md", "title": "Page", "content": "Body.", "url_path": "page/", "date": null })).unwrap();
        let reshape = || shape::shape_record(&post, &crate::syndication::landing::channels(root)["social"].spec, "https://example.org", &shape::ShapeOptions::default(), &posted);
        assert!(reshape().is_ok());
        std::fs::write(plugin.join("config.json"), r#"{"articles": false}"#).unwrap();
        assert_eq!(reshape().unwrap_err(), "retaining the prior version: the channel no longer accepts its original form");
    }
}
