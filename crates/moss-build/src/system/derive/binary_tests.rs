use super::*;
use crate::system::derive::test_support::{entry, script};
use tempfile::TempDir;

#[test]
fn a_missing_binary_says_it_is_not_installed() {
    let err = resolve_binary(&entry(std::path::Path::new("/nonexistent/unpack"), "1.0.0")).unwrap_err();
    assert!(err.message.contains("not installed"), "{err}");
}

#[test]
fn a_path_source_needs_no_download() {
    let dir = TempDir::new().unwrap();
    let bin = script(dir.path(), "d.sh", "true");
    assert_eq!(ensure_binary(&entry(&bin, "1.0.0"), None).unwrap(), bin);
}

fn download_entry(url: &str, sha256: &str) -> DerivationEntry {
    let mut e = entry(std::path::Path::new("/unused"), "1.0.0");
    e.contribution.run.artifact = serde_json::from_value(serde_json::json!([
        {"kind": "download", "url": url, "sha256": sha256, "archive_format": "tar.gz"}
    ]))
    .unwrap();
    e
}

#[test]
fn the_same_executable_keys_alike_downloaded_or_local() {
    let dir = TempDir::new().unwrap();
    let bin = script(dir.path(), "d.sh", "true");
    let local = entry(&bin, "1.0.0");
    let download = download_entry("https://example.test/a.tar.gz", &"a".repeat(64));
    assert_eq!(fingerprint(&local, Some(&bin)), fingerprint(&download, Some(&bin)));
}

#[test]
fn the_key_follows_bytes_and_fragments_not_locations() {
    let dir = TempDir::new().unwrap();
    let bin = script(dir.path(), "d.sh", "true");
    let other = script(dir.path(), "e.sh", "false");
    let e = entry(&bin, "1.0.0");
    let base = fingerprint(&e, Some(&bin));
    assert_ne!(fingerprint(&e, Some(&other)), base, "different bytes");
    let mut fragments = e.clone();
    fragments.contribution.fragments.clear();
    assert_ne!(fingerprint(&fragments, Some(&bin)), base, "different fragments");
    let elsewhere = TempDir::new().unwrap();
    let moved = script(elsewhere.path(), "d.sh", "true");
    assert_eq!(fingerprint(&entry(&moved, "1.0.0"), Some(&moved)), base, "same bytes elsewhere");

    let a = download_entry("https://one.test/a.tar.gz", &"a".repeat(64));
    let url = download_entry("https://two.test/b.tar.gz", &"a".repeat(64));
    let sha = download_entry("https://one.test/a.tar.gz", &"b".repeat(64));
    assert_eq!(fingerprint(&a, None), fingerprint(&url, None), "a changed URL alone");
    assert_ne!(fingerprint(&a, None), fingerprint(&sha, None), "a re-pinned archive");
}
