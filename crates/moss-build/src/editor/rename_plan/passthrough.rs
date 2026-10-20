//! `[build].passthrough` in `.moss/config.toml` names files and folders by
//! path, so a rename has to carry those entries along or the renamed folder
//! silently stops being passthrough.

use std::path::Path;

use toml_edit::{Document, Item, Value};

/// Rewrite every `[build].passthrough` entry equal to a moved path, or under
/// it, to follow the move. `moves` are root-relative `(from, to)` pairs.
/// A leading `!` and the entry's own leading or trailing `/` are kept, and
/// only the changed strings are touched, so comments and layout survive.
/// Returns each `(old, new)` entry written.
pub(super) fn follow_moves(project_root: &Path, moves: &[(String, String)]) -> Result<Vec<(String, String)>, String> {
    let path = crate::moss_paths::MossPaths::new(project_root).config();
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(Vec::new()) };
    let mut doc: Document = text.parse().map_err(|e| format!("{} is malformed and was left untouched: {e}", path.display()))?;
    let Some(entries) = doc
        .get_mut("build")
        .and_then(Item::as_table_like_mut)
        .and_then(|b| b.get_mut("passthrough"))
        .and_then(Item::as_array_mut)
    else {
        return Ok(Vec::new());
    };

    let mut changed = Vec::new();
    for entry in entries.iter_mut() {
        let Some(old) = entry.as_str() else { continue };
        let Some(new) = moves.iter().find_map(|(from, to)| followed(old, from, to)) else { continue };
        let mut value = Value::from(new.as_str());
        *value.decor_mut() = entry.decor().clone();
        changed.push((old.to_string(), new));
        *entry = value;
    }
    if changed.is_empty() {
        return Ok(changed);
    }
    if let Ok(root) = toml::from_str::<toml::Table>(&text) {
        if let Some(found) = crate::config::migrations::version_ahead(&root, crate::config::migrations::CURRENT_VERSION) {
            return Err(format!("{} is at schema_version {found}, newer than this build of moss supports; left untouched", path.display()));
        }
    }
    crate::infra::atomic_write::write_atomic(&path, &doc.to_string())?;
    Ok(changed)
}

/// `entry` after `from` moved to `to`, or `None` when the move does not touch it.
fn followed(entry: &str, from: &str, to: &str) -> Option<String> {
    let (negation, raw) = entry.strip_prefix('!').map_or(("", entry), |rest| ("!", rest));
    let path = raw.trim_matches('/');
    let rest = path.strip_prefix(from.trim_matches('/'))?;
    if !rest.is_empty() && !rest.starts_with('/') {
        return None;
    }
    let lead = if raw.starts_with('/') { "/" } else { "" };
    let trail = if raw.len() > 1 && raw.ends_with('/') { "/" } else { "" };
    Some(format!("{negation}{lead}{}{rest}{trail}", to.trim_matches('/')))
}
