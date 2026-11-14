//! Public address facts and allowlisted configuration; never export settings.
use std::{collections::HashMap, path::Path};
use serde_json::Value;
use super::{bundle::Bundle, text};
use crate::vault::import::scrape::writer::OutputPaths;

/// The bundle's redirects and author routes, as `(from, to)` rows on the old site.
pub(super) fn rows(bundle: &mut Bundle, users: &[Value], report: &mut Vec<String>) -> Result<Vec<(String, String)>, String> {
    let mut rows = Vec::new();
    if let Some(bytes) = bundle.read("redirects.yaml")? {
        match serde_yaml::from_slice::<Value>(&bytes) {
            Ok(Value::Object(groups)) => {
                for (status, values) in groups {
                    if !matches!(status.as_str(), "301" | "302") { report.push(format!("Redirect group {status} is unsupported.")); continue; }
                    if let Some(map) = values.as_object() { rows.extend(map.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))); }
                }
            }
            Ok(Value::Array(values)) => rows.extend(values.iter().filter_map(|v| Some((text(v, "from")?.into(), text(v, "to")?.into())))),
            _ => report.push("redirects.yaml could not be read; no redirects from that file were imported.".into()),
        }
    }
    for user in users {
        if let (Some(slug), Some(name)) = (text(user, "slug"), text(user, "name")) {
            rows.push((format!("/author/{slug}/"), format!("/authors/{}/", moss_core::terms::term_slug(name))));
        }
    }
    if bundle.read("routes.yaml")?.is_some() { report.push("Custom routes.yaml templates and collections need manual review; imported pages use their slugs.".into()); }
    Ok(rows)
}

/// Keep each written post's feed id, so readers do not see it again as new.
pub(super) fn ledger(folder: &Path, paths: &OutputPaths, ids: &HashMap<String, (String, String)>) -> Result<(), String> {
    let path = folder.join(crate::build::feeds::import_addresses::PATH);
    let mut ledger = crate::build::feeds::import_addresses::read(&path)?;
    for (origin, (uid, feed_id)) in ids {
        if paths.get(origin).is_some() {
            ledger.pages.insert(uid.clone(), feed_id.clone());
        }
    }
    let json = serde_json::to_string_pretty(&ledger).map_err(|e| e.to_string())?;
    super::super::page::write_note(folder, crate::build::feeds::import_addresses::PATH, &json)
}
