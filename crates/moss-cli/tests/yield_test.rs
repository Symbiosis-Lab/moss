//! Real-process proof that `ownership::request_yield` makes a `--watch`
//! owner give the folder up to whoever claims it next — the HTTP-requested
//! handoff alongside the owner-record/standby family `folder_owner_record_test.rs`
//! (refusal) and `standby_takeover_test.rs` (takeover-on-crash) already
//! cover. Shares the same process-spawning idioms with both via
//! `served_child.rs`.

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::time::{Duration, Instant};

#[tokio::test]
async fn request_yield_frees_the_folder_and_the_yielder_reports_the_next_owner() {
    let _guard = MOSS_HOME_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site_root = tempfile::tempdir().expect("tempdir for the site copy");
    copy_dir_recursive(&fixture_input_dir(), site_root.path());
    let site_dir = std::fs::canonicalize(site_root.path()).expect("canonicalize the site copy");

    // `request_yield`'s own `find_live_owner` calls below read `MOSS_HOME`
    // directly, same as the sibling real-process tests' do.
    std::env::set_var("MOSS_HOME", moss_home.path());

    // --- A: starts clean, becomes the recorded owner. ---
    let mut a = spawn_served_build(&site_dir, moss_home.path(), &["--watch", "--no-plugins"]);
    let _a_access_line = a.wait_for_access_line(Duration::from_secs(30));

    let owner_before = moss_build::ops::serve::ownership::find_live_owner(&site_dir)
        .await
        .unwrap_or_else(|| panic!("A must be the recorded owner before any yield"));
    assert_eq!(owner_before.pid, a.child.id());

    // --- Ask A to yield. ---
    let outcome = moss_build::ops::serve::ownership::request_yield(&site_dir)
        .await
        .unwrap_or_else(|e| panic!("request_yield could not reach A: {e}"));
    assert_eq!(
        outcome,
        moss_build::ops::serve::ownership::YieldOutcome::Accepted,
        "A is a Cli owner with no --strict conflicts; the yield must be accepted"
    );

    let _yielded_line = wait_for_line(&a.stderr, "Yielding ownership", Duration::from_secs(10))
        .unwrap_or_else(|| panic!("A never printed its yielded line; stderr:\n{}", joined(&a.stderr)));

    // --- A must drop ownership within five seconds. ---
    let dropped = {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if moss_build::ops::serve::ownership::find_live_owner(&site_dir).await.is_none() {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    assert!(dropped, "A must no longer be a live owner within five seconds of yielding");

    let a_lines_before_b = a.stderr_len();

    // --- B: a fresh process on the same folder, now free to acquire. ---
    let mut b = spawn_served_build(&site_dir, moss_home.path(), &["--watch", "--no-plugins"]);
    let b_access_line = b.wait_for_access_line(Duration::from_secs(30));
    let b_port = port_from_url_line(&b_access_line);
    let b_url = format!("http://127.0.0.1:{b_port}");

    // --- A must report B's own URL and stand by for it. ---
    let a_second_line = a.wait_for_access_line_after(a_lines_before_b, Duration::from_secs(10));
    assert!(a_second_line.contains(&b_url), "A's post-yield line must report B's own URL, got: {a_second_line}");
    let standing_by_line = wait_for_line(&a.stderr, "standing by", Duration::from_secs(10))
        .unwrap_or_else(|| panic!("A never printed its standing-by line; stderr:\n{}", joined(&a.stderr)));
    assert!(standing_by_line.contains("Cli"), "the standing-by line must name B's kind, got: {standing_by_line}");

    a.child.kill().expect("kill A");
    let _ = a.child.wait();
    a.join_drain_threads();
    b.child.kill().expect("kill B");
    let _ = b.child.wait();
    b.join_drain_threads();

    std::env::remove_var("MOSS_HOME");
}
