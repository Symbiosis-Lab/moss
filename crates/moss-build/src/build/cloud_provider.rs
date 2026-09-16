//! Cloud storage provider detection by path.
//!
//! The `SF_DATALESS` flag (see `icloud.rs`) is set by any macOS File Provider
//! extension that evicts a file's bytes — not just iCloud Drive. Dropbox,
//! Google Drive, OneDrive, and Box all use the File Provider API on modern
//! macOS and produce identical stall behavior when moss reads an evicted
//! file.
//!
//! This module maps a project path to the cloud provider that most likely
//! manages it, so the progress indicator can show "Downloading from Dropbox"
//! instead of the misleading "Downloading from iCloud" label we used to
//! always emit.
//!
//! Detection is path-based, not authoritative: we don't query File Provider
//! APIs. That keeps this module pure Rust with no entitlement requirements,
//! and the cost of a wrong guess is a slightly less accurate label, not a
//! broken build.
//!
//! ## Scope and limitations
//!
//! - **macOS-shaped.** Recognized prefixes are the macOS conventions. Linux
//!   gets the legacy `~/Dropbox` shortcut; Windows callers receive `None`
//!   and the UI falls back to a generic "cloud" label.
//! - **No symlink resolution.** If the user passes a symlinked path that
//!   resolves into a cloud-sync root, detection misses. Callers that care
//!   about this should `std::fs::canonicalize()` before calling. Today moss
//!   passes the folder the user opened, which is already the literal sync
//!   root in practice.

use std::path::Path;

/// Known cloud storage providers that use the macOS File Provider framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudProvider {
    ICloud,
    Dropbox,
    GoogleDrive,
    OneDrive,
    Box,
    /// Some other File Provider extension (Sync.com, pCloud, etc.) or a
    /// provider we couldn't identify from the path.
    Unknown,
}

impl CloudProvider {
    /// Human-readable provider name for use in UI labels.
    ///
    /// Returns `None` for `Unknown` — callers should fall back to a generic
    /// "cloud" label rather than guessing.
    pub fn display_name(self) -> Option<&'static str> {
        match self {
            CloudProvider::ICloud => Some("iCloud"),
            CloudProvider::Dropbox => Some("Dropbox"),
            CloudProvider::GoogleDrive => Some("Google Drive"),
            CloudProvider::OneDrive => Some("OneDrive"),
            CloudProvider::Box => Some("Box"),
            CloudProvider::Unknown => None,
        }
    }

    /// Stable machine-readable identifier, useful for analytics or event
    /// payloads that cross the Rust/TS boundary.
    pub fn id(self) -> &'static str {
        match self {
            CloudProvider::ICloud => "icloud",
            CloudProvider::Dropbox => "dropbox",
            CloudProvider::GoogleDrive => "google_drive",
            CloudProvider::OneDrive => "onedrive",
            CloudProvider::Box => "box",
            CloudProvider::Unknown => "unknown",
        }
    }
}

/// Detect the cloud provider that manages `path`, if any, based on the path
/// prefix.
///
/// Returns `None` when the path is outside any known cloud-sync root — i.e.
/// the project is local-only and no "downloading" label is needed.
///
/// Recognized roots (macOS):
/// - `~/Library/Mobile Documents/com~apple~CloudDocs/…` → iCloud (Files.app / iCloud Drive)
/// - `~/Library/Mobile Documents/<bundle-id>/…` → iCloud (any app's iCloud container,
///   e.g. `iCloud~md~obsidian` for Obsidian, `iCloud~com~apple~Pages` for Pages)
/// - `~/Library/CloudStorage/iCloud Drive…` → iCloud
/// - `~/Library/CloudStorage/Dropbox…` → Dropbox
/// - `~/Dropbox…` (legacy Dropbox before CloudStorage migration) → Dropbox
/// - `~/Library/CloudStorage/GoogleDrive-…` → Google Drive
/// - `~/Library/CloudStorage/OneDrive-…` → OneDrive
/// - `~/Library/CloudStorage/Box-Box…` → Box
///
/// Anything else under `~/Library/CloudStorage/` is reported as `Unknown`
/// (some File Provider extension we don't have a name for) rather than `None`,
/// because the stall behavior is identical and the UI should still show a
/// generic "cloud" label.
pub fn detect_from_path(path: &Path) -> Option<CloudProvider> {
    let path_str = path.to_string_lossy();

    // iCloud: any Mobile Documents container. `com~apple~CloudDocs` is the
    // Files.app / "iCloud Drive" root, but every iCloud-syncing app gets its
    // own bundle-IDed sibling (e.g. `iCloud~md~obsidian`, `iCloud~com~apple~Pages`).
    // All of them use the same File Provider extension and exhibit the same
    // SF_DATALESS eviction behavior, so we treat them uniformly.
    if let Some(rest) = path_str.split("/Library/Mobile Documents/").nth(1) {
        let root = rest.split('/').next().unwrap_or("");
        if !root.is_empty() {
            return Some(CloudProvider::ICloud);
        }
    }

    // Modern CloudStorage container — covers iCloud Drive and every
    // third-party File Provider extension.
    if let Some(rest) = path_str.split("/Library/CloudStorage/").nth(1) {
        // `rest` starts with the provider root directory name, e.g.
        //   "Dropbox/Projects/site" or "GoogleDrive-user@gmail.com/My Drive/…"
        let root = rest.split('/').next().unwrap_or("");
        // A bare `/Library/CloudStorage/` with nothing after it is not a
        // real project path — treat as "not in any cloud root" rather than
        // pretending we saw an unknown provider.
        if root.is_empty() {
            return None;
        }
        return Some(classify_cloudstorage_root(root));
    }

    // Legacy Dropbox location (pre-File Provider). Still common on machines
    // that opted out of the CloudStorage migration. Match `/Dropbox/` or a
    // trailing `/Dropbox` but not arbitrary directories that happen to
    // contain "Dropbox" as a substring.
    if has_home_relative_segment(&path_str, "Dropbox") {
        return Some(CloudProvider::Dropbox);
    }

    None
}

/// Classify a directory name that appears directly under `~/Library/CloudStorage/`.
///
/// Apple's convention is `<Provider>-<Account>` or `<Provider>Drive-<Account>`,
/// but some providers (Dropbox) just use the bare provider name.
fn classify_cloudstorage_root(root: &str) -> CloudProvider {
    // Match by prefix so that account suffixes like "-user@example.com" are
    // tolerated. Order matters: `OneDrive` must be checked before a generic
    // "Drive" match would win.
    if root == "Dropbox" || root.starts_with("Dropbox-") {
        CloudProvider::Dropbox
    } else if root.starts_with("GoogleDrive") {
        CloudProvider::GoogleDrive
    } else if root.starts_with("OneDrive") {
        CloudProvider::OneDrive
    } else if root.starts_with("Box-Box") || root == "Box" || root.starts_with("Box-") {
        CloudProvider::Box
    } else if root.starts_with("iCloud Drive") || root.starts_with("iCloudDrive") {
        CloudProvider::ICloud
    } else {
        CloudProvider::Unknown
    }
}

/// Returns true if `path_str` contains `/<segment>/` or ends with `/<segment>`
/// anywhere after the user's home directory marker. Used to match legacy
/// `~/Dropbox` paths without false-positive on e.g. `~/Projects/Dropbox-clone`.
fn has_home_relative_segment(path_str: &str, segment: &str) -> bool {
    // Look for `/<Users>/<name>/<segment>` on macOS and `/home/<name>/<segment>`
    // on Linux. This is intentionally narrow — we only match when the segment
    // sits directly under a home directory.
    let needles = [
        format!("/Users/"), // allow:served-path-url-construct (filesystem path prefix, not a URL)
        format!("/home/"),  // allow:served-path-url-construct (filesystem path prefix, not a URL)
    ];
    for needle in &needles {
        if let Some((_, after_home)) = path_str.split_once(needle.as_str()) {
            // after_home looks like "<username>/<segment>/..." or "<username>"
            if let Some((_user, after_user)) = after_home.split_once('/') {
                let target = segment;
                if after_user == target
                    || after_user.starts_with(&format!("{}/", target))
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn detects_icloud_mobile_documents() {
        let path = p("/Users/alice/Library/Mobile Documents/com~apple~CloudDocs/Sites/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::ICloud));
    }

    #[test]
    fn detects_icloud_obsidian_container() {
        // Obsidian stores its iCloud-synced vault under a bundle-IDed sibling
        // of `com~apple~CloudDocs`. Same File Provider, same eviction behavior.
        let path = p("/Users/alice/Library/Mobile Documents/iCloud~md~obsidian/Documents/Vault/Site");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::ICloud));
    }

    #[test]
    fn detects_icloud_pages_container() {
        // Apple's first-party apps use the `iCloud~com~apple~<App>` form.
        let path = p("/Users/alice/Library/Mobile Documents/iCloud~com~apple~Pages/Documents/draft.pages");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::ICloud));
    }

    #[test]
    fn empty_mobile_documents_root_returns_none() {
        // The container directory itself is not a project path.
        let path = p("/Users/alice/Library/Mobile Documents/");
        assert_eq!(detect_from_path(&path), None);
    }

    #[test]
    fn detects_icloud_cloudstorage() {
        let path = p("/Users/alice/Library/CloudStorage/iCloud Drive/Sites/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::ICloud));
    }

    #[test]
    fn detects_dropbox_cloudstorage() {
        let path = p("/Users/alice/Library/CloudStorage/Dropbox/Sites/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::Dropbox));
    }

    #[test]
    fn detects_dropbox_cloudstorage_with_account() {
        let path = p("/Users/alice/Library/CloudStorage/Dropbox-Personal/Sites/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::Dropbox));
    }

    #[test]
    fn detects_legacy_dropbox() {
        let path = p("/Users/alice/Dropbox/Sites/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::Dropbox));
    }

    #[test]
    fn detects_legacy_dropbox_linux() {
        let path = p("/home/alice/Dropbox/Sites/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::Dropbox));
    }

    #[test]
    fn does_not_confuse_dropbox_lookalike_folder() {
        let path = p("/Users/alice/Projects/Dropbox-clone/site");
        assert_eq!(detect_from_path(&path), None);
    }

    #[test]
    fn detects_google_drive() {
        let path = p("/Users/alice/Library/CloudStorage/GoogleDrive-alice@gmail.com/My Drive/Sites/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::GoogleDrive));
    }

    #[test]
    fn detects_onedrive() {
        let path = p("/Users/alice/Library/CloudStorage/OneDrive-Personal/Sites/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::OneDrive));
    }

    #[test]
    fn detects_box() {
        let path = p("/Users/alice/Library/CloudStorage/Box-Box/Sites/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::Box));
    }

    #[test]
    fn unknown_cloudstorage_provider_still_reports_cloud() {
        // Some future provider we haven't mapped — still reported so the UI
        // can show a generic cloud label instead of pretending it's local.
        let path = p("/Users/alice/Library/CloudStorage/Syncthing-Default/Sites/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::Unknown));
    }

    #[test]
    fn local_project_returns_none() {
        let path = p("/Users/alice/Sites/blog");
        assert_eq!(detect_from_path(&path), None);
    }

    #[test]
    fn tmp_path_returns_none() {
        let path = p("/tmp/test-site");
        assert_eq!(detect_from_path(&path), None);
    }

    #[test]
    fn bare_home_dropbox_still_detected() {
        // Edge: project path IS the Dropbox root, no children under it.
        let path = p("/Users/alice/Dropbox");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::Dropbox));
    }

    #[test]
    fn bare_cloudstorage_dropbox_still_detected() {
        // Edge: project path IS the Dropbox root under CloudStorage.
        let path = p("/Users/alice/Library/CloudStorage/Dropbox");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::Dropbox));
    }

    #[test]
    fn empty_cloudstorage_root_returns_none() {
        // Someone passed the CloudStorage directory itself — not a real
        // project path; should not fabricate a provider.
        let path = p("/Users/alice/Library/CloudStorage/");
        assert_eq!(detect_from_path(&path), None);
    }

    #[test]
    fn path_with_space_in_username() {
        let path = p("/Users/al ice/Library/CloudStorage/Dropbox/blog");
        assert_eq!(detect_from_path(&path), Some(CloudProvider::Dropbox));
    }

    #[test]
    fn display_name_and_id_are_stable() {
        assert_eq!(CloudProvider::ICloud.display_name(), Some("iCloud"));
        assert_eq!(CloudProvider::ICloud.id(), "icloud");
        assert_eq!(CloudProvider::Dropbox.display_name(), Some("Dropbox"));
        assert_eq!(CloudProvider::Dropbox.id(), "dropbox");
        assert_eq!(CloudProvider::GoogleDrive.display_name(), Some("Google Drive"));
        assert_eq!(CloudProvider::GoogleDrive.id(), "google_drive");
        assert_eq!(CloudProvider::OneDrive.display_name(), Some("OneDrive"));
        assert_eq!(CloudProvider::OneDrive.id(), "onedrive");
        assert_eq!(CloudProvider::Box.display_name(), Some("Box"));
        assert_eq!(CloudProvider::Box.id(), "box");
        assert_eq!(CloudProvider::Unknown.display_name(), None);
        assert_eq!(CloudProvider::Unknown.id(), "unknown");
    }
}
