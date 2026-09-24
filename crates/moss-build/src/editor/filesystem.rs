//! Filesystem helpers — directory listing, file tree, file/folder CRUD, and
//! collision-safe copies.
//!
//! These helpers back the `list_directory`, `list_tree`, `create_files`,
//! `create_folder`, `copy_files_to_project`, `get_file_info`,
//! and `resolve_url_for_file` Tauri commands in `commands.rs`.

// ── Public types ──────────────────────────────────────────────────────────

/// Metadata about a file for the editor's file viewer.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct EditorFileInfo {
    /// File size in bytes.
    pub size: u64,
    /// Last modified time as ISO 8601 string, if available.
    pub modified: Option<String>,
    /// Image width in pixels (images only).
    pub width: Option<u32>,
    /// Image height in pixels (images only).
    pub height: Option<u32>,
}

/// A directory entry returned by `list_directory`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DirEntry {
    /// File or folder name (not the full path).
    pub name: String,
    /// Absolute path to the entry.
    pub path: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// Last modified time as seconds since UNIX epoch. None if unavailable.
    pub modified: Option<f64>,
    /// Opaque cross-platform identity for this filesystem entry, serialized as
    /// decimal u64 (`Some("42")`) or `None` when not extractable. Used by
    /// EntryRegistry.reconcileSnapshot to reuse EntryIds when a rename moves
    /// an existing file_id to a new path.
    ///
    /// Unix: `Some(metadata.ino().to_string())` via MetadataExt.
    /// Windows (v1): always `None` — stable Rust doesn't expose file_index
    /// without nightly. Future PR can wire the `file-id` crate (already
    /// transitive via notify-debouncer-full) for proper Windows support.
    ///
    /// See: docs/archive/2026-05-26-treenode-inode.md
    pub file_id: Option<String>,
}

/// A recursive tree node for the full project file tree.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    /// Last modified time as seconds since UNIX epoch. None if unavailable.
    pub modified: Option<f64>,
    /// Resolved publish date as `YYYY-MM-DD`, or `None`.
    ///
    /// For markdown files: from frontmatter `date` field, filename `YYYY-MM-DD-…`
    /// prefix, or ctime fallback. For folders: max recency over descendants per
    /// the spec (see `compute_folder_recency`). Always `None` for non-md files.
    pub publish_date: Option<String>,
    /// Provenance of `publish_date`. Drives sort-zone classification:
    /// `Frontmatter` and `FilenamePrefix` are explicit (dated-md zone);
    /// `Ctime` is implicit (undated-md zone). For folders, always `None`.
    pub date_source: DateSource,
    /// Whether this node is its parent folder's ELECTED home file — the single
    /// winner of `moss_core::home::detect_home_file_in_folder_marked` (order:
    /// `home: true` frontmatter marker › index stems › self-named folder note ›
    /// first document alphabetically), computed per folder in
    /// `list_tree_inner_cached` with the folder's REAL basename (so the project
    /// root's self-named home is recognized too). At most one child per folder
    /// is `true`; always `false` for directories. The frontend consumes this
    /// flag (`entries.find(e => e.is_home)`) instead of re-deriving the
    /// election — the backend owns the home decision (consolidation-map
    /// homeRank row, docs/reference/target/).
    pub is_home: bool,
    /// Children of this directory. `None` for files.
    pub children: Option<Vec<TreeNode>>,
    pub hidden: u32,
    /// See `DirEntry::file_id`.
    pub file_id: Option<String>,
}

// Re-export so downstream modules and tests can reference DateSource via
// editor::filesystem::DateSource — single source of truth lives in moss-core.
pub use moss_core::date::DateSource;

/// Result of copying a single file into the project.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct CopiedFile {
    /// Original filename (basename of the source path).
    pub original_name: String,
    /// Final filename after any collision renaming (e.g. `photo-2.png`).
    pub final_name: String,
    /// Absolute path of the copied file.
    pub final_path: String,
    /// Path relative to the project root.
    pub relative_path: String,
}

/// Why a source file did not land in the vault.
///
/// The import loop used to `continue` past every failure and return only what
/// it managed to copy, so "copied nothing" and "copied everything" were the
/// same `Ok` value on the wire. Naming the reason is what lets the UI say the
/// one thing the user can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ImportSkipReason {
    /// The source is a cloud placeholder whose bytes have not arrived yet.
    /// moss has asked the provider for them, so retrying once the download
    /// finishes works. NOT a missing or broken file — see `build::icloud`'s
    /// "unreadable is not absent".
    StillInCloud,
    /// Anything waiting cannot fix: a genuinely absent source, a permissions
    /// problem, an unusable name, a rejected path.
    Failed,
}

/// One source the import did not land, and why.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct SkippedImport {
    /// Basename of the source that was skipped — the whole path when it has
    /// no usable basename, so the message can still name something.
    pub original_name: String,
    /// What the user can do about it.
    pub reason: ImportSkipReason,
}

/// What an into-vault copy actually did.
///
/// `skipped` is carried explicitly rather than left to be inferred from a
/// short `copied` list: "returned Ok with fewer files than asked for" is
/// exactly the shape the frontend read as total success.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct CopyReport {
    /// The files that are now in the vault.
    pub copied: Vec<CopiedFile>,
    /// The sources that are not, each with a reason.
    pub skipped: Vec<SkippedImport>,
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Extract an opaque cross-platform identity for a filesystem entry.
///
/// Returns `None` when identity cannot be determined (Windows on stable
/// Rust, filesystems that return 0 or error on inode lookup).
///
/// On Unix, returns `Some(metadata.ino().to_string())`. The u64-as-string
/// wire format avoids JavaScript's 53-bit number precision limit for very
/// large inodes (defensive against future filesystems).
///
/// Zed-inspired: Zed's `Entry.inode` collapses cross-platform to a single
/// u64 (see https://github.com/zed-industries/zed/blob/main/crates/worktree/src/worktree.rs#L3035).
/// moss returns `None` on Windows for v1 simplicity; tracked for future
/// `file-id` crate integration.
#[cfg(unix)]
fn extract_file_id(metadata: &std::fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.ino().to_string())
}

#[cfg(windows)]
fn extract_file_id(_metadata: &std::fs::Metadata) -> Option<String> {
    // Stable Rust does not expose Windows file_index without nightly
    // (rust-lang/rust#63010). Returning None means EntryRegistry's
    // byFileId is bypassed for this entry — same correctness as
    // path-only identity (today's behavior). When Windows file-id
    // support becomes urgent, switch to the `file-id` crate (already
    // transitive via notify-debouncer-full).
    None
}

/// Recursively visit every file under `dir` (internal entries excluded, same
/// as `list_directory_inner(show_internal = false)`), calling `visit` with the
/// entry and its project-relative `/`-separated path. An unreadable directory
/// is logged and skipped — the callers (wikilink completion, term resolution)
/// degrade rather than error.
pub fn walk_source_files(dir: &str, project_path: &str, visit: &mut dyn FnMut(&DirEntry, &str)) {
    let entries = match list_directory_inner(dir, project_path, false) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("source walk: skipping unreadable dir '{dir}': {e}");
            return;
        }
    };
    for entry in entries {
        if entry.is_dir {
            walk_source_files(&entry.path, project_path, visit);
            continue;
        }
        let rel = std::path::Path::new(&entry.path)
            .strip_prefix(project_path)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| entry.name.clone());
        visit(&entry, &rel);
    }
}

/// Inner implementation of `list_directory` for testability (no Tauri State dependency).
pub fn list_directory_inner(
    path: &str,
    project_path: &str,
    show_internal: bool,
) -> Result<Vec<DirEntry>, String> {
    Ok(list_directory_counted(path, project_path, show_internal)?.0)
}

fn list_directory_counted(path: &str, project_path: &str, show_internal: bool) -> Result<(Vec<DirEntry>, u32), String> {
    let dir = std::path::Path::new(path);
    if !dir.is_dir() { return Err(format!("'{}' is not a directory", path)); }

    // Compute this directory's path relative to the project root ("" for root,
    // ".moss" when listing .moss/ itself, ".moss/theme" when listing
    // .moss/theme/, etc.).
    let parent_relative = std::path::Path::new(path)
        .strip_prefix(project_path)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();

    let read_dir = std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory '{}': {}", path, e))?;

    let mut entries: Vec<DirEntry> = Vec::new();
    let mut hidden: u32 = 0;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("Error reading entry: {}", e))?;
        let name = entry.file_name().to_string_lossy().to_string();

        if let Some(reason) = crate::build::scan::classify::is_hidden_reason(&name, &parent_relative, show_internal) {
            hidden += reason.is_curated() as u32;
            continue;
        }

        let metadata = entry
            .metadata()
            .map_err(|e| format!("Failed to read metadata for '{}': {}", name, e))?;

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64());

        let file_id = extract_file_id(&metadata);

        entries.push(DirEntry {
            name,
            path: entry.path().to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            modified,
            file_id,
        });
    }

    entries.sort_by(cmp_dir_entry);

    Ok((entries, hidden))
}

/// Per-project publish-date cache passed to `list_tree_inner_cached`.
///
/// Keyed by `(file_path, mtime_secs)` so a stale entry never gets served.
/// The outer container is owned by `AppState`; this is a borrowed view.
///
/// The cached tuple is `(publish_date, date_source, home_marker)` — the third
/// element records whether the file's frontmatter set `home: true`, extracted
/// from the SAME 4 KB frontmatter read that resolves the date (zero extra
/// I/O), and feeds the per-folder home election in `list_tree_inner_cached`.
pub type PublishDateCacheView<'a> = std::sync::Mutex<
    &'a mut std::collections::HashMap<
        (std::path::PathBuf, u64),
        (Option<String>, DateSource, bool),
    >,
>;

/// Inner implementation of `list_tree` for testability.
///
/// `project_path` is the project root, used by `is_hidden` to compute the
/// relative path when filtering `.moss/` children.
///
/// This is a thin shim that calls `list_tree_inner_cached` with no cache
/// (every md file is parsed fresh). The Tauri command path passes a real
/// cache; tests use this shim.
#[cfg_attr(not(test), allow(dead_code))]
pub fn list_tree_inner(
    path: &str,
    project_path: &str,
    show_internal: bool,
) -> Result<TreeNode, String> {
    let mut empty = std::collections::HashMap::new();
    let cache = std::sync::Mutex::new(&mut empty);
    list_tree_inner_cached(path, project_path, show_internal, &cache)
}

/// Cached variant — used by the Tauri command, where `AppState` owns the cache.
///
/// Reads each `.md` file's first 4 KB once per (path, mtime) pair, caches
/// the resolved publish date, and feeds the result into the five-zone sort.
pub fn list_tree_inner_cached(
    path: &str,
    project_path: &str,
    show_internal: bool,
    cache: &PublishDateCacheView<'_>,
) -> Result<TreeNode, String> {
    let (entries, hidden) = list_directory_counted(path, project_path, show_internal)?;

    // Basenames of md children whose frontmatter carries the `home: true`
    // marker — collected from the same cached frontmatter read that resolves
    // publish dates, and fed to the home election below.
    let mut marked: Vec<String> = Vec::new();

    let mut children: Vec<TreeNode> = entries
        .into_iter()
        .map(|entry| {
            if entry.is_dir {
                list_tree_inner_cached(&entry.path, project_path, show_internal, cache)
            } else {
                let (publish_date, date_source) = if entry.name.to_ascii_lowercase().ends_with(".md") {
                    let (date, source, home_marker) = resolve_md_date(&entry, cache);
                    if home_marker {
                        marked.push(entry.name.clone());
                    }
                    (date, source)
                } else {
                    (None, DateSource::None)
                };
                Ok(TreeNode {
                    name: entry.name,
                    path: entry.path,
                    is_dir: false,
                    modified: entry.modified,
                    publish_date,
                    date_source,
                    is_home: false,
                    children: None,
                    hidden: 0,
                    file_id: entry.file_id,
                })
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Compute the max mtime across all children (recursive — children already
    // have their own `modified` computed).
    let max_modified = children
        .iter()
        .filter_map(|c| c.modified)
        .fold(None::<f64>, |acc, m| Some(acc.map_or(m, |a: f64| a.max(m))));

    // Who is this folder, per the OWNER of vault-root identity? Both questions
    // the home election below asks — "is THIS folder the project root" and
    // "what is the root called" — come from `vault_root::VaultRoot`, never
    // re-derived from the path string with a second `Path::file_name()` /
    // `trim_end_matches`. That re-derivation returns `None` / `""` for `.`, a
    // trailing slash and `/`, silently demoting a self-named root home
    // (`潮汐/潮汐.md`) off `/` — the `moss build .` bug one code path over.
    // `VaultRoot::resolve` collapses all three spellings the way the build
    // does, so the tree's `is_home` flag can never disagree with which file the
    // built site serves at the folder's URL. A SUBFOLDER keeps its plain
    // basename — a per-folder name read off a real absolute child path, not a
    // root identity.
    let project_root = crate::vault_root::VaultRoot::resolve(project_path);
    let is_root = crate::vault_root::VaultRoot::resolve(path) == project_root;
    let name = if is_root {
        project_root.name().to_string()
    } else {
        std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    };

    // Elect this folder's single home child — marker › index › self-named ›
    // first-doc-alphabetically, the SAME election the build runs
    // (`detect_home_file_in_folder_marked`), so the tree's flag can never
    // disagree with which file the built site serves at this folder's URL.
    // `name` is the folder's name from the owner above — the root's blessed
    // `VaultRoot::name`, a subfolder's basename — which the self-named rule
    // needs. Exactly one child gets the flag (basenames are unique within a
    // directory); folders are never home.
    let elected_home: Option<String> = {
        let file_names: Vec<&str> = children
            .iter()
            .filter(|c| !c.is_dir)
            .map(|c| c.name.as_str())
            .collect();
        let marked_refs: Vec<&str> = marked.iter().map(|s| s.as_str()).collect();
        let candidate =
            moss_core::home::detect_home_file_in_folder_marked(&file_names, &name, &marked_refs);
        // PROJECT ROOT ONLY: gate out the priority-5 alphabetical fallback,
        // mirroring `detect_root_home_source` (commands.rs, 014638c66) — the
        // root editor surface deliberately keeps its "create home page" CTA
        // rather than silently adopting a random article as the SITE home, so
        // the tree flag (which gates the "Create home file" context item)
        // must agree or the two in-app surfaces contradict each other on the
        // root. Subfolders keep the full election including the fallback
        // (build parity). The built site may still serve a fallback home at
        // `/` — that residual editor-vs-build divergence is the standing
        // 2026-06-22 decision, out of scope here.
        candidate
            .filter(|c| {
                if !is_root {
                    return true;
                }
                let stem = std::path::Path::new(c)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(c);
                marked_refs.iter().any(|m| m.eq_ignore_ascii_case(c))
                    || moss_core::home::is_home_file(stem, &name)
            })
            .map(|s| s.to_string())
    };
    if let Some(home_name) = elected_home {
        for c in children.iter_mut() {
            if !c.is_dir && c.name == home_name {
                c.is_home = true;
            }
        }
    }

    // Extract this directory's own file_id so it can be tracked in the
    // registry alongside file entries. Errors degrade to None (no identity
    // reuse for this directory).
    let dir_file_id = std::fs::metadata(path)
        .ok()
        .and_then(|m| extract_file_id(&m));

    let mut folder_node = TreeNode {
        name,
        path: path.to_string(),
        is_dir: true,
        modified: max_modified,
        publish_date: None,
        date_source: DateSource::None,
        is_home: false,
        children: Some(children),
        hidden,
        file_id: dir_file_id,
    };
    folder_node.publish_date = compute_folder_recency(&folder_node);
    sort_tree_nodes(
        folder_node.children.as_mut().unwrap(),
        &folder_node.name,
    );
    Ok(folder_node)
}

/// Resolve an md file's publish date AND its `home: true` frontmatter marker
/// from one cached 4 KB frontmatter read. Returns
/// `(publish_date, date_source, home_marker)`.
///
/// Both are read via `read_frontmatter_only` → `frontmatter_map`, which covers
/// both frontmatter dialects — so the tree flag now agrees with the build's
/// `file_has_home_marker` instead of missing simplified frontmatter (moss#937).
fn resolve_md_date(
    entry: &DirEntry,
    cache: &PublishDateCacheView<'_>,
) -> (Option<String>, DateSource, bool) {
    let path = std::path::PathBuf::from(&entry.path);
    let mtime_secs = entry.modified.map(|m| m as u64).unwrap_or(0);
    let key = (path.clone(), mtime_secs);

    {
        let guard = cache.lock().expect("publish-date cache poisoned");
        if let Some(cached) = guard.get(&key) {
            return cached.clone();
        }
    }

    // An offline file answers "no frontmatter", and that answer must not be
    // cached. Materialization on Sonoma+ is a staging-file swap that preserves
    // both size and mtime, so the arrival leaves `key` identical — the page
    // would stay dateless, and a `home: true` page stay un-flagged, for the
    // life of the process. Answer for this call, remember nothing.
    //
    // The stat comes first for the same reason as elsewhere: without the
    // fail-fast policy the read below blocks rather than erroring, and this
    // runs once per markdown file in the tree.
    //
    // Deliberately does NOT ask for the download. The supervisor owns asking:
    // it requests in priority order (home page first), caps each tick, and
    // backs off per file. This walk visits every markdown file in the vault, so
    // asking here would fire one request per evicted file per refresh — the
    // exact flood the cap exists to prevent, and unordered besides. The
    // supervisor sweeps the same vault; these files are already its business.
    let (frontmatter, cacheable) = if crate::build::icloud::is_evicted(&path) {
        (Default::default(), false)
    } else {
        match read_frontmatter_only(&path) {
            Ok(fm) => (fm, true),
            // The stat misses pre-Sonoma `.name.icloud` stubs and an eviction
            // landing between the two calls.
            Err(e) if crate::build::icloud::is_offline_not_absent(&path, &e) => {
                (Default::default(), false)
            }
            // A deleted or unreadable file is not coming back on its own; the
            // fallback answer is the right one and worth caching.
            Err(_) => (Default::default(), true),
        }
    };
    let home_marker = frontmatter
        .get("home")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let ctime_date = entry.modified.and_then(mtime_to_yyyy_mm_dd);
    let (date, source) = moss_core::date::resolve_publish_date(
        &frontmatter,
        &[&entry.name],
        ctime_date.as_deref(),
    );
    let resolved = (date, source, home_marker);

    if cacheable {
        let mut guard = cache.lock().expect("publish-date cache poisoned");
        guard.insert(key, resolved.clone());
    }

    resolved
}

/// Convert a UNIX-epoch-seconds mtime to a `YYYY-MM-DD` string in UTC.
fn mtime_to_yyyy_mm_dd(secs: f64) -> Option<String> {
    use std::time::{Duration, UNIX_EPOCH};
    let st = UNIX_EPOCH.checked_add(Duration::from_secs(secs as u64))?;
    let dt = chrono::DateTime::<chrono::Utc>::from(st);
    Some(dt.format("%Y-%m-%d").to_string())
}

/// Given a desired destination path, append `-2`, `-3`, ... before the
/// extension until a non-existing path is found. Returns the original path
/// unchanged if there is no collision.
///
/// ## Suffix format (unified in Task 5.1)
///
/// Uses **dash** (`-2`, `-3`, …) starting at 2 — consistent with
/// `copy_files_into` in `copy_in.rs` and `move_one` in `batch.rs`.
/// The previous underscore scheme (`_1`, `_2`) was retired to eliminate the
/// divergence between the drag-import path (dash) and the copy/paste path
/// (was underscore). Callers that documented `_1`-style names in comments or
/// tests should be updated accordingly.
pub fn resolve_collision(path: &std::path::Path) -> std::path::PathBuf {
    // symlink_metadata, not exists(): exists() follows symlinks, so a
    // DANGLING symlink at the destination reads as absent and fs::copy would
    // then write THROUGH it — outside the vault if it points there (the
    // moss#997 shape, one component deeper). Any pre-existing entry,
    // including a dangling symlink, gets collision-suffixed instead.
    if path.symlink_metadata().is_err() {
        return path.to_path_buf();
    }

    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let ext = path.extension().map(|e| e.to_string_lossy().to_string());
    let parent = path.parent().unwrap_or(path);

    for i in 2u64.. {
        let new_name = match &ext {
            Some(e) => format!("{}-{}.{}", stem, i, e),
            None => format!("{}-{}", stem, i),
        };
        let candidate = parent.join(&new_name);
        if candidate.symlink_metadata().is_err() {
            return candidate;
        }
    }

    unreachable!()
}

// ── Sort helpers ──────────────────────────────────────────────────────────

/// Sort comparator: directories first, then by mtime descending (newest
/// first), with case-insensitive alphabetical as tiebreaker.
pub fn cmp_dir_entry(a: &DirEntry, b: &DirEntry) -> std::cmp::Ordering {
    cmp_entry_fields(a.is_dir, a.modified, &a.name, b.is_dir, b.modified, &b.name)
}

/// Shared sort logic: directories first, then mtime descending, then
/// alphabetical.
pub fn cmp_entry_fields(
    a_is_dir: bool,
    a_modified: Option<f64>,
    a_name: &str,
    b_is_dir: bool,
    b_modified: Option<f64>,
    b_name: &str,
) -> std::cmp::Ordering {
    match (a_is_dir, b_is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => {
            let time_ord = match (b_modified, a_modified) {
                (Some(bm), Some(am)) => bm.partial_cmp(&am).unwrap_or(std::cmp::Ordering::Equal),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            };
            time_ord.then_with(|| a_name.to_lowercase().cmp(&b_name.to_lowercase()))
        }
    }
}

// ── Five-zone tree sort (publish-date aware) ──────────────────────────────

/// Sort children of a folder per the spec's five-zone ordering.
///
/// Zone order: elected-home → home-md → dated-md → undated-md → folders → non-md.
/// Within each zone: recency desc → mtime desc → alpha case-insensitive.
///
/// `parent_folder_name` is the containing folder's `file_name`, used by
/// `moss_core::home::is_home_file` for the self-named index convention.
///
/// ## Sort/flag agreement
///
/// The ELECTED home (`TreeNode::is_home`, set by `list_tree_inner_cached`
/// before sorting) is pinned FIRST, ahead of `Zone::HomeMd`. `zone_of`
/// classifies by filename shape alone, so it can put SEVERAL files in the
/// home zone (index.md + readme.md) — and would let a newer readme.md
/// out-sort the elected index.md — or MISS the election winner entirely
/// (a `home: true` marker file or the alphabetical-fallback home has an
/// ordinary filename). Pinning the single winner keeps the flag and the
/// frontend's home-pair row (folder + home collapsed onto one row, built
/// from the flagged entry) in agreement.
pub fn sort_tree_nodes(nodes: &mut [TreeNode], parent_folder_name: &str) {
    nodes.sort_by(|a, b| {
        match b.is_home.cmp(&a.is_home) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        let za = zone_of(a, parent_folder_name);
        let zb = zone_of(b, parent_folder_name);
        if za != zb {
            return za.cmp(&zb);
        }
        // Recency: empty string sorts last, otherwise lexicographic-as-date.
        let ra = recency_key(a);
        let rb = recency_key(b);
        match (ra.is_empty(), rb.is_empty()) {
            (false, false) => match rb.cmp(&ra) {
                std::cmp::Ordering::Equal => {}
                ord => return ord,
            },
            (false, true) => return std::cmp::Ordering::Less,
            (true, false) => return std::cmp::Ordering::Greater,
            (true, true) => {}
        }
        // mtime tiebreak — desc.
        match b.modified.partial_cmp(&a.modified).unwrap_or(std::cmp::Ordering::Equal) {
            std::cmp::Ordering::Equal => {}
            ord => return ord,
        }
        a.name.to_lowercase().cmp(&b.name.to_lowercase())
    });
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Zone {
    HomeMd = 0,
    DatedMd = 1,
    UndatedMd = 2,
    Folder = 3,
    NonMd = 4,
}

fn zone_of(node: &TreeNode, parent_folder_name: &str) -> Zone {
    if node.is_dir {
        return Zone::Folder;
    }
    let lower = node.name.to_ascii_lowercase();
    if !lower.ends_with(".md") {
        return Zone::NonMd;
    }
    let stem = node
        .name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(&node.name);
    if moss_core::home::is_home_file(stem, parent_folder_name) {
        return Zone::HomeMd;
    }
    match node.date_source {
        DateSource::Frontmatter | DateSource::FilenamePrefix => Zone::DatedMd,
        DateSource::Ctime | DateSource::None => Zone::UndatedMd,
    }
}

fn recency_key(node: &TreeNode) -> String {
    node.publish_date.clone().unwrap_or_default()
}

// ── Folder recency ────────────────────────────────────────────────────────

/// Compute a folder's recency string per spec Zone 4 rules.
///
/// Precedence:
///   1. Max explicit publish_date over md descendants (Frontmatter/FilenamePrefix).
///   2. Else max mtime over md descendants (as `YYYY-MM-DD`).
///   3. Else max mtime over non-md descendants.
///   4. Else folder's own mtime.
///
/// Returning `None` only if the folder has no descendants and no own mtime.
pub fn compute_folder_recency(folder: &TreeNode) -> Option<String> {
    let children = folder.children.as_deref().unwrap_or(&[]);

    let md_descendants = collect_descendants(children, &is_md_file);

    let max_explicit = md_descendants
        .iter()
        .filter(|n| matches!(n.date_source, DateSource::Frontmatter | DateSource::FilenamePrefix))
        .filter_map(|n| n.publish_date.clone())
        .max();
    if max_explicit.is_some() {
        return max_explicit;
    }

    let max_md_mtime = md_descendants
        .iter()
        .filter_map(|n| n.modified)
        .fold(None::<f64>, |acc, m| Some(acc.map_or(m, |a| a.max(m))));
    if let Some(m) = max_md_mtime {
        return mtime_to_yyyy_mm_dd(m);
    }

    let max_any_mtime = collect_descendants(children, &|_| true)
        .iter()
        .filter_map(|n| n.modified)
        .fold(None::<f64>, |acc, m| Some(acc.map_or(m, |a| a.max(m))));
    if let Some(m) = max_any_mtime {
        return mtime_to_yyyy_mm_dd(m);
    }

    folder.modified.and_then(mtime_to_yyyy_mm_dd)
}

/// Recursively collect every leaf descendant matching `pred`.
///
/// Walks into directory children but only `pred(node)` decides whether
/// a non-directory node is included. Returns a flat `Vec` of references
/// into the original tree.
fn collect_descendants<'a>(
    nodes: &'a [TreeNode],
    pred: &impl Fn(&TreeNode) -> bool,
) -> Vec<&'a TreeNode> {
    let mut out = Vec::new();
    for n in nodes {
        if !n.is_dir {
            if pred(n) {
                out.push(n);
            }
        } else if let Some(kids) = n.children.as_deref() {
            out.extend(collect_descendants(kids, pred));
        }
    }
    out
}

fn is_md_file(node: &TreeNode) -> bool {
    !node.is_dir && node.name.to_ascii_lowercase().ends_with(".md")
}

// ── Frontmatter helper for tree construction ──────────────────────────────

/// Read the first 4 KB of a file and parse just the frontmatter.
///
/// Returns an empty `HashMap` when the file has no frontmatter or the
/// `---` block isn't fully contained in the first 4 KB. Returns `Err`
/// only on I/O errors (permission, deleted mid-scan, etc.).
///
/// We only need the frontmatter for tree-construction sort decisions, so
/// reading the whole file would be wasteful. The body is parsed and
/// immediately discarded; for typical projects (~50 .md files × 4 KB)
/// this is well under one frame.
pub fn read_frontmatter_only(
    path: &std::path::Path,
) -> std::io::Result<std::collections::HashMap<String, serde_yaml::Value>> {
    use std::io::Read;
    // 4 KB covers ~99% of real-world frontmatter blocks. If a file's
    // frontmatter is larger, parsing will fail (closing `---` not in the
    // window) and the caller falls through to filename-prefix → ctime.
    const MAX_FRONTMATTER_BYTES: usize = 4096;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; MAX_FRONTMATTER_BYTES];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    let text = String::from_utf8_lossy(&buf);
    // `frontmatter_map`, not `parse`: the file tree needs the fields, and a
    // simplified-frontmatter page used to come back empty here — no date in the
    // tree, and its `home: true` never flagged (moss#937).
    Ok(moss_core::frontmatter::frontmatter_map(&text))
}

// ── Publish-date resolution ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- read_frontmatter_only ---

    #[test]
    fn read_frontmatter_finds_date() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.md");
        std::fs::write(&p, "---\ndate: 2025-11-15\ntitle: Hello\n---\nbody text").unwrap();
        let fm = read_frontmatter_only(&p).unwrap();
        assert_eq!(fm.get("date").and_then(|v| v.as_str()), Some("2025-11-15"));
    }

    #[test]
    fn read_frontmatter_handles_no_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.md");
        std::fs::write(&p, "no frontmatter here").unwrap();
        let fm = read_frontmatter_only(&p).unwrap();
        assert!(fm.is_empty());
    }

    // --- five-zone sort + folder recency ---

    fn node_md(name: &str, date: Option<&str>, source: DateSource, mtime: f64) -> TreeNode {
        TreeNode {
            name: name.to_string(),
            path: name.to_string(),
            is_dir: false,
            modified: Some(mtime),
            publish_date: date.map(|s| s.to_string()),
            date_source: source,
            is_home: false,
            children: None,
            hidden: 0,
            file_id: None,
        }
    }

    fn node_dir(name: &str, mtime: f64, publish_date: Option<&str>) -> TreeNode {
        TreeNode {
            name: name.to_string(),
            path: name.to_string(),
            is_dir: true,
            modified: Some(mtime),
            publish_date: publish_date.map(|s| s.to_string()),
            date_source: DateSource::None,
            is_home: false,
            children: Some(vec![]),
            hidden: 0,
            file_id: None,
        }
    }

    fn node_file(name: &str, mtime: f64) -> TreeNode {
        TreeNode {
            name: name.to_string(),
            path: name.to_string(),
            is_dir: false,
            modified: Some(mtime),
            publish_date: None,
            date_source: DateSource::None,
            is_home: false,
            children: None,
            hidden: 0,
            file_id: None,
        }
    }

    #[test]
    fn five_zone_sort_groups_in_order() {
        let mut nodes = vec![
            node_file("image.png", 100.0),
            node_dir("News", 50.0, Some("2025-11-15")),
            node_md("undated.md", Some("2024-01-01"), DateSource::Ctime, 90.0),
            node_md("dated.md", Some("2025-03-01"), DateSource::Frontmatter, 80.0),
            node_md("index.md", None, DateSource::None, 10.0),
        ];
        sort_tree_nodes(&mut nodes, "Project");
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["index.md", "dated.md", "undated.md", "News", "image.png"]
        );
    }

    #[test]
    fn dated_md_sorts_newest_first_with_mtime_tiebreak() {
        let mut nodes = vec![
            node_md("apple.md", Some("2025-11-15"), DateSource::Frontmatter, 100.0),
            node_md("zebra.md", Some("2025-11-15"), DateSource::Frontmatter, 200.0),
        ];
        sort_tree_nodes(&mut nodes, "Project");
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["zebra.md", "apple.md"]);
    }

    #[test]
    fn undated_md_sorts_by_mtime() {
        let mut nodes = vec![
            node_md("a.md", Some("2024-01-01"), DateSource::Ctime, 100.0),
            node_md("b.md", Some("2025-01-01"), DateSource::Ctime, 200.0),
        ];
        sort_tree_nodes(&mut nodes, "Project");
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["b.md", "a.md"]);
    }

    #[test]
    fn folder_recency_prefers_md_descendant_dates() {
        let folder = TreeNode {
            name: "News".to_string(),
            path: "News".to_string(),
            is_dir: true,
            modified: Some(50.0),
            publish_date: None,
            date_source: DateSource::None,
            is_home: false,
            children: Some(vec![
                node_md("article.md", Some("2025-11-15"), DateSource::Frontmatter, 80.0),
                node_file("photo.png", 999_999_999.0), // very recent image, must NOT win
            ]),
            hidden: 0,
            file_id: None,
        };
        assert_eq!(
            compute_folder_recency(&folder),
            Some("2025-11-15".to_string())
        );
    }

    #[test]
    fn folder_recency_falls_back_to_md_mtime_when_no_dates() {
        let folder = TreeNode {
            name: "Pages".to_string(),
            path: "Pages".to_string(),
            is_dir: true,
            modified: Some(50.0),
            publish_date: None,
            date_source: DateSource::None,
            is_home: false,
            children: Some(vec![
                node_md("a.md", None, DateSource::None, 100.0),
                node_md("b.md", None, DateSource::None, 200.0),
            ]),
            hidden: 0,
            file_id: None,
        };
        assert!(compute_folder_recency(&folder).is_some());
    }

    #[test]
    fn folder_recency_falls_back_to_non_md_mtime_when_no_md() {
        let folder = TreeNode {
            name: "assets".to_string(),
            path: "assets".to_string(),
            is_dir: true,
            modified: Some(50.0),
            publish_date: None,
            date_source: DateSource::None,
            is_home: false,
            children: Some(vec![node_file("photo.png", 200.0)]),
            hidden: 0,
            file_id: None,
        };
        assert!(compute_folder_recency(&folder).is_some());
    }

    #[test]
    fn folder_recency_uses_folder_mtime_when_empty() {
        let folder = node_dir("Empty", 100.0, None);
        assert!(compute_folder_recency(&folder).is_some());
    }

    // --- list_tree_inner integration ---

    #[test]
    fn list_tree_counts_hidden_moss_internal_entries() {
        // `.moss/` holds only non-allowlisted entries: `agents/SKILL.md` and
        // `state.toml`, no `config.toml`, no `theme/`. The listing of `.moss/`
        // itself sees two entries (`agents`, `state.toml`), both filtered by
        // `HiddenReason::MossInternal` — `children` reads empty, same as
        // before this change, but `hidden` must now say why.
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path();
        let moss_dir = proj.join(".moss");
        let agents_dir = moss_dir.join("agents");
        std::fs::create_dir_all(&agents_dir).unwrap();
        std::fs::write(agents_dir.join("SKILL.md"), "# agent skill").unwrap();
        std::fs::write(moss_dir.join("state.toml"), "").unwrap();

        let tree = list_tree_inner(proj.to_str().unwrap(), proj.to_str().unwrap(), true).unwrap();
        let moss_node = tree
            .children
            .as_ref()
            .unwrap()
            .iter()
            .find(|c| c.name == ".moss")
            .expect(".moss must be listed when show_internal is on");

        assert_eq!(moss_node.children.as_ref().unwrap().len(), 0);
        assert_eq!(moss_node.hidden, 2);
    }

    #[test]
    fn list_tree_does_not_count_os_junk_as_hidden() {
        // A folder holding nothing but a `.DS_Store` — the almost-universal
        // macOS case — must still read as genuinely empty: `hidden == 0`,
        // not "moss filtered something here."
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path();
        std::fs::write(proj.join(".DS_Store"), b"junk").unwrap();

        let tree = list_tree_inner(proj.to_str().unwrap(), proj.to_str().unwrap(), false).unwrap();

        assert_eq!(tree.children.as_ref().unwrap().len(), 0);
        assert_eq!(tree.hidden, 0);
    }

    #[test]
    fn list_tree_populates_publish_date_for_md() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path();
        std::fs::write(
            proj.join("post.md"),
            "---\ndate: 2025-11-15\n---\nbody",
        )
        .unwrap();
        std::fs::write(proj.join("undated.md"), "no frontmatter").unwrap();
        std::fs::write(proj.join("image.png"), b"png").unwrap();

        let tree = list_tree_inner(
            proj.to_str().unwrap(),
            proj.to_str().unwrap(),
            false,
        )
        .unwrap();

        let kids = tree.children.unwrap();
        let post = kids.iter().find(|c| c.name == "post.md").unwrap();
        let undated = kids.iter().find(|c| c.name == "undated.md").unwrap();
        let image = kids.iter().find(|c| c.name == "image.png").unwrap();

        assert_eq!(post.publish_date, Some("2025-11-15".to_string()));
        assert_eq!(post.date_source, DateSource::Frontmatter);

        assert!(undated.publish_date.is_some());
        assert_eq!(undated.date_source, DateSource::Ctime);

        assert_eq!(image.publish_date, None);
        assert_eq!(image.date_source, DateSource::None);
    }

    #[test]
    fn list_tree_caches_publish_dates_across_calls() {
        // Asserts the (path, mtime) cache actually serves repeat calls
        // without re-reading file content. We probe this by populating
        // the cache via a real list_tree call, then deleting the file
        // and verifying a SECOND call still returns the cached date.
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path();
        let post_path = proj.join("post.md");
        std::fs::write(&post_path, "---\ndate: 2025-11-15\n---\nbody").unwrap();

        // Same cache view across both calls — same project root.
        let mut entries: std::collections::HashMap<
            (std::path::PathBuf, u64),
            (Option<String>, DateSource, bool),
        > = std::collections::HashMap::new();

        // First call: parses file, populates cache.
        {
            let cache = std::sync::Mutex::new(&mut entries);
            let tree = list_tree_inner_cached(
                proj.to_str().unwrap(),
                proj.to_str().unwrap(),
                false,
                &cache,
            )
            .unwrap();
            let post = tree
                .children
                .unwrap()
                .into_iter()
                .find(|c| c.name == "post.md")
                .unwrap();
            assert_eq!(post.publish_date, Some("2025-11-15".to_string()));
        }
        assert_eq!(entries.len(), 1, "cache should hold one entry after first call");

        // Delete the file. If the second call re-read content it would
        // get a default frontmatter and fall through to ctime — but
        // because the (path, mtime) key still hits the cache, we get
        // the original frontmatter date back.
        //
        // We can't trivially remove the entry without changing mtime,
        // so we instead check by replacing the file with a different
        // body that would resolve to a different date if reparsed.
        // The same mtime would mean a cache hit; touching the file
        // would change mtime and miss the cache. We simulate "no read"
        // by using DirEntry directly.
        let modified_secs = std::fs::metadata(&post_path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let key = (post_path.clone(), modified_secs);
        assert!(
            entries.contains_key(&key),
            "cache should be keyed by (absolute path, mtime_secs)"
        );

        // Second call: cache should serve without touching the file. Truncate
        // the file to empty content; if we re-read, frontmatter parse fails
        // and date_source becomes Ctime. With the cache hit, date stays
        // Frontmatter/2025-11-15.
        std::fs::write(&post_path, "different body, no frontmatter").unwrap();
        // Restore mtime to match the cache key (atomic-rename simulator).
        let mtime = filetime::FileTime::from_unix_time(modified_secs as i64, 0);
        filetime::set_file_mtime(&post_path, mtime).unwrap();

        {
            let cache = std::sync::Mutex::new(&mut entries);
            let tree = list_tree_inner_cached(
                proj.to_str().unwrap(),
                proj.to_str().unwrap(),
                false,
                &cache,
            )
            .unwrap();
            let post = tree
                .children
                .unwrap()
                .into_iter()
                .find(|c| c.name == "post.md")
                .unwrap();
            assert_eq!(
                post.publish_date,
                Some("2025-11-15".to_string()),
                "cache hit should serve original date even though file content changed"
            );
            assert_eq!(post.date_source, DateSource::Frontmatter);
        }
    }

    // --- is_home election (backend emits the home decision) ---

    /// Collect (name, is_home) over a folder's children for assertions.
    fn home_flags(node: &TreeNode) -> Vec<(String, bool)> {
        node.children
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|c| (c.name.clone(), c.is_home))
            .collect()
    }

    fn list_named_project(proj: &std::path::Path) -> TreeNode {
        list_tree_inner(proj.to_str().unwrap(), proj.to_str().unwrap(), false).unwrap()
    }

    #[test]
    fn is_home_flags_self_named_root_home() {
        // The root-name parity case: the PROJECT ROOT's own basename elects a
        // self-named note (`Garden Path/Garden Path.md`), because the
        // election receives the REAL basename, not ''.
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("Garden Path");
        std::fs::create_dir(&proj).unwrap();
        std::fs::write(proj.join("Garden Path.md"), "# home").unwrap();
        std::fs::write(proj.join("poems.md"), "# poems").unwrap();

        let tree = list_named_project(&proj);
        let flags = home_flags(&tree);
        assert!(flags.contains(&("Garden Path.md".to_string(), true)));
        assert!(flags.contains(&("poems.md".to_string(), false)));
    }

    #[test]
    fn is_home_self_named_root_home_is_spelling_invariant() {
        // The self-named root home (`潮汐/潮汐.md`) is elected whether the
        // project root arrives as an ABSOLUTE path, with a TRAILING SLASH, or as
        // `.` — the root name now comes from `VaultRoot::resolve`, not
        // `Path::file_name()`. `Path::file_name()` is `None` for `.`, which left
        // the election with an empty root name; the self-named rule could not
        // fire, and the root's gated-out alphabetical fallback then elected NO
        // home at all (`潮汐.md` silently demoted off `/`).
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("\u{5728}\u{5834}");
        std::fs::create_dir(&proj).unwrap();
        std::fs::write(proj.join("\u{5728}\u{5834}.md"), "# home").unwrap();
        std::fs::write(proj.join("note.md"), "# note").unwrap();
        let home = ("\u{5728}\u{5834}.md".to_string(), true);

        // Absolute and trailing-slash spellings need no working-directory move.
        let abs = proj.to_string_lossy().to_string();
        let trailing = format!("{abs}/");
        for spelling in [abs.as_str(), trailing.as_str()] {
            let tree = list_tree_inner(spelling, spelling, false).unwrap();
            assert!(
                home_flags(&tree).contains(&home),
                "self-named root home must be elected for root spelled {spelling:?}"
            );
        }

        // `.` is inherently CWD-relative. Serialize the process-global chdir and
        // restore the working directory BEFORE asserting, so a panic can't leak
        // it into a parallel test. The rest of the lib suite resolves only
        // absolute paths, so this brief window races nothing.
        static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&proj).unwrap();
        let dot_tree = list_tree_inner(".", ".", false);
        std::env::set_current_dir(&prev).unwrap();
        assert!(
            home_flags(&dot_tree.unwrap()).contains(&home),
            "self-named root home must be elected when the root is given as `.`"
        );
    }

    #[test]
    fn is_home_flags_index_md() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("blog");
        std::fs::create_dir(&proj).unwrap();
        std::fs::write(proj.join("index.md"), "# home").unwrap();
        std::fs::write(proj.join("about.md"), "# about").unwrap();

        let tree = list_named_project(&proj);
        let flags = home_flags(&tree);
        assert!(flags.contains(&("index.md".to_string(), true)));
        assert!(flags.contains(&("about.md".to_string(), false)));
    }

    #[test]
    fn is_home_marker_beats_index_md_and_sorts_first() {
        // `home: true` frontmatter on a plainly-named file wins over index.md
        // (marker › index), matching the build's election — the rule the old
        // frontend homeRank mirror missed. The elected home must ALSO sort
        // first (ahead of index.md's Zone::HomeMd claim) so the flag and the
        // first-row home-pair agree.
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("site");
        std::fs::create_dir(&proj).unwrap();
        std::fs::write(proj.join("index.md"), "# not the home").unwrap();
        std::fs::write(proj.join("welcome.md"), "---\nhome: true\n---\n# home").unwrap();

        let tree = list_named_project(&proj);
        let flags = home_flags(&tree);
        assert!(flags.contains(&("welcome.md".to_string(), true)));
        assert!(flags.contains(&("index.md".to_string(), false)));
        let first = &tree.children.as_deref().unwrap()[0];
        assert_eq!(first.name, "welcome.md", "elected home must sort first");
        assert!(first.is_home);
    }

    #[test]
    fn is_home_alpha_fallback_flags_only_doc() {
        // A SUBFOLDER whose only doc is about.md HAS a home (priority-5
        // alphabetical fallback) — the build serves it at the folder URL, so
        // the tree flags it too. This is the intentional behavior change vs.
        // the old filename-only frontend mirror. (The PROJECT ROOT gates this
        // fallback out — see is_home_root_gates_out_alpha_fallback.)
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("site");
        let notes = proj.join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(proj.join("index.md"), "# site").unwrap();
        std::fs::write(notes.join("about.md"), "# about").unwrap();
        std::fs::write(notes.join("photo.png"), b"png").unwrap();

        let tree = list_named_project(&proj);
        let sub = tree
            .children
            .as_ref()
            .unwrap()
            .iter()
            .find(|c| c.name == "notes")
            .expect("notes subfolder");
        let flags = home_flags(sub);
        assert!(flags.contains(&("about.md".to_string(), true)));
        assert!(flags.contains(&("photo.png".to_string(), false)));
    }

    #[test]
    fn is_home_root_gates_out_alpha_fallback() {
        // PROJECT ROOT with only a fallback candidate: NOT flagged. Mirrors
        // detect_root_home_source (commands.rs) — the root editor surface
        // deliberately shows the "create home page" CTA instead of silently
        // adopting a random article as the SITE home, and the tree's
        // "Create home file" context item must agree with it. A marker,
        // index stem, or self-named note at the root still elects (covered
        // by the other is_home_* tests).
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("notes");
        std::fs::create_dir(&proj).unwrap();
        std::fs::write(proj.join("about.md"), "# about").unwrap();

        let tree = list_named_project(&proj);
        assert!(
            home_flags(&tree).iter().all(|(_, h)| !h),
            "root alpha-fallback candidate must not be flagged"
        );
    }

    #[test]
    fn is_home_flags_lang_suffixed_index() {
        // A lang-suffixed index (`index.zh-hans.md`, election priority 2) is
        // the folder's home — the old frontend mirror gave it rank 1 (≠ 0)
        // and wrongly offered "Create home file" for such folders.
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("docs");
        std::fs::create_dir(&proj).unwrap();
        std::fs::write(proj.join("index.zh-hans.md"), "# 首页").unwrap();
        std::fs::write(proj.join("guide.md"), "# guide").unwrap();

        let tree = list_named_project(&proj);
        let flags = home_flags(&tree);
        assert!(flags.contains(&("index.zh-hans.md".to_string(), true)));
        assert!(flags.contains(&("guide.md".to_string(), false)));
    }

    #[test]
    fn is_home_none_flagged_when_no_documents() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("assets");
        std::fs::create_dir(&proj).unwrap();
        std::fs::write(proj.join("photo.png"), b"png").unwrap();
        std::fs::write(proj.join("style.css"), "body{}").unwrap();

        let tree = list_named_project(&proj);
        assert!(
            home_flags(&tree).iter().all(|(_, h)| !h),
            "no doc → no home flagged"
        );
    }

    #[test]
    fn is_home_exactly_one_flagged_per_folder() {
        // Several candidates that the per-node zone predicate ALL calls
        // "home-shaped" (index.md, readme.md, self-named) — the election
        // flags exactly one (index.md, highest stem priority), and it sorts
        // first. Also checks a SUBFOLDER runs its own election with its own
        // basename.
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("docs");
        std::fs::create_dir(&proj).unwrap();
        std::fs::write(proj.join("index.md"), "# home").unwrap();
        std::fs::write(proj.join("readme.md"), "# readme").unwrap();
        std::fs::write(proj.join("docs.md"), "# self-named").unwrap();
        let sub = proj.join("guides");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("guides.md"), "# sub home").unwrap();
        std::fs::write(sub.join("intro.md"), "# intro").unwrap();

        let tree = list_named_project(&proj);
        let children = tree.children.as_deref().unwrap();
        let flagged: Vec<&str> = children
            .iter()
            .filter(|c| c.is_home)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(flagged, vec!["index.md"], "exactly one home per folder");
        assert_eq!(children[0].name, "index.md");

        let guides = children.iter().find(|c| c.name == "guides").unwrap();
        let sub_flags = home_flags(guides);
        assert!(sub_flags.contains(&("guides.md".to_string(), true)));
        assert!(sub_flags.contains(&("intro.md".to_string(), false)));
        assert_eq!(
            sub_flags.iter().filter(|(_, h)| *h).count(),
            1,
            "exactly one home in the subfolder too"
        );
    }
}
