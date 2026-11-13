use std::process::{Command, Output};

fn root_command(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["root"])
        .args(args)
        .env("HOME", home)
        .env("MOSS_HOME", home.join("tooling/.moss"))
        .env("MOSS_ENV", "production")
        .env_remove("MOSS_SETA_URL")
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
        serde_json::json!({"root": site.to_string_lossy(), "address": null}).to_string()
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
        r#"{"address":null,"root":null}"#
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

#[test]
fn root_excludes_relocated_tooling_marker() {
    let home = tempfile::tempdir().unwrap();
    let tooling = home.path().join("tooling/.moss");
    let loose = home.path().join("tooling/notes");
    std::fs::create_dir_all(&tooling).unwrap();
    std::fs::create_dir_all(&loose).unwrap();
    let output = root_command(home.path(), &[loose.to_str().unwrap(), "--json"]);
    assert!(output.status.success());
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(), serde_json::json!({"root": null, "address": null}));
}

#[cfg(unix)]
#[test]
fn root_excludes_tooling_marker_alias_but_preserves_site_alias() {
    use std::os::unix::fs::symlink;
    let home = tempfile::tempdir().unwrap();
    let tooling = home.path().join("tooling/.moss");
    let alias = home.path().join("alias");
    std::fs::create_dir_all(&tooling).unwrap();
    std::fs::create_dir_all(&alias).unwrap();
    symlink(&tooling, alias.join(".moss")).unwrap();
    let output = root_command(home.path(), &[alias.to_str().unwrap(), "--json"]);
    assert!(output.status.success());
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(), serde_json::json!({"root": null, "address": null}));

    let site = home.path().join("site");
    std::fs::create_dir_all(site.join(".moss")).unwrap();
    let site_alias = home.path().join("site-alias");
    symlink(&site, &site_alias).unwrap();
    let output = root_command(home.path(), &[site_alias.to_str().unwrap(), "--json"]);
    assert!(output.status.success());
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(), serde_json::json!({"root": site_alias, "address": null}));
}

fn address_of(home: &std::path::Path, site: &std::path::Path) -> serde_json::Value {
    let output = root_command(home, &[site.to_str().unwrap(), "--json"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["address"].clone()
}

#[test]
fn root_names_the_address_a_published_site_is_live_at() {
    let home = tempfile::tempdir().unwrap();
    let site = home.path().join("her-blog");
    std::fs::create_dir_all(site.join(".moss")).unwrap();
    let state = site.join(".moss/state.toml");

    // A site id alone is chosen before the first publish succeeds.
    std::fs::write(&state, "schema_version = 1\n\n[deployment]\nsite_id = \"her-blog\"\n").unwrap();
    assert_eq!(address_of(home.path(), &site), serde_json::Value::Null);

    let published = "schema_version = 1\n\n[deployment]\nsite_id = \"her-blog\"\n\n\
        [deployment.targets.\"moss:her-blog\"]\nlast_deployment_url = \"https://her-blog.mosspub.com\"\n";
    std::fs::write(&state, published).unwrap();
    assert_eq!(address_of(home.path(), &site), "her-blog.mosspub.com");

    // A custom domain is the address once its DNS is verified, not before.
    std::fs::write(site.join(".moss/config.toml"), "[site]\ndomain = \"her.example\"\n").unwrap();
    assert_eq!(address_of(home.path(), &site), "her-blog.mosspub.com");
    std::fs::write(&state, format!("{published}\n[deployment.observed]\nchecked_at = \"2026-01-01T00:00:00Z\"\ndns_configured = true\n")).unwrap();
    assert_eq!(address_of(home.path(), &site), "her.example");
}

#[test]
fn root_names_a_plugin_host_by_its_last_publish() {
    let home = tempfile::tempdir().unwrap();
    let site = home.path().join("her-blog");
    std::fs::create_dir_all(site.join(".moss")).unwrap();
    std::fs::write(site.join(".moss/config.toml"), "[hooks]\ndeploy = \"github\"\n").unwrap();
    let github = "[deployment.targets.github]\nlast_deployment_url = \"https://her.github.io/blog/\"\n";
    std::fs::write(site.join(".moss/state.toml"), format!("schema_version = 1\n\n{github}")).unwrap();
    assert_eq!(address_of(home.path(), &site), "her.github.io/blog");

    // A folder that moved from moss hosting keeps its site id; the active host still wins.
    let moved = format!("schema_version = 1\n\n[deployment]\nsite_id = \"her-blog\"\n\n\
        [deployment.targets.\"moss:her-blog\"]\nlast_deployment_url = \"https://her-blog.mosspub.com\"\n\n{github}");
    std::fs::write(site.join(".moss/state.toml"), moved).unwrap();
    assert_eq!(address_of(home.path(), &site), "her.github.io/blog");
}

#[test]
fn root_leaves_out_an_address_it_cannot_read() {
    let home = tempfile::tempdir().unwrap();
    let site = home.path().join("her-blog");
    std::fs::create_dir_all(site.join(".moss")).unwrap();
    // A state file from a newer moss is refused, so whether the site is live is unknown.
    std::fs::write(site.join(".moss/state.toml"), "schema_version = 999\n").unwrap();
    let output = root_command(home.path(), &[site.to_str().unwrap(), "--json"]);
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["root"], site.to_string_lossy().as_ref());
    assert!(json.get("address").is_none(), "{json}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("address unknown"));
}
