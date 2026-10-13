use super::*;
use crate::system::derive::test_support::{entry, script, DERIVED};
use crate::system::derive::lookup_cached;
use tempfile::TempDir;

#[test]
fn the_budget_is_between_one_and_four() {
    assert!((1..=4).contains(&derive_budget()));
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
