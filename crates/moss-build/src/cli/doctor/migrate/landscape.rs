//! A place-typed kind that draws moss's compiled-in map is given
//! `landscape = "detail"` and the same pack copied into `.moss/maps/`, so the
//! site draws the same map once moss no longer carries the pack itself.

/// The config text with `landscape = "detail"` on each place-typed kind that
/// has no `landscape`, or `None` when none needs it.
pub fn plan(config_text: &str) -> Result<Option<String>, String> {
    let mut table: toml::Table = toml::from_str(config_text).map_err(|e| format!("could not read .moss/config.toml: {e}"))?;
    let mut changed = false;
    for kind in table.get_mut("terms").and_then(toml::Value::as_table_mut).into_iter().flat_map(|terms| terms.iter_mut().map(|(_, kind)| kind)) {
        let Some(kind) = kind.as_table_mut() else { continue };
        if kind.get("type").and_then(toml::Value::as_str) == Some("place") && !kind.contains_key("landscape") {
            kind.insert("landscape".to_string(), toml::Value::String(crate::build::place_map::DETAIL_LANDSCAPE.to_string()));
            changed = true;
        }
    }
    if !changed {
        return Ok(None);
    }
    crate::infra::toml_rewrite::apply_changes(config_text, &table).map(Some)
}

/// Whether the folder, with `config_text` as its config, asks for the pack
/// and lacks a usable one.
pub fn needs_pack(folder: &std::path::Path, config_text: &str) -> bool {
    use crate::build::place_map::{find_landscape, wanted_landscape, Landscape};
    let Ok(cfg) = crate::config::ConfigFile::parse(config_text) else { return false };
    let maps = crate::moss_paths::MossPaths::new(folder).maps_dir();
    matches!(find_landscape(&maps, wanted_landscape(&cfg).as_deref()), Landscape::Missing(_))
}
