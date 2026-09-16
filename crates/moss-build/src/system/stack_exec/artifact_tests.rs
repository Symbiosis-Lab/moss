use super::*;
use crate::plugins::contributions::stack::{StackContribution, StackHooks, StackSource};
use tempfile::TempDir;

fn download_stack(url: &str, sha256: Option<&str>) -> StackContribution {
    StackContribution {
        id: "widget".to_string(),
        display_name: None,
        version: "1.0.0".to_string(),
        sources: vec![StackSource::Download {
            platform: None,
            url: url.to_string(),
            sha256: sha256.map(str::to_string),
            archive_format: "dmg".to_string(),
            executable: None,
        }],
        start: vec![],
        stop: vec![],
        uninstall: vec![],
        recover: vec![],
        self_heal_grace_secs: None,
        preserve_on_uninstall: vec![],
        hooks: StackHooks::default(),
    }
}

fn path_stack(binary: &str) -> StackContribution {
    StackContribution {
        id: "widget".to_string(),
        display_name: None,
        version: "1.0.0".to_string(),
        sources: vec![StackSource::Path { platform: None, binary: binary.to_string() }],
        start: vec![],
        stop: vec![],
        uninstall: vec![],
        recover: vec![],
        self_heal_grace_secs: None,
        preserve_on_uninstall: vec![],
        hooks: StackHooks::default(),
    }
}

/// No network reachable from a test, so the pre-seeded-cache pattern at
/// `registry_client/artifact.rs:732` is the way to exercise `obtain` without
/// one: `adopt_or_refuse_cached` is consulted before any request, exactly as
/// `cached_file_is_valid` is inside `fetch_verified` itself.
#[test]
fn cached_bytes_that_do_not_match_the_declaration_are_refused_and_removed() {
    let home_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();

    let wrong_bytes = b"not what review pinned";
    let dest = cache_path(cache_dir.path(), "widget", "1.0.0", "dmg");
    std::fs::write(&dest, wrong_bytes).unwrap();
    let actual = crate::system::large_download::file_sha256(&dest).unwrap();
    let expected = "b".repeat(64);

    let stack = download_stack("https://example.invalid/widget.dmg", Some(&expected));
    let home = StackHome { root: home_dir.path(), stack: &stack };

    let err = obtain(&home, cache_dir.path(), None).expect_err("mismatched cached bytes must be refused");
    match err {
        StackExecError::HashMismatch { url, expected: e, actual: a } => {
            assert_eq!(url, "https://example.invalid/widget.dmg");
            assert_eq!(e, expected);
            assert_eq!(a, actual);
        }
        other => panic!("expected HashMismatch, got {other:?}"),
    }
    assert!(!dest.exists(), "the mismatched file must not survive the refusal");
}

/// Neither refusal reaches the network: both are caught before `obtain` ever
/// derives a cache path, let alone calls `fetch_verified`.
#[test]
fn a_missing_or_placeholder_hash_refuses_before_any_bytes_move() {
    let home_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();

    let missing = download_stack("https://example.invalid/widget.dmg", None);
    let home = StackHome { root: home_dir.path(), stack: &missing };
    assert!(
        matches!(obtain(&home, cache_dir.path(), None), Err(StackExecError::MissingHash { .. })),
        "no sha256 declared"
    );

    let placeholder = download_stack(
        "https://example.invalid/widget.dmg",
        Some("<PLACEHOLDER-fill-in-on-release>"),
    );
    let home = StackHome { root: home_dir.path(), stack: &placeholder };
    assert!(
        matches!(obtain(&home, cache_dir.path(), None), Err(StackExecError::PlaceholderHash { .. })),
        "a placeholder sha256 is not a real one"
    );

    assert!(
        std::fs::read_dir(cache_dir.path()).unwrap().next().is_none(),
        "neither refusal may create anything under the cache dir"
    );
}

/// `binary_path`'s resolution for a `Path` source is pinned in
/// `layout_tests.rs::a_path_source_resolves_relative_and_absolute_binaries`;
/// this test is `obtain`'s half — nothing fetched, nothing cached.
#[test]
fn a_path_source_obtains_nothing_and_runs_the_declared_binary() {
    let home_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();

    let stack = path_stack("widget-cli");
    let home = StackHome { root: home_dir.path(), stack: &stack };

    let artifact = obtain(&home, cache_dir.path(), None).expect("a Path source obtains successfully");
    assert!(matches!(artifact, Artifact::AlreadyPresent));
    assert!(
        std::fs::read_dir(cache_dir.path()).unwrap().next().is_none(),
        "a Path source touches neither the cache dir nor the network"
    );
}

/// Moved from `stack_install.rs`'s `prune_stale_cache_keeps_the_pinned_version_and_its_in_flight_resume`,
/// with the identity rule generalized from the word "onionpress" to the
/// declaration's own id — the `other-stack` fixture is what pins that.
#[test]
fn prune_stale_cache_keeps_the_pinned_version_and_its_in_flight_resume() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let keep = dir.join("widget-1.0.1.dmg");
    let keep_part = dir.join("widget-1.0.1.dmg.part");
    let stale = dir.join("widget-1.0.0.dmg");
    let stale_part = dir.join("widget-0.9.0.dmg.part");
    let other_stack = dir.join("other-stack-1.0.0.dmg");
    let unrelated = dir.join("notes.txt");
    for p in [&keep, &keep_part, &stale, &stale_part, &other_stack, &unrelated] {
        std::fs::write(p, b"x").unwrap();
    }

    prune_stale_cache(dir, &keep, "widget");

    assert!(keep.exists(), "the pinned artifact survives");
    assert!(keep_part.exists(), "a resumable partial must NOT be pruned");
    assert!(!stale.exists(), "a superseded release of THIS stack is reclaimed");
    assert!(!stale_part.exists(), "so is its partial");
    assert!(other_stack.exists(), "a different stack's cache is never touched by this one's pruning");
    assert!(unrelated.exists(), "only stack artifacts are touched");
}
