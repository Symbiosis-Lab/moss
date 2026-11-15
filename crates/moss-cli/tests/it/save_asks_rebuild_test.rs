//! Real-process proof that an editor save or upload under `build --serve
//! --watch` asks for its own rebuild instead of waiting for the file watcher
//! to report it. The watcher is made blind (`MOSS_TEST_WATCH_BLIND`) and the
//! sweep's tick is set far beyond the test (`MOSS_TEST_SWEEP_TICK_MS`), so
//! only the write's own request can rebuild the page. The site is opened through a symlink; the CLI
//! canonicalizes the folder, so the save and the rebuild worker agree on it.
#![cfg(unix)]

use crate::served_child;

use served_child::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const PAGE: &str = "---\ntitle: Home\n---\n\nOriginal sentence.\n";
/// A page naming a picture that is not in the folder yet.
const ALBUM: &str = "---\ntitle: Album\n---\n\n![[harbour.png]]\n";
const MISSING: &str = "Picture not found: harbour.png";
const ONE_PIXEL_PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89,
    0, 0, 0, 0x0d, b'I', b'D', b'A', b'T', 0x78, 0xda, 0x63, 0x64, 0x60, 0xf8, 0x5f, 0x0f, 0, 2, 0x87, 1, 0x80, 0xeb, 0x47, 0xba, 0x92,
    0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
];

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
    assert!(site.page("").contains("Original sentence."), "premise: the first build is served");
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
    std::fs::write(site.join("album.md"), ALBUM).unwrap();
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
    fn page(&self, path: &str) -> String {
        ureq::get(&format!("http://localhost:{}/{path}", self.port)).call().map(|r| r.into_string().unwrap_or_default()).unwrap_or_default()
    }

    /// Upload the picture the album names into the folder's root, the way the
    /// editor's file pick does.
    fn upload_harbour(&self) {
        let mut body = b"--moss-test\r\nContent-Disposition: form-data; name=\"file\"; filename=\"harbour.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
        body.extend_from_slice(ONE_PIXEL_PNG);
        body.extend_from_slice(b"\r\n--moss-test--\r\n");
        let reply: serde_json::Value = ureq::post(&format!("http://localhost:{}/__moss/upload", self.port))
            .set("Content-Type", "multipart/form-data; boundary=moss-test")
            .set("X-Moss-Token", &self.token)
            .send_bytes(&body)
            .unwrap_or_else(|e| panic!("the upload was refused: {e}"))
            .into_json()
            .unwrap();
        assert_eq!(reply["copied"][0]["final_name"], "harbour.png", "{reply}");
    }

    /// Wait until the album draws its picture.
    fn wait_for_harbour(&self, within: Duration) {
        self.wait_for("album/", "its uploaded picture", within, |album| album.contains("harbour") && !album.contains(MISSING));
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
        let reply = self.mutate("save_editor_content", payload);
        assert_eq!(reply["status"], "saved", "{reply}");
        reply["hash"].as_str().unwrap().to_string()
    }

    /// Run one of the editor's commands over the server's route for them.
    fn mutate(&self, command: &str, args: serde_json::Value) -> serde_json::Value {
        ureq::post(&format!("http://localhost:{}/__moss/mutate/{command}", self.port))
            .set("Content-Type", "application/json")
            .set("X-Moss-Token", &self.token)
            .send_string(&args.to_string())
            .unwrap_or_else(|e| match e {
                ureq::Error::Status(code, r) => panic!("{command} was refused ({code}): {}", r.into_string().unwrap_or_default()),
                e => panic!("{command} never reached the server: {e}"),
            })
            .into_json()
            .unwrap()
    }

    fn wait_for_home(&self, text: &str, within: Duration) {
        self.wait_for("", text, within, |home| home.contains(text));
    }

    /// Wait until the page at `path` passes `shows`; `what` names it in the failure.
    fn wait_for(&self, path: &str, what: &str, within: Duration, shows: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + within;
        while !shows(&self.page(path)) {
            assert!(Instant::now() < deadline, "the served page never showed {what:?} within {within:?}; stderr:\n{}", joined(&self.child.stderr));
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
    assert!(site.page("").contains("Second sentence."));
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

/// The page named the picture before it was in the folder, so no later save
/// asks for the build that draws it.
#[test]
fn an_uploaded_picture_is_drawn_with_the_watcher_blind() {
    let site = serve();
    assert!(site.page("album/").contains(MISSING), "premise: the album names a picture that is not there");
    site.upload_harbour();
    site.wait_for_harbour(Duration::from_secs(20));
}

/// A build that listed the folder before the upload cannot draw the picture,
/// so the upload's request is built after it.
#[test]
fn a_picture_uploaded_during_the_first_build_is_built_by_its_own_request() {
    let site = spawn(300, "Preview server ready on");
    wait_for_line(&site.child.stderr, "[render] html pages", Duration::from_secs(60))
        .unwrap_or_else(|| panic!("the first build never rendered:\n{}", joined(&site.child.stderr)));
    site.upload_harbour();
    site.wait_for_harbour(Duration::from_secs(20));

    let lines = site.child.stderr.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let at = |needle: &str| lines.iter().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("no {needle:?} line:\n{}", lines.join("\n")));
    assert!(at("Rebuild asked by new file") < at("build.summary"), "premise: the upload was made during the first build:\n{}", lines.join("\n"));
}

/// The album names the picture by its file name alone, so moving it rewrites
/// no page and no save asks for the build that draws it at its new address.
#[test]
fn a_moved_picture_is_drawn_at_its_new_address_with_the_watcher_blind() {
    let site = serve();
    site.upload_harbour();
    site.wait_for_harbour(Duration::from_secs(20));
    site.settle(Duration::from_secs(2));
    site.mutate("move_entries", serde_json::json!({ "paths": ["harbour.png"], "targetDir": "notes" }));
    assert!(site.site.join("notes/harbour.png").exists(), "premise: the picture moved");
    site.wait_for("album/", "the picture at its new address", Duration::from_secs(20), |album| album.contains("notes/harbour"));
}
