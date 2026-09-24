//! What the file watcher is allowed to see, and where it registers.
//!
//! Two halves of one question:
//!
//! - **Registration** ([`watch_targets`]) — the set of paths handed to
//!   `notify::Watcher::watch`. This is the load-bearing half.
//! - **Filtering** ([`path_is_watchable`], [`path_passes_filter`]) — the
//!   per-event predicates applied to whatever still arrives.
//!
//! ## Why registration, not filtering
//!
//! moss watched the project root recursively and filtered moss's own output
//! back out per event. That works only for events that carry a path. A build
//! writing ~5700 files overflows the FSEvents buffer, and the overflow arrives
//! as a **pathless** `Other`+`Rescan` event, which no filter can attribute.
//! The watcher's only correct response to "I lost events" is a full rebuild —
//! which writes the output again. On the reported site that ran for 55 minutes
//! at ~1000% CPU, sealing an identical generation every 8.7s.
//!
//! Four earlier instances of the same class were each fixed by adding an
//! exclusion at one more filter site. This one cannot be, so registration
//! changed instead: **moss's output is never subscribed to in the first
//! place.** Verified against the vendored notify sources:
//!
//! - `RecursiveMode::NonRecursive` does **not** narrow the macOS kernel
//!   stream. `fsevent.rs` registers the path either way and filters recursion
//!   in its callback, so a non-recursive root watch still buffers every
//!   `.moss/build.nosync` event and still overflows. On macOS the root must not be
//!   registered **at all**.
//! - Linux (inotify) and Windows (`ReadDirectoryChangesW`) do narrow, so there
//!   the root is registered non-recursively — which is also what keeps
//!   root-level files covered by an inode-stable watch.
//! - macOS creates its stream with `kFSEventStreamCreateFlagFileEvents`, so
//!   root-level files can be registered individually instead. That is the one
//!   place the two platforms genuinely differ.
//!
//! Which paths are moss's own is **not** decided here. It comes from
//! `infra::moss_paths::MOSS_PATH_RULES`, the single registry every consumer
//! reads. Add a path moss writes there and this module excludes it for free.

use notify::RecursiveMode;
use std::path::{Path, PathBuf};

use crate::infra::moss_paths;

/// A path to hand to `notify::Watcher::watch`, with the mode to use.
/// Public so `tests/watch_self_trigger_test.rs` can subscribe to exactly what
/// the app subscribes to — an invariant test that computed its own set would
/// be testing itself.
pub type WatchTarget = (PathBuf, RecursiveMode);

/// Whether this platform's watcher narrows its kernel subscription when told
/// `NonRecursive`. False on macOS — see the module doc.
const RECURSIVE_MODE_NARROWS: bool = !cfg!(target_os = "macos");

/// Whether re-issuing `watch()` on a path already watched is cheap enough to
/// do on every reconcile tick.
///
/// True for inotify/`ReadDirectoryChangesW`, where it is one syscall against a
/// per-path registration — which is what lets the reconciler heal a watch the
/// kernel dropped underneath us. False on macOS, where `watch()` stops, rebuilds
/// and restarts the whole FSEvents stream; there it would be both expensive and
/// an event-loss window. FSEvents is path-based and does not lose watches, so it
/// needs no healing.
pub const REWATCH_IS_CHEAP: bool = RECURSIVE_MODE_NARROWS;

/// The exact set of paths the watcher should subscribe to for `root`.
///
/// Recomputed periodically by the watcher's reconciler, so it must stay a
/// cheap, pure function of one `read_dir` plus a handful of `exists` checks.
///
/// Returns targets in a stable order (directory entries sorted) so a caller
/// diffing two calls sees only real changes.
pub fn watch_targets(root: &Path) -> Vec<WatchTarget> {
    let mut targets: Vec<WatchTarget> = Vec::new();

    // The root itself, only where NonRecursive genuinely narrows. This is what
    // covers root-level `index.md` and the arrival of new top-level entries on
    // Linux/Windows; on macOS both are covered by per-entry targets plus the
    // reconciler.
    if RECURSIVE_MODE_NARROWS {
        targets.push((root.to_path_buf(), RecursiveMode::NonRecursive));
    }

    let mut entries: Vec<PathBuf> = match std::fs::read_dir(root) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
        // An unreadable root is the caller's problem (it will fail to watch
        // anything and log); returning what we have keeps this total.
        Err(_) => Vec::new(),
    };
    entries.sort();

    for entry in entries {
        let Some(name) = entry.file_name().and_then(|n| n.to_str()) else { continue };
        // `.moss/` is handled below by the registry allowlist, not here. Every
        // other dot-entry (`.git`, `.obsidian`, `.DS_Store`) and `node_modules`
        // is excluded, matching `path_is_watchable`'s rule.
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        // Root-level files moss writes itself (`AGENTS.md` and friends).
        if !moss_paths::is_watchable_rel(name) {
            continue;
        }
        if entry.is_dir() {
            // Nested-vault boundary (2026-08-19 design §4): a first-level dir
            // that owns its own `.moss/` is a different site — never subscribe
            // to its subtree. Deeper nested vaults stay covered by
            // [`path_in_nested_vault`] at the event filter.
            if entry.join(".moss").is_dir() {
                continue;
            }
            targets.push((entry, RecursiveMode::Recursive));
        } else if !RECURSIVE_MODE_NARROWS {
            // macOS only: no root watch, so each root-level file is its own
            // target. Elsewhere the root's non-recursive watch already covers
            // them, and an inotify watch on a file dies when an editor's
            // atomic save replaces the inode.
            //
            // Subscribing costs a rebuild here in a way it does not elsewhere:
            // the reconciler treats a newly appeared target as a content
            // change, bypassing the per-event filters. So apply the file-type
            // filter at registration. Without it moss's own `Icon\r` — which
            // `stamp_published_folder` writes into the project root on every
            // publish — would become a target and buy a full rebuild of the
            // site moss just published. That is instance 6 of the very class
            // this module exists to close.
            if !path_passes_filter(root, &entry) {
                continue;
            }
            targets.push((entry, RecursiveMode::NonRecursive));
        }
    }

    // The user-editable interior of `.moss/`. Registered explicitly because the
    // `.moss` directory as a whole is never watched.
    for rule in moss_paths::MOSS_PATH_RULES.iter().filter(|r| r.watched) {
        let path = root.join(rule.rel.trim_end_matches('/'));
        if !path.exists() {
            continue;
        }
        let mode = if rule.rel.ends_with('/') {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        targets.push((path, mode));
    }

    targets
}

/// Did the top-level target set change in a way that is a CONTENT change?
///
/// The sweep's every-tick verdict over two [`watch_targets`] snapshots
/// (paths only — the mode is registration detail). An entry that appeared
/// was populated *before* any watch could land on it, so its files' events
/// are gone and the set diff is the only signal; an entry that disappeared
/// emits no Remove event on macOS because the root is never watched there.
/// Both directions therefore rebuild. Two carve-outs, pinned by tests:
///
/// - `root_readable == false` masks every disappearance: the vault root
///   itself being gone makes everything "disappear" at once, and that is
///   the project-unavailable verdict's problem, never a rebuild.
/// - `.moss/data/social/` is exempt both ways: background social sync owns
///   that tree and decides for itself whether the preview refreshes.
///
/// This used to be the watcher-loop reconciler's job
/// (`build_shell/watch/reconcile.rs`), where it died with the loop it was
/// supposed to heal; phase 2 of the 2026-08-18 watcher-reliability design
/// moved the verdict into the sweep and left the reconciler subscription
/// upkeep only.
pub fn watch_set_content_change(
    prev: &[PathBuf],
    desired: &[PathBuf],
    root_readable: bool,
    social: &Path,
) -> bool {
    let appeared = desired
        .iter()
        .any(|p| !prev.contains(p) && !p.starts_with(social));
    let disappeared = root_readable
        && prev
            .iter()
            .any(|p| !desired.contains(p) && !p.starts_with(social));
    appeared || disappeared
}

/// Returns `true` if any element of `paths` is watchable under moss's
/// gitignore-style rules. Mirrors the per-event prefilter that used to live
/// inline in `start_file_watching` before the debouncer refactor.
///
/// It says what it dropped, which is unusual for a predicate and is the point:
/// this is the rebuild pump's FIRST filter, and it fails by `continue` — so a
/// vault whose every event was rejected looked exactly like a vault nobody was
/// editing, for weeks, with the watcher alive and delivering.
pub fn any_path_watchable(root: &Path, paths: &[PathBuf]) -> bool {
    if paths.iter().any(|p| path_is_watchable(root, p)) {
        return true;
    }
    log::debug!(
        target: "moss::build::watch",
        "Dropped {} unwatchable path(s): {:?}",
        paths.len(),
        paths
    );
    false
}

/// True when `path` lies inside a NESTED moss root below `root` — a subtree
/// that is a different site's territory (nested-vault boundary, 2026-08-19
/// design §4). Probes each directory strictly between `root` and `path` for
/// a `.moss/` of its own: one lstat per level, so it is safe per-event.
pub fn path_in_nested_vault(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else { return false };
    let comps: Vec<_> = rel.components().collect();
    let mut probe = root.to_path_buf();
    for comp in comps.iter().take(comps.len().saturating_sub(1)) {
        probe.push(comp);
        if probe.join(".moss").is_dir() {
            return true;
        }
    }
    false
}

/// Every path of the event belongs to some nested vault below `root`: the
/// event is the inner site's business, never the outer watcher's.
pub fn all_paths_in_nested_vault(root: &Path, paths: &[PathBuf]) -> bool {
    !paths.is_empty() && paths.iter().all(|p| path_in_nested_vault(root, p))
}

/// Returns `true` if every path in an event names something moss writes or
/// otherwise excludes from watch-triggered rebuilds, judged **relative to the
/// project root**.
///
/// [`path_is_watchable`] cannot answer this: it sees an absolute path with no
/// root to strip, so it can only reason about `.moss/` (which appears
/// literally in the path) — not about a root `AGENTS.md`, which is excluded
/// here regardless of who wrote it, and which is ordinary content one
/// directory down (`editor::filesystem` pins that distinction).
///
/// It matters on Linux and Windows, where [`watch_targets`] registers the
/// project root non-recursively and therefore hears about every root-level
/// file whether it is a target or not. On macOS nothing subscribes to those
/// paths in the first place; this keeps the platforms honest.
///
/// Events with no paths (the rescan flag) are **not** moss-written — they carry
/// no claim at all, and swallowing them here would silently drop the
/// mass-materialization signal the rescan handling exists for.
pub fn all_paths_moss_written(root: &Path, paths: &[PathBuf]) -> bool {
    if paths.is_empty() {
        return false;
    }
    paths.iter().all(|p| {
        p.strip_prefix(root)
            .ok()
            .and_then(|rel| rel.to_str())
            .is_some_and(|rel| {
                let rel = moss_core::slug::normalize_separators(rel);
                !rel.is_empty() && !moss_paths::is_watchable_rel(&rel)
            })
    })
}

/// The path INSIDE the vault, `/`-separated, or `None` when `path` is not
/// under `root`.
///
/// Both sides are normalized (`\`→`/`) and compared as strings rather than as
/// `Path` components, so a Windows-shaped path answers the same question on
/// any host — which is what lets the Windows cases below be plain unit tests.
/// The `/` boundary check is what keeps `…/site-backup/x` from stripping
/// against a root of `…/site`.
fn vault_rel(root: &Path, path: &Path) -> Option<String> {
    let root_str = moss_core::slug::normalize_separators(&root.to_string_lossy());
    let root_str = root_str.trim_end_matches('/');
    let path_str = moss_core::slug::normalize_separators(&path.to_string_lossy());
    if root_str.is_empty() {
        return Some(path_str);
    }
    let rest = path_str.strip_prefix(root_str)?;
    if rest.is_empty() {
        return Some(String::new());
    }
    Some(rest.strip_prefix('/')?.to_string())
}

/// The vault-relative path, falling back to the whole path when `path` is not
/// under `root`.
///
/// The fallback is the pre-2026-08-19 behaviour, so no call site is worse off
/// than it was — but it is also where a whole vault can go dark (see
/// [`path_is_watchable`]), so it says so once per process instead of never.
fn rel_or_whole(root: &Path, path: &Path) -> String {
    match vault_rel(root, path) {
        Some(rel) => rel,
        None => {
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                log::warn!(
                    target: "moss::build::watch",
                    "Watch filter asked about '{}', which is not inside '{}' — judging the \
                     whole path. If nothing in this folder ever rebuilds, this is why.",
                    path.display(),
                    root.display()
                );
            });
            moss_core::slug::normalize_separators(&path.to_string_lossy())
        }
    }
}

/// The tail after the LAST `.moss` component of a `/`-separated relative path,
/// or `None` if there is no such component (or nothing follows it).
///
/// Component-wise rather than `contains("/.moss/")`, because the path handed
/// in is vault-relative: `.moss/theme/style.css` has no leading `/` and would
/// miss a substring gate, which would then send a user-editable theme file to
/// the dotfile rule and stop theme edits from rebuilding.
fn after_last_moss(rel: &str) -> Option<&str> {
    let mut found = None;
    let mut rest = rel;
    // `split_once` rather than an index arithmetic walk: `clippy::string_slice`
    // is denied crate-wide, and rightly — byte indices into a `str` are a UTF-8
    // panic waiting for the first vault with a non-ASCII directory name.
    while let Some((head, tail)) = rest.split_once('/') {
        if head == ".moss" {
            found = Some(tail);
        }
        rest = tail;
    }
    found
}

/// The mount shapes a vault is really found at — the fixture behind the
/// relocation invariant every path-judging predicate is held to: **a
/// predicate's verdict on a file must not change when the vault is moved
/// under a different ancestor.**
///
/// Shared with `outcome_tests` and `sweep_tests` deliberately. The bug this
/// exists for has now been fixed three times at three call sites (the outcome
/// classifier, the sweep, and the rebuild pump); what wants pinning is the shape, so the
/// next predicate that consumes a vault path inherits the coverage instead of
/// re-earning it.
// Not cfg(test): consumed by the app crate's tests across the crate
// boundary, where a cfg(test) item would be configured out.
pub const VAULT_MOUNTS: &[(&str, &str)] = &[
    ("a plain path", "/home/u/Sites/blog"),
    // A real-world vault this bug hit: a Google shared drive reached through a
    // shortcut, which is the NORMAL layout for one — the user never chose it.
    (
        "a Google shared-drive shortcut",
        "/Users/u/Library/CloudStorage/GoogleDrive-a@b.com/.shortcut-targets-by-id/0ABCdEf/Editorial/blog",
    ),
    ("iCloud Drive", "/Users/u/Library/Mobile Documents/com~apple~CloudDocs/blog"),
    ("an ancestor literally named node_modules", "/home/u/node_modules/blog"),
    ("a vault whose own directory name is dot-prefixed", "/home/u/Sites/.blog"),
    // `TempDir::new()` mints `.tmpXXXXXX`, so every fixture in the suite is
    // already mounted under a dot-prefixed ancestor. That is load-bearing
    // coverage resting on a temp-dir implementation detail; stated here, it
    // survives a cleanup that switches to a non-dot prefix.
    ("a default tempfile::TempDir", "/tmp/.tmpA1b2C3/blog"),
];
// Deliberately POSIX-only. A Windows path is a separator question, not a mount
// question, and `std::path` cannot parse `C:\…` on a Linux host — a row here
// would silently measure the not-under-root fallback in every consumer that
// strips with `Path::strip_prefix`. Backslash coverage lives in
// `watch_tests::windows_backslash_*`, which asks it of this module directly.

/// Join a vault-relative path onto a [`VAULT_MOUNTS`] mount.
// Not cfg(test): consumed by the app crate's tests across the crate
// boundary, where a cfg(test) item would be configured out.
pub fn mount_join(mount: &str, rel: &str) -> PathBuf {
    PathBuf::from(format!("{mount}/{rel}"))
}

/// Returns `true` if `path` survives the gitignore-style watcher prefilter.
///
/// Second line of defence now that [`watch_targets`] keeps moss's output
/// unsubscribed: a target registered before a build created a moss-owned path
/// beneath it can still deliver one. Outside `.moss/` it rejects any
/// dotfile/dir and `node_modules`.
///
/// **Takes the vault root, and asks only about the path inside it.** The
/// dotfile rule votes on every component it is shown, so an absolute path let
/// every directory the user happens to keep the vault UNDER cast a vote: a
/// Google shared-drive vault lives below `.shortcut-targets-by-id`, and every
/// file in it was therefore judged unwatchable — the rebuild pump dropped
/// every event and the sweep that backstops it saw nothing either.
/// Where the vault sits is the user's business; only the path inside it is
/// moss's. The signature carries the root so that mistake cannot be made
/// again by a caller that forgets to strip it, the same reason
/// [`all_paths_moss_written`] takes one.
pub fn path_is_watchable(root: &Path, path: &Path) -> bool {
    rel_is_watchable(&rel_or_whole(root, path))
}

/// [`path_is_watchable`] over an already vault-relative, `/`-separated path.
fn rel_is_watchable(rel: &str) -> bool {
    if let Some(after_moss) = after_last_moss(rel) {
        return should_watch_moss_file(after_moss);
    }
    !rel.split('/').any(|component| {
        (component.starts_with('.') && !component.is_empty()) || component == "node_modules"
    })
}

/// Returns `true` if any element of `paths` passes the extension-based file
/// filter (markdown/asset types, plus the `.moss/*` user-editable allowlist).
pub fn any_path_passes_filter(root: &Path, paths: &[PathBuf]) -> bool {
    paths.iter().any(|p| path_passes_filter(root, p))
}

/// Vault-relative for the same reason [`path_is_watchable`] is: its `.moss/`
/// allowlist branch keys on a `.moss` component, so a vault kept under
/// `~/.moss-backups/` would otherwise have every file read as moss's own.
pub fn path_passes_filter(root: &Path, path: &Path) -> bool {
    rel_passes_filter(&rel_or_whole(root, path))
}

/// [`path_passes_filter`] over an already vault-relative, `/`-separated path.
fn rel_passes_filter(rel: &str) -> bool {
    if let Some(after_moss) = after_last_moss(rel) {
        return should_watch_moss_file(after_moss);
    }
    should_watch_file(rel)
}

/// Check if a path inside `.moss/` should be watched for changes.
///
/// Thin adapter over the registry: takes the `.moss/`-relative tail the
/// watcher predicates carry (they see absolute paths with no project root to
/// strip) and asks `moss_paths` the root-relative question. The allowlist
/// itself — `config.toml`, `theme/**`, `data/social/**`, `assets/**`, and
/// nothing else — lives in `MOSS_PATH_RULES`.
pub fn should_watch_moss_file(after_moss: &str) -> bool {
    let rel = moss_core::slug::normalize_separators(after_moss);
    moss_paths::is_watchable_rel(&format!(".moss/{rel}"))
}

/// Check if a file or directory should trigger recompilation
///
/// Note: Directory-level filtering (.moss/, node_modules/, .git/) is handled by
/// [`path_is_watchable`] and by which paths [`watch_targets`] subscribes to at
/// all. This function only filters by file type/extension.
pub fn should_watch_file(path: &str) -> bool {
    // Content files (markdown)
    let content_extensions = ["md", "markdown"];

    // Asset files that should trigger rebuilds. Images come from the same
    // list the editor's SourceAssetChanged notifications use
    // (`super::IMAGE_EXTENSIONS`), so an extension the build consumes cannot
    // be silently missing here.
    let asset_extensions = [
        // Styles
        "css",
        // Scripts
        "js",
        // Fonts
        "woff", "woff2", "ttf", "otf", "eot",
        // Video (the media pipeline's `video_exts` in media/pipeline.rs)
        "mov", "mp4", "webm", "avi", "mkv", "m4v",
        // Audio
        "mp3", "ogg", "wav", "m4a", "flac",
        // Documents
        "pdf",
        // Notebooks, rendered by build/notebook.rs
        "ipynb",
    ];

    // System files to ignore. Checked on the file NAME, not the whole path:
    // `Icon\r` (Finder's custom-icon carrier, which moss writes into every
    // published folder) contains no `.`, so the is_directory heuristic below
    // would otherwise classify it as a directory and watch it — meaning the
    // first publish immediately triggers a rebuild of the folder it just
    // published.
    let file_name = path.rsplit('/').next().unwrap_or(path);
    if crate::build::scan::classify::is_os_metadata_file(file_name) {
        return false;
    }

    // Check if this is a directory (ends with / or no extension). Judged on
    // the file NAME only, never the whole path: a project living at a dotted
    // path (`~/Sites/example.com/`) would otherwise make every directory
    // beneath it read as a file with a bogus extension, silently filtering
    // out directory create/rename/delete events.
    let is_directory = path.ends_with('/') || !file_name.contains('.');

    if is_directory {
        // Allow directories - gitignore matcher handles exclusions
        return true;
    }

    // Check file extension for files
    if let Some(extension) = file_name.rsplit('.').next() {
        let ext_lower = extension.to_lowercase();
        content_extensions.contains(&ext_lower.as_str())
            || asset_extensions.contains(&ext_lower.as_str())
            || super::IMAGE_EXTENSIONS.contains(&ext_lower.as_str())
    } else {
        false
    }
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
