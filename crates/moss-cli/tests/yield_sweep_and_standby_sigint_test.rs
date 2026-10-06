//! Real-process proofs for two edges of folder ownership under
//! `--serve --watch`: a process that yielded the folder must also stop its
//! reconciliation sweep (the new owner's sweep is the only one allowed to
//! react), and a process standing by for an owner must answer Ctrl-C.

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn get(port: u16, path: &str) -> String {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn generation(site: &Path) -> String {
    std::fs::read_to_string(site.join(".moss/build.nosync/current.generation")).unwrap_or_default()
}

/// The way an atomic-rename save replaces a page.
fn replace_page(site: &Path, body: &str) {
    let tmp = site.join(".index.md.tmp");
    std::fs::write(&tmp, format!("---\ntitle: Home\n---\n\n{body}\n")).unwrap();
    std::fs::rename(&tmp, site.join("index.md")).unwrap();
}

fn fresh_site() -> (tempfile::TempDir, tempfile::TempDir, std::path::PathBuf) {
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let root = tempfile::tempdir().expect("tempdir for the site");
    let site = std::fs::canonicalize(root.path()).unwrap().join("site");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.md"), "---\ntitle: Home\n---\n\nOriginal sentence.\n").unwrap();
    (moss_home, root, site)
}

#[tokio::test]
async fn a_yielded_process_stops_sweeping_and_the_next_owners_sweep_reacts() {
    let _guard = MOSS_HOME_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (moss_home, _root, site) = fresh_site();
    // SAFETY: the lock above serialises this file's tests; children inherit
    // the variables at spawn.
    unsafe {
        std::env::set_var("MOSS_HOME", moss_home.path());
        std::env::set_var("MOSS_TEST_WATCH_BLIND", "1");
        std::env::set_var("MOSS_TEST_SWEEP_TICK_MS", "300");
    }

    let a = spawn_served_build(&site, moss_home.path(), &["--watch", "--no-plugins"]);
    a.wait_for_access_line(Duration::from_secs(60));
    wait_for_line(&a.stderr, "seal+persist", Duration::from_secs(60))
        .unwrap_or_else(|| panic!("A never sealed its first build:\n{}", joined(&a.stderr)));

    let outcome = moss_build::ops::serve::ownership::request_yield(&site).await.expect("yield request");
    assert_eq!(outcome, moss_build::ops::serve::ownership::YieldOutcome::Accepted);
    wait_for_line(&a.stderr, "Yielding ownership", Duration::from_secs(10))
        .unwrap_or_else(|| panic!("A never printed its yielded line:\n{}", joined(&a.stderr)));

    // Only a sweep can notice this (the watcher is blind).
    let gen_before = generation(&site);
    let lines_before = a.stderr_len();
    replace_page(&site, "Replaced while yielded.");
    // Several 300ms ticks.
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(generation(&site), gen_before, "a yielded process promoted a new generation");
    let a_new_lines: Vec<String> = a.stderr.lock().unwrap().iter().skip(lines_before).cloned().collect();
    assert!(
        !a_new_lines.iter().any(|l| l.contains("seal+persist") || l.contains("promoted gen")),
        "a yielded process rebuilt; new stderr:\n{}",
        a_new_lines.join("\n")
    );

    // B takes over; its sweep is the only one left and must pick the edit up.
    let b = spawn_served_build(&site, moss_home.path(), &["--watch", "--no-plugins"]);
    let b_port = port_from_url_line(&b.wait_for_access_line(Duration::from_secs(60)));
    let deadline = Instant::now() + Duration::from_secs(15);
    while !get(b_port, "/").contains("Replaced while yielded.") {
        assert!(
            Instant::now() < deadline,
            "B's sweep never reacted; B stderr:\n{}",
            joined(&b.stderr)
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    unsafe {
        std::env::remove_var("MOSS_HOME");
        std::env::remove_var("MOSS_TEST_WATCH_BLIND");
        std::env::remove_var("MOSS_TEST_SWEEP_TICK_MS");
    }
}

#[tokio::test]
async fn ctrl_c_while_standing_by_exits_promptly_and_leaves_the_owner_alone() {
    let _guard = MOSS_HOME_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (moss_home, _root, site) = fresh_site();
    unsafe { std::env::set_var("MOSS_HOME", moss_home.path()) };

    let mut a = spawn_served_build(&site, moss_home.path(), &["--no-plugins"]);
    a.wait_for_access_line(Duration::from_secs(60));
    let owner = moss_build::ops::serve::ownership::find_live_owner(&site).await.expect("A owns the folder");
    assert_eq!(owner.pid, a.child.id());

    let mut b = spawn_served_build(&site, moss_home.path(), &["--watch", "--no-plugins"]);
    wait_for_line(&b.stderr, "standing by", Duration::from_secs(30))
        .unwrap_or_else(|| panic!("B never stood by:\n{}", joined(&b.stderr)));

    let status = Command::new("kill")
        .args(["-INT", &b.child.id().to_string()])
        .stdout(Stdio::null())
        .status()
        .expect("kill -INT");
    assert!(status.success());
    assert!(
        wait_for_exit(&mut b.child, Duration::from_secs(5)).is_some(),
        "B ignored Ctrl-C while standing by; stderr:\n{}",
        joined(&b.stderr)
    );

    let still = moss_build::ops::serve::ownership::find_live_owner(&site).await.expect("A must still own the folder");
    assert_eq!(still.pid, a.child.id(), "no owner record may be written by the process that stood by");
    assert!(matches!(a.child.try_wait(), Ok(None)), "A must still be live");

    unsafe { std::env::remove_var("MOSS_HOME") };
}
