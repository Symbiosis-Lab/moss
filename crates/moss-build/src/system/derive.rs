//! Running a plugin's declared derivation: one pinned binary, one input file,
//! one JSON answer — cached by the input's content so an unchanged file never
//! runs the binary twice.
//!
//! The binary reads `{"path": "<absolute input path>"}` on stdin and prints
//! exactly one JSON object on stdout, either
//! `{"kind":"derived","fragments":{"<name>":"<text>"},"facts":{"title":<string|null>,"description":<string|null>}}`
//! or `{"kind":"skip","reason":"<short>","message":"<sentences>"}`. Core knows
//! nothing about the file format; the declaration
//! (`plugins::contributions::derivation`) says which extensions go here and
//! which fragment names may come back.

pub mod batch;
pub mod binary;
pub mod cache;
pub mod registry;
pub mod run;
pub mod sandbox;
pub mod site;

#[cfg(test)]
pub(crate) mod test_support;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::plugins::contributions::derivation::FragmentType;

pub use batch::{derive_batch, derive_budget, DeriveJob};
pub use cache::{derive_cached, lookup_cached, lookup_failure, lookup_previous, remember_failure, remembered, DeriveResult};
pub use moss_core::resolve::embeds::derived::DerivedFacts;
pub use run::run_deriver;

/// The binary's answer, validated and in the form the cache stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DeriveOutcome {
    Derived {
        fragments: BTreeMap<String, String>,
        #[serde(default)]
        facts: DerivedFacts,
    },
    /// The binary looked at the file and chose not to derive it (it is
    /// encrypted, empty, …). A result, not an error: it is cached.
    Skip { reason: String, message: String },
}

impl DeriveOutcome {
    /// Every fragment must be one the declaration lists. All declared types
    /// carry text, so the declared type is checked by being a string at parse.
    pub fn check_against(
        &self,
        declared: &BTreeMap<String, FragmentType>,
    ) -> Result<(), DeriveFailure> {
        if let DeriveOutcome::Derived { fragments, .. } = self {
            if let Some(name) = fragments.keys().find(|n| !declared.contains_key(*n)) {
                return Err(DeriveFailure::new(format!(
                    "it produced a fragment `{name}` the plugin does not declare"
                )));
            }
        }
        Ok(())
    }
}

/// Why a file could not be derived. Never cached: the cause is usually the
/// environment (a missing binary, a timeout), and a retry may succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeriveFailure {
    pub message: String,
}

impl DeriveFailure {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for DeriveFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DeriveFailure {}
