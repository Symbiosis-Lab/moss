//! Crosses the REAL rename_entry FS-boundary (no mocks) to lock the bug class
//! that survived mocked unit tests. The existing TS tests
//! mock `renameEntry` and assert on the relative-path argument, baking in the
//! buggy contract; these tests exercise the real path-validation + `fs::rename`
//! core directly.
//!
//! A relative path must be rejected with the project-directory error; an
//! absolute in-project path must succeed and move the file on disk.
//!
//! Target: `moss_build::vault::fs::rename_entry_inner` — the core the app's
//! `rename_entry` command delegates to after resolving the project root. The real error string is
//! `"Paths must be within the project directory"`.
//!
//! `validate_entry_path` itself is unit-tested with the code it belongs to, in
//! `src/vault/fs_tests.rs` — five duplicate copies of those cases lived here
//! until 2026-08-06. What this file still guards is the command's USE of it:
//! that `rename_entry_inner` actually consults the boundary before touching the
//! filesystem.

/// A relative old-path is the original bug input: it does not start with the
/// absolute project root, so the boundary check must reject it rather than
/// letting `fs::rename` operate relative to the process cwd (escape).
#[test]
fn rename_entry_rejects_relative_path_with_project_dir_error() {
    let tmp = std::env::temp_dir().join(format!("moss_rename_bnd_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("post.md"), "# Hi\n").unwrap();

    // Relative old-path (the bug input) -> must error, not escape the project.
    let err = moss_build::vault::fs::rename_entry_inner(tmp.as_path(), "post.md", "renamed.md")
        .unwrap_err();
    assert!(
        err.contains("within the project directory"),
        "got: {err}"
    );

    // The file must NOT have been renamed.
    assert!(tmp.join("post.md").exists());
    std::fs::remove_dir_all(&tmp).ok();
}

/// An absolute in-project path is the fixed contract: it starts with the
/// project root, passes the boundary check, and the file moves on disk.
#[test]
fn rename_entry_accepts_absolute_in_project_path_and_moves_file() {
    let tmp = std::env::temp_dir().join(format!("moss_rename_bnd_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("post.md"), "# Hi\n").unwrap();
    let old_abs = tmp.join("post.md").to_string_lossy().to_string();
    let new_abs = tmp.join("renamed.md").to_string_lossy().to_string();

    moss_build::vault::fs::rename_entry_inner(tmp.as_path(), &old_abs, &new_abs).unwrap();
    assert!(!tmp.join("post.md").exists());
    assert!(tmp.join("renamed.md").exists());
    std::fs::remove_dir_all(&tmp).ok();
}

// ── rename-with-refs: shortcode + frontmatter asset paths ─────────────────
//
// The other REAL FS boundary this file guards: `rename_entry_with_refs_core`
// is the shared core behind both the `rename_entry_with_refs` command and
// `moss rename`. The fixture mirrors the riverbend/河灣 corpus shape — a
// gallery of bare CJK directory-relative paths in a subfolder — because that
// is where the reported data loss happened.

/// A temp project under the repo-local `target/test-tmp` base.
fn ref_tmp_project(tag: &str) -> std::path::PathBuf {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let base = std::fs::canonicalize(&base).unwrap();
    let dir = base.join(format!("rename_refs_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn w(root: &std::path::Path, rel: &str, content: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}

const RIVERBEND_DOC: &str = "---\n\
title: 河灣\n\
cover: 關於/頭像-顧海棠.png     # keep this comment\n\
---\n\
\n\
:::hero {image=\"關於/頭像-顧海棠.png\"}\n\
Overlay copy\n\
:::\n\
\n\
:::gallery 8 {.profiles}\n\
關於/頭像-顧海棠.png\n\
關於/頭像-顧海棠.png|cover top\n\
![](關於/頭像-顧海棠.png)\n\
![[關於/頭像-顧海棠.png]]\n\
關於/頭像-顧海清.png\n\
關於/頭像-顧海棠.jpg\n\
:::\n\
\n\
```\n\
關於/頭像-顧海棠.png\n\
```\n";

fn riverbend_fixture(tag: &str) -> std::path::PathBuf {
    let root = ref_tmp_project(tag);
    w(&root, "articles/河灣.md", RIVERBEND_DOC);
    for name in ["頭像-顧海棠.png", "頭像-顧海清.png", "頭像-顧海棠.jpg"] {
        w(&root, &format!("articles/關於/{name}"), "fake-bytes");
    }
    root
}

#[test]
fn rename_rewrites_every_shortcode_and_frontmatter_asset_path() {
    let root = riverbend_fixture("rename");
    let old = root.join("articles/關於/頭像-顧海棠.png");
    let new = root.join("articles/關於/avatar-gu.png");

    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &old.to_string_lossy(),
        &new.to_string_lossy(),
    )
    .expect("rename with refs");

    let out = std::fs::read_to_string(root.join("articles/河灣.md")).unwrap();

    // 1. All six live references, each in its own syntax, now point at the
    //    new name — document-relative, exactly as authored.
    assert_eq!(
        out.matches("關於/avatar-gu.png").count(),
        6,
        "six live refs should be rewritten, got:\n{out}"
    );
    for expected in [
        "cover: 關於/avatar-gu.png",
        "{image=\"關於/avatar-gu.png\"}",
        "\n關於/avatar-gu.png\n",
        "關於/avatar-gu.png|cover top",
        "![](關於/avatar-gu.png)",
        "![[關於/avatar-gu.png]]",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }

    // 2. A different file, and a same-stem DIFFERENT-extension file, and the
    //    fenced line, are all byte-identical.
    assert!(out.contains("關於/頭像-顧海清.png"), "unrelated file untouched");
    assert!(
        out.contains("關於/頭像-顧海棠.jpg"),
        "same stem, different extension is a DIFFERENT file:\n{out}"
    );
    assert!(
        out.contains("```\n關於/頭像-顧海棠.png\n```"),
        "a path inside a code fence is not a reference:\n{out}"
    );

    // 3. The frontmatter is otherwise byte-identical — key order, spacing,
    //    and the YAML comment survive (nothing was re-serialized).
    assert!(
        out.contains("cover: 關於/avatar-gu.png     # keep this comment"),
        "frontmatter round-trip:\n{out}"
    );
    assert!(out.starts_with("---\ntitle: 河灣\n"));

    // 4. Round-trip fidelity: the file's byte length changed by exactly
    //    6 × (len(new) − len(old)). Fails loudly if anything re-serialized.
    let delta = "avatar-gu.png".len() as isize - "頭像-顧海棠.png".len() as isize;
    assert_eq!(
        out.len() as isize - RIVERBEND_DOC.len() as isize,
        6 * delta,
        "only the six reference spans changed"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn rename_with_refs_skips_ambiguous_bare_asset_name_end_to_end() {
    // A second `頭像-顧海棠.png` elsewhere in the project makes `name_unique`
    // false, so the BARE gallery lines are left alone — a bare file name no
    // longer identifies one file. Path-qualified refs are unaffected here
    // because these refs are document-relative bare-ish paths.
    let root = riverbend_fixture("ambiguous");
    w(&root, "elsewhere/頭像-顧海棠.png", "fake-bytes");
    w(&root, "gallery.md", ":::gallery\n頭像-顧海棠.png\n:::\n");

    let old = root.join("articles/關於/頭像-顧海棠.png");
    let new = root.join("articles/關於/avatar-gu.png");
    moss_build::editor::ref_scan::rename_entry_with_refs_core(
        root.clone(),
        &old.to_string_lossy(),
        &new.to_string_lossy(),
    )
    .expect("rename with refs");

    let out = std::fs::read_to_string(root.join("gallery.md")).unwrap();
    assert_eq!(
        out, ":::gallery\n頭像-顧海棠.png\n:::\n",
        "an ambiguous bare file name must not be rewritten"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn clean_references_removes_shortcode_and_frontmatter_asset_paths() {
    let root = riverbend_fixture("clean");
    let target = root.join("articles/關於/頭像-顧海棠.png");

    moss_build::editor::ref_scan::clean_references_to_paths(&root, &[target.to_string_lossy().to_string()])
        .expect("clean refs");

    let out = std::fs::read_to_string(root.join("articles/河灣.md")).unwrap();
    // The only surviving occurrence is the one inside the code fence, which
    // was never a reference.
    assert_eq!(
        out.matches("頭像-顧海棠.png").count(),
        1,
        "every LIVE reference to the target is gone:\n{out}"
    );
    assert!(
        out.contains("```\n關於/頭像-顧海棠.png\n```"),
        "a path inside a code fence is not a reference:\n{out}"
    );
    // The gallery lines vanish whole — no blank residue, no orphan `|attrs`.
    assert!(!out.contains("|cover top"), "orphan attrs left behind:\n{out}");
    assert!(!out.contains("cover:"), "the whole cover: line is removed:\n{out}");
    assert!(
        out.contains("關於/頭像-顧海清.png") && out.contains("關於/頭像-顧海棠.jpg"),
        "untouched entries survive:\n{out}"
    );

    std::fs::remove_dir_all(&root).ok();
}
