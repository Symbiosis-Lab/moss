//! The publish view of a sealed manifest, and the record of which files of a
//! generation are built for preview only.
//!
//! The generation directory holds draft pages so the preview can serve them,
//! so anything that lists or reads that directory directly (a deploy plugin,
//! the site-file commands plugins call) needs this list to leave them out.
//! Recorded beside the generation, like the original-identities receipt.

use super::SealedManifest;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

fn receipt_path(generations: &Path, generation: &str) -> PathBuf {
    generations.join(format!(".{generation}.withheld.json"))
}

impl SealedManifest {
    /// The output-path → mode-tagged-hash entries that are published: every
    /// built file except a `draft: true` page's HTML. Every publish path (the
    /// upload, the change set, the sitemap-adjacent counts) reads this view.
    /// Borrowed when nothing is withheld, which is the common case.
    pub fn files(&self) -> std::borrow::Cow<'_, HashMap<String, String>> {
        let withheld = self.withheld();
        if withheld.is_empty() {
            return std::borrow::Cow::Borrowed(&self.inner.files);
        }
        std::borrow::Cow::Owned(
            self.inner.files.iter()
                .filter(|(key, _)| !withheld.contains(key.as_str()))
                .map(|(key, entry)| (key.clone(), entry.clone()))
                .collect(),
        )
    }

    /// Everything the generation holds, drafts included. Preview serves it and
    /// the generation directory is copied from it; nothing that publishes may.
    pub fn all_files(&self) -> &HashMap<String, String> {
        &self.inner.files
    }

    /// [`source_to_output`][Self::source_to_output] without the drafts: the page
    /// set a publish serves.
    pub fn published_source_to_output(&self) -> std::borrow::Cow<'_, HashMap<String, String>> {
        let withheld = self.withheld();
        if withheld.is_empty() {
            return std::borrow::Cow::Borrowed(&self.inner.source_to_output);
        }
        std::borrow::Cow::Owned(
            self.inner.source_to_output.iter()
                .filter(|(_, out)| !withheld.contains(out.as_str()))
                .map(|(src, out)| (src.clone(), out.clone()))
                .collect(),
        )
    }

    /// Output paths built for preview only: the pages whose source says
    /// `draft: true`. Derived from the persisted page metadata, so a page
    /// carried forward unrendered stays withheld.
    pub fn withheld(&self) -> HashSet<&str> {
        self.inner.page_meta.iter()
            .filter(|(_, meta)| meta.draft)
            .filter_map(|(source, _)| self.inner.source_to_output.get(source).map(String::as_str))
            .collect()
    }

    pub(crate) fn write_publish_view(&self, generations: &Path) -> std::io::Result<()> {
        let withheld: BTreeSet<&str> = self.withheld().into_iter().collect();
        let json = serde_json::to_string(&withheld).map_err(std::io::Error::other)?;
        crate::infra::atomic_write::write_atomic(&receipt_path(generations, self.generation_id()), &json)
            .map_err(std::io::Error::other)
    }
}

/// The paths of `generation` that must not leave the machine; empty when the
/// generation has no receipt (it holds no drafts).
pub(crate) fn read_withheld(generations: &Path, generation: &str) -> BTreeSet<String> {
    std::fs::read(receipt_path(generations, generation))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// [`read_withheld`] for the generation `current` names.
pub(crate) fn withheld_in_current(mp: &crate::moss_paths::MossPaths) -> BTreeSet<String> {
    mp.current_generation_id()
        .map(|generation| read_withheld(&mp.generations_dir(), &generation))
        .unwrap_or_default()
}
