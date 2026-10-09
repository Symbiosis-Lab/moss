//! `moss rename --help` prints the usage and exits 0, as the other commands
//! that parse `--help` themselves do; the flag is not a path to rename.

use std::process::Command;

#[test]
fn rename_help_prints_usage_and_exits_zero() {
    for flag in ["--help", "-h"] {
        let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
            .args(["rename", flag])
            .output()
            .expect("run moss-cli rename --help");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "moss rename {flag} exits 0; got {:?}", out.status);
        assert!(stdout.contains("Usage"), "moss rename {flag} prints usage on stdout: {stdout}");
    }
}
