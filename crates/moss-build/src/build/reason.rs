//! Why the host asked for a build. [`super::BuildTrigger`] says what changed on
//! disk; this says what the person did. Every token is fixed text from this
//! file or a `[site]` key name from the build's own schema, so a line that
//! carries one carries nothing a person wrote.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildReason {
    /// The folder was opened.
    Open,
    /// The watcher saw files change.
    Watch,
    /// The person asked for a rebuild.
    Manual,
    /// A publish builds before it uploads.
    Publish,
    /// `.moss/config.toml` changed outside `[site]`, or in several `[site]` keys.
    Config,
    /// One `[site]` key changed: its schema name, or `"other"` for a key the
    /// build does not read.
    Setting(&'static str),
    /// A plugin was installed, updated, removed or trusted.
    Plugins,
    /// The site's published address changed.
    SiteAddress,
}

impl BuildReason {
    /// The reason for a change to `[site]` key `key`.
    pub fn setting(key: &str) -> Self {
        let known = crate::config::site_settings::site_settings();
        Self::Setting(known.iter().map(|s| s.name).find(|&name| name == key).unwrap_or("other"))
    }

    /// Why a config change asked for a build: the one `[site]` key that changed,
    /// or `Config` for any other change. `None` when the two parse the same.
    pub fn for_config_change(before: &str, after: &str) -> Option<Self> {
        let (Ok(before), Ok(after)) = (before.parse::<toml::Table>(), after.parse::<toml::Table>()) else {
            return Some(Self::Config);
        };
        if before == after {
            return None;
        }
        let rest = |t: &toml::Table| t.iter().filter(|(k, _)| *k != "site").map(|(k, v)| (k.clone(), v.clone())).collect::<toml::Table>();
        if rest(&before) != rest(&after) {
            return Some(Self::Config);
        }
        let site = |t: &toml::Table| t.get("site").and_then(toml::Value::as_table).cloned().unwrap_or_default();
        let (before, after) = (site(&before), site(&after));
        let keys: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        let mut changed = keys.into_iter().filter(|k| before.get(*k) != after.get(*k));
        match (changed.next(), changed.next()) {
            (Some(key), None) => Some(Self::setting(key)),
            _ => Some(Self::Config),
        }
    }

    /// The reason for one build made of two requests. A watcher batch says
    /// least, so either other reason wins over it. A setting outranks `Config`,
    /// which is how the watcher reports the same write of `config.toml`; two
    /// different settings are `Config`. Otherwise the earlier reason stands.
    pub fn merge(self, other: Self) -> Self {
        use BuildReason::{Config, Setting, Watch};
        match (self, other) {
            (a, b) if a == b => a,
            (Watch, b) => b,
            (Setting(_), Setting(_)) => Config,
            (Config, b @ Setting(_)) => b,
            (a, _) => a,
        }
    }

    /// Every token a build can carry, for a reader that checks it accepts them all.
    pub fn every() -> Vec<Self> {
        use BuildReason::{Config, Manual, Open, Plugins, Publish, SiteAddress, Watch};
        let settings = crate::config::site_settings::site_settings().into_iter().map(|s| Self::Setting(s.name));
        [Open, Watch, Manual, Publish, Config, Plugins, SiteAddress].into_iter().chain(settings).chain([Self::Setting("other")]).collect()
    }
}

impl fmt::Display for BuildReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => f.write_str("open"),
            Self::Watch => f.write_str("watch"),
            Self::Manual => f.write_str("manual"),
            Self::Publish => f.write_str("publish"),
            Self::Config => f.write_str("config"),
            Self::Setting(key) => write!(f, "setting:{key}"),
            Self::Plugins => f.write_str("plugins"),
            Self::SiteAddress => f.write_str("site-address"),
        }
    }
}

#[cfg(test)]
#[path = "reason_tests.rs"]
mod tests;
