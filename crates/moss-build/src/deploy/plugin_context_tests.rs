//! Tests for the github-pages guard in `plugin_context.rs`.
//!
//! A subpath Pages repo serves the site under `username.github.io/repo/`, which
//! every root-relative URL moss emits would miss.

use super::{is_non_root_github_pages, refuses_subpath_pages};

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
