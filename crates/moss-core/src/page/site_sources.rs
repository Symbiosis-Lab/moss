//! Assemble page context from bytes supplied by any host, without I/O.
use std::collections::HashMap;
use super::{BundlePlugin, SiteBundle};
use super::listing_style::ListingStylePlan;
use super::plugin_source::read_manifest;

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SiteSources {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub texts: HashMap<String, String>,
    pub config_toml: Option<String>,
    pub detected_lang: Option<String>,
    #[serde(default)]
    pub plugins: Vec<PluginSource>,
    pub theme_style_css: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PluginSource {
    pub folder: String,
    pub manifest_json: String,
    pub entry_present: bool,
    pub admission: Admission,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Admission {
    Admitted,
    Refused { sentence: String },
}

/// Elect over paths alone when a host needs to know which text to read first.
pub fn home_file(files: &[String]) -> Option<String> {
    elect_home(files, &HashMap::new())
}

/// Paths whose text site assembly reads, in election order. Hosts may omit all
/// other texts; configuration, plugin manifests and theme CSS are separate inputs.
pub fn site_text_paths(files: &[String]) -> Vec<&str> {
    let mut root: Vec<&str> = files.iter().map(String::as_str)
        .filter(|p| !p.contains('/') && p.to_lowercase().ends_with(".md")).collect();
    root.sort();
    root
}

fn elect_home(files: &[String], texts: &HashMap<String, String>) -> Option<String> {
    let root = site_text_paths(files);
    let marked: Vec<&str> = root.iter().copied().filter(|path| {
        texts.get(*path).is_some_and(|text| {
            crate::frontmatter::frontmatter_map(text).get("home").and_then(|v| v.as_bool()) == Some(true)
        })
    }).collect();
    crate::home::detect_home_file_in_folder_marked(&root, "", &marked).map(str::to_string)
}

/// A valid language declared in frontmatter, retaining its case for callers
/// that need the authored tag. The site ladder normalizes its final value.
pub fn declared_lang_in_text(text: &str) -> Option<String> {
    let fields = crate::frontmatter::frontmatter_map(text);
    let lang = fields.get("lang")?.as_str()?.trim();
    crate::home::is_known_language_code(lang).then(|| lang.to_string())
}

/// Explicit configuration, home frontmatter, detected prose, then English; the
/// first known code wins, and a later rung is read only when every earlier one fails.
/// Lowercased because consumers compare the code as a string, though validation folds case.
pub fn resolve_site_default_lang(explicit: Option<&str>, home: impl FnOnce() -> Option<String>,
    detected: impl FnOnce() -> Option<String>) -> String {
    let known = |code: &str| {
        let code = code.trim();
        crate::home::is_known_language_code(code).then(|| code.to_lowercase())
    };
    explicit.and_then(known)
        .or_else(|| home().as_deref().and_then(known))
        .or_else(|| detected().as_deref().and_then(known))
        .unwrap_or_else(|| "en".to_string())
}

pub fn site_bundle(mut sources: SiteSources) -> (SiteBundle, Vec<String>) {
    let (config, mut warnings) = super::site_config::parse_config(sources.config_toml.as_deref().unwrap_or(""));
    let lang = resolve_site_default_lang(config.site_lang(),
        || elect_home(&sources.files, &sources.texts).and_then(|p| declared_lang_in_text(sources.texts.get(&p)?)),
        || sources.detected_lang.take());
    let (declared_name_lists, problems) = crate::terms::config::declared_name_list_fields(&config.terms_kinds());
    warnings.extend(problems);
    let mut manifests = Vec::new();
    // Folder order also makes warnings and equal-name ties independent of discovery order.
    sources.plugins.sort_by(|a, b| a.folder.cmp(&b.folder));
    for plugin in sources.plugins {
        if let Admission::Refused { sentence } = plugin.admission {
            warnings.push(format!("Plugin '{}' skipped: {sentence}", plugin.folder));
            continue;
        }
        let manifest = match read_manifest(&plugin.manifest_json) {
            Ok(manifest) => manifest,
            Err(error) => {
                warnings.push(format!("Plugin '{}' skipped: invalid manifest: {error}", plugin.folder));
                continue;
            }
        };
        let name = manifest.name.clone().unwrap_or(plugin.folder);
        if let Some(entry) = &manifest.entry {
            if !plugin.entry_present {
                warnings.push(format!("Plugin '{name}' skipped: entry point '{entry}' not found"));
                continue;
            }
        }
        manifests.push((name, manifest));
    }
    manifests.sort_by(|a, b| a.0.cmp(&b.0));
    let styles = ListingStylePlan::from_contributions(manifests.iter().filter_map(|(name, manifest)| {
        Some((name.as_str(), manifest.contributes.listing_styles.as_ref()?))
    }));
    warnings.extend(styles.conflicts.iter().cloned());
    let plugins: Vec<_> = manifests.into_iter().map(|(name, manifest)| BundlePlugin {
        name, fields: manifest.contributes.frontmatter.map(|f| f.fields).unwrap_or_default(),
    }).collect();
    let listing_styles = styles.ids().map(str::to_string).collect();
    sources.files.sort();
    let bundle = SiteBundle {
        files: sources.files, lang, declared_name_lists, plugins, listing_styles,
        theme_css: sources.theme_style_css.unwrap_or_default(), ..Default::default()
    };
    let schemas: Vec<_> = bundle.plugins.iter().map(|p| crate::schema::PluginSchema {
        name: &p.name, fields: Some(&p.fields),
    }).collect();
    let (_, mut conflicts) = crate::schema::compose_schema(&bundle.declared_name_lists, &schemas, &bundle.listing_styles);
    conflicts.sort();
    warnings.extend(conflicts);
    (bundle, warnings)
}

#[cfg(test)]
#[path = "site_sources_tests.rs"]
mod tests;
