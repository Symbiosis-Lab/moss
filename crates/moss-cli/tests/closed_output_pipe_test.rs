//! `moss … | head` must end quietly when the reader stops reading, the way
//! other command-line tools do, instead of panicking (and, in a release
//! build, sending a crash report). Spawns the real binary with an output
//! pipe whose read end is already closed, so the very first write fails.
#![cfg(unix)]

use std::process::{Command, Stdio};

/// The status a shell reports for a command ended by a closed pipe (128 + SIGPIPE).
const CLOSED_PIPE_STATUS: i32 = 141;

fn run_into_closed_pipe(args: &[&str]) -> std::process::Output {
    let (reader, writer) = std::io::pipe().expect("pipe");
    drop(reader);
    Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(args)
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .expect("spawn moss-cli")
}

#[test]
fn printing_into_a_closed_pipe_exits_quietly() {
    for args in [&["--help"][..], &["guide"], &["describe", "--json"]] {
        let out = run_into_closed_pipe(args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("panicked"), "moss {args:?} panicked:\n{stderr}");
        assert_eq!(out.status.code(), Some(CLOSED_PIPE_STATUS), "moss {args:?}: {stderr}");
    }
}

/// A full disk is not a reader that stopped: the output is truncated, so the
/// failure must still say so.
#[cfg(target_os = "linux")]
#[test]
fn printing_onto_a_full_disk_still_fails_loudly() {
    let full = std::fs::OpenOptions::new().write(true).open("/dev/full").expect("/dev/full");
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("--help")
        .stdout(full)
        .stderr(Stdio::piped())
        .output()
        .expect("spawn moss-cli");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_ne!(out.status.code(), Some(CLOSED_PIPE_STATUS), "{stderr}");
    assert!(stderr.contains("No space left on device"), "{stderr}");
}
