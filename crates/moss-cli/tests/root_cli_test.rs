use std::process::{Command, Output};

fn root_command(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["root"])
        .args(args)
        .env("HOME", home)
        .current_dir(home)
        .output()
        .unwrap()
}

#[test]
fn root_reports_only_canonical_owned_site_and_never_writes() {
    let home = tempfile::tempdir().unwrap();
    let site = home.path().join("projects/site");
    std::fs::create_dir_all(site.join(".moss")).unwrap();
    let nested = site.join("articles/post.md");
    std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
    std::fs::write(&nested, "# Post").unwrap();

    let cache = home.path().join(".moss/cache/unowned");
    std::fs::create_dir_all(cache.join("nested")).unwrap();
    let unowned = cache.join("nested/note.md");
    std::fs::write(&unowned, "# Note").unwrap();

    let owned = root_command(home.path(), &[nested.to_str().unwrap(), "--json"]);
    assert!(
        owned.status.success(),
        "{}",
        String::from_utf8_lossy(&owned.stderr)
    );
    assert!(owned.stderr.is_empty());
    assert_eq!(
        String::from_utf8(owned.stdout).unwrap().trim(),
        serde_json::json!({"root": site.to_string_lossy()}).to_string()
    );

    let loose = root_command(home.path(), &[unowned.to_str().unwrap(), "--json"]);
    assert!(
        loose.status.success(),
        "{}",
        String::from_utf8_lossy(&loose.stderr)
    );
    assert!(loose.stderr.is_empty());
    assert_eq!(
        String::from_utf8(loose.stdout).unwrap().trim(),
        r#"{"root":null}"#
    );

    assert_eq!(
        std::fs::read_dir(home.path()).unwrap().count(),
        2,
        "query must not write under the home fixture"
    );
}

#[test]
fn root_rejects_missing_path_and_unknown_flags() {
    let home = tempfile::tempdir().unwrap();
    for args in [&["--json"][..], &["/tmp", "--json", "--scan"], &["/tmp"]] {
        let output = root_command(home.path(), args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: moss root"));
    }
}
