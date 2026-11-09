//! The Windows half of the subtree stop. There is no `sh` to build a process
//! tree from, so the test binary re-runs itself as the child and the
//! grandchild ([`process_tree_role`]).
//!
//! The grandchild proves it is alive by growing a heartbeat file, so a test
//! reads "still running" directly instead of racing a timer against the
//! deadline.

use super::*;
use std::path::Path;
use std::time::{Duration, Instant};

const ROLE_ENV: &str = "MOSS_TEST_PROCESS_TREE_ROLE";
const DIR_ENV: &str = "MOSS_TEST_PROCESS_TREE_DIR";
const BEAT: Duration = Duration::from_millis(250);
/// Long against two cold starts of this binary under Wine, so the
/// grandchild is beating before the deadline. The `spawned` marker checks it.
const DEADLINE: Duration = Duration::from_secs(15);

fn role_command(role: &str, dir: &Path) -> std::process::Command {
    let module = module_path!().split_once("::").unwrap().1;
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args([
        &format!("{module}::process_tree_role"),
        "--exact",
        "--ignored",
        "--test-threads=1",
        "--nocapture",
    ])
    .env(ROLE_ENV, role)
    .env(DIR_ENV, dir);
    cmd
}

fn beats(dir: &Path) -> u64 {
    std::fs::metadata(dir.join("alive")).map_or(0, |m| m.len())
}

/// The child and grandchild of the tests below, never run on its own.
#[test]
#[ignore = "a process role for the tests in this module"]
fn process_tree_role() {
    let Ok(role) = std::env::var(ROLE_ENV) else {
        return;
    };
    let dir = std::path::PathBuf::from(std::env::var_os(DIR_ENV).unwrap());
    match role.as_str() {
        // Default stdio is inherited, so the grandchild holds the pipes moss
        // is draining. `spawned` is written only once it beats.
        "wedged" | "finishing" => {
            role_command("grandchild", &dir).spawn().unwrap();
            let waited = Instant::now();
            while beats(&dir) == 0 {
                assert!(waited.elapsed() < Duration::from_secs(60), "the grandchild never beat");
                std::thread::sleep(BEAT);
            }
            std::fs::write(dir.join("spawned"), "").unwrap();
            if role == "wedged" {
                std::thread::sleep(Duration::from_secs(120));
            }
        }
        // Bounded, and ends once its folder is gone, so a grandchild that
        // escaped does not outlive the test run by long.
        "grandchild" => {
            use std::io::Write;
            for _ in 0..240 {
                let beat = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("alive"))
                    .and_then(|mut f| f.write_all(b"."));
                if beat.is_err() {
                    return;
                }
                std::thread::sleep(BEAT);
            }
        }
        other => panic!("unknown role {other}"),
    }
}

/// Whether the heartbeat grows over a window long enough for several beats.
/// Termination is asynchronous, so a beat already under way is let land first.
fn still_beating(dir: &Path) -> bool {
    std::thread::sleep(BEAT * 2);
    let before = beats(dir);
    std::thread::sleep(BEAT * 8);
    beats(dir) > before
}

#[test]
fn a_wedged_child_cannot_hide_behind_a_grandchild_holding_the_pipes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let started = Instant::now();
    let err = run_bounded(&mut role_command("wedged", tmp.path()), DEADLINE, "wedged")
        .expect_err("a child past its deadline is a failure, not a hang");

    assert!(err.contains("timed out"), "says what happened: {err}");
    assert!(
        tmp.path().join("spawned").exists(),
        "the grandchild was not running by the deadline, so this proves nothing"
    );
    assert!(
        started.elapsed() < DEADLINE + Duration::from_secs(15),
        "returns instead of waiting on the grandchild (took {:?})",
        started.elapsed()
    );
    assert!(
        !still_beating(tmp.path()),
        "the whole job is stopped: a grandchild that outlives the deadline keeps both pipes open"
    );
}

#[test]
fn a_finished_child_leaves_what_it_started_running() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = run_bounded(
        &mut role_command("finishing", tmp.path()),
        Duration::from_secs(60),
        "finishing",
    )
    .expect("a child that exited is a result");

    assert!(out.status.success(), "{}", out.stderr);
    assert!(
        still_beating(tmp.path()),
        "a child that finished is not stopped, and neither is what it left running"
    );
}
