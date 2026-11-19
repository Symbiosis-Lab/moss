//! A build with no window reads the folder answers the app recorded under the
//! user's data directory, and writes none. HOME and XDG_DATA_HOME point at a
//! temporary root, so the person's own answers are never read. Not on Windows,
//! whose data directory comes from the Known Folder API and ignores HOME.
#![cfg(not(windows))]

use std::path::Path;
use std::process::{Command, Output};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn build(site: &Path, home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("build")
        .arg(site)
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("MOSS_HOME", home.join("moss"))
        .env_remove("MOSS_ENV")
        .output()
        .expect("spawn moss-cli")
}

/// Where the app keeps its answers under `home`, on macOS and elsewhere.
fn trust_files(home: &Path) -> [std::path::PathBuf; 2] {
    let file = "host.moss.publisher/trusted-folders.json";
    [home.join("Library/Application Support").join(file), home.join("data").join(file)]
}

#[test]
fn a_headless_build_runs_a_folder_s_plugins_only_when_the_app_trusted_the_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let site = tmp.path().join("site");
    write(&site.join(".moss/config.toml"), "# mine\n");
    write(&site.join("index.md"), "---\ntitle: Home\n---\nWelcome\n");
    write(&site.join("board/index.md"), "---\ntitle: Board\nchildren_style: board\n---\nIntro\n");
    write(&site.join("board/Beta.md"), "---\ntitle: Beta\n---\nbeta\n");
    write(
        &site.join(".moss/plugins/boardview/manifest.json"),
        r#"{"name":"boardview","version":"1.0.0","entry":"main.js","contributes":{"listing_styles":{"board":{"label":"Board"}}}}"#,
    );
    write(
        &site.join(".moss/plugins/boardview/main.js"),
        r#"var BoardviewPlugin = (() => ({ async listing(ctx) { return { html: '<ul class="boardview"></ul>' }; } }))();"#,
    );
    let page = || std::fs::read_to_string(site.join(".moss/build.nosync/current/board/index.html")).unwrap();
    let stderr = |o: &Output| String::from_utf8_lossy(&o.stderr).into_owned();

    let refused = build(&site, &home);
    assert!(refused.status.success(), "{}", stderr(&refused));
    assert!(!page().contains("boardview"), "ran in a folder nobody trusted: {}", page());
    assert!(stderr(&refused).contains("off until you trust this folder"), "{}", stderr(&refused));

    let record = serde_json::json!({"folders": [{"dir": site.canonicalize().unwrap(), "trusted": true}]}).to_string();
    for file in trust_files(&home) {
        write(&file, &record);
    }
    let trusted = build(&site, &home);
    assert!(trusted.status.success(), "{}", stderr(&trusted));
    assert!(page().contains(r#"<ul class="boardview">"#), "the trusted folder's plugin did not run: {}", page());
    for file in trust_files(&home) {
        assert_eq!(std::fs::read_to_string(&file).unwrap(), record, "a headless build wrote the record");
    }
}
