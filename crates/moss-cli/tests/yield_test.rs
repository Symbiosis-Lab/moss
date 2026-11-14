//! Real-process proof that `ownership::request_yield` makes a `--watch`
//! owner give the folder up to whoever claims it next, while a page of its
//! preview is still open, and take it back when that process leaves — the HTTP-requested
//! handoff alongside the owner-record/standby family `folder_owner_record_test.rs`
//! (refusal) and `standby_takeover_test.rs` (takeover-on-crash) already
//! cover. Shares the same process-spawning idioms with both via
//! `served_child.rs`.

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::time::{Duration, Instant};

/// A previewed page keeps `/__moss/reload` open for as long as it is on
/// screen, so a server that waited for its connections would never let go.
#[tokio::test]
async fn a_yielded_folder_goes_to_the_next_process_and_comes_back_with_a_page_open() {
    use moss_build::ops::serve::ownership::{find_live_owner, request_yield, YieldOutcome};
    use std::io::{Read, Write};

    let _guard = MOSS_HOME_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    let site_root = tempfile::tempdir().expect("tempdir for the site copy");
    copy_dir_recursive(&fixture_input_dir(), site_root.path());
    let site_dir = std::fs::canonicalize(site_root.path()).expect("canonicalize the site copy");
    // This process's own `find_live_owner` and `request_yield` read it.
    std::env::set_var("MOSS_HOME", moss_home.path());

    let mut a = spawn_served_build(&site_dir, moss_home.path(), &["--watch", "--no-plugins"]);
    let a_port = port_from_url_line(&a.wait_for_access_line(Duration::from_secs(30)));
    assert_eq!(find_live_owner(&site_dir).await.expect("A owns the folder").pid, a.child.id());

    let mut page = std::net::TcpStream::connect(("127.0.0.1", a_port)).expect("connect the open page");
    write!(page, "GET /__moss/reload HTTP/1.1\r\nHost: 127.0.0.1:{a_port}\r\n\r\n").expect("open the reload stream");

    // And a request still arriving, which A cannot end: it must not hold the folder either.
    let mut slow = std::net::TcpStream::connect(("127.0.0.1", a_port)).expect("connect the slow request");
    write!(slow, "GET / HTTP/1.1\r\nHost: 127.0.0.1:{a_port}\r\n").expect("send half a request");

    // B is already standing by, as a second editor's preview would be, so
    // the five seconds below hold no process start.
    let mut b = spawn_served_build(&site_dir, moss_home.path(), &["--watch", "--no-plugins"]);
    wait_for_line(&b.stderr, "standing by", Duration::from_secs(30))
        .unwrap_or_else(|| panic!("B never stood by; stderr:\n{}", joined(&b.stderr)));

    assert_eq!(request_yield(&site_dir).await.expect("yield request"), YieldOutcome::Accepted);
    let yielded = Instant::now();
    let a_lines_before_b = a.stderr_len();

    let b_url = loop {
        match find_live_owner(&site_dir).await {
            Some(owner) if owner.pid == b.child.id() => break owner.url,
            _ if yielded.elapsed() > Duration::from_secs(5) => panic!(
                "B did not get the folder within five seconds of the yield; A:\n{}\nB:\n{}",
                joined(&a.stderr),
                joined(&b.stderr)
            ),
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    };

    // A ends the stream it was told to stop serving, so the page can find B.
    page.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut rest = Vec::new();
    let ended = page.read_to_end(&mut rest);
    assert!(ended.is_ok(), "A kept the page's stream open after yielding ({:?} on): {ended:?}", yielded.elapsed());

    let a_ready = a.wait_for_access_line_after(a_lines_before_b, Duration::from_secs(10));
    assert!(a_ready.contains(&b_url), "A's stand-by line must give B's address, got: {a_ready}");
    wait_for_line(&a.stderr, "standing by", Duration::from_secs(10))
        .unwrap_or_else(|| panic!("A never stood by; stderr:\n{}", joined(&a.stderr)));

    let a_lines_before_return = a.stderr_len();
    b.child.kill().expect("kill B");
    let _ = b.child.wait();
    b.join_drain_threads();

    let a_again = port_from_url_line(&a.wait_for_access_line_after(a_lines_before_return, Duration::from_secs(15)));
    let owner = find_live_owner(&site_dir).await.expect("A is found again once it serves again");
    assert_eq!((owner.pid, owner.url), (a.child.id(), format!("http://127.0.0.1:{a_again}")));

    drop(slow);
    a.child.kill().expect("kill A");
    let _ = a.child.wait();
    a.join_drain_threads();
    std::env::remove_var("MOSS_HOME");
}
