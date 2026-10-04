//! The publish gate's fourth rule: a publish that would take addresses offline
//! for a reason other than the author removing them. The records are
//! process-global, so every case keys on its own folder.

use super::refuse_publish;
use super::removal_gate::refusal_text;
use crate::build::served_path::served_address;
use crate::build::manifest::change_set::{RemovalReason, RemovedAddress};
use crate::system::build_records::records;

fn address(path: &str, reason: RemovalReason) -> RemovedAddress {
    RemovedAddress { path: path.into(), reason, moved_to: None, source: None }
}

fn moved(path: &str, to: &str, source: &str) -> RemovedAddress {
    RemovedAddress {
        path: path.into(),
        reason: RemovalReason::Unexplained,
        moved_to: Some(to.into()),
        source: Some(source.into()),
    }
}

fn seal(folder: &str, removed: Vec<RemovedAddress>) {
    records().record_removed_addresses(folder, removed);
}

#[test]
fn an_unexplained_removal_refuses_the_publish() {
    seal("/removal-refuses", vec![address("feed.xml", RemovalReason::Unexplained)]);
    let err = refuse_publish("/removal-refuses").expect_err("an unexplained removal must stop the publish");
    assert!(err.contains("/feed.xml"), "{err}");
}

#[test]
fn accepting_that_set_lets_the_publish_through_and_a_rebuild_of_it_does_not_ask_again() {
    let removed = vec![address("feed.xml", RemovalReason::Unexplained)];
    seal("/removal-accepted", removed.clone());
    assert!(refuse_publish("/removal-accepted").is_err());

    records().accept_unexplained_removals("/removal-accepted");
    assert!(refuse_publish("/removal-accepted").is_ok());

    seal("/removal-accepted", removed);
    assert!(refuse_publish("/removal-accepted").is_ok(), "an identical rebuild must not ask again");
}

#[test]
fn a_different_unexplained_address_after_an_acceptance_refuses_again() {
    seal("/removal-new-one", vec![address("feed.xml", RemovalReason::Unexplained)]);
    records().accept_unexplained_removals("/removal-new-one");

    seal(
        "/removal-new-one",
        vec![address("feed.xml", RemovalReason::Unexplained), address("a/index.html", RemovalReason::Unexplained)],
    );
    let err = refuse_publish("/removal-new-one").expect_err("a new address needs its own acceptance");
    assert!(err.contains("/a/") && !err.contains("/feed.xml"), "only the new address is asked about: {err}");
}

/// The acceptance covers the addresses it named, not a count: fewer is still
/// inside it.
#[test]
fn an_accepted_set_with_one_address_fewer_still_passes() {
    seal(
        "/removal-fewer",
        vec![address("a.xml", RemovalReason::Unexplained), address("b.xml", RemovalReason::Unexplained)],
    );
    records().accept_unexplained_removals("/removal-fewer");

    seal("/removal-fewer", vec![address("b.xml", RemovalReason::Unexplained)]);
    assert!(refuse_publish("/removal-fewer").is_ok());
}

#[test]
fn removals_the_author_made_never_refuse() {
    seal("/removal-author", vec![address("gone/index.html", RemovalReason::AuthorRemoved)]);
    assert!(refuse_publish("/removal-author").is_ok());
}

#[test]
fn a_rebuild_that_restores_the_address_clears_the_refusal() {
    seal("/removal-restored", vec![address("feed.xml", RemovalReason::Unexplained)]);
    assert!(refuse_publish("/removal-restored").is_err());
    seal("/removal-restored", Vec::new());
    assert!(refuse_publish("/removal-restored").is_ok());
}

/// A moved page: the line the author can paste, and both ways out, in text a
/// caller with no accept button shows as it stands.
#[test]
fn the_refusal_for_a_moved_page_names_the_redirect_and_both_ways_out() {
    let text = refusal_text(&[moved("old/index.html", "new/index.html", "old.md")]);
    assert_eq!(
        text,
        "Nothing published — 1 address your site has served would stop working, and you did not remove it:\n\
         \x20 /old/ now lives at /new/. To keep the old link working, add \"/old/\" = \"/new/\" under [redirects] in .moss/config.toml.\n\
         Either keep the address working (for a moved page, add the redirect shown), or accept losing it and publish with `moss deploy --accept-removals`."
    );
}

#[test]
fn the_refusal_for_a_generated_file_says_it_is_no_longer_produced() {
    let text = refusal_text(&[address("feed.xml", RemovalReason::Unexplained)]);
    assert!(text.contains("  /feed.xml is no longer produced by your site.\n"), "{text}");
    assert!(text.contains("--accept-removals"), "{text}");
}

#[test]
fn the_refusal_for_a_file_still_in_the_folder_names_it() {
    let text = refusal_text(&[RemovedAddress {
        source: Some("files/a.pdf".into()),
        ..address("files/a.pdf", RemovalReason::Unexplained)
    }]);
    assert!(text.contains("  /files/a.pdf is still in your folder (files/a.pdf) but is no longer published.\n"), "{text}");
}

#[test]
fn the_list_is_capped_at_twenty_with_the_rest_counted() {
    let many: Vec<RemovedAddress> =
        (0..23).map(|i| address(&format!("f{i:02}.xml"), RemovalReason::Unexplained)).collect();
    let text = refusal_text(&many);
    assert!(text.contains("23 addresses"), "{text}");
    assert!(text.contains("/f19.xml") && !text.contains("/f20.xml"), "{text}");
    assert!(text.contains("  and 3 more\n"), "{text}");
}

/// Only an `index.html` is served at its directory. Every other output keeps
/// its full path, so the address printed (and the redirect suggested) is the
/// one the redirect table matches.
#[test]
fn only_an_index_page_prints_as_its_directory() {
    assert_eq!(served_address("index.html"), "/");
    assert_eq!(served_address("a/index.html"), "/a/");
    assert_eq!(served_address("a/b/index.html"), "/a/b/");
    assert_eq!(served_address("scale-compare.html"), "/scale-compare.html");
    assert_eq!(served_address("notes/x.html"), "/notes/x.html");
    assert_eq!(served_address("LICENSE"), "/LICENSE");
    assert_eq!(served_address("feed.xml"), "/feed.xml");
}

#[test]
fn a_moved_hand_made_page_suggests_the_redirect_the_table_matches() {
    let text = refusal_text(&[RemovedAddress {
        path: "scale-compare.html".into(),
        reason: RemovalReason::Unexplained,
        moved_to: Some("compare/index.html".into()),
        source: Some("scale-compare.html".into()),
    }]);
    assert!(text.contains("add \"/scale-compare.html\" = \"/compare/\" under [redirects]"), "{text}");
}
