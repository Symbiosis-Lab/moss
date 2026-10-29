use super::*;
use tempfile::TempDir;

/// A folder with pages `a.md` and `b.md`, and three jobs: 0 re-read, 1 arrived,
/// 2 re-read and embedded by `a.md`.
fn fixture() -> (TempDir, Vec<Vec<String>>, Vec<bool>) {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.md"), "a").unwrap();
    std::fs::write(dir.path().join("b.md"), "b").unwrap();
    let pages = vec![vec!["b.md".to_string()], vec!["b.md".to_string()], vec!["a.md".to_string()]];
    (dir, pages, vec![false, true, false])
}

fn schedule(dir: &TempDir, pages: Vec<Vec<String>>, arrived: Vec<bool>) -> Schedule {
    Schedule::new(dir.path(), pages, arrived)
}

#[test]
fn the_viewed_page_comes_first_then_what_arrived_then_the_rest() {
    let (dir, pages, arrived) = fixture();
    note_viewed(dir.path(), "/a");
    let s = schedule(&dir, pages, arrived);
    assert_eq!([s.tier(2), s.tier(1), s.tier(0)], [Tier::Viewed, Tier::Arrived, Tier::Rest]);
}

#[test]
fn without_a_viewed_page_arrivals_still_go_first() {
    let (dir, pages, arrived) = fixture();
    let s = schedule(&dir, pages, arrived);
    assert_eq!([s.tier(0), s.tier(1), s.tier(2)], [Tier::Rest, Tier::Arrived, Tier::Rest]);
}

#[test]
fn navigating_changes_which_files_lead() {
    let (dir, pages, arrived) = fixture();
    note_viewed(dir.path(), "/a");
    let s = schedule(&dir, pages, arrived);
    assert_eq!([s.tier(0), s.tier(2)], [Tier::Rest, Tier::Viewed]);
    note_viewed(dir.path(), "/b");
    assert_eq!([s.tier(0), s.tier(1), s.tier(2)], [Tier::Viewed, Tier::Viewed, Tier::Rest]);
}

#[test]
fn a_navigation_while_reads_run_never_cancels_them() {
    use crate::system::derive::batch::{derive_batch_by, DeriveJob};
    use crate::system::derive::test_support::{entry, script};
    let dir = TempDir::new().unwrap();
    let slow = script(dir.path(), "d.sh", &format!("sleep 0.3\n{}", crate::system::derive::test_support::DERIVED));
    let e = entry(&slow, "1.0.0");
    let jobs: Vec<_> = (0..3)
        .map(|i| {
            let input = dir.path().join(format!("{i}.xyz"));
            std::fs::write(&input, i.to_string()).unwrap();
            DeriveJob { entry: &e, input }
        })
        .collect();
    let site = crate::moss_paths::MossPaths::new(&dir.path().join("site"));
    let navigations = std::sync::atomic::AtomicUsize::new(0);
    let nav_dir = dir.path().to_path_buf();
    let results = derive_batch_by(&site, &jobs, false, &|| false, std::sync::Arc::new(move |_| {
        // Every ranking is a navigation to something else.
        note_viewed(&nav_dir, &format!("/{}", navigations.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        Tier::Rest
    }), |_, _| {});
    assert!(results.iter().all(|r| r.as_ref().is_some_and(|r| r.is_ok())), "every started read finished");
}
