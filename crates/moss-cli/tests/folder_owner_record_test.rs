//! Real-process proof for `moss_build::ops::serve::ownership`: a second real
//! `moss` process pointed at a folder a first one is already serving must
//! find that out and refuse, rather than silently minting a second server on
//! a different port. Structured after the cross-process build lock's own
//! real-process proof in `folder_build_lock_test.rs`, which this reuses the
//! fixture and process-spawning idioms from.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Env vars are process-global and `cargo test` runs this file's tests in
/// parallel by default, so without this lock one test's `MOSS_HOME` override
/// (needed for THIS process's own `find_live_owner` calls, which do not go
/// through `Command::env`) can leak into another test's child process.
static MOSS_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap_or_else(|e| panic!("create {}: {e}", dst.display()));
    for entry in std::fs::read_dir(src).unwrap_or_else(|e| panic!("read_dir {}: {e}", src.display())) {
        let entry = entry.unwrap();
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest);
        } else {
            std::fs::copy(&path, &dest).unwrap_or_else(|e| panic!("copy {}: {e}", path.display()));
        }
    }
}

/// Same fixture `folder_build_lock_test.rs` uses: smallest snapshot-site
/// corpus entry documented to build clean under `--no-plugins`.
fn fixture_input_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("moss-cli sits under crates/")
        .join("moss-build/tests/fixtures/snapshot-sites/slots-site/input")
}

fn drain_lines(reader: impl std::io::Read, into: Arc<Mutex<Vec<String>>>) {
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        into.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(line);
    }
}

fn joined(lines: &Arc<Mutex<Vec<String>>>) -> String {
    lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).join("\n")
}

fn wait_for_line(lines: &Arc<Mutex<Vec<String>>>, needle: &str, timeout: Duration) -> Option<String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(line) = lines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|l| l.contains(needle))
        {
            return Some(line.clone());
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Spawn `moss-cli build <site_dir> --serve --no-plugins` under `moss_home`,
/// piping both streams into background-drained buffers so neither pipe can
/// fill and stall the child.
struct ServedChild {
    child: Child,
    stdout: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<Vec<String>>>,
    stdout_thread: Option<std::thread::JoinHandle<()>>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
}

fn spawn_served_build(site_dir: &Path, moss_home: &Path) -> ServedChild {
    let mut child = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("build")
        .arg(site_dir)
        .arg("--serve")
        .arg("--no-plugins")
        .env("MOSS_HOME", moss_home)
        .env("MOSS_LOG_LEVEL", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn moss-cli build --serve");

    let stdout = Arc::new(Mutex::new(Vec::new()));
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let stdout_pipe = child.stdout.take().expect("piped stdout");
    let stderr_pipe = child.stderr.take().expect("piped stderr");
    let stdout_bg = stdout.clone();
    let stderr_bg = stderr.clone();
    let stdout_thread = std::thread::spawn(move || drain_lines(stdout_pipe, stdout_bg));
    let stderr_thread = std::thread::spawn(move || drain_lines(stderr_pipe, stderr_bg));

    ServedChild { child, stdout, stderr, stdout_thread: Some(stdout_thread), stderr_thread: Some(stderr_thread) }
}

impl ServedChild {
    fn wait_for_access_line(&self, timeout: Duration) -> String {
        // The build result (including the "Access at" line) prints via
        // `cli_eprintln!` — stderr, not stdout.
        wait_for_line(&self.stderr, "Access at", timeout).unwrap_or_else(|| {
            panic!(
                "child never printed its Access-at line; stdout:\n{}\nstderr:\n{}",
                joined(&self.stdout),
                joined(&self.stderr)
            )
        })
    }

    fn join_drain_threads(&mut self) {
        if let Some(t) = self.stdout_thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.stderr_thread.take() {
            let _ = t.join();
        }
    }
}

/// Pulls `http://127.0.0.1:<port>` (or `http://localhost:<port>`) out of a
/// line containing it, returning the port alone.
fn port_from_url_line(line: &str) -> u16 {
    let after = line.rsplit(':').next().expect("line must contain a port after the last colon");
    after
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .unwrap_or_else(|e| panic!("could not parse a port out of {line:?}: {e}"))
}

#[tokio::test]
async fn a_second_serve_on_the_same_folder_is_refused_and_a_third_recovers_after_a_crash() {
    let _guard = MOSS_HOME_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site_root = tempfile::tempdir().expect("tempdir for the site copy");
    copy_dir_recursive(&fixture_input_dir(), site_root.path());
    let site_dir = std::fs::canonicalize(site_root.path()).expect("canonicalize the site copy");

    // This process's own `find_live_owner` calls below read `MOSS_HOME`
    // directly, same as `folder_build_lock_test.rs`'s `acquire` call does.
    std::env::set_var("MOSS_HOME", moss_home.path());

    // --- First process: starts clean, becomes the recorded owner. ---
    let mut first = spawn_served_build(&site_dir, moss_home.path());
    let access_line = first.wait_for_access_line(Duration::from_secs(30));
    let first_port = port_from_url_line(&access_line);

    let owner = moss_build::ops::serve::ownership::find_live_owner(&site_dir)
        .await
        .unwrap_or_else(|| panic!("find_live_owner found nothing right after the first server reported ready"));
    assert_eq!(owner.url, format!("http://127.0.0.1:{first_port}"), "the record must name the port just reported");
    assert_eq!(owner.pid, first.child.id(), "the record must name the first child's own pid");

    // --- Second process: same folder, must be refused, not given a second port. ---
    let mut second = spawn_served_build(&site_dir, moss_home.path());
    let status = wait_for_exit(&mut second.child, Duration::from_secs(30))
        .unwrap_or_else(|| panic!("the conflicting second serve never exited; stderr:\n{}", joined(&second.stderr)));
    second.join_drain_threads();
    assert_eq!(
        status.code(),
        Some(moss_build::ops::serve::ownership::ALREADY_SERVED_EXIT_CODE),
        "a folder with a live owner must exit the distinct already-served code; stdout:\n{}\nstderr:\n{}",
        joined(&second.stdout),
        joined(&second.stderr),
    );
    assert!(
        joined(&second.stderr).contains("already served"),
        "the second process must name the existing owner on stderr; got:\n{}",
        joined(&second.stderr)
    );

    // --- Kill the first like a crash: the lock releases, the record does not. ---
    first.child.kill().expect("kill the first child");
    let _ = first.child.wait();
    first.join_drain_threads();

    // The OS releases the flock the instant the process dies; give that a
    // moment to be observable before asserting on it.
    let saw_dead = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if moss_build::ops::serve::ownership::find_live_owner(&site_dir).await.is_none() {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    assert!(saw_dead, "find_live_owner must go None once the recorded owner is killed");
    let record_path = moss_build::ops::serve::ownership::record_path_for_test(&site_dir);
    assert!(
        record_path.exists(),
        "the stale record file must survive a crash — only a clean Drop removes it"
    );

    // --- Third process: no live owner, so it must acquire and overwrite the stale record. ---
    let mut third = spawn_served_build(&site_dir, moss_home.path());
    // The first process is dead and its port is free again, so the third is
    // free to land on the exact same port — nothing here asserts about
    // which port, only about which process now owns the record.
    let _third_access_line = third.wait_for_access_line(Duration::from_secs(30));

    let new_owner = moss_build::ops::serve::ownership::find_live_owner(&site_dir)
        .await
        .unwrap_or_else(|| panic!("find_live_owner found nothing after the third process started"));
    assert_eq!(new_owner.pid, third.child.id(), "the third process must have overwritten the stale record");

    third.child.kill().expect("kill the third child");
    let _ = third.child.wait();
    third.join_drain_threads();

    std::env::remove_var("MOSS_HOME");
}
