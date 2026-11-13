//! The two reuse permissions, pinned. Each test names what it forbids: on one
//! side a stale page served to the user, on the other a full 226-file reparse
//! charged to an edit that could not have invalidated one.

use super::*;
use crate::build::BuildTrigger;
use std::path::PathBuf;

/// An event batch never reads the disk, so no folder or seal is needed.
fn proof(trigger: BuildTrigger) -> ChangeProof {
    ChangeProof::for_build(&trigger, Path::new(""), "", &LastSeal::default())
}

fn paths(rels: &[&str]) -> Vec<PathBuf> {
    rels.iter().map(PathBuf::from).collect()
}

// ── The render skip: markdown-only, ContentOnly-only ──────────────────────

#[test]
fn the_render_skip_opens_only_for_a_content_only_markdown_batch() {
    assert!(proof(BuildTrigger::ContentOnly(paths(&["a.md"]))).allows_incremental_skip());
    assert!(
        !proof(BuildTrigger::ContentOnly(paths(&["a.md", "s.css"]))).allows_incremental_skip(),
        "a stylesheet moves build-globals baked into every page"
    );
    assert!(!proof(BuildTrigger::ContentOnly(vec![])).allows_incremental_skip());
    assert!(!proof(BuildTrigger::Full).allows_incremental_skip());
}

/// `Structural` must keep closing the render skip even now that it opens the
/// parse cache: a create or a delete moves pages in and out of listings, nav
/// and folder indexes, none of which is a page's own fingerprint.
#[test]
fn the_render_skip_stays_shut_for_a_structural_batch() {
    assert!(!proof(BuildTrigger::Structural(paths(&["a.md"]))).allows_incremental_skip());
}

// ── The parse cache: an extension question, not a trigger question ────────

/// THE fix. Loop A's reparse is the single largest item in a rebuild (6.5 s of
/// an 11.9 s build on a 226-page vault), and `Structural` was switching it off
/// wholesale — including when the trigger was structural for a reason that
/// cannot touch any markdown parse. Safety does not come from the trigger
/// here: it comes from `inputs_fingerprint`, which hashes the sorted
/// `markdown_files` list, so a create/delete/rename of markdown moves the
/// fingerprint and bypasses the cache for the whole build regardless.
#[test]
fn a_structural_markdown_batch_may_still_replay_the_parse_cache() {
    assert!(proof(BuildTrigger::Structural(paths(&["a.md"]))).allows_parse_cache_reuse());
    assert!(
        proof(BuildTrigger::Structural(paths(&["a.md", "s.css"]))).allows_parse_cache_reuse(),
        "stylesheets are parse-irrelevant whether the batch is a create or an edit"
    );
}

/// The allowlist is the whole safety argument and outranks the trigger in
/// both directions. An image baked dimensions and LQIP into parsed HTML,
/// so it disables the cache no matter how it arrived.
#[test]
fn an_image_disables_the_parse_cache_under_any_trigger() {
    assert!(!proof(BuildTrigger::Structural(paths(&["a.md", "hero.png"]))).allows_parse_cache_reuse());
    assert!(!proof(BuildTrigger::ContentOnly(paths(&["a.md", "hero.png"]))).allows_parse_cache_reuse());
    assert!(
        !proof(BuildTrigger::Structural(paths(&["c.toml"]))).allows_parse_cache_reuse(),
        "config.toml moves site scalars process_markdown_file reads"
    );
    assert!(
        !proof(BuildTrigger::Structural(paths(&["LICENSE"]))).allows_parse_cache_reuse(),
        "extensionless is unrecognised, and unrecognised is ineligible"
    );
}

/// `Full` is every non-watch entry point — `moss build`, `moss preview`,
/// deploy. They render everything by contract and must not consult a cache
/// this process happens to be holding.
#[test]
fn a_full_build_reuses_nothing() {
    assert!(!proof(BuildTrigger::Full).allows_parse_cache_reuse());
    assert!(!proof(BuildTrigger::Full).allows_incremental_skip());
}

#[test]
fn an_empty_batch_carries_no_information_so_it_reuses_nothing() {
    assert!(!proof(BuildTrigger::Structural(vec![])).allows_parse_cache_reuse());
    assert!(!proof(BuildTrigger::ContentOnly(vec![])).allows_parse_cache_reuse());
}

#[test]
fn every_page_extension_counts_as_markdown_for_the_gates() {
    for ext in ["md", "markdown", "mdown", "mkd", "MD", "Mkd"] {
        assert!(is_markdown_extension(ext), "{ext} is a page source");
    }
    assert!(!is_markdown_extension("txt"));
}

// ── A build that watched nothing ──────────────────────────────────────────

/// A save queued behind the first build is folded into it: the proof that
/// build asks for compares the whole disk with the last seal, so it sees that
/// path too. `Full` still absorbs everything.
#[test]
fn unobserved_absorbs_disk_changes_and_full_absorbs_unobserved() {
    let edit = || BuildTrigger::ContentOnly(paths(&["a.md"]));
    let created = || BuildTrigger::Structural(paths(&["b.md"]));
    assert_eq!(BuildTrigger::Unobserved.merge(edit()), BuildTrigger::Unobserved);
    assert_eq!(created().merge(BuildTrigger::Unobserved), BuildTrigger::Unobserved);
    assert_eq!(BuildTrigger::Unobserved.merge(BuildTrigger::Unobserved), BuildTrigger::Unobserved);
    assert_eq!(BuildTrigger::Unobserved.merge(BuildTrigger::Full), BuildTrigger::Full);
    assert_eq!(BuildTrigger::Full.merge(BuildTrigger::Unobserved), BuildTrigger::Full);
}

/// A manifest proof opens both gates even when nothing changed, where an
/// empty event batch carries no information.
#[test]
fn a_manifest_proof_opens_both_gates_even_when_nothing_changed() {
    assert!(ChangeProof::Manifest.allows_incremental_skip());
    assert!(ChangeProof::Manifest.allows_parse_cache_reuse());
    assert!(!ChangeProof::None.allows_incremental_skip());
    assert!(!ChangeProof::None.allows_parse_cache_reuse());
}

/// A folder with no sealed manifest proves nothing.
#[test]
fn an_unobserved_build_with_no_seal_renders_everything() {
    let dir = tempfile::tempdir().unwrap();
    let seal = LastSeal::read(dir.path(), Default::default());
    assert_eq!(ChangeProof::for_build(&BuildTrigger::Unobserved, dir.path(), "b", &seal), ChangeProof::None);
}

/// Agent guidance refreshes on every build that is not a watch batch, the
/// unobserved one included.
#[test]
fn only_a_watch_batch_is_a_watch_batch() {
    assert!(!BuildTrigger::Unobserved.is_watch_batch());
    assert!(!BuildTrigger::Full.is_watch_batch());
    assert!(BuildTrigger::ContentOnly(paths(&["a.md"])).is_watch_batch());
    assert!(BuildTrigger::Structural(paths(&["a.md"])).is_watch_batch());
}
