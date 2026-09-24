//! Regression test for the epoch-mint-order half of the open-double-build
//! fix (part 2, thermo-review hardening on commit 9b795440b9).
//!
//! `build_folder` (in the desktop app's build shell) mints its `admission_epoch`
//! immediately, before its own `run_pipeline` call, instead of leaving it
//! `None` for that call's post-hoc fallback — because once a worker exists
//! from folder-open onward (part 1 of the same fix), a worker-admitted
//! rebuild for the SAME folder can mint an admission-time epoch before the
//! open build's own post-hoc mint would have run. `lifecycle::promote`'s epoch
//! guard is unchanged and already thoroughly tested, but with
//! HAND-PICKED epoch values (`try_promote_refuses_a_repeat_of_the_epoch_
//! already_on_current` and neighbors) — what those cannot catch is whether
//! the open build's REAL epoch ends up lower than a concurrently-admitted
//! worker build's, regardless of which one's pipeline work finishes first.
//! This test drives that through the real `run_pipeline` ->
//! `materialize_and_promote` path, not a hand-picked `lifecycle::promote` call,
//! mirroring the two callers' actual mint-order, and reads the answer off
//! `current` — the generation the preview serves and deploy publishes.
//!
//! Boundary, stated plainly: this does not invoke the `#[tauri::command]`
//! `build_folder` itself — this repo has no mock-Tauri-app test harness, and
//! building one for a single test crosses the ladder's "does this need to
//! exist" line. It reconstructs the two calls' RELATIVE epoch-mint order
//! exactly as the desktop app's fixed code produces it (open mints first, a
//! worker admission mints later — see `next_promotion_epoch`'s own doc:
//! "call in build order"), then drives both builds through the real
//! seal/promotion machinery this crate owns.

use crate::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
use crate::vault_root::VaultRoot;

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    // Under target/, like editor/resolve/links.rs's own pipeline test: the
    // system tempdir can itself be under a moss-managed folder on some
    // machines, which would make this test pass/fail for the wrong reason.
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-tmp");
    std::fs::create_dir_all(&base).unwrap();
    let tmp = tempfile::Builder::new()
        .prefix("moss-epoch-order")
        .tempdir_in(&base)
        .unwrap();
    let folder = tmp.path().to_path_buf();
    std::fs::write(folder.join("index.md"), "# Home\n\nbody\n").unwrap();
    (tmp, folder)
}

fn cfg(folder: &std::path::Path, epoch: u64) -> PipelineConfig {
    PipelineConfig {
        root: VaultRoot::resolve(folder),
        progress: crate::build::null_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        // shell_attached: false + exits_after_build: true routes the seal
        // (materialize_and_promote) through run_pipeline's
        // SYNCHRONOUS arm (build.rs's "No app context..." branch) instead of
        // a detached background task — the whole point here, since the test
        // needs to control exactly when each build's promotion attempt runs.
        host: crate::build::ports::host::test_host_ports(),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: Some(epoch),
        live_port: None,
    }
}

fn any_output_path_contains(dir: &std::path::Path, needle: &str) -> bool {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .any(|e| e.path().to_string_lossy().contains(needle))
}

/// Runs the scenario: a "worker" build and an "open" build of the same
/// folder. The worker's build always completes and promotes FIRST (real
/// wall-clock order in the test); the epoch each mints is controlled by
/// `open_epoch_is_lower`, matching whichever of the two call-order shapes is
/// under test. Returns whether the worker's exclusive page is in the
/// generation `current` points at once the open build's later seal has run.
///
/// `current`, not `staging`: staging is shared mutable scratch that the next
/// build overwrites, and reading the answer off it only ever worked because
/// a Superseded tail happened to skip the stale sweep. What the promotion
/// guard actually decides is which frozen generation the preview serves and
/// deploy publishes, and that is what this asks.
async fn worker_page_survives_a_later_open_seal(open_epoch_is_lower: bool) -> bool {
    let (_tmp, folder) = fixture();
    // In production the open build's lease holds this folder's lifecycle
    // record (and its promotion epoch) from before the worker build promotes;
    // run back to back here, the test holds it instead, or a parallel test's
    // folder lookup could evict it between the two builds.
    let _record = crate::build::lifecycle::lock_for(&crate::moss_paths::MossPaths::new(&folder));

    // Mint order matches production: whichever call happens first gets the
    // lower epoch from the SAME monotonic counter `next_promotion_epoch` —
    // build_folder's own early mint and a worker's admission-time mint both
    // call it directly, with nothing hand-picked.
    let (open_epoch, worker_epoch) = if open_epoch_is_lower {
        let o = crate::build::ship::next_promotion_epoch();
        let w = crate::build::ship::next_promotion_epoch();
        (o, w)
    } else {
        let w = crate::build::ship::next_promotion_epoch();
        let o = crate::build::ship::next_promotion_epoch();
        (o, w)
    };

    // The worker's rebuild captures a page the open build's own (earlier,
    // slower-to-finish) scan never saw.
    let extra = folder.join("worker-only.md");
    std::fs::write(&extra, "# Extra\n\nbody\n").unwrap();
    run_pipeline(cfg(&folder, worker_epoch)).await.expect("worker build");

    let mp = crate::moss_paths::MossPaths::new(&folder);
    assert!(
        any_output_path_contains(&mp.staging_dir(), "worker-only"),
        "precondition: the worker's build must have published its page"
    );

    // The open build's own scan never saw the extra page — remove it before
    // running the "slower" open build's pipeline, so its manifest reflects
    // the OLDER view its real scan would have captured, exactly as a build
    // that started before the file existed and took longer to finish would.
    std::fs::remove_file(&extra).unwrap();
    run_pipeline(cfg(&folder, open_epoch)).await.expect("open build");

    // Resolve the symlink: walkdir does not descend through one.
    let current = std::fs::canonicalize(mp.current_ptr()).expect("current must point somewhere");
    any_output_path_contains(&current, "worker-only")
}

/// The scenario the thermo review named: the worker-admitted build finishes
/// and promotes first; the open build's epoch — lower, because it was
/// minted first, in true admission order — must be refused as Superseded
/// when it finishes after, so `current` keeps pointing at the worker's
/// generation.
#[tokio::test(flavor = "multi_thread")]
async fn open_builds_lower_epoch_is_refused_and_current_keeps_the_workers_page() {
    assert!(
        worker_page_survives_a_later_open_seal(true).await,
        "the open build's epoch, minted first (lower), must be refused when \
         it finishes after an already-promoted worker build — `current` must \
         still be the worker's generation, page and all"
    );
}

/// The failure mode this whole fix prevents, demonstrated directly rather
/// than asserted only by absence: if the open build's epoch were instead
/// the HIGHER one — which is what `build_folder`'s OLD post-hoc mint could
/// produce, since it can land on either side of a concurrent worker
/// admission's own early mint — its later, stale seal wrongly promotes, and
/// `current` becomes a generation rendered from a scan that never saw the
/// worker's page. A canary: if this ever starts passing the OTHER way
/// (survives), something in `lifecycle::promote`'s epoch comparison changed, not
/// just this ordering.
#[tokio::test(flavor = "multi_thread")]
async fn a_higher_open_epoch_would_wrongly_promote_over_the_workers_generation() {
    assert!(
        !worker_page_survives_a_later_open_seal(false).await,
        "expected the known-bad epoch ordering to roll `current` back onto the \
         open build's stale generation, which has no worker page — if it \
         survived, the epoch guard is no longer comparing raw values, and this \
         test (and the ordering fix's rationale) needs re-checking, not deleting"
    );
}
