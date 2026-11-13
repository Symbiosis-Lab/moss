//! Downward detection of nested moss roots, and root-shape classification.
//!
//! The upward walk (`vault_root::VaultRoot::find_containing`) answers "which
//! ancestor owns this path". This module answers the other two questions the
//! nested-folder guard needs, both pure and injectable:
//!
//! * [`classify_root`] — does this path LOOK like a folder nobody should turn
//!   into a site (a cloud-provider mount, the home directory, `/`)? Zero I/O:
//!   string shape only, so the answer is available before any walk.
//! * [`find_nested_roots`] — which descendants already ARE moss roots? A
//!   bounded breadth-first walk whose trigger is one `lstat`
//!   (`child/.moss` is a directory), so it is safe on evicted File Provider
//!   trees where file *bytes* fail but directory metadata is local.
//!
//! The app-side decision layer consumes the [`NestedRootsReport`] and turns it
//! into a dialog/decision; this module never decides anything.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// What kind of folder a candidate root is, judged by path shape alone.
///
/// Everything except `Normal` is a folder that is almost never a site root:
/// turning it into one sweeps a whole drive/home into a build (the 2026-08-17
/// Google-Drive-root incident).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootClass {
    Normal,
    /// A cloud provider's mount, or its FIRST level (the localized "My Drive"
    /// wrapper — never matched by name, only by depth, so `我的雲端硬碟`
    /// classifies identically).
    CloudProviderRoot { provider: String },
    HomeDir,
    /// `~/Desktop`, `~/Documents`, `~/Downloads`.
    SystemSpecial(String),
    FilesystemRoot,
}

/// One nested moss root found below the scanned folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedRootInfo {
    /// `/`-separated path relative to the scanned root (build convention).
    pub rel_path: String,
    pub abs_path: String,
    /// Folder basename — the future site name (`VaultRoot::name` semantics).
    pub name: String,
    /// `Some(true)` = `[deployment].site_id` present · `Some(false)` =
    /// preview-only · `None` = state unreadable (evicted / permissions).
    pub published: Option<bool>,
    pub site_id: Option<String>,
    /// The site has no `.moss/`; its data sits in this copy of one.
    pub data_in: Option<String>,
}

/// What a copy of `.moss` shows about itself, read without downloading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyEvidence {
    /// A set-aside with a record of a publish.
    Published,
    /// A set-aside in which every record of a publish is provably absent.
    Unpublished,
    /// A sync client's copy whose listing is on this computer: nothing more
    /// is needed to know it is the folder's site data.
    Listed,
    /// Something it needs is still in the cloud: waiting can answer.
    Arriving,
    /// Here, but it cannot be read: waiting cannot change that.
    Unreadable,
}

/// A `.moss/` under another name (see [`is_moss_copy`]) and what its own
/// state says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MossCopy {
    pub name: String,
    pub evidence: CopyEvidence,
}

impl MossCopy {
    /// Did moss set this aside (`.moss.removed-<date>`), rather than a sync
    /// client copy it?
    pub fn is_set_aside(&self) -> bool {
        is_set_aside_name(&self.name)
    }

    /// Is this the folder's site data, to be put back rather than replaced?
    /// A conflict copy always is. A set-aside is unless it provably records
    /// no publish: a folder set aside on purpose while unpublished stays so.
    /// The walk asks too, since a folder holding site data is a boundary.
    pub fn is_site_data(&self) -> bool {
        !(self.is_set_aside() && self.evidence == CopyEvidence::Unpublished)
    }
}

fn is_set_aside_name(name: &str) -> bool {
    name.strip_prefix(".moss.removed-").is_some_and(|stamp| !stamp.is_empty() && !stamp.contains(['/', '\\']))
}

/// The detector's whole answer, handed to the app-side decision layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedRootsReport {
    pub root: String,
    pub root_class: RootClass,
    /// At most `max_reported`, shallowest-first (BFS order).
    pub nested: Vec<NestedRootInfo>,
    /// A depth / directory / report cap was hit: the scan may have missed
    /// roots. The DECISION layer turns this into ask-or-proceed using
    /// `root_class`; the detector never silently proceeds past a cap.
    pub truncated: bool,
    /// Telemetry: directories the walk listed.
    pub dirs_visited: usize,
    /// Copies of a `.moss/` sitting directly in the root. Filled only when
    /// the root has no `.moss/` of its own.
    pub moss_copies: Vec<MossCopy>,
    /// Every entry of the root itself was listed. When false, a `.moss` or a
    /// copy of one may sit in the root unseen.
    pub root_listing_complete: bool,
}

/// Bounds for [`find_nested_roots`]. Injectable so cap behavior is testable
/// without materializing thousands of directories.
#[derive(Debug, Clone)]
pub struct ScanLimits {
    pub max_depth: usize,
    pub max_dirs: usize,
    pub max_reported: usize,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self { max_depth: 6, max_dirs: 4000, max_reported: 5 }
    }
}

/// Does `dir` own a `.moss/` directory — i.e. is it a moss root? One `lstat`,
/// plus two `canonicalize` calls where a `.moss/` is found.
/// Also the subtree-boundary predicate for future scanner/watcher pruning.
///
/// moss's own home (`MOSS_HOME`, by default `~/.moss`) is tooling, not a site
/// marker, so its parent is never a site, through a symlink either.
pub fn owns_moss(dir: &Path) -> bool {
    let marker = dir.join(".moss");
    marker.is_dir() && !is_moss_home(&marker)
}

fn is_moss_home(marker: &Path) -> bool {
    let Ok(home) = crate::infra::home::moss_home() else { return false };
    let home = crate::vault_root::resolve_input(home);
    home == marker
        || matches!((marker.canonicalize(), home.canonicalize()), (Ok(m), Ok(h)) if m == h)
}

/// Is this path under a known cloud-storage mount? Substring shape match,
/// tolerant of how the home directory is spelled. Owner of the provider
/// prefix list (moved here from `build/pipeline.rs`, which re-exports it).
pub fn is_cloud_storage_path(source_path: &str) -> bool {
    source_path.contains("/CloudStorage/")
        || source_path.contains("com~apple~CloudDocs")
        || source_path.contains("/Dropbox/")
        || source_path.contains("/Dropbox (")
        || source_path.contains("/Volumes/GoogleDrive")
        || source_path.contains("/OneDrive")
}

/// Classify a candidate root by path shape. Zero filesystem I/O.
///
/// The cloud rule is localization-independent: a provider account dir under
/// `~/Library/CloudStorage/` and its first level (Google's localized
/// "My Drive" / "我的雲端硬碟" wrapper) both classify as
/// [`RootClass::CloudProviderRoot`]; the wrapper is matched by DEPTH, never by
/// name. Two levels down is a real folder again → `Normal`. The same
/// depth rule covers `/Volumes/GoogleDrive*` (File Stream, whose first level
/// is the same localized wrapper). Legacy `~/Dropbox`* and `~/OneDrive*` have
/// no wrapper level, so only the mount folder itself classifies.
pub fn classify_root(path: &Path, home: Option<&Path>) -> RootClass {
    if path.parent().is_none() {
        return RootClass::FilesystemRoot;
    }

    // /Volumes/GoogleDrive* — mount or its first level.
    {
        let comps: Vec<String> = path
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        if comps.len() >= 2
            && comps.len() <= 3
            && comps[0] == "Volumes"
            && comps[1].starts_with("GoogleDrive")
        {
            return RootClass::CloudProviderRoot { provider: "Google Drive".to_string() };
        }
    }

    let Some(home) = home else { return RootClass::Normal };
    if path == home {
        return RootClass::HomeDir;
    }

    if let Ok(rel) = path.strip_prefix(home) {
        let owned: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let parts: Vec<&str> = owned.iter().map(String::as_str).collect();
        match parts.as_slice() {
            [name] if *name == "Desktop" || *name == "Documents" || *name == "Downloads" => {
                return RootClass::SystemSpecial(name.to_string());
            }
            // ~/Library/CloudStorage itself: every provider's account dirs
            // live directly inside — as whole-drive-shaped as it gets.
            ["Library", "CloudStorage"] => {
                return RootClass::CloudProviderRoot { provider: "CloudStorage".to_string() };
            }
            // Account dir (`GoogleDrive-a@b`) or its first level (the
            // localized wrapper). Deeper is a real folder → Normal below.
            ["Library", "CloudStorage", acct]
            | ["Library", "CloudStorage", acct, _] => {
                return RootClass::CloudProviderRoot { provider: provider_from_account_dir(acct) };
            }
            // Legacy mounts directly under home; no wrapper level.
            [name] if *name == "Dropbox" || name.starts_with("Dropbox (") => {
                return RootClass::CloudProviderRoot { provider: "Dropbox".to_string() };
            }
            [name] if name.starts_with("OneDrive") => {
                return RootClass::CloudProviderRoot { provider: "OneDrive".to_string() };
            }
            _ => {}
        }
    }

    RootClass::Normal
}

/// `GoogleDrive-alice@example.com` → `GoogleDrive`. The account dir is always
/// `<Provider>-<account>`; a dir with no `-` is its own provider name.
fn provider_from_account_dir(acct: &str) -> String {
    acct.split('-').next().unwrap_or(acct).to_string()
}

/// Find every moss root nested below `root`, bounded by `limits`.
///
/// Returns an empty report immediately when `root` itself is already a moss
/// root (opening an existing vault costs one stat; the remediation path uses
/// [`scan_below`] directly to look inside an existing root). Otherwise a
/// bounded BFS: never follows symlinks, skips `.git` / `node_modules` /
/// `.moss` and its copies, prunes below a match. The trigger is `child/.moss` `is_dir()` —
/// one `lstat`, no file read — so an evicted cloud tree cannot hang the walk;
/// only the ≤ `max_reported` FOUND candidates and any set-aside copy of a
/// `.moss` get their `state.toml` read,
/// and a failed read labels them `published: None` instead of erroring.
pub fn find_nested_roots(root: &Path, home: Option<&Path>, limits: &ScanLimits) -> NestedRootsReport {
    if owns_moss(root) {
        return NestedRootsReport {
            root: root.to_string_lossy().into_owned(),
            root_class: classify_root(root, home),
            nested: Vec::new(),
            truncated: false,
            dirs_visited: 0,
            moss_copies: Vec::new(),
            root_listing_complete: true,
        };
    }
    scan_below(root, home, limits)
}

/// The walk itself, without the root-owns-`.moss` short-circuit. Used by
/// [`find_nested_roots`] for prevention and directly by the remediation path
/// ("this root already has `.moss` — what does it contain?").
pub fn scan_below(root: &Path, home: Option<&Path>, limits: &ScanLimits) -> NestedRootsReport {
    let mut nested: Vec<NestedRootInfo> = Vec::new();
    let mut truncated = false;
    let mut visited = 0usize;
    let root_owned = owns_moss(root);
    let mut moss_copies: Vec<MossCopy> = Vec::new();
    let mut root_listing_complete = true;
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        // A dir beyond the depth bound is never examined: its mere presence
        // means the scan is incomplete, which the decision layer must see.
        if depth > limits.max_depth {
            truncated = true;
            continue;
        }
        if visited >= limits.max_dirs {
            truncated = true;
            break;
        }

        // The root itself is the caller's candidate, not a nested root.
        if depth > 0 && owns_moss(&dir) {
            nested.push(nested_root_info(&dir, root, None));
            if nested.len() >= limits.max_reported {
                truncated |= !queue.is_empty();
                break;
            }
            continue; // prune: a root is a content boundary
        }

        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            // An unlistable directory (permissions, eviction) is an
            // UNEXPLORED subtree, not an empty one — the decision layer
            // must not mistake this for a clean scan.
            Err(_) => {
                truncated = true;
                if depth == 0 {
                    root_listing_complete = false;
                }
                continue;
            }
        };
        visited += 1;
        let mut subdirs: Vec<PathBuf> = Vec::new();
        let mut copies: Vec<MossCopy> = Vec::new();
        for entry in entries {
            // Same reasoning as an unlistable dir, one level down: a dropped
            // entry, or one whose type is unknown, is unexplored, not absent.
            let Ok((entry, ft)) = entry.and_then(|e| e.file_type().map(|ft| (e, ft))) else {
                truncated = true;
                if depth == 0 {
                    root_listing_complete = false;
                }
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".git" || name == "node_modules" || name == ".moss" {
                continue;
            }
            // Do not follow symlinks; `file_type()` reports the link itself.
            if !ft.is_dir() || ft.is_symlink() {
                continue;
            }
            if is_moss_copy(&name) {
                copies.push(moss_copy(&entry.path(), &name));
            } else {
                subdirs.push(entry.path());
            }
        }
        if depth == 0 {
            if !root_owned {
                if !copies.is_empty() {
                    let set_aside = copies.iter().filter(|c| c.is_set_aside()).count();
                    let published = copies.iter().filter(|c| c.evidence == CopyEvidence::Published).count();
                    log::info!("[nested_roots] {} copies of .moss: {} from a sync client, {set_aside} set aside ({published} published)", copies.len(), copies.len() - set_aside);
                }
                moss_copies = copies;
            }
        } else if let Some(copy) = copies.iter().find(|c| !root_owned && c.is_site_data()) {
            // A site whose data sits under another name is a content
            // boundary too, and is reported so its own folder gets opened.
            // Inside a site that owns `.moss`, a copy is its leftover instead.
            nested.push(nested_root_info(&dir, root, Some(copy)));
            if nested.len() >= limits.max_reported {
                truncated |= !queue.is_empty();
                break;
            }
            continue;
        }
        queue.extend(subdirs.into_iter().map(|d| (d, depth + 1)));
    }

    NestedRootsReport {
        root: root.to_string_lossy().into_owned(),
        root_class: classify_root(root, home),
        nested,
        truncated,
        dirs_visited: visited,
        moss_copies,
        root_listing_complete,
    }
}

/// A `.moss/` under another name: a sync client's copy of it (see
/// [`is_sync_copy_of`]) or the set-aside moss leaves when it takes a folder's
/// site data out of use (`.moss.removed-<date>`). Never a site marker, and
/// never walked: a copy holds a build cache of thousands of folders, enough to
/// exhaust the scan on its own.
pub fn is_moss_copy(name: &str) -> bool {
    is_sync_copy_of(name, ".moss") || is_set_aside_name(name)
}

/// A sync client's copy of the entry `base`: `base 2` (Google Drive, iCloud),
/// `base (1)` or `base (conflicted copy …)` (Dropbox), `base.sync-conflict-…`
/// (Syncthing). OneDrive's `base-<computer>` is left out: it cannot be told
/// from a real name such as `.moss-cache`. A name with a path separator is
/// never a copy.
pub fn is_sync_copy_of(name: &str, base: &str) -> bool {
    if name.contains(['/', '\\']) {
        return false;
    }
    let Some(rest) = name.strip_prefix(base) else { return false };
    if let Some(stamp) = rest.strip_prefix(".sync-conflict-") {
        return !stamp.is_empty();
    }
    let Some(rest) = rest.strip_prefix(' ') else { return false };
    (!rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        || (rest.len() > 2 && rest.starts_with('(') && rest.ends_with(')'))
}

/// What a copy says about itself, read without downloading. A directory the
/// cloud has not listed onto this computer is unknown, never empty: inside it,
/// a file reads as absent.
fn moss_copy(path: &Path, name: &str) -> MossCopy {
    let evidence = match local_listing(path) {
        Err(evidence) => evidence,
        Ok(names) if is_set_aside_name(name) => records_publish(path, &names),
        Ok(_) => CopyEvidence::Listed,
    };
    MossCopy { name: name.to_string(), evidence }
}

/// Why something a copy holds could not be read.
fn failed_read(error: &std::io::Error) -> CopyEvidence {
    if crate::build::icloud::is_dataless_unavailable(error) { CopyEvidence::Arriving } else { CopyEvidence::Unreadable }
}

/// The names in `dir`, when its listing is on this computer and reads whole.
fn local_listing(dir: &Path) -> Result<Vec<std::ffi::OsString>, CopyEvidence> {
    if crate::build::icloud::is_dataless_dir(dir) {
        return Err(CopyEvidence::Arriving);
    }
    std::fs::read_dir(dir)
        .and_then(|entries| entries.map(|entry| entry.map(|e| e.file_name())).collect())
        .map_err(|e| failed_read(&e))
}

/// Folders in a `.moss` whose contents record a publish.
pub(crate) const PUBLISH_TRAILS: [&[&str]; 3] = [&["deploy", "records"], &["history", "publishes"], &["keys"]];

/// An arrival could still prove a publish, so it outranks a file that is
/// here and cannot be read.
fn note(answer: &mut CopyEvidence, found: CopyEvidence) {
    if *answer != CopyEvidence::Arriving {
        *answer = found;
    }
}

/// Does a set-aside record a published site? Every place a publish leaves a
/// trace is asked: the state's site id or publish evidence, a publish record,
/// a history entry, an address key, a signed-in email. Any trace answers
/// `Published`; `Unpublished` needs each to be provably absent.
fn records_publish(dir: &Path, names: &[std::ffi::OsString]) -> CopyEvidence {
    let has = |names: &[std::ffi::OsString], name: &str| names.iter().any(|n| n == name);
    let mut answer = CopyEvidence::Unpublished;
    if has(names, "state.toml") {
        match state_evidence(&dir.join("state.toml")) {
            CopyEvidence::Published => return CopyEvidence::Published,
            CopyEvidence::Unpublished => {}
            found => note(&mut answer, found),
        }
    }
    for trail in PUBLISH_TRAILS {
        let mut here = dir.to_path_buf();
        let mut listed = names.to_vec();
        let mut found = true;
        for part in trail {
            if !has(&listed, part) {
                found = false;
                break;
            }
            here.push(part);
            match local_listing(&here) {
                Ok(next) => listed = next,
                Err(evidence) => {
                    note(&mut answer, evidence);
                    found = false;
                    break;
                }
            }
        }
        if found && !listed.is_empty() {
            return CopyEvidence::Published;
        }
    }
    if has(names, "identity") {
        let identity = dir.join("identity");
        match local_listing(&identity) {
            Ok(inner) if has(&inner, "public.json") => match email_evidence(&identity.join("public.json")) {
                CopyEvidence::Published => return CopyEvidence::Published,
                CopyEvidence::Unpublished => {}
                found => note(&mut answer, found),
            },
            Ok(_) => {}
            Err(evidence) => note(&mut answer, evidence),
        }
    }
    answer
}

/// The bytes of a file a copy holds, or why there are none. Classified from
/// the one probe: a file that lands a moment later must not read as broken.
fn local_file(path: &Path) -> Result<Option<Vec<u8>>, CopyEvidence> {
    use crate::build::cloud_readiness::{probe_input, Found};
    match probe_input(path) {
        Ok(Found::Bytes(bytes)) => Ok(Some(bytes)),
        Ok(Found::InCloud) => Err(CopyEvidence::Arriving),
        Err(e) if crate::build::icloud::is_definitely_absent(path, &e) => Ok(None),
        Err(e) => Err(failed_read(&e)),
    }
}

fn state_evidence(state: &Path) -> CopyEvidence {
    let text = match local_file(state) {
        Ok(None) => return CopyEvidence::Unpublished,
        Ok(Some(bytes)) => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return CopyEvidence::Unreadable,
        },
        Err(evidence) => return evidence,
    };
    match crate::build::site_config::deployment_state_from_document(state, &text) {
        Ok(s) if s.site_id.is_some() || s.records_publish_evidence() => CopyEvidence::Published,
        Ok(_) => CopyEvidence::Unpublished,
        Err(_) => CopyEvidence::Unreadable,
    }
}

/// A signed-in email is a publish trace. A file that does not parse says
/// nothing either way, which is not the same as saying no.
fn email_evidence(public: &Path) -> CopyEvidence {
    let bytes = match local_file(public) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return CopyEvidence::Unpublished,
        Err(evidence) => return evidence,
    };
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(value) if value.get("email").and_then(|e| e.as_str()).is_some_and(|e| !e.is_empty()) => CopyEvidence::Published,
        Ok(_) => CopyEvidence::Unpublished,
        Err(_) => CopyEvidence::Unreadable,
    }
}

/// Label one found candidate. Fenced like the detector's only other read,
/// a set-aside's state (`moss_copy`):
/// any failure yields `published: None` ("unknown"), never an error — on an
/// evicted File Provider tree the bytes may be unavailable while the
/// directory metadata that triggered the match is local.
fn nested_root_info(dir: &Path, root: &Path, data_in: Option<&MossCopy>) -> NestedRootInfo {
    let rel = dir.strip_prefix(root).unwrap_or(dir);
    let (published, site_id) = match data_in {
        Some(copy) => match copy.evidence {
            CopyEvidence::Published => (Some(true), None),
            CopyEvidence::Unpublished => (Some(false), None),
            _ => (None, None),
        },
        None => read_publish_state(dir),
    };
    NestedRootInfo {
        rel_path: moss_core::slug::normalize_separators(&rel.to_string_lossy()),
        abs_path: dir.to_string_lossy().into_owned(),
        name: dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        published,
        site_id,
        data_in: data_in.map(|c| c.name.clone()),
    }
}

fn read_publish_state(dir: &Path) -> (Option<bool>, Option<String>) {
    publish_state(&dir.join(".moss").join("state.toml"), Some(false))
}

/// What a `state.toml` says: `(published, site_id)`. Any publish trace counts,
/// since a plugin publish records no `site_id`. A file still in the cloud,
/// unreadable, unparsable or written by a newer moss is `None`, never an
/// error that stops the scan; a provably absent one is `absent`. It never
/// waits on a download: the scan visits several folders before a build starts.
fn publish_state(state: &Path, absent: Option<bool>) -> (Option<bool>, Option<String>) {
    use crate::build::site_config::{deployment_state_from_document, read_managed_toml_no_wait};
    match read_managed_toml_no_wait(state) {
        Ok(None) => (absent, None),
        Ok(Some(text)) => match deployment_state_from_document(state, &text) {
            Ok(s) => (Some(s.site_id.is_some() || s.records_publish_evidence()), s.site_id),
            Err(_) => (None, None),
        },
        Err(_) => (None, None),
    }
}

#[cfg(test)]
#[path = "nested_roots_tests.rs"]
mod tests;
