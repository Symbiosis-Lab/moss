//! The two reuse permissions, pinned. Each test names what it forbids: on one
//! side a stale page served to the user, on the other a full 226-file reparse
//! charged to an edit that could not have invalidated one.

use super::*;
use crate::build::{BuildTrigger, PluginMode, VaultRoot};
use std::path::PathBuf;

fn cfg(trigger: BuildTrigger) -> PipelineConfig {
    PipelineConfig {
        root: VaultRoot::resolve(&PathBuf::from("/tmp/moss-gate-test")),
        progress: crate::build::null_sink(),
        plugins: PluginMode::Blocking,
        watch: true,
        start_server: false,
        host: crate::build::ports::host::test_host_ports(),
        trigger,
        exits_after_build: false,
        site_url_override: None,
        admission_epoch: None,
        server_port: None,
        live_port: None,
    }
}

fn paths(rels: &[&str]) -> Vec<PathBuf> {
    rels.iter().map(PathBuf::from).collect()
}

// ── The render skip: markdown-only, ContentOnly-only ──────────────────────

#[test]
fn the_render_skip_opens_only_for_a_content_only_markdown_batch() {
    assert!(cfg(BuildTrigger::ContentOnly(paths(&["a.md"]))).allows_incremental_skip());
    assert!(
        !cfg(BuildTrigger::ContentOnly(paths(&["a.md", "s.css"]))).allows_incremental_skip(),
        "a stylesheet moves build-globals baked into every page"
    );
    assert!(!cfg(BuildTrigger::ContentOnly(vec![])).allows_incremental_skip());
    assert!(!cfg(BuildTrigger::Full).allows_incremental_skip());
}

/// `Structural` must keep closing the render skip even now that it opens the
/// parse cache: a create or a delete moves pages in and out of listings, nav
/// and folder indexes, none of which is a page's own fingerprint.
#[test]
fn the_render_skip_stays_shut_for_a_structural_batch() {
    assert!(!cfg(BuildTrigger::Structural(paths(&["a.md"]))).allows_incremental_skip());
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
    assert!(cfg(BuildTrigger::Structural(paths(&["a.md"]))).allows_parse_cache_reuse());
    assert!(
        cfg(BuildTrigger::Structural(paths(&["a.md", "s.css"]))).allows_parse_cache_reuse(),
        "stylesheets are parse-irrelevant whether the batch is a create or an edit"
    );
}

/// The allowlist is the whole safety argument and outranks the trigger in
/// both directions. An image baked dimensions and LQIP into parsed HTML,
/// so it disables the cache no matter how it arrived.
#[test]
fn an_image_disables_the_parse_cache_under_any_trigger() {
    assert!(!cfg(BuildTrigger::Structural(paths(&["a.md", "hero.png"]))).allows_parse_cache_reuse());
    assert!(!cfg(BuildTrigger::ContentOnly(paths(&["a.md", "hero.png"]))).allows_parse_cache_reuse());
    assert!(
        !cfg(BuildTrigger::Structural(paths(&["c.toml"]))).allows_parse_cache_reuse(),
        "config.toml moves site scalars process_markdown_file reads"
    );
    assert!(
        !cfg(BuildTrigger::Structural(paths(&["LICENSE"]))).allows_parse_cache_reuse(),
        "extensionless is unrecognised, and unrecognised is ineligible"
    );
}

/// `Full` is every non-watch entry point — `moss build`, `moss preview`,
/// deploy. They render everything by contract and must not consult a cache
/// this process happens to be holding.
#[test]
fn a_full_build_reuses_nothing() {
    assert!(!cfg(BuildTrigger::Full).allows_parse_cache_reuse());
    assert!(!cfg(BuildTrigger::Full).allows_incremental_skip());
}

#[test]
fn an_empty_batch_carries_no_information_so_it_reuses_nothing() {
    assert!(!cfg(BuildTrigger::Structural(vec![])).allows_parse_cache_reuse());
    assert!(!cfg(BuildTrigger::ContentOnly(vec![])).allows_parse_cache_reuse());
}
