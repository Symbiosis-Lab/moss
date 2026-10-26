//! What the watcher does for folders the author chose to show as pages: write
//! the stub for a file that just arrived (and move its read to the front of the
//! derive queue), and carry a folder's decision along when the folder is
//! renamed.
//!
//! Only watcher events call this. A build scan never does, so a stub the author
//! deleted stays deleted.

use std::path::{Path, PathBuf};

use notify::event::{ModifyKind, RenameMode};
use notify::EventKind;
use notify_debouncer_full::DebouncedEvent;

use crate::build::watch::{collect_raw_create_keys, raw_create_key};
use crate::moss_paths::MossPaths;
use crate::vault::derive_folders::{decisions, heal_renamed_folders, Decision};
use crate::vault::derive_state::site_registry;
use crate::vault::derive_stubs::stub_arrivals;

/// [`react`] for the async watcher: blocking work (config and page reads,
/// possibly waiting on a cloud download) goes to the blocking pool and the pump
/// moves on. A stub it writes arrives as a create event of its own.
pub fn react_detached(folder_path: &str, events: &[DebouncedEvent]) {
    let every: Vec<&DebouncedEvent> = events.iter().collect();
    let renames = crate::build::watch::extract_rename_pairs(&every, folder_path);
    let (events, folder) = (events.to_vec(), folder_path.to_string());
    tokio::task::spawn_blocking(move || react(&folder, &events, &renames));
}

/// React to one batch of watcher events. Costs one read of `config.toml` when
/// the site has made no derive decisions. Blocking: the caller runs it off the
/// async runtime.
///
/// A rename made through moss re-keys its folder decision in the rename engine;
/// healing here covers folders moved outside moss (Finder, a sync client), and
/// is a no-op for a key that already moved. A pair is followed only while the
/// disk still shows it: `from` gone and `to` there. Renames that chain inside
/// one debounce window (`a` to `b`, then `c` to `a`) would otherwise replay the
/// first pair and move the folder now called `a` onto `b`.
pub fn react(folder_path: &str, events: &[DebouncedEvent], renames: &[(String, String)]) {
    let site = MossPaths::new(Path::new(folder_path));
    let on_disk: Vec<(String, String)> = renames
        .iter()
        .filter(|(from, to)| !Path::new(folder_path).join(from).exists() && Path::new(folder_path).join(to).exists())
        .cloned()
        .collect();
    if let Err(e) = heal_renamed_folders(&site, &on_disk) {
        log::warn!(target: "derive", "could not follow a renamed folder in config.toml: {e}");
    }
    let created = arrivals(events, Path::new(folder_path));
    crate::system::derive::priority::note_arrived(Path::new(folder_path), &created);
    if created.is_empty() || !decisions(&site).values().any(|d| matches!(d, Decision::Show(_))) {
        return;
    }
    match site_registry(&site) {
        Ok(registry) => {
            stub_arrivals(&site, &registry, &created);
        }
        Err(e) => log::warn!(target: "derive", "could not list the site's plugins: {e}"),
    }
}

/// Files that just appeared: created, or moved in. A file dragged in from
/// elsewhere on the same disk is a rename the debouncer cannot pair, reported
/// as `Name(Any)` or `Name(To)` with only the destination. A rename inside the
/// site is a paired `Name(Both)` and is not an arrival.
fn arrivals(events: &[DebouncedEvent], root: &Path) -> Vec<PathBuf> {
    let mut keys = collect_raw_create_keys(events, root, |p| p.is_dir());
    for ev in events {
        if matches!(ev.kind, EventKind::Modify(ModifyKind::Name(mode)) if mode != RenameMode::Both) {
            keys.extend(ev.paths.iter().filter(|p| p.is_file()).filter_map(|p| raw_create_key(root, p)));
        }
    }
    keys.sort();
    keys.dedup();
    keys.into_iter().map(|key| root.join(key)).collect()
}

#[cfg(test)]
#[path = "derive_folders_tests.rs"]
mod derive_folders_tests;
