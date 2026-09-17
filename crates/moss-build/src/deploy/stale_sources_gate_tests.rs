//! The publish gate's third rule: a structural source (a page, `config.toml`,
//! the user stylesheet) the last build carried forward rather than read.
//! Mirrors `missing_media_gate_tests.rs` and `promised_dead_links_gate_tests.rs`
//! at the `refuse_publish` layer; the wiring that PRODUCES this record from a
//! real seal is tested at the full-pipeline level in
//! `build/pipeline_tests.rs` (`a_page_that_could_not_be_read_no_longer_withholds_the_rest_of_the_site`
//! and its siblings), and the path list itself in `build/cloud_ledger_tests.rs`.

use super::{refuse_publish, stale_source_refusal_text};

/// The records are process-global, so every case keys on its own folder —
/// otherwise two tests running in parallel share one verdict.
fn record(folder: &str, stale: Vec<String>) {
    crate::system::build_records::records().record_stale_sources(folder, stale);
}

/// Distinct wording from both other refusals: the source is not broken and
/// nothing is mid-encode — moss is showing a real page, just an old one.
#[test]
fn the_refusal_names_the_source() {
    let one = stale_source_refusal_text(&["keeper.md".to_string()]);
    assert!(
        one.starts_with("Nothing published — keeper.md could not be read"),
        "{one}"
    );
    assert!(one.contains("Publish again once it's readable"), "{one}");

    let many = stale_source_refusal_text(&["a.md".to_string(), "config.toml".to_string()]);
    assert!(many.contains("2 sources could not be read"), "{many}");
    assert!(many.contains("a.md, config.toml"), "{many}");
}

/// A clean seal — nothing carried forward — publishes.
#[test]
fn an_empty_list_does_not_block() {
    record("/stale-empty", Vec::new());
    assert!(refuse_publish("/stale-empty").is_ok());
}

/// A folder nothing sealed in this process publishes too — absent is not a
/// verdict, same distinction `missing_media` draws.
#[test]
fn no_seal_in_this_process_does_not_block() {
    assert!(refuse_publish("/stale-never-sealed").is_ok());
}

/// The shape the whole fix exists for: a page (or config.toml, or the
/// stylesheet) this build had to carry forward.
#[test]
fn a_stale_source_blocks_the_publish() {
    record("/stale-one", vec!["keeper.md".to_string()]);
    let err = refuse_publish("/stale-one").expect_err("a stale source must stop the publish");
    assert!(err.contains("keeper.md"), "{err}");
}

/// Verdicts are per folder — another site's stale page must not block this one.
#[test]
fn the_verdict_is_keyed_by_folder() {
    record("/stale-keyed-other", vec!["a.md".to_string()]);
    record("/stale-keyed-site", Vec::new());
    assert!(refuse_publish("/stale-keyed-site").is_ok());
    assert!(refuse_publish("/stale-keyed-other").is_err());
}

/// The follow-up rebuild's clean seal clears the earlier refusal — there is
/// no override, so a page that stays offline forever would otherwise leave the
/// author with no way out other than deleting it.
#[test]
fn a_rebuild_that_can_read_everything_clears_an_earlier_refusal() {
    record("/stale-clears", vec!["keeper.md".to_string()]);
    assert!(refuse_publish("/stale-clears").is_err());

    record("/stale-clears", Vec::new());
    assert!(
        refuse_publish("/stale-clears").is_ok(),
        "a clean rebuild must replace the previous verdict, not merge with it"
    );
}

/// The three rules are independent, and any one alone is enough to refuse —
/// a folder cleared of missing media and in-flight promises but still
/// carrying a stale source must still be refused.
#[test]
fn any_rule_alone_is_enough_to_refuse() {
    crate::system::build_records::records().record_missing_media("/stale-either", Vec::new());
    crate::system::build_records::records().record_promised_dead_links("/stale-either", Vec::new());
    record("/stale-either", vec!["config.toml".to_string()]);
    let err = refuse_publish("/stale-either").expect_err("the stale-source rule alone must refuse");
    assert!(err.contains("config.toml"), "{err}");
}
