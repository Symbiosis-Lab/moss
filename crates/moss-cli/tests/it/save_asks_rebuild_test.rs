//! Real-process proof that an editor save under `build --serve --watch` asks
//! for its own rebuild instead of waiting for the file watcher to report it.
//! The watcher is made blind (`MOSS_TEST_WATCH_BLIND`) and the sweep's tick is
//! set far beyond the test (`MOSS_TEST_SWEEP_TICK_MS`), so only the save's own
//! request can rebuild the page. The site is opened through a symlink; the CLI
//! canonicalizes the folder, so the save and the rebuild worker agree on it.
#![cfg(unix)]

use crate::served_child;

use served_child::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const PAGE: &str = "---\ntitle: Home\n---\n\nOriginal sentence.\n";

struct Site {
    _root: tempfile::TempDir,
    _home: tempfile::TempDir,
    site: PathBuf,
    child: ServedChild,
    port: u16,
    token: String,
}

/// A served, watching site whose first build has sealed, reached through a
/// symlink to its real folder.
fn serve() -> Site {
    let site = spawn(0, "Access at");
    wait_for_line(&site.child.stderr, "seal+persist", Duration::from_secs(60))
        .unwrap_or_else(|| panic!("the first build never sealed:\n{}", joined(&site.child.stderr)));
    assert!(site.home_page().contains("Original sentence."), "premise: the first build is served");
    site
}

/// The site with `notes` more pages in a folder of their own, spawned and
/// waited for as far as the first stderr line holding `ready`, which must
/// carry the server's address.
fn spawn(notes: usize, ready: &str) -> Site {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("real/site/notes")).unwrap();
    std::os::unix::fs::symlink(root.path().join("real"), root.path().join("alias")).unwrap();
    let site = root.path().join("alias/site");
    std::fs::write(site.join("index.md"), PAGE).unwrap();
    for i in 0..notes {
        std::fs::write(site.join(format!("notes/note-{i}.md")), format!("Note {i}.\n")).unwrap();
    }

    let mut command = Command::new(env!("CARGO_BIN_EXE_moss-cli"));
    command
        .args(["build"])
        .arg(&site)
        .args(["--serve", "--watch", "--no-plugins"])
        .env("MOSS_TEST_WATCH_BLIND", "1")
        .env("MOSS_TEST_SWEEP_TICK_MS", "600000");
    let child = spawn_piped(command, home.path());
    let line = wait_for_line(&child.stderr, ready, Duration::from_secs(60))
        .unwrap_or_else(|| panic!("the server never said {ready:?}:\n{}", joined(&child.stderr)));
    let port = port_from_url_line(&line);
    let token = std::fs::read_to_string(site.join(".moss/build.nosync/http-token")).expect("the carrier token is published");
    Site { _root: root, _home: home, site, child, port, token: token.trim().to_string() }
}

impl Site {
    fn home_page(&self) -> String {
        ureq::get(&format!("http://localhost:{}/", self.port)).call().map(|r| r.into_string().unwrap_or_default()).unwrap_or_default()
    }

    /// Save `body` over the page whose bytes hash to `expected`, the way the
    /// editor does; returns the hash of what was written.
    fn save(&self, body: &str, expected: &str) -> String {
        let payload = serde_json::json!({
            "filePath": "index.md",
            "frontmatter": { "title": "Home" },
            "body": body,
            "expectedHash": expected,
        });
        let reply: serde_json::Value = ureq::post(&format!("http://localhost:{}/__moss/mutate/save_editor_content", self.port))
            .set("Content-Type", "application/json")
            .set("X-Moss-Token", &self.token)
            .send_string(&payload.to_string())
            .unwrap_or_else(|e| match e {
                ureq::Error::Status(code, r) => panic!("the save was refused ({code}): {}", r.into_string().unwrap_or_default()),
                e => panic!("the save never reached the server: {e}"),
            })
            .into_json()
            .unwrap();
        assert_eq!(reply["status"], "saved", "{reply}");
        reply["hash"].as_str().unwrap().to_string()
    }

    fn wait_for_home(&self, text: &str, within: Duration) {
        let deadline = Instant::now() + within;
        while !self.home_page().contains(text) {
            assert!(Instant::now() < deadline, "the served page never showed {text:?} within {within:?}; stderr:\n{}", joined(&self.child.stderr));
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn count(&self, needle: &str, from: usize) -> usize {
        self.child.stderr.lock().unwrap_or_else(std::sync::PoisonError::into_inner)[from..].iter().filter(|l| l.contains(needle)).count()
    }

    /// Wait until stderr has been quiet for `quiet`, so a rebuild still to come
    /// has started and finished.
    fn settle(&self, quiet: Duration) {
        let (mut last, mut since) = (self.child.stderr_len(), Instant::now());
        let deadline = Instant::now() + Duration::from_secs(60);
        while since.elapsed() < quiet {
            assert!(Instant::now() < deadline, "the child never went quiet; stderr:\n{}", joined(&self.child.stderr));
            std::thread::sleep(Duration::from_millis(100));
            let now = self.child.stderr_len();
            if now != last {
                (last, since) = (now, Instant::now());
            }
        }
    }
}

fn hash_of(path: &Path) -> String {
    moss_build::editor::content::content_hash(&std::fs::read(path).unwrap())
}

#[test]
fn a_save_is_served_with_the_watcher_blind() {
    let site = serve();
    let from = site.child.stderr_len();
    site.save("Saved sentence.", &hash_of(&site.site.join("index.md")));
    site.wait_for_home("Saved sentence.", Duration::from_secs(3));
    assert_eq!(site.count("Rebuild asked by save", from), 1, "{}", joined(&site.child.stderr));
    assert_eq!(site.count("Sweep caught", from), 0, "only the save rebuilt the page");
}

#[test]
fn two_saves_50ms_apart_serve_the_second_with_at_most_two_builds() {
    let site = serve();
    site.settle(Duration::from_secs(2));
    let from = site.child.stderr_len();
    let first = site.save("First sentence.", &hash_of(&site.site.join("index.md")));
    std::thread::sleep(Duration::from_millis(50));
    site.save("Second sentence.", &first);
    site.wait_for_home("Second sentence.", Duration::from_secs(3));
    site.settle(Duration::from_secs(2));
    assert!(site.home_page().contains("Second sentence."));
    let builds = site.count("build.summary", from);
    assert!((1..=2).contains(&builds), "{builds} builds for two saves; stderr:\n{}", joined(&site.child.stderr));
}

/// The server answers the editor while the first build still runs: a save
/// made after that build read the page is built by its own request.
#[test]
fn a_save_made_during_the_first_build_is_built_by_its_own_request() {
    let site = spawn(300, "Preview server ready on");
    wait_for_line(&site.child.stderr, "[render] html pages", Duration::from_secs(60))
        .unwrap_or_else(|| panic!("the first build never rendered:\n{}", joined(&site.child.stderr)));
    site.save("Saved sentence.", &hash_of(&site.site.join("index.md")));
    site.wait_for_home("Saved sentence.", Duration::from_secs(20));

    let lines = site.child.stderr.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let at = |needle: &str| lines.iter().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("no {needle:?} line:\n{}", lines.join("\n")));
    assert!(at("Rebuild asked by save") < at("build.summary"), "premise: the save was made during the first build:\n{}", lines.join("\n"));
}
