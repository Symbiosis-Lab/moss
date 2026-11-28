//! `moss migrate --list`: the retired names a folder still uses, each with
//! what replaces it and how `moss migrate` moves the folder to it. Reads
//! files only; never builds or writes.

use std::path::Path;

use moss_core::contract::retired::{Migrate, Retired};

const THEME_CSS: &str = ".moss/theme/style.css";

/// One retired row the folder still uses.
#[derive(Debug)]
pub(super) struct Found {
    pub row: &'static Retired,
    pub files: Vec<String>,
    pub occurrences: usize,
}

/// Every retired row the folder uses, in the order the build warns about them.
pub(super) fn found(folder: &Path) -> Result<Vec<Found>, String> {
    let css = match std::fs::read_to_string(folder.join(THEME_CSS)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("could not read {THEME_CSS}: {e}")),
    };
    let stripped = crate::build::theme_lint::strip_comments(&css);
    Ok(crate::build::theme_lint::retired_uses(&stripped)
        .into_iter()
        .map(|(row, occurrences)| Found { row, files: vec![THEME_CSS.to_string()], occurrences })
        .collect())
}

fn migrate_word(row: &Retired) -> &'static str {
    match row.migrate {
        Migrate::Rename(_) => "rename",
        Migrate::Gone => "gone",
    }
}

pub(super) fn to_json(found: &[Found]) -> serde_json::Value {
    serde_json::json!({
        "retired": found.iter().map(|f| serde_json::json!({
            "id": f.row.id(),
            "kind": f.row.kind.slug(),
            "name": f.row.name,
            "retired_in": f.row.retired_in,
            "migrate": migrate_word(f.row),
            "replaced_by": f.row.replaced_by(),
            "keeps_look": f.row.keeps_look(),
            "files": f.files,
            "occurrences": f.occurrences,
            "note": f.row.note,
        })).collect::<Vec<_>>(),
    })
}

pub(super) fn to_text(found: &[Found]) -> String {
    if found.is_empty() {
        return "Nothing to migrate.\n".to_string();
    }
    let mut out = String::new();
    for f in found {
        let times = if f.occurrences == 1 { "once".to_string() } else { format!("{} times", f.occurrences) };
        let now = f.row.replaced_by().map_or_else(|| "nothing replaces it".to_string(), |now| format!("now {now}"));
        out.push_str(&format!("{}  {} ({times}), retired {}: {}; {now}\n", f.row.id(), f.files.join(", "), f.row.when(), f.row.note));
    }
    out
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;
