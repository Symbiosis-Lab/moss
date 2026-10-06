//! A shared cache shard can be cloud-only. Ask for it once and let the caller
//! use local working bytes or regenerate the optional record.

use super::RecordMode;
use crate::build::cloud_readiness::request_download;
use crate::build::icloud::{is_dataless_dir, is_dataless_unavailable};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Touch {
    Dir,
    File,
}

#[cfg(test)]
fn seam(path: &Path, touch: Touch) -> io::Result<()> {
    if touch == Touch::Dir && path.parent().is_some_and(Path::is_dir) {
        return Ok(());
    }
    crate::build::icloud::pretend::refusal_below(path).map_or(Ok(()), Err)
}

#[cfg(not(test))]
fn seam(_path: &Path, _touch: Touch) -> io::Result<()> {
    Ok(())
}

fn responsible_dir(root: &Path, path: &Path) -> Option<PathBuf> {
    if !path.starts_with(root) {
        return None;
    }
    path.ancestors()
        .skip(1)
        .filter(|dir| dir.starts_with(root))
        .filter(|dir| is_dataless_dir(dir))
        .last()
        .map(Path::to_path_buf)
}

/// Attempt a cache write once. A provider refusal requests the outermost
/// cloud-only directory; the caller can retry on a later build.
pub(super) fn in_shard<T>(
    root: &Path,
    path: &Path,
    _mode: RecordMode,
    touch: Touch,
    op: impl Fn() -> io::Result<T>,
) -> io::Result<T> {
    let result = seam(path, touch).and_then(|()| op());
    if result.as_ref().err().is_some_and(is_dataless_unavailable) {
        if let Some(dir) = responsible_dir(root, path) {
            request_download(&dir);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn a_cloud_only_shard_is_requested_once_without_retrying_the_write() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("objects");
        let shard = root.join("ab");
        std::fs::create_dir_all(&shard).unwrap();
        let _cloud = crate::build::icloud::pretend::evicted_until_requested(&shard);
        let dest = shard.join("blob");
        let attempts = std::cell::Cell::new(0);
        let first = in_shard(&root, &dest, RecordMode::Wait, Touch::File, || {
            attempts.set(attempts.get() + 1);
            std::fs::write(&dest, b"bytes")
        });
        assert!(first.is_err());
        assert_eq!(attempts.get(), 0, "the test provider refused before writing");
        assert_eq!(crate::build::icloud::pretend::requests_for(&shard), 1);
        in_shard(&root, &dest, RecordMode::Wait, Touch::File, || std::fs::write(&dest, b"bytes"))
            .expect("next build can write after the request");
    }
}
