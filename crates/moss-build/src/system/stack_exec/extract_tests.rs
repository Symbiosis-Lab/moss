use super::*;
use crate::system::stack_exec::layout;
use crate::system::stack_exec::{Artifact, StackHome};
use crate::plugins::contributions::stack::{StackContribution, StackHooks, StackSource};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write as _;
use tar::{Builder, Header};
use tempfile::TempDir;

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut gz = Vec::new();
    {
        let mut encoder = GzEncoder::new(&mut gz, Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap();
    }
    gz
}

/// A `.tar.gz` with one regular-file entry at `name`, with the raw name
/// bytes written directly into the header — `Header::set_path` and
/// `Builder::append_data` both refuse a `..` component or an absolute path
/// themselves, which is convenient for legitimate archive-building but means
/// neither can produce the hostile fixture this test needs. A real attacker
/// crafting a Zip-Slip archive by hand has no such scruples, and the tar
/// FORMAT itself has no zip-slip guard — which is exactly why the extractor
/// arm this drives is written fresh rather than reusing `tar::Archive::unpack`.
fn tar_gz_with_entry(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut header = Header::new_gnu();
    let name_bytes = name.as_bytes();
    header.as_old_mut().name[..name_bytes.len()].copy_from_slice(name_bytes);
    header.set_size(contents.len() as u64);
    header.set_cksum();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = Builder::new(&mut tar_bytes);
        builder.append(&header, contents).unwrap();
        builder.finish().unwrap();
    }
    gzip(&tar_bytes)
}

fn write_archive(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn the_zip_arm_goes_through_the_hardened_path_entry() {
    let src = TempDir::new().unwrap();
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    let zip_bytes = crate::plugins::install::zip_extract::tests::zip_with_files(&[("../escape.txt", b"pwned")]);
    let archive = write_archive(&src, "a.zip", &zip_bytes);

    extract("zip", &archive, &target).expect_err("a traversal entry must be refused through the dispatcher");
    assert!(!base.path().join("escape.txt").exists(), "must not land outside the target dir");
}

fn download_stack_with_executable(archive_format: &str, executable: Option<&str>) -> StackContribution {
    StackContribution {
        id: "widget".to_string(),
        display_name: None,
        version: "1.0.0".to_string(),
        sources: vec![StackSource::Download {
            platform: None,
            url: "https://example.invalid/widget".to_string(),
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

#[test]
fn a_raw_artifact_lands_under_the_home_and_nowhere_else() {
    let home = TempDir::new().unwrap();
    let artifact_dir = TempDir::new().unwrap();
    let artifact = artifact_dir.path().join("widget.bin");
    std::fs::write(&artifact, b"raw-bytes").unwrap();

    let stack = download_stack_with_executable("raw", Some("nested/widget"));
    let dest = layout::binary_path(home.path(), &stack).unwrap();

    extract("raw", &artifact, &dest).unwrap();

    assert!(dest.starts_with(home.path()), "the raw artifact must land under the stack home");
    assert_eq!(std::fs::read(&dest).unwrap(), b"raw-bytes");

    // The raw arm's destination name is itself an `enclosed()` path: a
    // climbing `executable` must be refused before it ever reaches `extract`.
    let hostile = download_stack_with_executable("raw", Some("../../../tmp/moss-s4-raw-pwn"));
    let err = layout::binary_path(home.path(), &hostile)
        .expect_err("a climbing raw destination must be refused");
    assert!(matches!(err, StackExecError::UnsafeDeclaredPath { .. }), "got {err:?}");
}

#[test]
fn the_rotation_calls_before_replace_while_the_old_bundle_is_still_in_place() {
    let home = TempDir::new().unwrap();
    let stack = download_stack_with_executable("zip", None);
    let stack_home = StackHome { root: home.path(), stack: &stack };

    // An existing bundle, as if a prior install already landed.
    let bundle = layout::bundle_root(home.path());
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(bundle.join("marker.txt"), b"old").unwrap();

    let zip_bytes = crate::plugins::install::zip_extract::tests::zip_with_files(&[("app.bin", b"new")]);
    let artifact_dir = TempDir::new().unwrap();
    let artifact_path = artifact_dir.path().join("widget.zip");
    std::fs::write(&artifact_path, &zip_bytes).unwrap();

    let mut fired = 0;
    let mut before_replace = || {
        fired += 1;
        // The rotation has not started yet: the OLD bundle must still be
        // readable at the bundle path when this fires.
        assert!(bundle.join("marker.txt").exists(), "old bundle must still be in place");
        assert_eq!(std::fs::read(bundle.join("marker.txt")).unwrap(), b"old");
    };

    super::super::stage(&stack_home, &Artifact::File(artifact_path), &mut || Ok(()), &mut before_replace)
        .expect("stage must succeed");

    assert_eq!(fired, 1, "before_replace must fire exactly once");
    assert_eq!(std::fs::read(bundle.join("app.bin")).unwrap(), b"new", "the new bundle must be in place");
    assert!(!bundle.join("marker.txt").exists(), "the old bundle's contents must be gone after promotion");
}

/// `before_extract` is the hook a caller uses to stop whatever the OLD
/// bundle is currently running (S4 review item 4 — this used to be a
/// freestanding step the app ran before ever calling `stage`, verified only
/// by hand on a Mac). It must fire before anything is unpacked: nothing may
/// exist at the scratch path yet when it runs.
#[test]
fn the_rotation_calls_before_extract_before_anything_is_unpacked() {
    let home = TempDir::new().unwrap();
    let stack = download_stack_with_executable("zip", None);
    let stack_home = StackHome { root: home.path(), stack: &stack };
    let incoming = layout::incoming_path(home.path());

    let zip_bytes = crate::plugins::install::zip_extract::tests::zip_with_files(&[("app.bin", b"new")]);
    let artifact_dir = TempDir::new().unwrap();
    let artifact_path = artifact_dir.path().join("widget.zip");
    std::fs::write(&artifact_path, &zip_bytes).unwrap();

    let mut fired = 0;
    let mut before_extract = || {
        fired += 1;
        assert!(!incoming.exists(), "nothing must be unpacked yet when before_extract fires");
        Ok(())
    };

    super::super::stage(&stack_home, &Artifact::File(artifact_path), &mut before_extract, &mut || {})
        .expect("stage must succeed");

    assert_eq!(fired, 1, "before_extract must fire exactly once");
}

/// A caller's "stop the old process" step can fail — `stage` must not go on
/// to extract and rotate as if it had succeeded, or a stack that refused to
/// quit gets its bundle replaced out from under it anyway.
#[test]
fn a_before_extract_refusal_aborts_before_any_bytes_are_unpacked() {
    let home = TempDir::new().unwrap();
    let stack = download_stack_with_executable("zip", None);
    let stack_home = StackHome { root: home.path(), stack: &stack };
    let bundle = layout::bundle_root(home.path());

    let zip_bytes = crate::plugins::install::zip_extract::tests::zip_with_files(&[("app.bin", b"new")]);
    let artifact_dir = TempDir::new().unwrap();
    let artifact_path = artifact_dir.path().join("widget.zip");
    std::fs::write(&artifact_path, &zip_bytes).unwrap();

    let mut before_extract = || Err(StackExecError::Extract("could not stop the running stack".to_string()));
    let mut before_replace = || panic!("before_replace must not fire when before_extract refuses");

    let err = super::super::stage(&stack_home, &Artifact::File(artifact_path), &mut before_extract, &mut before_replace)
        .expect_err("a before_extract refusal must abort staging");
    assert!(matches!(err, StackExecError::Extract(_)), "got {err:?}");
    assert!(!bundle.exists(), "nothing must be staged when before_extract refuses");
}

/// No extractor arm carries an archive entry's own unix mode through: a
/// `tar.gz` or `zip` entry lands via `File::create` (platform default mode),
/// and a `raw` download keeps whatever mode the fetch gave it — usually not
/// executable. `stage` sets the bit itself, once, on whatever
/// `layout::binary_path` resolves to after promotion.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(unix)]
#[test]
fn a_staged_zip_binary_is_executable() {
    let home = TempDir::new().unwrap();
    let stack = download_stack_with_executable("zip", Some("widget.bin"));
    let stack_home = StackHome { root: home.path(), stack: &stack };

    let zip_bytes = crate::plugins::install::zip_extract::tests::zip_with_files(&[("widget.bin", b"#!/bin/sh\n")]);
    let artifact_dir = TempDir::new().unwrap();
    let artifact_path = artifact_dir.path().join("widget.zip");
    std::fs::write(&artifact_path, &zip_bytes).unwrap();

    super::super::stage(&stack_home, &Artifact::File(artifact_path), &mut || Ok(()), &mut || {}).expect("stage must succeed");

    let bin = layout::binary_path(home.path(), &stack).unwrap();
    assert!(is_executable(&bin), "a staged zip binary must be executable");
}

#[cfg(unix)]
#[test]
fn a_staged_tar_gz_binary_is_executable() {
    let home = TempDir::new().unwrap();
    let stack = download_stack_with_executable("tar.gz", Some("widget.bin"));
    let stack_home = StackHome { root: home.path(), stack: &stack };

    let archive = tar_gz_with_entry("widget.bin", b"#!/bin/sh\n");
    let artifact_dir = TempDir::new().unwrap();
    let artifact_path = artifact_dir.path().join("widget.tar.gz");
    std::fs::write(&artifact_path, &archive).unwrap();

    super::super::stage(&stack_home, &Artifact::File(artifact_path), &mut || Ok(()), &mut || {}).expect("stage must succeed");

    let bin = layout::binary_path(home.path(), &stack).unwrap();
    assert!(is_executable(&bin), "a staged tar.gz binary must be executable");
}

#[cfg(unix)]
#[test]
fn a_staged_raw_binary_is_executable() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempDir::new().unwrap();
    let stack = download_stack_with_executable("raw", None);
    let stack_home = StackHome { root: home.path(), stack: &stack };

    let artifact_dir = TempDir::new().unwrap();
    let artifact_path = artifact_dir.path().join("widget.bin");
    std::fs::write(&artifact_path, b"#!/bin/sh\n").unwrap();
    // A freshly downloaded artifact is not executable by default.
    std::fs::set_permissions(&artifact_path, std::fs::Permissions::from_mode(0o644)).unwrap();

    super::super::stage(&stack_home, &Artifact::File(artifact_path), &mut || Ok(()), &mut || {}).expect("stage must succeed");

    let bin = layout::binary_path(home.path(), &stack).unwrap();
    assert!(is_executable(&bin), "a staged raw binary must be executable");
}

/// Moved from `stack_install.rs`'s `hdiutil_mount_point_parsed`.
#[test]
fn hdiutil_mount_point_parsed() {
    let out = "/dev/disk4          \tGUID_partition_scheme          \t\n\
               /dev/disk4s1        \tApple_HFS                      \t/Volumes/OnionPress\n";
    assert_eq!(parse_hdiutil_mount_point(out), Some(std::path::PathBuf::from("/Volumes/OnionPress")));
    assert_eq!(parse_hdiutil_mount_point("/dev/disk4\tGUID\t"), None);
}
