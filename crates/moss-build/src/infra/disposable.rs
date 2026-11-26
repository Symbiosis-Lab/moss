//! Collectors for machine stores that keep one entry per version: a pinned
//! asset digest, a deriver version. Different folders, and different moss
//! versions on one machine, can each pin a different entry, so an entry goes
//! when nothing has resolved it for a while, never because a newer one
//! arrived. Losing one costs a download, never work.

use std::path::Path;
use std::time::{Duration, SystemTime};

/// How long an entry nothing resolves is kept.
pub const UNUSED_FOR: Duration = Duration::from_secs(30 * 24 * 3600);

const DAY: Duration = Duration::from_secs(24 * 3600);

/// Records that `entry` is being resolved, by its own modification time, before
/// it is used, so a sweep that claims it afterwards sees it fresh. At most once
/// a day, so a build does not touch the store on every page.
pub fn mark_used(entry: &Path) {
    if last_used(entry).is_some_and(|t| t.elapsed().is_ok_and(|age| age < DAY)) {
        return;
    }
    let _ = filetime::set_file_mtime(entry, filetime::FileTime::now());
}

fn last_used(entry: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(entry).and_then(|m| m.modified()).ok()
}

/// Removes each entry directly in `parent` that `candidate` accepts by name
/// and that nothing has used for `idle`, returning how many went. `claim`
/// takes the entry's own lock without waiting; an entry it cannot claim is
/// being installed or published and stays. Never descends into an entry
/// `candidate` refuses, so a sweep touches only its own store.
pub fn collect_unused<G>(
    parent: &Path,
    idle: Duration,
    candidate: impl Fn(&str) -> bool,
    claim: impl Fn(&Path) -> Option<G>,
) -> usize {
    let Ok(entries) = std::fs::read_dir(parent) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !candidate(name) {
            continue;
        }
        let path = entry.path();
        let stale = last_used(&path).is_some_and(|t| t.elapsed().is_ok_and(|age| age >= idle));
        if !stale {
            continue;
        }
        let Some(_claimed) = claim(&path) else { continue };
        // Read again under the claim: a resolve marks the entry before using it.
        if !last_used(&path).is_some_and(|t| t.elapsed().is_ok_and(|age| age >= idle)) {
            continue;
        }
        let gone = if entry.file_type().is_ok_and(|t| t.is_dir()) {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if gone.is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        log::info!("removed {removed} unused entries from {}", parent.display());
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn age(path: &Path, days: u64) {
        let then = SystemTime::now() - Duration::from_secs(days * 24 * 3600);
        filetime::set_file_mtime(path, filetime::FileTime::from_system_time(then)).unwrap();
    }

    #[test]
    fn an_unused_entry_goes_and_a_recently_used_one_stays() {
        let root = tempfile::tempdir().unwrap();
        for name in ["old", "recent", "other-store"] {
            std::fs::create_dir_all(root.path().join(name).join("bundle")).unwrap();
            age(&root.path().join(name), 40);
        }
        mark_used(&root.path().join("recent"));

        let removed = collect_unused(root.path(), UNUSED_FOR, |n| n != "other-store", |_| Some(()));

        assert_eq!(removed, 1);
        assert!(!root.path().join("old").exists(), "an entry unused for 40 days goes");
        assert!(root.path().join("recent").exists(), "an entry used today stays");
        assert!(root.path().join("other-store").exists(), "a name the store does not own is never touched");
    }

    #[test]
    fn an_entry_that_cannot_be_claimed_stays() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("installing")).unwrap();
        age(&root.path().join("installing"), 40);

        assert_eq!(collect_unused(root.path(), UNUSED_FOR, |_| true, |_| None::<()>), 0);
        assert!(root.path().join("installing").exists());
    }

    #[test]
    fn an_entry_resolved_while_the_sweep_claims_it_stays() {
        let root = tempfile::tempdir().unwrap();
        let entry = root.path().join("pin");
        std::fs::create_dir(&entry).unwrap();
        age(&entry, 40);

        let removed = collect_unused(root.path(), UNUSED_FOR, |_| true, |path| {
            mark_used(path);
            Some(())
        });

        assert_eq!(removed, 0);
        assert!(entry.exists(), "marked between the first look and the claim, so in use");
    }

    #[test]
    fn mark_used_skips_an_entry_used_within_the_day() {
        let root = tempfile::tempdir().unwrap();
        let entry = root.path().join("e");
        std::fs::create_dir(&entry).unwrap();
        let hour_ago = SystemTime::now() - Duration::from_secs(3600);
        filetime::set_file_mtime(&entry, filetime::FileTime::from_system_time(hour_ago)).unwrap();

        mark_used(&entry);

        let after = std::fs::metadata(&entry).unwrap().modified().unwrap();
        assert!(after.elapsed().unwrap() >= Duration::from_secs(3000), "no write within the day");
    }
}
