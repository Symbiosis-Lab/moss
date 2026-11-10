use super::*;

#[test]
fn anchor_target_resolves_heading_text_from_the_current_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("about.md"),
        "# Intro\n\nSome text.\n\n## Facilities\n\nWheelchair access.\n",
    )
    .unwrap();
    let info = reference_card_for("#facilities", "about.md", tmp.path()).unwrap();
    assert_eq!(info.heading.as_deref(), Some("Facilities"));
    assert_eq!(info.source_path.as_deref(), Some("about.md"));
    assert!(!info.generated);
}

#[test]
fn anchor_target_with_no_matching_heading_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("about.md"), "# Intro\n").unwrap();
    let info = reference_card_for("#nope", "about.md", tmp.path()).unwrap();
    assert_eq!(info.heading, None);
}

#[test]
fn a_page_link_on_a_site_never_built_opens_the_source_the_lint_follows() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("notes")).unwrap();
    std::fs::write(tmp.path().join("index.md"), "[[Page]]\n").unwrap();
    std::fs::write(tmp.path().join("notes/Page.md"), "---\ntitle: The page\n---\nBody.\n").unwrap();
    let info = reference_card_for("Page", "index.md", tmp.path()).unwrap();
    assert_eq!(info.source_path.as_deref(), Some("notes/Page.md"));
    assert_eq!(info.title, "The page");
    assert!(!info.generated);
}

#[test]
fn a_page_named_decomposed_on_disk_opens_under_its_composed_path() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("index.md"), "[[Caf\u{e9}]]\n").unwrap();
    std::fs::write(tmp.path().join("Cafe\u{301}.md"), "---\ntitle: The cafe\n---\nBody.\n").unwrap();
    let info = reference_card_for("Caf\u{e9}", "index.md", tmp.path()).unwrap();
    assert_eq!(info.source_path.as_deref(), Some("Caf\u{e9}.md"));
    assert_eq!(info.title, "The cafe");
}
