//! `moss describe` names no hosting domain: publishing is host-neutral, and a
//! domain in the contract text would be a second source of truth for it.

use std::process::Command;

#[test]
fn describe_summary_names_no_hosting_domain() {
    let out = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .arg("describe")
        .output()
        .expect("run moss-cli describe");
    assert!(out.status.success(), "moss describe exits 0; got {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let hits: Vec<&str> = stdout.lines().filter(|l| l.contains("mosspub.com")).collect();
    assert!(hits.is_empty(), "moss describe names mosspub.com: {hits:?}");
}
