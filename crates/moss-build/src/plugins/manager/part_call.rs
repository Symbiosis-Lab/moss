//! A provider part's code, called by moss: the address part's zone verbs and
//! the analytics part's two. The app and the terminal call the same door.

use super::{AskedBy, HookRun, PluginManager};
use crate::plugins::contributions::provider::{self, Part};
use serde::{Deserialize, Serialize};

/// One record of a zone, as the address part's verbs read and write it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DnsRecord {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub content: String,
    pub ttl: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxied: Option<bool>,
}

/// Visits to one page on one day, what an analytics part that `reads`
/// `daily_counts` returns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct DailyCount {
    pub date: String,
    pub path: String,
    pub visits: u64,
}

/// A verb of a part's code with its argument, the one object the export
/// receives.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(untagged)]
pub enum PartCall {
    /// `dns_zones`: the account's zones, or the one named.
    DnsZones { name: Option<String> },
    /// `dns_records`: every record of a zone.
    DnsRecords { zone: String },
    /// `dns_set`: replace every record of one name and type.
    DnsSet {
        zone: String,
        name: String,
        #[serde(rename = "type")]
        kind: String,
        records: Vec<DnsRecord>,
    },
    /// `dns_delete`: remove one record.
    DnsDelete { zone: String, record: DnsRecord },
    /// `analytics_site`: the site id for `host`, made when there is none.
    AnalyticsSite { host: String },
    /// `analytics_read`: what the service counted since `since`, an ISO date.
    AnalyticsRead { site_id: String, since: String },
}

impl PartCall {
    /// The export's name.
    pub fn verb(&self) -> &'static str {
        match self {
            PartCall::DnsZones { .. } => "dns_zones",
            PartCall::DnsRecords { .. } => "dns_records",
            PartCall::DnsSet { .. } => "dns_set",
            PartCall::DnsDelete { .. } => "dns_delete",
            PartCall::AnalyticsSite { .. } => "analytics_site",
            PartCall::AnalyticsRead { .. } => "analytics_read",
        }
    }

    fn part(&self) -> Part {
        match self {
            PartCall::AnalyticsSite { .. } | PartCall::AnalyticsRead { .. } => Part::Analytics,
            _ => Part::Address,
        }
    }

    /// A write changes the account, so it never shares another's run.
    fn writes(&self) -> bool {
        matches!(self, PartCall::DnsSet { .. } | PartCall::DnsDelete { .. })
    }
}

/// Why a part's code did not do what it was asked.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PartRefusal {
    /// The plugin is not installed, or declares no such part.
    NotServed,
    /// The account's key lacks the permission the part names in `needs`.
    MissingPermission { needs: Option<String> },
    /// The service no longer accepts the stored key.
    SignIn,
    /// The paste is not a key of the service's shape.
    WrongKey { shape: String },
    /// Anything else, in the plugin's or the engine's words.
    Failed { message: String },
}

impl PartRefusal {
    /// What the refusal means and what to do, for a person; `keys` is where
    /// the plugin's keys are made.
    pub fn words(&self, plugin: &str, keys: Option<&str>) -> String {
        let at = keys.map(|u| format!(" at {u}")).unwrap_or_default();
        match self {
            PartRefusal::NotServed => format!("{plugin} is not installed in this folder, or serves no such part"),
            PartRefusal::WrongKey { shape } => format!("that is not a {plugin} key: {shape}"),
            PartRefusal::SignIn => format!("{plugin} has no key it accepts: make a new one{at}"),
            PartRefusal::MissingPermission { needs: Some(needs) } => format!("the key does not have {needs}: make a new one with {needs}{at}"),
            PartRefusal::MissingPermission { needs: None } => format!("the key lacks a permission {plugin} needs: make a new one{at}"),
            PartRefusal::Failed { message } => message.clone(),
        }
    }
}

/// What a plugin's export returns to refuse, rather than throwing.
#[derive(Deserialize)]
struct Refused {
    refused: String,
    #[serde(default)]
    needs: Option<String>,
}

impl PluginManager {
    /// Run `call` in the plugin's code and return what the export returned.
    /// Reads asked with the same argument at once share one run.
    pub async fn call_part(&self, plugin_name: &str, call: &PartCall) -> Result<serde_json::Value, PartRefusal> {
        let plugin = self.find_plugin_by_name(plugin_name).ok_or(PartRefusal::NotServed)?;
        let parts = provider::parts(&plugin.manifest);
        let part = parts.iter().find(|p| p.part == call.part()).ok_or(PartRefusal::NotServed)?;
        let failed = |message: String| PartRefusal::Failed { message };
        let args = serde_json::to_string(call).map_err(|e| failed(e.to_string()))?;
        let ran = if call.writes() {
            let _queued = self.part_runs.lock().await;
            match self.run_hook(&plugin, call.verb(), &args, &self.host.state.hooks, AskedBy::Person).await {
                HookRun::NeverRan(e) => Err(e),
                HookRun::Finished(result) => result,
            }
        } else {
            let key = format!("{}::{}::{args}", plugin.manifest.name, call.verb());
            self.run_shared(&plugin, call.verb(), key, &args, AskedBy::Person, Some(&self.part_runs)).await
        };
        let returned = ran.map_err(failed)?.1.unwrap_or(serde_json::Value::Null);
        match serde_json::from_value::<Refused>(returned.clone()) {
            Ok(r) if r.refused == "missing_permission" => Err(PartRefusal::MissingPermission { needs: r.needs.or_else(|| part.needs.clone()) }),
            Ok(r) if r.refused == "sign_in" => Err(PartRefusal::SignIn),
            Ok(r) => Err(failed(r.refused)),
            Err(_) => Ok(returned),
        }
    }
}
