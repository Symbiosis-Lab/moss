//! Real-process proof that `build --serve --watch` reconciles an edit its
//! watcher never acted on. The watcher is made blind (`MOSS_TEST_WATCH_BLIND`) and
//! the sweep's tick is shortened (`MOSS_TEST_SWEEP_TICK_MS`); the page is replaced
//! the way an atomic-rename save does it, and only the sweep can notice.

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

fn get(port: u16, path: &str) -> String {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn the_sweep_reconciles_an_atomic_rename_the_watcher_never_acted_on() {
    let moss_home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let site = std::fs::canonicalize(root.path()).unwrap().join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nOriginal sentence.\n").unwrap();

    // SAFETY: this binary holds one test, so nothing else reads the environment
    // concurrently; the child inherits both variables at spawn.
    unsafe {
        std::env::set_var("MOSS_TEST_WATCH_BLIND", "1");
        std::env::set_var("MOSS_TEST_SWEEP_TICK_MS", "300");
    }
    let child = spawn_served_build(&site, moss_home.path(), &["--watch", "--no-plugins"]);
    let line = child.wait_for_access_line(Duration::from_secs(60));
    let port = port_from_url_line(&line);
    wait_for_line(&child.stderr, "seal+persist", Duration::from_secs(60))
        .unwrap_or_else(|| panic!("first build never sealed:\n{}", joined(&child.stderr)));
    assert!(get(port, "/").contains("Original sentence."), "premise: the first build is served");

    // Atomic-rename save: write a temp file, rename it over the page.
    let tmp = site.join(".index.md.tmp");
    std::fs::write(&tmp, "---\ntitle: Home\n---\n\nReplaced sentence.\n").unwrap();
    std::fs::rename(&tmp, site.join("index.md")).unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if get(port, "/").contains("Replaced sentence.") {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the served page never picked up the rename; stderr:\n{}",
            joined(&child.stderr)
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}
