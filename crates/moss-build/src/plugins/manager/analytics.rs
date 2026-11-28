//! The analytics part's journey, one code path for the app and the terminal:
//! connect the plugin's account to the site, then read back what it counted.

use super::part_call::{DailyCount, PartCall, PartRefusal};
use super::PluginManager;
use crate::identity::keystore::Scope;
use crate::identity::secrets::SecretStore;
use crate::plugins::contributions::provider::{self, SignIn};

/// The file the counts are kept in, under `.moss/data/`.
pub const VISITS_FILE: &str = "visits.jsonl";

/// How far back the first read-back reaches.
const FIRST_READ_DAYS: i64 = 30;

impl PluginManager {
    /// Keep `paste` as the plugin's token (or use the one kept, when `None`),
    /// ask the plugin for the site's id at its domain, and name the plugin in
    /// `[services.analytics]`. Returns the site's id.
    pub async fn connect_analytics(&self, plugin_name: &str, paste: Option<&str>) -> Result<String, PartRefusal> {
        let failed = |message: String| PartRefusal::Failed { message };
        let plugin = self.find_plugin_by_name(plugin_name).ok_or(PartRefusal::NotServed)?;
        if let (Some(paste), Some(SignIn::Key(credential))) = (paste, provider::sign_in(&plugin.manifest)) {
            let key = credential.fits(paste).ok_or_else(|| PartRefusal::WrongKey { shape: credential.shape() })?;
            let scope = Scope::plugin(plugin_name).map_err(|e| failed(format!("{e:?}")))?;
            SecretStore::require("connecting analytics")
                .map_err(failed)?
                .set(&scope, provider::TOKEN, &key)
                .map_err(|e| failed(format!("{e:?}")))?;
        }
        let host = crate::build::site_config::get_domain_config(&self.project_path)
            .map_err(failed)?
            .domain
            .ok_or_else(|| failed("the site has no domain yet: set [site].url first".into()))?;
        let answer = self.call_part(plugin_name, &PartCall::AnalyticsSite { host }).await?;
        let site_id = answer["site_id"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| failed(format!("{plugin_name} answered no site_id")))?;
        crate::vault::config::save_analytics_service(&self.project_path, plugin_name, site_id).map_err(failed)?;
        Ok(site_id.to_string())
    }

    /// Read what the connected analytics plugin counted since the last read
    /// (its last day again, which may have grown), into `.moss/data/visits.jsonl`.
    /// Returns how many days' rows were written; `NotServed` when the site
    /// names no analytics plugin.
    pub async fn read_analytics(&self) -> Result<usize, PartRefusal> {
        let failed = |message: String| PartRefusal::Failed { message };
        let services = crate::build::site_config::get_services_config(&self.project_path).map_err(failed)?;
        let service = services.analytics.ok_or(PartRefusal::NotServed)?;
        let (plugin_name, site_id) = service.plugin().ok_or(PartRefusal::NotServed)?;
        let file = crate::moss_paths::MossPaths::from_project_str(&self.project_path).data_dir().join(VISITS_FILE);
        let kept = read_visits(&file);
        let since = kept.iter().map(|c| c.date.as_str()).max().map(str::to_string).unwrap_or_else(|| {
            (chrono::Utc::now().date_naive() - chrono::Days::new(FIRST_READ_DAYS as u64)).to_string()
        });
        let call = PartCall::AnalyticsRead { site_id: site_id.to_string(), since: since.clone() };
        let answer = self.call_part(plugin_name, &call).await?;
        let read: Vec<DailyCount> = serde_json::from_value(answer).map_err(|e| failed(format!("{plugin_name} answered counts moss cannot read: {e}")))?;
        let days = read.iter().map(|c| &c.date).collect::<std::collections::BTreeSet<_>>().len();
        let rows: Vec<_> = kept.into_iter().filter(|c| c.date < since).chain(read).collect();
        write_visits(&file, &rows).map_err(failed)?;
        Ok(days)
    }
}

/// After a publish, read the counts back when the site names an analytics
/// plugin: the line to print, `None` when it names none.
pub async fn read_after_publish(folder: &str) -> Option<String> {
    let services = crate::build::site_config::get_services_config(folder).ok()?;
    let plugin = services.analytics.as_ref()?.plugin()?.0.to_string();
    let host = crate::plugins::adapter_host::AdapterHost::headless(crate::build::ports::reporter::discard_owned());
    let read = match PluginManager::new(folder, host) {
        Ok(manager) => manager.read_analytics().await,
        Err(message) => Err(PartRefusal::Failed { message }),
    };
    Some(match read {
        Ok(days) => format!("Read {days} days of visits from {plugin}"),
        Err(refusal) => format!("Visits not read from {plugin}: {}", refusal.words(&plugin, None)),
    })
}

/// The counts kept, oldest first; a line moss cannot read is skipped.
pub fn read_visits(file: &std::path::Path) -> Vec<DailyCount> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn write_visits(file: &std::path::Path, rows: &[DailyCount]) -> Result<(), String> {
    let mut text = String::new();
    for row in rows {
        text.push_str(&serde_json::to_string(row).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crate::infra::atomic_write::write_atomic(file, text)
}

#[cfg(test)]
#[path = "analytics_tests.rs"]
mod tests;
