//! Decide a save from bytes, without opening or writing a file.

use super::ParsedFrontmatter;
use crate::page::content_hash;

/// A save plan. Hosts write `bytes` only for `Write`; conflicts carry the
/// same parsed document as the engine command. Bytes serialize as JSON u8 arrays.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum SaveDecision {
    Write { bytes: Vec<u8> },
    AlreadySaved,
    Conflict { disk: ParsedFrontmatter },
    Gone,
}

/// Preserve frontmatter formatting, then compare the proposed bytes before
/// checking the expected hash. Missing disk bytes with an expected hash mean
/// the source is gone; without an expected hash they allow creating a new source.
/// An absent expected hash opts out of the conflict check.
pub fn decide_save(
    on_disk: Option<&[u8]>,
    expected_hash: Option<&str>,
    frontmatter: &serde_json::Value,
    body: &str,
) -> Result<SaveDecision, String> {
    if on_disk.is_none() && expected_hash.is_some() {
        return Ok(SaveDecision::Gone);
    }
    let disk = on_disk.map(std::str::from_utf8).transpose().map_err(|e| e.to_string())?;
    let mut fields = super::conversion::json_to_yaml_map(frontmatter)?;
    coerce_union_fields(&mut fields);
    let content = match disk.and_then(|text| edit_in_place(text, &fields, body)) {
        Some(text) => text,
        None => crate::frontmatter::serialize(&fields, body)?,
    };
    if on_disk == Some(content.as_bytes()) {
        return Ok(SaveDecision::AlreadySaved);
    }
    if let (Some(bytes), Some(expected), Some(text)) = (on_disk, expected_hash, disk) {
        if content_hash(bytes) != expected {
            return Ok(SaveDecision::Conflict { disk: super::parse_frontmatter_of(text.to_string())? });
        }
    }
    Ok(SaveDecision::Write { bytes: content.into_bytes() })
}

/// The saved file as the one on disk with only the changed fields' lines
/// rewritten, so a save keeps the author's key order, quoting and comments.
/// `None` sends the save to the full re-serialize: the frontmatter on disk is
/// not valid YAML (the editor then holds the raw block in `body`), the file
/// uses CRLF (the editor's body is LF), every field was removed, or the line
/// editor refuses the change.
pub fn edit_in_place(on_disk: &str, fm_map: &std::collections::HashMap<String, serde_yaml::Value>, body: &str) -> Option<String> {
    if fm_map.is_empty() || on_disk.contains("\r\n") || crate::frontmatter::parse(on_disk).frontmatter_error.is_some() {
        return None;
    }
    let mut out = crate::frontmatter::set_fields(on_disk, fm_map).ok()?;
    out.truncate(crate::frontmatter::frontmatter_span(&out)?.body);
    out.push_str(body);
    Some(out)
}

/// Guard the union frontmatter fields (`children`, `series`) before writing.
///
/// The editor saves the AUTHORED single-key form — a valid `bool`, wikilink
/// `string`, or wikilink `sequence` is written through unchanged (preserving
/// `children: "[[News]]"` byte-for-byte). Only a NON-canonical shape (e.g. a map
/// or number that the widget should never produce, but a malformed paste might)
/// is coerced to the canonical authored form via the shared normalizer, so a
/// junk value can never reach the file. v1 hard-codes the two union field names;
/// other fields pass through unchanged.
pub fn coerce_union_fields(fm_map: &mut std::collections::HashMap<String, serde_yaml::Value>) {
    use serde_yaml::Value;

    if let Some(v) = fm_map.get("children") {
        let canonical = matches!(v, Value::Bool(_) | Value::String(_));
        if !canonical {
            let norm = crate::frontmatter_union::normalize_children(v);
            fm_map.insert("children".to_string(), Value::Bool(norm.children));
        }
    }

    if let Some(v) = fm_map.get("series") {
        let canonical = match v {
            Value::Bool(_) => true,
            Value::Sequence(items) => items.iter().all(|i| matches!(i, Value::String(_))),
            _ => false,
        };
        if !canonical {
            let norm = crate::frontmatter_union::normalize_series(v);
            fm_map.insert("series".to_string(), Value::Bool(norm.series));
        }
    }

    // uid is moss-owned metadata and is always semantically a string. YAML may
    // have parsed an all-digit uid as a number (e.g. `uid: 46160604`). Re-quote
    // it on write so the on-disk file self-heals to `uid: "46160604"` — closing
    // the loop on the build-side coercion. Author-owned
    // fields are never silently rewritten; uid is the one field moss owns.
    if let Some(v) = fm_map.get("uid") {
        if matches!(v, Value::Number(_) | Value::Bool(_)) {
            if let Some(s) = crate::frontmatter::value_as_string(v) {
                fm_map.insert("uid".to_string(), Value::String(s));
            }
        }
    }
}

