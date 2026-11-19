//! The stores this computer keeps for one site folder, named by the folder.
//!
//! Each folder gets one name, a hash of its canonical path ([`name_for`]),
//! shared by two parts: a small record under the durable root (`folder.json`:
//! a stable id, the path, the folder's public key, its volume) and the
//! disposable stores under the cache root, which the system may purge at the
//! cost of only time. Records kept elsewhere for a folder (trust answers,
//! plugin secrets) key on the id, so they follow the folder through a rename.
//!
//! A rename keeps the id. On a miss for a path, a store is moved to it when its
//! folder is gone from the same volume (by volume UUID, never mount path) and
//! its recorded key is one this folder proves: derived from
//! `.moss/identity/secret-key`, or listed in `previous-keys.jsonl`. A copy
//! whose original is still there, a folder carrying only `public.json`, or a
//! folder whose volume cannot be identified starts fresh.

use crate::identity::keypair;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

const RECORD: &str = "folder.json";

/// The name of `folder`'s stores, the same for every spelling of the same
/// directory: sha256 of its canonical path. A path that cannot be
/// canonicalized (it does not exist yet) is hashed as given.
pub fn name_for(folder: &Path) -> String {
    use sha2::{Digest, Sha256};
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    format!("{:x}", Sha256::digest(canonical.to_string_lossy().as_bytes()))
}

/// The filesystem a folder lives on: its UUID and where it is mounted now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub id: String,
    pub root: PathBuf,
}

/// One folder's stores, as [`FolderStores::resolve`] hands them out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderStore {
    /// Survives a rename and a key rotation; a copy gets its own.
    pub id: String,
    /// Disposable: anything here can be rebuilt from the folder.
    pub cache_dir: PathBuf,
    /// This computer's facts about the folder, kept across a cache purge and
    /// moved with the folder on a rename.
    pub record_dir: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Record {
    id: String,
    path: PathBuf,
    #[serde(default)]
    pubkey: Option<String>,
    #[serde(default)]
    volume: Option<RecordedVolume>,
}

/// The folder's place on its volume, relative to the volume's root, so a
/// volume mounted under another name still finds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RecordedVolume {
    id: String,
    rel: PathBuf,
}

pub struct FolderStores {
    record_root: PathBuf,
    cache_root: PathBuf,
    volume_of: fn(&Path) -> Option<Volume>,
}

impl FolderStores {
    /// `record_root` must survive a cache purge; `cache_root` need not. Neither
    /// may be inside a synced folder.
    pub fn new(record_root: PathBuf, cache_root: PathBuf) -> Self {
        Self::with_volume_lookup(record_root, cache_root, volume_of)
    }

    /// The running app's stores for `folder`: records under its non-roaming
    /// root, caches under the platform cache directory, both named by its
    /// identifier. A folder under the system temporary directory, and every
    /// folder a test binary opens, gets stores under the temporary directory
    /// instead: tests build there, and a test, or the binary one spawns, must
    /// never fill real app data.
    pub fn running_for(folder: &Path) -> Option<Self> {
        let temp = std::env::temp_dir();
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        if crate::infra::app_data::in_test_binary() || canonical(folder).starts_with(canonical(&temp)) {
            let root = temp.join("moss-folder-stores");
            return Some(Self::new(root.join("records"), root.join("caches")));
        }
        let local = crate::infra::app_data::AppRoots::running()?.local;
        let caches = dirs::cache_dir()?.join(local.file_name()?).join("folders");
        Some(Self::new(local.join("folders"), caches))
    }

    pub(crate) fn with_volume_lookup(
        record_root: PathBuf,
        cache_root: PathBuf,
        volume_of: fn(&Path) -> Option<Volume>,
    ) -> Self {
        Self { record_root, cache_root, volume_of }
    }

    /// The stores for `folder`, creating, moving after a rename or setting
    /// aside a replaced folder's as needed. `folder` must exist.
    pub fn resolve(&self, folder: &Path) -> io::Result<FolderStore> {
        let path = std::fs::canonicalize(folder)?;
        // The app and a command-line build open a folder at the same moment;
        // without this both mint an id, or both move a renamed folder's store.
        let _held = self.lock()?;
        let name = name_for(&path);
        let key = keypair::proven_pubkey(&path);
        let volume = (self.volume_of)(&path).and_then(|v| {
            Some((RecordedVolume { id: v.id, rel: path.strip_prefix(&v.root).ok()?.to_path_buf() }, v.root))
        });
        let fresh = Record {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.clone(),
            pubkey: key.clone(),
            volume: volume.as_ref().map(|(v, _)| v.clone()),
        };

        let record = match self.read(&name) {
            Some(mut rec) => match (&rec.pubkey, &key) {
                (Some(old), Some(new)) if old != new && !chain_of(&path).contains(old) => {
                    // Replaced in place by another site: its stores are not ours.
                    self.set_aside(&name)?;
                    fresh
                }
                _ => {
                    // A rotation in the chain, a folder's first key, or a key
                    // not readable right now: same folder, same stores.
                    if key.is_some() {
                        rec.pubkey = key;
                    }
                    rec.path = path;
                    rec.volume = fresh.volume;
                    rec
                }
            },
            None => match self.renamed_from(&key, &volume, &path)? {
                Some((old_name, mut rec)) => {
                    // allow:raw_write this computer's per-folder stores, outside any synced folder
                    std::fs::create_dir_all(&self.record_root)?;
                    crate::platform::rename_exclusive(&self.record_root.join(&old_name), &self.record_root.join(&name))?;
                    let old_cache = self.cache_root.join(&old_name);
                    if old_cache.exists() && !self.cache_root.join(&name).exists() {
                        crate::platform::rename_exclusive(&old_cache, &self.cache_root.join(&name))?;
                    }
                    rec.pubkey = key;
                    rec.path = path;
                    rec.volume = fresh.volume;
                    rec
                }
                None => fresh,
            },
        };
        self.write(&name, &record)?;
        Ok(FolderStore { id: record.id, cache_dir: self.cache_root.join(&name), record_dir: self.record_root.join(&name) })
    }

    /// The one store whose folder is gone from this folder's volume and whose
    /// recorded key this folder proves. None when there is no proof, no
    /// identified volume, or more than one such store.
    fn renamed_from(
        &self,
        key: &Option<String>,
        volume: &Option<(RecordedVolume, PathBuf)>,
        path: &Path,
    ) -> io::Result<Option<(String, Record)>> {
        let (Some(key), Some((volume, root))) = (key, volume) else { return Ok(None) };
        let chain = chain_of(path);
        let entries = match std::fs::read_dir(&self.record_root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains('.') {
                continue; // set aside, waiting for the collector
            }
            let Some(rec) = self.read(&name) else { continue };
            let Some(rv) = &rec.volume else { continue };
            let proven = rec.pubkey.as_ref().is_some_and(|k| k == key || chain.contains(k));
            let gone = rv.id == volume.id && root.join(&rv.rel).symlink_metadata().is_err();
            if proven && gone {
                found.push((name, rec));
            }
        }
        Ok(if found.len() == 1 { found.pop() } else { None })
    }

    /// Move a replaced folder's two parts out of its name for the collector.
    fn set_aside(&self, name: &str) -> io::Result<()> {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let aside = format!("{name}.replaced-{stamp}");
        for root in [&self.record_root, &self.cache_root] {
            if root.join(name).exists() {
                crate::platform::rename_exclusive(&root.join(name), &root.join(&aside))?;
            }
        }
        Ok(())
    }

    /// Held for one resolve, across every folder: resolves are rare and short.
    fn lock(&self) -> io::Result<std::fs::File> {
        // allow:raw_write this computer's per-folder stores, outside any synced folder
        std::fs::create_dir_all(&self.record_root)?;
        // allow:raw_write this computer's per-folder stores, outside any synced folder
        let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.record_root.join(".lock"))?;
        file.lock()?;
        Ok(file)
    }

    fn read(&self, name: &str) -> Option<Record> {
        // allow:raw_read app-data record, outside any vault — never cloud-synced
        let text = std::fs::read_to_string(self.record_root.join(name).join(RECORD)).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write(&self, name: &str, record: &Record) -> io::Result<()> {
        let dir = self.record_root.join(name);
        // allow:raw_write this computer's per-folder stores, outside any synced folder
        std::fs::create_dir_all(&dir)?;
        crate::infra::atomic_write::write_json_atomic(&dir.join(RECORD), record).map_err(io::Error::other)
    }
}

fn chain_of(folder: &Path) -> Vec<String> {
    keypair::read_previous_keys(folder).into_iter().map(|k| k.pubkey).collect()
}

/// The volume UUID and mount point Foundation reports for `path`.
#[cfg(target_os = "macos")]
fn volume_of(path: &Path) -> Option<Volume> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSString, NSURLResourceKey, NSURL, NSURLVolumeURLKey, NSURLVolumeUUIDStringKey};
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
    let get = |key: &NSURLResourceKey| {
        let mut value: Option<Retained<AnyObject>> = None;
        // SAFETY: both keys are documented to yield an object (NSString, NSURL).
        unsafe { url.getResourceValue_forKey_error(&mut value, key) }.ok()?;
        value
    };
    // SAFETY: Foundation's constant keys, initialized before any call.
    let (uuid_key, url_key) = unsafe { (NSURLVolumeUUIDStringKey, NSURLVolumeURLKey) };
    let id = get(uuid_key)?.downcast::<NSString>().ok()?.to_string();
    let root = get(url_key)?.downcast::<NSURL>().ok()?.path()?.to_string();
    Some(Volume { id, root: PathBuf::from(root) })
}

/// The filesystem id `statfs` reports (derived from the filesystem UUID on
/// ext4, xfs and btrfs) and the mount point found by walking up while the
/// device stays the same. A filesystem reporting no id is not identified.
#[cfg(target_os = "linux")]
fn volume_of(path: &Path) -> Option<Volume> {
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: a zeroed statfs is a valid out-parameter; `c` outlives the call.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    // SAFETY: fsid_t is two c_ints with private fields.
    let fsid: [i32; 2] = unsafe { std::mem::transmute(st.f_fsid) };
    if fsid == [0, 0] {
        return None;
    }
    let dev = path.metadata().ok()?.dev();
    let mut root = path;
    while let Some(parent) = root.parent() {
        if parent.metadata().ok()?.dev() != dev {
            break;
        }
        root = parent;
    }
    Some(Volume { id: format!("{:08x}{:08x}", fsid[0], fsid[1]), root: root.to_path_buf() })
}

/// Not identified yet: a renamed folder starts a fresh store here.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn volume_of(_path: &Path) -> Option<Volume> {
    None
}

#[cfg(test)]
#[path = "folder_store_tests.rs"]
mod tests;
