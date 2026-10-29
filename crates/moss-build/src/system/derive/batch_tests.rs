use super::*;
use crate::system::derive::test_support::{entry, script, DERIVED};
use crate::system::derive::lookup_cached;
use crate::system::derive::priority::Tier;
use tempfile::TempDir;

#[test]
fn the_environment_sets_the_budget() {
    // The only test that touches the variable; the pool other tests share only needs a thread count.
    std::env::set_var("MOSS_DERIVE_JOBS", "7");
    let set = derive_budget();
    std::env::remove_var("MOSS_DERIVE_JOBS");
    assert_eq!(set, 7);
    assert!((1..=4).contains(&derive_budget()), "the default without it");
}

#[test]
fn a_batch_derives_each_file_and_reports_a_failure_beside_the_successes() {
    let dir = TempDir::new().unwrap();
    let site = MossPaths::new(&dir.path().join("site"));
    // Fails on the file named bad.xyz only.
    let bin = script(
        dir.path(),
        "d.sh",
        &format!("case \"$REQ\" in *bad.xyz*) echo broken >&2; exit 2;; esac\n{DERIVED}"),
    );
    let e = entry(&bin, "1.0.0");
    let names = ["a.xyz", "bad.xyz", "c.xyz"];
    let jobs: Vec<_> = names
        .iter()
        .map(|n| {
            let p = dir.path().join(n);
            std::fs::write(&p, n).unwrap();
            DeriveJob { entry: &e, input: p }
        })
        .collect();
    let seen = Mutex::new(Vec::new());
    let results = derive_batch(&site, &jobs, false, &|| false, |done, total| seen.lock().unwrap().push((done, total)));
    assert_eq!(*seen.lock().unwrap(), vec![(1, 3), (2, 3), (3, 3)]);
    let results: Vec<_> = results.into_iter().map(|r| r.expect("not stopped")).collect();
    assert!(results[0].is_ok() && results[2].is_ok());
    assert!(results[1].as_ref().unwrap_err().message.contains("broken"));
}

#[test]
fn a_stopped_batch_starts_nothing_and_reports_nothing() {
    let dir = TempDir::new().unwrap();
    let site = MossPaths::new(&dir.path().join("site"));
    let e = entry(&script(dir.path(), "d.sh", DERIVED), "1.0.0");
    let input = dir.path().join("a.xyz");
    std::fs::write(&input, "a").unwrap();
    let jobs = vec![DeriveJob { entry: &e, input }];
    let results = derive_batch(&site, &jobs, false, &|| true, |_, _| panic!("nothing ran"));
    assert!(results.iter().all(Option::is_none));
    assert!(lookup_cached(&site, &e, &jobs[0].input).is_none());
}

#[test]
fn derive_jobs_overrides_the_default_within_one_to_sixty_four_and_anything_else_is_ignored() {
    // The default: half the cores, between one and four.
    assert_eq!(budget_from(None, 16), 4);
    assert_eq!(budget_from(None, 4), 2);
    assert_eq!(budget_from(None, 1), 1);
    assert_eq!(budget_from(Some("1"), 16), 1);
    assert_eq!(budget_from(Some(" 12 "), 2), 12);
    assert_eq!(budget_from(Some("64"), 2), 64);
    for ignored in ["0", "65", "-3", "many", "", "2.5"] {
        assert_eq!(budget_from(Some(ignored), 16), 4, "{ignored:?}");
    }
}

#[test]
fn a_batch_that_arrives_mid_read_is_ordered_against_what_has_not_started() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = TempDir::new().unwrap();
    let site = MossPaths::new(&dir.path().join("site"));
    let slow = script(dir.path(), "d.sh", &format!("sleep 0.3\n{DERIVED}"));
    let e = entry(&slow, "1.0.0");
    let make = |prefix: &str, n: usize| -> Vec<DeriveJob<'_>> {
        (0..n)
            .map(|i| {
                let input = dir.path().join(format!("{prefix}{i}.xyz"));
                std::fs::write(&input, format!("{prefix}{i}")).unwrap();
                DeriveJob { entry: &e, input }
            })
            .collect()
    };
    let (a_jobs, b_jobs) = (make("a", 24), make("b", 1));
    let a_done = AtomicUsize::new(0);
    let a_done_when_b_returned = std::thread::scope(|s| {
        s.spawn(|| {
            derive_batch_by(&site, &a_jobs, false, &|| false, Arc::new(|_| Tier::Rest), |done, _| a_done.store(done, Ordering::SeqCst))
        });
        std::thread::sleep(std::time::Duration::from_millis(150));
        let b = derive_batch_by(&site, &b_jobs, false, &|| false, Arc::new(|_| Tier::Viewed), |_, _| {});
        assert!(b[0].as_ref().is_some_and(|r| r.is_ok()));
        a_done.load(Ordering::SeqCst)
    });
    assert!(a_done_when_b_returned < 24, "the late batch's file did not wait for the whole first batch");
    assert_eq!(a_done.load(Ordering::SeqCst), 24, "the first batch still finished every read");
}
