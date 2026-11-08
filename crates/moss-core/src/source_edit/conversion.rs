//! Frontmatter helpers — YAML read/write and JSON↔YAML conversion.
//!
//! These helpers are called by the `parse_frontmatter` and `save_editor_content`
//! Tauri commands in `commands.rs`.

use std::collections::HashMap;

/// Convert a `HashMap<String, serde_yaml::Value>` to `serde_json::Value`.
///
/// Serialises to a YAML string then re-parses as JSON-compatible to handle all
/// YAML types (arrays, nested objects, etc.) correctly.
pub fn yaml_map_to_json(
    map: &HashMap<String, serde_yaml::Value>,
) -> Result<serde_json::Value, String> {
    let yaml_str =
        serde_yaml::to_string(map).map_err(|e| format!("YAML serialize error: {}", e))?;
    let json_value: serde_json::Value =
        serde_yaml::from_str(&yaml_str).map_err(|e| format!("YAML-to-JSON error: {}", e))?;
    Ok(json_value)
}

/// Convert a `serde_json::Value` (from the frontend) to a
/// `HashMap<String, serde_yaml::Value>`.
///
/// Inverse of [`yaml_map_to_json`]. Goes through a JSON string → YAML parse to
/// correctly translate all types (strings, numbers, booleans, arrays, objects).
pub fn json_to_yaml_map(
    json: &serde_json::Value,
) -> Result<HashMap<String, serde_yaml::Value>, String> {
    let json_str =
        serde_json::to_string(json).map_err(|e| format!("JSON serialize error: {}", e))?;
    let map: HashMap<String, serde_yaml::Value> =
        serde_yaml::from_str(&json_str).map_err(|e| format!("JSON-to-YAML error: {}", e))?;
    Ok(map)
}
