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
    RemovedAddress { path: path.into(), reason, moved_to: None, source: None, withdrawn_by: None }
}

fn moved(path: &str, to: &str, source: &str) -> RemovedAddress {
    RemovedAddress {
        path: path.into(),
        reason: RemovalReason::Unexplained,
        moved_to: Some(to.into()),
        source: Some(source.into()),
        withdrawn_by: None,
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
         Either keep the address working (for a moved page or file, add the redirect shown), or accept losing it and publish with `moss deploy --accept-removals`."
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
    assert!(text.contains("  and 3 more; `moss deploy --dry-run --json` lists every one\n"), "{text}");
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
        withdrawn_by: None,
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
            RemovedAddress { path: "legacy-feed.xml".into(), reason: RemovalReason::Unexplained, moved_to: None, source: None, withdrawn_by: None },
            RemovedAddress {
                path: "old/index.html".into(),
                reason: RemovalReason::Unexplained,
                moved_to: Some("new/index.html".into()),
                source: Some("moved.md".into()),
                withdrawn_by: None,
            },
            RemovedAddress {
                path: "files/a.pdf".into(),
                reason: RemovalReason::Unexplained,
                moved_to: None,
                source: Some("files/a.pdf".into()),
                withdrawn_by: None,
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

/// Moved files under one folder, its subfolders included, are one line: the
/// folder redirect that keeps them all. A file moved alone keeps its own line.
#[test]
fn files_moved_with_their_folder_print_one_folder_line() {
    let text = refusal_text(&[
        moved("review/old/index.html", "review/client/old/index.html", "review/old/index.html"),
        moved("review/old/plan.pdf", "review/client/old/plan.pdf", "review/old/plan.pdf"),
        moved("review/old/notes/a.png", "review/client/old/notes/a.png", "review/old/notes/a.png"),
        moved("review/old/notes/b.png", "review/client/old/notes/b.png", "review/old/notes/b.png"),
        moved("files/x.pdf", "archive/x.pdf", "files/x.pdf"),
    ]);
    assert!(text.starts_with("Nothing published — 5 addresses"), "{text}");
    assert!(
        text.contains("  4 addresses under /review/old/ now live under /review/client/old/. To keep them working, add \"/review/old/\" = \"/review/client/old/\" under [redirects]"),
        "{text}"
    );
    assert!(text.contains("add \"/files/x.pdf\" = \"/archive/x.pdf\""), "{text}");
    assert_eq!(text.matches("under [redirects]").count(), 2, "{text}");
}

/// Files moved out of the site root share no folder line: one would name the
/// root, which a folder line cannot.
#[test]
fn files_moved_out_of_the_root_keep_their_own_lines() {
    let text = refusal_text(&[moved("a.pdf", "files/a.pdf", "a.pdf"), moved("b.pdf", "files/b.pdf", "b.pdf")]);
    assert!(!text.contains("\"/\" ="), "{text}");
    assert_eq!(text.matches("under [redirects]").count(), 2, "{text}");
}

/// Files moved up into the folder that held their folder share no folder
/// line either: it would move a folder into one that contains it.
#[test]
fn files_moved_up_into_the_surrounding_folder_keep_their_own_lines() {
    let text = refusal_text(&[moved("docs/x/a.pdf", "docs/a.pdf", "docs/x/a.pdf"), moved("docs/x/b.pdf", "docs/b.pdf", "docs/x/b.pdf")]);
    assert!(!text.contains("\"/docs/x/\" = \"/docs/\""), "{text}");
    assert_eq!(text.matches("under [redirects]").count(), 2, "{text}");
}

/// The lines that keep every moved address working, as they go in the config:
/// one per folder moved together, one per other move, nothing for an address
/// that did not move.
#[test]
fn redirect_lines_are_the_config_lines_for_every_move() {
    let removed = [
        moved("review/old/a.pdf", "review/new/a.pdf", "review/old/a.pdf"),
        moved("review/old/b.pdf", "review/new/b.pdf", "review/old/b.pdf"),
        moved("files/x.pdf", "archive/x.pdf", "files/x.pdf"),
        address("feed.xml", RemovalReason::Unexplained),
    ];
    let lines = super::removal_gate::redirect_lines(&pending_removals(&removed, None));
    assert_eq!(lines, [r#""/review/old/" = "/review/new/""#, r#""/files/x.pdf" = "/archive/x.pdf""#]);
}

/// A file name with a quote still gives a line TOML reads back as written.
#[test]
fn a_redirect_line_quotes_an_address_toml_can_read_back() {
    let line = super::removal_gate::redirect_line("/say \"hi\".pdf", "/new.pdf");
    let table: toml::Table = toml::from_str(&line).unwrap();
    assert_eq!(table["/say \"hi\".pdf"].as_str(), Some("/new.pdf"));
}

fn source_version(hash: &str, mtime: Option<(u64, u32)>) -> crate::build::manifest::SealedManifest {
    use crate::build::manifest::{HashBucket, PendingManifest};
    use crate::build::served_path::ServedPath;
    let mut pending = PendingManifest::new(Default::default());
    let output = ServedPath::from_source("note/index.html").unwrap();
    pending.register(&output, hash.as_bytes(), HashBucket::Files);
    pending.register_source_mapping("note.md".into(), &output);
    pending.register_page_source_hash("note.md".into(), crate::build::types::SourceMetadata {
        hash: hash.into(),
        stat: crate::build::stat::FileStat {
            mtime_nanos: mtime.map(|t| t.1),
            ..crate::build::stat::FileStat::whole_second(1, mtime.map_or(0, |t| t.0))
        },
    });
    pending.seal()
}

fn publish_version(folder: &std::path::Path, hash: &str, mtime: Option<(u64, u32)>, day: u32) {
    let sealed = source_version(hash, mtime);
    crate::deploy::history::HistoryStore::in_vault(folder)
        .snapshot(folder, &crate::deploy::history::tree::SourceTree::from_sealed(&sealed), "moss:example", &format!("2026-01-{day:02}T00:00:00Z")).unwrap();
}

fn check_version(folder: &std::path::Path, hash: &str, mtime: Option<(u64, u32)>) -> Result<(), String> {
    super::removal_gate::refuse_stale_generation(folder, &source_version(hash, mtime))
}

fn accept_version(folder: &std::path::Path, hash: &str, mtime: Option<(u64, u32)>) {
    let stale = super::removal_gate::stale_content(folder, &source_version(hash, mtime));
    records().accept_stale_content(folder.to_str().unwrap(), stale);
}

#[test]
fn stale_content_uses_source_time_not_publish_time_and_round_trips_it() {
    let dir = tempfile::tempdir().unwrap();
    publish_version(dir.path(), "latest", Some((100, 20)), 1);
    let history = crate::deploy::history::HistoryStore::in_vault(dir.path());
    assert_eq!(history.list_records()[0].1.entries["note.md"].source_mtime, Some((100, 20)));
    let error = check_version(dir.path(), "restored", Some((100, 10))).unwrap_err();
    assert!(error.contains("note.md") && error.contains("--accept-stale-content"), "{error}");
    assert!(check_version(dir.path(), "latest", Some((1, 0))).is_ok(), "unchanged bytes pass");
    assert!(check_version(dir.path(), "new edit", Some((101, 0))).is_ok(), "an edit can predate publish time");
    assert!(check_version(dir.path(), "clone", Some((200, 0))).is_ok(), "checkout timestamps pass");
    assert!(check_version(dir.path(), "unknown time", None).is_ok());
}

#[test]
fn stale_content_old_published_hash_needs_acceptance_even_with_a_fresh_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    publish_version(dir.path(), "old", Some((100, 0)), 1);
    publish_version(dir.path(), "latest", Some((200, 0)), 2);
    let error = check_version(dir.path(), "old", Some((300, 0))).unwrap_err();
    assert!(error.contains("deliberate revert"), "{error}");
    accept_version(dir.path(), "old", Some((300, 0)));
    assert!(check_version(dir.path(), "old", Some((300, 0))).is_ok());
    assert!(check_version(dir.path(), "other rollback", Some((50, 0))).is_err(), "acceptance names content");
}

#[test]
fn stale_content_first_publish_new_pages_and_legacy_records_have_no_timestamp_opinion() {
    let dir = tempfile::tempdir().unwrap();
    assert!(check_version(dir.path(), "first", Some((1, 0))).is_ok());
    publish_version(dir.path(), "latest", None, 1);
    assert!(check_version(dir.path(), "edited", Some((1, 0))).is_ok());
    let legacy: crate::deploy::history::Entry = serde_json::from_str(r#"{"mode":"100644","hash":"old"}"#).unwrap();
    assert_eq!(legacy.source_mtime, None);
    let history = crate::deploy::history::HistoryStore::in_vault(dir.path());
    let (id, mut record) = history.list_records().pop().unwrap();
    record.entries.clear();
    std::fs::write(history.store_dir().join("publishes").join(format!("{id}.json")), serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(check_version(dir.path(), "new page", Some((1, 0))).is_ok());
}

#[test]
fn stale_content_ignores_manual_saves_and_other_publish_targets() {
    let dir = tempfile::tempdir().unwrap();
    let history = crate::deploy::history::HistoryStore::in_vault(dir.path());
    history.snapshot_manual(dir.path(), &crate::deploy::history::tree::SourceTree::from_sealed(&source_version("manual", Some((900, 0)))), "moss:example", None).unwrap();
    assert!(check_version(dir.path(), "first", Some((1, 0))).is_ok());
    publish_version(dir.path(), "latest", Some((100, 0)), 1);
    assert!(check_version(dir.path(), "manual", Some((101, 0))).is_ok());
    history.snapshot(dir.path(), &crate::deploy::history::tree::SourceTree::from_sealed(&source_version("other", Some((900, 0)))), "moss:other", "2026-01-02T00:00:00Z").unwrap();
    crate::vault::deployment_state::save_domain_config(dir.path().to_str().unwrap(), &crate::config::deployment::DomainDeploymentConfig {
        site_id: Some("example".into()), ..Default::default()
    }).unwrap();
    assert!(check_version(dir.path(), "other", Some((101, 0))).is_ok());
}

async fn collision_push(plugin: bool) {
    use crate::build::manifest::PendingManifest;
    use crate::deploy::{plugin_push, push};
    let folder = tempfile::tempdir().unwrap();
    let sink = crate::deploy::progress::silent();
    let ports = crate::deploy::one_shot::HeadlessDeployPorts;
    let managers = crate::plugins::manager::ManagerCache::headless(crate::build::ports::reporter::discard_owned());
    let identity = crate::identity::Identity::generate().unwrap();
    let events_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
    let messages = vec![
        "one.md and two.md both publish to /shared/. Give each source a unique address.".into(),
        "`docs.md` and `docs/index.html` both publish to /docs/. Give each source a unique address.".into(),
    ];
    let mut pending = PendingManifest::new(Default::default());
    pending.record_output_collisions(messages.clone());
    let sealed = pending.seal();
    let error = if plugin {
        plugin_push::run_plugin_deploy_inner(&plugin_push::PluginDeployContext {
            folder: folder.path(), sealed: Some(&sealed), managers: &managers, sink: &sink, ports: &ports,
        }).await.unwrap_err()
    } else {
        push::push_site_inner(&sealed, &push::PushContext {
            folder_path: folder.path(), identity: &identity, site_id: "collision-test",
            sink: &sink, ports: &ports, events_lock: &events_lock,
        }).await.unwrap_err().to_string()
    };
    assert_eq!(error, messages.join("\n"));
}

#[tokio::test]
async fn output_collisions_refuse_hosted_push() {
    collision_push(false).await;
}

#[tokio::test]
async fn output_collisions_refuse_plugin_push() {
    collision_push(true).await;
}
