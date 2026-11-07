//! The site's source files at one moment, path → hash and size: what a
//! version records and what a restore compares against.
//!
//! Two producers, one shape. A publish takes the tree its sealed build read
//! ([`SourceTree::from_sealed`]), so the history record and the deploy
//! baseline name the same bytes. Everything the author does by hand (save a
//! version, restore, compare a version with now) takes the tree the disk
//! holds ([`SourceTree::on_disk`]) and never waits for a build: the editor
//! and any host without a build pipeline can keep versions.
//! A never-built folder lacks sources only a build records, such as theme fonts outside the sweep and synchronous config keys.
//!
//! The disk tree reuses the sweep's own population rule
//! (`ops::watch::sweep::walk::drift_eligible`), which a test pins to the set
//! a real build records, and its compare against the last build's manifest
//! (`build::watch::drift::detect_drift`): an unchanged file keeps the hash
//! the build recorded, and only a changed or new file is read and hashed.
//! Synchronous config sources always use raw disk bytes, because the build
//! hashes the theme stylesheet after rewriting its asset references.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::build::manifest::SealedManifest;
use crate::build::stat::FileStat;
use crate::build::types::SourceMetadata;
use crate::build::watch::{source_metadata_verdict, SourceVerdict};

#[derive(Debug, Clone, PartialEq)]
pub struct SourceTree {
    sources: HashMap<String, SourceMetadata>,
    /// The sealed build's generation, or a digest of the entries for a disk
    /// tree. A disk tree's id never matches a deploy baseline, so a version
    /// saved by hand never reads as the one that is live.
    generation_id: String,
}

impl SourceTree {
    pub fn from_sealed(sealed: &SealedManifest) -> Self {
        Self { sources: sealed.sources().clone(), generation_id: sealed.generation_id().to_string() }
    }

    /// The tree as the disk holds it now. A file that cannot be read keeps
    /// the hash the last build recorded ("unreadable is not absent"); a file
    /// still in the cloud that no build has seen is left out, as a build
    /// would leave it out.
    pub fn on_disk(vault_root: &Path) -> Result<Self, String> {
        use crate::build::watch::drift;
        use crate::ops::watch::sweep::walk;

        let walked = walk::walk_with_error(vault_root, None, None)
            .map_err(|e| format!("could not read the site's files: {e}"))?;
        let baseline = walk::read_baseline_from_disk(vault_root).unwrap_or_default();
        let report = drift::detect_drift(&baseline, &walked.files, &[], vault_root, drift::probe_missing, None, true);

        let mut changed: Vec<String> = report.drifted.into_keys().collect();
        changed.extend(crate::build::manifest::SYNCHRONOUS_CONFIG_SOURCE_KEYS.iter().map(|key| (*key).to_string()));
        // The build walks `.moss/theme` itself, so a theme file added since has no baseline entry.
        changed.extend(theme_source_keys(vault_root).into_iter().filter(|rel| !baseline.sources.contains_key(rel)));
        // Other build-recorded sources outside the sweep keep its stat verdict.
        let walked_rels: HashSet<&str> = walked.files.iter().map(|f| f.rel.as_str()).collect();
        for (rel, meta) in &baseline.sources {
            if walked_rels.contains(rel.as_str()) || changed.contains(rel) {
                continue;
            }
            let abs = vault_root.join(rel);
            let changed_or_gone = std::fs::metadata(&abs).map_or(true, |md| {
                matches!(source_metadata_verdict(meta, &md, &abs, baseline.captured_at), SourceVerdict::Changed)
            });
            if changed_or_gone {
                changed.push(rel.clone());
            }
        }

        let mut sources = baseline.sources;
        for rel in &changed {
            let abs = vault_root.join(rel);
            match hash_file(&abs) {
                Ok(meta) => {
                    sources.insert(rel.clone(), meta);
                }
                // A file evicted to the cloud is offline, not deleted.
                Err(e) if crate::build::icloud::is_definitely_absent(&abs, &e) => {
                    sources.remove(rel);
                }
                Err(e) => log::debug!("history: could not read {rel}, keeping the hash the last build recorded: {e}"),
            }
        }
        Ok(Self::from_sources(sources))
    }

    pub(crate) fn from_sources(sources: HashMap<String, SourceMetadata>) -> Self {
        let mut entries: Vec<(&String, &String)> = sources.iter().map(|(path, meta)| (path, &meta.hash)).collect();
        entries.sort();
        let mut digest = Sha256::new();
        for (path, hash) in entries {
            digest.update(path.as_bytes());
            digest.update([0]);
            digest.update(hash.as_bytes());
            digest.update([0]);
        }
        let hex = format!("{:x}", digest.finalize());
        let generation_id = format!("source-{}", hex.get(..12).unwrap_or(&hex));
        Self { sources, generation_id }
    }

    pub(crate) fn sources(&self) -> &HashMap<String, SourceMetadata> {
        &self.sources
    }

    pub(crate) fn generation_id(&self) -> &str {
        &self.generation_id
    }

    /// Path → hash, the shape `change_set::diff_hashes` compares.
    pub(crate) fn hashes(&self) -> HashMap<String, String> {
        self.sources.iter().map(|(path, meta)| (path.clone(), meta.hash.clone())).collect()
    }
}

/// Every file under `.moss/theme` as the build keys it. A cloud placeholder is
/// left out: its bytes cannot be read, and a build has recorded it if it can.
fn theme_source_keys(vault_root: &Path) -> Vec<String> {
    let dir = vault_root.join(".moss").join("theme");
    walkdir::WalkDir::new(&dir).into_iter().filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && crate::build::icloud::icloud_stub_target(entry.path()).is_none())
        .filter_map(|entry| {
            let rel = entry.path().strip_prefix(&dir).ok()?;
            Some(format!(".moss/theme/{}", moss_core::slug::normalize_separators(&rel.to_string_lossy())))
        })
        .collect()
}

/// Stream `path` through SHA-256, the hash the build records for a source.
fn hash_file(path: &Path) -> std::io::Result<SourceMetadata> {
    // allow:raw_read a source file the sweep walked; an error keeps the last recorded hash
    let mut file = std::fs::File::open(path)?;
    let stat = FileStat::of(&file.metadata()?);
    let mut digest = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
    }
    Ok(SourceMetadata::from_stat(format!("{:x}", digest.finalize()), stat))
}

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;
