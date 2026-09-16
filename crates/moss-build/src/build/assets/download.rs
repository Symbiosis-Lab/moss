//! Shared download infrastructure with byte-level progress reporting.
//!
//! Provides reusable HTTP download, SHA-256 verification, and disk space checking
//! for use by both git download and FFmpeg download modules.
//!
//! # Design
//!
//! - Uses `ureq` (sync HTTP client) to avoid async runtime conflicts with Tauri.
//! - Reads in 64KB chunks for granular progress reporting.
//! - Progress callback is optional — callers can pass `None` to skip reporting.
//!
//! # Example
//!
//! ```rust,ignore
//! use moss::build::assets::download::{download_with_progress, verify_sha256};
//!
//! let data = download_with_progress(
//!     "https://example.com/file.tar.gz",
//!     300,
//!     Some(&|downloaded, total| {
//!         println!("Downloaded {} / {:?} bytes", downloaded, total);
//!     }),
//! )?;
//!
//! verify_sha256(&data, "expected_hex_hash")?;
//! ```

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// Chunk size for progress-reporting reads: 64KB.
const CHUNK_SIZE: usize = 64 * 1024;

/// Progress callback type: `(bytes_downloaded, total_bytes_option)`.
///
/// Called after each 64KB chunk is read. `total_bytes_option` is `None` when the
/// server does not provide a `Content-Length` header.
pub type DownloadProgress = dyn Fn(u64, Option<u64>) + Send + Sync;

/// Maximum download size: 500 MB. Rejects responses larger than this to prevent
/// accidental multi-GB allocations from malformed Content-Length headers.
const MAX_DOWNLOAD_SIZE: u64 = 500 * 1024 * 1024;

/// Downloads a URL to bytes with optional chunk-level progress reporting.
///
/// Uses `ureq` (sync HTTP client) which automatically follows redirects (up to 5),
/// so CDN redirect chains (common for GitHub Releases) are handled transparently.
///
/// Note: `timeout_secs` controls the HTTP connection/response timeout per request,
/// not the total wall-clock time for the entire download.
///
/// # Arguments
///
/// * `url` - The URL to download from.
/// * `timeout_secs` - HTTP request timeout in seconds (e.g. 300 for 5 minutes).
/// * `on_progress` - Optional callback invoked after each 64KB chunk with
///   `(bytes_downloaded_so_far, total_bytes_or_none)`.
///
/// # Errors
///
/// Returns `Err(String)` if the HTTP request fails, the response body cannot be read,
/// or the Content-Length exceeds the maximum download size (500 MB).
pub fn download_with_progress(
    url: &str,
    timeout_secs: u64,
    on_progress: Option<&DownloadProgress>,
) -> Result<Vec<u8>, String> {
    // Route through the same system proxy the WKWebView (and the plugin HTTP
    // host-fns) use — resolved per-URL via crate::system::proxy. Without this,
    // a bare `ureq::get` connects DIRECTLY, which on a GFW/proxied network
    // crawls (e.g. release-assets.githubusercontent.com throttled to ~20 KB/s)
    // and the download appears stuck at 0%. Mirrors `proxied_agent` in
    // plugins/runtime/download.rs.
    let mut builder = ureq::AgentBuilder::new().timeout(Duration::from_secs(timeout_secs));
    if let Some(proxy_url) = crate::system::proxy::resolve_proxy_for_url(url) {
        match ureq::Proxy::new(&proxy_url) {
            Ok(proxy) => builder = builder.proxy(proxy),
            Err(e) => log::warn!(
                "download: resolved proxy '{}' is invalid ({}); connecting directly",
                proxy_url,
                e
            ),
        }
    }
    let response = builder
        .build()
        .get(url)
        .call()
        .map_err(|e| format!("HTTP request failed for {}: {}", url, e))?;

    // Read Content-Length header for total size (may be absent).
    let total_bytes: Option<u64> = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());

    // Reject downloads that are too large.
    if let Some(len) = total_bytes {
        if len > MAX_DOWNLOAD_SIZE {
            return Err(format!(
                "Download too large: {} MB (max {} MB)",
                len / (1024 * 1024),
                MAX_DOWNLOAD_SIZE / (1024 * 1024),
            ));
        }
    }

    // Pre-allocate buffer if we know the total size.
    let mut bytes: Vec<u8> = match total_bytes {
        Some(len) => Vec::with_capacity(len as usize),
        None => Vec::new(),
    };

    let mut reader = response.into_reader();
    let mut chunk = [0u8; CHUNK_SIZE];
    let mut downloaded: u64 = 0;

    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("Failed to read response body from {}: {}", url, e))?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        downloaded += n as u64;

        // Enforce size limit even when Content-Length is absent or lies.
        if downloaded > MAX_DOWNLOAD_SIZE {
            return Err(format!(
                "Download exceeded maximum size ({} MB) while streaming from {}",
                MAX_DOWNLOAD_SIZE / (1024 * 1024),
                url,
            ));
        }

        if let Some(cb) = on_progress {
            cb(downloaded, total_bytes);
        }
    }

    Ok(bytes)
}

/// Verifies that the SHA-256 hash of `data` matches the expected hex string.
///
/// # Arguments
///
/// * `data` - The bytes to hash.
/// * `expected_hex` - The expected SHA-256 hash as a lowercase hex string.
///
/// # Errors
///
/// Returns `Err(String)` with a descriptive message if the hash does not match.
pub fn verify_sha256(data: &[u8], expected_hex: &str) -> Result<(), String> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let actual_hex = hex::encode(result);

    if actual_hex == expected_hex.to_lowercase() {
        Ok(())
    } else {
        Err(format!(
            "SHA-256 mismatch: expected {}, got {}",
            expected_hex.to_lowercase(),
            actual_hex,
        ))
    }
}

/// Pre-flight check that sufficient disk space is available at `path`.
///
/// Uses `libc::statvfs` on Unix systems for a direct, dependency-free check.
///
/// # Arguments
///
/// * `path` - A path on the filesystem to check (must exist, or a parent must exist).
/// * `required_bytes` - The minimum number of free bytes required.
///
/// # Errors
///
/// Returns `Err(String)` if the available space is less than `required_bytes`,
/// or if the filesystem cannot be queried.
#[cfg(unix)]
pub fn check_disk_space(path: &Path, required_bytes: u64) -> Result<(), String> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;

    // Find an existing ancestor directory to stat.
    let check_path = find_existing_ancestor(path)?;

    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(check_path.as_os_str().as_bytes())
        .map_err(|e| format!("Invalid path for statvfs: {}", e))?;

    let mut stat = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: c_path is a valid null-terminated CString. stat is an uninitialized
    // MaybeUninit<statvfs> — statvfs writes to it via the out-pointer. We check
    // the return value before using the result.
    let ret = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };

    if ret != 0 {
        return Err(format!(
            "Failed to query disk space for {}: statvfs returned {}",
            check_path.display(),
            std::io::Error::last_os_error(),
        ));
    }

    // SAFETY: statvfs returned 0 (success), guaranteeing the struct was fully initialized.
    let stat = unsafe { stat.assume_init() };
    // Available blocks for non-privileged users * fragment size.
    // Casts are intentional for cross-platform portability (f_bavail/f_frsize
    // types vary across Unix targets).
    #[allow(clippy::unnecessary_cast)]
    let available = stat.f_bavail as u64 * stat.f_frsize as u64;

    if available < required_bytes {
        let available_mb = available / (1024 * 1024);
        let required_mb = required_bytes / (1024 * 1024);
        Err(format!(
            "Insufficient disk space: {} MB available, {} MB required",
            available_mb, required_mb,
        ))
    } else {
        Ok(())
    }
}

/// Fallback for non-Unix platforms (Windows).
#[cfg(not(unix))]
pub fn check_disk_space(_path: &Path, _required_bytes: u64) -> Result<(), String> {
    // TODO: Implement disk space check for non-Unix platforms.
    // For now, skip the check and let downloads proceed.
    Ok(())
}

/// Walks up from `path` to find the first existing ancestor directory.
fn find_existing_ancestor(path: &Path) -> Result<std::path::PathBuf, String> {
    let mut current = path.to_path_buf();
    loop {
        if current.exists() {
            return Ok(current);
        }
        if !current.pop() {
            return Err(format!(
                "Cannot find any existing ancestor for path: {}",
                path.display()
            ));
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    // -------------------------------------------------------------------------
    // verify_sha256 tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_verify_sha256_valid() {
        // SHA-256 of "hello world" (no newline)
        let data = b"hello world";
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(verify_sha256(data, expected).is_ok());
    }

    #[test]
    fn test_verify_sha256_valid_uppercase() {
        // Verify that uppercase hex is also accepted.
        let data = b"hello world";
        let expected = "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9";
        assert!(verify_sha256(data, expected).is_ok());
    }

    #[test]
    fn test_verify_sha256_invalid() {
        let data = b"hello world";
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let result = verify_sha256(data, wrong_hash);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("SHA-256 mismatch"),
            "Error should mention SHA-256 mismatch, got: {}",
            err
        );
        assert!(
            err.contains("b94d27b9"),
            "Error should include actual hash, got: {}",
            err
        );
    }

    #[test]
    fn test_verify_sha256_empty_data() {
        // SHA-256 of empty input is a well-known constant.
        let data = b"";
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(verify_sha256(data, expected).is_ok());
    }

    // -------------------------------------------------------------------------
    // download_with_progress tests
    // -------------------------------------------------------------------------

    #[test]
    #[ignore] // Requires network access to httpbin.org — run with `cargo test -- --ignored`
    fn test_download_with_progress_chunks() {
        // Download 1024 bytes from httpbin and verify progress callbacks fire
        // with strictly increasing byte counts.
        let call_count = Arc::new(AtomicU64::new(0));
        let last_downloaded = Arc::new(AtomicU64::new(0));

        let cc = Arc::clone(&call_count);
        let ld = Arc::clone(&last_downloaded);

        let result = download_with_progress(
            "https://httpbin.org/bytes/1024",
            30,
            Some(&move |downloaded, _total| {
                cc.fetch_add(1, Ordering::SeqCst);
                // Each callback should report >= previous downloaded amount.
                let prev = ld.swap(downloaded, Ordering::SeqCst);
                assert!(
                    downloaded >= prev,
                    "Downloaded bytes should be non-decreasing: {} < {}",
                    downloaded,
                    prev
                );
            }),
        );

        assert!(result.is_ok(), "Download should succeed: {:?}", result.err());
        let data = result.unwrap();
        assert_eq!(data.len(), 1024, "Should have downloaded exactly 1024 bytes");

        // At least one progress callback should have fired.
        assert!(
            call_count.load(Ordering::SeqCst) >= 1,
            "Progress callback should have been called at least once"
        );
    }

    #[test]
    #[ignore] // Requires network access to httpbin.org — run with `cargo test -- --ignored`
    fn test_download_with_progress_no_callback() {
        // Download should work fine when on_progress is None.
        let result = download_with_progress("https://httpbin.org/bytes/512", 30, None);
        assert!(result.is_ok(), "Download should succeed: {:?}", result.err());
        let data = result.unwrap();
        assert_eq!(data.len(), 512, "Should have downloaded exactly 512 bytes");
    }

    #[test]
    #[ignore] // Requires network access (DNS resolution) — run with `cargo test -- --ignored`
    fn test_download_with_progress_invalid_url() {
        let result = download_with_progress(
            "https://this-domain-does-not-exist-12345.example.com/file",
            5,
            None,
        );
        assert!(result.is_err(), "Should fail for invalid URL");
        let err = result.unwrap_err();
        assert!(
            err.contains("HTTP request failed"),
            "Error should mention HTTP failure, got: {}",
            err
        );
    }

    // -------------------------------------------------------------------------
    // check_disk_space tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_check_disk_space_sufficient() {
        // Requesting 1 byte should always succeed on a healthy system.
        let result = check_disk_space(Path::new("/tmp"), 1);
        assert!(
            result.is_ok(),
            "1 byte should be available: {:?}",
            result.err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_check_disk_space_insufficient() {
        // Request an absurdly large amount — should fail.
        // Unix-only: the non-Unix `check_disk_space` is an intentional no-op
        // stub (returns Ok), so the "insufficient" path doesn't exist there.
        let result = check_disk_space(Path::new("/tmp"), u64::MAX);
        assert!(result.is_err(), "u64::MAX bytes should not be available");
        let err = result.unwrap_err();
        assert!(
            err.contains("Insufficient disk space"),
            "Error should mention insufficient space, got: {}",
            err
        );
    }

    #[test]
    fn test_check_disk_space_nonexistent_path() {
        // Should walk up to find an existing ancestor.
        let result = check_disk_space(Path::new("/tmp/nonexistent_dir_abc123/child"), 1);
        assert!(
            result.is_ok(),
            "Should resolve to /tmp and succeed: {:?}",
            result.err()
        );
    }
}
