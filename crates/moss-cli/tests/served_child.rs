//! Shared real-process harness for spawning `moss-cli build --serve` and
//! reading its "Access at" line off stderr — used by `folder_owner_record_test.rs`
//! (the refusal path) and `standby_takeover_test.rs` (the `--watch` standby
//! path), so the two don't carry two copies of the same process-spawning
//! idioms. Not itself a test file: brought in via `#[path] mod served_child;`
//! rather than `tests/common/mod.rs`, since this tree never creates a
//! `mod.rs`.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Env vars are process-global and `cargo test` runs a file's tests in
/// parallel by default, so without this lock one test's `MOSS_HOME` override
/// (needed for THIS process's own `find_live_owner` calls, which do not go
/// through `Command::env`) can leak into another test's child process.
#[allow(dead_code)]
pub static MOSS_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

#[allow(dead_code)]
pub fn copy_dir_recursive(src: &Path, dst: &Path) {
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

/// Smallest snapshot-site corpus entry documented to build clean under
/// `--no-plugins`.
#[allow(dead_code)]
pub fn fixture_input_dir() -> PathBuf {
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

#[allow(dead_code)]
pub fn joined(lines: &Arc<Mutex<Vec<String>>>) -> String {
    lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).join("\n")
}

fn wait_for_line_from(
    lines: &Arc<Mutex<Vec<String>>>,
    needle: &str,
    skip: usize,
    timeout: Duration,
) -> Option<String> {
    let deadline = Instant::now() + timeout;
    loop {
        {
            let guard = lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(line) = guard.iter().skip(skip).find(|l| l.contains(needle)) {
                return Some(line.clone());
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[allow(dead_code)]
pub fn wait_for_line(lines: &Arc<Mutex<Vec<String>>>, needle: &str, timeout: Duration) -> Option<String> {
    wait_for_line_from(lines, needle, 0, timeout)
}

#[allow(dead_code)]
pub fn wait_for_exit(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
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

/// Spawn `moss-cli build <site_dir> --serve <extra_args...>` under
/// `moss_home`, piping both streams into background-drained buffers so
/// neither pipe can fill and stall the child.
#[allow(dead_code)]
pub struct ServedChild {
    pub child: Child,
    pub stdout: Arc<Mutex<Vec<String>>>,
    pub stderr: Arc<Mutex<Vec<String>>>,
    stdout_thread: Option<std::thread::JoinHandle<()>>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
}

#[allow(dead_code)]
pub fn spawn_served_build(site_dir: &Path, moss_home: &Path, extra_args: &[&str]) -> ServedChild {
    let mut child = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("build")
        .arg(site_dir)
        .arg("--serve")
        .args(extra_args)
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
    /// The build result (including the "Access at" line) prints via
    /// `cli_eprintln!` — stderr, not stdout.
    #[allow(dead_code)]
    pub fn wait_for_access_line(&self, timeout: Duration) -> String {
        self.wait_for_access_line_after(0, timeout)
    }

    /// Same, but only looks at stderr lines from index `skip` onward — so a
    /// caller that already consumed one "Access at" line (the owner it's
    /// standing by for) can wait specifically for the NEXT one, printed once
    /// this process takes over for real.
    #[allow(dead_code)]
    pub fn wait_for_access_line_after(&self, skip: usize, timeout: Duration) -> String {
        wait_for_line_from(&self.stderr, "Access at", skip, timeout).unwrap_or_else(|| {
            panic!(
                "child never printed an Access-at line after index {skip}; stdout:\n{}\nstderr:\n{}",
                joined(&self.stdout),
                joined(&self.stderr)
            )
        })
    }

    /// How many stderr lines have been captured so far — the `skip` value for
    /// a later [`Self::wait_for_access_line_after`] call that wants only
    /// lines printed from this point on.
    #[allow(dead_code)]
    pub fn stderr_len(&self) -> usize {
        self.stderr.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len()
    }

    #[allow(dead_code)]
    pub fn join_drain_threads(&mut self) {
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
#[allow(dead_code)]
pub fn port_from_url_line(line: &str) -> u16 {
    let after = line.rsplit(':').next().expect("line must contain a port after the last colon");
    after
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .unwrap_or_else(|e| panic!("could not parse a port out of {line:?}: {e}"))
}
