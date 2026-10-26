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

fn drain(schedule: &Schedule<'_>, mut waiting: Vec<usize>) -> Vec<usize> {
    let mut order = Vec::new();
    while !waiting.is_empty() {
        order.push(waiting.remove(schedule.pick(&waiting)));
    }
    order
}

#[test]
fn the_viewed_page_comes_first_then_what_arrived_then_the_rest_in_order() {
    let (dir, pages, arrived) = fixture();
    note_viewed(dir.path(), "/a");
    let schedule = Schedule::new(dir.path(), pages.iter().map(Vec::as_slice).collect(), arrived);
    assert_eq!(drain(&schedule, vec![0, 1, 2]), vec![2, 1, 0]);
}

#[test]
fn without_a_viewed_page_arrivals_still_go_first() {
    let (dir, pages, arrived) = fixture();
    let schedule = Schedule::new(dir.path(), pages.iter().map(Vec::as_slice).collect(), arrived);
    assert_eq!(drain(&schedule, vec![0, 1, 2]), vec![1, 0, 2]);
}

#[test]
fn navigating_moves_the_new_pages_files_ahead_of_what_has_not_started() {
    let (dir, pages, arrived) = fixture();
    note_viewed(dir.path(), "/a");
    let schedule = Schedule::new(dir.path(), pages.iter().map(Vec::as_slice).collect(), arrived);
    let mut waiting = vec![0, 1, 2];
    assert_eq!(waiting.remove(schedule.pick(&waiting)), 2);
    note_viewed(dir.path(), "/b");
    // Both of b's files now lead, in job order; the one that started stays started.
    assert_eq!(waiting, vec![0, 1]);
    assert_eq!(schedule.tier(0), Tier::Viewed);
    assert_eq!(drain(&schedule, waiting), vec![0, 1]);
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
    let results = derive_batch_by(&site, &jobs, false, &|| false, &|_| {
        // Every pick is a navigation to something else.
        note_viewed(dir.path(), &format!("/{}", navigations.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        0
    }, |_, _| {});
    assert!(results.iter().all(|r| r.as_ref().is_some_and(|r| r.is_ok())), "every started read finished");
}
