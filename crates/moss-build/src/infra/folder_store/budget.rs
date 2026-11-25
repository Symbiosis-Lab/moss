//! The second stage of the store sweep: while the stores' recorded sizes
//! total more than the budget, live stores unused longest give up their old
//! generations.

use super::*;
use crate::build::store_gc;

/// A store's size file, from its last seal: logical bytes, so a cloned file
/// counts in full and the sweep errs toward deleting.
fn store_size(cache_root: &Path, name: &str) -> Option<u64> {
    store_gc::recorded_store_size(&cache_root.join(name).join("build"))
}

/// What the stores may hold in all: `MOSS_STORE_BUDGET_GB` gigabytes, else a
/// tenth of the volume holding `root`.
pub(super) fn store_budget(root: &Path) -> Option<u64> {
    if let Some(gb) = std::env::var("MOSS_STORE_BUDGET_GB").ok().and_then(|v| v.trim().parse::<f64>().ok()) {
        return Some((gb.max(0.0) * 1e9) as u64);
    }
    crate::build::assets::download::volume_space(root).ok().map(|(_, total)| total / 10)
}

/// Whether `folder` has a serve owner now. The owner lock is only looked for,
/// never taken, so admitting an owner stays non-blocking.
fn in_use(folder: &Path) -> bool {
    crate::infra::folder_lock::lock_path_named(folder, "owner").is_ok_and(|lock| lock.symlink_metadata().is_ok())
}

/// Mark `record_dir`'s store as used now when its last use is over a day old,
/// so a process left open for weeks still counts as recent.
pub(crate) fn touch_use(record_dir: &Path) {
    let day = std::time::Duration::from_secs(24 * 3600);
    let record = record_dir.join(RECORD);
    if old_by(&record, day) {
        let _ = filetime::set_file_mtime(&record, filetime::FileTime::now());
    }
}

impl FolderStores {
    /// The recorded sizes of every store, summed; a store with none counts as nothing.
    pub(super) fn recorded_total(&self) -> u64 {
        let names = std::fs::read_dir(&self.record_root).into_iter().flatten().flatten();
        names.filter_map(|e| store_size(&self.cache_root, &e.file_name().to_string_lossy())).sum()
    }

    /// While the recorded total of the live stores is over `budget`, take the
    /// generations beyond `current`, the pinned and the last-deployed out of
    /// the store unused longest, then the next. Returns how many stores gave
    /// some up. A store is left alone while its folder has a serve owner, a
    /// build holds the folder's build lock or a publish holds a generation
    /// pinned; staging and the current generation are never touched.
    pub fn trim_live(&self, budget: u64) -> io::Result<usize> {
        let entries = match std::fs::read_dir(&self.record_root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let mut live: Vec<_> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| !name.starts_with('.') && !name.contains(".replaced-"))
            .filter_map(|name| {
                let rec = self.read(&name)?;
                // A folder away on another volume cannot say what it last deployed.
                let present = rec.path.is_dir();
                Some((modified(&self.record_root.join(&name).join(RECORD)), name, rec, present))
            })
            .collect();
        let mut total = self.recorded_total();
        live.sort_by_key(|(used, ..)| *used);
        let mut trimmed = 0;
        for (_, name, rec, present) in live {
            if total <= budget {
                break;
            }
            let Some(before) = store_size(&self.cache_root, &name) else { continue };
            if !present || in_use(&rec.path) {
                continue;
            }
            // Held through the delete: no seal can replace the current
            // generation, so a publish that starts now pins one that stays.
            let Ok(Some(_build)) = crate::infra::folder_lock::try_acquire_named(&rec.path, "build") else { continue };
            if crate::infra::folder_lock::publish_pinned(&rec.path) {
                continue;
            }
            // An owner may have arrived while the build lock was taken; one
            // arriving after this waits on that lock to build.
            if self.lock().map(|_held| in_use(&rec.path))? {
                continue;
            }
            let Ok(deployed) = crate::build::site_config::get_domain_config(&rec.path.to_string_lossy()) else { continue };
            let build_dir = self.cache_root.join(&name).join("build");
            if let Err(e) = store_gc::trim_to_current(&build_dir, deployed.last_deployed_generation_id.as_deref()) {
                log::info!("store trim stopped for {} ({e})", rec.path.display());
                continue;
            }
            total = total.saturating_sub(before.saturating_sub(store_size(&self.cache_root, &name).unwrap_or(0)));
            trimmed += 1;
        }
        Ok(trimmed)
    }
}
