//! Tests for `plugin_context.rs`: the github-pages guard, and the deploy
//! hook's copy of the site.
//!
//! A subpath Pages repo serves the site under `username.github.io/repo/`, which
//! every root-relative URL moss emits would miss.

use super::*;

const PROJECT_REPO: &str = "https://github.com/user/my-project.git";

#[test]
fn origins_that_serve_from_the_root() {
    for origin in [
        "git@github.com:user/user.github.io.git",
        "https://github.com/user/user.github.io.git",
        "https://github.com/user/user.github.io/",
        "https://gitlab.com/user/repo.git",
    ] {
        assert!(!is_non_root_github_pages(origin), "{origin}");
    }
}

#[test]
fn origins_that_serve_from_a_subpath() {
    for origin in [
        "git@github.com:user/my-project.git",
        PROJECT_REPO,
        "git@github.com:org/some-repo",
    ] {
        assert!(is_non_root_github_pages(origin), "{origin}");
    }
}

#[test]
fn github_pages_into_a_project_repo_without_a_domain_is_refused() {
    assert!(refuses_subpath_pages(Some("github"), None, Some(PROJECT_REPO)));
}

/// The regression: a vault kept under version control on GitHub publishes
/// somewhere else entirely, and `origin` says nothing about that target.
#[test]
fn every_other_deploy_plugin_publishes_regardless_of_origin() {
    for plugin in [Some("onionpress"), Some("matters"), None] {
        assert!(
            !refuses_subpath_pages(plugin, None, Some(PROJECT_REPO)),
            "{plugin:?}"
        );
    }
}

#[test]
fn a_domain_a_root_repo_or_no_remote_lets_github_pages_through() {
    assert!(!refuses_subpath_pages(Some("github"), Some("example.com"), Some(PROJECT_REPO)));
    assert!(!refuses_subpath_pages(Some("github"), None, Some("git@github.com:user/user.github.io.git")));
    assert!(!refuses_subpath_pages(Some("github"), None, None));
}

// --- the deploy hook's copy of the site ---

static NOT_CANCELLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);


fn site_with_generation(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let generation = MossPaths::new(tmp.path()).generation_dir("g1");
    for (rel, text) in files {
        let p = generation.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    (tmp, generation)
}

fn files_under(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    out.sort();
    out
}

#[test]
fn the_copy_holds_exactly_the_listed_files_read_only_and_goes_on_drop() {
    let (tmp, generation) =
        site_with_generation(&[("index.html", "home"), ("a/b.html", "b"), ("wip/index.html", "draft")]);
    let listed = vec!["index.html".to_string(), "a/b.html".to_string()];

    let scratch = DeployScratch::create(tmp.path(), "g1", listed, || {}, &NOT_CANCELLED).unwrap();
    let context = build_deploy_context(tmp.path().to_str().unwrap(), &scratch, None);
    let dir = PathBuf::from(&context.site_dir);
    assert!(dir.starts_with(MossPaths::new(tmp.path()).deploy_scratch_dir()));
    assert_eq!(files_under(&dir), vec!["a/b.html", "index.html"], "a file not listed must not be copied");
    assert_eq!(std::fs::read_to_string(dir.join("a/b.html")).unwrap(), "b");
    assert!(std::fs::metadata(dir.join("index.html")).unwrap().permissions().readonly());
    let index = PathBuf::from(&context.index_file);
    assert_eq!(index.parent(), dir.parent());

    std::fs::write(&index, "git wrote this").unwrap();
    drop(scratch);
    assert!(!dir.exists(), "the copy is removed when the hook call ends");
    assert!(!index.exists(), "and so is the index git wrote beside it");
    assert!(generation.join("wip/index.html").exists(), "the generation itself is untouched");
}

#[test]
fn what_a_crashed_publish_left_is_cleared_and_a_running_publish_is_waited_for() {
    let (tmp, _generation) = site_with_generation(&[("index.html", "home")]);
    let root = MossPaths::new(tmp.path()).deploy_scratch_dir();
    std::fs::create_dir_all(root.join("g0/sub")).unwrap();
    std::fs::write(root.join("g0/sub/f.html"), "x").unwrap();
    set_readonly(&root.join("g0/sub/f.html"), true).unwrap();
    std::fs::write(root.join("g0.index"), "i").unwrap();

    let first = DeployScratch::create(tmp.path(), "g1", vec!["index.html".to_string()], || {}, &NOT_CANCELLED).unwrap();
    assert!(!root.join("g0").exists(), "a leftover copy is removed, read-only files and all");
    assert!(!root.join("g0.index").exists());

    let folder = tmp.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let waiting = tx.clone();
    let second = std::thread::spawn(move || {
        let on_wait = move || waiting.send("waiting").unwrap();
        let scratch = DeployScratch::create(&folder, "g1", vec!["index.html".to_string()], on_wait, &NOT_CANCELLED).unwrap();
        tx.send("copied").unwrap();
        drop(scratch);
    });
    assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(10)), Ok("waiting"), "the wait is reported");
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(300)).is_err(),
        "a second publish of the folder waits while the first holds its copy"
    );
    assert!(root.join("g1/index.html").exists(), "and does not clear the first one's copy");
    drop(first);
    assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(10)), Ok("copied"), "then it runs");
    second.join().unwrap();
}

/// A copy built on top of a leftover it could not clear would hold more than
/// the listed files.
#[cfg(unix)]
#[test]
fn a_leftover_that_cannot_be_cleared_refuses_the_copy() {
    use std::os::unix::fs::PermissionsExt;
    let (tmp, _generation) = site_with_generation(&[("index.html", "home")]);
    let root = MossPaths::new(tmp.path()).deploy_scratch_dir();
    std::fs::create_dir_all(root.join("g0")).unwrap();
    let parent = root.parent().unwrap().to_path_buf();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = DeployScratch::create(tmp.path(), "g1", vec!["index.html".to_string()], || {}, &NOT_CANCELLED);
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = result.err().expect("the copy is refused");
    assert!(err.contains("could not clear"), "{err}");
}

/// The lock order a publish relies on: taking the deploy lock never waits on
/// the build lock, so a build that is running (or waiting) cannot hold up a
/// publish's copy, and a publish cannot hold up a build.
#[test]
fn the_deploy_lock_does_not_wait_on_the_build_lock() {
    let (tmp, _generation) = site_with_generation(&[("index.html", "home")]);
    let _build = crate::infra::folder_lock::acquire(tmp.path()).unwrap();
    let folder = tmp.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let made = DeployScratch::create(&folder, "g1", vec!["index.html".to_string()], || {}, &NOT_CANCELLED).is_ok();
        let _ = tx.send(made);
    });
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_secs(10)),
        Ok(true),
        "the copy is made while a build holds the build lock"
    );
}

#[test]
fn a_generation_gone_while_waiting_or_a_cancelled_publish_makes_no_copy() {
    let (tmp, _generation) = site_with_generation(&[("index.html", "home")]);
    let root = MossPaths::new(tmp.path()).deploy_scratch_dir();
    let gone = DeployScratch::create(tmp.path(), "g9", vec!["index.html".to_string()], || {}, &NOT_CANCELLED);
    assert!(gone.err().expect("refused").contains("rebuilt while waiting"));

    let cancelled = std::sync::atomic::AtomicBool::new(true);
    let made = DeployScratch::create(tmp.path(), "g1", vec!["index.html".to_string()], || {}, &cancelled);
    assert!(made.err().expect("refused").contains("cancelled"));
    assert!(!root.join("g1").exists(), "nothing was copied");
}
