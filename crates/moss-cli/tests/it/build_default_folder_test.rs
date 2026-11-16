//! `moss build` with no folder builds the site containing the working
//! directory, as `moss history` resolves it; with a folder it builds that one.
//! All names and text are invented.

use std::path::Path;
use std::process::{Command, Output};

const MOSS: &str = env!("CARGO_BIN_EXE_moss-cli");

fn run_in(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(MOSS)
        .args(args)
        .current_dir(cwd)
        .env("MOSS_HOME", home)
        .env_remove("MOSS_ENV")
        .env_remove("MOSS_SETA_URL")
        .output()
        .expect("spawn moss-cli")
}

fn make_site(root: &Path) -> std::path::PathBuf {
    let site = root.join("garden");
    std::fs::create_dir_all(site.join("posts")).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Garden Notes\n---\n\nWelcome.\n").unwrap();
    std::fs::write(site.join("posts/first.md"), "---\ntitle: First Bed\n---\n\nSeeds went in.\n").unwrap();
    site
}

fn assert_built(site: &Path, o: &Output) {
    assert!(
        o.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        site.join(".moss/build.nosync/current/index.html").is_file(),
        "no built home page under {}",
        site.display()
    );
}

#[test]
fn build_with_no_folder_builds_the_site_it_is_run_in() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let site = make_site(tmp.path());

    let o = run_in(&home, &site, &["build", "--strict", "--no-plugins"]);
    assert_built(&site, &o);
}

#[test]
fn build_with_no_folder_from_a_subfolder_builds_the_enclosing_site() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let site = make_site(tmp.path());
    // The first build creates the site's `.moss/`; the subfolder build must find it.
    assert_built(&site, &run_in(&home, &site, &["build", "--no-plugins"]));
    std::fs::remove_dir_all(site.join(".moss/build.nosync")).unwrap();

    let o = run_in(&home, &site.join("posts"), &["build", "--strict", "--no-plugins"]);
    assert_built(&site, &o);
}

#[test]
fn build_with_a_folder_still_builds_that_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let site = make_site(tmp.path());
    let elsewhere = tmp.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();

    let o = run_in(&home, &elsewhere, &["build", site.to_str().unwrap(), "--strict", "--no-plugins"]);
    assert_built(&site, &o);
}
