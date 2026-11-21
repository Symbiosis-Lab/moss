use super::*;
use crate::ops::serve::{carrier_token, start_server, ServeConfig};

const CHILD_ROOT: &str = "MOSS_TEST_CARRIER_ANNOUNCEMENT_ROOT";
const CHILD_MODE: &str = "MOSS_TEST_CARRIER_ANNOUNCEMENT_MODE";
const CHILD_TEST: &str = "ops::serve::session::tests::carrier_announcement_child";
const PREFIX: &str = "HTTP carrier token file: ";

#[test]
fn carrier_announcement_normal() {
    check_announcement("normal");
}

#[test]
fn carrier_announcement_quiet() {
    check_announcement("quiet");
}

#[test]
fn carrier_announcement_failed_publication() {
    check_announcement("failure");
}

fn check_announcement(mode: &str) {
    let scratch = tempfile::tempdir().unwrap();
    // The token lives outside the vault: put escaping characters in its store
    // root too. Quotes are legal filename characters on Unix, not Windows.
    let name = if cfg!(unix) { "store with \"quotes\"" } else { "store with spaces" };
    let store = scratch.path().join(name);
    std::fs::create_dir(&store).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD_ROOT, scratch.path())
        .env(CHILD_MODE, mode)
        .env("TMPDIR", &store)
        .env("TMP", &store)
        .env("TEMP", &store)
        .output()
        .expect("re-exec the test binary to capture actual stderr");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "child failed: {stderr}");
    let lines: Vec<_> = stderr.lines().filter_map(|line| line.strip_prefix(PREFIX)).collect();
    if mode == "failure" {
        assert!(lines.is_empty(), "failed publication must not announce a token file");
        return;
    }
    assert_eq!(lines.len(), 1, "one announcement per publication, including quiet mode");
    let path: PathBuf = serde_json::from_str(lines[0]).expect("path is a JSON string");
    let expected = std::fs::read_to_string(scratch.path().join("expected-path.json")).unwrap();
    assert_eq!(lines[0], expected, "announce the resolved token_path as JSON");
    assert!(path.is_absolute());
    assert!(path.starts_with(&store));
    assert!(!path.starts_with(scratch.path().join("vault")));
    if cfg!(unix) {
        assert!(lines[0].contains("\\\"quotes\\\""), "JSON must escape quotes");
    }
    let token = std::fs::read_to_string(&path).expect("announced file is readable");
    assert!(!token.trim().is_empty(), "published token is available");
    assert!(!stderr.contains(token.trim()), "stderr must never contain token bytes");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(token.trim()), "stdout must never contain token bytes");
}

// Inert in the suite; only the parent re-exec enables this real server fixture.
#[test]
fn carrier_announcement_child() {
    let Some(root) = std::env::var_os(CHILD_ROOT).map(PathBuf::from) else { return; };
    let mode = std::env::var(CHILD_MODE).unwrap();
    let vault = root.join("vault");
    std::fs::create_dir_all(vault.join(".moss")).unwrap();
    let path = carrier_token::token_path(&vault);
    std::fs::write(root.join("expected-path.json"), serde_json::to_string(&path).unwrap()).unwrap();
    if mode == "failure" {
        std::fs::create_dir_all(&path).unwrap();
    }
    // Quiet is thread-local in unit tests, so bind on the thread holding it.
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let _quiet = (mode == "quiet").then(crate::build::cli_output::quiet);
        let ctx = InvokeCtx::standalone();
        let site_dir = Arc::new(RwLock::new(vault));
        let (port, _, shutdown) = start_server(ServeConfig {
            invoke: Some(ctx.clone()),
            ..ServeConfig::new(site_dir.clone(), 61950)
        }).await.expect("real carrier server starts");
        let first = ctx.bind(&site_dir).unwrap();
        assert!(Arc::ptr_eq(&first, &ctx.bind(&site_dir).unwrap()), "same vault reuses its session");
        if mode != "failure" {
            let published = std::fs::read_to_string(&path).unwrap();
            assert!(published.trim() == first.token().as_str(), "file holds the active token");
        }
        shutdown.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !crate::ops::serve::port::is_port_available(port) {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("server releases both listeners on shutdown");
    });
}
