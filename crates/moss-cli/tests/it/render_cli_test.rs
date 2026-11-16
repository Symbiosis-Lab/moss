use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn render_stream_recovers_from_bad_lines_and_does_not_write_site_state() {
    let folder = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_moss-cli"))
        .args(["render", "--json"])
        .current_dir(folder.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "{{bad json}}").unwrap();
    writeln!(stdin, "{{\"id\":2,\"source\":\"---\\ntitle: T\\n---\\n# Hello\\n\"}}").unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stderr.is_empty(), "protocol warnings must not pollute stderr: {}", String::from_utf8_lossy(&output.stderr));
    let lines = String::from_utf8(output.stdout).unwrap();
    let rows = lines.lines().map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()).collect::<Vec<_>>();
    assert_eq!(rows.len(), 2, "stdout is one JSON object per input line");
    assert_eq!(rows[0]["id"], serde_json::Value::Null);
    assert!(rows[0]["error"].as_str().unwrap().contains("invalid JSON"));
    assert_eq!(rows[1]["id"], 2);
    assert!(rows[1]["html"].as_str().unwrap().contains("Hello"));
    assert!(rows[1]["css"].as_str().unwrap().contains("@layer tokens"));
    assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0, "render must not scan or write site state");
}
