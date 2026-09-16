//! The publish gate: what it refuses, and — just as load-bearing — what it lets through.
//!
//! Moved with [`super::refuse_publish`] at C4f; unchanged except for the two
//! paths that used to name this crate from outside it.

use super::{refusal_text, refuse_publish};
use crate::build::types::MissingMedia;

/// The records are process-global, so every case keys on its own folder —
/// otherwise two tests running in parallel share one verdict.
fn record(folder: &str, missing: Vec<MissingMedia>) {
    crate::system::build_records::records().record_missing_media(folder, missing);
}

fn missing(source: &str, reference: &str) -> MissingMedia {
    MissingMedia {
        source_path: source.into(),
        reference: reference.into(),
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
    assert!(one.contains("Fix these, then publish again"), "{one}");

    let many = refusal_text(4);
    assert!(many.contains("4 files are missing"), "{many}");

    for text in [&one, &many] {
        for jargon in ["reference", "registry", "resolve", "diagnostic", "asset"] {
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
    record("/empty-list", Vec::new());
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
    record("/one-missing", vec![missing("about.md", "hero.jpg")]);
    let err = refuse_publish("/one-missing").expect_err("a missing file must stop the publish");
    assert!(err.contains("1 file is missing"), "{err}");
}

/// The verdict is per folder. Another site's broken images must not block this
/// one — the record outlives a folder switch.
#[test]
fn the_verdict_is_keyed_by_folder() {
    record("/keyed-other", vec![missing("a.md", "x.png")]);
    record("/keyed-site", Vec::new());
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
    record("/clears", vec![missing("about.md", "hero.jpg")]);
    assert!(refuse_publish("/clears").is_err());

    record("/clears", Vec::new());
    assert!(
        refuse_publish("/clears").is_ok(),
        "a clean rebuild must replace the previous verdict, not merge with it"
    );
}
