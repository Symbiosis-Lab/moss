//! Generation-owned identity of the original bytes behind preview variants.

use super::{PendingManifest, SealedManifest};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Original {
    pub source: String,
    pub oid: String,
    pub size: u64,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Originals {
    pub generation: String,
    pub images: BTreeMap<String, Original>,
}

pub(crate) fn receipt_path(generations: &Path, generation: &str) -> PathBuf {
    generations.join(format!(".{generation}.originals.json"))
}

impl Originals {
    pub(crate) fn get(&self, url: &str) -> Option<Original> {
        if let Some(original) = self.images.get(url) { return Some(original.clone()); }
        // Older cached HTML can retain the canonical source URL even after
        // its containing page gains a URL override. Only recorded identities
        // may supply that alias; today's source registry has no authority.
        let mut found = None;
        for original in self.images.values() {
            if !default_urls(&original.source).iter().any(|candidate| candidate == url) { continue; }
            if found.as_ref().is_some_and(|old| old != original) { return None; }
            found = Some(original.clone());
        }
        found
    }

    pub(crate) fn read(generations: &Path, generation: &str) -> std::io::Result<Option<Self>> {
        let bytes = match std::fs::read(receipt_path(generations, generation)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let receipt: Self = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        if receipt.generation != generation { return Err(std::io::Error::other("original identities name a different generation")); }
        Ok(Some(receipt))
    }

    /// Caller holds this generation's write lock. An output id alone does not
    /// identify its source bytes, so its first source binding is immutable.
    pub(crate) fn bind(&self, generations: &Path) -> std::io::Result<bool> {
        let path = receipt_path(generations, &self.generation);
        if let Some(existing) = Self::read(generations, &self.generation)? { return Ok(existing == *self); }
        let json = serde_json::to_string(self).map_err(std::io::Error::other)?;
        crate::infra::atomic_write::write_atomic(&path, &json).map(|()| true).map_err(std::io::Error::other)
    }

    pub(crate) fn legacy(hashes: &crate::types::content::SiteHashes, generation: String) -> Self {
        let mut candidates: BTreeMap<String, Option<Original>> = BTreeMap::new();
        for (source, metadata) in &hashes.sources {
            let ext = Path::new(source).extension().and_then(|ext| ext.to_str()).unwrap_or("");
            if !moss_core::asset_paths::is_ladder_source_ext(ext) { continue; }
            let original = Original { source: source.clone(), oid: metadata.hash.clone(), size: metadata.size };
            for url in default_urls(source) {
                candidates.entry(url).and_modify(|old| {
                    if old.as_ref() != Some(&original) { *old = None; }
                }).or_insert(Some(original.clone()));
            }
        }
        Self { generation, images: candidates.into_iter().filter_map(|(url, original)| original.map(|original| (url, original))).collect() }
    }
}

fn default_urls(source: &str) -> Vec<String> {
    let mapped = moss_core::resolve::output_url::resolve_path_with_overrides(source, &Default::default());
    std::iter::once(mapped.clone()).chain(std::iter::once(moss_core::asset_paths::to_webp(&mapped)))
        .chain(moss_core::asset_paths::LADDER.iter().map(|width| moss_core::asset_paths::to_webp_rung(&mapped, *width))).collect()
}

impl PendingManifest {
    pub(crate) fn register_preview_original(&mut self, url: String, source: String) {
        // Two sources promising one URL cannot establish its original owner.
        self.preview_originals.entry(url).and_modify(|old| {
            if old.as_deref() != Some(&source) { *old = None; }
        }).or_insert(Some(source));
    }
}

impl SealedManifest {
    pub(crate) fn write_preview_originals(&self, generations: &Path) -> std::io::Result<()> {
        let images = self.preview_originals.iter().filter_map(|(url, source)| {
            let source = source.as_ref()?;
            let metadata = self.inner.sources.get(source)?;
            Some((url.clone(), Original {
                source: source.clone(), oid: metadata.hash.clone(), size: metadata.size,
            }))
        }).collect();
        let receipt = Originals { generation: self.generation_id().into(), images };
        receipt.bind(generations).map(|_| ())
    }
}
