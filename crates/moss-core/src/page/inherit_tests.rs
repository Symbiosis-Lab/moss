use super::*;

#[test]
fn cascade_runs_deepest_folder_first_and_skips_its_source() {
    let pages = [
        ("index.html", true),
        ("notes/index.html", true),
        ("notes/a.html", false),
        ("about.html", true),
        ("notes/deep/index.html", false),
    ];
    let plan = cascade_plan(pages);
    assert_eq!(
        plan,
        vec![
            CascadeStep { source: 1, from: "/notes/".into(), targets: vec![2, 4] },
            CascadeStep { source: 0, from: "/".into(), targets: vec![1, 2, 3, 4] },
        ]
    );
}

#[test]
fn a_page_ending_in_index_is_not_a_folder_home() {
    assert!(cascade_plan([("reindex.html", true), ("a.html", false)]).is_empty());
}

#[test]
fn hidden_home_is_the_nearest_hiding_folder_and_not_its_own_home() {
    let pages = [
        ("drafts/index.html", true),
        ("drafts/a.html", false),
        ("drafts/old/index.html", true),
        ("drafts/old/b.html", false),
        ("c.html", false),
    ];
    assert_eq!(
        hidden_homes(pages),
        vec![None, Some("drafts/".into()), Some("drafts/".into()), Some("drafts/old/".into()), None]
    );
}
