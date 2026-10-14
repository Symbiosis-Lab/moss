//! `moss import <folder>` and `moss import --status` against a fake plugin that
//! accepts a made-up `.fake` extension, so nothing here depends on a real
//! file format. All names and text are invented.

use std::path::Path;
use std::process::{Command, Output};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

fn moss(home: &Path, args: &[&str]) -> Output {
    Command::new(MOSS)
        .args(args)
        .env("MOSS_HOME", home)
        .env_remove("MOSS_ENV")
        .output()
        .expect("spawn moss-cli")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn plugin(site: &Path, name: &str, derivation: &str) {
    plugin_pinned(site, name, derivation, Path::new("/bin/true"));
}

fn plugin_pinned(site: &Path, name: &str, derivation: &str, binary: &Path) {
    let dir = site.join(".moss/plugins").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        format!(
            r##"{{"name":"{name}","version":"1.0.0","contributes":{{"derivations":{{"{derivation}":{{
                "accepts":["fake"],"noun":{{"one":"thing","other":"things"}},
                "fragments":{{"body":"markdown"}},"stub":["","#body"],
                "run":{{"binary":"unpack","artifact":[{{"kind":"path","binary":"{}"}}]}}}}}}}}}}"##,
            binary.display()
        ),
    )
    .unwrap();
}

struct Fixture {
    _tmp: tempfile::TempDir,
    home: std::path::PathBuf,
    site: std::path::PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let site = tmp.path().join("site");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(site.join(".moss")).unwrap();
    std::fs::create_dir_all(site.join("reports")).unwrap();
    std::fs::write(site.join(".moss/config.toml"), "# mine\n").unwrap();
    std::fs::write(site.join("reports/Q1 Summary.fake"), "one").unwrap();
    std::fs::write(site.join("reports/Q2.fake"), "two").unwrap();
    Fixture { _tmp: tmp, home, site }
}

#[test]
fn without_allow_plugins_a_folder_borne_plugin_is_refused_as_a_build_refuses_it() {
    let f = fixture();
    plugin(&f.site, "fakes", "fake-pages");
    let folder = f.site.join("reports");
    for args in [vec!["import", folder.to_str().unwrap()], vec!["import", "--status", f.site.to_str().unwrap()]] {
        let o = moss(&f.home, &args);
        assert!(!o.status.success(), "{}", stdout(&o));
        let err = stderr(&o);
        assert!(err.contains("has not been allowed to run here") && err.contains("--allow-plugins"), "{err}");
    }
    assert!(!folder.join("Q2.md").exists(), "no page is written for a plugin the build would refuse");
}

#[test]
fn expand_refuses_a_folder_borne_plugin_without_allow_plugins_too() {
    let f = fixture();
    plugin(&f.site, "fakes", "fake-pages");
    std::fs::write(f.site.join("page.md"), "![[reports/Q2.fake#body]]\n").unwrap();
    let page = f.site.join("page.md");
    let o = moss(&f.home, &["expand", page.to_str().unwrap()]);
    assert!(!o.status.success() && stderr(&o).contains("--allow-plugins"), "{}", stderr(&o));
    let o = moss(&f.home, &["expand", page.to_str().unwrap(), "--allow-plugins", "--dry-run"]);
    assert!(!stderr(&o).contains("will not run"), "{}", stderr(&o));
}

#[test]
fn importing_a_folder_shows_its_files_as_pages_and_remembers_the_choice() {
    let f = fixture();
    plugin(&f.site, "fakes", "fake-pages");
    let folder = f.site.join("reports");
    let o = moss(&f.home, &["import", folder.to_str().unwrap(), "--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.starts_with(&format!("Showing 2 things as pages in {}", folder.display())), "{out}");
    assert!(out.contains("Q1 Summary.md") && out.contains("Q2.md"), "{out}");

    let page = std::fs::read_to_string(folder.join("Q1 Summary.md")).unwrap();
    assert_eq!(page, "---\ninferred: [title, description]\n---\n![[Q1 Summary.fake]]\n\n![[Q1 Summary.fake#body]]\n");
    let config = std::fs::read_to_string(f.site.join(".moss/config.toml")).unwrap();
    assert!(config.contains("# mine") && config.contains("\"reports/\" = [\"fake-pages\"]"), "{config}");

    let again = moss(&f.home, &["import", folder.to_str().unwrap(), "--allow-plugins"]);
    assert!(again.status.success());
    assert!(stdout(&again).lines().count() == 1, "a second run writes no pages: {}", stdout(&again));
}

#[test]
fn a_folder_no_plugin_accepts_says_so_and_changes_nothing() {
    let f = fixture();
    let o = moss(&f.home, &["import", f.site.join("reports").to_str().unwrap(), "--allow-plugins"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("no installed plugin turns the files"), "{}", stderr(&o));
    assert_eq!(std::fs::read_to_string(f.site.join(".moss/config.toml")).unwrap(), "# mine\n");
    assert!(!f.site.join("reports/Q2.md").exists());
}

#[test]
fn two_derivations_on_one_folder_need_with() {
    let f = fixture();
    plugin(&f.site, "alpha", "alpha-pages");
    // A second plugin on another extension, so both have files here.
    let dir = f.site.join(".moss/plugins/beta");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        r##"{"name":"beta","version":"1.0.0","contributes":{"derivations":{"beta-pages":{
            "accepts":["other"],"noun":{"one":"item","other":"items"},
            "fragments":{"body":"markdown"},"stub":[""],
            "run":{"binary":"unpack","artifact":[{"kind":"path","binary":"/bin/true"}]}}}}}"##,
    )
    .unwrap();
    std::fs::write(f.site.join("reports/x.other"), "x").unwrap();
    let folder = f.site.join("reports");

    let o = moss(&f.home, &["import", folder.to_str().unwrap(), "--allow-plugins"]);
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(err.contains("--with") && err.contains("alpha-pages") && err.contains("beta-pages"), "{err}");

    let o = moss(&f.home, &["import", folder.to_str().unwrap(), "--with", "beta-pages", "--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).starts_with("Showing 1 item as pages"), "{}", stdout(&o));
    assert!(folder.join("x.md").exists() && !folder.join("Q2.md").exists());
}

#[test]
fn status_json_reports_decision_counts_missing_pages_and_declined_files() {
    let f = fixture();
    plugin(&f.site, "fakes", "fake-pages");
    // Seed a decline for one file the way a build's derive would store it.
    let skipper = f.site.join("skip.sh");
    std::fs::write(
        &skipper,
        "#!/bin/sh\ncat >/dev/null\nprintf '{\"kind\":\"skip\",\"reason\":\"locked\",\"message\":\"It is locked.\"}'\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&skipper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The manifest pins the skipper, so the stored decline is under the pin the CLI will look up.
    plugin_pinned(&f.site, "fakes", "fake-pages", &skipper);
    let paths = moss_build::moss_paths::MossPaths::new(&f.site);
    let entry = moss_build::vault::derive_state::site_registry(&paths).unwrap().for_extension("fake").unwrap().clone();
    moss_build::system::derive::derive_cached(&paths, &entry, &f.site.join("reports/Q2.fake")).unwrap();

    let o = moss(&f.home, &["import", "--status", f.site.to_str().unwrap(), "--json", "--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"folders": [{
            "folder": "reports/", "derivation_id": "fake-pages",
            "noun_one": "thing", "noun_other": "things", "count": 2,
            "decision": "undecided", "stubs_missing": 0, "failed": [],
            "skipped": [{"file": "Q2.fake", "reason": "locked", "message": "It is locked."}]
        }]})
    );

    let shown = moss(&f.home, &["import", f.site.join("reports").to_str().unwrap(), "--allow-plugins"]);
    assert!(shown.status.success());
    std::fs::remove_file(f.site.join("reports/Q2.md")).unwrap();
    let o = moss(&f.home, &["import", "--status", f.site.to_str().unwrap(), "--json", "--allow-plugins"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["folders"][0]["decision"], "show");
    assert_eq!(v["folders"][0]["stubs_missing"], 1);

    let text = moss(&f.home, &["import", "--status", f.site.to_str().unwrap(), "--allow-plugins"]);
    let text = stdout(&text);
    assert!(text.contains("reports/  2 things  shown as pages  (1 pages missing)"), "{text}");
    assert!(text.contains("skipped Q2.fake: It is locked."), "{text}");
}

#[test]
fn the_url_and_file_forms_still_parse_as_before() {
    let f = fixture();
    let o = moss(&f.home, &["import", "--bogus-flag"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("unknown option: --bogus-flag"), "{}", stderr(&o));
    let o = moss(&f.home, &["import"]);
    assert!(stderr(&o).contains("missing URL"), "{}", stderr(&o));
}

/// reports/ (2 files) -> 2024/ (1) -> deep/ (1), a sibling old/ already kept,
/// an unrelated notes/ without files the plugin accepts, and a hidden folder.
fn tree_fixture() -> Fixture {
    let f = fixture();
    for (dir, file) in [("reports/2024", "Jan.fake"), ("reports/2024/deep", "Deep.fake"), ("reports/old", "Old.fake")] {
        std::fs::create_dir_all(f.site.join(dir)).unwrap();
        std::fs::write(f.site.join(dir).join(file), "x").unwrap();
    }
    std::fs::create_dir_all(f.site.join("reports/notes")).unwrap();
    std::fs::write(f.site.join("reports/notes/a.txt"), "plain").unwrap();
    std::fs::create_dir_all(f.site.join("reports/.hidden")).unwrap();
    std::fs::write(f.site.join("reports/.hidden/Secret.fake"), "x").unwrap();
    std::fs::write(f.site.join(".moss/config.toml"), "# mine\n[derive]\n\"reports/old/\" = false\n").unwrap();
    plugin(&f.site, "fakes", "fake-pages");
    f
}

#[test]
fn recursive_import_covers_the_tree_and_leaves_a_decided_folder_alone() {
    let f = tree_fixture();
    let reports = f.site.join("reports");
    let o = moss(&f.home, &["import", reports.to_str().unwrap(), "--recursive", "--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            "reports/  shown as pages  2 pages written",
            "reports/2024/  shown as pages  1 page written",
            "reports/2024/deep/  shown as pages  1 page written",
            "reports/old/  kept as files, left as it was  0 pages written",
            "4 folders, 4 pages written",
        ],
        "{out}"
    );
    for page in ["Q2.md", "Q1 Summary.md", "2024/Jan.md", "2024/deep/Deep.md"] {
        assert!(reports.join(page).exists(), "{page}");
    }
    assert!(!reports.join("old/Old.md").exists() && !reports.join(".hidden/Secret.md").exists());
    let config = std::fs::read_to_string(f.site.join(".moss/config.toml")).unwrap();
    for key in ["\"reports/\" = [\"fake-pages\"]", "\"reports/2024/\" = [\"fake-pages\"]", "\"reports/2024/deep/\" = [\"fake-pages\"]", "\"reports/old/\" = false"] {
        assert!(config.contains(key), "{key} in {config}");
    }
    assert!(!config.contains("notes") && !config.contains("hidden"), "{config}");

    // Everything now has a decision: a second run changes and writes nothing, and succeeds.
    let again = moss(&f.home, &["import", reports.to_str().unwrap(), "-r", "--allow-plugins"]);
    assert!(again.status.success(), "{}", stderr(&again));
    let out = stdout(&again);
    assert!(out.contains("reports/2024/  shown as pages, left as it was  0 pages written"), "{out}");
    assert!(out.ends_with("4 folders, 0 pages written\n"), "{out}");
    assert_eq!(std::fs::read_to_string(f.site.join(".moss/config.toml")).unwrap(), config);
}

#[test]
fn recursive_import_with_nothing_to_show_is_the_same_error_as_one_folder() {
    let f = tree_fixture();
    let notes = f.site.join("reports/notes");
    let o = moss(&f.home, &["import", notes.to_str().unwrap(), "-r", "--allow-plugins"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("no installed plugin turns the files"), "{}", stderr(&o));
}

#[test]
fn recursive_import_reports_a_folder_two_plugins_claim_and_with_resolves_it() {
    let f = fixture();
    plugin(&f.site, "alpha", "alpha-pages");
    let dir = f.site.join(".moss/plugins/beta");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        r##"{"name":"beta","version":"1.0.0","contributes":{"derivations":{"beta-pages":{
            "accepts":["other"],"noun":{"one":"item","other":"items"},
            "fragments":{"body":"markdown"},"stub":[""],
            "run":{"binary":"unpack","artifact":[{"kind":"path","binary":"/bin/true"}]}}}}}"##,
    )
    .unwrap();
    std::fs::write(f.site.join("reports/x.other"), "x").unwrap();
    let reports = f.site.join("reports");

    let o = moss(&f.home, &["import", reports.to_str().unwrap(), "-r", "--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("reports/  not changed: several plugins accept these files"), "{}", stdout(&o));
    assert!(!reports.join("x.md").exists());

    let o = moss(&f.home, &["import", reports.to_str().unwrap(), "-r", "--with", "beta-pages", "--allow-plugins"]);
    assert!(stdout(&o).starts_with("reports/  shown as pages  1 page written"), "{}", stdout(&o));
    assert!(reports.join("x.md").exists() && !reports.join("Q2.md").exists());
}

#[test]
fn recursive_import_reports_a_decided_folder_as_decided_even_when_two_plugins_claim_it() {
    let f = fixture();
    plugin(&f.site, "alpha", "alpha-pages");
    let dir = f.site.join(".moss/plugins/beta");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        r##"{"name":"beta","version":"1.0.0","contributes":{"derivations":{"beta-pages":{
            "accepts":["other"],"noun":{"one":"item","other":"items"},
            "fragments":{"body":"markdown"},"stub":[""],
            "run":{"binary":"unpack","artifact":[{"kind":"path","binary":"/bin/true"}]}}}}}"##,
    )
    .unwrap();
    std::fs::write(f.site.join("reports/x.other"), "x").unwrap();
    let config = "# mine\n[derive]\n\"reports/\" = false\n";
    std::fs::write(f.site.join(".moss/config.toml"), config).unwrap();
    let reports = f.site.join("reports");

    let o = moss(&f.home, &["import", reports.to_str().unwrap(), "-r", "--allow-plugins"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.contains("reports/  kept as files, left as it was  0 pages written"), "{out}");
    assert!(!out.contains("several plugins"), "{out}");
    assert_eq!(std::fs::read_to_string(f.site.join(".moss/config.toml")).unwrap(), config);
    assert!(!reports.join("Q2.md").exists() && !reports.join("x.md").exists());
}
