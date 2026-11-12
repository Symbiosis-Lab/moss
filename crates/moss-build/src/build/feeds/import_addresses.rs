//! Public address facts retained by an import, independent of its base texts.
use std::{collections::BTreeMap, path::Path};
use serde::{Deserialize, Serialize};
pub(crate) const PATH: &str = ".moss/data/import/addresses.json";
#[derive(Serialize, Deserialize)]
pub(crate) struct Addresses {
    pub version: u32,
    /// An imported page's uid -> the feed entry id the old site gave it.
    pub pages: BTreeMap<String, String>,
}
const VERSION: u32 = 2;
impl Default for Addresses { fn default() -> Self { Self { version: VERSION, pages: BTreeMap::new() } } }
pub(crate) fn read(path: &Path) -> Result<Addresses, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(s) => s, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Addresses::default()),
        Err(e) => return Err(format!("Cannot read imported addresses: {e}")),
    };
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("Invalid imported addresses: {e}"))?;
    match value["version"].as_u64() {
        // Version 1 named each page's uid, which no Ghost feed emits as a guid.
        Some(1) => Ok(Addresses::default()),
        Some(v) if v == u64::from(VERSION) => serde_json::from_value(value).map_err(|e| format!("Invalid imported addresses: {e}")),
        _ => Err("Unsupported imported address version".into()),
    }
}
pub(crate) fn apply(root: &Path, documents: &mut [crate::build::types::ParsedDocument]) -> Result<(), String> {
    let book = read(&root.join(PATH))?;
    for doc in documents {
        doc.import_feed_id = doc.uid.as_ref().and_then(|uid| book.pages.get(uid)).cloned();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_version_ledger_reads_as_empty_and_the_current_one_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("addresses.json");
        std::fs::write(&path, r#"{"version":1,"pages":["00000000-0000-4000-8000-000000000001"]}"#).unwrap();
        assert!(read(&path).unwrap().pages.is_empty());
        let mut book = Addresses::default();
        book.pages.insert("uid".into(), "65a1b2c3d4e5f60718293a4b".into());
        std::fs::write(&path, serde_json::to_string(&book).unwrap()).unwrap();
        assert_eq!(read(&path).unwrap().pages["uid"], "65a1b2c3d4e5f60718293a4b");
    }
}
