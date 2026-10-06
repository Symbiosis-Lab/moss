//! Plugin/theme installation from the registry (M2) and the build-time
//! plugin downloader (`build.rs`).
//!
//! Target-architecture home for install/download code. The registry client lives
//! here as `registry_client`, in three layers split by what each is blind to.
//! `discovery`/`bundled`/`registry` migrate under this module later.

pub mod registry_client;
pub mod zip_extract;

use semver::Version;

/// An id from any origin — an index served at the pinned URL, or a row the
/// frontend asked to install or uninstall — reaches a `Path::join` and, on
/// those two paths, an `fs::remove_dir_all`. So `../../..` would escape
/// `.moss/plugins` and delete an attacker-chosen directory.
/// `zip_extract` already hardens the names *inside* an archive; this is the
/// same boundary at the same strength for the name of the directory it
/// unpacks into. Refusing here rather than at install means everything reading
/// the catalog — the rows, the update check, the install lookup — gets it at
/// once. (`RevokedList` does not route through here; a revocation id is only
/// ever compared, never joined to a path.)
///
/// The charset is what the registry's own submission rules already require, so
/// a rejection means a malformed or hostile document, never a real plugin.
pub(crate) fn is_usable_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// What to do with a plugin already installed when another copy of it turns up.
///
/// One owner for both origins. moss ships plugins inside the binary and the
/// registry publishes them over the network, and the arithmetic — is this one
/// newer, is the installed one newer, are they the same version — does not
/// depend on which. It lived in `bundled.rs` until the registry needed the
/// same answer; a second copy would have been a second set of tie-break rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VersionDecision {
    /// The candidate is newer and may replace what is here.
    Update,
    /// Leave the installed copy alone.
    Skip,
    /// Same version — whether to replace is a question about content, not
    /// versions, and the caller answers it by comparing bytes.
    CompareHash,
}

/// Compare an installed version against a candidate.
///
/// Semver when both parse: newer candidate is [`VersionDecision::Update`] when
/// `may_replace`, a newer installed copy is always [`VersionDecision::Skip`]
/// (someone put it there deliberately), equal is
/// [`VersionDecision::CompareHash`].
///
/// When either version is not semver, the two are only ever compared for
/// equality — something that cannot say which of two versions is newer has no
/// business claiming one is — so differing means `Skip`.
pub(crate) fn version_update_decision(
    installed_version: &str,
    candidate_version: &str,
    may_replace: bool,
) -> VersionDecision {
    match (
        Version::parse(installed_version),
        Version::parse(candidate_version),
    ) {
        (Ok(installed), Ok(candidate)) => {
            if candidate > installed {
                if may_replace {
                    VersionDecision::Update
                } else {
                    VersionDecision::Skip
                }
            } else if installed > candidate {
                VersionDecision::Skip
            } else {
                VersionDecision::CompareHash
            }
        }
        _ if installed_version != candidate_version => VersionDecision::Skip,
        _ => VersionDecision::CompareHash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The charset, at the owner. What the rule is *for* — dropping a hostile
    /// row rather than failing the document — is asserted where the catalog is
    /// parsed; this is the rule itself, which both commands also depend on.
    #[test]
    fn an_id_that_could_not_be_a_directory_name_is_not_an_id() {
        for bad in ["", "Upper", "with space", "dot.dot", "sub/dir", "trailing\u{5c}"] {
            assert!(!is_usable_id(bad), "{bad:?} must not be usable as a directory name");
        }
        for good in ["github", "matters", "x-2", "0"] {
            assert!(is_usable_id(good), "{good:?} is what real ids look like");
        }
    }

    /// One table, because every case is the same question asked of a different
    /// pair. `may_replace` is false in exactly one row: it does not change what
    /// is newer, only whether being newer is allowed to act.
    #[test]
    fn a_version_pair_decides_what_happens_to_the_installed_copy() {
        let cases = [
            ("0.1.0", "0.2.0", true, VersionDecision::Update, "a newer candidate replaces"),
            ("0.1.0", "0.2.0", false, VersionDecision::Skip, "newer, but replacing is not allowed"),
            ("1.0.0", "1.0.1", true, VersionDecision::Update, "a patch bump is newer"),
            ("999.0.0", "1.0.0", true, VersionDecision::Skip, "someone installed a newer one on purpose"),
            ("1.0.0", "1.0.0", true, VersionDecision::CompareHash, "same version — ask the bytes"),
            ("abc", "abc", true, VersionDecision::CompareHash, "same unparseable version — ask the bytes"),
            ("abc", "def", true, VersionDecision::Skip, "neither parses, so neither is newer"),
            ("1.0.0", "not-semver", true, VersionDecision::Skip, "one side unparseable is not comparable"),
            ("not-semver", "2.0.0", true, VersionDecision::Skip, "and it is not comparable the other way either"),
        ];
        for (installed, candidate, may_replace, want, why) in cases {
            assert_eq!(
                version_update_decision(installed, candidate, may_replace),
                want,
                "{installed} -> {candidate} (may_replace={may_replace}): {why}"
            );
        }
    }
}
