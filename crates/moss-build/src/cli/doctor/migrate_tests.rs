use super::*;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Three pier pages with an eyebrow div and a site config with a comment.
fn site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".moss/config.toml", "# the site\n[site]\nlang = \"en\"\n");
    for n in 1..=3 {
        write(dir.path(), &format!("Piers/p{n}.md"), &format!("---\ntitle: \"Pier {n}\"\n---\n\n::: {{.eyebrow}}\nPier {n} of 3\n:::\n\nText {n}.\n"));
    }
    dir
}

fn args(list: &[&str]) -> Args {
    parse_args(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
}

#[test]
fn a_dry_run_lift_lists_every_change_and_writes_nothing() {
    let dir = site();
    let before = std::fs::read_to_string(dir.path().join("Piers/p1.md")).unwrap();
    let report = migrate(dir.path(), &args(&["--lift", ".eyebrow", "--as", "stage", "--place", "eyebrow"])).unwrap();
    assert_eq!(report.changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), vec!["Piers/p1.md", "Piers/p2.md", "Piers/p3.md", ".moss/config.toml"]);
    assert!(!report.applied);
    assert_eq!(std::fs::read_to_string(dir.path().join("Piers/p1.md")).unwrap(), before);
    let text = report.to_text();
    assert!(text.contains("+stage: Pier 1 of 3\n"), "{text}");
    assert!(text.contains("-::: {.eyebrow}\n"), "{text}");
    assert!(text.ends_with("This would write 4 files. Nothing was written; run again with --apply to write it.\n"), "{text}");
}

#[test]
fn applied_changes_skip_a_file_edited_since_it_was_read() {
    let dir = site();
    let report = migrate(dir.path(), &args(&["--lift", "eyebrow", "--as", "stage", "--place", "eyebrow"])).unwrap();
    let mut changes = report.changes;
    write(dir.path(), "Piers/p2.md", "edited meanwhile\n");
    let mut skipped = Vec::new();
    write_all(dir.path(), &mut changes, &mut skipped);
    assert_eq!(skipped.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), vec!["Piers/p2.md"]);
    assert_eq!(std::fs::read_to_string(dir.path().join("Piers/p2.md")).unwrap(), "edited meanwhile\n");
    assert_eq!(std::fs::read_to_string(dir.path().join("Piers/p1.md")).unwrap(), "---\ntitle: \"Pier 1\"\nstage: Pier 1 of 3\n---\n\nText 1.\n");
    assert!(std::fs::read_to_string(dir.path().join(".moss/config.toml")).unwrap().starts_with("# the site\n"));
}

#[test]
fn the_theme_still_styling_the_class_is_noted() {
    let dir = site();
    write(dir.path(), ".moss/theme/style.css", ".eyebrow { color: red; }\n");
    let report = migrate(dir.path(), &args(&["--lift", "eyebrow", "--as", "stage", "--place", "eyebrow"])).unwrap();
    assert!(report.notes.iter().any(|n| n.contains("[data-field=\"stage\"]")), "{:?}", report.notes);
}

#[test]
fn nothing_to_rewrite_says_so_without_building() {
    let dir = site();
    let report = migrate(dir.path(), &args(&[])).unwrap();
    assert_eq!(report.check, Check::NotNeeded);
    assert_eq!(report.to_text(), "\nNothing to rewrite.\n");
}

#[test]
fn flags_that_go_together_are_checked() {
    let parse = |list: &[&str]| parse_args(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert!(parse(&["--lift", "x"]).is_err());
    assert!(parse(&["--as", "x"]).is_err());
    assert!(parse(&["--place", "eyebrow"]).is_err());
    assert!(parse(&["--bogus"]).is_err());
    assert!(parse(&["--lift", "x", "--as", "y"]).unwrap_err().contains("--place"));
    assert_eq!(parse(&["site", "--lift", ".x", "--as", "y", "--place", "none"]).unwrap().lift.as_deref(), Some("x"));
}

#[test]
fn a_diff_shows_each_change_with_a_line_of_context() {
    let d = diff("a\nb\nc\nd\ne\nf\ng\nh\n", "a\nb\nC\nd\ne\nf\ng\nh\nnew\n");
    assert_eq!(d, "@@ line 2 @@\n b\n-c\n+C\n d\n@@ line 8 @@\n h\n+new\n");
}


#[test]
fn a_value_flag_does_not_take_the_next_flag_as_its_value() {
    let parse = |list: &[&str]| parse_args(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert!(parse(&["--lift", "--apply", "--as", "x", "--place", "none"]).is_err());
}

#[test]
fn a_write_that_fails_is_listed_and_the_config_is_written_first() {
    let dir = site();
    let report = migrate(dir.path(), &args(&["--lift", "eyebrow", "--as", "stage", "--place", "eyebrow"])).unwrap();
    let mut changes = report.changes;
    changes.push(FileChange { path: "Piers/p1.md/not-a-dir.md".into(), before: String::new(), after: "x".into() });
    let mut skipped = Vec::new();
    write_all(dir.path(), &mut changes, &mut skipped);
    assert_eq!(changes[0].path, CONFIG);
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(skipped[0].reason.starts_with("could not be written"), "{skipped:?}");
}

#[test]
fn a_diff_of_a_huge_rewrite_shows_it_whole_without_matching() {
    let before: String = (0..3000).map(|i| format!("a{i}\n")).collect();
    let after: String = (0..3000).map(|i| format!("b{i}\n")).collect();
    let d = diff(&before, &after);
    assert_eq!(d.lines().filter(|l| l.starts_with('-')).count(), 3000);
    assert_eq!(diff("a\n", "a"), "@@ line endings or the final newline change @@\n");
}
