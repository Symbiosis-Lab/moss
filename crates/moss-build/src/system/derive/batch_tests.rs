use super::*;
use crate::system::derive::test_support::{entry, script, DERIVED};
use crate::system::derive::lookup_cached;
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
