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
        trigger: BuildTrigger::Full, reason: None,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: Some(epoch),
        live_port: None,
        rename_pairs: Vec::new(),
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

/// Every `/_moss/style.<hash>.css` a page in `dir` links, with whether `dir`
/// holds that file.
fn linked_stylesheets(dir: &std::path::Path) -> Vec<(String, bool)> {
    let mut linked = std::collections::BTreeSet::new();
    for entry in walkdir::WalkDir::new(dir).follow_links(true).into_iter().filter_map(Result::ok) {
        if entry.path().extension().is_some_and(|e| e == "html") {
            let html = std::fs::read_to_string(entry.path()).unwrap_or_default();
            linked.extend(
                crate::build::manifest::link_audit::url_attr_values(&html)
                    .into_iter()
                    .filter(|href| href.starts_with("/_moss/style.") && href.ends_with(".css")),
            );
        }
    }
    linked.into_iter().map(|href| {
        let present = dir.join(href.trim_start_matches('/')).is_file();
        (href, present)
    }).collect()
}

/// The default stylesheet a sealed manifest names.
fn sealed_stylesheet(sealed: &crate::build::manifest::SealedManifest) -> String {
    sealed
        .files()
        .keys()
        .find(|key| key.starts_with("_moss/style.") && key.ends_with(".css"))
        .cloned()
        .expect("a sealed build names its stylesheet")
}

/// Whether another process could take `folder`'s build lock right now: a
/// separate open of the lock file contends exactly as another process would.
fn folder_lock_is_free(folder: &std::path::Path) -> bool {
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(crate::infra::folder_lock::lock_path_named(folder, "build").unwrap())
        .unwrap();
    let free = lock.try_lock().is_ok();
    if free {
        lock.unlock().unwrap();
    }
    free
}

/// A second moss process that starts building a folder while a one-shot build
/// of it (`moss build`, `moss deploy`) is between its render and its
/// promotion must wait for that promotion.
///
/// The stylesheet is named by its content hash, so two builds with different
/// stylesheets name different files. When the second build ran in that span,
/// each build's final sweep deleted the other's stylesheet from staging, and
/// the first build's seal saw pages naming a file it never wrote: the publish
/// it was building for refused, or a generation without its stylesheet was
/// promoted.
///
/// The second process is simulated in-process: the second build runs inside
/// the span only if the lock is free there.
#[tokio::test(flavor = "multi_thread")]
async fn a_build_waits_for_a_one_shot_build_to_promote_before_it_renders() {
    // The lock file lives under `MOSS_HOME`, which other tests set.
    let _home = crate::infra::home::MOSS_HOME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_tmp, folder) = fixture();
    let mp = crate::moss_paths::MossPaths::new(&folder);
    let _record = crate::build::lifecycle::lock_for(&mp);
    std::fs::write(folder.join("a.md"), "# A\n\nplain\n").unwrap();

    let first_epoch = crate::build::ship::next_promotion_epoch();
    let second_epoch = crate::build::ship::next_promotion_epoch();
    // A callout ships the callout styles: the second build's stylesheet has
    // a different name from the first's.
    let second_stylesheet = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let second_build = {
        let (folder, second_stylesheet) = (folder.clone(), second_stylesheet.clone());
        move || {
            std::fs::write(folder.join("a.md"), "# A\n\n> [!note]\n> A callout.\n").unwrap();
            let (folder, second_stylesheet) = (folder.clone(), second_stylesheet.clone());
            std::thread::spawn(move || {
                let mut second = cfg(&folder, second_epoch);
                let sealed = crate::deploy::one_shot::capture_seal(&mut second.host);
                let runtime = tokio::runtime::Runtime::new().unwrap();
                runtime.block_on(run_pipeline(second)).expect("second build");
                *second_stylesheet.lock().unwrap() = sealed.lock().unwrap().as_ref().map(sealed_stylesheet);
            })
            .join()
            .unwrap();
        }
    };

    // Each point the hook was called at, and whether the lock was free there.
    let points = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, bool)>::new()));
    let hook: std::sync::Arc<dyn Fn(&str) + Send + Sync> = {
        let (folder, points, second_build) = (folder.clone(), points.clone(), second_build.clone());
        std::sync::Arc::new(move |point: &str| {
            let free = folder_lock_is_free(&folder);
            points.lock().unwrap().push((point.to_string(), free));
            if point == "rendered" && free {
                second_build();
            }
        })
    };

    let mut first = cfg(&folder, first_epoch);
    let sealed = crate::deploy::one_shot::capture_seal(&mut first.host);
    crate::build::ONE_SHOT_TAIL_TEST_HOOK
        .scope(hook, run_pipeline(first))
        .await
        .expect("first build");
    let first_sealed = sealed.lock().unwrap().take();
    let points = points.lock().unwrap().clone();
    let names: Vec<&str> = points.iter().map(|(point, _)| point.as_str()).collect();
    assert_eq!(
        names,
        ["rendered", "sealed", "shipping"],
        "the hook must run before the first build's seal, after it, and before its ship"
    );
    if !points[0].1 {
        second_build();
    }

    let current = std::fs::canonicalize(mp.current_ptr()).expect("current must point somewhere");
    for dir in [mp.staging_dir(), current] {
        let linked = linked_stylesheets(&dir);
        assert!(!linked.is_empty(), "precondition: pages in {} link a stylesheet", dir.display());
        assert!(
            linked.iter().all(|(_, present)| *present),
            "every stylesheet a page in {} links must be there: {linked:?}",
            dir.display()
        );
    }
    assert!(
        points.iter().all(|(_, free)| !free),
        "the first build must hold the folder build lock from its render through its ship: {points:?}"
    );
    let first_sealed = first_sealed.expect("the first build must promote what it rendered");
    let second = second_stylesheet.lock().unwrap().clone().expect("the second build must promote too");
    assert_ne!(
        sealed_stylesheet(&first_sealed),
        second,
        "precondition: the two builds must name different stylesheets, or nothing could go missing"
    );
}

/// A build's place in the cross-process promotion order is taken when it gets
/// the folder build lock, not when it starts.
///
/// A build that started first but waited out another process's whole render
/// rendered second, and must promote second. Stamped at its start, it carried
/// the older stamp, so the other build's later materialize still saw itself as
/// the newer one and promoted over it, from a staging directory the waiting
/// build's sweep had since emptied of its stylesheet.
#[tokio::test(flavor = "multi_thread")]
async fn a_build_that_waited_for_the_folder_build_lock_is_ordered_after_the_wait() {
    let _home = crate::infra::home::MOSS_HOME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (_tmp, folder) = fixture();
    let _record = crate::build::lifecycle::lock_for(&crate::moss_paths::MossPaths::new(&folder));

    let held = crate::infra::folder_lock::acquire(&folder).expect("folder build lock");
    let build = {
        let folder = folder.clone();
        let epoch = crate::build::ship::next_promotion_epoch();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(run_pipeline(cfg(&folder, epoch))).expect("build");
        })
    };
    // Long enough for the build to start and reach the lock.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let released_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    drop(held);
    tokio::task::spawn_blocking(move || build.join().unwrap()).await.unwrap();

    let promoted = crate::infra::folder_lock::last_promoted_epoch(&folder);
    assert_ne!(promoted, 0, "the build never promoted");
    assert!(promoted >= released_at, "the build promoted under a stamp older than the moment it got the lock");
}

// ── Seal tails that a newer seal has overtaken ──────────────────────────────

fn sealed_page(stage: &std::path::Path, body: &[u8]) -> crate::build::manifest::SealedManifest {
    sealed_page_with(stage, body, |_| {})
}

fn sealed_page_with(stage: &std::path::Path, body: &[u8], shape: impl FnOnce(&mut crate::build::manifest::PendingManifest)) -> crate::build::manifest::SealedManifest {
    use crate::build::manifest::{HashBucket, PendingManifest};
    std::fs::write(stage.join("index.html"), body).unwrap();
    let mut pending = PendingManifest::new(crate::types::content::SiteHashes::default());
    pending.register(&crate::build::served_path::ServedPath::from_source("index.html").unwrap(), body, HashBucket::Files);
    shape(&mut pending);
    pending.seal()
}

async fn tail(
    mp: &crate::moss_paths::MossPaths,
    session: &std::sync::Arc<crate::system::folder_session::FolderSession>,
    sealed: crate::build::manifest::SealedManifest,
    render: u64,
    handoff: crate::build::Handoff,
) -> Option<crate::build::seal_phase::PendingSeal> {
    let numbered = crate::build::lifecycle::Render::numbered(mp, render);
    tail_with(mp, session, sealed, render, numbered, crate::build::SealGuards { handoff, ..Default::default() }).await
}

async fn tail_with(
    mp: &crate::moss_paths::MossPaths,
    session: &std::sync::Arc<crate::system::folder_session::FolderSession>,
    sealed: crate::build::manifest::SealedManifest,
    locked_at: u64,
    render: crate::build::lifecycle::Render,
    guards: crate::build::SealGuards,
) -> Option<crate::build::seal_phase::PendingSeal> {
    let ports = crate::build::SealPorts {
        events: crate::build::null_sink(),
        announcer: crate::build::ports::host::test_host_ports().announcer.clone(),
        server_diff: None,
    };
    crate::build::advertise_sealed(
        &ports,
        mp,
        &mp.hashes(),
        &mp.staging_dir(),
        sealed,
        None,
        std::sync::Arc::new(|_: &str| false),
        Some(session.clone()),
        crate::build::ship::next_promotion_epoch(),
        locked_at,
        Some(render),
        &mp.project_root().to_string_lossy(),
        guards,
    )
    .await
}

/// The tail of a serving build whose render `show_render` minted.
async fn serving_tail(fx: &SealFixture, sealed: crate::build::manifest::SealedManifest, render: crate::build::lifecycle::Render) {
    let guards = crate::build::SealGuards { handoff: crate::build::Handoff::Lane, sweep: crate::build::lifecycle::SealSweep::Serving(Default::default()), ..Default::default() };
    let _ = tail_with(&fx.mp, &fx.session, sealed, render.seq(), render, guards).await;
}

/// Residue of earlier builds: a page whose source is gone, and a torn write.
fn residue(stage: &std::path::Path) -> [std::path::PathBuf; 2] {
    std::fs::create_dir_all(stage.join("gone")).unwrap();
    std::fs::write(stage.join("gone/index.html"), b"<h1>gone</h1>").unwrap();
    std::fs::write(stage.join("index.html.pending.1"), b"torn").unwrap();
    [stage.join("gone/index.html"), stage.join("index.html.pending.1")]
}

/// The edit loop's sweep runs after the preview paints: the tail of the
/// render on screen removes what its manifest does not name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serving_tail_sweeps_what_its_render_does_not_name() {
    let fx = seal_fixture();
    let stage = fx.mp.staging_dir();
    let left = residue(&stage);
    let (render, _) = crate::build::lifecycle::show_render(&fx.mp, true);
    serving_tail(&fx, sealed_page(&stage, b"<html><body>now</body></html>"), render).await;
    for path in &left {
        assert!(!path.exists(), "{} survived the tail", path.display());
    }
    assert!(!stage.join("gone").exists());
    assert!(stage.join("index.html").is_file());
}

/// A save renders while the older render's tail waits for the stage lock. The
/// newer render's outputs are not in the older manifest, and must survive it,
/// even when the newer render is withheld and the older one stays on screen.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_newer_renders_outputs_survive_an_older_tail() {
    let fx = seal_fixture();
    let stage = fx.mp.staging_dir();
    let (older, _) = crate::build::lifecycle::show_render(&fx.mp, true);
    let sealed = sealed_page(&stage, b"<html><body>older</body></html>");
    let (_newer, _) = crate::build::lifecycle::show_render(&fx.mp, false);
    std::fs::create_dir_all(stage.join("new")).unwrap();
    std::fs::write(stage.join("new/index.html"), b"<h1>new</h1>").unwrap();
    serving_tail(&fx, sealed, older).await;
    assert!(stage.join("new/index.html").is_file(), "the older tail swept the newer render's page");
}

/// The next build skips its own sweep only when the tail of the last build to
/// enter swept; otherwise it sweeps before render, as a producer trusts a file
/// at its address.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_next_build_sweeps_unless_the_newest_tail_did() {
    let fx = seal_fixture();
    let stage = fx.mp.staging_dir();
    let permit = crate::build::lifecycle::permit_for_test();
    let skips = |locked_at| crate::build::lifecycle::unless_tail_swept(&fx.mp, locked_at, Some(&permit)).is_none();
    let (older, _) = crate::build::lifecycle::show_render(&fx.mp, true);
    assert!(!skips(older.seq()), "nothing has swept yet");
    let sealed = sealed_page(&stage, b"<html><body>older</body></html>");
    let (newer, _) = crate::build::lifecycle::show_render(&fx.mp, true);
    assert!(!skips(newer.seq()));
    serving_tail(&fx, sealed, older).await;
    serving_tail(&fx, sealed_page(&stage, b"<html><body>newer</body></html>"), newer).await;
    assert!(skips(100), "the tail of the last build to enter swept");
    assert!(!skips(101), "a build entered since, and may have written residue, even if it failed before rendering");
}

/// A build in another process entered after this render's: the tail sweeps
/// against a manifest that is not the next build's baseline, so it leaves the
/// next build sweeping.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tail_marks_staging_swept_only_for_the_last_build_to_enter() {
    let fx = seal_fixture();
    let stage = fx.mp.staging_dir();
    let permit = crate::build::lifecycle::permit_for_test();
    let skips = |locked_at| crate::build::lifecycle::unless_tail_swept(&fx.mp, locked_at, Some(&permit)).is_none();
    let (render, _) = crate::build::lifecycle::show_render(&fx.mp, true);
    skips(render.seq());
    skips(50);
    serving_tail(&fx, sealed_page(&stage, b"<html><body>now</body></html>"), render).await;
    assert!(!skips(100), "the tail marked a sweep for a build that was not the last to enter");
}

/// Only the render on screen sweeps: a withheld render's tail leaves staging
/// to the page the preview still shows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_withheld_renders_tail_sweeps_nothing() {
    let fx = seal_fixture();
    let stage = fx.mp.staging_dir();
    let left = residue(&stage);
    let (render, _) = crate::build::lifecycle::show_render(&fx.mp, false);
    serving_tail(&fx, sealed_page(&stage, b"<html><body>withheld</body></html>"), render).await;
    assert!(left.iter().all(|path| path.exists()), "a render off screen swept staging");
}

/// A seal that does not ship leaves the previous manifest as the next build's
/// baseline, which may name what this one dropped.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seal_that_does_not_ship_sweeps_nothing() {
    let fx = seal_fixture();
    let stage = fx.mp.staging_dir();
    let left = residue(&stage);
    let (render, _) = crate::build::lifecycle::show_render(&fx.mp, true);
    let sealed = sealed_page_with(&stage, b"<html><body>unverified</body></html>", |p| p.mark_unverified("index.html".into(), "still in the cloud".into()));
    serving_tail(&fx, sealed, render).await;
    assert!(left.iter().all(|path| path.exists()), "a withheld seal swept staging");
}

/// Another build's workers may be writing staging, so the tail leaves it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tail_sweeps_nothing_while_another_build_writes() {
    let fx = seal_fixture();
    let stage = fx.mp.staging_dir();
    let left = residue(&stage);
    let _other = crate::build::lifecycle::cache_write_lease(&fx.mp);
    let (render, _) = crate::build::lifecycle::show_render(&fx.mp, true);
    serving_tail(&fx, sealed_page(&stage, b"<html><body>now</body></html>"), render).await;
    assert!(left.iter().all(|path| path.exists()), "the tail swept under another build's writes");
}

/// Holds the folder's lifecycle record, as a detached tail does for its whole
/// life: an idle record is evicted by the next lookup of another folder,
/// which parallel tests make constantly.
struct SealFixture {
    _tmp: tempfile::TempDir,
    _record: std::sync::Arc<crate::build::lifecycle::LifecycleCell>,
    mp: crate::moss_paths::MossPaths,
    session: std::sync::Arc<crate::system::folder_session::FolderSession>,
}

fn seal_fixture() -> SealFixture {
    let (tmp, folder) = fixture();
    let mp = crate::moss_paths::MossPaths::new(&folder);
    std::fs::create_dir_all(mp.staging_dir()).unwrap();
    let session = crate::system::folder_session::FolderSession::new(mp.project_root().to_path_buf());
    SealFixture { _record: crate::build::lifecycle::lock_for(&mp), _tmp: tmp, mp, session }
}

/// Tails run detached and finish in any order. One that lands after a newer
/// render's tail must not roll back what that tail wrote: `hashes.json`, and
/// the seal the debounce lane will promote.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_older_tail_that_lands_after_a_newer_one_changes_nothing() {
    let SealFixture { mp, session, .. } = &seal_fixture();
    let older = sealed_page(&mp.staging_dir(), b"<html><body>older</body></html>");
    let newer = sealed_page(&mp.staging_dir(), b"<html><body>newer</body></html>");
    let newer_gen = newer.generation_id().to_string();

    assert!(tail(&mp, &session, newer, 2, crate::build::Handoff::Lane).await.is_none());
    let hashes_after_newer = std::fs::read(mp.hashes()).expect("the newer tail writes hashes.json");
    assert!(tail(&mp, &session, older, 1, crate::build::Handoff::Lane).await.is_none());

    assert_eq!(std::fs::read(mp.hashes()).unwrap(), hashes_after_newer, "the older tail must not rewrite hashes.json");
    crate::build::seal_phase::settle(&mp).await;
    assert_eq!(mp.current_generation_id().expect("a generation is promoted"), newer_gen, "the lane must promote the newer seal");
}

/// The common case under fast saving: several tails seal together and queue
/// on the stage-write lock. One that is overtaken while it waits must leave
/// the lock at once, writing nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tail_overtaken_while_it_waits_for_the_lock_writes_nothing() {
    let SealFixture { mp, session, .. } = &seal_fixture();
    let older = sealed_page(&mp.staging_dir(), b"<html><body>older</body></html>");
    let held = session.lock_stage_write().await;

    let waiting = {
        let (root, session) = (mp.project_root().to_path_buf(), session.clone());
        tokio::spawn(async move {
            let mp = crate::moss_paths::MossPaths::new(&root);
            tail(&mp, &session, older, 1, crate::build::Handoff::Lane).await.is_none()
        })
    };
    // The tail notes its seal before it asks for the lock.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !crate::build::lifecycle::Render::numbered(&mp, 0).newer_sealed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the tail must note its seal before it waits for the lock");
    crate::build::lifecycle::Render::numbered(&mp, 2).note_sealed();
    drop(held);

    assert!(waiting.await.unwrap());
    assert!(!mp.hashes().exists(), "an overtaken tail must not write hashes.json");
    crate::build::seal_phase::settle(&mp).await;
    assert!(mp.current_generation_id().is_err(), "nor hand the lane a seal to promote");
}

/// A render whose asset walk stopped for a newer one never promotes: its
/// generation lacks what the walk never reached. The preview keeps the last
/// whole generation, through a newer render that is withheld too, until a
/// render whose walk finished promotes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_render_whose_walk_was_superseded_never_promotes() {
    let SealFixture { mp, session, .. } = &seal_fixture();
    let lane = || crate::build::Handoff::Lane;
    let current = || mp.current_generation_id().expect("a generation is promoted");
    let whole = sealed_page(&mp.staging_dir(), b"<html><body>whole</body></html>");
    let whole_gen = whole.generation_id().to_string();
    assert!(tail(mp, session, whole, 1, lane()).await.is_none());
    crate::build::seal_phase::settle(mp).await;
    assert_eq!(current(), whole_gen);

    let half = sealed_page_with(&mp.staging_dir(), b"<html><body>half</body></html>", |p| p.mark_superseded());
    assert!(tail(mp, session, half, 2, lane()).await.is_none());
    crate::build::seal_phase::settle(mp).await;
    assert_eq!(current(), whole_gen, "a half-walked render is not shown");

    let withheld = sealed_page_with(&mp.staging_dir(), b"<html><body>withheld</body></html>", |p| p.mark_unverified("index.html".into(), "still in the cloud".into()));
    let _ = tail(mp, session, withheld, 3, lane()).await;
    crate::build::seal_phase::settle(mp).await;
    assert_eq!(current(), whole_gen, "nor is a withheld one");

    let arrived = sealed_page(&mp.staging_dir(), b"<html><body>arrived</body></html>");
    let arrived_gen = arrived.generation_id().to_string();
    assert!(tail(mp, session, arrived, 4, lane()).await.is_none());
    crate::build::seal_phase::settle(mp).await;
    assert_eq!(current(), arrived_gen, "the build after the files arrive promotes");
}

/// The one-shot arm's caller reads `current` as soon as the tail returns, so
/// its tail never yields, whatever else has sealed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_caller_hand_off_never_yields() {
    let SealFixture { mp, session, .. } = &seal_fixture();
    crate::build::lifecycle::Render::numbered(mp, 9).note_sealed();
    let sealed = sealed_page(&mp.staging_dir(), b"<html><body>one-shot</body></html>");
    let pending = tail(&mp, &session, sealed, 1, crate::build::Handoff::Caller).await;
    assert!(pending.is_some(), "the caller gets its seal back");
    assert!(mp.hashes().exists(), "and the tail wrote hashes.json");
}

#[test]
fn a_render_is_overtaken_only_by_a_newer_sealed_one() {
    let SealFixture { mp, .. } = &seal_fixture();
    let render = |seq| crate::build::lifecycle::Render::numbered(mp, seq);
    assert!(!render(1).newer_sealed(), "nothing has sealed");
    render(3).note_sealed();
    render(2).note_sealed();
    assert!(render(2).newer_sealed(), "3 overtakes 2, even noted first");
    assert!(!render(3).newer_sealed(), "a render does not overtake itself");
}

/// The lane hand-off happens under the stage-write lock. A newer tail that
/// seals while an older one is about to hand off must wait for it, or the
/// older request lands last and the lane promotes the older render.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_newer_tail_cannot_hand_off_before_an_older_one_that_holds_the_lock() {
    let SealFixture { mp, session, .. } = &seal_fixture();
    let older = sealed_page(&mp.staging_dir(), b"<html><body>older</body></html>");
    let newer = sealed_page(&mp.staging_dir(), b"<html><body>newer</body></html>");
    let newer_gen = newer.generation_id().to_string();

    let newer = std::sync::Mutex::new(Some(newer));
    let spawned = std::sync::Arc::new(std::sync::Mutex::new(None::<tokio::task::JoinHandle<()>>));
    let hook: std::sync::Arc<dyn Fn() + Send + Sync> = {
        let (root, session, spawned) = (mp.project_root().to_path_buf(), session.clone(), spawned.clone());
        std::sync::Arc::new(move || {
            let Some(newer) = newer.lock().unwrap().take() else { return };
            let (root, session) = (root.clone(), session.clone());
            // Spawned outside the hook's scope, so the newer tail runs no hook.
            *spawned.lock().unwrap() = Some(tokio::runtime::Handle::current().spawn(async move {
                let mp = crate::moss_paths::MossPaths::new(&root);
                tail(&mp, &session, newer, 2, crate::build::Handoff::Lane).await;
            }));
            // Time enough for the newer tail to finish, if the lock let it.
            std::thread::sleep(std::time::Duration::from_millis(500));
        })
    };
    crate::build::SEAL_HANDOFF_TEST_HOOK.scope(hook, tail(mp, session, older, 1, crate::build::Handoff::Lane)).await;
    spawned.lock().unwrap().take().expect("the older tail reached its hand-off").await.unwrap();

    crate::build::seal_phase::settle(mp).await;
    assert_eq!(mp.current_generation_id().expect("a generation is promoted"), newer_gen, "the newer seal must be the lane's last");
}
