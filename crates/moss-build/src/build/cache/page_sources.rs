//! Which rendered bytes the last build's slot pass used, per page: the source oid
//! of each page's `html/slots` record, as the cache sweep reads it. A style save
//! changes every page's HTML and files a whole new set of records, and the sweep
//! must drop the set the site has moved on from; age alone would keep every saved
//! style for its 90 days.

use crate::moss_paths::MossPaths;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// The transform a slot-injection record is filed under.
pub(crate) const SLOT_INJECT_TRANSFORM: &str = "html/slots";

/// Save the whole site's pages: `scanned` are the ones this pass read, and a
/// carried page takes its entry from the last save. A carried page with no entry
/// means the list cannot name the whole site, so the list is removed and the
/// sweep falls back to age.
pub(crate) fn save<'a>(paths: &MossPaths, scanned: BTreeMap<String, String>, carried: impl Iterator<Item = &'a str>) {
    let path = paths.cache_page_sources();
    let previous: Option<BTreeMap<String, String>> =
        std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let mut sources = scanned;
    for page in carried {
        if sources.contains_key(page) {
            continue;
        }
        let Some(oid) = previous.as_ref().and_then(|previous| previous.get(page)) else {
            remove(&path);
            return;
        };
        sources.insert(page.to_string(), oid.clone());
    }
    let written = serde_json::to_vec(&sources)
        .map_err(std::io::Error::other)
        .and_then(|bytes| crate::build::io_utils::write_output_if_changed(&path, &bytes));
    if let Err(e) = written {
        log::info!("[cache] could not save {}: {}", path.display(), e);
        // Left in place, the last build's list would condemn this build's pages.
        remove(&path);
    }
}

fn remove(path: &Path) {
    // allow:unlink a per-machine cache record, not staging
    let _ = std::fs::remove_file(path);
}

/// The source oids the last pass used. `None` when no list exists; an unreadable
/// or unparseable list is an error, like the sweep's other mark inputs.
pub(super) fn read(path: &Path) -> std::io::Result<Option<HashSet<String>>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let sources: BTreeMap<String, String> =
        serde_json::from_slice(&bytes).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(sources.into_values().collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(dir: &Path) -> MossPaths {
        let paths = MossPaths::from_moss_dir(dir.join(".moss"));
        std::fs::create_dir_all(paths.cache_dir()).unwrap();
        paths
    }

    #[test]
    fn a_carried_page_keeps_the_source_the_last_pass_saved() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        save(&paths, BTreeMap::from([("a".to_string(), "one".to_string()), ("b".to_string(), "two".to_string())]), std::iter::empty());

        save(&paths, BTreeMap::from([("a".to_string(), "three".to_string())]), std::iter::once("b"));

        let live = read(&paths.cache_page_sources()).unwrap().unwrap();
        assert_eq!(live, HashSet::from(["three".to_string(), "two".to_string()]));
    }

    #[test]
    fn a_carried_page_no_list_names_removes_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths(dir.path());
        save(&paths, BTreeMap::from([("a".to_string(), "one".to_string())]), std::iter::empty());

        save(&paths, BTreeMap::new(), std::iter::once("unknown"));

        assert_eq!(read(&paths.cache_page_sources()).unwrap(), None);
    }
}
