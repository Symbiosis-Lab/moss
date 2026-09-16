//! What moss knows about the bytes it installed — for a plugin folder or a
//! staged stack. One type answers "which bytes are these, and are they the
//! ones we meant" for both; the file name stays a per-artifact constant,
//! because renaming a file on disk is the one change that would strand an
//! install.
//!
//! A plugin receipt is written into the unpacked directory before it is
//! swapped into place, so the receipt and the code it describes become the
//! installed plugin in one rename. Its presence therefore means the install
//! finished; its absence means a bundled plugin, or one installed before
//! receipts existed. What the ordering rules out is narrower than either: no
//! *registry-installed* version is ever live without one — a version live
//! without a receipt would be read as unreceipted by the next update, which
//! keeps everything, and that update would then record a list not naming
//! what it kept.
//!
//! Not a trust signal. It travels with the folder, so a shared folder ships
//! it along with the code; whether these bytes are moss's own is answered by
//! comparing `bundled::code_hash` to `bundled::bundled_code_hash`, and whether
//! the user allowed them is recorded in app data ([`super::approval`]).
//!
//! A stack receipt is written only after bring-up is proven, beside the
//! staged bundle — moss#1120's fix, so a re-pinned artifact reaches a
//! machine that already has one staged. `receipt_on` records which of the
//! two this was, truthfully, though nothing reads it back yet.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::index::IndexEntry;

/// The file name for a plugin receipt, dotted so it sorts and reads as
/// moss's rather than the plugin's. A hostile archive can contain this name;
/// it is written after extraction, so what the archive claimed is
/// overwritten by what happened.
pub const RECEIPT: &str = ".moss-install.json";

/// The file name for a stack receipt, a SIBLING of the staged `.app`.
/// Dot-prefixed with a non-`.app` name so it is never mistaken for an
/// install by `is_installed` — same convention as the executor's
/// `layout::incoming_path`/`outgoing_path`.
pub const STACK_RECEIPT: &str = ".installed-manifest.json";

/// Where a stack install's bytes came from. A `Local` install (the dev
/// override) is never judged against the pinned manifest — the override was
/// a deliberate act, and auto-replacing a build somebody is testing would
/// attribute its behavior to the wrong binary. The verdict is a property of
/// the DISK, not of whichever process happens to be reading it with which
/// environment. Absent entirely from a plugin receipt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    #[default]
    Pinned,
    Local,
}

/// *When* a receipt was written relative to the install being proven — the
/// one thing that genuinely differs between a plugin install (extract time)
/// and a stack install (only after bring-up answers). No production code
/// reads this yet (S2 of the plugin-owned-stack-lifecycle design); it is
/// written truthfully now so a receipt written between S2 and the executor
/// that does read it can never be told apart from one that did not care.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReceiptOn {
    #[default]
    Extract,
    Bringup,
}

/// What a successful install records about itself.
///
/// `id` / `download_url` / `files` are the plugin receipt's fields, unused by
/// a stack receipt; `origin` is absent from a plugin receipt today. Each is
/// `skip_serializing_if`, so a plugin receipt gains no `source` key an older
/// moss has never seen. `receipt_on` is skipped only at its default
/// (`Extract`, the plugin path's truthful value) — a stack write always
/// records the non-default `Bringup`, so it always appears there, and a
/// stack receipt still writes exactly `{version, sha256, source,
/// receipt_on}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Receipt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub version: String,
    /// The hash the registry pinned at review time — the identity of the
    /// exact bytes that were unpacked here.
    pub sha256: String,
    /// Where those bytes came from, kept because "which origin served this"
    /// is a question a user reporting a problem cannot otherwise answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    /// Every path this archive laid down, relative to the plugin directory.
    ///
    /// It is what makes an update able to tell the plugin's own files from
    /// the last version's code. Anything in the directory that is not in
    /// this list was written by the plugin and must survive; anything in it
    /// that the new archive does not bring was deliberately deleted upstream
    /// and must not be carried forward. Without it an update has to guess,
    /// and both guesses are wrong — an allowlist deletes the user's
    /// credentials, and "keep everything unmatched" restores code a new
    /// version removed.
    ///
    /// `None` — a receipt from before this field existed — is "this install
    /// did not record", which is not the same as an empty list meaning "it
    /// laid down nothing", and the two must not collapse: the first has to
    /// fall back to keeping everything, the second would delete it all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    /// Wire name stays `source`: renaming it would read every existing
    /// `Local` dev stamp as `Pinned` and replace a build somebody is testing.
    /// `None` — absent, as in every plugin receipt today — is not the same
    /// claim as `Some(Origin::Pinned)`; both are judged the same way by
    /// [`stale_against_pin`], but only a stack write ever sets it.
    #[serde(default, rename = "source", skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    #[serde(default, skip_serializing_if = "is_extract")]
    pub receipt_on: ReceiptOn,
}

fn is_extract(on: &ReceiptOn) -> bool {
    *on == ReceiptOn::Extract
}

impl Receipt {
    /// A stack receipt, written only after bring-up is proven.
    pub fn at_bringup(version: String, sha256: String, origin: Origin) -> Self {
        Self {
            id: None,
            version,
            sha256,
            download_url: None,
            files: None,
            origin: Some(origin),
            receipt_on: ReceiptOn::Bringup,
        }
    }

    /// A plugin receipt, written into staging before the swap.
    pub fn at_extract(
        id: String,
        version: String,
        sha256: String,
        download_url: String,
        files: Vec<String>,
    ) -> Self {
        Self {
            id: Some(id),
            version,
            sha256,
            download_url: Some(download_url),
            files: Some(files),
            origin: None,
            receipt_on: ReceiptOn::Extract,
        }
    }
}

/// A placeholder sha256 (pre-release) means the real download can't be
/// verified yet — callers must use the local-DMG dev path instead.
pub fn is_placeholder_sha256(sha: &str) -> bool {
    let s = sha.trim();
    s.is_empty() || s.starts_with("<PLACEHOLDER") || !s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Is the staged copy behind the pin this build embeds?
///
/// Takes the pin as two strings rather than a `StackContribution` on purpose:
/// this crate's own [`crate::plugins::contributions::stack`] type is the one
/// declaration shape, and this function stays agnostic of it.
///
/// **No receipt counts as stale.** Every app staged before stamping existed
/// has no receipt, and those are exactly the installs the pin never reached;
/// treating "unknown" as current would preserve that forever (moss#1120). The
/// exceptions are a placeholder pin (nothing installable to be behind) and an
/// [`Origin::Local`] install (the dev override owns that machine's copy).
pub fn stale_against_pin(receipt: Option<&Receipt>, version: &str, sha256: &str) -> bool {
    if is_placeholder_sha256(sha256) {
        return false;
    }
    match receipt {
        None => true,
        Some(r) if r.origin == Some(Origin::Local) => false,
        Some(r) => r.version != version || r.sha256 != sha256,
    }
}

/// Write `receipt` as `file_name` inside `dir`.
pub fn write_at(dir: &Path, file_name: &str, receipt: &Receipt) -> Result<(), String> {
    let body = serde_json::to_string_pretty(receipt)
        .map_err(|e| format!("serialize the install receipt: {e}"))?;
    // allow:raw_write the install receipt lives under .moss/plugins/<name>/ or
    // beside a staged stack, not build output
    fs::write(dir.join(file_name), body).map_err(|e| format!("write the install receipt: {e}"))
}

/// Read `file_name` back from `dir`. Missing, torn or garbled all fold to
/// `None` — the ordinary state of a bundled plugin or a pre-stamp stack, not
/// an error. A present-but-unparseable file still says so in the log; an
/// absent file stays silent.
pub fn read_at(dir: &Path, file_name: &str) -> Option<Receipt> {
    let path = dir.join(file_name);
    let bytes = fs::read(&path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(receipt) => Some(receipt),
        Err(e) => {
            log::warn!(
                "[install] unreadable {} ({e}) — treating the install as needing replacement",
                path.display()
            );
            None
        }
    }
}

pub fn write(plugin_dir: &Path, entry: &IndexEntry, files: Vec<String>) -> Result<(), String> {
    let receipt = Receipt::at_extract(
        entry.id.clone(),
        entry.version.clone(),
        entry.sha256.to_lowercase(),
        entry.download_url.clone(),
        files,
    );
    write_at(plugin_dir, RECEIPT, &receipt)
}

/// The receipt in `plugin_dir`, or `None` — which is the ordinary state of a
/// bundled plugin, not an error.
pub fn read(plugin_dir: &Path) -> Option<Receipt> {
    read_at(plugin_dir, RECEIPT)
}

#[cfg(test)]
#[path = "receipt_tests.rs"]
mod tests;
