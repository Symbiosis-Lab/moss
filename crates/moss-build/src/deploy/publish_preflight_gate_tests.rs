//! The publish gate: what it refuses, and — just as load-bearing — what it lets through.

use super::{refusal_text, refuse_publish};
use crate::build::types::{MissingReferenceOccurrence, PublishPreflightProjection, SourceRevision, SourceSpan};

/// The records are process-global, so every case keys on its own folder —
/// otherwise two tests running in parallel share one verdict.
fn record(folder: &str, generation: u64, missing_references: Vec<MissingReferenceOccurrence>) {
    crate::system::build_records::records().install_publish_preflight(
        folder,
        PublishPreflightProjection {
            build_generation: generation,
            missing_references,
        },
    );
}

fn missing(source: &str, reference: &str) -> MissingReferenceOccurrence {
    MissingReferenceOccurrence {
        source_path: source.into(),
        source_revision: SourceRevision::from_source("revision"),
        reference: reference.into(),
        source_span: SourceSpan { start_byte: 0, end_byte: 1, line: 1 },
    }
}

/// The refusal text is what a user reads, so it says what did not happen.
///
/// Not "publish failed" — publishing did not fail, it did not start. And no
/// mechanism vocabulary: no "unresolved reference", no "asset registry".
#[test]
fn the_refusal_names_the_outcome_not_the_mechanism() {
    let one = refusal_text(1);
    assert!(one.starts_with("Nothing published — 1 file is missing"), "{one}");
    assert!(one.contains("broken file"), "{one}");
    assert!(one.contains("Fix these, then publish again"), "{one}");

    let many = refusal_text(4);
    assert!(many.contains("4 files are missing"), "{many}");

    for text in [&one, &many] {
        for jargon in ["reference", "registry", "resolve", "diagnostic", "asset", "image", "media"] {
            assert!(
                !text.to_lowercase().contains(jargon),
                "user-facing refusal leaks '{jargon}': {text}"
            );
        }
    }
}

/// A clean build publishes. An empty list is a real verdict, not a missing one.
#[test]
fn an_empty_list_does_not_block() {
    record("/empty-list", 1, Vec::new());
    assert!(refuse_publish("/empty-list").is_ok());
}

/// A folder nothing built in this process publishes too. Absent is not a
/// verdict — refusing there would block a publish over a fact nobody computed.
#[test]
fn no_build_in_this_process_does_not_block() {
    assert!(refuse_publish("/never-built").is_ok());
}

/// One broken reference is enough. This is the gate the whole change exists for.
#[test]
fn a_single_missing_file_blocks_the_publish() {
    record("/one-missing", 1, vec![missing("about.md", "hero.jpg")]);
    let err = refuse_publish("/one-missing").expect_err("a missing file must stop the publish");
    assert!(err.contains("1 file is missing"), "{err}");
}

/// The gate is about authored files, not a particular HTML element: documents
/// can refer to a download or a video just as they can refer to a picture.
#[test]
fn missing_non_image_files_block_the_publish() {
    record(
        "/missing-non-images",
        1,
        vec![missing("guide.md", "handbook.pdf"), missing("about.md", "intro.mp4")],
    );
    let err = refuse_publish("/missing-non-images").expect_err("missing files must stop the publish");
    assert!(err.contains("2 files are missing"), "{err}");
    assert!(err.contains("broken file"), "{err}");
}

/// The verdict is per folder. Another site's broken files must not block this
/// one — the record outlives a folder switch.
#[test]
fn the_verdict_is_keyed_by_folder() {
    record("/keyed-other", 1, vec![missing("a.md", "x.png")]);
    record("/keyed-site", 1, Vec::new());
    assert!(refuse_publish("/keyed-site").is_ok());
    assert!(refuse_publish("/keyed-other").is_err());
}

/// Fixing the file unblocks the publish.
///
/// The single worst outcome available in this design is a stale verdict
/// blocking a publish for a file the author already fixed — there is no
/// override, so they would have no way out. The rebuild's write must replace,
/// never merge.
#[test]
fn a_rebuild_that_finds_nothing_clears_an_earlier_refusal() {
    record("/clears", 1, vec![missing("about.md", "hero.jpg")]);
    assert!(refuse_publish("/clears").is_err());

    record("/clears", 2, Vec::new());
    assert!(
        refuse_publish("/clears").is_ok(),
        "a clean rebuild must replace the previous verdict, not merge with it"
    );
}
