//! Fingerprint of the running moss binary, for build-cache invalidation.
//!
//! Had a second fingerprint — a SHA-256 over every installed enhance plugin's
//! files, which forced a rebuild and browser refresh when plugin code changed.
//! Retiring the `enhance` capability meant that digest covered an empty
//! set on every build; it and its `SiteHashes.plugin_fingerprint` field went
//! with it. Nothing a plugin does now reaches page bytes at render time: a
//! process hook writes SOURCE files, and the content hashes already see those.

use sha2::{Digest, Sha256};
use std::fs;
use std::sync::OnceLock;

/// Fingerprint of the running moss binary based on file metadata (size + mtime).
///
/// Computed once on first call (via `OnceLock`). When this value differs from
/// the fingerprint stored in a previous build's `hashes.json`, the build
/// pipeline treats all HTML/CSS/JS output as stale and forces regeneration +
/// browser refresh.
///
/// This catches any change to embedded assets (`include_str!` CSS, HTML
/// templates), rendering logic, or class/attribute names — changes that
/// modify output but don't touch user source files.
///
/// Uses mtime+size (like Make/Cargo) instead of reading the full binary,
/// which avoids a multi-second read+hash for large debug binaries (100MB+).
/// Video and media asset caching is unaffected (keyed on source content hash).
pub fn builder_fingerprint() -> String {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT
        .get_or_init(|| {
            let exe = match std::env::current_exe() {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("Failed to locate current binary: {}", e);
                    // Return a unique value so the build treats output as stale
                    // (safe fallback: forces rebuild rather than serving stale content)
                    return format!("{:x}", Sha256::digest(b"unknown-binary"));
                }
            };
            match fs::metadata(&exe) {
                Ok(meta) => {
                    let size = meta.len();
                    let mtime = meta
                        .modified()
                        .map(|t| {
                            t.duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos()
                        })
                        .unwrap_or(0);
                    format!("{}-{}", size, mtime)
                }
                Err(e) => {
                    log::warn!("Failed to stat binary at {:?}: {}", exe, e);
                    format!("{:x}", Sha256::digest(b"unreadable-binary"))
                }
            }
        })
        .clone()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_builder_fingerprint_is_stable() {
        let fp1 = super::builder_fingerprint();
        let fp2 = super::builder_fingerprint();

        assert!(!fp1.is_empty(), "builder fingerprint should not be empty");
        // Format is "size-mtime_nanos", e.g. "15728640-1710200000000000000"
        assert!(
            fp1.contains('-'),
            "fingerprint should be size-mtime format, got: {}",
            fp1
        );
        assert_eq!(fp1, fp2, "builder fingerprint should be stable across calls");
    }
}
