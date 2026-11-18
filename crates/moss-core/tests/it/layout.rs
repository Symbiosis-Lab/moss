//! A file in tests/it that main.rs does not declare compiles nowhere, so its
//! tests silently stop running; a top-level tests/*.rs links a whole binary of
//! its own. Both fail here until main.rs names them.
//! moss-build and moss-cli include this file by path, so the three crates share one copy.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn stems(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".rs").map(str::to_owned))
        .collect()
}

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

#[test]
fn every_file_in_tests_it_is_declared_in_main() {
    let it = tests_dir().join("it");
    let main = fs::read_to_string(it.join("main.rs")).unwrap();
    let mut declared: BTreeSet<String> = main
        .lines()
        .filter_map(|l| l.strip_prefix("mod ")?.strip_suffix(';'))
        .map(str::to_owned)
        .collect();
    let mut on_disk = stems(&it);
    on_disk.remove("main");
    // moss-build and moss-cli declare this file under a `#[path]` attribute and hold no copy.
    declared.remove("layout");
    on_disk.remove("layout");
    assert_eq!(declared, on_disk, "tests/it/main.rs and the directory disagree");
}

#[test]
fn every_top_level_test_file_is_named_as_its_own_process() {
    let named: BTreeSet<String> = crate::OWN_PROCESS.iter().map(|s| s.to_string()).collect();
    let on_disk = stems(&tests_dir());
    let unnamed: Vec<_> = on_disk.difference(&named).collect();
    let gone: Vec<_> = named.difference(&on_disk).collect();
    assert!(
        unnamed.is_empty() && gone.is_empty(),
        "top-level tests/*.rs not in OWN_PROCESS: {unnamed:?} (each is a binary of its own: move it into tests/it, or name it in tests/it/main.rs if it sets process-wide state); named with no file: {gone:?}"
    );
}
