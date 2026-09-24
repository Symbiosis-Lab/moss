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
    /// Telemetry: directories actually visited.
    pub dirs_visited: usize,
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

/// Does `dir` own a `.moss/` directory — i.e. is it a moss root? One `lstat`.
/// Also the subtree-boundary predicate for future scanner/watcher pruning.
pub fn owns_moss(dir: &Path) -> bool {
    dir.join(".moss").is_dir()
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
/// `.moss`, prunes below a match. The trigger is `child/.moss` `is_dir()` —
/// one `lstat`, no file read — so an evicted cloud tree cannot hang the walk;
/// only the ≤ `max_reported` FOUND candidates get their `state.toml` read,
/// and a failed read labels them `published: None` instead of erroring.
pub fn find_nested_roots(root: &Path, home: Option<&Path>, limits: &ScanLimits) -> NestedRootsReport {
    if owns_moss(root) {
        return NestedRootsReport {
            root: root.to_string_lossy().into_owned(),
            root_class: classify_root(root, home),
            nested: Vec::new(),
            truncated: false,
            dirs_visited: 0,
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
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        // A dir beyond the depth bound is never examined: its mere presence
        // means the scan is incomplete, which the decision layer must see.
        if depth > limits.max_depth {
            truncated = true;
            continue;
        }
        visited += 1;
        if visited > limits.max_dirs {
            truncated = true;
            break;
        }

        // The root itself is the caller's candidate, not a nested root.
        if depth > 0 && owns_moss(&dir) {
            if let Ok(rel) = dir.strip_prefix(root) {
                nested.push(nested_root_info(&dir, rel));
            }
            if nested.len() >= limits.max_reported {
                if !queue.is_empty() {
                    truncated = true;
                }
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
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else {
                // Same reasoning as an unlistable dir, one level down: a
                // dropped entry is unexplored, not absent.
                truncated = true;
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".git" || name == "node_modules" || name == ".moss" {
                continue;
            }
            // Do not follow symlinks; `file_type()` reports the link itself.
            match entry.file_type() {
                Ok(ft) if ft.is_dir() && !ft.is_symlink() => {
                    queue.push_back((entry.path(), depth + 1));
                }
                _ => {}
            }
        }
    }

    NestedRootsReport {
        root: root.to_string_lossy().into_owned(),
        root_class: classify_root(root, home),
        nested,
        truncated,
        dirs_visited: visited,
    }
}

/// Label one found candidate. The ONLY file read in the detector, and fenced:
/// any failure yields `published: None` ("unknown"), never an error — on an
/// evicted File Provider tree the bytes may be unavailable while the
/// directory metadata that triggered the match is local.
fn nested_root_info(dir: &Path, rel: &Path) -> NestedRootInfo {
    let (published, site_id) = read_publish_state(dir);
    NestedRootInfo {
        rel_path: moss_core::slug::normalize_separators(&rel.to_string_lossy()),
        abs_path: dir.to_string_lossy().into_owned(),
        name: dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        published,
        site_id,
    }
}

fn read_publish_state(dir: &Path) -> (Option<bool>, Option<String>) {
    let state_path = dir.join(".moss").join("state.toml");
    match std::fs::read_to_string(&state_path) {
        Ok(text) => match toml::from_str::<toml::Value>(&text) {
            Ok(value) => {
                let site_id = value
                    .get("deployment")
                    .and_then(|d| d.get("site_id"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                (Some(site_id.is_some()), site_id)
            }
            // Unparseable is indistinguishable from a half-synced write.
            Err(_) => (None, None),
        },
        // No state.toml at all: a previewed-but-never-deployed root.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Some(false), None),
        // Evicted / permission-denied: unknown, never a hang or an error.
        Err(_) => (None, None),
    }
}

#[cfg(test)]
#[path = "nested_roots_tests.rs"]
mod tests;
