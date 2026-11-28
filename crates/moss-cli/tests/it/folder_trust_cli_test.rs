//! A build with no window reads the folder answers the app recorded under the
//! user's data directory, and writes none. HOME and XDG_DATA_HOME point at a
//! temporary root, so the person's own answers are never read. Not on Windows,
//! whose data directory comes from the Known Folder API and ignores HOME.
#![cfg(not(windows))]

use moss_build::moss_paths::MossPaths;
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
    let page = || std::fs::read_to_string(MossPaths::new(&site).current_ptr().join("board/index.html")).unwrap();
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

    let withdrawn = serde_json::json!({"folders": [{"dir": site.canonicalize().unwrap(), "trusted": false}]}).to_string();
    for file in trust_files(&home) {
        write(&file, &withdrawn);
    }
    let refused_again = build(&site, &home);
    assert!(refused_again.status.success(), "{}", stderr(&refused_again));
    assert!(!page().contains("boardview"), "kept a plugin's output after trust was withdrawn: {}", page());
}

#[test]
fn sync_runs_the_process_hooks_as_a_person_s_run_and_waits_for_trust() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let site = tmp.path().join("site");
    write(&site.join(".moss/config.toml"), "# mine\n");
    write(&site.join("index.md"), "---\ntitle: Home\n---\nWelcome\n");
    write(
        &site.join(".moss/plugins/importer/manifest.json"),
        r#"{"name":"importer","version":"1.0.0","entry":"main.js","global_name":"Importer","capabilities":["process"]}"#,
    );
    write(
        &site.join(".moss/plugins/importer/main.js"),
        r#"var Importer = (() => ({ async process(ctx) { await globalThis.__TAURI__.core.invoke("write_plugin_file", { relativePath: "trigger", content: ctx.trigger }); return { success: true }; } }))();"#,
    );
    let sync = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_moss-cli"))
            .arg("sync")
            .arg(&site)
            .args(extra)
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("MOSS_HOME", home.join("moss"))
            .env_remove("MOSS_ENV")
            .output()
            .expect("spawn moss-cli")
    };
    let ran = site.join(".moss/plugins/importer/trigger");
    let stderr = |o: &Output| String::from_utf8_lossy(&o.stderr).into_owned();

    let refused = sync(&[]);
    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    assert!(stderr(&refused).contains("so importer cannot run"), "{}", stderr(&refused));
    assert!(!ran.exists(), "ran in a folder nobody trusted");

    let allowed = sync(&["--allow-plugins"]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert_eq!(std::fs::read_to_string(&ran).unwrap(), "settings_manual");
    assert!(!MossPaths::new(&site).current_ptr().exists(), "sync built the site");
}

#[test]
fn a_failed_process_hook_fails_moss_sync_and_a_strict_build() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let site = tmp.path().join("site");
    write(&site.join(".moss/config.toml"), "# mine\n");
    write(&site.join("index.md"), "---\ntitle: Home\n---\nWelcome\n");
    write(
        &site.join(".moss/plugins/breaker/manifest.json"),
        r#"{"name":"breaker","version":"1.0.0","entry":"main.js","global_name":"Breaker","capabilities":["process"]}"#,
    );
    write(&site.join(".moss/plugins/breaker/main.js"), r#"var Breaker = (() => ({ async process(ctx) { throw new Error("no posts"); } }))();"#);
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_moss-cli"))
            .args(args)
            .arg(&site)
            .arg("--allow-plugins")
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("MOSS_HOME", home.join("moss"))
            .env_remove("MOSS_ENV")
            .output()
            .expect("spawn moss-cli")
    };
    let stderr = |o: &Output| String::from_utf8_lossy(&o.stderr).into_owned();

    let sync = run(&["sync"]);
    assert_eq!(sync.status.code(), Some(1), "{}", stderr(&sync));
    assert!(stderr(&sync).contains("process hook failed: breaker"), "{}", stderr(&sync));

    let strict = run(&["build", "--strict"]);
    assert!(!strict.status.success(), "a strict build passed with a failed process hook: {}", stderr(&strict));
}
