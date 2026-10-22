//! The read-only copy of `.moss/config.toml` a `process` hook receives as
//! `site_config`.
//!
//! A plugin may read the site's own settings (for example the tables a map
//! plugin declares) but never a credential, so two things are withheld: the
//! top-level `[plugins]` table, which holds other plugins' settings (a plugin's
//! own already arrives as `config`), and any key anywhere whose name marks it a
//! secret, such as `[services.email].api_key`.

use super::ConfigFile;
use serde_json::{Map, Value as Json};
use toml::Value;

const WITHHELD_TABLES: &[&str] = &["plugins"];
const SECRET_MARKERS: &[&str] = &[
    "token", "secret", "password", "passwd", "api_key", "apikey", "credential", "private_key",
];

fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase().replace('-', "_");
    SECRET_MARKERS.iter().any(|m| k.contains(m))
}

fn to_json(value: &Value) -> Json {
    match value {
        Value::String(s) => Json::String(s.clone()),
        Value::Integer(i) => Json::from(*i),
        Value::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
        Value::Boolean(b) => Json::Bool(*b),
        Value::Datetime(d) => Json::String(d.to_string()),
        Value::Array(a) => Json::Array(a.iter().map(to_json).collect()),
        Value::Table(t) => Json::Object(
            t.iter()
                .filter(|(k, _)| !is_secret_key(k))
                .map(|(k, v)| (k.clone(), to_json(v)))
                .collect(),
        ),
    }
}

impl ConfigFile {
    /// The config as a JSON object with secrets and other plugins' settings
    /// removed. An empty config is `{}`.
    pub fn plugin_view(&self) -> Map<String, Json> {
        self.root
            .iter()
            .filter(|(k, _)| !WITHHELD_TABLES.contains(&k.as_str()) && !is_secret_key(k))
            .map(|(k, v)| (k.clone(), to_json(v)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_site_tables_and_withholds_secrets_and_plugin_settings() {
        let cfg = ConfigFile::parse(
            r#"
[site]
title = "A"
[map.layers.sheet]
source = "https://example.org/iiif"
zoom = 3
[services.email]
provider = "buttondown"
api_key = "sk-live"
[plugins.other]
access_token = "t"
[nested.deep]
Client-Secret = "x"
keep = true
"#,
        )
        .unwrap();
        let v = Json::Object(cfg.plugin_view());
        assert_eq!(v["site"]["title"], "A");
        assert_eq!(v["map"]["layers"]["sheet"]["zoom"], 3);
        assert_eq!(v["services"]["email"]["provider"], "buttondown");
        assert!(v["services"]["email"].get("api_key").is_none());
        assert!(v.get("plugins").is_none());
        assert!(v["nested"]["deep"].get("Client-Secret").is_none());
        assert_eq!(v["nested"]["deep"]["keep"], true);
        assert!(!v.to_string().contains("sk-live"));
    }

    #[test]
    fn empty_config_is_an_empty_object() {
        assert!(ConfigFile::empty().plugin_view().is_empty());
    }
}
