//! The publish gate's second rule: this build's own unfulfilled promise (a
//! video, or its poster, still mid-encode when the page that embeds it
//! sealed). Mirrors `publish_preflight_gate_tests.rs` at the `refuse_publish`
//! layer; the wiring that PRODUCES this record from a real seal is tested in
//! `build/promise_gate_tests.rs`, and the pure audit ∩ promises filter in
//! `build/manifest/link_audit_tests.rs`.

use super::{in_flight_refusal_text, refuse_publish};
use crate::build::manifest::link_audit::DeadLink;

/// The records are process-global, so every case keys on its own folder —
/// otherwise two tests running in parallel share one verdict.
fn record(folder: &str, dead: Vec<DeadLink>) {
    crate::system::build_records::records().record_promised_dead_links(folder, dead);
}

fn dead(page: &str, href: &str) -> DeadLink {
    DeadLink { page: page.into(), href: href.into() }
}

/// Distinct wording from the missing-file refusal: the reference isn't
/// broken, so "fix this" would send an author hunting for a file that does
/// not exist.
#[test]
fn the_refusal_reads_as_wait_not_fix() {
    let one = in_flight_refusal_text(1);
    assert!(one.starts_with("Nothing published — 1 file is still being prepared"), "{one}");
    assert!(one.contains("Publish again in a moment"), "{one}");
    assert!(!one.to_lowercase().contains("fix"), "must not read as the author's mistake: {one}");

    let many = in_flight_refusal_text(3);
    assert!(many.contains("3 files are still being prepared"), "{many}");
}

/// A clean seal — nothing pending — publishes.
#[test]
fn an_empty_list_does_not_block() {
    record("/promise-empty", Vec::new());
    assert!(refuse_publish("/promise-empty").is_ok());
}

/// A folder nothing sealed in this process publishes too — absent is not a
/// verdict, same distinction the preflight report draws.
#[test]
fn no_seal_in_this_process_does_not_block() {
    assert!(refuse_publish("/promise-never-sealed").is_ok());
}

/// The shape the whole fix exists for: a video (and its poster) still
/// mid-encode when the page referencing them sealed.
#[test]
fn an_unfulfilled_promise_blocks_the_publish() {
    record("/promise-one", vec![dead("clip/index.html", "/videos/clip.mp4")]);
    let err = refuse_publish("/promise-one").expect_err("an in-flight promise must stop the publish");
    assert!(err.contains("1 file is still being prepared"), "{err}");
}

/// Verdicts are per folder — another site's in-flight video must not block
/// this one.
#[test]
fn the_verdict_is_keyed_by_folder() {
    record("/promise-keyed-other", vec![dead("a/index.html", "/videos/a.mp4")]);
    record("/promise-keyed-site", Vec::new());
    assert!(refuse_publish("/promise-keyed-site").is_ok());
    assert!(refuse_publish("/promise-keyed-other").is_err());
}

/// The follow-up rebuild's clean seal clears the earlier refusal — there is
/// no override, so a stale verdict would leave the author with no way out.
#[test]
fn a_rebuild_that_finds_nothing_pending_clears_an_earlier_refusal() {
    record("/promise-clears", vec![dead("clip/index.html", "/videos/clip.mp4")]);
    assert!(refuse_publish("/promise-clears").is_err());

    record("/promise-clears", Vec::new());
    assert!(
        refuse_publish("/promise-clears").is_ok(),
        "a clean rebuild must replace the previous verdict, not merge with it"
    );
}

/// The two rules are independent, and either alone is enough to refuse — a
/// folder cleared of missing files but still carrying an in-flight promise
/// must still be refused.
#[test]
fn either_rule_alone_is_enough_to_refuse() {
    crate::system::build_records::records().install_publish_preflight(
        "/promise-either",
        crate::build::types::PublishPreflightProjection { build_generation: 1, missing_references: Vec::new() },
    );
    record("/promise-either", vec![dead("clip/index.html", "/videos/clip.thumb.jpg")]);
    let err = refuse_publish("/promise-either").expect_err("the in-flight rule alone must refuse");
    assert!(err.contains("still being prepared"), "{err}");
}
