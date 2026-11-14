//! Real-process proof that a page made or renamed through the editor under
//! `build --serve --watch` asks for its own rebuild instead of waiting for the
//! file watcher to report it. The watcher is made blind
//! (`MOSS_TEST_WATCH_BLIND`) and the sweep's tick is set far beyond the test
//! (`MOSS_TEST_SWEEP_TICK_MS`), so only the command's own request can build
//! the page.
#![cfg(unix)]

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn status(port: u16, path: &str) -> u16 {
    match ureq::get(&format!("http://localhost:{port}{path}")).call() {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("the server did not answer: {e}"),
    }
}

/// A site of `pages` served with the watcher blind and the sweep out of reach,
/// its watch started.
struct Served {
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    site: PathBuf,
    child: ServedChild,
    port: u16,
}

impl Served {
    fn start(pages: &[(&str, &str)]) -> Self {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let site = std::fs::canonicalize(root.path()).unwrap().join("site");
        std::fs::create_dir_all(&site).unwrap();
        for (name, text) in pages {
            std::fs::write(site.join(name), text).unwrap();
        }
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
        Served { _dirs: (home, root), site, child, port }
    }

    /// Run a write command as the editor does, then wait for `address`.
    fn asks_and_is_served(&self, command: &str, args: serde_json::Value, address: &str) {
        let token = std::fs::read_to_string(self.site.join(".moss/build.nosync/http-token")).expect("the carrier token is published");
        let from = self.child.stderr_len();
        ureq::post(&format!("http://localhost:{}/__moss/mutate/{command}", self.port))
            .set("Content-Type", "application/json")
            .set("X-Moss-Token", token.trim())
            .send_string(&args.to_string())
            .unwrap_or_else(|e| panic!("{command} was refused: {e}"));
        let deadline = Instant::now() + Duration::from_secs(3);
        while status(self.port, address) != 200 {
            assert!(Instant::now() < deadline, "{address} was not served within 3 s; stderr:\n{}", joined(&self.child.stderr));
            std::thread::sleep(Duration::from_millis(50));
        }
        let lines = self.child.stderr.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(lines[from..].iter().filter(|l| l.contains("Sweep caught")).count(), 0, "only the command's request rebuilt the page");
    }
}

const HOME: (&str, &str) = ("index.md", "---\ntitle: Home\n---\n\nA sentence.\n");

/// A new page is an empty `untitled.md` until it is given a title.
#[test]
fn a_new_page_given_its_title_is_served_with_the_watcher_blind() {
    let served = Served::start(&[HOME, ("untitled.md", "")]);
    assert_eq!(status(served.port, "/untitled/"), 200, "premise: the empty page is served");
    served.asks_and_is_served("rename_entry_with_refs", serde_json::json!({ "oldPath": "untitled.md", "newPath": "quiet-harbour.md" }), "/quiet-harbour/");
    assert!(served.site.join("quiet-harbour.md").exists());
}

/// A page left untitled is served as it is made.
#[test]
fn a_new_page_is_served_with_the_watcher_blind() {
    let served = Served::start(&[HOME]);
    assert_eq!(status(served.port, "/untitled/"), 404, "premise: no such page yet");
    served.asks_and_is_served("create_files", serde_json::json!({ "files": [{ "dir": served.site.to_string_lossy(), "name": "untitled", "frontmatter": {} }] }), "/untitled/");
}
