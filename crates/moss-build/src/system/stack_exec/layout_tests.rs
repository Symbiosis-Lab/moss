use super::*;
use crate::plugins::contributions::stack::{StackContribution, StackHooks, StackSource};
use tempfile::TempDir;

fn download_stack(archive_format: &str, executable: Option<&str>) -> StackContribution {
    StackContribution {
        id: "widget".to_string(),
        display_name: None,
        version: "1.0.0".to_string(),
        sources: vec![StackSource::Download {
            platform: None,
            url: "https://example.invalid/widget.bin".to_string(),
            sha256: Some("a".repeat(64)),
            archive_format: archive_format.to_string(),
            executable: executable.map(str::to_string),
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

#[test]
fn a_declared_executable_cannot_climb_out_of_the_stack_home() {
    let home = TempDir::new().unwrap();

    let climbing = download_stack("dmg", Some("../../../tmp/x"));
    let err = binary_path(home.path(), &climbing).expect_err("a climbing executable must be refused");
    assert!(matches!(err, StackExecError::UnsafeDeclaredPath { field: "executable", .. }), "got {err:?}");

    let absolute = download_stack("dmg", Some("/tmp/x"));
    let err = binary_path(home.path(), &absolute).expect_err("an absolute executable must be refused");
    assert!(matches!(err, StackExecError::UnsafeDeclaredPath { field: "executable", .. }), "got {err:?}");
}

#[test]
fn a_declared_executable_overrides_the_default_bundle_path() {
    let home = TempDir::new().unwrap();

    // Resolved under `bundle_root`, not `home` directly — that is where
    // `stage` actually extracts a non-`raw` archive (the S4 blocker: a
    // declared `executable` resolved against `home` named a path one level
    // too high and never existed).
    let with_executable = download_stack("zip", Some("nested/bin/widget"));
    assert_eq!(
        binary_path(home.path(), &with_executable).unwrap(),
        bundle_root(home.path()).join("nested/bin/widget")
    );

    let without_executable = download_stack("zip", None);
    assert_eq!(binary_path(home.path(), &without_executable).unwrap(), bundle_root(home.path()));
}

#[test]
fn a_path_source_resolves_relative_and_absolute_binaries() {
    let home = TempDir::new().unwrap();

    let relative = path_stack("widget");
    assert_eq!(binary_path(home.path(), &relative).unwrap(), home.path().join("widget"));

    let absolute = path_stack("/usr/local/bin/widget");
    assert_eq!(binary_path(home.path(), &absolute).unwrap(), PathBuf::from("/usr/local/bin/widget"));
}

/// Moved from `stack_install.rs`'s `path_derivation`, rewritten against a
/// declaration rather than the hardcoded `OnionPress.app` constant. The
/// `executable: None` case alone hid the S4 blocker — `bundle_root ==
/// binary_path` for it either way, so a `binary_path` that resolved against
/// `home` directly still passed. The OnionPress-shaped declared-executable
/// case beside it is what actually pins the resolution root.
#[test]
fn path_derivation() {
    let base = Path::new("/home/u/.moss/stacks/widget");
    let stack = download_stack("zip", None);
    assert_eq!(bundle_root(base), base.join("bundle"));
    assert_eq!(binary_path(base, &stack).unwrap(), base.join("bundle"));

    let with_executable = download_stack("zip", Some("Widget.app/Contents/MacOS/widget"));
    assert_eq!(
        binary_path(base, &with_executable).unwrap(),
        base.join("bundle").join("Widget.app/Contents/MacOS/widget")
    );
}

/// Moved from `stack_install.rs`'s `is_installed_reads_disk`.
#[test]
fn is_installed_reads_disk() {
    let home = TempDir::new().unwrap();
    let stack = download_stack("zip", None);
    assert!(!is_installed(home.path(), &stack), "no bundle => not installed");
    std::fs::create_dir_all(bundle_root(home.path())).unwrap();
    assert!(is_installed(home.path(), &stack), "bundle on disk => installed");
}
