//! Public address facts and allowlisted configuration; never export settings.
use std::{collections::HashMap, path::Path};
use serde_json::Value;
use crate::{build::feeds::redirect_table::{Addr, Target}, vault::config::{load_managed_toml, subtable, write_managed_toml}};
use super::{bundle::Bundle, text};
use crate::vault::import::scrape::writer::OutputPaths;

pub(super) fn write(bundle: &mut Bundle, folder: &Path, site: Option<&url::Url>, paths: &OutputPaths, users: &[Value], ids: &HashMap<String, (String, String)>, report: &mut Vec<String>) -> Result<(), String> {
    let config_path = folder.join(".moss/config.toml");
    let mut config = load_managed_toml(&config_path)?;
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
    let redirects = subtable(&mut config.root, "redirects")?;
    for (from, to) in rows {
        let Some(site) = site else { report.push(format!("Redirect {from} skipped: the source host is unknown.")); continue };
        let from = match own_path(&from, site) {
            Some(from) if !from.contains(['*', '[', ']', '(', ')', '^', '$', '|']) && Addr::parse(&from).is_ok() => from,
            _ => { report.push(format!("Redirect {from} skipped: source must be an exact path on the original host, without queries or fragments.")); continue; }
        };
        let to = own_path(&to, site).unwrap_or(to);
        let to = if let Ok(mut address) = site.join(&to) {
            let suffix = format!("{}{}", address.query().map(|q| format!("?{q}")).unwrap_or_default(), address.fragment().map(|f| format!("#{f}")).unwrap_or_default());
            address.set_query(None); address.set_fragment(None);
            paths.get(address.as_str()).map(|p| format!("{}{suffix}", served(p))).unwrap_or(to)
        } else { to };
        if Target::parse(&to).is_err() { report.push(format!("Redirect {from} skipped: unsupported target.")); continue; }
        if from == to { continue; }
        if redirects.contains_key(&from) { report.push(format!("Existing redirect {from} was preserved.")); continue; }
        redirects.insert(from, toml::Value::String(to));
    }
    std::fs::create_dir_all(folder.join(".moss")).map_err(|e| e.to_string())?;
    write_managed_toml(&config_path, &config.original, &config.root, crate::config::migrations::CURRENT_VERSION)?;
    let path = folder.join(crate::build::feeds::import_addresses::PATH);
    let mut ledger = crate::build::feeds::import_addresses::read(&path)?;
    for (origin, (uid, feed_id)) in ids {
        if paths.get(origin).is_some() {
            ledger.pages.insert(uid.clone(), feed_id.clone());
        }
    }
    let json = serde_json::to_string_pretty(&ledger).map_err(|e| e.to_string())?;
    super::super::page::write_note(folder, crate::build::feeds::import_addresses::PATH, &json)?;
    if bundle.read("routes.yaml")?.is_some() { report.push("Custom routes.yaml templates and collections need manual review; imported pages use their slugs.".into()); }
    Ok(())
}
fn own_path(value: &str, site: &url::Url) -> Option<String> {
    if value.starts_with('/') && !value.starts_with("//") { return Some(value.into()); }
    let url = url::Url::parse(value).ok()?;
    (url.origin() == site.origin() && url.query().is_none() && url.fragment().is_none()).then(|| url.path().into())
}
fn served(path: &str) -> String {
    let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("index");
    let output = moss_core::frontmatter_typed::compute_url_path(path, stem == "index", None, stem);
    format!("/{}", crate::build::scan::article_map::to_pretty_url(&output))
}
