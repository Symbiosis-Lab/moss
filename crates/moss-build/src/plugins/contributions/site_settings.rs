//! `contributes.site_settings`: settings moss draws in a plugin's tab and
//! writes into the site's own `[services.<plugin-id>]` table, where an agent or
//! a text editor reads and changes them like any other site setting.
//!
//! The table is fixed by the plugin's id, the same table `public_data` may
//! publish from, so a manifest can never point a write anywhere else.

use serde::{Deserialize, Serialize};

use super::settings::{fill_site_defaults, validate_site_fields, Field, FieldType};

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct SiteSettingsContribution {
    /// The fields, in display order, in the vocabulary of `setup.settings`
    /// plus `object`, `map` and `array`.
    #[serde(default)]
    pub settings: Vec<Field>,

    /// Buttons that each make one call to a moss service on the site's behalf.
    #[serde(default)]
    pub actions: Vec<SiteAction>,
}

/// One button: a call to a service moss knows the address of, signed with the
/// folder's identity. The answer's `url`, when it is `https`, is opened.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct SiteAction {
    pub id: String,
    pub label: String,
    /// The heading the button is drawn under.
    #[serde(default)]
    pub group: Option<String>,
    /// A name in moss's own table of services, never an address.
    pub service: String,
    /// `POST` or `GET`.
    pub method: String,
    /// The path on that service; `{site}` is the folder's site id.
    pub path: String,
}

impl SiteSettingsContribution {
    pub(crate) fn validate(&mut self) -> Result<(), String> {
        validate_site_fields(&self.settings)?;
        fill_site_defaults(&mut self.settings);
        for (index, action) in self.actions.iter().enumerate() {
            let id = &action.id;
            let id_ok = !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            if !id_ok {
                return Err(format!("site_settings action `{id}`: an id is lowercase letters, digits and hyphens"));
            }
            if self.actions[..index].iter().any(|earlier| earlier.id == *id) {
                return Err(format!("site_settings action `{id}` is declared twice"));
            }
            if !matches!(action.method.as_str(), "POST" | "GET") {
                return Err(format!("site_settings action `{id}`: `method` is POST or GET"));
            }
            if !service_path_ok(&action.path) {
                return Err(format!("site_settings action `{id}`: `path` starts with one / and may hold only `{{site}}` as a placeholder"));
            }
        }
        use moss_core::untrusted_text::{bounded, MAX_NAME};
        for action in &mut self.actions {
            action.label = bounded(&action.label, MAX_NAME);
            if let Some(group) = &mut action.group {
                *group = bounded(group, MAX_NAME);
            }
        }
        Ok(())
    }

    pub fn action(&self, id: &str) -> Option<&SiteAction> {
        self.actions.iter().find(|a| a.id == id)
    }

    /// Is writing `value` at `path` (relative to the plugin's table) a change
    /// the manifest draws: every step, and every key inside the value,
    /// declared and of its declared kind. A removal (`None`) needs only the
    /// path. `vault::config::save_plugin_settings` runs it on every change.
    pub fn check_change(&self, path: &[String], value: Option<&serde_json::Value>) -> Result<(), String> {
        let field = self.target(path)?;
        match value {
            Some(value) => shape_ok(field, value).map_err(|why| format!("`{}`: {why}", path.join("."))),
            None => Ok(()),
        }
    }

    /// The field, or the map's item, that `path` ends at.
    fn target(&self, path: &[String]) -> Result<&Field, String> {
        let refuse = || format!("`{}` is not a setting this plugin draws", path.join("."));
        let mut fields: &[Field] = &self.settings;
        let mut rest = path;
        loop {
            let (first, tail) = rest.split_first().ok_or_else(refuse)?;
            let field = fields.iter().find(|f| &f.key == first).ok_or_else(refuse)?;
            match field.field_type {
                FieldType::Object if !tail.is_empty() => {
                    fields = &field.fields;
                    rest = tail;
                }
                FieldType::Map if !tail.is_empty() => {
                    let (id, after) = tail.split_first().ok_or_else(refuse)?;
                    map_id_ok(field, id)?;
                    let item = field.items.as_deref().ok_or_else(refuse)?;
                    if after.is_empty() {
                        return Ok(item);
                    }
                    if item.field_type != FieldType::Object {
                        return Err(refuse());
                    }
                    fields = &item.fields;
                    rest = after;
                }
                _ if tail.is_empty() => return Ok(field),
                _ => return Err(refuse()),
            }
        }
    }
}

fn map_id_ok(map: &Field, id: &str) -> Result<(), String> {
    let pattern = map.key_pattern.as_deref().unwrap_or(super::settings::DEFAULT_KEY_PATTERN);
    match regex::Regex::new(pattern).is_ok_and(|re| re.is_match(id)) {
        true => Ok(()),
        false => Err(format!("`{id}` is not an id `{}` accepts", map.key)),
    }
}

/// Is `value` of the shape `field` declares, at every depth.
fn shape_ok(field: &Field, value: &serde_json::Value) -> Result<(), String> {
    use serde_json::Value;
    match (field.field_type, value) {
        (FieldType::String, Value::String(_)) | (FieldType::Number, Value::Number(_)) | (FieldType::Boolean, Value::Bool(_)) => Ok(()),
        (FieldType::Object, Value::Object(members)) => members.iter().try_for_each(|(key, member)| {
            let declared = field.fields.iter().find(|f| f.key == *key).ok_or_else(|| format!("`{key}` is not a setting this plugin draws"))?;
            shape_ok(declared, member).map_err(|why| format!("{key}: {why}"))
        }),
        (FieldType::Map, Value::Object(entries)) => entries.iter().try_for_each(|(id, entry)| {
            map_id_ok(field, id)?;
            shape_ok(field.items.as_deref().ok_or("a map has no items")?, entry).map_err(|why| format!("{id}: {why}"))
        }),
        (FieldType::Array, Value::Array(values)) => {
            values.iter().try_for_each(|v| shape_ok(field.items.as_deref().ok_or("an array has no items")?, v))
        }
        (kind, _) => Err(format!("a `{}` is expected", format!("{kind:?}").to_lowercase())),
    }
}

/// `path` with the site id in place of `{site}`, or `None` for a folder with
/// no site id, or one whose id could reshape the path.
pub fn service_path(path: &str, site_id: Option<&str>) -> Option<String> {
    let id = site_id?;
    let safe = !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    safe.then(|| path.replace("{site}", id))
}

/// A path on one of moss's services: no host, no query, no other placeholder,
/// so the manifest chooses only which of the service's routes is called.
pub(crate) fn service_path_ok(path: &str) -> bool {
    let rest = path.replace("{site}", "");
    path.starts_with('/')
        && !path.starts_with("//")
        && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
        && !rest.contains("..")
}

#[cfg(test)]
#[path = "site_settings_tests.rs"]
mod tests;
