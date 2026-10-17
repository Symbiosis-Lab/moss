//! Which plugin handles which file extension.
//!
//! The build asks one question per source file — "does a plugin derive
//! fragments from this kind of file?" — and this is the answer, built once
//! from the discovered plugins. If two plugins accept the same extension the
//! one whose name sorts first wins and the other is reported, so a site never
//! changes behaviour with directory-listing order.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::plugins::contributions::derivation::DerivationContribution;
use crate::plugins::types::Plugin;

/// One derivation, with everything needed to run and cache it.
#[derive(Debug, Clone, PartialEq)]
pub struct DerivationEntry {
    pub plugin_id: String,
    pub derivation_id: String,
    /// The plugin's manifest version: part of the cache key, so shipping a new
    /// version re-derives.
    pub plugin_version: String,
    pub plugin_dir: PathBuf,
    pub contribution: DerivationContribution,
}

#[derive(Debug, Default, Clone)]
pub struct DerivationRegistry {
    by_extension: BTreeMap<String, DerivationEntry>,
    diagnostics: Vec<String>,
}

impl DerivationRegistry {
    pub fn from_plugins(plugins: &[Plugin]) -> Self {
        let mut ordered: Vec<&Plugin> = plugins.iter().collect();
        ordered.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));

        let mut registry = Self::default();
        for plugin in ordered {
            let Some(derivations) =
                plugin.manifest.contributes.as_ref().and_then(|c| c.derivations.as_ref())
            else {
                continue;
            };
            // BTreeMap order: derivation ids are visited alphabetically.
            for (derivation_id, contribution) in derivations {
                for ext in &contribution.accepts {
                    if let Some(winner) = registry.by_extension.get(ext) {
                        registry.diagnostics.push(format!(
                            "`{}/{}` also accepts `.{ext}` files but `{}/{}` already handles them; \
                             `{}/{}` is ignored for that type",
                            plugin.manifest.name,
                            derivation_id,
                            winner.plugin_id,
                            winner.derivation_id,
                            plugin.manifest.name,
                            derivation_id,
                        ));
                        continue;
                    }
                    registry.by_extension.insert(
                        ext.clone(),
                        DerivationEntry {
                            plugin_id: plugin.manifest.name.clone(),
                            derivation_id: derivation_id.clone(),
                            plugin_version: plugin.manifest.version.clone(),
                            plugin_dir: plugin.path.clone(),
                            contribution: contribution.clone(),
                        },
                    );
                }
            }
        }
        registry
    }

    /// The derivation for a file extension, with or without the dot, in any case.
    pub fn for_extension(&self, ext: &str) -> Option<&DerivationEntry> {
        self.by_extension.get(&ext.trim_start_matches('.').to_ascii_lowercase())
    }

    /// Every derivation once, however many extensions it accepts, ordered by
    /// the first extension it handles.
    pub fn entries(&self) -> impl Iterator<Item = &DerivationEntry> {
        let mut seen = std::collections::BTreeSet::new();
        self.by_extension
            .values()
            .filter(move |e| seen.insert((e.plugin_id.as_str(), e.derivation_id.as_str())))
    }

    /// Sentences about extensions two plugins both claimed.
    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub fn is_empty(&self) -> bool {
        self.by_extension.is_empty()
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod registry_tests;
