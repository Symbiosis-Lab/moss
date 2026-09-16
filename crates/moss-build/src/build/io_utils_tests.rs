//! Tests for the output-write primitive (ADR-043).
//!
//! The property that actually matters — "a write over a cloud-evicted
//! destination succeeds" — cannot be tested on any CI moss has: it needs a real
//! macOS File Provider vault. `output_write_invariant_test` is the proxy, by
//! proving no output write uses an `O_TRUNC` open. What is testable here is
//! everything downstream of that choice: atomicity, inode freshness, no litter.

use super::*;
#[cfg(unix)] // inode identity: the two *_inode tests below are unix-only
use std::os::unix::fs::MetadataExt;
use tempfile::tempdir;

#[test]
fn writes_when_file_missing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sub/new.txt");
    write_output(&path, b"hello").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"hello");
}

#[test]
fn overwrites_an_existing_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("x.txt");
    fs::write(&path, b"old").unwrap();
    write_output(&path, b"new").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"new");
}

/// The whole point of ADR-043: the destination is replaced, never opened for
/// truncation. A fresh inode is the observable consequence — and it is also
/// what makes the CAS-hardlink hazard (CLAUDE.md `fs::hard_link` rule) unable
/// to arise on these paths.
#[test]
#[cfg(unix)] // .ino() — Windows has no stable file-identity API; the temp+rename mechanism is shared code
fn write_replaces_the_inode_rather_than_truncating() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("x.txt");
    fs::write(&path, b"old").unwrap();
    let before = fs::metadata(&path).unwrap().ino();

    write_output(&path, b"new").unwrap();

    let after = fs::metadata(&path).unwrap().ino();
    assert_ne!(before, after, "output write must mint a new inode, not truncate in place");
}

/// A hardlink into the CAS must not have the build's bytes written *through*
/// it — that is the shared-inode eviction hazard `hardlink_invariant_test`
/// exists for, arriving from the other direction.
#[test]
fn write_does_not_propagate_through_a_hardlink() {
    let dir = tempdir().unwrap();
    let blob = dir.path().join("blob.bin");
    let out = dir.path().join("out.bin");
    fs::write(&blob, b"cas bytes").unwrap();
    fs::hard_link(&blob, &out).unwrap(); // allow:hard_link fixture for the negative case

    write_output(&out, b"rebuilt").unwrap();

    assert_eq!(fs::read(&blob).unwrap(), b"cas bytes", "the CAS blob must be untouched");
    assert_eq!(fs::read(&out).unwrap(), b"rebuilt");
}

#[test]
fn write_leaves_no_pending_temp_behind() {
    let dir = tempdir().unwrap();
    write_output(&dir.path().join("a.txt"), b"a").unwrap();
    write_output(&dir.path().join("a.txt"), b"b").unwrap();

    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".pending."))
        .collect();
    assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
}

/// A failed write must not litter staging: `ship_phase`'s orphan sweep and the
/// preview server both walk that tree.
#[test]
fn a_failed_write_cleans_up_its_temp() {
    let dir = tempdir().unwrap();
    // A directory at the destination path makes `rename` fail while the temp
    // write itself succeeds — the one branch that has litter to clean up.
    let path = dir.path().join("occupied");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("child"), b"x").unwrap();

    assert!(write_output(&path, b"bytes").is_err());

    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".pending."))
        .collect();
    assert!(leftovers.is_empty(), "failed write left litter: {leftovers:?}");
}

#[test]
#[cfg(unix)] // .ino() — same as write_replaces_the_inode_rather_than_truncating
fn copy_output_replaces_the_destination_inode() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("nested/dst.bin");
    fs::write(&src, b"payload").unwrap();
    fs::create_dir_all(dst.parent().unwrap()).unwrap();
    fs::write(&dst, b"stale").unwrap();
    let before = fs::metadata(&dst).unwrap().ino();

    copy_output(&src, &dst).unwrap();

    assert_eq!(fs::read(&dst).unwrap(), b"payload");
    assert_ne!(fs::metadata(&dst).unwrap().ino(), before);
}

#[test]
fn copy_output_creates_parent_dirs() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src.bin");
    fs::write(&src, b"payload").unwrap();
    copy_output(&src, &dir.path().join("a/b/c.bin")).unwrap();
    assert_eq!(fs::read(dir.path().join("a/b/c.bin")).unwrap(), b"payload");
}

#[test]
fn skips_when_content_identical() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("same.txt");
    fs::write(&path, b"same").unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    let changed = write_output_if_changed(&path, b"same").unwrap();

    assert!(!changed);
    let after = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(before, after, "mtime must not change on skip");
}

#[test]
fn writes_when_content_differs() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("diff.txt");
    fs::write(&path, b"old").unwrap();
    assert!(write_output_if_changed(&path, b"new").unwrap());
    assert_eq!(fs::read(&path).unwrap(), b"new");
}

/// An unreadable destination means "different", never "wait for it". On macOS
/// the real case is `EDEADLK` from the fail-fast policy; a missing file is the
/// portable stand-in for the same branch.
#[test]
fn an_unreadable_destination_is_treated_as_different() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sub/gone.txt");
    assert!(write_output_if_changed(&path, b"fresh").unwrap());
    assert_eq!(fs::read(&path).unwrap(), b"fresh");
}

// ----- create_output_dir_all: ADR-043 for directories -----
//
// The branch that matters — `EDEADLK` from a provider refusing to materialize a
// directory entry — needs a real macOS File Provider vault and cannot run on any
// CI moss has, the same limitation as the write path above. What is testable is
// that the ordinary paths are unchanged and that the replacement branch, once
// entered, is a replacement rather than a wait.

#[test]
fn creates_a_missing_directory_chain() {
    let dir = tempdir().unwrap();
    let deep = dir.path().join("a/b/c");
    create_output_dir_all(&deep).unwrap();
    assert!(deep.is_dir());
}

#[test]
fn an_existing_directory_is_left_alone() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("keep");
    fs::create_dir(&sub).unwrap();
    let marker = sub.join("evidence.txt");
    fs::write(&marker, b"still here").unwrap();

    create_output_dir_all(&sub).unwrap();

    assert_eq!(
        fs::read(&marker).unwrap(),
        b"still here",
        "the non-failing path must never remove anything — the whole staging \
         tree lives behind this call"
    );
}

/// A real error that is NOT a cloud refusal must still fail. Removing-and-
/// remaking is licensed by ADR-043 for regenerable output the provider will not
/// hand back; it is not a general "retry harder" for permission errors, which
/// would delete a user's directory to work around a misconfiguration.
#[test]
fn a_non_cloud_error_is_reported_not_worked_around() {
    let dir = tempdir().unwrap();
    let blocker = dir.path().join("blocker");
    fs::write(&blocker, b"i am a file").unwrap();

    let err = create_output_dir_all(&blocker.join("child")).unwrap_err();

    assert_ne!(err.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(
        fs::read(&blocker).unwrap(),
        b"i am a file",
        "a file in the way is a real error, not something to silently delete"
    );
}

// ----- is_regenerable_output: what may be deleted -----
//
// The predicate that stands between a scratch directory and the user's site.
// It is what the `EDEADLK` branch consults, so it is tested directly: the
// branch itself cannot run here, but the answer it depends on can.

#[test]
fn only_paths_below_moss_build_are_regenerable() {
    let yes = [
        "/Users/x/Google Drive/site/.moss/build/staging",
        "/Users/x/site/.moss/build/generations/7/assets",
    ];
    for p in yes {
        assert!(is_regenerable_output(Path::new(p)), "{p} is output moss can remake");
    }
}

/// Every one of these is an ancestor of some output path, so a walk from the
/// filesystem root reaches them all before it reaches `.moss/build`. If any
/// answered `true`, one refused `create_dir` would `remove_dir_all` the user's
/// vault to fix a staging directory.
#[test]
fn a_vault_and_its_content_are_never_regenerable() {
    let no = [
        "/",
        "/Users",
        "/Users/x",
        "/Users/x/Google Drive",
        "/Users/x/Google Drive/site",
        "/Users/x/Google Drive/site/.moss",
        "/Users/x/Google Drive/site/.moss/theme",
        "/Users/x/Google Drive/site/posts",
    ];
    for p in no {
        assert!(!is_regenerable_output(Path::new(p)), "{p} holds something only the user has");
    }
}

/// `.moss/build` itself is excluded deliberately, not by accident of the
/// boundary: remaking it drops every sealed generation at once, and a sealed
/// generation is what keeps a site being read from going dark.
#[test]
fn the_build_root_itself_is_not_replaceable() {
    assert!(!is_regenerable_output(Path::new("/Users/x/site/.moss/build")));
}

/// The four states a presence check has to tell apart. "Empty" and "dataless"
/// are the ones the checks this predicate replaced got wrong: a 0-byte stub
/// and a cloud-only placeholder both look like a file to `is_file()`, and
/// trusting either as "already built" is what makes the next copy fail.
///
/// Dataless itself is not reachable in a test — it needs a real File Provider
/// vault — so `is_evicted` is exercised through its own type's tests and what
/// is proven here is that an empty file is not a present output.
#[test]
fn output_present_separates_a_real_output_from_a_stub() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("never-written.html");
    let empty = dir.path().join("stub.html");
    let real = dir.path().join("page.html");
    fs::write(&empty, b"").unwrap();
    fs::write(&real, b"<h1>hi</h1>").unwrap();

    assert!(!output_present(&missing), "a missing output is absent");
    assert!(!output_present(&empty), "a 0-byte stub is absent, not an output");
    assert!(output_present(&real), "a file with bytes is present");
}

/// A directory is not an output. The manifest never names one, and treating
/// one as present would let a directory standing where a file belongs pass
/// the check and fail the copy.
#[test]
fn output_present_rejects_a_directory() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("current");
    fs::create_dir(&sub).unwrap();
    assert!(!output_present(&sub));
}

/// The symlink arm, which the regular-file test and `is_evicted` are both
/// false for. `120000:` manifest entries ship via `read_link` and are never
/// read through, so a link is present when the link itself is — including a
/// dangling one, whose target is not moss's to produce. A predicate without
/// this arm would unlink every preserved symlink from the generation.
#[cfg(unix)]
#[test]
fn output_present_sees_a_symlink_by_the_link_not_the_target() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("real.html");
    fs::write(&target, b"<h1>hi</h1>").unwrap();

    let live = dir.path().join("live-link");
    std::os::unix::fs::symlink(&target, &live).unwrap();
    assert!(output_present(&live), "a link to a real file is present");

    let dangling = dir.path().join("dangling-link");
    std::os::unix::fs::symlink(dir.path().join("gone.html"), &dangling).unwrap();
    assert!(
        output_present(&dangling),
        "a link is present when the link is — its target is not moss's output"
    );
}

/// The same link, two entries, two answers. `ship_phase` picks its arm from
/// the entry's mode, so presence has to be asked in those terms: a dangling
/// link under a `100644:` entry is bytes that are not there, and calling it
/// present is how a kept entry becomes a failed copy.
#[cfg(unix)]
#[test]
fn entry_output_present_asks_through_the_link_only_for_a_file_entry() {
    use crate::build::io_utils::entry_output_present;
    use crate::types::content::{MODE_FILE, MODE_SYMLINK};

    let dir = tempdir().unwrap();
    let dangling = dir.path().join("dangling-link");
    std::os::unix::fs::symlink(dir.path().join("gone.html"), &dangling).unwrap();

    assert!(entry_output_present(&dangling, MODE_SYMLINK));
    assert!(!entry_output_present(&dangling, MODE_FILE));

    let target = dir.path().join("real.html");
    fs::write(&target, b"<h1>hi</h1>").unwrap();
    let live = dir.path().join("live-link");
    std::os::unix::fs::symlink(&target, &live).unwrap();
    assert!(
        entry_output_present(&live, MODE_FILE),
        "a link that resolves to bytes still ships as a file"
    );
}
