//! `contributes.stack` — a plugin's declaration of the long-lived local
//! companion process moss installs and runs on its behalf (ADR-080 item 3).
//!
//! This slice (S3 of docs/archive/2026-09-10-plugin-owned-stack-lifecycle-design.md)
//! declares the whole contract and consumes only the pin: `version`, the
//! winning [`StackSource`]'s `url` + `sha256`. Nothing here spawns a process
//! or extracts an archive — that is S4's executor
//! (`crates/moss-build/src/system/stack_exec.rs`).

use serde::{Deserialize, Serialize};

/// One plugin's declared stack. `id` is what a `Need::Stack` need resolves
/// against (`PluginManifest::stack_id`); a plugin declares at most one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StackContribution {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub version: String,
    pub sources: Vec<StackSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub start: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uninstall: Vec<String>,
    /// The escalation ladder, rung by rung, cheapest first. moss runs the argv
    /// it is given and nothing else: that is what makes "moss mints no onion
    /// keys" mechanical rather than reviewed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recover: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_heal_grace_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preserve_on_uninstall: Vec<String>,
    #[serde(default, skip_serializing_if = "StackHooks::is_empty")]
    pub hooks: StackHooks,
}

impl StackContribution {
    /// ADR-080 item 5: a `download` source with no `sha256` is refused —
    /// not warned, not defaulted. A `path` source is exempt: no artifact is
    /// fetched, so there is nothing to hash.
    pub fn validate(&self) -> Result<(), StackDeclarationError> {
        if self.id.trim().is_empty() {
            return Err(StackDeclarationError::EmptyId);
        }
        if self.sources.is_empty() {
            return Err(StackDeclarationError::NoSources { stack: self.id.clone() });
        }
        for source in &self.sources {
            if let StackSource::Download { sha256: None, url, .. } = source {
                return Err(StackDeclarationError::MissingSha256 {
                    stack: self.id.clone(),
                    url: url.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Where a stack's artifact comes from, on the wire as `{"kind": "download" |
/// "path", ...}` — a JSON literal an author writes by hand, so the tag is
/// asserted against that literal rather than a serialize round trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StackSource {
    Download {
        /// `darwin-arm64`, `linux-x64`, …: the vocabulary
        /// `binary_resolver::get_current_platform` already produces. Absent
        /// means every platform.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        platform: Option<String>,
        url: String,
        /// `Option` so the refusal in [`StackContribution::validate`] can
        /// name the field, instead of serde reporting a missing key on a
        /// variant the author may not know they picked.
        #[serde(default)]
        sha256: Option<String>,
        archive_format: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executable: Option<String>,
    },
    /// A binary the user already installed. S4 resolves no `PATH`: `binary`
    /// is used as given when absolute, and joined onto the stack's home
    /// otherwise (see `system::stack_exec::layout::binary_path`) — a `PATH`
    /// search is S5's to add, if it is ever wanted. No artifact is fetched,
    /// so there is nothing to hash (ADR-080 ruling addition).
    Path {
        /// `darwin-arm64`, `linux-x64`, …: the vocabulary
        /// `binary_resolver::get_current_platform` already produces. Absent
        /// means every platform.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        platform: Option<String>,
        binary: String,
    },
}

impl StackSource {
    fn platform(&self) -> Option<&str> {
        match self {
            StackSource::Download { platform, .. } => platform.as_deref(),
            StackSource::Path { platform, .. } => platform.as_deref(),
        }
    }
}

/// The one source this platform can install from, if any.
///
/// The first entry in `sources`, in list order, whose `platform` is absent
/// (a wildcard) or equals `platform_key` wins. `platform_key` is `None` when
/// the host's platform could not be determined
/// (`binary_resolver::get_current_platform` erroring) — in that case only
/// wildcard entries can match. Pure over its arguments, so the test passes
/// the key in rather than reading the host's; the one production caller
/// (`system::stack_install::pin::artifact_source`) supplies the host's own
/// key.
pub fn artifact_source<'a>(
    stack: &'a StackContribution,
    platform_key: Option<&str>,
) -> Option<&'a StackSource> {
    stack.sources.iter().find(|source| match source.platform() {
        None => true,
        Some(p) => Some(p) == platform_key,
    })
}

/// Which hooks the plugin exports for this stack. Booleans, following
/// `setup.check: true`: the plugin says it implements the hook, and the export
/// names are fixed — `stackHealth` and `stackProvision`. A plugin declares at
/// most one stack, so no two health hooks can collide on one name, and a
/// declared-but-missing export is already an ordinary `Ok(None)` at
/// `manager.rs:611-613` rather than an error needing a name to report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct StackHooks {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub health: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provision: bool,
}

impl StackHooks {
    fn is_empty(&self) -> bool {
        !self.health && !self.provision
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StackDeclarationError {
    MissingSha256 { stack: String, url: String },
    NoSources { stack: String },
    EmptyId,
}

impl std::fmt::Display for StackDeclarationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StackDeclarationError::MissingSha256 { stack, url } => write!(
                f,
                "stack `{stack}` declares a download source with no sha256 ({url}) — a \
                 download source must be hashed"
            ),
            StackDeclarationError::NoSources { stack } => {
                write!(f, "stack `{stack}` declares no sources")
            }
            StackDeclarationError::EmptyId => {
                write!(f, "a stack declaration needs a non-empty id")
            }
        }
    }
}

#[cfg(test)]
#[path = "stack_tests.rs"]
mod stack_tests;
