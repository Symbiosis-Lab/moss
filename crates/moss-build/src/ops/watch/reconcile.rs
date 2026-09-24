//! Keeping the watcher's subscription set in step with the project.
//!
//! Since the project root is no longer watched recursively
//! ([`crate::build::watch::scope`]),
//! nothing covers a top-level entry that appears after startup. A low-frequency
//! tick re-derives the target set and applies the difference: one `read_dir`
//! over ~10 entries, bounded work, unlike the recursive root watch it replaces.
//!
//! Subscription upkeep ONLY. This module used to also hold the *verdict* —
//! "an entry appeared/disappeared, rebuild" — which moved to the sweep
//! (the app-side `build_shell::watch::sweep`, phase 2 of the 2026-08-18
//! watcher-reliability design):
//! the sweep owns every rebuild decision, and this loop's remaining job is to
//! keep the event accelerator subscribed to what exists. That split is why it
//! no longer needs the app handle, the server port, or a trigger path.

use std::path::Path;

use notify::Watcher;

use crate::build::watch::scope;

/// How often the watcher re-derives its subscription set.
///
/// Two seconds keeps "drop a folder in and see events flow from it" feeling
/// immediate without making the `read_dir` noticeable.
pub(crate) const INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

type Debouncer = notify_debouncer_full::Debouncer<
    notify::RecommendedWatcher,
    notify_debouncer_full::FileIdMap,
>;

/// Re-derive the watch set and apply the difference: watch what appeared,
/// unwatch what vanished. Whether either amounts to a content change worth a
/// rebuild is the sweep's call ([`scope::watch_set_content_change`]), made
/// against its own snapshot on its own tick.
pub(crate) fn targets(debouncer: &mut Debouncer, root: &Path, watched: &mut Vec<scope::WatchTarget>) {
    let desired = scope::watch_targets(root);

    for (path, _) in watched.iter() {
        if !desired.iter().any(|(p, _)| p == path) {
            let _ = debouncer.watcher().unwatch(path);
            debouncer.cache().remove_root(path);
            log::info!(target: "moss::build::watch", "No longer watching {} — top-level entry disappeared", path.display());
        }
    }

    // Counted, not listed: the first reconcile of a large tree adds hundreds of
    // roots at once, and "moss is watching the directories it should be" is a
    // fact about the whole set, not about each one. Paths stay at DEBUG.
    let mut newly_watched = 0usize;
    let mut next: Vec<scope::WatchTarget> = Vec::with_capacity(desired.len());
    for (path, mode) in desired {
        let known = watched.iter().any(|(p, m)| *p == path && *m == mode);
        // Re-issue even for targets we believe we hold. `watched` is moss's own
        // bookkeeping, not the kernel's: notify drops a watch behind our back on
        // `IN_DELETE_SELF`/`IN_MOVE_SELF`, and an inotify watch on a FILE dies
        // outright when an editor's atomic save replaces the inode
        // (`.moss/config.toml` is exactly that shape). Both leave the path in
        // `desired` AND in `watched`, so a diff-only reconciler would never
        // restore it, and every later edit under it would be silently invisible
        // for the life of the process. `watch()` is idempotent per path, so
        // re-issuing is one cheap `inotify_add_watch` that heals it in a tick.
        //
        // Not on macOS, where `watch()` stops, rebuilds and restarts the entire
        // FSEvents stream — expensive, and an event-loss window every tick.
        // FSEvents is path-based and does not lose watches, so it needs none of
        // this. See `scope::REWATCH_IS_CHEAP`.
        if known && !scope::REWATCH_IS_CHEAP {
            next.push((path, mode));
            continue;
        }
        if let Err(e) = debouncer.watcher().watch(&path, mode) {
            // Transient — the entry may have vanished between read_dir and now.
            // The next tick retries.
            log::debug!(target: "moss::build::watch", "Not watching {}: {}", path.display(), e);
            continue;
        }
        if !known {
            debouncer.cache().add_root(&path, mode);
            newly_watched += 1;
            log::debug!(target: "moss::build::watch", "Now watching {}", path.display());
        }
        next.push((path, mode));
    }
    *watched = next;

    if newly_watched > 0 {
        log::info!(target: "moss::build::watch", "Now watching {} more path(s)", newly_watched);
    }
}
