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
