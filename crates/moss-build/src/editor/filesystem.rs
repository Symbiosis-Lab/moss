//! Filesystem helpers — directory listing, file tree, file/folder CRUD, and
//! collision-safe copies.
//!
//! These helpers back the `list_directory`, `list_tree`, `create_files`,
//! `create_folder`, `copy_files_to_project`, `get_file_info`,
//! and `resolve_url_for_file` Tauri commands in `commands.rs`.


mod bounded;
mod directory;
mod publish_dates;
pub use bounded::{bounded_tree, DOCUMENT_TREE_BUDGET};
pub use publish_dates::{PublishDateCacheView, PublishDateMemo};
pub use directory::list_directory_inner;
#[cfg(test)]
use directory::list_directory_counted;

// ── Public types ──────────────────────────────────────────────────────────

pub use moss_core::source_edit::{EditorFileInfo, DirEntry, TreeNode, SpecialFileKind};
pub use moss_core::source_edit::tree::{cmp_dir_entry, cmp_entry_fields, sort_tree_nodes, compute_folder_recency};

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

/// Inner implementation of `list_tree` for testability.
///
/// `project_path` is the project root, used by `is_hidden` to compute the
/// relative path when filtering `.moss/` children.
///
/// The walk with no memo: every page is read fresh.
pub fn list_tree_inner(
    path: &str,
    project_path: &str,
    show_internal: bool,
) -> Result<TreeNode, String> {
    let mut empty = std::collections::HashMap::new();
    let cache = std::sync::Mutex::new(&mut empty);
    list_tree_inner_cached(path, project_path, show_internal, &cache)
}

/// The walk with a memo ([`PublishDateMemo::with_folder`] lends one).
///
/// Reads each `.md` file's first 4 KB once per (path, mtime) pair, caches
/// the resolved publish date, and feeds the result into the five-zone sort.
pub fn list_tree_inner_cached(
    path: &str,
    project_path: &str,
    show_internal: bool,
    cache: &PublishDateCacheView<'_>,
) -> Result<TreeNode, String> {
    list_tree_inner_cached_with_error(path, project_path, show_internal, cache)
        .map_err(|failure| failure.to_string())
}

pub fn list_tree_inner_cached_with_error(
    path: &str,
    project_path: &str,
    show_internal: bool,
    cache: &PublishDateCacheView<'_>,
) -> Result<TreeNode, crate::build::cloud_readiness::storage::Unavailable> {
    let (entries, options) = tree_listing(path, project_path, show_internal, cache)?;
    let mut tree = moss_core::source_edit::build_tree_root(&entries, &options);
    // Preserve the spelling the caller supplied for the containing directory.
    tree.path = path.to_string();
    Ok(tree)
}

/// Collect directory facts with the existing availability handling and memo.
fn tree_listing(
    path: &str,
    project_path: &str,
    show_internal: bool,
    cache: &PublishDateCacheView<'_>,
) -> Result<(Vec<moss_core::source_edit::ListingEntry>, moss_core::source_edit::TreeOptions), crate::build::cloud_readiness::storage::Unavailable> {
    let root = crate::vault_root::VaultRoot::resolve(project_path);
    let directory = crate::vault_root::VaultRoot::resolve(path);
    let relative = directory.path().strip_prefix(root.path()).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    let options = moss_core::source_edit::TreeOptions {
        root_name: root.name().to_string(),
        path_prefix: project_path.to_string(),
        directory: relative.clone(), show_internal,
        page_settings_dirs: crate::build::scan::classify::page_settings_dirs(project_path),
        passthrough: Vec::new(),
    };
    let mut listing = Vec::new();
    collect_tree_listing(path, project_path, &relative, show_internal, cache, &mut listing)?;
    Ok((listing, options))
}

fn collect_tree_listing(
    path: &str,
    project_path: &str,
    relative: &str,
    show_internal: bool,
    cache: &PublishDateCacheView<'_>,
    listing: &mut Vec<moss_core::source_edit::ListingEntry>,
) -> Result<(), crate::build::cloud_readiness::storage::Unavailable> {
    use moss_core::source_edit::ListingEntry;
    let (entries, hidden) = directory::list_directory_sized(path, project_path, show_internal)?;
    let metadata = std::fs::metadata(path).ok();
    listing.push(ListingEntry {
        path: relative.to_string(), is_dir: true, size: metadata.as_ref().map_or(0, |m| m.len()),
        modified: metadata.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64()),
        file_id: metadata.as_ref().and_then(extract_file_id),
        frontmatter: None, resolved_date: None, hidden,
    });
    for (entry, size) in entries {
        let rel = if relative.is_empty() { entry.name.clone() } else { format!("{relative}/{}", entry.name) };
        if entry.is_dir {
            collect_tree_listing(&entry.path, project_path, &rel, show_internal, cache, listing)?;
        } else {
            let resolved_date = entry.name.to_ascii_lowercase().ends_with(".md").then(|| resolve_md_date(&entry, cache));
            listing.push(ListingEntry {
                path: rel, is_dir: false,
                size,
                modified: entry.modified, file_id: entry.file_id, frontmatter: None, resolved_date, hidden: 0,
            });
        }
    }
    Ok(())
}

/// Resolve an md file's publish date AND its `home: true` frontmatter marker
/// from one cached 4 KB frontmatter read. Returns
/// `(publish_date, date_source, home_marker)`.
///
/// Both are read via `read_frontmatter_only` → `frontmatter_map`, which covers
/// both frontmatter dialects — so the tree flag now agrees with the build's
/// `file_has_home_marker` instead of missing simplified frontmatter.
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
    let resolved = moss_core::source_edit::tree::resolve_md_date(&entry.name, entry.modified, &frontmatter);

    if cacheable {
        let mut guard = cache.lock().expect("publish-date cache poisoned");
        guard.insert(key, resolved.clone());
    }

    resolved
}

/// `path`, or the first of `name-2.ext`, `name-3.ext`, … beside it that
/// nothing occupies on disk.
pub fn resolve_collision(path: &std::path::Path) -> std::path::PathBuf {
    first_free(path, |_| false)
}

/// `path`, or the first `-2`, `-3`, … suffixed name beside it that is neither
/// on disk nor `claimed`.
pub(crate) fn first_free(path: &std::path::Path, claimed: impl Fn(&std::path::Path) -> bool) -> std::path::PathBuf {
    // symlink_metadata, not exists(): exists() follows symlinks, so a
    // dangling symlink at the destination reads as absent and a write would
    // go through it, outside the folder if it points there.
    let parent = path.parent().unwrap_or(path);
    parent.join(moss_core::source_edit::free_name(parent, path.file_name().unwrap_or_default(), |p| {
        p.symlink_metadata().is_ok() || claimed(p)
    }))
}

// ── Frontmatter helper for tree construction ──────────────────────────────

/// Read and parse the core's bounded frontmatter prefix for tree construction.
pub fn read_frontmatter_only(
    path: &std::path::Path,
) -> std::io::Result<std::collections::HashMap<String, serde_yaml::Value>> {
    use std::io::Read;
    use moss_core::source_edit::tree::{frontmatter_from_prefix, FRONTMATTER_PREFIX_BYTES};
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::with_capacity(FRONTMATTER_PREFIX_BYTES);
    file.take(FRONTMATTER_PREFIX_BYTES as u64).read_to_end(&mut buf)?;
    Ok(frontmatter_from_prefix(&buf))
}

// ── Publish-date resolution ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_name_skips_disk_entries_and_claims() {
        let dir = tempfile::tempdir().unwrap();
        let wanted = dir.path().join("a.md");
        assert_eq!(resolve_collision(&wanted), wanted);
        std::fs::write(&wanted, "").unwrap();
        std::fs::create_dir(dir.path().join("a-2.md")).unwrap();
        assert_eq!(first_free(&wanted, |p| p == dir.path().join("a-3.md")), dir.path().join("a-4.md"));
    }

    #[cfg(unix)]
    #[test]
    fn free_name_skips_dangling_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let wanted = dir.path().join("a.md");
        std::os::unix::fs::symlink(dir.path().join("missing"), &wanted).unwrap();
        assert_eq!(resolve_collision(&wanted), dir.path().join("a-2.md"));
    }

    #[test]
    fn directory_open_failure_keeps_path_and_operation_at_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-directory");
        std::fs::write(&file, "file").unwrap();

        let error = list_directory_inner(
            file.to_str().unwrap(),
            dir.path().to_str().unwrap(),
            false,
        )
        .unwrap_err();

        assert!(error.contains(file.to_str().unwrap()), "{error}");
        assert!(error.contains("opening directory listing"), "{error}");
        assert!(error.contains("is not a directory"), "{error}");
        let unavailable = list_directory_counted(file.to_str().unwrap(), dir.path().to_str().unwrap(), false).unwrap_err();
        assert_eq!(unavailable.pending, None);
        #[cfg(unix)]
        assert_eq!(unavailable.failure.raw_os_error(), Some(libc::ENOTDIR));
        #[cfg(windows)]
        assert_eq!(unavailable.failure.raw_os_error(), Some(267));
    }

    #[test]
    fn availability_editor_tree_recovers_child_metadata_after_parent_eof() {
        use crate::build::cloud_readiness::storage::TestFault;
        use crate::build::cloud_readiness::StorageOperation;
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child.md");
        std::fs::write(&child, "# child").unwrap();
        let _fault = TestFault::install(dir.path(), &child, StorageOperation::EntryMetadata, 1, std::time::Duration::from_secs(3));
        let tree = list_tree_inner(dir.path().to_str().unwrap(), dir.path().to_str().unwrap(), false).unwrap();
        assert_eq!(tree.children.unwrap().len(), 1);
    }

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
    fn tree_nodes_carry_their_special_role_and_agent_files_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("Quarry");
        for f in [
            "index.md", "index.html", "AGENTS.md", "footer.md", "404.md", "welcome-email.md", "about.md",
            "fr/index.md", "fr/index.html", "fr/footer.md", "docs/footer.md", "docs/index.md", "docs/index.html",
            ".moss/footer.md",
        ] {
            let path = proj.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "# x").unwrap();
        }
        std::fs::write(proj.join(".moss/config.toml"), "[build]\npassthrough = [\"index.html\", \"fr/index.html\"]\n").unwrap();

        let tree = list_named_project(&proj);
        fn collect(node: &TreeNode, root: &str, out: &mut Vec<(String, SpecialFileKind)>) {
            if let Some(kind) = node.special {
                out.push((node.path.strip_prefix(root).unwrap().trim_start_matches('/').to_string(), kind));
            }
            for child in node.children.iter().flatten() {
                collect(child, root, out);
            }
        }
        let mut found = Vec::new();
        collect(&tree, proj.to_str().unwrap(), &mut found);
        found.sort_by(|a, b| a.0.cmp(&b.0));
        use SpecialFileKind::*;
        assert_eq!(
            found,
            vec![
                ("404.md".to_string(), NotFound),
                ("AGENTS.md".to_string(), AgentInstructions),
                ("footer.md".to_string(), Footer),
                ("fr/footer.md".to_string(), Footer),
                ("fr/index.md".to_string(), PageSettings),
                ("index.md".to_string(), PageSettings),
                ("welcome-email.md".to_string(), WelcomeEmail),
            ]
        );
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
        // The self-named root home (`河灣/河灣.md`) is elected whether the
        // project root arrives as an ABSOLUTE path, with a TRAILING SLASH, or as
        // `.` — the root name now comes from `VaultRoot::resolve`, not
        // `Path::file_name()`. `Path::file_name()` is `None` for `.`, which left
        // the election with an empty root name; the self-named rule could not
        // fire, and the root's gated-out alphabetical fallback then elected NO
        // home at all (`河灣.md` silently demoted off `/`).
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("\u{521d}\u{96ea}");
        std::fs::create_dir(&proj).unwrap();
        std::fs::write(proj.join("\u{521d}\u{96ea}.md"), "# home").unwrap();
        std::fs::write(proj.join("note.md"), "# note").unwrap();
        let home = ("\u{521d}\u{96ea}.md".to_string(), true);

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

#[cfg(test)]
mod listing_parity_tests {
    use super::*;

    #[test]
    fn desktop_walk_and_flat_listing_produce_the_same_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        for (path, text) in [
            ("index.md", "---\ntitle: Home\n---\n"),
            ("chosen.md", "home: true\n---\nBody"),
            ("dated.md", "---\ndate: 2025-11-15\n---\n"),
            ("2024-01-01-post.md", "Body"),
            ("plain.md", "Body"),
            ("notes/note.md", "date: 2026-01-01\n---\nBody"),
            ("notes/.draft.md", "Body"),
            ("photo.png", "image"),
            (".secret", "hidden"),
            (".moss/config.toml", "[build]\npassthrough = [\"index.html\", \"fr/index.html\", \"app/index.html\"]\n"),
            ("index.html", "html"),
            ("fr/index.html", "html"),
            ("fr/index.md", "body"),
            ("app/index.html", "html"),
            ("app/index.md", "body"),
            (".moss/theme/style.css", ""),
            (".moss/cache/private", ""),
            ("node_modules/package/code.js", ""),
            ("notes/Thumbs.db", ""),
        ] {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let header = "---\ndate: 2099-01-01\nhome: true\npadding: ";
        let limit = moss_core::source_edit::tree::FRONTMATTER_PREFIX_BYTES;
        let long = format!("{header}{}\n---\nBody", "x".repeat(limit));
        let split = format!("{header}{}河\n---\nBody", "x".repeat(limit - header.len() - 1));
        for (name, text) in [("long-frontmatter.md", long), ("split-utf8.md", split)] {
            std::fs::write(dir.path().join("notes").join(name), text).unwrap();
        }
        for show_internal in [false, true] {
            let mut memo = std::collections::HashMap::new();
            let cache = std::sync::Mutex::new(&mut memo);
            let (listing, options) = tree_listing(root, root, show_internal, &cache).unwrap();
            let desktop = list_tree_inner_cached(root, root, show_internal, &cache).unwrap();
            let core = moss_core::source_edit::build_tree_root(&listing, &options);
            assert_eq!(serde_json::to_value(&desktop).unwrap(), serde_json::to_value(&core).unwrap());

            // A byte-only host can supply the raw listing instead of the desktop
            // memo and prefilter. Both must yield the identical flags and counts.
            let raw: Vec<_> = walkdir::WalkDir::new(dir.path()).into_iter().map(|entry| {
                let entry = entry.unwrap();
                let meta = entry.metadata().unwrap();
                let relative = entry.path().strip_prefix(dir.path()).unwrap().to_string_lossy().replace('\\', "/");
                moss_core::source_edit::ListingEntry {
                    path: relative, is_dir: meta.is_dir(), size: meta.len(),
                    modified: meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs_f64()),
                    file_id: extract_file_id(&meta),
                    frontmatter: (!meta.is_dir()).then(|| std::fs::read_to_string(entry.path()).unwrap()),
                    resolved_date: None, hidden: 0,
                }
            }).collect();
            let byte_options = moss_core::source_edit::TreeOptions {
                page_settings_dirs: Default::default(),
                passthrough: vec!["index.html".into(), "fr/index.html".into(), "app/index.html".into()],
                ..options
            };
            let from_bytes = moss_core::source_edit::build_tree_root(&raw, &byte_options);
            assert_eq!(serde_json::to_value(&desktop).unwrap(), serde_json::to_value(&from_bytes).unwrap());
            let names: Vec<_> = desktop.children.as_ref().unwrap().iter().map(|c| c.name.as_str()).collect();
            assert_eq!(&names[..5], ["chosen.md", "index.md", "dated.md", "2024-01-01-post.md", "plain.md"]);
        }
    }
}
