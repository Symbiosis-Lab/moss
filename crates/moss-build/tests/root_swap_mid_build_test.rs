//! Whether a cloud sync client renaming `.moss/build.nosync` aside — to `build 2` —
//! mid-build survives the build: the output still lands under the RENAMED
//! directory (not the fresh, empty one the client puts back at the old
//! path), the generation still gets promoted, and a preview request
//! afterward still serves the built page.
//!
//! Modelled on `root_identity_log_test.rs`, which this file's setup mirrors;
//! where that one only reads the identity log, this one drives an actual
//! rename-aside from inside it. The rename fires the moment the "start" line
//! is logged — deterministic because `log::Log::log` runs synchronously, on
//! the build's own thread, the instant `log::info!` is called; no sleep, no
//! race against the pipeline's own timing.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

struct Capture {
    lines: Mutex<Vec<String>>,
    /// The `.moss/build.nosync` directory to swap aside the moment `phase=start` is
    /// logged, taken once so nothing can trigger it a second time.
    swap_at_start: Mutex<Option<PathBuf>>,
}

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        let line = record.args().to_string();
        if line.contains("build.root phase=start") {
            if let Some(build_dir) = self.swap_at_start.lock().unwrap().take() {
                // Exactly what a cloud sync client does: the live directory
                // keeps its inode under a new name, and a fresh directory —
                // holding only what the client itself would have re-created,
                // an empty `cache/` — takes the old one.
                let renamed = build_dir.with_file_name("build 2");
                std::fs::rename(&build_dir, &renamed).expect("rename aside");
                std::fs::create_dir_all(build_dir.join("cache")).expect("decoy cache/");
            }
        }
        self.lines.lock().unwrap().push(line);
    }
    fn flush(&self) {}
}

static CAPTURE: Capture = Capture {
    lines: Mutex::new(Vec::new()),
    swap_at_start: Mutex::new(None),
};

fn lines_containing(needle: &str) -> Vec<String> {
    CAPTURE
        .lines
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
/// seal tail is one of them.
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rename_aside_mid_build_still_promotes_under_the_renamed_directory() {
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
        .prefix("moss-root-swap")
        .tempdir_in(&base)
        .unwrap();
    let folder = tmp.path().canonicalize().unwrap();
    std::fs::write(folder.join("index.md"), "---\ntitle: Home\n---\n\nHello.\n").unwrap();
    let folder_str = folder.to_string_lossy().to_string();

    let build_dir = folder.join(".moss").join("build.nosync");
    let renamed_dir = build_dir.with_file_name("build 2");
    *CAPTURE.swap_at_start.lock().unwrap() = Some(build_dir.clone());

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
        // after its build) sets it — the detached seal tail is what carries
        // the promotion and the ship-time identity check this test watches.
        exits_after_build: false,
        site_url_override: Some("https://example.com".to_string()),
        server_port: None,
        admission_epoch: None,
        live_port: None,
    })
    .await
    .expect("build");

    until_quiet().await;

    // The swap actually happened — otherwise everything below would pass for
    // the wrong reason.
    assert!(
        renamed_dir.is_dir(),
        "the rename-aside must have run from the start-line hook"
    );
    assert!(
        build_dir.join("cache").is_dir(),
        "the decoy the sync client leaves behind"
    );

    let start = lines_containing("build.root phase=start");
    let ship = lines_containing("build.root phase=ship");
    assert_eq!(start.len(), 1, "{start:?}");
    assert_eq!(ship.len(), 1, "{ship:?}");
    assert!(ship[0].contains("swapped=true"), "{}", ship[0]);
    assert_ne!(
        field(&start[0], "ino="),
        field(&ship[0], "ino="),
        "the naive `.moss/build.nosync` path now names the decoy, a different inode"
    );

    // The build's actual output — the promoted generation — lands under the
    // RENAMED directory, never the fresh one the client put back at the old
    // name.
    let promoted_index = renamed_dir.join("current").join("index.html");
    assert!(
        promoted_index.exists(),
        "promotion must land under the renamed directory: {}",
        renamed_dir.display()
    );
    assert!(
        !build_dir.join("current").exists(),
        "the decoy must never receive the build's output"
    );

    // Scan's own cache write (the hash index) must also follow the renamed
    // root — the one hand-joined `.moss/build.nosync/cache` path this increment
    // routes through `MossPaths` instead of a bare join.
    assert!(
        renamed_dir.join("cache").join("hash-index.json").exists(),
        "the scan cache must follow the renamed root, not the decoy"
    );
    assert!(
        !build_dir.join("cache").join("hash-index.json").exists(),
        "the decoy's cache must stay untouched"
    );

    // A live preview request must serve the promoted page.
    use moss_build::ops::serve::{start_server, ServeConfig};
    let (port, shutdown) = start_server(ServeConfig::new(served.clone(), 62_532))
        .await
        .expect("server starts");
    let response = reqwest::get(format!("http://127.0.0.1:{port}/"))
        .await
        .expect("request completes");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let _ = shutdown.send(());
}
