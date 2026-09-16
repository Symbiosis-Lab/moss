use super::*;
use std::fs;

fn tmp(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "moss_vault_paths_{label}_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

// ---- the contract: plain absolute paths are untouched ----

#[test]
fn plain_absolute_path_passes_through_byte_identical() {
    // macOS temp dirs are symlinks (/var → /private/var). A resolver that
    // canonicalized unconditionally would rewrite this path and silently change
    // recents entries, window titles and session keys for every symlinked vault.
    let site = tmp("plain").join("潮汐");
    fs::create_dir_all(&site).unwrap();
    let root = VaultRoot::resolve(&site);
    assert_eq!(root.as_str(), site.to_string_lossy());
    assert_eq!(root.name(), "潮汐");
}

#[test]
fn trailing_separator_is_dropped_without_touching_the_filesystem() {
    let site = tmp("slash").join("blog");
    fs::create_dir_all(&site).unwrap();
    let with_slash = format!("{}/", site.to_string_lossy());
    let root = VaultRoot::resolve(&with_slash);
    assert_eq!(
        root.as_str(),
        site.to_string_lossy(),
        "trailing slash must not survive"
    );
    assert_eq!(root.name(), "blog");
}

// ---- the bug class ----

#[test]
fn dot_resolves_to_the_real_directory_name_without_touching_the_filesystem() {
    let site = tmp("dot").join("潮汐");
    fs::create_dir_all(&site).unwrap();
    for raw in [".", "./", "潮汐"] {
        let cwd = if raw == "潮汐" {
            site.parent().unwrap().to_path_buf()
        } else {
            site.clone()
        };
        let root = VaultRoot::resolve_in(Path::new(raw), &cwd);
        assert_eq!(root.name(), "潮汐", "`{raw}` must name the real directory");
        // `.` is answered lexically — `Path::components()` already drops interior CurDir.
        // On macOS the temp dir is a symlink, so a canonicalize here would rewrite
        // `/var/...` to `/private/var/...` and change the recents entry / window title /
        // session key for every symlinked vault.
        assert_eq!(
            root.path(),
            site.as_path(),
            "`{raw}` must not rewrite the spelling"
        );
    }
}

#[test]
fn parent_dir_names_the_parent_not_the_folder_walked_out_of() {
    // `Path::new("/a/b/..").file_name()` is None and
    // `components().filter_map(Normal).last()` returns "b" — the folder the user
    // asked to LEAVE. Both are wrong; only the filesystem knows the answer, which is
    // why `..` is the one input that canonicalizes.
    let base = tmp("parent");
    let site = base.join("blog");
    fs::create_dir_all(&site).unwrap();
    let root = VaultRoot::resolve_in(Path::new(".."), &site);
    assert_eq!(
        root.name(),
        base.file_name().unwrap().to_string_lossy(),
        "`..` must name the parent directory"
    );
    // Compare PATHS against `canonicalize(&base)`, never against `base` — on macOS
    // `base` is `/var/folders/…` and the canonical form is `/private/var/folders/…`.
    // Only the `..` arm has this asymmetry.
    assert_eq!(root.path(), fs::canonicalize(&base).unwrap());
}

#[test]
fn nonexistent_relative_path_collapses_lexically_instead_of_staying_raw() {
    let base = tmp("missing");
    let root = VaultRoot::resolve_in(Path::new("./not-yet/../not-yet"), &base);
    assert_eq!(root.path(), base.join("not-yet"));
    assert_eq!(root.name(), "not-yet");
}

#[test]
fn filesystem_root_has_an_empty_name_exactly_as_before() {
    // Not a fallback: preserving today's value. Inventing one would change
    // home-election semantics (ADR territory, 05 row 54).
    let root = VaultRoot::resolve("/");
    assert_eq!(root.name(), "");
    assert_eq!(root.name_opt(), None);
}

#[test]
fn an_empty_input_stays_empty_instead_of_becoming_the_cwd() {
    // `""` is the ABSENCE of a folder path (an unset argument, a blank Tauri
    // command payload), not a relative path to the process CWD. `cwd.join("")`
    // is the CWD, so resolving it would make `build_sync("")` silently build
    // whatever directory moss happens to be running in — the exact
    // "builds a different site and never errors" class this module exists to kill.
    // The emptiness guard at `run_pipeline` (and its siblings) must keep firing.
    let cwd = tmp("empty_input");
    assert_eq!(resolve_input_in(Path::new(""), &cwd), PathBuf::new());
    let root = VaultRoot::resolve_in(Path::new(""), &cwd);
    assert_eq!(root.as_str(), "");
    assert_eq!(root.name(), "");
    assert_eq!(root.name_opt(), None);
}

// ---- containing(): the `.moss/` walk ----

#[test]
fn containing_finds_the_ancestor_owning_dot_moss() {
    let site = tmp("containing");
    fs::create_dir_all(site.join(".moss")).unwrap();
    fs::create_dir_all(site.join("works")).unwrap();
    fs::write(site.join("works/post.md"), "# Hi\n").unwrap();
    let root = VaultRoot::containing(&site.join("works/post.md"));
    assert_eq!(root.path(), site.as_path());
    assert_eq!(root.name(), site.file_name().unwrap().to_string_lossy());
}

#[test]
fn containing_does_not_redescend_through_a_dot_dot_argument() {
    // `Path::new("/a/b/..").parent()` is `/a/b`, so an ancestors() walk over an
    // unresolved `..` path re-enters the folder the user asked to leave. Resolving
    // FIRST is what terminates that.
    let base = tmp("redescend");
    let site = base.join("blog");
    fs::create_dir_all(site.join(".moss")).unwrap();
    let root = VaultRoot::containing_in(Path::new(".."), &site);
    assert_ne!(root.name(), "blog", "must not resolve back into `blog`");
    assert_eq!(root.path(), fs::canonicalize(&base).unwrap());
}

#[test]
fn containing_falls_back_to_the_directory_itself_when_no_marker_exists() {
    let site = tmp("nomarker").join("fresh");
    fs::create_dir_all(&site).unwrap();
    assert_eq!(VaultRoot::containing(&site).path(), site.as_path());
}

// ---- VaultTarget: root + optional in-vault file to open ----

#[test]
fn target_resolve_reports_the_file_relative_to_the_owning_root() {
    let site = tmp("target");
    fs::create_dir_all(site.join(".moss")).unwrap();
    fs::create_dir_all(site.join("works")).unwrap();
    fs::write(site.join("works/post.md"), "# Hi\n").unwrap();
    let t = VaultTarget::resolve(&site.join("works/post.md")).unwrap();
    assert_eq!(t.root.path(), site.as_path());
    assert_eq!(t.target_file.as_deref(), Some("works/post.md"));
}

#[test]
fn target_resolve_on_the_root_itself_has_no_target_file() {
    let site = tmp("target_root");
    fs::create_dir_all(site.join(".moss")).unwrap();
    let t = VaultTarget::resolve(&site).unwrap();
    assert_eq!(t.target_file, None);
}

#[test]
fn target_resolve_rejects_non_markdown_and_missing_paths() {
    let site = tmp("target_reject");
    fs::write(site.join("a.txt"), "x").unwrap();
    assert!(matches!(
        VaultTarget::resolve(&site.join("a.txt")),
        Err(VaultPathError::UnsupportedFileType(_))
    ));
    assert!(matches!(
        VaultTarget::resolve(&site.join("nope.md")),
        Err(VaultPathError::PathDoesNotExist(_))
    ));
}

// ---- RootKind: telling a real vault from the walk's fallback (ADR-038) ----

#[test]
fn find_containing_returns_none_when_no_ancestor_owns_dot_moss() {
    // The distinction `containing` cannot express. It answers with the start
    // directory here, which is right for a folder the user chose and wrong for a
    // file they merely double-clicked.
    let loose = tmp("find_none").join("fresh");
    fs::create_dir_all(&loose).unwrap();
    assert_eq!(VaultRoot::find_containing(&loose), None);
    assert_eq!(VaultRoot::containing(&loose).path(), loose.as_path());
}

#[test]
fn find_containing_agrees_with_containing_when_a_vault_exists() {
    // containing() is implemented in terms of find_containing(), so a hit must
    // produce the identical root — the two can never disagree.
    let site = tmp("find_some");
    fs::create_dir_all(site.join(".moss")).unwrap();
    fs::create_dir_all(site.join("posts")).unwrap();
    fs::write(site.join("posts/a.md"), "# A").unwrap();

    let found = VaultRoot::find_containing(&site.join("posts/a.md")).unwrap();
    assert_eq!(found.path(), site.as_path());
    assert_eq!(found, VaultRoot::containing(&site.join("posts/a.md")));
}

#[test]
fn loose_markdown_file_with_no_vault_is_a_document() {
    // The regression this exists for: `moss edit ~/Downloads/notes.md` resolved
    // its root to ~/Downloads and then built and served that entire folder.
    let downloads = tmp("loose_doc");
    fs::create_dir_all(&downloads).unwrap();
    fs::write(downloads.join("notes.md"), "# Notes").unwrap();

    let t = VaultTarget::resolve(&downloads.join("notes.md")).unwrap();
    assert_eq!(t.root_kind, RootKind::Unowned);
    assert!(
        t.is_loose_document(),
        "a file with no .moss/ ancestor is a document"
    );
}

#[test]
fn a_file_inside_a_vault_is_not_a_document() {
    // Belongs to a site, so it keeps its preview and its build.
    let site = tmp("in_vault_doc");
    fs::create_dir_all(site.join(".moss")).unwrap();
    fs::write(site.join("post.md"), "# Post").unwrap();

    let t = VaultTarget::resolve(&site.join("post.md")).unwrap();
    assert_eq!(t.root_kind, RootKind::Vault);
    assert!(!t.is_loose_document());
}

#[test]
fn a_folder_without_dot_moss_is_not_a_document() {
    // The asymmetry that keeps first-run intact: choosing a FOLDER is a
    // statement of intent about a site, so it still builds even with no .moss/.
    // Only the file case is a document.
    let fresh = tmp("fresh_folder_doc").join("newsite");
    fs::create_dir_all(&fresh).unwrap();

    let t = VaultTarget::resolve(&fresh).unwrap();
    assert_eq!(t.root_kind, RootKind::Unowned);
    assert_eq!(t.target_file, None);
    assert!(
        !t.is_loose_document(),
        "an unowned FOLDER is a new project, not a document — it must still build"
    );
}

#[test]
fn containing_never_walks_up_into_the_home_directory() {
    // Regression: right-clicking `~/repos/project/` (no `.moss/`) used to walk up and
    // find `~/.moss/` — moss's app-level cache, not a project marker — and then build
    // the entire home directory.
    let home = dirs::home_dir().expect("home dir must exist for this test");
    let project = home.join(format!("moss_test_no_walkup_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&project).unwrap();

    let root = VaultRoot::containing(&project);
    assert_ne!(root.path(), home.as_path(), "must NOT elect the home directory");
    assert_eq!(root.path(), project.as_path());

    let _ = fs::remove_dir_all(&project);
}

#[test]
fn containing_walks_up_from_a_file_to_the_marker() {
    let project = tmp("file_walkup");
    fs::create_dir_all(project.join(".moss")).unwrap();
    fs::create_dir_all(project.join("posts")).unwrap();
    fs::write(project.join("posts/article.md"), "# Test").unwrap();

    let root = VaultRoot::containing(&project.join("posts/article.md"));
    assert_eq!(root.path(), project.as_path());
}
