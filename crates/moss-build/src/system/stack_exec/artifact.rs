//! `obtain`: turn a stack's declared source into bytes on disk, or refuse
//! before any of them move. A `Download` source is cached under a
//! version-keyed name and its mandatory sha256 checked before it is ever
//! handed back; a `Path` source has nothing to fetch at all.

use std::path::{Path, PathBuf};

use crate::plugins::contributions::stack::StackSource;
use crate::plugins::install::registry_client::receipt::is_placeholder_sha256;
use crate::system::large_download::{self, ProgressFn};

use super::{Artifact, StackExecError, StackHome};

/// `{cache_dir}/{id}-{version}.{format}` — version-keyed so a manifest bump
/// never reuses the previous release's bytes, and keyed on the declaration's
/// own id rather than any one stack's name.
fn cache_path(cache_dir: &Path, id: &str, version: &str, format: &str) -> PathBuf {
    cache_dir.join(format!("{id}-{version}.{format}"))
}

/// Delete cached artifacts (and their in-flight partials) for `stack_id`
/// other than `keep`. Identity is the declaration's id, not a hardcoded
/// vendor name, so a second stack's cache is never touched by this one's
/// pruning and vice versa.
fn prune_stale_cache(cache_dir: &Path, keep: &Path, stack_id: &str) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    let keep_part = large_download::part_path(keep);
    let prefix = format!("{stack_id}-");
    for entry in entries.flatten() {
        let path = entry.path();
        let is_this_stacks_artifact = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(&prefix));
        if is_this_stacks_artifact && path != keep && path != keep_part {
            log::info!("[stack-exec] pruning stale cached artifact {}", path.display());
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// `Ok(true)`: a file already at `dest` matches `expected_sha256` — adopt it,
/// no request made. `Ok(false)`: nothing is at `dest` — fall through to
/// [`large_download::fetch_verified`]. `Err(HashMismatch)`: something is at
/// `dest` and it does NOT match — removed before returning, because a file
/// under a version-keyed name that hashes wrong is corruption or a swap, not
/// a value worth quietly re-downloading over (that is `fetch_verified`'s own
/// behavior, and deliberately not this one's).
fn adopt_or_refuse_cached(dest: &Path, url: &str, expected_sha256: &str) -> Result<bool, StackExecError> {
    if !dest.is_file() {
        return Ok(false);
    }
    let actual = large_download::file_sha256(dest).map_err(StackExecError::Obtain)?;
    if actual.eq_ignore_ascii_case(expected_sha256) {
        return Ok(true);
    }
    std::fs::remove_file(dest)
        .map_err(|e| StackExecError::Obtain(format!("remove mismatched cached artifact {}: {e}", dest.display())))?;
    Err(StackExecError::HashMismatch {
        url: url.to_string(),
        expected: expected_sha256.to_string(),
        actual,
    })
}

/// Download (or adopt the cached copy), verify the mandatory sha256, and
/// return the artifact. Refuses a missing or placeholder hash before the
/// first byte moves. A `Path` source has no artifact: this returns
/// `Artifact::AlreadyPresent` without touching the cache or the network.
pub fn obtain(
    home: &StackHome<'_>,
    cache_dir: &Path,
    on_bytes: Option<&ProgressFn<'_>>,
) -> Result<Artifact, StackExecError> {
    let source = super::layout::winning_source(home.stack).ok_or_else(|| StackExecError::NoSourceForPlatform {
        stack: home.stack.id.clone(),
    })?;
    let (url, sha256, format) = match source {
        StackSource::Path { .. } => return Ok(Artifact::AlreadyPresent),
        StackSource::Download { url, sha256, archive_format, .. } => (url, sha256, archive_format),
    };
    let sha256 = match sha256 {
        None => {
            return Err(StackExecError::MissingHash {
                stack: home.stack.id.clone(),
                url: url.clone(),
            })
        }
        Some(s) if is_placeholder_sha256(s) => {
            return Err(StackExecError::PlaceholderHash {
                stack: home.stack.id.clone(),
                url: url.clone(),
            })
        }
        Some(s) => s,
    };

    let dest = cache_path(cache_dir, &home.stack.id, &home.stack.version, format);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| StackExecError::Obtain(format!("create {}: {e}", dir.display())))?;
        prune_stale_cache(dir, &dest, &home.stack.id);
    }

    if adopt_or_refuse_cached(&dest, url, sha256)? {
        return Ok(Artifact::File(dest));
    }

    large_download::fetch_verified(url, &dest, sha256, &format!("[stack-exec:{}]", home.stack.id), on_bytes)
        .map_err(StackExecError::Obtain)?;
    Ok(Artifact::File(dest))
}

#[cfg(test)]
#[path = "artifact_tests.rs"]
mod artifact_tests;
