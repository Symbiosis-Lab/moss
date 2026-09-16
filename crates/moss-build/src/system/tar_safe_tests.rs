use super::*;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write as _;
use tar::{Builder, EntryType, Header};
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

/// A `.tar.gz` with one entry of `entry_type` at `name` (raw name bytes,
/// bypassing `Header::set_path`'s own refusal of `..`/absolute paths — a
/// real hostile archive has no such scruples) and `mode`.
fn tar_gz_with_named_entry(name: &str, entry_type: EntryType, mode: u32, contents: &[u8]) -> Vec<u8> {
    let mut header = Header::new_gnu();
    let name_bytes = name.as_bytes();
    header.as_old_mut().name[..name_bytes.len()].copy_from_slice(name_bytes);
    header.set_entry_type(entry_type);
    header.set_mode(mode);
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

fn tar_gz_with_entry(name: &str, contents: &[u8]) -> Vec<u8> {
    tar_gz_with_named_entry(name, EntryType::Regular, 0o644, contents)
}

fn tar_gz_with_symlink(name: &str, target: &str) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = Builder::new(&mut tar_bytes);
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        builder.append_link(&mut header, name, target).unwrap();
        builder.finish().unwrap();
    }
    gzip(&tar_bytes)
}

fn tar_gz_with_hardlink_entry(name: &str, target: &str) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = Builder::new(&mut tar_bytes);
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Link);
        header.set_size(0);
        builder.append_link(&mut header, name, target).unwrap();
        builder.finish().unwrap();
    }
    gzip(&tar_bytes)
}

fn encode_octal12(value: u64) -> [u8; 12] {
    let mut buf = [b'0'; 12];
    buf[11] = 0;
    let digits = format!("{value:o}");
    let bytes = digits.as_bytes();
    let start = 11 - bytes.len();
    buf[start..11].copy_from_slice(bytes);
    buf
}

/// A `.tar.gz` with one GNU-sparse entry whose on-disk (archived) footprint
/// is `footprint.len()` bytes, but whose declared real size — the size
/// after the sparse hole is filled in with zeroes on read — is `real_size`.
/// The tar-native equivalent of zip's declared-size lie: the header's plain
/// `size` field stays small, while the actual byte count the entry streams
/// on read equals `real_size`.
fn tar_gz_with_sparse_hole(name: &str, footprint: &[u8], real_size: u64) -> Vec<u8> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::GNUSparse);
    header.set_path(name).unwrap();
    header.set_size(footprint.len() as u64);
    {
        let gnu = header.as_gnu_mut().unwrap();
        gnu.set_real_size(real_size);
        gnu.sparse[0].offset = encode_octal12(real_size - footprint.len() as u64);
        gnu.sparse[0].numbytes = encode_octal12(footprint.len() as u64);
    }
    header.set_cksum();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = Builder::new(&mut tar_bytes);
        builder.append(&header, footprint).unwrap();
        builder.finish().unwrap();
    }
    gzip(&tar_bytes)
}

/// A `.tar.gz` with `count` distinct regular-file entries, `name-0`
/// through `name-<count-1>`.
fn tar_gz_with_n_entries(count: usize) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = Builder::new(&mut tar_bytes);
        for i in 0..count {
            let mut header = Header::new_gnu();
            header.set_path(format!("file-{i}")).unwrap();
            header.set_size(0);
            header.set_cksum();
            builder.append(&header, std::io::empty()).unwrap();
        }
        builder.finish().unwrap();
    }
    gzip(&tar_bytes)
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[test]
fn a_climbing_entry_is_refused() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    let bytes = tar_gz_with_entry("../escape.txt", b"pwned");
    let err = extract_tar_gz(&bytes[..], &target, SymlinkPolicy::Reject, 100, 1_000_000)
        .expect_err("a climbing entry must be refused");
    assert!(matches!(err, TarSafeError::UnsafeEntry { .. }), "got {err:?}");
    assert!(!base.path().join("escape.txt").exists(), "must not land outside the target dir");
}

#[test]
fn an_absolute_entry_is_refused() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    let bytes = tar_gz_with_entry("/tmp/moss-tar-safe-pwn", b"pwned");
    let err = extract_tar_gz(&bytes[..], &target, SymlinkPolicy::Reject, 100, 1_000_000)
        .expect_err("an absolute entry must be refused");
    assert!(matches!(err, TarSafeError::UnsafeEntry { .. }), "got {err:?}");
    assert!(!Path::new("/tmp/moss-tar-safe-pwn").exists(), "must not land at the absolute path");
}

#[test]
fn a_directory_entry_normalizing_to_empty_is_accepted_and_ignored_but_a_file_entry_is_refused() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    let dir_only = tar_gz_with_named_entry("./", EntryType::Directory, 0o755, b"");
    extract_tar_gz(&dir_only[..], &target, SymlinkPolicy::Reject, 100, 1_000_000)
        .expect("a directory entry normalizing to empty must be accepted and ignored");

    let file_variant = tar_gz_with_named_entry("./", EntryType::Regular, 0o644, b"pwned");
    let err = extract_tar_gz(&file_variant[..], &target, SymlinkPolicy::Reject, 100, 1_000_000)
        .expect_err("a file entry normalizing to empty names nothing to write and must be refused");
    assert!(matches!(err, TarSafeError::UnsafeEntry { .. }), "got {err:?}");
}

#[test]
fn a_hard_link_entry_is_refused_under_either_policy() {
    for policy in [SymlinkPolicy::Reject, SymlinkPolicy::ContainedRelative] {
        let base = TempDir::new().unwrap();
        let target = base.path().join("install");

        let bytes = tar_gz_with_hardlink_entry("link", "some/other/file");
        let err = extract_tar_gz(&bytes[..], &target, policy, 100, 1_000_000)
            .expect_err("a hardlink entry must be refused");
        assert!(matches!(err, TarSafeError::UnsafeEntry { .. }), "got {err:?}");
        assert!(!target.join("link").exists(), "no write must happen before the refusal");
    }
}

#[test]
fn a_fifo_entry_is_skipped_and_nothing_is_written() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    let bytes = tar_gz_with_named_entry("a-fifo", EntryType::Fifo, 0o644, b"");
    extract_tar_gz(&bytes[..], &target, SymlinkPolicy::Reject, 100, 1_000_000)
        .expect("a fifo entry must be skipped, not refused");
    assert!(!target.join("a-fifo").exists(), "a fifo entry must never be materialized");
}

#[test]
fn a_lying_header_cannot_bypass_the_byte_budget() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    // 4 archived bytes, expanding on read to 1,000,000 bytes via a sparse
    // hole. A budget of 100 must still refuse it.
    let bytes = tar_gz_with_sparse_hole("bomb.bin", b"WXYZ", 1_000_000);
    let err = extract_tar_gz(&bytes[..], &target, SymlinkPolicy::Reject, 100, 100)
        .expect_err("the expanded size must be refused");
    assert!(matches!(err, TarSafeError::Io(_)), "got {err:?}");
}

#[test]
fn the_entry_count_budget_is_refused() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    let bytes = tar_gz_with_n_entries(2);
    let err = extract_tar_gz(&bytes[..], &target, SymlinkPolicy::Reject, 1, 1_000_000)
        .expect_err("exceeding the entry-count budget must be refused");
    assert!(matches!(err, TarSafeError::Io(_)), "got {err:?}");
}

#[cfg(unix)]
#[test]
fn a_0o777_file_lands_0o755_group_and_other_write_are_dropped() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    let bytes = tar_gz_with_named_entry("widget.bin", EntryType::Regular, 0o777, b"contents");
    extract_tar_gz(&bytes[..], &target, SymlinkPolicy::Reject, 100, 1_000_000).unwrap();

    assert_eq!(mode_of(&target.join("widget.bin")), 0o755);
}

#[cfg(unix)]
#[test]
fn a_non_declared_file_at_0o700_keeps_its_mode() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    let bytes = tar_gz_with_named_entry("private.bin", EntryType::Regular, 0o700, b"contents");
    // Calling `extract_tar_gz` directly, never through `stage`, whose
    // post-hoc chmod of the declared binary would make this test green for
    // the wrong reason.
    extract_tar_gz(&bytes[..], &target, SymlinkPolicy::Reject, 100, 1_000_000).unwrap();

    assert_eq!(mode_of(&target.join("private.bin")), 0o700);
}

#[test]
fn a_symlink_entry_is_refused_under_reject() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    let bytes = tar_gz_with_symlink("link", "some/target");
    let err = extract_tar_gz(&bytes[..], &target, SymlinkPolicy::Reject, 100, 1_000_000)
        .expect_err("a symlink entry must be refused under SymlinkPolicy::Reject");
    assert!(matches!(err, TarSafeError::UnsafeEntry { .. }), "got {err:?}");
    assert!(!target.join("link").exists(), "no write must happen before the refusal");
}

#[test]
fn a_symlink_with_an_absolute_or_climbing_target_is_refused_under_contained_relative() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");
    let absolute = tar_gz_with_symlink("link", "/etc/passwd");
    let err = extract_tar_gz(&absolute[..], &target, SymlinkPolicy::ContainedRelative, 100, 1_000_000)
        .expect_err("a symlink with an absolute target must be refused");
    assert!(matches!(err, TarSafeError::UnsafeEntry { .. }), "got {err:?}");

    let base2 = TempDir::new().unwrap();
    let target2 = base2.path().join("install");
    let climbing = tar_gz_with_symlink("dir/link", "../../escape");
    let err = extract_tar_gz(&climbing[..], &target2, SymlinkPolicy::ContainedRelative, 100, 1_000_000)
        .expect_err("a symlink whose target contains .. must be refused");
    assert!(matches!(err, TarSafeError::UnsafeEntry { .. }), "got {err:?}");
}

#[cfg(unix)]
#[test]
fn a_contained_dot_dot_free_symlink_is_materialized_under_contained_relative() {
    let base = TempDir::new().unwrap();
    let target = base.path().join("install");

    let bytes = tar_gz_with_symlink("Current", "A");
    extract_tar_gz(&bytes[..], &target, SymlinkPolicy::ContainedRelative, 100, 1_000_000)
        .expect("a contained, ..-free symlink must be materialized");

    assert_eq!(std::fs::read_link(target.join("Current")).unwrap(), PathBuf::from("A"));
}
