//! Which computers a plugin's native program runs on, and the one sentence
//! that says so when this computer is not among them.
//!
//! A derivation's artifacts are downloads keyed by platform (`darwin-arm64`,
//! `linux-x64`, …, as `binary_resolver::get_current_platform` spells them);
//! one without a platform fits every computer. Install succeeds either way,
//! so without this the shortfall surfaces only at the first use, as a failure
//! in a build log. The catalog, the install and that first use all word it
//! through [`sentence`].

use super::derivation::DerivationContribution;
use crate::plugins::install::registry_client::enforce::Refusal;
use crate::plugins::install::registry_client::index::IndexEntry;
use crate::plugins::types::PluginManifest;
use moss_core::untrusted_text::{bounded, MAX_NAME};

fn host() -> Option<String> {
    crate::build::assets::binary_resolver::get_current_platform().ok()
}

/// Plain words for a platform key. A key this table does not know is shown
/// as written, so a registry that adds one before moss learns it still
/// produces a sentence.
fn words(key: &str) -> &str {
    match key {
        "darwin-arm64" => "Apple silicon Macs",
        "darwin-x64" => "Intel Macs",
        "linux-x64" => "64-bit Linux computers",
        "linux-arm64" => "ARM Linux computers",
        "windows-x64" => "Windows computers",
        "windows-arm64" => "ARM Windows computers",
        other => other,
    }
}

/// "Not available on this computer: PDF OCR runs on Apple silicon Macs only."
pub fn sentence(name: &str, platforms: &[String]) -> String {
    let mut named: Vec<&str> = Vec::new();
    for key in platforms {
        let w = words(key);
        if !named.contains(&w) {
            named.push(w);
        }
    }
    let where_ = match named.as_slice() {
        [] => "other computers".to_string(),
        [one] => one.to_string(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    };
    format!("Not available on this computer: {} runs on {where_} only.", bounded(name, MAX_NAME))
}

fn unavailable(platforms: &[String]) -> Refusal {
    Refusal::Unavailable { platforms: platforms.iter().map(|p| bounded(p, MAX_NAME)).collect() }
}

/// The platforms an index entry says it runs on, when this computer is not
/// one of them. An absent or empty list means every platform.
pub fn entry_refusal(entry: &IndexEntry) -> Option<Refusal> {
    refusal_for_listed(entry.platforms.as_deref()?, host().as_deref())
}

fn refusal_for_listed(listed: &[String], host: Option<&str>) -> Option<Refusal> {
    let excluded = !listed.is_empty() && !host.is_some_and(|h| listed.iter().any(|p| p == h));
    excluded.then(|| unavailable(listed))
}

/// The platforms a derivation's artifacts name, when none of its sources fits
/// this computer. Empty only for a declaration with no sources, which the
/// manifest parse already refuses.
pub fn unserved(derivation: &DerivationContribution) -> Option<Vec<String>> {
    unserved_on(derivation, host().as_deref())
}

fn unserved_on(derivation: &DerivationContribution, host: Option<&str>) -> Option<Vec<String>> {
    let sources = &derivation.run.artifact;
    let fits = sources.iter().any(|s| match s.platform() {
        None => true,
        Some(p) => Some(p) == host,
    });
    if fits {
        return None;
    }
    let mut named: Vec<String> = sources.iter().filter_map(|s| s.platform()).map(str::to_string).collect();
    named.sort();
    named.dedup();
    Some(named)
}

/// The refusal for an unpacked plugin one of whose derivations has no
/// artifact for this computer.
pub fn manifest_refusal(manifest: &PluginManifest) -> Option<Refusal> {
    let derivations = manifest.contributes.as_ref()?.derivations.as_ref()?;
    let host = host();
    derivations.values().find_map(|d| unserved_on(d, host.as_deref())).map(|p| unavailable(&p))
}

#[cfg(test)]
#[path = "platform_tests.rs"]
mod platform_tests;
