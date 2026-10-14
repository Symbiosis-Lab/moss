//! Derivation results through the content-addressed cache.
//!
//! Keyed by the input file's content hash, the transform name
//! `derive:<plugin>/<derivation>`, the plugin's manifest version and a
//! fingerprint of its pin (the declared binary sources and fragments, plus a
//! local binary's content), so editing a file, shipping a new plugin version,
//! re-pinning without a version bump or rebuilding a local binary re-derives
//! and nothing else does. Both outcomes are stored — a skip is as expensive to
//! rediscover as a result. A failure is stored only as a note for `--status`:
//! it is usually the environment's doing, so it never stops a retry.

use std::path::Path;

use super::{binary, run, DeriveFailure, DeriveOutcome};
use crate::build::cache::{RecordMode, TransformCache, TransformEntry};
use crate::moss_paths::MossPaths;
use crate::system::derive::registry::DerivationEntry;

pub type DeriveResult = Result<DeriveOutcome, DeriveFailure>;

fn transform_name(entry: &DerivationEntry) -> String {
    format!("derive:{}/{}", entry.plugin_id, entry.derivation_id)
}

fn params(entry: &DerivationEntry) -> serde_json::Value {
    serde_json::json!({ "version": entry.plugin_version, "pin": binary::pin_fingerprint(entry) })
}

fn failure_transform_name(entry: &DerivationEntry) -> String {
    format!("derive-failed:{}/{}", entry.plugin_id, entry.derivation_id)
}

fn lookup_bytes(site: &MossPaths, transform: &str, entry: &DerivationEntry, source_oid: &str) -> Option<Vec<u8>> {
    let transforms = TransformCache::for_site(site);
    let hit = transforms.find_cached_entry(source_oid, transform, &params(entry), RecordMode::Request)?;
    let path = transforms.objects().get_path(&hit.oid)?;
    std::fs::read(path).ok()
}

fn lookup_oid(site: &MossPaths, entry: &DerivationEntry, source_oid: &str) -> Option<DeriveOutcome> {
    // Unreadable or from another shape: a miss, and the next run overwrites it.
    serde_json::from_slice(&lookup_bytes(site, &transform_name(entry), entry, source_oid)?).ok()
}

/// The stored outcome for this file, if there is one. Never runs the binary,
/// so the render path can call it freely.
pub fn lookup_cached(site: &MossPaths, entry: &DerivationEntry, input: &Path) -> Option<DeriveOutcome> {
    let oid = binary::content_hash(input).ok()?;
    lookup_oid(site, entry, &oid)
}

/// Why the last run on this exact file content failed, for a status report.
/// Written for people to read, never consulted to skip a run: a failure is
/// usually the environment's, and the next attempt may succeed.
pub fn lookup_failure(site: &MossPaths, entry: &DerivationEntry, input: &Path) -> Option<String> {
    let oid = binary::content_hash(input).ok()?;
    String::from_utf8(lookup_bytes(site, &failure_transform_name(entry), entry, &oid)?).ok()
}

fn store_bytes(
    site: &MossPaths,
    transform: String,
    entry: &DerivationEntry,
    source_oid: &str,
    source_size: u64,
    bytes: &[u8],
) -> Result<(), String> {
    let transforms = TransformCache::for_site(site);
    let oid = transforms.objects().store_bytes(bytes, RecordMode::Request)?;
    let record = TransformEntry { oid, size: bytes.len() as u64, params: params(entry) };
    transforms
        .merge(source_oid, source_size, RecordMode::Request, |rec| {
            rec.transforms.insert(transform, record);
        })
        .map(|_| ())
}

/// Results the cache could not hand back, by the same key the cache uses
/// (content hash, transform, params). Without it a result that cannot be
/// stored or read back (disk full, an evicted object that is offline) misses
/// on every rebuild, so each one starts the same read again. Only
/// [`derive_remembering`] writes it — the background reads of a preview — and
/// only a preview's placeholder decision reads it ([`remembered`]): a build
/// that waits for text always runs the deriver itself.
type Memo = std::collections::HashMap<String, Result<DeriveOutcome, DeriveFailure>>;

fn memo() -> &'static std::sync::Mutex<Memo> {
    static MEMO: std::sync::OnceLock<std::sync::Mutex<Memo>> = std::sync::OnceLock::new();
    MEMO.get_or_init(Default::default)
}

fn memo_key(entry: &DerivationEntry, source_oid: &str) -> String {
    format!("{source_oid}\u{0}{}\u{0}{}", transform_name(entry), params(entry))
}

fn remember(entry: &DerivationEntry, source_oid: &str, result: Result<DeriveOutcome, DeriveFailure>) {
    if let Ok(mut memo) = memo().lock() {
        memo.insert(memo_key(entry, source_oid), result);
    }
}

/// What a background read of exactly this file content ended in, when the
/// cache cannot say.
pub fn remembered(entry: &DerivationEntry, input: &Path) -> Option<Result<DeriveOutcome, DeriveFailure>> {
    let oid = binary::content_hash(input).ok()?;
    memo().lock().ok()?.get(&memo_key(entry, &oid)).cloned()
}

/// Remember that `entry` could not even be started for `input` (its binary
/// would not install), so a preview does not ask again for the same bytes.
pub fn remember_failure(entry: &DerivationEntry, input: &Path, failure: &DeriveFailure) {
    if let Ok(oid) = binary::content_hash(input) {
        remember(entry, &oid, Err(failure.clone()));
    }
}

/// The outcome for `input`: from the cache when this exact content was derived
/// by this plugin version, otherwise by running the pinned binary.
pub fn derive_cached(site: &MossPaths, entry: &DerivationEntry, input: &Path) -> DeriveResult {
    derive_with(site, entry, input, false)
}

/// [`derive_cached`] that also records, for [`remembered`], a result the cache
/// could not keep.
pub fn derive_remembering(site: &MossPaths, entry: &DerivationEntry, input: &Path) -> DeriveResult {
    derive_with(site, entry, input, true)
}

fn run_pinned(entry: &DerivationEntry, input: &Path) -> Result<run::Answer, DeriveFailure> {
    let binary = binary::resolve_binary(entry)?;
    let answer = run::run_deriver_answer(&binary, input, run::derive_timeout())?;
    answer.outcome.check_against(&entry.contribution.fragments)?;
    Ok(answer)
}

fn derive_with(site: &MossPaths, entry: &DerivationEntry, input: &Path, remembering: bool) -> DeriveResult {
    let source_oid = binary::content_hash(input).map_err(DeriveFailure::new)?;
    if let Some(outcome) = lookup_oid(site, entry, &source_oid) {
        return Ok(outcome);
    }
    let size = std::fs::metadata(input).map(|m| m.len()).unwrap_or(0);
    match run_pinned(entry, input) {
        Ok(run::Answer { raw, outcome }) => {
            // The deriver's own text is stored, not the parsed struct: the
            // entry is read with whatever this moss knows when it is read.
            // Stored and read back: a write that lands where the next lookup
            // cannot see it (an object parked offline) is as good as none.
            let stored = store_bytes(site, transform_name(entry), entry, &source_oid, size, raw.as_bytes())
                .and_then(|()| lookup_oid(site, entry, &source_oid).map(|_| ()).ok_or_else(|| "it could not be read back".to_string()));
            if let Err(e) = stored {
                log::warn!(target: "derive", "could not cache the result for {}: {e}", transform_name(entry));
                if remembering {
                    remember(entry, &source_oid, Ok(outcome.clone()));
                }
            }
            Ok(outcome)
        }
        Err(failure) => {
            // Best effort, and only for `--status`: a failed write changes nothing else.
            let _ = store_bytes(site, failure_transform_name(entry), entry, &source_oid, size, failure.message.as_bytes());
            if remembering {
                remember(entry, &source_oid, Err(failure.clone()));
            }
            Err(failure)
        }
    }
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod cache_tests;
