//! Failures while applying or undoing a rename: a page write that fails, a
//! rollback that cannot finish. The writer is passed in so a full disk or a
//! refused write can be staged without one.

use super::*;
use crate::editor::rename_plan::{plan_moves, AppliedEdit, PlannedMove};
use std::fs;

fn w(root: &Path, rel: &str, content: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

fn r(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn snapshot_tree(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    walk_all_files(root).into_iter().map(|p| { let b = fs::read(root.join(&p)).unwrap(); (p, b) }).collect()
}

#[test]
fn a_failed_rollback_names_the_paths_it_left_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    w(root, "pic.jpg", "x");
    w(root, "photo.jpg", "someone recreated the old name");
    w(root, "page.md", "![](pic.jpg)\n");
    // The old name is taken again, so moving pic.jpg back cannot succeed.
    let applied = RenameApplyResult {
        moves: vec![PlannedMove { old_path: "photo.jpg".into(), new_path: "pic.jpg".into(), is_dir: false }],
        edits: vec![AppliedEdit { file: "page.md".into(), byte_from: 4, byte_to: 11, old_text: "photo.jpg".into(), new_text: "pic.jpg".into() }],
        skipped: vec![],
    };

    let err = roll_back(root, &applied, &WriteFailure { cause: "boom".to_string(), damaged: Vec::new() });

    assert!(err.contains("boom"), "{err}");
    assert!(err.contains("rollback failed"), "{err}");
    assert!(err.contains("[pic.jpg]"), "names the entry still at its new path: {err}");
    assert!(err.contains("page.md"), "names the page still rewritten: {err}");
}

// ── A page write that fails after truncating the page ────────────────────

/// A page writer that cuts the file to half its new length, then fails, the
/// way a full disk does. After `heal_after` failures it writes normally.
fn truncating_writer(heal_after: usize) -> impl Fn(&Path, &str) -> std::io::Result<()> {
    let failures = std::cell::Cell::new(0usize);
    move |path, text| {
        if failures.get() >= heal_after {
            return fs::write(path, text);
        }
        failures.set(failures.get() + 1);
        fs::write(path, &text[..text.len() / 2])?;
        Err(std::io::Error::other("no space left on device"))
    }
}

/// A page writer that writes the first page and refuses every later write
/// without touching the file, the way a read-only file does.
fn refusing_after_first_writer() -> impl Fn(&Path, &str) -> std::io::Result<()> {
    let writes = std::cell::Cell::new(0usize);
    move |path, text| {
        writes.set(writes.get() + 1);
        if writes.get() == 1 {
            return fs::write(path, text);
        }
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }
}

fn two_pages_naming_a_photo(root: &Path) -> RenamePlan {
    w(root, "media/photo.jpg", "x");
    w(root, "a1.md", "![](media/photo.jpg)\n");
    w(root, "a2.md", "![](media/photo.jpg)\n");
    plan_moves(
        root,
        &[(root.join("media/photo.jpg").to_string_lossy().into(), root.join("media/pic.jpg").to_string_lossy().into())],
    )
    .expect("plan")
}

#[test]
fn a_page_whose_write_fails_partway_is_restored_from_the_text_in_hand() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    let plan = two_pages_naming_a_photo(root);
    let before = snapshot_tree(root);

    let err = apply_planned_moves_with(root, &plan, &truncating_writer(1)).expect_err("write fails");

    assert!(err.contains("no space left"), "{err}");
    assert_eq!(snapshot_tree(root), before, "the truncated page is written back, the rest rolled back");
}

#[test]
fn a_page_that_cannot_be_restored_is_named_as_possibly_damaged() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    let plan = two_pages_naming_a_photo(root);

    let err = apply_planned_moves_with(root, &plan, &truncating_writer(usize::MAX)).expect_err("write fails");

    let truncated = ["a1.md", "a2.md"].into_iter().find(|p| r(root, p).len() < 20).expect("one page was cut");
    assert!(err.contains(truncated) && err.contains("damaged"), "names {truncated}: {err}");
    assert!(!err.contains("nothing was left changed"), "{err}");
}

fn applied_two_pages(root: &Path) -> RenameApplyResult {
    let plan = two_pages_naming_a_photo(root);
    apply_planned_moves(root, &plan).expect("apply")
}

#[test]
fn an_undo_whose_page_write_fails_partway_restores_the_page_from_the_text_in_hand() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    let applied = applied_two_pages(root);
    let renamed = snapshot_tree(root);

    let err = undo_applied_with(root, &applied, &truncating_writer(1)).expect_err("write fails");

    assert!(err.contains("no space left"), "{err}");
    for page in ["a1.md", "a2.md"] {
        assert_eq!(r(root, page).as_bytes(), renamed[page].as_slice(), "{page} must not be left cut");
    }
}

#[test]
fn an_undo_page_that_cannot_be_restored_is_named_as_possibly_damaged() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    let applied = applied_two_pages(root);

    let err = undo_applied_with(root, &applied, &truncating_writer(usize::MAX)).expect_err("write fails");

    let truncated = ["a1.md", "a2.md"].into_iter().find(|p| r(root, p).len() <= 10).expect("one page was cut");
    assert!(err.contains(truncated) && err.contains("damaged"), "names {truncated}: {err}");
}

#[test]
fn an_undo_that_stops_partway_names_the_pages_it_already_restored() {
    let tmp = tempfile::tempdir().unwrap();
    let root = &fs::canonicalize(tmp.path()).unwrap();
    let applied = applied_two_pages(root);

    let err = undo_applied_with(root, &applied, &refusing_after_first_writer()).expect_err("second write refused");

    let restored: Vec<&str> =
        ["a1.md", "a2.md"].into_iter().filter(|p| r(root, p) == "![](media/photo.jpg)\n").collect();
    assert_eq!(restored.len(), 1, "exactly one page was written back");
    assert!(err.contains(&format!("[{}]", restored[0])), "names {}: {err}", restored[0]);
}
