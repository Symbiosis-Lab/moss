//! The fetched half of a link card, kept in the site folder so it syncs.
//!
//! A card's title, icon and picture come from another site, so two machines
//! fetching one link can get different answers. What was fetched first is an
//! input of the build, stored under `.moss/data/social/links/`, and every
//! machine that syncs the folder builds the same card from it. What a build
//! derives from the picture stays in `build.nosync`.
//!
//! One file per link, named by the SHA-256 of its URL, so two machines that
//! fetch different links write different files and a sync client has nothing
//! to merge; only that exact name is read. A picture is named by the SHA-256
//! of its bytes and never deleted by a build: an entry another machine wrote
//! may name one this machine has not received yet.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::build::cloud_ledger::{InputEvidence, InputRole};
use crate::infra::atomic_write::write_atomic;
use crate::moss_paths::MossPaths;

/// One link's fetched metadata. Fields in alphabetical order and no fetch
/// time, so two identical fetches write identical bytes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Fetched {
    /// `<sha256>.<ext>` under `covers/`; `None` when the page names no picture
    /// or its download failed.
    pub cover: Option<String>,
    pub favicon: Option<String>,
    pub og_image: Option<String>,
    pub title: Option<String>,
    pub url: String,
}

/// The hex SHA-256 of a URL: the name of its entry here and in the old cache.
pub(crate) fn url_key(url: &str) -> String {
    hex::encode(Sha256::digest(url.as_bytes()))
}

fn links_dir(moss_dir: &Path) -> PathBuf {
    MossPaths::from_moss_dir(moss_dir.to_path_buf()).social_dir().join("links")
}

fn entry_path(moss_dir: &Path, url: &str) -> PathBuf {
    links_dir(moss_dir).join(format!("{}.json", url_key(url)))
}

/// Where a `cover` name's bytes live.
pub(crate) fn cover_path(moss_dir: &Path, cover: &str) -> PathBuf {
    links_dir(moss_dir).join("covers").join(cover)
}

/// Marks a file of the snapshot that only the cloud holds (and that
/// `outcome` has already asked for) pending in this build's input ledger, as
/// `places.toml` is: the preview builds without it, and a publish waits for a
/// build that read it, so no machine publishes a card that differs because of
/// what had synced.
pub(crate) fn hold(path: &Path, evidence: Option<&InputEvidence>) {
    if let Some(evidence) = evidence {
        evidence.require(path, InputRole::GeneratedData);
        evidence.pending(path);
    }
}

/// Whether a picture's bytes are on this machine. Never downloads; an evicted
/// picture is not a missing one, and on macOS 12 and 13 eviction leaves only a
/// hidden `.name.icloud` stub, so `exists()` alone cannot tell.
pub(crate) fn is_here(path: &Path) -> bool {
    !crate::build::icloud::is_still_in_the_cloud(path) && path.exists()
}

/// What the folder holds for one link.
#[derive(Debug)]
pub(crate) enum Entry {
    Fetched(Fetched),
    /// Requested from the cloud. Fetching it here instead would overwrite
    /// another machine's card.
    InCloud,
    Absent,
}

pub(crate) fn read(moss_dir: &Path, url: &str, evidence: Option<&InputEvidence>) -> Entry {
    use crate::build::outcome::{classify_unreadable, not_yet, Unreadable};
    let path = entry_path(moss_dir, url);
    let root = moss_dir.parent().unwrap_or(moss_dir);
    let read = if not_yet(root, &path) { Err(Unreadable::NotYet) } else { std::fs::read(&path).map_err(|e| classify_unreadable(root, &path, e)) };
    match read {
        Ok(bytes) => serde_json::from_slice(&bytes).map_or(Entry::Absent, Entry::Fetched),
        Err(Unreadable::NotYet) => {
            hold(&path, evidence);
            Entry::InCloud
        }
        Err(Unreadable::Broken(_)) => Entry::Absent,
    }
}

/// Saves an entry, leaving the file alone when it already holds these bytes,
/// so a build that fetched nothing new does not touch the synced folder.
pub(crate) fn write(moss_dir: &Path, entry: &Fetched) -> Result<(), String> {
    let mut json = serde_json::to_string_pretty(entry).map_err(|e| e.to_string())?;
    json.push('\n');
    let path = entry_path(moss_dir, &entry.url);
    if std::fs::read(&path).is_ok_and(|old| old == json.as_bytes()) {
        return Ok(());
    }
    write_atomic(&path, json)
}

/// Stores a picture's bytes under their own hash and returns its `cover` name.
pub(crate) fn store_cover(moss_dir: &Path, bytes: &[u8], ext: &str) -> Option<String> {
    let name = format!("{}.{ext}", hex::encode(Sha256::digest(bytes)));
    let path = cover_path(moss_dir, &name);
    if !path.exists() {
        write_atomic(&path, bytes).ok()?;
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str, title: &str) -> Fetched {
        Fetched { cover: None, favicon: None, og_image: None, title: Some(title.into()), url: url.into() }
    }

    #[test]
    fn identical_fetches_write_identical_bytes_with_sorted_keys() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://example.org/a";
        write(dir.path(), &entry(url, "A")).unwrap();
        let bytes = std::fs::read(entry_path(dir.path(), url)).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let keys: Vec<&str> = text.lines().filter_map(|l| l.trim().strip_prefix('"')?.split('"').next()).collect();
        assert_eq!(keys, ["cover", "favicon", "og_image", "title", "url"]);
        let other = tempfile::tempdir().unwrap();
        write(other.path(), &entry(url, "A")).unwrap();
        assert_eq!(std::fs::read(entry_path(other.path(), url)).unwrap(), bytes);
    }

    #[test]
    fn a_cover_is_stored_once_under_its_own_hash() {
        let dir = tempfile::tempdir().unwrap();
        let name = store_cover(dir.path(), b"bytes", "png").unwrap();
        assert_eq!(name, format!("{}.png", hex::encode(Sha256::digest(b"bytes"))));
        assert_eq!(store_cover(dir.path(), b"bytes", "png").unwrap(), name);
        assert_eq!(std::fs::read(cover_path(dir.path(), &name)).unwrap(), b"bytes");
    }
}
