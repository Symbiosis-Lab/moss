//! The config.toml keys shared by page bundles and builds. Unknown keys belong
//! to the host; malformed values must not erase their valid neighbours.
use std::collections::BTreeMap;
use serde::Deserialize;
use crate::terms::config::RawKind;

/// One deserialization shape for config.toml, independent of the typed contract.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    Bool(bool),
    Text(String),
    Integer(i64),
    Float(f64),
    /// From a host whose own TOML parser reads datetimes (basic-toml refuses
    /// them), so that no typed read mistakes one for text or a table.
    Datetime,
    Array(Vec<ConfigValue>),
    Table(BTreeMap<String, ConfigValue>),
}

impl<'de> Deserialize<'de> for ConfigValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ConfigValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a config value")
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(ConfigValue::Bool(value))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(ConfigValue::Integer(value))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                i64::try_from(value).map(ConfigValue::Integer).map_err(E::custom)
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Ok(ConfigValue::Float(value))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(ConfigValue::Text(value.to_string()))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(ConfigValue::Text(value))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element()? { values.push(value); }
                Ok(ConfigValue::Array(values))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = map.next_entry()? { values.insert(key, value); }
                Ok(ConfigValue::Table(values))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

impl ConfigValue {
    pub(crate) fn text(&self) -> Option<&String> {
        match self { Self::Text(value) => Some(value), _ => None }
    }
    pub(crate) fn boolean(&self) -> Option<bool> {
        match self { Self::Bool(value) => Some(*value), _ => None }
    }
    pub(crate) fn integer(&self) -> Option<i64> {
        match self { Self::Integer(value) => Some(*value), _ => None }
    }
    pub(crate) fn table(&self) -> Option<&BTreeMap<String, Self>> {
        match self { Self::Table(value) => Some(value), _ => None }
    }
    pub(crate) fn array(&self) -> Option<&Vec<Self>> {
        match self { Self::Array(value) => Some(value), _ => None }
    }
    /// A label as `(language tag, text)` pairs: plain text is the `en` label
    /// every language falls back to; a table gives one per tag, sorted.
    pub fn labels(&self) -> Option<Vec<(String, String)>> {
        match self {
            Self::Text(text) => Some(vec![("en".into(), text.clone())]),
            Self::Table(table) => table.iter().map(|(tag, label)| Some((tag.clone(), label.text()?.clone()))).collect(),
            _ => None,
        }
    }
}

/// A present value `convert` refuses is a warning, and reads as absent.
pub(crate) fn read<'a, T>(value: Option<&'a ConfigValue>, convert: impl FnOnce(&'a ConfigValue) -> Option<T>,
    key: &str, expected: &str, warnings: &mut Vec<String>) -> Option<T> {
    let value = value?;
    let result = convert(value);
    if result.is_none() { warnings.push(format!("{key} must be {expected}; ignoring it")); }
    result
}

/// Narrow typed config shared across hosts. Plugin contributions come from the
/// separately supplied manifests, through `plugin_source::read_manifest`; the
/// config's `[plugins]` settings (including arrays of tables) are host-owned.
#[derive(Debug, Clone, Default)]
pub struct CoreConfig {
    lang: Option<String>,
    kinds: Vec<RawKind>,
    /// The legacy `[terms] author = false` / `tags = false` switches.
    disabled: Vec<String>,
    nav: Vec<(String, Option<i32>)>,
}

impl CoreConfig {
    /// Read the typed keys of a parsed config, with one warning per value of
    /// the wrong type; a malformed value never erases its valid neighbours.
    pub fn from_value(value: &ConfigValue) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let mut config = Self::default();
        let Some(table) = value.table() else { return (config, warnings) };
        let site = read(table.get("site"), ConfigValue::table, "[site]", "a table", &mut warnings);
        config.lang = site.and_then(|site| read(site.get("lang"), |v| v.text().cloned(),
            "[site] lang", "text", &mut warnings));
        let terms = read(table.get("terms"), ConfigValue::table, "[terms]", "a table", &mut warnings);
        for (key, value) in terms.into_iter().flatten() {
            match value {
                ConfigValue::Table(kind) => {
                    // A nav entry needs no `fields`: `[terms.authors] nav = true` lists the built-in.
                    let nav = read(kind.get("nav"), ConfigValue::boolean,
                        &format!("[terms.{key}] nav"), "true or false", &mut warnings);
                    let weight = read(kind.get("weight"), ConfigValue::integer,
                        &format!("[terms.{key}] weight"), "an integer", &mut warnings);
                    if nav == Some(true) {
                        config.nav.push((key.clone(), weight.and_then(|w| i32::try_from(w).ok())));
                    }
                    config.kinds.extend(RawKind::read(key, kind, &mut warnings));
                }
                ConfigValue::Bool(false) => config.disabled.push(key.clone()),
                _ => {}
            }
        }
        (config, warnings)
    }

    pub fn site_lang(&self) -> Option<&str> {
        self.lang.as_deref()
    }

    pub fn terms_kinds(&self) -> Vec<RawKind> {
        crate::terms::config::with_builtins(&self.kinds, &self.disabled)
    }

    /// The kinds with `nav = true` and their `weight`, by key; hosts apply
    /// their recorded written order.
    pub fn terms_nav(&self) -> Vec<(String, Option<i32>)> {
        self.nav.clone()
    }
}

/// Parse config text for a host with no TOML parser of its own; a host with one
/// hands [`CoreConfig::from_value`] its parsed table. A syntax unsupported by
/// basic-toml produces a named warning rather than quietly reading different defaults.
pub fn parse_config(text: &str) -> (CoreConfig, Vec<String>) {
    match basic_toml::from_str::<ConfigValue>(text) {
        Ok(value) => CoreConfig::from_value(&value),
        Err(error) => (CoreConfig::default(), vec![format!("Failed to parse config.toml with basic-toml: {error}")]),
    }
}

#[cfg(test)]
#[path = "site_config_tests.rs"]
mod tests;
