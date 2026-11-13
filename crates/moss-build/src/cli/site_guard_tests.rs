use super::*;
use crate::nested_roots::{CopyEvidence, MossCopy};

fn report(nested: Vec<NestedRootInfo>, truncated: bool) -> NestedRootsReport {
    NestedRootsReport { root: "/scan".into(), root_class: RootClass::Normal, nested, truncated, dirs_visited: 4000, moss_copies: Vec::new(), root_listing_complete: true }
}

fn site(published: Option<bool>) -> NestedRootInfo {
    NestedRootInfo { rel_path: "draft".into(), abs_path: "/scan/draft".into(), name: "draft".into(), published, site_id: None, data_in: None }
}

#[test]
fn an_incomplete_root_listing_never_offers_to_make_a_site() {
    for nested in [vec![], vec![site(Some(false))]] {
        let mut r = report(nested, true);
        r.root_listing_complete = false;
        assert_eq!(plan_unowned_open(&r, Listing::Settled), UnownedPlan::Confirm { offer_create_anyway: false });
    }
}

#[test]
fn a_scan_cut_short_below_a_complete_root_listing_still_offers_it() {
    assert_eq!(plan_unowned_open(&report(vec![], true), Listing::Settled), UnownedPlan::Confirm { offer_create_anyway: true });
    assert_eq!(plan_unowned_open(&report(vec![site(Some(false))], true), Listing::Settled), UnownedPlan::Confirm { offer_create_anyway: false });
}

/// A large, deep folder whose own listing was complete can still become
/// a site after its scan hits the cap.
#[test]
fn a_large_folder_with_a_complete_root_listing_is_offered_set_up() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..40 {
        std::fs::create_dir_all(tmp.path().join(format!("notes-{i}/deeper"))).unwrap();
    }
    let limits = ScanLimits { max_depth: 6, max_dirs: 20, max_reported: 5 };
    let report = find_nested_roots(tmp.path(), None, &limits);
    assert!(report.truncated && report.root_listing_complete);
    assert_eq!(plan_unowned_open(&report, Listing::Settled), UnownedPlan::Confirm { offer_create_anyway: true });
}

#[test]
fn a_complete_scan_with_one_unpublished_site_still_offers_it() {
    assert_eq!(plan_unowned_open(&report(vec![site(Some(false))], false), Listing::Settled), UnownedPlan::Confirm { offer_create_anyway: true });
    assert_eq!(plan_unowned_open(&report(vec![site(Some(true))], false), Listing::Settled), UnownedPlan::Confirm { offer_create_anyway: false });
    assert_eq!(plan_unowned_open(&report(vec![], false), Listing::Settled), UnownedPlan::Proceed);
}

/// A site whose `.moss` was set aside, with the set-aside's cache big
/// enough to exhaust the scan. Skipping the copy alone would read as a
/// clean folder and mint a new `.moss`.
#[test]
fn a_site_whose_moss_was_set_aside_is_never_proceeded_on() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..129 {
        std::fs::create_dir_all(tmp.path().join(format!("page-{i}"))).unwrap();
    }
    let copy = tmp.path().join(".moss.removed-20261030");
    for i in 0..300 {
        std::fs::create_dir_all(copy.join(format!("cache/objects/{i:03x}"))).unwrap();
    }
    std::fs::write(copy.join("state.toml"), "[deployment]\nsite_id = \"real-site\"\n").unwrap();

    let limits = ScanLimits { max_depth: 6, max_dirs: 200, max_reported: 5 };
    let report = find_nested_roots(tmp.path(), None, &limits);
    assert!(!report.truncated);
    assert_eq!(plan_unowned_open(&report, Listing::Settled), UnownedPlan::HealCopy { copy: ".moss.removed-20261030".into() });
    let folder = tmp.path().to_string_lossy();
    assert!(cli_refusal_message(&report, Listing::Settled, &folder, "history")
        .contains(&format!("mv \"{folder}/.moss.removed-20261030\" \"{folder}/.moss\"")));
    assert!(!tmp.path().join(".moss").exists(), "planning writes nothing");
}

#[test]
fn several_copies_are_refused() {
    let mut r = report(vec![], false);
    r.moss_copies = [".moss 2", ".moss (1)", ".moss.removed-20260101"]
        .map(|name| MossCopy { name: name.into(), evidence: if name.contains("removed") { CopyEvidence::Unpublished } else { CopyEvidence::Listed } })
        .to_vec();
    assert_eq!(
        plan_unowned_open(&r, Listing::Settled),
        UnownedPlan::RefuseCopies { copies: vec![".moss 2".into(), ".moss (1)".into()] },
        "an unpublished set-aside is not site data"
    );
    assert!(cli_refusal_message(&r, Listing::Settled, "/scan", "build").contains("'.moss 2', '.moss (1)'"));
}

fn copy(name: &str, evidence: CopyEvidence) -> MossCopy {
    MossCopy { name: name.into(), evidence }
}

fn with_copy(c: MossCopy) -> NestedRootsReport {
    let mut r = report(vec![], false);
    r.moss_copies = vec![c];
    r
}

#[test]
fn one_copy_is_healed_only_when_nothing_else_can_be_meant() {
    let heal = |c: MossCopy| UnownedPlan::HealCopy { copy: c.name.clone() };
    for c in [copy(".moss 2", CopyEvidence::Listed), copy(".moss.removed-20261030", CopyEvidence::Published)] {
        assert_eq!(plan_unowned_open(&with_copy(c.clone()), Listing::Settled), heal(c));
    }
    let wait = |name: &str| UnownedPlan::AwaitCopy { copy: name.into() };
    assert_eq!(plan_unowned_open(&with_copy(copy(".moss 2", CopyEvidence::Listed)), Listing::Settling), wait(".moss 2"), "a .moss may be arriving");
    assert_eq!(plan_unowned_open(&with_copy(copy(".moss 2", CopyEvidence::Arriving)), Listing::Settled), wait(".moss 2"), "the copy has not arrived");
    assert_eq!(plan_unowned_open(&with_copy(copy(".moss.removed-20261030", CopyEvidence::Arriving)), Listing::Settled), wait(".moss.removed-20261030"), "a set-aside whose records could not all be read");
    let mut r = with_copy(copy(".moss 2", CopyEvidence::Listed));
    r.root_listing_complete = false;
    assert_eq!(plan_unowned_open(&r, Listing::Settled), wait(".moss 2"), "another copy may sit unseen");
    assert_eq!(plan_unowned_open(&with_copy(copy(".moss 2", CopyEvidence::Arriving)), Listing::Overdue), UnownedPlan::AskCopy { copy: ".moss 2".into() }, "after the wait, ask");
}

#[test]
fn a_copy_still_arriving_leaves_the_command_line_a_way_out() {
    let msg = cli_refusal_message(&with_copy(copy(".moss 2", CopyEvidence::Arriving)), Listing::Settled, "/scan", "build");
    assert!(msg.contains("still arriving"), "msg: {msg}");
    assert!(msg.contains(r#"mv "/scan/.moss 2" "/scan/.moss""#), "msg: {msg}");
}

fn set_aside_with_a_publish(root: &Path) -> std::path::PathBuf {
    let copy = root.join(".moss.removed-20261030");
    std::fs::create_dir_all(&copy).unwrap();
    std::fs::write(copy.join("state.toml"), "[deployment]\nsite_id = \"a-site\"\n").unwrap();
    copy.join("state.toml")
}

#[test]
fn the_command_line_requests_a_copy_it_cannot_decide_and_renames_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let state = set_aside_with_a_publish(tmp.path());
    let _cloud = crate::build::icloud::pretend::evicted(&state);
    let mut requested = Vec::new();
    let refusal = guard_cli_open_with(&tmp.path().to_string_lossy(), "build", None, |dir| requested.push(dir.to_path_buf())).unwrap_err();
    assert!(refusal.contains("still arriving"), "{refusal}");
    assert_eq!(requested, [state.parent().unwrap().to_path_buf()]);
    assert!(state.parent().unwrap().is_dir() && !tmp.path().join(".moss").exists());
}

#[test]
fn evidence_that_lands_when_asked_is_acted_on_in_the_same_open() {
    let tmp = tempfile::tempdir().unwrap();
    let state = set_aside_with_a_publish(tmp.path());
    let _cloud = crate::build::icloud::pretend::evicted_until_requested(&state);
    guard_cli_open_in(&tmp.path().to_string_lossy(), "build", None).unwrap();
    assert!(tmp.path().join(".moss/state.toml").is_file());
    assert!(!state.parent().unwrap().exists());
}

#[test]
fn a_copy_here_but_unreadable_is_asked_about_once_the_folder_has_listed() {
    let name = ".moss.removed-20261030";
    let unreadable = with_copy(copy(name, CopyEvidence::Unreadable));
    assert_eq!(plan_unowned_open(&unreadable, Listing::Settled), UnownedPlan::AskCopy { copy: name.into() });
    assert_eq!(plan_unowned_open(&unreadable, Listing::Settling), UnownedPlan::AwaitCopy { copy: name.into() }, "a .moss may be arriving");
    assert_eq!(plan_unowned_open(&unreadable, Listing::Overdue), UnownedPlan::AskCopy { copy: name.into() });
}

#[test]
fn a_quiet_folder_still_listing_waits_with_no_copy_in_sight() {
    let empty = report(vec![], false);
    assert_eq!(plan_unowned_open(&empty, Listing::Settling), UnownedPlan::AwaitFolder);
    assert_eq!(plan_unowned_open(&empty, Listing::Overdue), UnownedPlan::Confirm { offer_create_anyway: true }, "only the person can know");
    assert_eq!(plan_unowned_open(&empty, Listing::Settled), UnownedPlan::Proceed);
    let mut unseen = report(vec![], false);
    unseen.root_listing_complete = false;
    assert_eq!(plan_unowned_open(&unseen, Listing::Settled), UnownedPlan::AwaitFolder);
    assert!(cli_refusal_message(&empty, Listing::Settling, "/scan", "build").contains("still arriving"));
}

#[test]
fn deciding_a_copy_requests_everything_the_plan_reads() {
    let (dirs, files) = copy_evidence(Path::new("/c"));
    for dir in ["/c", "/c/identity", "/c/deploy", "/c/deploy/records", "/c/history", "/c/history/publishes", "/c/keys"] {
        assert!(dirs.contains(&Path::new(dir).to_path_buf()), "{dir} in {dirs:?}");
    }
    assert_eq!(files, [Path::new("/c/state.toml").to_path_buf(), Path::new("/c/identity/public.json").to_path_buf()]);
}

/// moss sets a `.moss` aside over nested sites, and a home or drive root is
/// questioned on its own: there the copy is left alone, and set-up is never
/// offered beside it.
#[test]
fn a_copy_in_a_folder_that_is_questioned_anyway_is_left_alone() {
    let published = copy(".moss.removed-20261030", CopyEvidence::Published);
    let mut nested = with_copy(published.clone());
    nested.nested = vec![site(Some(false))];
    let mut home = with_copy(published.clone());
    home.root_class = RootClass::HomeDir;
    let mut cut_short = with_copy(published);
    cut_short.truncated = true;
    for r in [nested, home, cut_short] {
        assert_eq!(plan_unowned_open(&r, Listing::Settled), UnownedPlan::Confirm { offer_create_anyway: false }, "{r:?}");
    }
}

#[test]
fn the_command_line_puts_the_copy_back_and_a_reading_command_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".moss 2/cache")).unwrap();
    std::fs::create_dir_all(tmp.path().join("pages")).unwrap();
    let folder = tmp.path().to_string_lossy();
    let refusal = guard_cli_open_in(&folder, "history", None).unwrap_err();
    assert!(refusal.contains("mv "), "{refusal}");
    assert!(!tmp.path().join(".moss").exists());
    guard_cli_open_in(&folder, "build", None).unwrap();
    assert!(tmp.path().join(".moss/cache").is_dir());
    assert!(!tmp.path().join(".moss 2").exists());
}

#[test]
fn restore_never_replaces_a_moss_that_arrived() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".moss 2/cache")).unwrap();
    std::fs::create_dir(tmp.path().join(".moss")).unwrap();
    assert!(restore_moss_copy(tmp.path(), ".moss 2").is_err());
    assert!(tmp.path().join(".moss 2/cache").is_dir());
    assert!(restore_moss_copy(tmp.path(), "pages").is_err(), "not a copy");
}
