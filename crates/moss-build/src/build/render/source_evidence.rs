//! The physical-source evidence seam between Loop A and the publish gate.

use crate::build::types::ParsedDocument;
use serde::{Deserialize, Serialize};
use specta::Type;

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

/// Physical-source evidence for one authored reference: a missing asset, or a
/// picture from another site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReferenceOccurrence {
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

/// A link from a published page to a `draft: true` page, which is built for
/// preview and never published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DraftLinkOccurrence {
    /// Markdown file holding the link, relative to the site root.
    pub source_path: String,
    /// The link as the built page carries it.
    pub reference: String,
    /// The draft page's source file.
    pub draft_source: String,
    /// The page is withheld because its source is empty, not by `draft: true`.
    #[serde(default)]
    pub empty: bool,
    /// One-based line of the link in `source_path`, when it could be found.
    pub line: Option<u32>,
}

/// The completed build's publish-time evidence, replaced atomically per
/// folder. An absent projection means no build in this process has answered;
/// a projection with no references is the completed clean answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PublishPreflightProjection {
    /// Ordered at build admission, so a later build cannot be overwritten by
    /// an older build that happens to complete after it.
    pub build_generation: u64,
    pub missing_references: Vec<ReferenceOccurrence>,
    /// Required inputs this attempt could not read. A clean later attempt
    /// replaces this list, even while an older complete seal remains selected.
    #[serde(default)]
    pub unresolved_inputs: Vec<String>,
    /// Links from published pages to drafts, which would be dead on the live site.
    #[serde(default)]
    pub drafts_linked: Vec<DraftLinkOccurrence>,
    /// Pictures the pages show from other sites: candidates the publish
    /// probes, never a verdict.
    #[serde(default)]
    pub remote_pictures: Vec<ReferenceOccurrence>,
}

/// A fresh source's bytes as Loop A read them, between parsing and the cache
/// snapshot. A build never writes them back, so evidence, hash and the uid
/// derived from them all describe the file as it was read.
pub(super) struct SourceRecord {
    pub document_index: usize,
    pub source_path: String,
    pub source: String,
    /// The file's stat, taken BEFORE `source` was read, so the record never
    /// pairs a later write's stat with these bytes' hash.
    stat: Option<crate::build::stat::FileStat>,
}

impl SourceRecord {
    pub(super) fn fresh(
        document_index: usize,
        source_path: String,
        source: String,
        stat_before_read: Option<crate::build::stat::FileStat>,
    ) -> Self {
        Self { document_index, source_path, source, stat: stat_before_read }
    }

    /// The file's stat from before `source` was read.
    pub(super) fn stat(&self) -> Option<crate::build::stat::FileStat> {
        self.stat
    }

    /// The manifest's record of `source` at the stat it was read at.
    pub(super) fn source_metadata(&self) -> crate::build::types::SourceMetadata {
        super::blocking::source_metadata(self.stat, self.source.as_bytes())
    }
}

/// What one source's authored references hold for the publish: the missing
/// assets that refuse it, and the pictures from other sites it probes.
#[derive(Debug, Default)]
pub struct AuthoredEvidence {
    pub missing: Vec<ReferenceOccurrence>,
    pub remote_pictures: Vec<ReferenceOccurrence>,
}

impl AuthoredEvidence {
    /// Both lists from one walk over one immutable physical source.
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
    ) -> Self {
        let source_revision = SourceRevision::from_source(source);
        let mut evidence = Self::default();
        for candidate in moss_core::resolve::authored_assets::extract_authored_asset_refs(source, parse_config) {
            let list = if candidate.missing_asset(source_path, context).is_some() {
                &mut evidence.missing
            } else if candidate.is_embed && moss_core::resolve::ext_kind::is_remote_image_url(&candidate.raw.text) {
                &mut evidence.remote_pictures
            } else {
                continue;
            };
            let line = source[..candidate.raw.ref_from].bytes().filter(|byte| *byte == b'\n').count().saturating_add(1);
            let (Ok(start_byte), Ok(end_byte), Ok(line)) =
                (candidate.raw.ref_from.try_into(), candidate.raw.ref_to.try_into(), line.try_into())
            else {
                continue;
            };
            list.push(ReferenceOccurrence {
                source_path: source_path.to_string(),
                source_revision: source_revision.clone(),
                reference: candidate.raw.text,
                source_span: SourceSpan { start_byte, end_byte, line },
            });
        }
        evidence
    }
}

/// Assemble the publish verdict only after Loop A has produced its complete,
/// indexed document set. Both fresh parses and cache replays carry their own
/// physical-source evidence, so this has no parallel side channel to merge.
pub(super) fn flatten_occurrences(
    documents: &[ParsedDocument],
    of: impl Fn(&ParsedDocument) -> &[ReferenceOccurrence],
) -> Vec<ReferenceOccurrence> {
    let mut all: Vec<_> = documents.iter().flat_map(|document| of(document).iter().cloned()).collect();
    all.sort_by(|left, right| {
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
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::folder_index::{NoFolderIndex, NoUrlIndex};
    use moss_core::content_graph::ContentGraphBuilder;
    use moss_core::resolve::reference::ReferenceContext;

    #[test]
    fn source_revision_is_a_transparent_hex_string_for_desktop_bindings() {
        let revision = SourceRevision::from_source("the final bytes");
        let json = serde_json::to_value(&revision).expect("revision serializes");
        let wire = json.as_str().expect("transparent wire string");
        assert_eq!(wire, revision.as_str());
        assert_eq!(wire.len(), 64);
        assert!(wire.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    }

    fn missing(path: &str, at: u32) -> ReferenceOccurrence {
        ReferenceOccurrence {
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
        let folders = NoFolderIndex;
        let urls = NoUrlIndex;
        let context = ReferenceContext {
            assets: &graph,
            folders: &folders,
            urls: &urls,
        };

        let evidence = AuthoredEvidence::from_authored_source(
            "notes/猫.md",
            source,
            &moss_core::ast::ParseConfig::default(),
            &context,
        )
        .missing;

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
    fn only_a_picture_from_another_site_is_a_remote_candidate() {
        let source = "![local](here.png)\n\n![far](https://cdn.example.net/a/cat.jpg)\n\n![video](https://www.youtube.com/watch?v=abc)\n\n[a link](https://cdn.example.net/b.jpg)\n\n<img src=\"https://pics.example.org/dog.webp\">\n";
        let graph = ContentGraphBuilder::new().build();
        let (folders, urls) = (NoFolderIndex, NoUrlIndex);
        let context = ReferenceContext { assets: &graph, folders: &folders, urls: &urls };

        let evidence = AuthoredEvidence::from_authored_source("p.md", source, &moss_core::ast::ParseConfig::default(), &context);

        assert_eq!(evidence.missing.iter().map(|o| o.reference.as_str()).collect::<Vec<_>>(), vec!["here.png"]);
        assert_eq!(
            evidence.remote_pictures.iter().map(|o| (o.reference.as_str(), o.source_span.line)).collect::<Vec<_>>(),
            vec![("https://cdn.example.net/a/cat.jpg", 3), ("https://pics.example.org/dog.webp", 9)],
        );
        for item in &evidence.remote_pictures {
            assert_eq!(&source[item.source_span.start_byte as usize..item.source_span.end_byte as usize], item.reference);
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
        let flattened = flatten_occurrences(&[cached, fresh], |d| &d.missing_reference_occurrences);
        assert_eq!(
            flattened
                .iter()
                .map(|item| (item.source_path.as_str(), item.source_span.start_byte))
                .collect::<Vec<_>>(),
            vec![("a.md", 2), ("a.md", 7), ("z.md", 9)],
        );
    }

    /// The manifest record pairs a stat with a hash, and the stat must be the one
    /// taken BEFORE the bytes were read: a same-size write landing after the read
    /// then shows a stat the record does not have, so the watcher hashes and sees
    /// the change. Stat'd after the read, the record would carry the new write's
    /// stat beside the old bytes' hash, and the watcher's fast path would vouch
    /// for bytes the build never read.
    #[test]
    fn a_write_after_the_read_leaves_a_record_the_file_no_longer_matches() {
        let temp = tempfile::tempdir().unwrap();
        let disk = temp.path().join("note.md");
        std::fs::write(&disk, "hello").unwrap();
        // The same-size write lands the moment the read returns.
        let (stat_before_read, source) = crate::build::stat::stat_then(&disk, |path| {
            let read = std::fs::read_to_string(path);
            let previous_mtime = std::fs::metadata(path).unwrap().modified().unwrap();
            std::fs::write(path, "world").unwrap();
            // Back-to-back writes can share a filesystem clock tick.
            crate::build::stat::FileStat::stamp_in_the_second_of(path, previous_mtime);
            read
        });
        let record = SourceRecord::fresh(0, "note.md".to_string(), source.unwrap(), stat_before_read);

        let now = std::fs::metadata(&disk).unwrap();
        assert_eq!(
            crate::build::watch::source_metadata_verdict(&record.source_metadata(), &now, &disk, None),
            crate::build::watch::SourceVerdict::Changed,
        );
    }
}
