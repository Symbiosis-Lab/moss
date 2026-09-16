use super::*;
use std::io::Write;
use zip::write::FileOptions;
use zip::ZipWriter;

/// Build an in-memory zip from `(name, contents)` regular-file entries.
///
/// `pub(crate)`: `registry_client::artifact`'s tests need a real archive
/// to feed `install_into`, and this is the one place that already knows
/// how to build one — a second copy would drift from this one silently.
pub(crate) fn zip_with_files(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = ZipWriter::new(io::Cursor::new(&mut buf));
        let opts = FileOptions::default().unix_permissions(0o644);
        for (name, contents) in files {
            w.start_file(*name, opts).unwrap();
            w.write_all(contents).unwrap();
        }
        w.finish().unwrap();
    }
    buf
}

#[test]
fn extracts_a_normal_archive_into_target() {
    let zip = zip_with_files(&[
        ("manifest.json", br#"{"name":"x"}"#),
        ("main.bundle.js", b"globalThis.X={}"),
    ]);
    let dir = tempfile::tempdir().unwrap();

    extract_zip_safe(&zip, dir.path()).expect("normal archive must extract");

    let manifest = std::fs::read(dir.path().join("manifest.json")).unwrap();
    assert_eq!(manifest, br#"{"name":"x"}"#);
    let bundle = std::fs::read(dir.path().join("main.bundle.js")).unwrap();
    assert_eq!(bundle, b"globalThis.X={}");
}

#[test]
fn rejects_parent_traversal_entry() {
    // Entry "../escaped.txt" would land in the tempdir root (outside the
    // install target). Kept inside the tempdir so a naive extractor's write
    // is auto-cleaned, but the hardened extractor must reject it.
    let zip = zip_with_files(&[("../escaped.txt", b"pwned")]);
    let base = tempfile::tempdir().unwrap();
    let target = base.path().join("install");

    let err =
        extract_zip_safe(&zip, &target).expect_err("parent-traversal entry must be rejected");
    assert!(
        matches!(err, ZipExtractError::PathTraversal(_)),
        "expected PathTraversal, got {err:?}"
    );
    assert!(
        !base.path().join("escaped.txt").exists(),
        "traversal entry must not be written outside target"
    );
}

#[test]
fn rejects_absolute_path_entry() {
    // An absolute entry name pointing inside our own tempdir: a naive
    // join() replaces the target with the absolute path. Contained here for
    // safety; the hardened extractor must reject it.
    let base = tempfile::tempdir().unwrap();
    let abs_name = base.path().join("abs_escaped.txt");
    let abs_name = abs_name.to_str().unwrap();
    let zip = zip_with_files(&[(abs_name, b"pwned")]);
    let target = base.path().join("install");

    let err =
        extract_zip_safe(&zip, &target).expect_err("absolute-path entry must be rejected");
    assert!(
        matches!(err, ZipExtractError::PathTraversal(_)),
        "expected PathTraversal, got {err:?}"
    );
}

#[test]
fn rejects_symlink_entry() {
    let mut buf = Vec::new();
    {
        let mut w = ZipWriter::new(io::Cursor::new(&mut buf));
        w.add_symlink("link", "/etc/passwd", FileOptions::default())
            .unwrap();
        w.finish().unwrap();
    }
    let dir = tempfile::tempdir().unwrap();

    let err = extract_zip_safe(&buf, dir.path()).expect_err("symlink entry must be rejected");
    assert!(
        matches!(err, ZipExtractError::SymlinkEntry(_)),
        "expected SymlinkEntry, got {err:?}"
    );
}

#[test]
fn rejects_too_many_entries() {
    let zip = zip_with_files(&[("a", b"1"), ("b", b"2"), ("c", b"3")]);
    let dir = tempfile::tempdir().unwrap();

    let err = extract_zip_safe_limited(&zip, dir.path(), 2, u64::MAX)
        .expect_err("entry count over the cap must be rejected");
    assert!(matches!(err, ZipExtractError::TooLarge), "got {err:?}");
}

#[test]
fn rejects_oversize_decompressed_total() {
    let zip = zip_with_files(&[("big.bin", &[0u8; 100])]);
    let dir = tempfile::tempdir().unwrap();

    let err = extract_zip_safe_limited(&zip, dir.path(), usize::MAX, 10)
        .expect_err("decompressed total over the cap must be rejected");
    assert!(matches!(err, ZipExtractError::TooLarge), "got {err:?}");
}

/// Build a zip with one STORED entry whose central-directory
/// `uncompressed_size` header is a LIE (`lied_size`) while the real content
/// is `content`. The size cap must bound ACTUAL decompressed bytes, so a
/// header lie must not slip a bomb past it.
fn zip_with_lying_size(name: &str, content: &[u8], lied_size: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut w = ZipWriter::new(io::Cursor::new(&mut buf));
        let opts = FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o644);
        w.start_file(name, opts).unwrap();
        w.write_all(content).unwrap();
        w.finish().unwrap();
    }
    // Patch the central-directory record's uncompressed_size (offset +24
    // from the PK\x01\x02 signature); `ZipFile::size()` reads this field.
    // compressed_size (offset +20) is left real so the reader still streams
    // the full content.
    let sig = [0x50u8, 0x4b, 0x01, 0x02];
    let pos = buf
        .windows(4)
        .position(|w| w == sig)
        .expect("central directory header present");
    buf[pos + 24..pos + 28].copy_from_slice(&lied_size.to_le_bytes());
    buf
}

#[test]
fn declared_size_lie_cannot_bypass_the_cap() {
    // Real content is 100 bytes; the header claims 1. A cap of 10 must
    // still reject it — the guard bounds bytes actually written, not the
    // attacker-controlled declared size.
    let zip = zip_with_lying_size("bomb.bin", &[7u8; 100], 1);
    let dir = tempfile::tempdir().unwrap();

    let err = extract_zip_safe_limited(&zip, dir.path(), usize::MAX, 10)
        .expect_err("a lying uncompressed_size must not bypass the size cap");
    assert!(matches!(err, ZipExtractError::TooLarge), "got {err:?}");
}

#[test]
fn symlink_mode_detection() {
    assert!(is_symlink_mode(Some(S_IFLNK | 0o777)));
    assert!(!is_symlink_mode(Some(0o100644))); // regular file
    assert!(!is_symlink_mode(Some(0o040755))); // directory
    assert!(!is_symlink_mode(None));
}

/// Test 6 of the S4 plan: the path-taking entry `extract_zip_path_limited`
/// enforces the SAME ceilings its in-memory sibling does, since a stack
/// artifact is too large to buffer just to call `extract_zip_safe`.
#[test]
fn the_path_entry_enforces_the_budget_it_is_given() {
    let zip = zip_with_files(&[("big.bin", &[0u8; 100])]);
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("stack.zip");
    std::fs::write(&archive, &zip).unwrap();

    let target = dir.path().join("install");
    let err = extract_zip_path_limited(&archive, &target, usize::MAX, 1)
        .expect_err("decompressed total over the cap must be rejected");
    assert!(matches!(err, ZipExtractError::TooLarge), "got {err:?}");

    let target = dir.path().join("install-ok");
    extract_zip_path_limited(&archive, &target, MAX_ENTRIES, MAX_TOTAL_UNCOMPRESSED)
        .expect("the real ceiling must extract a small archive");
    assert_eq!(std::fs::read(target.join("big.bin")).unwrap(), vec![0u8; 100]);
}
