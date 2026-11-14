//! Real-process proof that a rename made through the editor under
//! `build --serve --watch` asks for its own rebuild instead of waiting for the
//! file watcher to report it. The watcher is made blind
//! (`MOSS_TEST_WATCH_BLIND`) and the sweep's tick is set far beyond the test
//! (`MOSS_TEST_SWEEP_TICK_MS`), so only the rename's own request can build the
//! page under its new name.
#![cfg(unix)]

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::process::Command;
use std::time::{Duration, Instant};

fn status(port: u16, path: &str) -> u16 {
    match ureq::get(&format!("http://localhost:{port}{path}")).call() {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("the server did not answer: {e}"),
    }
}

/// A new page is an empty `untitled.md` until it is given a title.
#[test]
fn a_new_page_given_its_title_is_served_with_the_watcher_blind() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(root.path()).unwrap().join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nA sentence.\n").unwrap();
    std::fs::write(site.join("untitled.md"), "").unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_moss-cli"));
    command
        .args(["build"])
        .arg(&site)
        .args(["--serve", "--watch", "--no-plugins"])
        .env("MOSS_TEST_WATCH_BLIND", "1")
        .env("MOSS_TEST_SWEEP_TICK_MS", "600000");
    let child = spawn_piped(command, home.path());
    let port = port_from_url_line(&child.wait_for_access_line(Duration::from_secs(60)));
    // The rebuild worker is registered with the watch, after the first build.
    wait_for_line(&child.stderr, "File watcher active", Duration::from_secs(60))
        .unwrap_or_else(|| panic!("the watch never started:\n{}", joined(&child.stderr)));
    assert_eq!(status(port, "/untitled/"), 200, "premise: the empty page is served");
    let token = std::fs::read_to_string(site.join(".moss/build.nosync/http-token")).expect("the carrier token is published");

    let from = child.stderr_len();
    ureq::post(&format!("http://localhost:{port}/__moss/mutate/rename_entry_with_refs"))
        .set("Content-Type", "application/json")
        .set("X-Moss-Token", token.trim())
        .send_string(&serde_json::json!({ "oldPath": "untitled.md", "newPath": "quiet-harbour.md" }).to_string())
        .unwrap_or_else(|e| panic!("the rename was refused: {e}"));
    assert!(site.join("quiet-harbour.md").exists());

    let deadline = Instant::now() + Duration::from_secs(3);
    while status(port, "/quiet-harbour/") != 200 {
        assert!(Instant::now() < deadline, "the renamed page was not served within 3 s; stderr:\n{}", joined(&child.stderr));
        std::thread::sleep(Duration::from_millis(50));
    }
    let lines = child.stderr.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(lines[from..].iter().filter(|l| l.contains("Sweep caught")).count(), 0, "only the rename rebuilt the page");
}
