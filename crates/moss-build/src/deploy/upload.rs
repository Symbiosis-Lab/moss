//! What a deploy checks before it sends a file: that the bytes match the
//! manifest entry, and when they do not, whether the file has settled into a
//! version it may ship instead (a self-heal), within the deploy's cap.
//!
//! Every sizing number is in `seta::upload_policy`.

use std::path::Path;

use crate::seta::upload_policy;

/// Digest of an in-memory buffer — the manifest's xxh3_64, as lowercase hex.
fn hash_bytes(bytes: &[u8]) -> String {
    crate::build::assets::paths::compute_binary_hash(bytes)
}

/// Digest of a file read incrementally — identical output to [`hash_bytes`]
/// over the same content, without buffering it.
///
/// Streaming keeps the integrity check at every file size, including files
/// too large to buffer.
///
/// It bumps the stall clock per buffer. This runs for every chunked
/// file *before* its first request, so it credits no bytes and emits no
/// progress: without the bump, hashing a large video on a slow disk (or an
/// iCloud file that is materialising) would be indistinguishable from a
/// wedged publish and `activity::watchdog` would cancel it during work that
/// is going perfectly well.
/// [`hash_file`] on a blocking thread, so many files hash at once.
async fn hash_file_blocking(path: &Path) -> Result<String, String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || hash_file(&path)).await.map_err(|e| format!("Failed to hash a file: {e}"))?
}

fn hash_file(path: &Path) -> Result<String, String> {
    crate::build::assets::paths::compute_binary_hash_file_with_heartbeat(path, &|| {
        crate::infra::liveness::bump()
    })
}

/// Compare a digest against the manifest's, tolerating a manifest entry with no
/// hash at all.
///
/// An empty `expected` means a legacy or regressed manifest entry that carries
/// no hash. Skipping is deliberate: failing the deploy would strand a user
/// behind a stale build cache with no way forward, and the server's own
/// completeness gate still applies.
fn compare(file_path: &str, expected: &str, actual: &str) -> Result<(), String> {
    if expected.is_empty() {
        log::debug!(
            "[deploy] no hash in manifest entry for {file_path} — skipping integrity check \
             (re-build to generate one)"
        );
        return Ok(());
    }
    if expected != actual {
        return Err(format!(
            "Deploy integrity error: file bytes do not match sealed manifest for {file_path} \
             (expected {expected}, got {actual})"
        ));
    }
    Ok(())
}

/// Verify an in-memory body against its manifest hash.
///
/// Kept as the buffered form for a deploy send, which already holds the
/// exact bytes it is about to send. That is a strictly stronger check than
/// re-reading the path: it cannot be defeated by the file changing between the
/// hash and the send.
pub(crate) fn verify_bytes(
    file_path: &str,
    bytes: &[u8],
    expected: &str,
) -> Result<(), String> {
    compare(file_path, expected, &hash_bytes(bytes))
}

/// Settle pause between the first and second read of a file whose hash
/// didn't match the sealed manifest, before trusting it enough to self-heal.
///
/// A single fresh read is not proof of anything: a torn read mid-write looks
/// exactly like a finished rebuild's bytes, and only re-checking after a
/// pause tells the two apart. 250ms is long enough for a write in progress to
/// land and short enough to go unnoticed beside an upload.
pub(crate) const DRIFT_SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// How many leading bytes to sample when checking whether a file looks like
/// a zeroed-out iCloud eviction stub, without reading a large chunked file in
/// full. Matches the streaming hasher's own read buffer size ([`hash_file`]).
const ZEROED_STUB_SAMPLE_BYTES: usize = 64 * 1024;

/// The one corruption shape this module knows how to recognize by content:
/// iCloud's "optimize storage" eviction zeroes a file in place, leaving a
/// stub of the right length and the wrong (all-zero) bytes —
/// `a_zeroed_icloud_stub_is_rejected_before_it_is_uploaded` is the fixture
/// this exists for. A torn write mid-rebuild does not look like this: it
/// leaves whatever partial content the writer had actually flushed, which is
/// essentially never all zero, so this check does not fire on the race this
/// module exists to tolerate.
fn looks_like_zeroed_stub(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(|&b| b == 0)
}

/// Read up to `n` leading bytes of `path` — a cheap corruption sniff that
/// does not require loading a large chunked file in full just to check
/// whether it looks like a zeroed stub.
async fn read_leading_bytes(path: &Path, n: usize) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    // `path` is always the sealed generation's `canonical` file under
    // `.moss/build.nosync/generations/`, not a vault input; the bytes only feed
    // looks_like_zeroed_stub below, so a torn or corrupted read is caught by
    // content rather than trusted as the file's real state.
    // allow:raw_read built output — dataless is absent
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let mut buf = vec![0u8; n];
    let read = file
        .read(&mut buf)
        .await
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    buf.truncate(read);
    Ok(buf)
}

/// How many self-heals one deploy tolerates before treating the pattern
/// itself as the problem.
///
/// A drifted hash or two in one deploy is exactly the race this module
/// exists to tolerate — a background rebuild racing the first one. More than
/// that, in the SAME deploy, is not the same failure: it is evidence of
/// something systemic (a stale or wrong stage directory, a build that never
/// finished) that healing file-by-file would otherwise paper over silently,
/// one warning at a time, until nobody notices the publish shipped mostly
/// self-healed content. Floor of 3 so a small publish isn't capped at zero; 5%
/// beyond that so a large one's tolerance scales with it.
///
/// `uploading` is how many files THIS deploy uploads (what
/// the server asked for), not how many the site has: an incremental publish
/// uploads a handful of files, so its cap is the floor of 3 however large the
/// site is. That is deliberate — the cap is a share of what this deploy is
/// moving, since only those files can drift from the sealed manifest in it.
pub(crate) fn self_heal_cap(uploading: usize) -> usize {
    (uploading / 20).max(3)
}

/// Enforce [`self_heal_cap`] against one more self-heal, a file's or a
/// link's. Bumps the shared counter and, once the cap is crossed, returns the
/// failure that ends the publish instead of quietly healing every remaining file.
pub(crate) fn charge_self_heal(
    throughput: &upload_policy::Throughput,
    cap: usize,
    file_path: &str,
) -> Result<(), String> {
    let n = throughput.note_self_heal();
    if n as usize > cap {
        return Err(format!(
            "deploy: {n} files have drifted from the sealed manifest in this publish (cap {cap}) \
             — likely something systemic (a stale build stage, a wrong directory) rather than an \
             isolated race; aborting instead of self-healing the rest (triggered by {file_path})"
        ));
    }
    Ok(())
}

/// The hash `file_path`'s bytes at `canonical` have settled on, when they no
/// longer match `expected`; `None` when they match. A file that differs is
/// trusted only if it reads the same after [`DRIFT_SETTLE_DELAY`] and is not a
/// zeroed stub, within the deploy's self-heal cap.
pub(crate) async fn settled_hash(
    file_path: &str,
    canonical: &Path,
    expected: &str,
    throughput: &upload_policy::Throughput,
    self_heal_cap: usize,
) -> Result<Option<String>, String> {
    let first = hash_file_blocking(canonical).await?;
    let Err(mismatch) = compare(file_path, expected, &first) else { return Ok(None) };
    tokio::time::sleep(DRIFT_SETTLE_DELAY).await;
    if hash_file_blocking(canonical).await? != first {
        return Err(format!("{mismatch} (still changing {DRIFT_SETTLE_DELAY:?} later — not self-healing a moving target)"));
    }
    if looks_like_zeroed_stub(&read_leading_bytes(canonical, ZEROED_STUB_SAMPLE_BYTES).await?) {
        return Err(format!("{mismatch} (looks like a zeroed stub, not a rebuilt file — refusing to self-heal)"));
    }
    charge_self_heal(throughput, self_heal_cap, file_path)?;
    log::warn!("[deploy] {mismatch} — stable after a {DRIFT_SETTLE_DELAY:?} settle pause, uploading the bytes actually on disk instead");
    Ok(Some(first))
}

/// The byte length of every regular-file entry in `manifest`, read from `dir`.
///
/// Open sends this as `sizes`, so the server can refuse a publish over the
/// site's storage cap before any upload. A symlink entry carries a target string,
/// not content, so it has none. A file is stat'd through any link on disk, as
/// its upload reads it. An entry that cannot be stat'd is left out rather than
/// reported as 0, and its upload fails with its own error. The stats run off
/// the async runtime: a large site has thousands.
pub(crate) async fn regular_file_sizes(
    dir: &Path,
    manifest: &std::collections::HashMap<String, String>,
) -> Result<std::collections::HashMap<String, u64>, String> {
    use crate::types::content::{parse_entry, MODE_FILE};
    let dir = dir.to_path_buf();
    let files: Vec<String> = manifest
        .iter()
        .filter(|(_, entry)| parse_entry(entry).0 == MODE_FILE)
        .map(|(path, _)| path.clone())
        .collect();
    tokio::task::spawn_blocking(move || {
        files
            .into_iter()
            .filter_map(|path| {
                let meta = std::fs::metadata(dir.join(&path)).ok()?;
                meta.is_file().then(|| (path, meta.len()))
            })
            .collect()
    })
    .await
    .map_err(|e| format!("Failed to read file sizes: {e}"))
}

#[cfg(test)]
#[path = "upload_tests.rs"]
mod tests;
