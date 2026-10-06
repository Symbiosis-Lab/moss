//! Real-process proof that a `--watch` child stands by for an existing owner
//! instead of refusing like a one-shot `moss build --serve` does
//! (`folder_owner_record_test.rs`), and takes over the moment that owner
//! goes away. Shares its process-spawning idioms with that file via
//! `served_child.rs`.

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::time::Duration;

#[tokio::test]
async fn a_watching_second_process_stands_by_and_takes_over_when_the_owner_dies() {
    let _guard = MOSS_HOME_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site_root = tempfile::tempdir().expect("tempdir for the site copy");
    copy_dir_recursive(&fixture_input_dir(), site_root.path());
    let site_dir = std::fs::canonicalize(site_root.path()).expect("canonicalize the site copy");

    // This process's own `find_live_owner` call below reads `MOSS_HOME`
    // directly, same as `folder_owner_record_test.rs`'s does.
    std::env::set_var("MOSS_HOME", moss_home.path());

    // --- A: starts clean, becomes the recorded owner. ---
    let mut a = spawn_served_build(&site_dir, moss_home.path(), &["--watch", "--no-plugins"]);
    let a_access_line = a.wait_for_access_line(Duration::from_secs(30));
    let a_port = port_from_url_line(&a_access_line);
    let a_url = format!("http://127.0.0.1:{a_port}");

    // --- B: same folder, also --watch — must stand by, not refuse and exit. ---
    let mut b = spawn_served_build(&site_dir, moss_home.path(), &["--watch", "--no-plugins"]);
    let b_first_line = b.wait_for_access_line(Duration::from_secs(30));
    assert!(b_first_line.contains(&a_url), "B's standby line must report A's own URL, got: {b_first_line}");

    let standing_by_line = wait_for_line(&b.stderr, "standing by", Duration::from_secs(10))
        .unwrap_or_else(|| panic!("B never printed its standing-by line; stderr:\n{}", joined(&b.stderr)));
    assert!(
        standing_by_line.contains("Cli"),
        "the standing-by line must name the owner's kind, got: {standing_by_line}"
    );

    // B must still be alive and waiting — A2's refusal would have exited by now.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        matches!(b.child.try_wait(), Ok(None)),
        "a --watch child must stand by on a conflict rather than exit; stderr:\n{}",
        joined(&b.stderr)
    );

    let lines_before_kill = b.stderr_len();

    // --- Kill A like a crash: B's poll must notice within its 2s interval and take over. ---
    a.child.kill().expect("kill A");
    let _ = a.child.wait();
    a.join_drain_threads();

    let b_second_line = b.wait_for_access_line_after(lines_before_kill, Duration::from_secs(10));
    assert!(
        !b_second_line.contains(&a_url),
        "B's takeover URL must be its own, not A's dead one, got: {b_second_line}"
    );

    let new_owner = moss_build::ops::serve::ownership::find_live_owner(&site_dir)
        .await
        .unwrap_or_else(|| panic!("find_live_owner found nothing after B's takeover"));
    assert_eq!(new_owner.pid, b.child.id(), "B must be the recorded owner once it takes over from the dead A");

    b.child.kill().expect("kill B");
    let _ = b.child.wait();
    b.join_drain_threads();

    std::env::remove_var("MOSS_HOME");
}
