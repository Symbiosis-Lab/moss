//! A file in tests/it that main.rs does not declare compiles nowhere, so its
//! tests silently stop running; this fails on that.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn stems(dir: &PathBuf) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".rs").map(str::to_owned))
        .collect()
}

#[test]
fn every_file_in_tests_it_is_declared_in_main() {
    let it = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/it");
    let main = fs::read_to_string(it.join("main.rs")).unwrap();
    let declared: BTreeSet<String> = main
        .lines()
        .filter_map(|l| l.strip_prefix("mod ")?.strip_suffix(';'))
        .map(str::to_owned)
        .collect();
    let mut on_disk = stems(&it);
    on_disk.remove("main");
    assert_eq!(declared, on_disk, "tests/it/main.rs and the directory disagree");
}
