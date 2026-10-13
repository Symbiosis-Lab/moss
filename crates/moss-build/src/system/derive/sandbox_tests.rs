//! Real children under the real sandbox. macOS only: elsewhere there is no
//! confinement to test.
#![cfg(target_os = "macos")]

use super::*;
use crate::system::bounded_command::run_bounded;
use std::time::Duration;

fn scratch() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("moss-sandbox-test-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run_sh(script: &str, scratch: &Path) -> crate::system::bounded_command::BoundedOutput {
    let mut cmd = command(Path::new("/bin/sh"), scratch);
    cmd.arg("-c").arg(script);
    run_bounded(&mut cmd, Duration::from_secs(30), "sandboxed sh").expect("the sandbox must start")
}

#[test]
fn the_sandbox_denies_network_and_writes_outside_scratch_but_allows_scratch() {
    let scratch = scratch();
    // A sibling of the scratch dir, never the real home directory.
    let outside = tempfile::TempDir::new().unwrap();
    let home_probe = outside.path().canonicalize().unwrap().join("probe");

    // A local listener, so a successful connect proves the network is open and
    // a refusal cannot be blamed on the internet being down.
    let port = serve_ok();

    let out = run_sh(
        &format!(
            "echo ok > \"$TMPDIR/inside\" && echo inside=ok; \
             echo no > '{home}' 2>/dev/null && echo home=WROTE || echo home=denied; \
             /usr/bin/curl -s -m 5 http://127.0.0.1:{port}/ >/dev/null 2>&1 && echo net=OPEN || echo net=denied",
            home = home_probe.display()
        ),
        &scratch,
    );
    assert!(!home_probe.exists());

    assert!(out.stdout.contains("inside=ok"), "scratch is writable: {out:?}");
    assert!(out.stdout.contains("home=denied"), "a write outside scratch is refused: {out:?}");
    assert!(out.stdout.contains("net=denied"), "the network is refused: {out:?}");
    assert!(scratch.join("inside").exists());
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn the_control_without_the_sandbox_can_write_home_and_reach_the_network() {
    // Without this, "denied" above could just mean the probes are broken.
    let outside = tempfile::TempDir::new().unwrap();
    let home_probe = outside.path().join("control");
    let port = serve_ok();
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(format!(
        "echo no > '{h}' && echo home=WROTE; /usr/bin/curl -s -m 5 http://127.0.0.1:{port}/ >/dev/null 2>&1 && echo net=OPEN",
        h = home_probe.display()
    ));
    let out = run_bounded(&mut cmd, Duration::from_secs(30), "control").unwrap();
    assert!(out.stdout.contains("home=WROTE"), "{out:?}");
    assert!(out.stdout.contains("net=OPEN"), "{out:?}");
}

#[test]
fn the_sandbox_cannot_signal_other_processes_and_a_control_can() {
    let scratch = scratch();
    let mut victim = std::process::Command::new("/bin/sleep").arg("60").spawn().unwrap();
    let script = format!("kill -0 {} 2>/dev/null && echo sig=SENT || echo sig=denied", victim.id());
    let confined = run_sh(&script, &scratch);
    let mut control = Command::new("/bin/sh");
    control.arg("-c").arg(&script);
    let control = run_bounded(&mut control, Duration::from_secs(30), "control").unwrap();
    let _ = victim.kill();
    let _ = victim.wait();
    assert!(control.stdout.contains("sig=SENT"), "{control:?}");
    assert!(confined.stdout.contains("sig=denied"), "{confined:?}");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn a_real_binary_that_loads_system_libraries_still_runs_confined() {
    let scratch = scratch();
    let mut cmd = command(Path::new("/usr/bin/curl"), &scratch);
    cmd.arg("--version");
    let out = run_bounded(&mut cmd, Duration::from_secs(30), "sandboxed curl").unwrap();
    assert!(out.status.success() && out.stdout.starts_with("curl "), "{out:?}");
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn the_environment_is_cleared_to_an_allowlist() {
    let scratch = scratch();
    // A unique variable name no other test reads.
    std::env::set_var("MOSS_SANDBOX_TEST_SECRET", "s3cret");
    let out = run_sh("echo \"secret=[$MOSS_SANDBOX_TEST_SECRET] tmp=$TMPDIR\"", &scratch);
    assert!(out.stdout.contains("secret=[]"), "{out:?}");
    assert!(out.stdout.contains(&format!("tmp={}", scratch.display())), "{out:?}");
    let _ = std::fs::remove_dir_all(&scratch);
}

/// A local server that answers every request `200`, so a probe succeeds
/// exactly when the network is reachable. Returns its port.
fn serve_ok() -> u16 {
    use std::io::Write;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    port
}

