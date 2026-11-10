use super::*;
use crate::nested_roots::MossCopy;

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
        assert_eq!(plan_unowned_open(&r), UnownedPlan::Confirm { offer_create_anyway: false });
    }
}

#[test]
fn a_scan_cut_short_below_a_complete_root_listing_still_offers_it() {
    assert_eq!(plan_unowned_open(&report(vec![], true)), UnownedPlan::Confirm { offer_create_anyway: true });
    assert_eq!(plan_unowned_open(&report(vec![site(Some(false))], true)), UnownedPlan::Confirm { offer_create_anyway: false });
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
    assert_eq!(plan_unowned_open(&report), UnownedPlan::Confirm { offer_create_anyway: true });
}

#[test]
fn a_complete_scan_with_one_unpublished_site_still_offers_it() {
    assert_eq!(plan_unowned_open(&report(vec![site(Some(false))], false)), UnownedPlan::Confirm { offer_create_anyway: true });
    assert_eq!(plan_unowned_open(&report(vec![site(Some(true))], false)), UnownedPlan::Confirm { offer_create_anyway: false });
    assert_eq!(plan_unowned_open(&report(vec![], false)), UnownedPlan::Proceed);
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
    assert_eq!(plan_unowned_open(&report), UnownedPlan::RestoreCopy { copy: ".moss.removed-20261030".into() });
    let folder = tmp.path().to_string_lossy();
    assert!(cli_refusal_message(&report, &folder, "build")
        .contains(&format!("mv \"{folder}/.moss.removed-20261030\" \"{folder}/.moss\"")));
    assert!(!tmp.path().join(".moss").exists(), "planning writes nothing");
}

#[test]
fn several_copies_are_refused() {
    let mut r = report(vec![], false);
    r.moss_copies = [".moss 2", ".moss (1)", ".moss.removed-20260101"]
        .map(|name| MossCopy { name: name.into(), published: Some(false) })
        .to_vec();
    assert_eq!(
        plan_unowned_open(&r),
        UnownedPlan::RefuseCopies { copies: vec![".moss 2".into(), ".moss (1)".into()] },
        "an unpublished set-aside is not site data"
    );
    assert!(cli_refusal_message(&r, "/scan", "build").contains("'.moss 2', '.moss (1)'"));
}
