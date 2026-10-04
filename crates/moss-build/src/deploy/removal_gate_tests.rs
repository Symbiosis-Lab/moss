//! The publish gate's fourth rule: a publish that would take addresses offline
//! for a reason other than the author removing them. The records are
//! process-global, so every case keys on its own folder.

use super::refuse_publish;
use super::removal_gate::refusal_text as render_refusal;
use crate::build::manifest::change_set::pending_removals;
use crate::build::served_path::served_address;
use crate::build::manifest::change_set::{PendingRemoval, RemovalCause, RemovalReason, RemovedAddress};
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

/// The refusal text for `removed`, from the pending list the way production
/// builds it.
fn refusal_text(removed: &[RemovedAddress]) -> String {
    render_refusal(&pending_removals(removed, None))
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

    records().accept_all_pending_removals("/removal-accepted");
    assert!(refuse_publish("/removal-accepted").is_ok());

    seal("/removal-accepted", removed);
    assert!(refuse_publish("/removal-accepted").is_ok(), "an identical rebuild must not ask again");
}

#[test]
fn a_different_unexplained_address_after_an_acceptance_refuses_again() {
    seal("/removal-new-one", vec![address("feed.xml", RemovalReason::Unexplained)]);
    records().accept_all_pending_removals("/removal-new-one");

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
    records().accept_all_pending_removals("/removal-fewer");

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

// ── Acceptance: the truth table ──

fn unexplained(path: &str) -> RemovedAddress {
    address(path, RemovalReason::Unexplained)
}

fn paths(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

fn pending_paths(folder: &str) -> Vec<String> {
    records().pending_removals(folder).into_iter().map(|p| p.path).collect()
}

/// nothing removed: nothing pending, the publish passes.
#[test]
fn row_nothing_removed() {
    seal("/row-none", Vec::new());
    assert!(pending_paths("/row-none").is_empty());
    assert!(refuse_publish("/row-none").is_ok());
}

/// unexplained and not accepted: pending, refused.
#[test]
fn row_unexplained_not_accepted() {
    seal("/row-unaccepted", vec![unexplained("a.xml")]);
    assert_eq!(pending_paths("/row-unaccepted"), ["a.xml"]);
    assert!(refuse_publish("/row-unaccepted").is_err());
}

/// accepted: not pending, passes.
#[test]
fn row_accepted() {
    seal("/row-accepted", vec![unexplained("a.xml")]);
    assert!(records().accept_unexplained_removals("/row-accepted", &paths(&["a.xml"])).is_empty());
    assert!(pending_paths("/row-accepted").is_empty());
    assert!(refuse_publish("/row-accepted").is_ok());
}

/// accept A, then a new B appears: refused, naming B only.
#[test]
fn row_accept_a_then_b_appears() {
    seal("/row-a-then-b", vec![unexplained("a.xml")]);
    records().accept_unexplained_removals("/row-a-then-b", &paths(&["a.xml"]));
    seal("/row-a-then-b", vec![unexplained("a.xml"), unexplained("b.xml")]);

    assert_eq!(pending_paths("/row-a-then-b"), ["b.xml"]);
    let err = refuse_publish("/row-a-then-b").unwrap_err();
    assert!(err.contains("/b.xml") && !err.contains("/a.xml"), "{err}");
}

/// accept A, then accept B: both accepted (acceptance adds).
#[test]
fn row_accept_a_then_accept_b() {
    seal("/row-both", vec![unexplained("a.xml"), unexplained("b.xml")]);
    records().accept_unexplained_removals("/row-both", &paths(&["a.xml"]));
    records().accept_unexplained_removals("/row-both", &paths(&["b.xml"]));

    assert!(pending_paths("/row-both").is_empty());
    assert!(refuse_publish("/row-both").is_ok());
}

/// accept A, A is fixed, A goes offline again: asked again.
#[test]
fn row_accepted_then_fixed_then_offline_again_asks_again() {
    seal("/row-again", vec![unexplained("a.xml")]);
    records().accept_unexplained_removals("/row-again", &paths(&["a.xml"]));
    seal("/row-again", Vec::new());
    seal("/row-again", vec![unexplained("a.xml")]);

    assert_eq!(pending_paths("/row-again"), ["a.xml"]);
    assert!(refuse_publish("/row-again").is_err());
}

/// A path that is not a current unexplained removal is ignored, and the call
/// reports it back so a caller can tell.
#[test]
fn row_a_path_that_is_not_a_current_removal_is_ignored_and_reported() {
    seal("/row-ignored", vec![unexplained("a.xml"), address("gone/index.html", RemovalReason::AuthorRemoved)]);
    let ignored = records().accept_unexplained_removals("/row-ignored", &paths(&["nope.xml", "gone/index.html", "a.xml"]));

    assert_eq!(ignored, ["nope.xml", "gone/index.html"]);
    assert!(records().accepted_removals("/row-ignored").iter().eq(["a.xml"].iter()));
    assert!(pending_paths("/row-ignored").is_empty());
}

/// an identical rebuild after acceptance: passes.
#[test]
fn row_identical_rebuild_after_acceptance_passes() {
    seal("/row-identical", vec![unexplained("a.xml")]);
    records().accept_unexplained_removals("/row-identical", &paths(&["a.xml"]));
    seal("/row-identical", vec![unexplained("a.xml")]);
    assert!(refuse_publish("/row-identical").is_ok());
}

/// The race: a caller shows the list, a rebuild adds B before the person's
/// "accept" arrives, and the acceptance covers what was shown, not B.
#[test]
fn accepting_what_was_shown_does_not_accept_what_appeared_after() {
    seal("/race", vec![unexplained("a.xml")]);
    let shown: Vec<String> = pending_paths("/race");
    seal("/race", vec![unexplained("a.xml"), unexplained("b.xml")]);

    records().accept_unexplained_removals("/race", &shown);

    assert_eq!(pending_paths("/race"), ["b.xml"]);
    assert!(refuse_publish("/race").is_err());
}

/// The pending list in served form, with each cause's data.
#[test]
fn pending_removals_carry_the_served_address_and_the_cause() {
    seal(
        "/pending-causes",
        vec![
            RemovedAddress { path: "legacy-feed.xml".into(), reason: RemovalReason::Unexplained, moved_to: None, source: None },
            RemovedAddress {
                path: "old/index.html".into(),
                reason: RemovalReason::Unexplained,
                moved_to: Some("new/index.html".into()),
                source: Some("moved.md".into()),
            },
            RemovedAddress {
                path: "files/a.pdf".into(),
                reason: RemovalReason::Unexplained,
                moved_to: None,
                source: Some("files/a.pdf".into()),
            },
        ],
    );
    let pending = records().pending_removals("/pending-causes");
    let by_path = |p: &str| pending.iter().find(|r| r.path == p).unwrap().clone();

    assert_eq!(
        by_path("legacy-feed.xml"),
        PendingRemoval { path: "legacy-feed.xml".into(), address: "/legacy-feed.xml".into(), cause: RemovalCause::Generated }
    );
    assert_eq!(by_path("old/index.html").address, "/old/");
    assert_eq!(by_path("old/index.html").cause, RemovalCause::Moved { to: "/new/".into() });
    assert_eq!(by_path("files/a.pdf").cause, RemovalCause::StillInFolder { source: "files/a.pdf".into() });
}

#[test]
fn each_cause_serializes_with_a_kind_tag() {
    assert_eq!(serde_json::to_value(RemovalCause::Generated).unwrap(), serde_json::json!({"kind": "generated"}));
    assert_eq!(
        serde_json::to_value(RemovalCause::Moved { to: "/new/".into() }).unwrap(),
        serde_json::json!({"kind": "moved", "to": "/new/"})
    );
    assert_eq!(
        serde_json::to_value(RemovalCause::StillInFolder { source: "a.pdf".into() }).unwrap(),
        serde_json::json!({"kind": "still_in_folder", "source": "a.pdf"})
    );
}

/// Two threads, one accepting and one sealing, with the invariant checked under
/// the records' own lock after every seal: nothing accepted that the last seal
/// does not report as an unexplained removal. An acceptance that read the old
/// build's list, then wrote after a seal had pruned, would leave such a path;
/// with the removals and the accepted set under one lock that cannot happen.
/// Without the fix the check fails within a few thousand rounds (the
/// accept/seal interleaving is the scheduler's, so this is a loop, not a
/// proof).
#[test]
fn an_acceptance_cannot_straddle_a_seal() {
    let folder = "/race-threads";
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let accepter = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                records().accept_unexplained_removals(folder, &paths(&["a.xml"]));
            }
        })
    };
    for round in 0..20_000 {
        // Alternate a build that loses `a.xml` with one that has it back.
        seal(folder, if round % 2 == 0 { Vec::new() } else { vec![unexplained("a.xml")] });
        let stray = records().accepted_but_not_removed(folder);
        assert!(stray.is_empty(), "round {round}: accepted but no longer a removal: {stray:?}");
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    accepter.join().unwrap();
}
