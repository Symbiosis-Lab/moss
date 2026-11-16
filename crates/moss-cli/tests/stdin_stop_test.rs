//! A program that starts `moss build --serve` and sets
//! `MOSS_STOP_ON_STDIN_CLOSE=1` takes the server down with it: the server
//! stops when its standard input closes. Without the variable a closed input
//! means nothing, since a shell job or a script starts moss that way.

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn serve_with_piped_stdin(site: &std::path::Path, moss_home: &std::path::Path, opt_in: bool) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_moss-cli"));
    command.args(["build"]).arg(site).args(["--serve", "--watch", "--no-plugins"]);
    if opt_in {
        command.env(moss_build::ops::stop::STOP_ON_STDIN_CLOSE, "1");
    }
    command
        .env("MOSS_HOME", moss_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn moss-cli build --serve")
}

/// Returns once the first build is over and the process only waits to be stopped.
fn wait_until_idle(child: &mut Child) {
    use std::io::BufRead;
    let stderr = std::io::BufReader::new(child.stderr.take().expect("piped stderr"));
    let (idle, seen) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in stderr.lines().map_while(Result::ok) {
            if line.contains("Press Ctrl+C") {
                let _ = idle.send(());
            }
        }
    });
    seen.recv_timeout(Duration::from_secs(60)).expect("the child never finished its first build");
}

async fn wait_until_serving(site: &std::path::Path, child: &Child) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while moss_build::ops::serve::ownership::find_live_owner(site).await.map(|o| o.pid) != Some(child.id()) {
        assert!(Instant::now() < deadline, "the child never served the folder");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn a_served_build_stops_when_its_host_closes_stdin_only_if_asked() {
    let _guard = MOSS_HOME_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let moss_home = tempfile::tempdir().expect("tempdir for MOSS_HOME");
    std::env::set_var("MOSS_HOME", moss_home.path());

    for opt_in in [true, false] {
        let site_root = tempfile::tempdir().expect("tempdir for the site copy");
        copy_dir_recursive(&fixture_input_dir(), site_root.path());
        let site = std::fs::canonicalize(site_root.path()).expect("canonicalize the site copy");

        let mut child = serve_with_piped_stdin(&site, moss_home.path(), opt_in);
        wait_until_serving(&site, &child).await;
        wait_until_idle(&mut child);
        drop(child.stdin.take());

        let exited = wait_for_exit(&mut child, Duration::from_secs(if opt_in { 30 } else { 3 }));
        assert_eq!(exited.is_some(), opt_in, "opt_in={opt_in}: exit status {exited:?}");
    }
    std::env::remove_var("MOSS_HOME");
}
