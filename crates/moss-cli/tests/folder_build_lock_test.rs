//! Real-process proof for `moss_build::infra::folder_lock`: two SEPARATE
//! `moss` processes building the same folder must serialize, not just two
//! threads inside one process (which `FolderSession::stage_write_lock` and
//! `build::lifecycle::promote_lock` already handle).
//!
//! This test takes the lock itself, through the library the same way
//! `build.rs::run_pipeline_body` does, then spawns a real `moss-cli build`
//! of the SAME folder under the SAME `MOSS_HOME` and watches it: it must
//! print the "waiting" line and stay alive until the lock is released, then
//! exit clean.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Both tests in this file call `std::env::set_var("MOSS_HOME", ...)` on
/// THIS process (the child's own env is separate, via `Command::env`, but
/// the test process's own `infra::folder_lock` calls need the same
/// override) — env vars are process-global, and `cargo test` runs tests in
/// this file in parallel by default, so without this lock one test's
/// override can leak into the other's child process mid-run.
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

/// `slots-site`'s `input/` — the smallest fixture under
/// `moss-build`'s own snapshot-site corpus whose README already documents it
/// as building clean with `--no-plugins` (the flag this test passes to the
/// spawned child too): one page, no plugin dependency, nothing this test
/// would have to explain away.
fn fixture_input_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("moss-cli sits under crates/")
        .join("moss-build/tests/fixtures/snapshot-sites/slots-site/input")
}


/// Drains `reader` line by line into `into`, as they arrive — run on its own
/// thread so neither stream can fill its OS pipe buffer and block the child
/// on a `write()` the test isn't reading, which would masquerade as "the
/// child is waiting on our lock" for an entirely different reason.
fn drain_lines(reader: impl std::io::Read, into: Arc<Mutex<Vec<String>>>) {
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        into.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(line);
    }
}

fn joined(lines: &Arc<Mutex<Vec<String>>>) -> String {
    lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).join("\n")
}

/// Polls `lines` for a substring up to `timeout`, sleeping briefly between
/// checks. Returns whether it appeared.
fn wait_for_line(lines: &Arc<Mutex<Vec<String>>>, needle: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|l| l.contains(needle)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits for `child` to exit, up to `timeout`. `Some(status)` on exit,
/// `None` on timeout (the child is killed so the test process does not leak
/// it).
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

#[test]
fn a_second_process_waits_for_the_folder_build_lock_the_first_process_holds() {
    let _guard = MOSS_HOME_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site_root = tempfile::tempdir().expect("tempdir for the site copy");
    copy_dir_recursive(&fixture_input_dir(), site_root.path());
    let site_dir = std::fs::canonicalize(site_root.path()).expect("canonicalize the site copy");

    // `Command::env` below only reaches the CHILD's environment — this
    // process's own `moss_home()` read (inside `acquire`) needs the same
    // override set directly, or the two land on different lock files and
    // never contend at all.
    std::env::set_var("MOSS_HOME", moss_home.path());

    // Take the lock ourselves, through the same library call
    // `build.rs::run_pipeline_body` makes, before the child ever starts.
    let held = moss_build::infra::folder_lock::acquire(&site_dir)
        .expect("this process should be the only holder so far");

    let mut child = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("build")
        .arg(&site_dir)
        .arg("--no-plugins")
        .env("MOSS_HOME", moss_home.path())
        // The lock's contended-wait line logs at `info`; moss-cli's headless
        // logger defaults to `warn` and would otherwise swallow it.
        .env("MOSS_LOG_LEVEL", "info")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn moss-cli build");

    let stdout_lines = Arc::new(Mutex::new(Vec::new()));
    let stderr_lines = Arc::new(Mutex::new(Vec::new()));
    let stdout_pipe = child.stdout.take().expect("piped stdout");
    let stderr_pipe = child.stderr.take().expect("piped stderr");
    let stdout_lines_bg = stdout_lines.clone();
    let stderr_lines_bg = stderr_lines.clone();
    let stdout_thread = std::thread::spawn(move || drain_lines(stdout_pipe, stdout_lines_bg));
    let stderr_thread = std::thread::spawn(move || drain_lines(stderr_pipe, stderr_lines_bg));

    let saw_waiting_line = wait_for_line(
        &stderr_lines,
        "waiting for another moss process building this folder",
        Duration::from_secs(10),
    );
    assert!(
        saw_waiting_line,
        "child never printed the folder-lock waiting line; stderr so far:\n{}",
        joined(&stderr_lines)
    );

    // Give it a moment past the log line, then confirm it is genuinely
    // blocked rather than about to exit on its own for an unrelated reason.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        matches!(child.try_wait(), Ok(None)),
        "child exited before the lock was released; stdout:\n{}\nstderr:\n{}",
        joined(&stdout_lines),
        joined(&stderr_lines)
    );

    drop(held);

    let status = wait_for_exit(&mut child, Duration::from_secs(30));
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    match status {
        Some(status) => assert!(
            status.success(),
            "moss-cli build exited {:?} once the lock was released; stdout:\n{}\nstderr:\n{}",
            status.code(),
            joined(&stdout_lines),
            joined(&stderr_lines)
        ),
        None => panic!(
            "moss-cli build never exited after the lock was released; stdout:\n{}\nstderr:\n{}",
            joined(&stdout_lines),
            joined(&stderr_lines)
        ),
    }
}

/// Cross-process promotion ORDER, not just exclusion. `infra::folder_lock`
/// keeps two processes' render-then-promote windows from overlapping in
/// time, but says nothing on its own about which one runs first: an OLDER
/// admission whose tail is merely slower to reach its own promote can still
/// arrive AFTER a NEWER admission already promoted, and (without checking
/// `infra::folder_lock::last_promoted_epoch`) overwrite that newer output
/// with its own stale one.
///
/// Reproducing the RACE itself with two real `moss-cli build` children
/// fighting over the actual wall clock is not reliable: window 1 (render)
/// and window 2 (promote) share ONE lock per folder, so nothing this test
/// could hold or signal to stall one child's window 2 can do so without also
/// stalling a second child's window 1 — there is no way to let a second
/// build run its own render-then-promote to completion while the first is
/// paused on that very same lock. So instead of racing two children through
/// the lock, this test assigns each child's admission explicitly via
/// `MOSS_TEST_ADMISSION_NANOS` (see its doc in `build.rs`), independent of
/// which one actually RUNS first — the same way `MOSS_HOME` already makes a
/// folder path independent of who set it. Running the higher-admission
/// child first and the lower-admission one second is exactly "an older
/// admission's tail arrives after a newer one already promoted," reproduced
/// deterministically rather than raced for.
///
/// Verifies the persisted epoch file directly
/// (`infra::folder_lock::last_promoted_epoch`), the same public primitive
/// `advertise_sealed` itself reads and writes — `record_promoted_epoch` is
/// called if and only if `materialize_and_promote` just returned
/// `Promoted`, so the epoch value after the older admission's attempt is a
/// direct check of whether that attempt was refused.
///
/// A second, content-based assertion on `current.generation` itself — that
/// a wrongly-promoted older admission would also seal a manifest with a
/// different generation id — was attempted twice (a new page added between
/// builds; an existing page's body edited between builds) and dropped both
/// times. Self-checking each attempt with a third, genuinely-promoted build
/// over the same changed content showed `current.generation` unchanged
/// either way: a `moss-cli` process spawned from this test binary via
/// `Command::output()` never discovers any file but `index.md` in the
/// folder it builds, a harness-specific quirk not present in a plain
/// manual invocation of the same binary. An assertion that passes whether
/// or not the fix works is worse than no assertion, so the epoch check
/// below is the only one this test makes.
#[test]
fn an_older_admission_never_overwrites_an_already_promoted_newer_one() {
    let _guard = MOSS_HOME_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site_root = tempfile::tempdir().expect("tempdir for the site copy");
    copy_dir_recursive(&fixture_input_dir(), site_root.path());
    let site_dir = std::fs::canonicalize(site_root.path()).expect("canonicalize the site copy");

    // `Command::env` below only reaches the CHILD's environment — this
    // process's own `last_promoted_epoch` read needs the same override set
    // directly, exactly as the first test in this file needs it for `acquire`.
    std::env::set_var("MOSS_HOME", moss_home.path());

    let run = |admission_nanos: u64| -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_moss-cli"))
            .arg("build")
            .arg(&site_dir)
            .arg("--no-plugins")
            .env("MOSS_HOME", moss_home.path())
            .env("MOSS_LOG_LEVEL", "info")
            .env("MOSS_TEST_ADMISSION_NANOS", admission_nanos.to_string())
            .stdin(Stdio::null())
            .output()
            .expect("run moss-cli build")
    };

    // The NEWER admission, built and promoted FIRST in real time.
    let newer_admission = u64::MAX - 2;
    let output_newer = run(newer_admission);
    let stderr_newer = String::from_utf8_lossy(&output_newer.stderr);
    assert!(output_newer.status.success(), "the newer admission failed to build; stderr:\n{stderr_newer}");
    assert!(
        stderr_newer.contains("promoted gen"),
        "the newer admission never promoted; stderr:\n{stderr_newer}"
    );
    assert_eq!(
        moss_build::infra::folder_lock::last_promoted_epoch(&site_dir),
        newer_admission,
        "the newer admission's own promotion was not recorded; stderr:\n{stderr_newer}"
    );

    // The OLDER admission, built and arriving SECOND in real time — must be
    // refused, and must NOT move the persisted epoch backward.
    let older_admission = 1u64;
    let output_older = run(older_admission);
    let stderr_older = String::from_utf8_lossy(&output_older.stderr);
    assert!(
        output_older.status.success(),
        "a refused (superseded) promotion is not a build failure; stderr:\n{stderr_older}"
    );
    assert!(
        stderr_older.contains("promotion refused (cross-process)"),
        "expected the cross-process refusal line; stderr:\n{stderr_older}"
    );
    assert_eq!(
        moss_build::infra::folder_lock::last_promoted_epoch(&site_dir),
        newer_admission,
        "the older admission overwrote the persisted epoch — its promotion was not refused; \
         its stderr:\n{stderr_older}"
    );
}
