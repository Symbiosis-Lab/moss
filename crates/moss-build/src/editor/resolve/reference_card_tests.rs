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
