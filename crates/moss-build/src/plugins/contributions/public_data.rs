//! `contributes.public_data`: the part of a site's settings a plugin's element
//! reads in the visitor's browser, published as `_moss/extensions/<plugin>.json`.
//!
//! What is published is a projection of the plugin's own table through the
//! declared paths, never a copied subtree: a key added to the table later, at
//! any depth, is dropped with a warning instead of published.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::config::ConfigFile;
use crate::plugins::types::Plugin;

/// The version of the `{schema, data}` envelope. What is inside `data` is the
/// plugin's own.
const ENVELOPE_SCHEMA: u32 = 1;

/// Which settings a plugin publishes from its own table, `[services.<plugin-id>]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicDataContribution {
    /// Dotted paths to the values that may be published, `*` standing for any
    /// one key (the ids of a map): `currency`, `cancel.late_percent`,
    /// `services.*.name`. A path ends at a value that is not a table: a
    /// string, number, boolean, date or list of those.
    pub keys: Vec<String>,
    /// Keys the build supplies when the table leaves them out: the address of
    /// one of moss's services for this site. Derived on every build and never
    /// written, so it follows the folder's environment; a written value wins.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub fill: std::collections::BTreeMap<String, Fill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    /// A name in moss's own table of services, never an address.
    pub service: String,
    /// The path on it; `{site}` is the folder's site id.
    pub path: String,
}

/// What a fill needs from the folder: where it publishes and under which id.
#[derive(Debug, Clone, Default)]
pub struct FillContext {
    pub environment: crate::config::environment::HostingEnvironment,
    pub site_id: Option<String>,
}

impl FillContext {
    /// The folder's environment and the site id its last publish recorded.
    pub fn for_folder(folder_path: &str) -> Self {
        Self {
            environment: crate::build::site_config::resolve_environment(folder_path),
            site_id: crate::build::site_config::read_deployment_state(folder_path).ok().and_then(|s| s.site_id),
        }
    }

    /// `<service address><path>` for this folder, `{site}` filled in; `None`
    /// where the service does not run or the folder has no site id yet.
    pub fn address(&self, service: &str, path: &str) -> Option<String> {
        let base = self.environment.service_url(service)?;
        Some(format!("{base}{}", super::site_settings::service_path(path, self.site_id.as_deref())?))
    }
}

/// Refuse a declaration that could never publish anything, or could publish
/// a whole table, when the manifest is read.
pub fn validate(plugin: &str, data: &PublicDataContribution) -> Result<(), String> {
    moss_core::page::block_names::validate_plugin_id(plugin)?;
    if data.keys.is_empty() {
        return Err("public_data: `keys` must name at least one key".to_string());
    }
    for key in &data.keys {
        let steps: Vec<&str> = key.split('.').collect();
        let step_ok = |s: &&str| *s == "*" || (!s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));
        if !steps.iter().all(step_ok) || steps[0] == "*" {
            return Err(format!("public_data: `{key}` is not a path of keys below the plugin's table, such as `services.*.name`"));
        }
        if let Some(step) = steps.iter().find(|s| crate::config::plugin_view::is_secret_key(s)) {
            return Err(format!("public_data: `{step}` in `{key}` looks like a secret and cannot be published"));
        }
        if let Some(outer) = data.keys.iter().find(|other| *other != key && key.starts_with(&format!("{other}."))) {
            return Err(format!("public_data: `{key}` is inside `{outer}`; a path ends at a value, so list only the values inside it"));
        }
    }
    if let Some(at) = Shape::of(&data.keys).mixed_wildcard("") {
        return Err(format!("public_data: `{at}` lists both `*` and a named key; `*` already stands for every key there"));
    }
    if let Some(twice) = data.keys.iter().enumerate().find_map(|(i, k)| data.keys[..i].contains(k).then_some(k)) {
        return Err(format!("public_data: `{twice}` is listed twice"));
    }
    for (key, fill) in &data.fill {
        if !data.keys.contains(key) || key.contains('.') {
            return Err(format!("public_data: `fill.{key}` names a key that is not a top-level path in `keys`"));
        }
        if !super::site_settings::service_path_ok(&fill.path) {
            return Err(format!("public_data: `fill.{key}.path` starts with one / and may hold only `{{site}}` as a placeholder"));
        }
    }
    Ok(())
}

/// The declared paths as a tree: a step either ends a path or leads on.
#[derive(Default)]
struct Shape<'a> {
    end: bool,
    steps: std::collections::BTreeMap<&'a str, Shape<'a>>,
}

impl<'a> Shape<'a> {
    fn of(keys: &'a [String]) -> Self {
        let mut root = Shape::default();
        for key in keys {
            key.split('.').fold(&mut root, |shape, step| shape.steps.entry(step).or_default()).end = true;
        }
        root
    }

    /// The first step under which `*` sits beside a named key, which would
    /// hide the wildcard from that key. Never the top level, where `*` is refused.
    fn mixed_wildcard(&self, at: &str) -> Option<String> {
        if self.steps.len() > 1 && self.steps.contains_key("*") {
            return Some(at.to_string());
        }
        self.steps.iter().find_map(|(step, shape)| shape.mixed_wildcard(&if at.is_empty() { step.to_string() } else { format!("{at}.{step}") }))
    }

    /// `value` with only what this shape declares, or `None` when nothing of
    /// it may be published; `dropped` names each left-out path.
    fn project(&self, value: &toml::Value, at: &str, dropped: &mut Vec<String>) -> Option<serde_json::Value> {
        use toml::Value;
        let table = match value {
            Value::Table(_) if self.end => None,
            Value::Array(items) if self.end && items.iter().any(|v| matches!(v, Value::Table(_) | Value::Array(_))) => None,
            _ if self.end => return Some(crate::config::plugin_view::to_json(value)),
            Value::Table(table) => Some(table),
            _ => None,
        };
        let Some(table) = table else {
            dropped.push(at.to_string());
            return None;
        };
        let mut out = serde_json::Map::new();
        for (key, member) in table {
            let path = if at.is_empty() { key.clone() } else { format!("{at}.{key}") };
            // A name `*` admits is still held back when it looks like a secret,
            // the rule a declared path is held to.
            let wild = || self.steps.get("*").filter(|_| !crate::config::plugin_view::is_secret_key(key));
            match self.steps.get(key.as_str()).or_else(wild) {
                Some(shape) => out.extend(shape.project(member, &path, dropped).map(|v| (key.clone(), v))),
                None => dropped.push(path),
            }
        }
        Some(serde_json::Value::Object(out))
    }
}

/// Whether the installed plugin named `id` declares `public_data`, which makes
/// `[services.<id>]` its table, so moss's own reader for that table stays off.
pub fn owns_service_table(plugins: &[Plugin], id: &str) -> bool {
    plugins.iter().any(|p| p.manifest.name == id && p.manifest.contributes.as_ref().is_some_and(|c| c.public_data.is_some()))
}

/// One file a build publishes for a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicFile {
    pub plugin: String,
    pub body: String,
}

/// What each plugin publishes from this site's settings, and a sentence for each
/// key held back. A table with nothing to publish writes no file, so a reader
/// gets a 404 rather than an empty answer it could mistake for the settings.
pub fn project(plugins: &[Plugin], config: &ConfigFile, folder: &FillContext) -> (Vec<PublicFile>, Vec<String>) {
    let mut files = Vec::new();
    let mut problems = Vec::new();
    for plugin in plugins {
        let Some(declared) = plugin.manifest.contributes.as_ref().and_then(|c| c.public_data.as_ref()) else { continue };
        let name = &plugin.manifest.name;
        let Some(table) = config.section(&["services", name]).filter(|t| t.is_table()) else { continue };
        let mut dropped = Vec::new();
        let projected = Shape::of(&declared.keys).project(table, "", &mut dropped);
        // A value the plugin's own settings draw is private by design, so only
        // a key nothing declares is worth a warning.
        let site = plugin.manifest.contributes.as_ref().and_then(|c| c.site_settings.as_ref());
        let drawn = |path: &str| site.is_some_and(|s| s.check_change(&path.split('.').map(str::to_string).collect::<Vec<_>>(), None).is_ok());
        problems.extend(dropped.into_iter().filter(|path| !drawn(path)).map(|path| {
            format!("[services.{name}] `{path}` is not something plugin `{name}` publishes, so it stays private")
        }));
        let Some(serde_json::Value::Object(mut data)) = projected else { continue };
        if data.is_empty() {
            continue;
        }
        for (key, fill) in &declared.fill {
            if !data.contains_key(key) {
                if let Some(value) = folder.address(&fill.service, &fill.path) {
                    data.insert(key.clone(), serde_json::Value::String(value));
                }
            }
        }
        let body = serde_json::json!({"schema": ENVELOPE_SCHEMA, "data": data}).to_string();
        files.push(PublicFile { plugin: name.clone(), body });
    }
    (files, problems)
}

/// Write each file at its fixed, unhashed address: an element fetches it by name.
pub fn emit(files: &[PublicFile], output_dir: &Path, pending: &mut PendingManifest) -> Result<(), String> {
    let mut ctx = BuildContext::for_render(output_dir, pending);
    for file in files {
        ctx.emit_held(&ServedPath::for_extension_data(&file.plugin), file.body.clone().into_bytes(), HashBucket::Files)
            .map_err(|e| format!("Failed to publish the settings of plugin `{}`: {e}", file.plugin))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::types::PluginManifest;
    use serde_json::json;

    fn parse(data: serde_json::Value) -> Result<PluginManifest, String> {
        PluginManifest::parse(&json!({"name":"catalog","version":"1.0.0","contributes":{"public_data":data}}).to_string())
            .map_err(|e| e.to_string())
    }

    #[test]
    fn a_malformed_declaration_is_refused_and_a_good_one_needs_no_entry() {
        assert!(parse(json!({"keys":["items","services.*.name","services.*.labels.*"]})).is_ok());
        for bad in [
            json!({"keys":[]}),
            json!({"keys":[""]}),
            json!({"keys":["items..name"]}),
            json!({"keys":["*"]}),
            json!({"keys":["*.name"]}),
            json!({"keys":["items","items.name"]}),
            json!({"keys":["items","items"]}),
            json!({"keys":["items","api_token"]}),
            json!({"keys":["services.*.api_token"]}),
            json!({"keys":["services.*.name","services.walk.price"]}),
            json!({"keys":["services.*","services.walk"]}),
        ] {
            assert!(parse(bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_missing_service_address_is_derived_for_the_environment_and_a_written_one_wins() {
        use crate::config::environment::HostingEnvironment;
        let manifest = parse(json!({"keys":["endpoint","items"],
            "fill":{"endpoint":{"service":"commerce","path":"/sites/{site}/catalog"}}})).unwrap();
        let plugin = Plugin { manifest, path: "catalog".into() };
        let unwritten = ConfigFile::parse("schema_version = 6\n[services.catalog]\nitems = [1]\n").unwrap();
        let endpoint = |config: &ConfigFile, environment, site_id: Option<&str>| {
            let folder = FillContext { environment, site_id: site_id.map(str::to_string) };
            let (files, _) = project(std::slice::from_ref(&plugin), config, &folder);
            let body: serde_json::Value = serde_json::from_str(&files[0].body).unwrap();
            body["data"]["endpoint"].as_str().map(str::to_string)
        };
        assert_eq!(endpoint(&unwritten, HostingEnvironment::Staging, Some("her-blog")).as_deref(), Some("https://commerce-staging.mosspub.com/sites/her-blog/catalog"));
        assert_eq!(endpoint(&unwritten, HostingEnvironment::Production, Some("her-blog")), None, "no address where the service does not run");
        assert_eq!(endpoint(&unwritten, HostingEnvironment::Staging, None), None, "a folder never published has no site id");
        let written = ConfigFile::parse("schema_version = 6\n[services.catalog]\nendpoint = 'https://example.com/own'\n").unwrap();
        assert_eq!(endpoint(&written, HostingEnvironment::Staging, Some("her-blog")).as_deref(), Some("https://example.com/own"));
        assert!(parse(json!({"keys":["items"],"fill":{"endpoint":{"service":"commerce","path":"/x"}}})).is_err(), "a fill outside keys");
        assert!(parse(json!({"keys":["shop.endpoint"],"fill":{"shop.endpoint":{"service":"commerce","path":"/x"}}})).is_err(), "a fill is a top-level key");
        assert!(parse(json!({"keys":["endpoint"],"fill":{"endpoint":{"service":"commerce","path":"https://evil.example/"}}})).is_err());
    }

    fn project_bare(plugins: &[Plugin], config: &ConfigFile) -> (Vec<PublicFile>, Vec<String>) {
        project(plugins, config, &FillContext::default())
    }

    #[test]
    fn only_declared_paths_publish_at_any_depth_and_the_rest_are_named() {
        let manifest = parse(json!({"keys":["endpoint","timezone","opens","tags","cancel.late_percent","services.*.name","services.*.labels.*"]})).unwrap();
        let plugin = Plugin { manifest, path: "catalog".into() };
        let config = ConfigFile::parse(r#"schema_version = 6
[services.catalog]
endpoint = "https://example.com/c"
timezone = { calendar = "https://cal.example/a.ics" }
opens = 2026-11-10
tags = ["a", { calendar = "https://cal.example/b.ics" }]
private_note = "unpublished"
[services.catalog.cancel]
late_percent = 50
calendar = "https://cal.example/c.ics"
[services.catalog.services.walk]
name = "Walk"
calendar = "https://cal.example/d.ics"
labels = { phone = "Phone", extra = { calendar = "https://cal.example/e.ics" }, api_token = "unpublished" }
"#).unwrap();
        let (files, mut problems) = project(std::slice::from_ref(&plugin), &config, &FillContext::default());
        let body: serde_json::Value = serde_json::from_str(&files[0].body).unwrap();
        assert_eq!(body, json!({"schema":1,"data":{
            "endpoint":"https://example.com/c","opens":"2026-11-10",
            "cancel":{"late_percent":50},
            "services":{"walk":{"name":"Walk","labels":{"phone":"Phone"}}}}}));
        assert!(!files[0].body.contains("cal.example") && !files[0].body.contains("unpublished"), "{}", files[0].body);
        problems.sort();
        let named: Vec<&str> = problems.iter().filter_map(|p| p.split('`').nth(1)).collect();
        assert_eq!(named, ["cancel.calendar", "private_note", "services.walk.calendar", "services.walk.labels.api_token", "services.walk.labels.extra", "tags", "timezone"], "{problems:?}");
        let (none, quiet) = project_bare(std::slice::from_ref(&plugin), &ConfigFile::parse("schema_version = 6\n").unwrap());
        assert!(none.is_empty() && quiet.is_empty(), "an unwritten table publishes nothing");
        let (none, _) = project_bare(std::slice::from_ref(&plugin), &ConfigFile::parse("schema_version = 6\n[services.catalog]\nprivate_note = 'x'\n").unwrap());
        assert!(none.is_empty(), "a table with no declared value writes no file");
    }

    #[test]
    fn a_drawn_value_left_unpublished_stays_private_without_a_warning() {
        let manifest = PluginManifest::parse(&json!({"name":"catalog","version":"1.0.0","contributes":{
            "public_data":{"keys":["currency"]},
            "site_settings":{"settings":[{"key":"currency","type":"string"},{"key":"notice_hours","type":"number"}]}}}).to_string()).unwrap();
        let plugin = Plugin { manifest, path: "catalog".into() };
        let config = ConfigFile::parse("schema_version = 6\n[services.catalog]\ncurrency = 'usd'\nnotice_hours = 12\ncalendar = 'https://cal.example/a.ics'\n").unwrap();
        let (files, problems) = project_bare(std::slice::from_ref(&plugin), &config);
        assert!(!files[0].body.contains("notice_hours") && !files[0].body.contains("cal.example"));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("`calendar`"), "{problems:?}");
    }
}
