//! Real-process proof for `moss_build::ops::serve::ownership`: a second real
//! `moss` process pointed at a folder a first one is already serving must
//! find that out and refuse, rather than silently minting a second server on
//! a different port. Structured after the cross-process build lock's own
//! real-process proof in `folder_build_lock_test.rs`, which this reuses the
//! fixture and process-spawning idioms from. Shares its own process-spawning
//! idioms with `standby_takeover_test.rs` via `served_child.rs`.

#[path = "it/served_child.rs"]
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
    let verified = moss_build::ops::serve::ownership::discover_owner(&site_dir).await.unwrap().unwrap();
    assert_eq!(verified.health_version, env!("CARGO_PKG_VERSION"), "health reports the running product, not its library version");
    assert!(verified.capabilities.preview && verified.capabilities.authenticated_events);

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
    let named = format!("already served at {} by Cli (pid {},", owner.url, first.child.id());
    assert!(
        joined(&second.stderr).contains(&named),
        "the second process must name the existing owner ({named}) on stderr; got:\n{}",
        joined(&second.stderr)
    );

    // --- Kill the first like a crash: the lock releases, the record stays. ---
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

    // --- A start on another folder removes the killed server's record. ---
    let other_root = tempfile::tempdir().expect("tempdir for a second site copy");
    copy_dir_recursive(&fixture_input_dir(), other_root.path());
    let mut fourth = spawn_served_build(other_root.path(), moss_home.path(), &["--no-plugins"]);
    let _fourth_access_line = fourth.wait_for_access_line(Duration::from_secs(30));
    let records: Vec<_> = std::fs::read_dir(moss_home.path().join("servers"))
        .expect("the servers directory exists")
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(records.len(), 1, "only the running server keeps a record; found {records:?}");
    let other_owner = moss_build::ops::serve::ownership::find_live_owner(other_root.path())
        .await
        .unwrap_or_else(|| panic!("the fourth process names itself"));
    assert_eq!(other_owner.pid, fourth.child.id());

    fourth.child.kill().expect("kill the fourth child");
    let _ = fourth.child.wait();
    fourth.join_drain_threads();

    std::env::remove_var("MOSS_HOME");
}

/// A child test process waits at the same barrier as its peer, tries once, and
/// retains any guard until the parent has observed both results.
#[test]
fn ownership_contender() {
    let Some(root) = std::env::var_os("MOSS_OWNER_BARRIER") else { return };
    let root = std::path::PathBuf::from(root);
    let name = std::env::var("MOSS_OWNER_CONTENDER").unwrap();
    std::fs::write(root.join(format!("{name}.ready")), "ready").unwrap();
    wait_for_file(&root.join("go"));
    let result = moss_build::ops::serve::ownership::acquire(
        &root.join("site"), moss_build::ops::serve::ownership::HostKind::Cli,
        "test".into(), String::new(),
    );
    let outcome = match &result {
        Ok(_) => "acquired",
        Err(moss_build::ops::serve::ownership::AcquireError::Held) => "held",
        Err(error) => panic!("unexpected error: {error:?}"),
    };
    std::fs::write(root.join(format!("{name}.result")), outcome).unwrap();
    wait_for_file(&root.join("release"));
    drop(result);
}

fn wait_for_file(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        assert!(Instant::now() < deadline, "timed out waiting for {}", path.display());
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn simultaneous_process_starts_acquire_exactly_one_guard() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("site")).unwrap();
    let mut children = Vec::new();
    for name in ["a", "b"] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "ownership_contender", "--nocapture"])
            .env("MOSS_OWNER_BARRIER", root.path()).env("MOSS_OWNER_CONTENDER", name);
        children.push(spawn_piped(command, &root.path().join("home")));
    }
    for name in ["a", "b"] { wait_for_file(&root.path().join(format!("{name}.ready"))); }
    std::fs::write(root.path().join("go"), "go").unwrap();
    for name in ["a", "b"] { wait_for_file(&root.path().join(format!("{name}.result"))); }
    let mut outcomes: Vec<_> = ["a", "b"].iter().map(|name| {
        std::fs::read_to_string(root.path().join(format!("{name}.result"))).unwrap()
    }).collect();
    std::fs::write(root.path().join("release"), "release").unwrap();
    for child in &mut children {
        let status = wait_for_exit(&mut child.child, Duration::from_secs(20)).unwrap();
        child.join_drain_threads();
        assert!(status.success(), "{}", joined(&child.stderr));
    }
    outcomes.sort();
    assert_eq!(outcomes, ["acquired", "held"]);
}

#[test]
fn uncertain_owner_and_lock_failure_refuse_before_output_mutation() {
    let _guard = MOSS_HOME_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let home = tempfile::tempdir().unwrap();
    let site = tempfile::tempdir().unwrap();
    std::fs::write(site.path().join("index.md"), "# Must stay untouched\n").unwrap();
    std::env::set_var("MOSS_HOME", home.path());
    let foreign = moss_build::infra::folder_lock::try_acquire_named(site.path(), "owner").unwrap().unwrap();
    for serve in [true, false] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_moss-cli"));
        command.arg("build").arg(site.path()).arg("--no-plugins");
        if serve { command.arg("--serve"); }
        let mut child = spawn_piped(command, home.path());
        let status = wait_for_exit(&mut child.child, Duration::from_secs(30)).unwrap();
        child.join_drain_threads();
        assert_eq!(status.code(), Some(moss_build::ops::serve::ownership::ALREADY_SERVED_EXIT_CODE), "{}", joined(&child.stderr));
        assert!(!site.path().join(".moss").exists(), "uncertain owner must block even migrations and output setup");
    }
    drop(foreign);
    std::fs::remove_dir(home.path().join("locks")).unwrap();
    std::fs::write(home.path().join("locks"), "not a directory").unwrap();
    let mut child = spawn_served_build(site.path(), home.path(), &["--no-plugins"]);
    let status = wait_for_exit(&mut child.child, Duration::from_secs(30)).unwrap();
    child.join_drain_threads();
    assert_eq!(status.code(), Some(1), "storage errors are distinct from contention: {}", joined(&child.stderr));
    assert!(!site.path().join(".moss").exists(), "lock errors must prevent output setup");
    std::env::remove_var("MOSS_HOME");
}
