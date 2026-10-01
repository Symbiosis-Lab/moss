//! Real-process proof for `moss_build::ops::serve::ownership`: a second real
//! `moss` process pointed at a folder a first one is already serving must
//! find that out and refuse, rather than silently minting a second server on
//! a different port. Structured after the cross-process build lock's own
//! real-process proof in `folder_build_lock_test.rs`, which this reuses the
//! fixture and process-spawning idioms from. Shares its own process-spawning
//! idioms with `standby_takeover_test.rs` via `served_child.rs`.

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::time::{Duration, Instant};

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
    let mut first = spawn_served_build(&site_dir, moss_home.path(), &["--no-plugins"]);
    let access_line = first.wait_for_access_line(Duration::from_secs(30));
    let first_port = port_from_url_line(&access_line);

    let owner = moss_build::ops::serve::ownership::find_live_owner(&site_dir)
        .await
        .unwrap_or_else(|| panic!("find_live_owner found nothing right after the first server reported ready"));
    assert_eq!(owner.url, format!("http://127.0.0.1:{first_port}"), "the record must name the port just reported");
    assert_eq!(owner.pid, first.child.id(), "the record must name the first child's own pid");

    // --- Second process: same folder, must be refused, not given a second port. ---
    let mut second = spawn_served_build(&site_dir, moss_home.path(), &["--no-plugins"]);
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
    let mut third = spawn_served_build(&site_dir, moss_home.path(), &["--no-plugins"]);
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
