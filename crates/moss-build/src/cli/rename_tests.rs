use super::*;
use std::fs;

/// A folder that was never built (no `.moss/` anywhere above it) with a page
/// that links to `notes/beta.md` both ways.
fn unbuilt_site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    fs::create_dir_all(p.join("notes")).unwrap();
    fs::create_dir_all(p.join("blog")).unwrap();
    fs::write(p.join("index.md"), "# Home\n").unwrap();
    fs::write(p.join("notes/beta.md"), "# Beta\n").unwrap();
    fs::write(
        p.join("blog/post.md"),
        "# Post\n\n[p](../notes/beta.md) and [[beta]]\n",
    )
    .unwrap();
    dir
}

fn args(old: &str, new: &str) -> Vec<String> {
    vec![old.to_string(), new.to_string()]
}

#[test]
fn unbuilt_folder_rewrites_references_across_the_current_directory() {
    let site = unbuilt_site();
    let line = rename_in(&args("notes/beta.md", "notes/gamma.md"), site.path(), None).unwrap();

    let post = fs::read_to_string(site.path().join("blog/post.md")).unwrap();
    assert!(post.contains("../notes/gamma.md"), "{post}");
    assert!(post.contains("[[gamma]]"), "{post}");
    assert!(site.path().join("notes/gamma.md").exists());
    assert_eq!(
        line,
        "Renamed notes/beta.md → notes/gamma.md (2 references rewritten in 1 file)"
    );
}

#[test]
fn a_rename_that_touches_no_reference_says_so() {
    let site = unbuilt_site();
    let line = rename_in(&args("index.md", "home.md"), site.path(), None).unwrap();
    assert_eq!(line, "Renamed index.md → home.md (no references to rewrite)");
}

#[test]
fn owner_of_moss_folder_still_wins_over_current_directory() {
    let site = unbuilt_site();
    fs::create_dir_all(site.path().join(".moss")).unwrap();
    // cwd is a subfolder, yet the `.moss/` owner is the whole site.
    let line = rename_in(
        &args("beta.md", "gamma.md"),
        &site.path().join("notes"),
        None,
    )
    .unwrap();
    let post = fs::read_to_string(site.path().join("blog/post.md")).unwrap();
    assert!(post.contains("../notes/gamma.md"), "{post}");
    assert!(line.contains("2 references rewritten in 1 file"), "{line}");
}

#[test]
fn unbuilt_folder_refuses_a_path_outside_the_current_directory() {
    let site = unbuilt_site();
    let before = fs::read_to_string(site.path().join("blog/post.md")).unwrap();
    let cwd = site.path().join("notes");

    let err = rename_in(&args("beta.md", "../blog/beta.md"), &cwd, None).unwrap_err();

    assert!(err.contains("no site found"), "{err}");
    assert!(err.contains("moss build"), "{err}");
    assert!(site.path().join("notes/beta.md").exists());
    assert!(!site.path().join("blog/beta.md").exists());
    assert_eq!(fs::read_to_string(site.path().join("blog/post.md")).unwrap(), before);
}

/// What a refused `moss rename` tells the user to do: the guard runs only when
/// no folder above the file has a `.moss/`, so the file is in no site yet.
const NOT_IN_A_SITE: &str =
    "The file is not inside a moss site. Run `moss build` once on the folder that should be the site, then run the rename again.";

#[test]
fn every_refusal_of_an_unbuilt_folder_says_the_file_is_in_no_site() {
    use crate::nested_roots::{NestedRootsReport, RootClass};
    let report = |root_class, truncated| NestedRootsReport {
        root: "/x".into(),
        root_class,
        nested: Vec::new(),
        truncated,
        dirs_visited: 4000,
    };
    let cases = [
        report(RootClass::FilesystemRoot, false),
        report(RootClass::CloudProviderRoot { provider: "Google Drive".into() }, false),
        report(RootClass::Normal, true),
    ];
    for r in &cases {
        let rename = crate::cli::site_guard::cli_refusal_message(r, "/x", "rename");
        assert!(rename.ends_with(NOT_IN_A_SITE), "{rename}");
        let build = crate::cli::site_guard::cli_refusal_message(r, "/x", "build");
        assert!(build.contains("moss build") && !build.contains(NOT_IN_A_SITE), "{build}");
    }
}

/// A loose page in a folder that is no site, with the folder's other pages.
fn loose_folder(dir: &std::path::Path) {
    fs::write(dir.join("todo.md"), "# Todo\n").unwrap();
    fs::write(dir.join("notes.md"), "[t](todo.md)\n").unwrap();
}

#[test]
fn unbuilt_home_folder_is_refused_and_nothing_changes() {
    let home = tempfile::tempdir().unwrap();
    loose_folder(home.path());
    // moss keeps its own app data in `~/.moss/`; that does not make the home
    // folder a site.
    fs::create_dir_all(home.path().join(".moss")).unwrap();

    let err = rename_in(&args("todo.md", "done.md"), home.path(), Some(home.path())).unwrap_err();

    assert!(err.contains("your home folder"), "{err}");
    assert!(err.ends_with(NOT_IN_A_SITE), "{err}");
    assert!(home.path().join("todo.md").exists() && !home.path().join("done.md").exists());
    assert_eq!(fs::read_to_string(home.path().join("notes.md")).unwrap(), "[t](todo.md)\n");
}

#[test]
fn unbuilt_folder_holding_other_sites_is_refused_and_nothing_changes() {
    let dir = tempfile::tempdir().unwrap();
    loose_folder(dir.path());
    fs::create_dir_all(dir.path().join("journal/.moss")).unwrap();
    fs::write(dir.path().join("journal/entry.md"), "[t](../todo.md)\n").unwrap();

    let err = rename_in(&args("todo.md", "done.md"), dir.path(), None).unwrap_err();

    assert!(err.contains("contains a moss site at journal"), "{err}");
    assert!(err.ends_with(NOT_IN_A_SITE), "{err}");
    assert!(dir.path().join("todo.md").exists() && !dir.path().join("done.md").exists());
    assert_eq!(fs::read_to_string(dir.path().join("journal/entry.md")).unwrap(), "[t](../todo.md)\n");
}

#[test]
fn a_file_outside_every_site_renamed_from_inside_a_site_is_refused_as_in_no_site() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("site/.moss")).unwrap();
    fs::create_dir_all(dir.path().join("site/notes")).unwrap();
    fs::create_dir_all(dir.path().join("loose")).unwrap();
    loose_folder(&dir.path().join("loose"));

    let cwd = dir.path().join("site/notes");
    let err = rename_in(&args("../../loose/todo.md", "../../loose/done.md"), &cwd, None).unwrap_err();

    assert!(err.ends_with(NOT_IN_A_SITE), "{err}");
    assert!(!err.contains("moss rename"), "{err}");
    assert!(dir.path().join("loose/todo.md").exists() && !dir.path().join("loose/done.md").exists());
}
