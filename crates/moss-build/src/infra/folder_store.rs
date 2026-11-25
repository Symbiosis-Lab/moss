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
/// directory: sha256 of its canonical path. A folder that does not exist yet
/// is named by its deepest existing ancestor's canonical path plus the rest,
/// which is the name it will have once created.
pub fn name_for(folder: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(canonical_to_be(folder).to_string_lossy().as_bytes()))
}

fn canonical_to_be(folder: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut at = folder;
    loop {
        if let Ok(found) = std::fs::canonicalize(at) {
            return missing.iter().rev().fold(found, |path, part| path.join(part));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name);
                at = parent;
            }
            _ => return folder.to_path_buf(),
        }
    }
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
    /// instead. A test that runs the moss binary on a folder elsewhere gives
    /// it its own HOME, or its stores land in real app data.
    pub fn running_for(folder: &Path) -> Option<Self> {
        let temp = std::env::temp_dir();
        if crate::infra::app_data::in_test_binary() || canonical_to_be(folder).starts_with(canonical_to_be(&temp)) {
            return Some(Self::temporary());
        }
        Self::app()
    }

    /// The stores every folder under the temporary directory shares.
    fn temporary() -> Self {
        let root = std::env::temp_dir().join("moss-folder-stores");
        Self::new(root.join("records"), root.join("caches"))
    }

    fn app() -> Option<Self> {
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
        // The volume lookup costs milliseconds, so a known folder keeps the
        // volume it recorded. A stale one can only make a later rename miss,
        // which starts a fresh store: a lost cache, never lost data.
        let volume_of = || {
            (self.volume_of)(&path).and_then(|v| {
                Some((RecordedVolume { id: v.id, rel: path.strip_prefix(&v.root).ok()?.to_path_buf() }, v.root))
            })
        };
        let fresh = |volume: Option<(RecordedVolume, PathBuf)>| Record {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.clone(),
            pubkey: key.clone(),
            volume: volume.map(|(v, _)| v),
        };

        let record = match self.read(&name) {
            Some(mut rec) => match (&rec.pubkey, &key) {
                (Some(old), Some(new)) if old != new && !chain_of(&path).contains(old) => {
                    // Replaced in place by another site: its stores are not ours.
                    self.set_aside(&name)?;
                    fresh(volume_of())
                }
                _ => {
                    // A rotation in the chain, a folder's first key, or a key
                    // not readable right now: same folder, same stores.
                    if rec.volume.is_none() {
                        rec.volume = volume_of().map(|(v, _)| v);
                    }
                    if key.is_some() {
                        rec.pubkey = key;
                    }
                    rec.path = path;
                    rec
                }
            },
            None => {
                let volume = volume_of();
                match self.renamed_from(&key, &volume, &path)? {
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
                        rec.volume = volume.map(|(v, _)| v);
                        rec
                    }
                    None => fresh(volume),
                }
            }
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

    /// Where `folder`'s stores will be once it exists, writing nothing: a
    /// reader asking before the folder is made finds the place a build will use.
    fn to_be(&self, folder: &Path) -> FolderStore {
        let name = name_for(folder);
        FolderStore { id: String::new(), cache_dir: self.cache_root.join(&name), record_dir: self.record_root.join(&name) }
    }

    /// Remove the stores of folders that are gone: the recorded path is missing
    /// while its parent exists or its volume is mounted, and nothing resolved it
    /// for `grace`. A folder on an unmounted volume keeps its stores. A store
    /// set aside for a replaced folder goes once it is older than `grace`.
    pub fn collect(&self, grace: std::time::Duration) -> io::Result<usize> {
        let entries = match std::fs::read_dir(&self.record_root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let old = |path: &Path| old_by(path, grace);
        let gone = |name: &str| {
            if name.contains(".replaced-") {
                old(&self.record_root.join(name))
            } else {
                self.read(name).is_some_and(|rec| old(&self.record_root.join(name).join(RECORD)) && self.folder_gone(&rec))
            }
        };
        let mut removed = 0;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Judged unlocked, since every process sweeps once and most entries
            // stay; judged again under the lock, where a resolve may have revived it.
            if name.starts_with('.') || !gone(&name) {
                continue;
            }
            let _held = self.lock()?;
            if !gone(&name) {
                continue;
            }
            for root in [&self.cache_root, &self.record_root] {
                // allow:unlink this computer's store for a folder that no longer exists
                match std::fs::remove_dir_all(root.join(&name)) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                }
            }
            removed += 1;
        }
        // Written for a folder before it existed, and the folder never came.
        let orphan_grace = grace.max(std::time::Duration::from_secs(24 * 3600));
        let orphan = |path: &Path, name: &str| !name.starts_with('.') && !self.record_root.join(name).exists() && old_by(path, orphan_grace);
        for entry in std::fs::read_dir(&self.cache_root).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !orphan(&entry.path(), &name) {
                continue;
            }
            let _held = self.lock()?;
            // allow:unlink this computer's cache for a folder that was never made
            if orphan(&entry.path(), &name) && std::fs::remove_dir_all(entry.path()).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn folder_gone(&self, rec: &Record) -> bool {
        if rec.path.symlink_metadata().is_ok() {
            return false;
        }
        if rec.path.parent().is_some_and(|parent| parent.is_dir()) {
            return true;
        }
        // The parent is missing too: gone only if its volume is mounted now.
        let Some(recorded) = &rec.volume else { return false };
        let Some(existing) = rec.path.ancestors().find(|a| a.exists()) else { return false };
        (self.volume_of)(existing).is_some_and(|v| v.id == recorded.id)
    }

    /// Held for one resolve or one removal, across every folder.
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
        #[cfg(test)]
        {
            *RECORD_WRITES.lock().unwrap_or_else(|e| e.into_inner()).entry(name.to_string()).or_insert(0) += 1;
        }
        let dir = self.record_root.join(name);
        // allow:raw_write this computer's per-folder stores, outside any synced folder
        std::fs::create_dir_all(&dir)?;
        crate::infra::atomic_write::write_json_atomic(&dir.join(RECORD), record).map_err(io::Error::other)
    }
}

/// The folder whose temporary-root store holds `path`, by its record: a test
/// that has only a served directory finds its folder this way.
#[cfg(test)]
pub(crate) fn test_folder_holding(path: &Path) -> Option<PathBuf> {
    let stores = FolderStores::temporary();
    let rest = path.strip_prefix(&stores.cache_root).ok()?;
    let name = rest.components().next()?.as_os_str().to_str()?;
    stores.read(name).map(|record| record.path)
}

fn old_by(path: &Path, grace: std::time::Duration) -> bool {
    path.symlink_metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > grace)
}

/// `folder.json` writes by store name, for the tests that count them.
#[cfg(test)]
pub(crate) static RECORD_WRITES: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, usize>>> =
    std::sync::LazyLock::new(Default::default);

/// How long a gone folder's stores wait before [`collect_gone_once`] removes
/// them: a folder moved aside for a while keeps its facts.
const GONE_GRACE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

/// Once per process, in the background, from the first resolve: remove the
/// stores of folders that are gone. The shared temporary root goes at once, since nothing else empties it;
/// the app's after [`GONE_GRACE`]. A test binary never sweeps real app data.
fn collect_gone_once() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new().name("folder-store-gc".into()).spawn(|| {
            sweep_unstored(&std::env::temp_dir());
            let mut sweeps = vec![(FolderStores::temporary(), std::time::Duration::ZERO)];
            if !crate::infra::app_data::in_test_binary() {
                sweeps.extend(FolderStores::app().map(|stores| (stores, GONE_GRACE)));
            }
            for (stores, grace) in sweeps {
                if let Err(e) = stores.collect(grace) {
                    log::info!("folder store sweep stopped ({e})");
                }
            }
        });
    });
}

/// `folder`'s stores for this process, resolved once per folder.
///
/// A folder not created yet gets the place its stores will have. When no
/// store can be resolved otherwise, the folder gets a scratch store for this
/// process only, so a reader finds nothing and a build that did not check
/// [`try_for_folder`] runs without stored facts rather than keeping them in
/// the folder.
pub fn for_folder(folder: &Path) -> FolderStore {
    match try_for_folder(folder) {
        Ok(store) => store,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            FolderStores::running_for(folder).map_or_else(|| unstored(folder), |stores| stores.to_be(folder))
        }
        Err(_) => unstored(folder),
    }
}

/// [`for_folder`], or why this computer has no store for `folder`. A build
/// asks this before it writes, so its output never falls back into scratch.
pub fn try_for_folder(folder: &Path) -> io::Result<FolderStore> {
    static RESOLVED: std::sync::LazyLock<Memo> = std::sync::LazyLock::new(Default::default);
    memoized(&RESOLVED, folder, |canonical| {
        let resolved = FolderStores::running_for(canonical)
            .ok_or_else(|| io::Error::other("no app-data directory"))
            .and_then(|stores| stores.resolve(canonical));
        collect_gone_once();
        if let Err(e) = &resolved {
            log::warn!("no computer store for {} ({e}); this run keeps nothing for it", folder.display());
        }
        resolved
    })
}

type Answer = Result<FolderStore, (io::ErrorKind, String)>;
/// One cell per folder, shared by its spellings.
type Memo = std::sync::Mutex<std::collections::HashMap<PathBuf, std::sync::Arc<std::sync::OnceLock<Answer>>>>;

/// `folder`'s answer from `memo`, resolving it once. A resolve blocks only the
/// callers asking for the same folder, never every path lookup in the process.
fn memoized(memo: &Memo, folder: &Path, resolve: impl FnOnce(&Path) -> io::Result<FolderStore>) -> io::Result<FolderStore> {
    let answer = |known: &Answer| known.clone().map_err(|(kind, message)| io::Error::new(kind, message));
    let known = memo.lock().unwrap_or_else(|e| e.into_inner()).get(folder).cloned();
    if let Some(found) = known.as_ref().and_then(|cell| cell.get()) {
        return answer(found);
    }
    // Not created yet: try again on the next call.
    let canonical = std::fs::canonicalize(folder)?;
    let cell = {
        let mut known = memo.lock().unwrap_or_else(|e| e.into_inner());
        let cell = known.entry(canonical.clone()).or_default().clone();
        known.insert(folder.to_path_buf(), cell.clone());
        cell
    };
    answer(cell.get_or_init(|| resolve(&canonical).map_err(|e| (e.kind(), e.to_string()))))
}

const UNSTORED_PREFIX: &str = "moss-unstored-";

fn unstored(folder: &Path) -> FolderStore {
    let root = std::env::temp_dir().join(format!("{UNSTORED_PREFIX}{}", std::process::id())).join(name_for(folder));
    FolderStore { id: String::new(), cache_dir: root.join("cache"), record_dir: root.join("record") }
}

/// Removes the scratch stores of processes that have exited: Windows never
/// ages its temporary directory out.
pub(crate) fn sweep_unstored(temp: &Path) {
    let Ok(entries) = std::fs::read_dir(temp) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.strip_prefix(UNSTORED_PREFIX)).and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if pid != std::process::id() && !pid_alive(pid) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// A pid reused by an unrelated process reads as alive, which only keeps a
/// stale record until that process exits too.
#[cfg(unix)]
pub(crate) fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else { return false };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 delivers nothing; it only checks the pid exists.
    let signalled = unsafe { libc::kill(pid, 0) == 0 };
    signalled || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
pub(crate) fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: a null handle (no such process) is checked before use, and the
    // handle is closed once its exit code is read.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let read = GetExitCodeProcess(handle, &mut code) != 0;
        let _ = CloseHandle(handle);
        read && code == STILL_ACTIVE as u32
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
