//! The physical-source evidence seam between Loop A and the publish gate.

use crate::build::types::ParsedDocument;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

/// A byte span in a physical markdown source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SourceSpan {
    pub start_byte: u32,
    pub end_byte: u32,
    /// One-based line containing this byte range in the same final source.
    pub line: u32,
}

/// SHA-256 of the final UTF-8 source bytes that produced an occurrence.
///
/// The transparent lowercase hexadecimal string is the stable serde and
/// Specta wire representation. Keeping it as text, rather than serializing
/// SHA-256's 32 bytes as an array, lets the desktop compare the digest it
/// calculates from the open file without a binary conversion boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
#[specta(transparent)]
pub struct SourceRevision(String);

impl SourceRevision {
    pub fn from_source(source: &str) -> Self {
        use sha2::{Digest, Sha256};

        Self(format!("{:x}", Sha256::digest(source.as_bytes())))
    }

    pub fn as_str(&self) -> &str { &self.0 }
}

/// Physical-source evidence for a missing authored asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MissingReferenceOccurrence {
    /// Markdown file holding the reference, relative to the site root.
    pub source_path: String,
    /// SHA-256 of the exact source bytes this occurrence was extracted from.
    pub source_revision: SourceRevision,
    /// The reference exactly as the author typed it.
    pub reference: String,
    /// Exact destination bytes in the physical source, never a post-resolution
    /// or transclusion coordinate.
    pub source_span: SourceSpan,
}

/// The completed build's publish-time evidence, replaced atomically per
/// folder. An absent projection means no build in this process has answered;
/// a projection with no references is the completed clean answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PublishPreflightProjection {
    /// Ordered at build admission, so a later build cannot be overwritten by
    /// an older build that happens to complete after it.
    pub build_generation: u64,
    pub missing_references: Vec<MissingReferenceOccurrence>,
}

/// The one owner of a fresh source's bytes between parsing and the cache
/// snapshot. UID normalization may change `final_source`, but evidence, hash
/// and disk write always consume that same field after normalization ends.
pub(super) struct FinalSourceRecord {
    pub document_index: usize,
    pub source_path: String,
    pub disk_path: PathBuf,
    pub original_source: String,
    pub final_source: String,
    pub original_uid: Option<String>,
}

impl FinalSourceRecord {
    pub(super) fn fresh(
        document_index: usize,
        source_path: String,
        disk_path: PathBuf,
        source: String,
        original_uid: Option<String>,
    ) -> Self {
        Self {
            document_index,
            source_path,
            disk_path,
            final_source: source.clone(),
            original_source: source,
            original_uid,
        }
    }

    pub(super) fn mint_missing_uid(&mut self, document: &mut ParsedDocument) {
        if document.uid.is_some() || document.slot_only {
            return;
        }
        let uid = crate::build::markdown::generate_uid(&self.source_path);
        self.final_source = crate::build::markdown::insert_uid_into_frontmatter(&self.final_source, &uid);
        document.uid = Some(uid);
    }

    pub(super) fn plan_duplicate_uid(&mut self, new_uid: &str) -> bool {
        let updated = crate::build::markdown::replace_uid_in_frontmatter(&self.final_source, new_uid);
        if updated == self.final_source {
            return self.final_source.contains(&format!("uid: \"{new_uid}\""));
        }
        self.final_source = updated;
        true
    }

    /// Persist only the final owned bytes. A failed write restores the exact
    /// prior source and tells the caller to restore its parsed UID as well.
    pub(super) fn write_final(&mut self) -> bool {
        if self.final_source == self.original_source {
            return true;
        }
        // allow:raw_write this writes authored markdown, not regenerable build output
        match std::fs::write(&self.disk_path, &self.final_source) {
            Ok(()) => true,
            Err(error) => {
                log::warn!(
                    "Failed to write normalized uid into '{}': {}",
                    self.source_path,
                    error
                );
                self.final_source.clone_from(&self.original_source);
                false
            }
        }
    }
}

impl MissingReferenceOccurrence {
    /// Build publish-blocking evidence from one immutable physical source.
    ///
    /// `extract_authored_asset_refs` owns syntax recognition and the shared
    /// resolver owns the missing-asset verdict. Keeping the conversion here
    /// makes a document's cached parse snapshot self-contained: fresh and
    /// replayed documents carry the same evidence shape.
    pub fn from_authored_source(
        source_path: &str,
        source: &str,
        parse_config: &moss_core::ast::ParseConfig,
        context: &moss_core::resolve::reference::ReferenceContext,
    ) -> Vec<Self> {
        let source_revision = SourceRevision::from_source(source);
        moss_core::resolve::authored_assets::extract_authored_asset_refs(source, parse_config)
            .into_iter()
            .filter_map(|candidate| {
                candidate.missing_asset(source_path, context)?;
                Some(MissingReferenceOccurrence {
                    source_path: source_path.to_string(),
                    source_revision: source_revision.clone(),
                    reference: candidate.raw.text,
                    source_span: SourceSpan {
                        start_byte: candidate.raw.ref_from.try_into().ok()?,
                        end_byte: candidate.raw.ref_to.try_into().ok()?,
                        line: source[..candidate.raw.ref_from]
                            .bytes()
                            .filter(|byte| *byte == b'\n')
                            .count()
                            .saturating_add(1)
                            .try_into()
                            .ok()?,
                    },
                })
            })
            .collect()
    }
}

/// Assemble the publish verdict only after Loop A has produced its complete,
/// indexed document set. Both fresh parses and cache replays carry their own
/// physical-source evidence, so this has no parallel side channel to merge.
pub(super) fn flatten_missing_reference_occurrences(
    documents: &[ParsedDocument],
) -> Vec<MissingReferenceOccurrence> {
    let mut missing: Vec<_> = documents
        .iter()
        .flat_map(|document| document.missing_reference_occurrences.iter().cloned())
        .collect();
    missing.sort_by(|left, right| {
        (
            left.source_path.as_str(),
            left.source_span.start_byte,
            left.source_span.end_byte,
            left.reference.as_str(),
        )
            .cmp(&(
                right.source_path.as_str(),
                right.source_span.start_byte,
                right.source_span.end_byte,
                right.reference.as_str(),
            ))
    });
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::folder_index::{NoFolderIndex, NoUrlIndex};
    use moss_core::ast::resolve_urls::GraphAssetIndex;
    use moss_core::content_graph::ContentGraphBuilder;
    use moss_core::resolve::reference::ReferenceContext;
    use sha2::Digest;

    #[test]
    fn source_revision_is_a_transparent_hex_string_for_desktop_bindings() {
        let revision = SourceRevision::from_source("the final bytes");
        let json = serde_json::to_value(&revision).expect("revision serializes");
        let wire = json.as_str().expect("transparent wire string");
        assert_eq!(wire, revision.as_str());
        assert_eq!(wire.len(), 64);
        assert!(wire.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    }

    fn missing(path: &str, at: u32) -> MissingReferenceOccurrence {
        MissingReferenceOccurrence {
            source_path: path.to_string(),
            source_revision: SourceRevision::from_source("revision"),
            reference: format!("{path}-{at}"),
            source_span: SourceSpan {
                start_byte: at,
                end_byte: at + 1,
                line: 1,
            },
        }
    }

    #[test]
    fn source_evidence_keeps_every_blocking_candidate_on_its_physical_span() {
        let source = "before\r\n![猫](gone.png)\r\n![[also-gone.jpg#part]]\r\n:::hero missing.webp\r\nbody\r\n:::\r\n";
        let graph = ContentGraphBuilder::new().build();
        let assets = GraphAssetIndex(&graph);
        let folders = NoFolderIndex;
        let urls = NoUrlIndex;
        let context = ReferenceContext {
            assets: &assets,
            folders: &folders,
            urls: &urls,
        };

        let evidence = MissingReferenceOccurrence::from_authored_source(
            "notes/猫.md",
            source,
            &moss_core::ast::ParseConfig::default(),
            &context,
        );

        assert_eq!(
            evidence.iter().map(|item| item.reference.as_str()).collect::<Vec<_>>(),
            vec!["gone.png", "also-gone.jpg#part", "missing.webp"],
        );
        assert!(evidence.iter().all(|item| item.source_revision.as_str().len() == 64));
        assert_eq!(evidence[0].source_span.line, 2);
        assert_eq!(evidence[1].source_span.line, 3);
        assert_eq!(evidence[2].source_span.line, 4);
        for item in &evidence {
            let span = item.source_span.start_byte as usize..item.source_span.end_byte as usize;
            assert_eq!(&source[span], item.reference);
        }
    }

    #[test]
    fn flattening_document_evidence_is_deterministic_across_fresh_and_cached_order() {
        let cached = ParsedDocument {
            missing_reference_occurrences: vec![missing("z.md", 9)],
            ..Default::default()
        };
        let fresh = ParsedDocument {
            missing_reference_occurrences: vec![missing("a.md", 7), missing("a.md", 2)],
            ..Default::default()
        };
        let flattened = flatten_missing_reference_occurrences(&[cached, fresh]);
        assert_eq!(
            flattened
                .iter()
                .map(|item| (item.source_path.as_str(), item.source_span.start_byte))
                .collect::<Vec<_>>(),
            vec![("a.md", 2), ("a.md", 7), ("z.md", 9)],
        );
    }

    #[test]
    fn uid_writeback_hands_evidence_the_exact_bytes_it_writes() {
        let source = "---\ntitle: Hé\n---\n![alt](gone.png)\n";
        let temp = tempfile::tempdir().unwrap();
        let disk = temp.path().join("note.md");
        std::fs::write(&disk, source).unwrap();
        let mut document = ParsedDocument::default();
        let mut record = FinalSourceRecord::fresh(
            0,
            "note.md".to_string(),
            disk.clone(),
            source.to_string(),
            None,
        );
        record.mint_missing_uid(&mut document);
        assert!(record.write_final());
        let written = std::fs::read_to_string(&disk).unwrap();
        assert_eq!(written, record.final_source);
        let offset = record.final_source.find("gone.png").expect("reference survives uid insertion");
        assert!(offset > source.find("gone.png").unwrap(), "uid shifts the physical span");

        let graph = ContentGraphBuilder::new().build();
        let assets = GraphAssetIndex(&graph);
        let folders = NoFolderIndex;
        let urls = NoUrlIndex;
        let context = ReferenceContext {
            assets: &assets,
            folders: &folders,
            urls: &urls,
        };
        let evidence = MissingReferenceOccurrence::from_authored_source(
            "note.md",
            &record.final_source,
            &moss_core::ast::ParseConfig::default(),
            &context,
        );
        assert_eq!(evidence.len(), 1);
        let revision = format!("{:x}", sha2::Sha256::digest(written.as_bytes()));
        assert_eq!(evidence[0].source_revision.as_str(), revision);
        let span = evidence[0].source_span.start_byte as usize..evidence[0].source_span.end_byte as usize;
        assert_eq!(&written[span], "gone.png");
    }

    #[test]
    fn duplicate_uid_reassignment_keeps_evidence_on_its_final_written_bytes() {
        let source = "---\ntitle: Hé\nuid: old-id\n---\n![alt](gone.png)\n";
        let temp = tempfile::tempdir().unwrap();
        let disk = temp.path().join("copy.md");
        std::fs::write(&disk, source).unwrap();
        let mut record = FinalSourceRecord::fresh(
            0,
            "copy.md".to_string(),
            disk.clone(),
            source.to_string(),
            Some("old-id".to_string()),
        );
        assert!(record.plan_duplicate_uid("replacement-id"));
        assert!(record.write_final());
        let written = std::fs::read_to_string(&disk).unwrap();

        let graph = ContentGraphBuilder::new().build();
        let assets = GraphAssetIndex(&graph);
        let folders = NoFolderIndex;
        let urls = NoUrlIndex;
        let context = ReferenceContext { assets: &assets, folders: &folders, urls: &urls };
        let evidence = MissingReferenceOccurrence::from_authored_source(
            "copy.md", &record.final_source, &moss_core::ast::ParseConfig::default(), &context,
        );
        assert_eq!(record.final_source, written);
        assert_eq!(
            evidence[0].source_revision.as_str(),
            format!("{:x}", sha2::Sha256::digest(written.as_bytes()))
        );
        let span = evidence[0].source_span.start_byte as usize..evidence[0].source_span.end_byte as usize;
        assert_eq!(&written[span], "gone.png");
    }
}
