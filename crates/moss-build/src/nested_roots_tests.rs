use super::*;
use std::fs;

fn limits() -> ScanLimits {
    ScanLimits::default()
}

/// Make `dir` a moss root; `state` writes `.moss/state.toml` with that body.
fn make_root(dir: &Path, state: Option<&str>) {
    let moss = dir.join(".moss");
    fs::create_dir_all(&moss).unwrap();
    if let Some(body) = state {
        fs::write(moss.join("state.toml"), body).unwrap();
    }
}

fn published(dir: &Path, site_id: &str) {
    make_root(
        dir,
        Some(&format!("[deployment]\nsite_id = \"{site_id}\"\ndeploy_method = \"moss\"\n")),
    );
}

fn rel_paths(report: &NestedRootsReport) -> Vec<&str> {
    report.nested.iter().map(|n| n.rel_path.as_str()).collect()
}

// ---- find_nested_roots: trigger + labeling ----

#[test]
fn preview_only_root_fires() {
    // THE incident regression: a `.moss/` that was never deployed (no
    // state.toml at all) must be detected.
    let tmp = tempfile::tempdir().unwrap();
    let child = tmp.path().join("draft");
    make_root(&child, None);

    let report = find_nested_roots(tmp.path(), None, &limits());
    assert_eq!(rel_paths(&report), vec!["draft"]);
    assert_eq!(report.nested[0].published, Some(false));
    assert_eq!(report.nested[0].site_id, None);
    assert_eq!(report.nested[0].name, "draft");
    assert!(!report.truncated);
}

#[test]
fn preview_only_state_without_site_id_is_not_published() {
    let tmp = tempfile::tempdir().unwrap();
    make_root(&tmp.path().join("draft"), Some("[deployment]\ndeploy_method = \"moss\"\n"));

    let report = find_nested_roots(tmp.path(), None, &limits());
    assert_eq!(report.nested[0].published, Some(false));
    assert_eq!(report.nested[0].site_id, None);
}

#[test]
fn published_root_carries_site_id() {
    let tmp = tempfile::tempdir().unwrap();
    published(&tmp.path().join("blog"), "my-blog");

    let report = find_nested_roots(tmp.path(), None, &limits());
    assert_eq!(report.nested[0].published, Some(true));
    assert_eq!(report.nested[0].site_id.as_deref(), Some("my-blog"));
}

#[test]
fn empty_site_id_is_preview_only() {
    let tmp = tempfile::tempdir().unwrap();
    make_root(&tmp.path().join("draft"), Some("[deployment]\nsite_id = \"\"\n"));

    let report = find_nested_roots(tmp.path(), None, &limits());
    assert_eq!(report.nested[0].published, Some(false));
}

#[test]
fn unparseable_state_is_unknown_never_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    make_root(&tmp.path().join("half-synced"), Some("[deployment\nnot toml"));

    let report = find_nested_roots(tmp.path(), None, &limits());
    assert_eq!(report.nested[0].published, None);
    assert_eq!(report.nested[0].site_id, None);
}

#[test]
#[cfg(unix)]
fn unreadable_state_is_unknown_never_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let child = tmp.path().join("evicted");
    published(&child, "hidden");
    let state = child.join(".moss/state.toml");
    fs::set_permissions(&state, fs::Permissions::from_mode(0o000)).unwrap();
    // Root runs bypass file permissions; only assert "unknown" when denied.
    let denied = fs::read_to_string(&state).is_err();

    let report = find_nested_roots(tmp.path(), None, &limits());
    fs::set_permissions(&state, fs::Permissions::from_mode(0o644)).unwrap();

    assert_eq!(report.nested.len(), 1, "detection must still fire on the dir stat");
    if denied {
        assert_eq!(report.nested[0].published, None, "unreadable must be unknown");
        assert_eq!(report.nested[0].site_id, None);
    }
}

// ---- ordering, bounds, pruning ----

#[test]
fn shallowest_first_ordering() {
    let tmp = tempfile::tempdir().unwrap();
    make_root(&tmp.path().join("a/b/deep"), None);
    make_root(&tmp.path().join("shallow"), None);

    let report = find_nested_roots(tmp.path(), None, &limits());
    assert_eq!(rel_paths(&report), vec!["shallow", "a/b/deep"]);
}

#[test]
fn root_owning_moss_short_circuits_but_scan_below_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    make_root(tmp.path(), None);
    make_root(&tmp.path().join("inner"), None);

    let report = find_nested_roots(tmp.path(), None, &limits());
    assert!(report.nested.is_empty());
    assert_eq!(report.dirs_visited, 0);

    let below = scan_below(tmp.path(), None, &limits());
    assert_eq!(rel_paths(&below), vec!["inner"]);
}

#[test]
fn match_subtree_is_pruned() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = tmp.path().join("outer");
    make_root(&outer, None);
    make_root(&outer.join("deeper"), None);

    let report = find_nested_roots(tmp.path(), None, &limits());
    assert_eq!(rel_paths(&report), vec!["outer"]);
}

#[test]
fn skips_excluded_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    for skip in [".git", "node_modules"] {
        make_root(&tmp.path().join(skip).join("site"), None);
    }
    let report = find_nested_roots(tmp.path(), None, &limits());
    assert!(report.nested.is_empty());
    assert!(!report.truncated);
}

#[test]
#[cfg(unix)]
fn does_not_follow_symlinked_dir() {
    use std::os::unix::fs::symlink;
    let outside = tempfile::tempdir().unwrap();
    let real = outside.path().join("real_site");
    make_root(&real, None);

    let root = tempfile::tempdir().unwrap();
    symlink(&real, root.path().join("linked")).unwrap();

    let report = find_nested_roots(root.path(), None, &limits());
    assert!(report.nested.is_empty());
}

#[test]
fn depth_cap_sets_truncated_instead_of_silently_proceeding() {
    let tmp = tempfile::tempdir().unwrap();
    let mut deep = tmp.path().to_path_buf();
    for i in 0..8 {
        deep = deep.join(format!("d{i}"));
    }
    make_root(&deep, None);

    let report = find_nested_roots(tmp.path(), None, &limits());
    assert!(report.nested.is_empty(), "beyond depth bound must not be reported");
    assert!(report.truncated, "an unexplored subtree must be visible to the decision layer");

    // Within the bound: found, and a shallow clean tree is NOT truncated.
    make_root(&tmp.path().join("a/b/site"), None);
    let report = find_nested_roots(tmp.path(), None, &limits());
    assert_eq!(rel_paths(&report), vec!["a/b/site"]);
}

#[test]
fn dir_cap_sets_truncated() {
    let tmp = tempfile::tempdir().unwrap();
    for name in ["s1", "s2", "s3"] {
        make_root(&tmp.path().join(name), None);
    }
    let small = ScanLimits { max_depth: 6, max_dirs: 2, max_reported: 5 };
    let report = find_nested_roots(tmp.path(), None, &small);
    assert!(report.nested.len() <= 1);
    assert!(report.truncated, "a cap hit must never read as a clean scan");
}

#[test]
fn report_cap_stops_early_and_sets_truncated() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..4 {
        make_root(&tmp.path().join(format!("s{i}")), None);
    }
    let small = ScanLimits { max_depth: 6, max_dirs: 4000, max_reported: 2 };
    let report = find_nested_roots(tmp.path(), None, &small);
    assert_eq!(report.nested.len(), 2);
    assert!(report.truncated);
}

#[test]
#[cfg(unix)]
fn unlistable_dir_sets_truncated_instead_of_reading_as_clean() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let opaque = tmp.path().join("opaque");
    fs::create_dir_all(&opaque).unwrap();
    fs::set_permissions(&opaque, fs::Permissions::from_mode(0o000)).unwrap();
    // Root runs list anyway; only assert when the denial applied.
    let denied = fs::read_dir(&opaque).is_err();

    let report = find_nested_roots(tmp.path(), None, &limits());
    fs::set_permissions(&opaque, fs::Permissions::from_mode(0o755)).unwrap();

    if denied {
        assert!(report.truncated, "an unlistable subtree must not read as a clean scan");
    }
}

#[test]
fn clean_shallow_tree_is_not_truncated() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("docs/img")).unwrap();
    let report = find_nested_roots(tmp.path(), None, &limits());
    assert!(report.nested.is_empty());
    assert!(!report.truncated);
    assert!(report.dirs_visited >= 3);
}

// ---- classify_root ----

#[test]
fn classify_cloud_storage_shapes_are_localization_independent() {
    let home = Path::new("/Users/alice");
    let acct = home.join("Library/CloudStorage/GoogleDrive-a@b");
    // Account dir itself.
    assert_eq!(
        classify_root(&acct, Some(home)),
        RootClass::CloudProviderRoot { provider: "GoogleDrive".into() }
    );
    // First level: the localized wrapper, matched by DEPTH not name.
    for wrapper in ["My Drive", "我的雲端硬碟"] {
        assert_eq!(
            classify_root(&acct.join(wrapper), Some(home)),
            RootClass::CloudProviderRoot { provider: "GoogleDrive".into() },
            "wrapper {wrapper:?} must classify as provider root"
        );
    }
    // Two levels down is a real folder again.
    assert_eq!(
        classify_root(&acct.join("My Drive/projects"), Some(home)),
        RootClass::Normal
    );
    // The CloudStorage container itself.
    assert!(matches!(
        classify_root(&home.join("Library/CloudStorage"), Some(home)),
        RootClass::CloudProviderRoot { .. }
    ));
}

#[test]
fn classify_file_stream_dropbox_and_onedrive() {
    let home = Path::new("/Users/alice");
    assert!(matches!(
        classify_root(Path::new("/Volumes/GoogleDrive"), Some(home)),
        RootClass::CloudProviderRoot { .. }
    ));
    assert!(matches!(
        classify_root(Path::new("/Volumes/GoogleDrive/My Drive"), Some(home)),
        RootClass::CloudProviderRoot { .. }
    ));
    assert_eq!(
        classify_root(Path::new("/Volumes/GoogleDrive/My Drive/site"), Some(home)),
        RootClass::Normal
    );
    assert_eq!(
        classify_root(&home.join("Dropbox"), Some(home)),
        RootClass::CloudProviderRoot { provider: "Dropbox".into() }
    );
    assert_eq!(
        classify_root(&home.join("Dropbox (Personal)"), Some(home)),
        RootClass::CloudProviderRoot { provider: "Dropbox".into() }
    );
    assert_eq!(
        classify_root(&home.join("OneDrive - Acme"), Some(home)),
        RootClass::CloudProviderRoot { provider: "OneDrive".into() }
    );
    // A folder INSIDE a legacy mount is normal (no wrapper level exists).
    assert_eq!(classify_root(&home.join("Dropbox/site"), Some(home)), RootClass::Normal);
}

#[test]
fn classify_home_system_special_and_filesystem_root() {
    let home = Path::new("/Users/alice");
    assert_eq!(classify_root(home, Some(home)), RootClass::HomeDir);
    assert_eq!(
        classify_root(&home.join("Desktop"), Some(home)),
        RootClass::SystemSpecial("Desktop".into())
    );
    assert_eq!(
        classify_root(&home.join("Documents"), Some(home)),
        RootClass::SystemSpecial("Documents".into())
    );
    assert_eq!(
        classify_root(&home.join("Downloads"), Some(home)),
        RootClass::SystemSpecial("Downloads".into())
    );
    assert_eq!(classify_root(Path::new("/"), Some(home)), RootClass::FilesystemRoot);
    // Deep normal folders stay normal.
    assert_eq!(
        classify_root(&home.join("Sites/blog"), Some(home)),
        RootClass::Normal
    );
    assert_eq!(
        classify_root(&home.join("Desktop/project"), Some(home)),
        RootClass::Normal
    );
}

// ---- owns_moss / is_cloud_storage_path ----

#[test]
fn owns_moss_requires_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!owns_moss(tmp.path()));
    fs::write(tmp.path().join(".moss"), "a file, not a dir").unwrap();
    assert!(!owns_moss(tmp.path()));
    fs::remove_file(tmp.path().join(".moss")).unwrap();
    fs::create_dir(tmp.path().join(".moss")).unwrap();
    assert!(owns_moss(tmp.path()));
}

#[test]
fn cloud_storage_path_predicate_covers_known_mounts() {
    // Vectors moved here from build/pipeline_tests.rs with the predicate.
    assert!(is_cloud_storage_path("/Users/a/Library/CloudStorage/GoogleDrive-x/My Drive/site"));
    assert!(is_cloud_storage_path("/Users/a/Library/CloudStorage/Dropbox/site"));
    assert!(is_cloud_storage_path("/Users/a/Library/CloudStorage/OneDrive-Personal/blog"));
    assert!(is_cloud_storage_path("/Users/a/Library/Mobile Documents/com~apple~CloudDocs/site"));
    assert!(is_cloud_storage_path("/Users/a/Dropbox/site"));
    assert!(is_cloud_storage_path("/Users/a/Dropbox (Personal)/site"));
    assert!(is_cloud_storage_path("/Users/a/Dropbox (Acme Inc)/site"));
    assert!(is_cloud_storage_path("/Volumes/GoogleDrive/My Drive/site"));
    assert!(is_cloud_storage_path("/Users/a/OneDrive/site"));
    assert!(is_cloud_storage_path("/Users/a/OneDrive - Acme/site"));
    // Local paths must NOT trigger the heuristic; "/Dropbox/" and
    // "/Dropbox (" deliberately require a path-segment boundary.
    assert!(!is_cloud_storage_path("/Users/a/Sites/blog"));
    assert!(!is_cloud_storage_path("/Users/a/Documents/site"));
    assert!(!is_cloud_storage_path("/tmp/build-fixture"));
    assert!(!is_cloud_storage_path("/var/folders/abc/site"));
    assert!(!is_cloud_storage_path("/Users/a/MyDropboxBackup/site"));
}
