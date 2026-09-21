//! What the build root's identity log actually says, over a real build and a
//! real preview request.
//!
//! `root_identity.rs`'s own unit tests cover the pure pieces (the sibling
//! counter, the formatter); this file is what proves the call sites wired
//! into `run_pipeline` and the preview server actually fire, with the same
//! identity, end to end. Its own test binary, and one test, because
//! `log::set_logger` is process-global and one-shot (same reasoning as
//! `hash_index_tally_test.rs`).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

struct Capture(Mutex<Vec<String>>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.0.lock().unwrap().push(record.args().to_string());
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture(Mutex::new(Vec::new()));

fn lines_containing(needle: &str) -> Vec<String> {
    CAPTURE
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains(needle))
        .cloned()
        .collect()
}

/// The value of a `key=` field in a captured line, e.g. `field(line, "ino=")`.
fn field<'a>(line: &'a str, key: &str) -> &'a str {
    line.split_whitespace()
        .find_map(|tok| tok.strip_prefix(key))
        .unwrap_or_else(|| panic!("{key} missing from {line:?}"))
}

/// Wait until the runtime's spawned tasks have all finished — the detached
/// seal tail is one of them. `block_on`'s own root future is not counted
/// (only `tokio::spawn`ed work is), which is what makes this reach 0 rather
/// than spin until the timeout.
async fn until_quiet() {
    let metrics = tokio::runtime::Handle::current().metrics();
    for _ in 0..4000 {
        if metrics.num_alive_tasks() == 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!(
        "background seal work never finished: {} tasks alive",
        metrics.num_alive_tasks()
    );
}

/// One build, held long enough to see its own detached seal tail (mirrors a
/// `moss build --serve` process, which is exactly the scenario a cloud sync
/// client's rename-aside threatens: a build running, and a preview server
/// serving, at once) — then one real HTTP request against the output it
/// produced, missing on purpose, to exercise the preview server's own
/// observation of the root it is serving.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn build_and_serve_report_the_same_root_identity_through_every_phase() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Info);

    use moss_build::build::{null_sink, run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("moss-root-identity")
        .tempdir_in(&base)
        .unwrap();
    let folder = tmp.path().canonicalize().unwrap();
    std::fs::write(folder.join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
    let folder_str = folder.to_string_lossy().to_string();

    // The same Arc the build moves, so the server started below reads
    // wherever the build actually put its output.
    let served = Arc::new(RwLock::new(PathBuf::new()));
    let mut host = cli_host_ports(&folder_str);
    host.site_dir = Some(served.clone());

    run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&folder),
        progress: null_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host,
        trigger: BuildTrigger::Full,
        // False, as a `moss build --serve` process (which never exits right
        // after its build) sets it — the detached seal tail, not the inline
        // one-shot tail, is what a long-running preview process takes.
        exits_after_build: false,
        site_url_override: Some("https://example.com".to_string()),
        server_port: None,
        admission_epoch: None,
        live_port: None,
    })
    .await
    .expect("build");

    until_quiet().await;

    let start = lines_containing("build.root phase=start");
    let ship = lines_containing("build.root phase=ship");
    // `adopt_server` runs more than once per build (a cold-start check, then
    // again once the render actually shows) — the exact count is its own
    // business, not this test's; what matters here is that it logs at all,
    // and that a later request adds one more line on top.
    let served_before_404 = lines_containing("build.root phase=serve").len();

    assert_eq!(start.len(), 1, "{start:?}");
    assert_eq!(ship.len(), 1, "{ship:?}");
    assert!(served_before_404 >= 1, "adopt_server must log at least once per build");

    let ino = field(&start[0], "ino=");
    assert_eq!(
        field(&ship[0], "ino="),
        ino,
        "ship must report the same root the build started with"
    );
    assert!(ship[0].contains("swapped=false"), "{}", ship[0]);

    // Serve time: a real 404 through the real router.
    use moss_build::ops::serve::{start_server, ServeConfig};
    let (port, shutdown) = start_server(ServeConfig::new(served.clone(), 62_531))
        .await
        .expect("server starts");
    let response = reqwest::get(format!("http://127.0.0.1:{port}/this-page-does-not-exist"))
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
    let _ = shutdown.send(());

    let served_after_404 = lines_containing("build.root phase=serve");
    assert!(
        served_after_404.len() > served_before_404,
        "the 404 must add a serve-phase line: {served_after_404:?}"
    );
    let from_404 = served_after_404.last().unwrap();
    assert!(from_404.contains("marked="), "{from_404}");
    assert_eq!(
        field(from_404, "ino="),
        ino,
        "the 404 must report the same root the build produced"
    );
}
