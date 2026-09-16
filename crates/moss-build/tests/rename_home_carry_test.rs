//! Tests for folder rename carrying a self-named home file (Task 3.1).
//!
//! When a FOLDER is renamed and its home file is CURRENTLY SELF-NAMED
//! (stem == OLD folder name, case-insensitive, lang-suffix aware), the home
//! file is also renamed to match the NEW folder name. This is best-effort:
//! errors are warned but never cause the rename to fail.
//!
//! Index/README/non-self-named homes are NOT renamed.

fn test_tmp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// Self-named home: `游记/游记.md` → after rename dir to `旅行/`, the home
/// becomes `旅行/旅行.md`; old name `旅行/游记.md` no longer exists.
#[test]
fn folder_rename_carries_self_named_home() {
    let root = test_tmp();

    // Create `游记/` with `游记.md` and a sibling file.
    let folder = root.path().join("游记");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("游记.md"), "# 游记\n").unwrap();
    std::fs::write(folder.join("章节一.md"), "# 章节一\n").unwrap();

    let old_abs = folder.to_string_lossy().to_string();
    let new_abs = root.path().join("旅行").to_string_lossy().to_string();

    moss_build::vault::fs::rename_entry_inner(root.path(), &old_abs, &new_abs).unwrap();

    let new_folder = root.path().join("旅行");
    // Home file was renamed
    assert!(
        new_folder.join("旅行.md").exists(),
        "expected 旅行/旅行.md to exist after rename"
    );
    // Old home name is gone
    assert!(
        !new_folder.join("游记.md").exists(),
        "expected 旅行/游记.md to be gone after rename"
    );
    // Sibling untouched
    assert!(new_folder.join("章节一.md").exists());
}

/// index.md stays index.md — not renamed to new folder name.
#[test]
fn folder_rename_leaves_index_home_untouched() {
    let root = test_tmp();

    let folder = root.path().join("游记");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("index.md"), "# index\n").unwrap();

    let old_abs = folder.to_string_lossy().to_string();
    let new_abs = root.path().join("旅行").to_string_lossy().to_string();

    moss_build::vault::fs::rename_entry_inner(root.path(), &old_abs, &new_abs).unwrap();

    let new_folder = root.path().join("旅行");
    assert!(new_folder.join("index.md").exists(), "index.md must stay");
    assert!(
        !new_folder.join("旅行.md").exists(),
        "no 旅行.md must be created"
    );
}

/// A non-self-named home (intro.md) is left exactly as-is.
#[test]
fn folder_rename_leaves_non_self_named_home_untouched() {
    let root = test_tmp();

    let folder = root.path().join("游记");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("intro.md"), "# intro\n").unwrap();

    let old_abs = folder.to_string_lossy().to_string();
    let new_abs = root.path().join("旅行").to_string_lossy().to_string();

    moss_build::vault::fs::rename_entry_inner(root.path(), &old_abs, &new_abs).unwrap();

    let new_folder = root.path().join("旅行");
    assert!(new_folder.join("intro.md").exists(), "intro.md must stay");
    assert!(
        !new_folder.join("旅行.md").exists(),
        "no 旅行.md must be created"
    );
}

/// Renaming a plain FILE (not a directory) must not trigger home logic.
#[test]
fn file_rename_does_not_trigger_home_logic() {
    let root = test_tmp();

    std::fs::write(root.path().join("old.md"), "# old\n").unwrap();

    let old_abs = root.path().join("old.md").to_string_lossy().to_string();
    let new_abs = root.path().join("new.md").to_string_lossy().to_string();

    moss_build::vault::fs::rename_entry_inner(root.path(), &old_abs, &new_abs).unwrap();

    assert!(root.path().join("new.md").exists());
    assert!(!root.path().join("old.md").exists());
}
