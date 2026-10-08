//! End-to-end coverage of the debounced materialize phase, through the real
//! `run_pipeline` and the real long-lived seal tail — the shape
//! `build/overlap_tests.rs` and `build/pipeline_tests.rs` already use for the
//! seal, reused here rather than re-invented. `build/debounce_tests.rs`
//! already pins the generic debouncer's own timing properties on a paused
//! clock; these tests are about what THIS caller does with it, on real time
//! (a `CapturingSpawner`/`TokioSpawner` mix, per test, matching the existing
//! seal test suites) — small fixtures finish in well under the 20 s default
//! `IDLE`, which is what lets "nothing materializes yet" be a meaningful
//! assertion rather than a race.

use super::*;
use crate::build::ports::host::{test_host_ports, HostPorts};
use crate::build::ports::spawner::TokioSpawner;
use crate::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
use crate::deploy::one_shot::capture_seal;
use crate::system::folder_session::FolderSession;
use crate::types::services::BuildServices;
use std::sync::{Arc, RwLock};

struct Vault {
    // `register_session` drains every session in the process-global registry,
    // so a test that registers one (a deploy, a one-shot build, any other
    // `Vault`) shuts down the session of a seal test running beside it, and
    // that test's seal never promotes. Tests that register sessions serialize
    // on this lock; it is held for the vault's whole life.
    _serial: std::sync::MutexGuard<'static, ()>,
    _tmp: tempfile::TempDir,
    folder: std::path::PathBuf,
    folder_key: String,
    mp: crate::moss_paths::MossPaths,
    session: Arc<FolderSession>,
    served: Arc<RwLock<std::path::PathBuf>>,
    _record: Arc<crate::build::lifecycle::LifecycleCell>,
}

impl Drop for Vault {
    fn drop(&mut self) {
        crate::system::folder_session::registry().remove(&self.folder_key);
    }
}

impl Vault {
    fn new() -> Self {
        let serial = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("test-tmp");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new().prefix("moss-seal-phase").tempdir_in(&base).unwrap();
        let folder = tmp.path().canonicalize().unwrap();
        let folder_key = folder.to_string_lossy().to_string();
        let session = FolderSession::new(folder.clone());
        crate::system::folder_session::registry().insert(folder_key.clone(), session.clone());
        let mp = crate::moss_paths::MossPaths::new(&folder);
        let record = crate::build::lifecycle::lock_for(&mp);
        std::fs::write(folder.join("index.md"), "---\ntitle: Home\n---\n\nv0\n").unwrap();
        Self { _serial: serial, _tmp: tmp, folder, folder_key, mp, session, served: Arc::new(RwLock::new(std::path::PathBuf::new())), _record: record }
    }

    /// One long-lived-arm build (`exits_after_build: false`, a real
    /// `TokioSpawner`) — the detached tail this whole module debounces.
    async fn build(&self) {
        self.build_with(self.host()).await;
    }

    fn host(&self) -> HostPorts {
        let mut services = BuildServices::headless();
        services.session = Some(self.session.clone());
        HostPorts { site_dir: Some(self.served.clone()), spawner: Arc::new(TokioSpawner), services, ..test_host_ports() }
    }

    async fn build_with(&self, host: HostPorts) {
        run_pipeline(PipelineConfig {
            root: crate::vault::paths::VaultRoot::resolve(&self.folder),
            progress: crate::build::null_sink(),
            plugins: PluginMode::Skip,
            watch: false,
            start_server: false,
            host,
            trigger: BuildTrigger::Full,
            exits_after_build: false,
            site_url_override: None,
            server_port: None,
            admission_epoch: None,
            live_port: None,
        })
        .await
        .expect("build");
    }

    /// Wait for the per-build half (`advertise_sealed`) to finish — NOT for
    /// the (now debounced) materialize phase, which no longer gates
    /// `has_ui_bound()` the moment it is handed to the lane. See
    /// `build.rs`'s call site comment on why that decoupling is deliberate.
    async fn drained(&self) {
        for _ in 0..2000 {
            if !self.session.has_ui_bound() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("the seal's per-build half never finished");
    }

    fn edit(&self, body: &str) {
        std::fs::write(self.folder.join("index.md"), format!("---\ntitle: Home\n---\n\n{body}\n")).unwrap();
    }

    fn generations(&self) -> std::collections::BTreeSet<String> {
        crate::build::store_gc::list_generations(&self.mp.generations_dir()).into_iter().collect()
    }

    /// Build, seal and promote the vault as it stands; returns what `current`
    /// then serves.
    async fn promote(&self) -> String {
        self.build().await;
        self.drained().await;
        settle(&self.mp).await;
        self.mp.current_generation_id().expect("the seal must have promoted a generation")
    }

    /// Every file in generation `gen`, with its inode — see [`rewritten`]. A copy renames a fresh
    /// file into place, so a file the seal rewrote comes back with a new one.
    #[cfg(unix)]
    fn inodes(&self, gen: &str) -> std::collections::BTreeMap<std::path::PathBuf, u64> {
        use std::os::unix::fs::MetadataExt;
        walkdir::WalkDir::new(self.mp.generation_dir(gen))
            .into_iter()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().is_file())
            .map(|e| (e.path().to_path_buf(), e.metadata().unwrap().ino()))
            .collect()
    }
}

/// (a) A burst of watch rebuilds inside the idle window must materialize
/// exactly once, over the LATEST content — not once per build, which is the
/// whole cost this stage exists to remove (measured at ~9 CPU-s per 3,750
/// creates-plus-deletes inside a cloud-synced vault).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn n_rapid_watch_builds_inside_the_idle_window_materialize_exactly_once() {
    let vault = Vault::new();
    for i in 0..5 {
        vault.edit(&format!("edit {i}"));
        vault.build().await;
        vault.drained().await;
    }
    assert!(
        vault.generations().is_empty(),
        "nothing may materialize before the idle window elapses or settle() is forced"
    );

    crate::build::seal_phase::settle(&vault.mp).await;

    assert_eq!(
        vault.generations().len(),
        1,
        "five builds inside one idle window must collapse to exactly one materialize"
    );
    let staged = std::fs::read_to_string(vault.mp.staging_dir().join("index.html")).unwrap();
    assert!(staged.contains("edit 4"), "staging must already show the latest edit (never debounced)");
}

/// (b) A pending seal that is never forced — the shape a kill or a crash
/// leaves behind — must not disturb `current`: it stays exactly at whichever
/// generation the LAST completed materialize promoted, valid and readable.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pending_seal_left_unforced_leaves_current_at_the_prior_valid_generation() {
    let vault = Vault::new();

    vault.build().await;
    vault.drained().await;
    crate::build::seal_phase::settle(&vault.mp).await;
    let prior = vault.mp.current_generation_id().expect("build 1 must have promoted a generation");

    // Build 2: an edit, its per-build half done, its materialize debounced
    // and never forced — the "kill mid-debounce" state. Nothing further runs.
    vault.edit("v1");
    vault.build().await;
    vault.drained().await;
    // Margin so a debounce that wrongly fired immediately (rather than
    // deferring) has time to finish before the assertion below — without
    // this, a broken immediate-materialize could race the check and pass by
    // luck. Nowhere near the real 20 s IDLE, so this changes nothing about
    // what a correctly-debounced run actually does.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert_eq!(
        vault.mp.current_generation_id().expect("current must still resolve"),
        prior,
        "current must not move until a materialize actually completes"
    );
    assert!(
        vault.mp.generation_dir(&prior).join("index.html").is_file(),
        "the prior generation current still points at must remain a complete, valid tree"
    );
}

/// (c) Calling `settle` immediately after a save — no natural idle wait —
/// must force a synchronous materialize before `current_generation_id()` is
/// read, which is exactly the check a plugin deploy makes
/// (`deploy::plugin_push::site_dir_for_plugin`) before handing bytes to a
/// deploy target.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settle_forces_a_synchronous_seal_before_generation_id_is_read() {
    let vault = Vault::new();
    let mut host = vault.host();
    let captured = capture_seal(&mut host);
    vault.build_with(host).await;
    vault.drained().await;

    // Immediately, no sleep: the deploy-shaped scenario this test names.
    crate::build::seal_phase::settle(&vault.mp).await;

    let sealed = captured.lock().unwrap().take().expect("the materialize phase must have adopted a manifest");
    assert_eq!(
        vault.mp.current_generation_id().expect("current must resolve right after settle"),
        sealed.generation_id(),
        "a deploy reading current_generation_id() right after settle() must see the \
         generation it just sealed, not an older one"
    );
}

/// A local stylesheet can reach staging only in the deferred asset-copy
/// worker. The first preview projection must wait; once the merged seal has
/// its bytes, the matching render may be revealed and promoted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_page_with_late_copied_local_stylesheet_becomes_servable() {
    use crate::system::folder_session::PreviewSource;
    let vault = Vault::new();
    vault.session.set_preview_requirement("/".into(), PreviewSource::File("index.md".into())).unwrap();
    std::fs::write(vault.folder.join("critical.css"), "body { color: navy; }\n").unwrap();
    vault.edit("<link rel=\"stylesheet\" href=\"/critical.css\">\n\nready");

    vault.build().await;
    vault.drained().await;
    assert!(vault.mp.staging_dir().join("critical.css").is_file(), "deferred copy must land before the seal projection");
    assert!(std::fs::read_to_string(vault.mp.staging_dir().join("index.html")).unwrap().contains("critical.css"));
    assert_eq!(*vault.served.read().unwrap(), vault.mp.staging_dir(), "the ready stage should be revealed before the materialize debounce");
    crate::build::seal_phase::settle(&vault.mp).await;

    assert!(vault.mp.current_generation_id().is_ok(), "a complete selected page should promote");
    assert_eq!(*vault.served.read().unwrap(), vault.mp.staging_dir(), "the late-ready latest render should be revealed");
}

/// The selected page cannot promote a generation while its linked local CSS
/// is absent from the sealed file set and actual served root.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selected_page_with_missing_local_stylesheet_cannot_promote() {
    use crate::system::folder_session::PreviewSource;
    let vault = Vault::new();
    vault.session.set_preview_requirement("/".into(), PreviewSource::File("index.md".into())).unwrap();
    vault.edit("<link rel=\"stylesheet\" href=\"/missing.css\">\n\nnot ready");

    vault.build().await;
    vault.drained().await;
    assert!(std::fs::read_to_string(vault.mp.staging_dir().join("index.html")).unwrap().contains("missing.css"));
    crate::build::seal_phase::settle(&vault.mp).await;

    assert!(vault.mp.current_generation_id().is_err(), "a selected route with a missing required stylesheet must not promote");
    assert!(!vault.mp.hashes().exists(), "a withheld incomplete attempt must not replace the incremental baseline");
}

/// (f) An app that bounds its quit with a `tokio::time::timeout` around
/// `settle` must get control back at that bound even while a slow
/// materialize is still copying. That holds only because the copy runs on
/// `spawn_blocking`; inline, the timeout could not fire until it finished.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_materialize_does_not_block_a_timeout_wrapping_settle() {
    let vault = Vault::new();
    vault.build().await;
    vault.drained().await;

    let started = std::time::Instant::now();
    let result = MATERIALIZE_TEST_DELAY
        .scope(std::time::Duration::from_secs(2), async {
            tokio::time::timeout(std::time::Duration::from_millis(200), settle(&vault.mp)).await
        })
        .await;
    let elapsed = started.elapsed();

    assert!(
        result.is_err(),
        "a 200ms timeout wrapping settle must fire before a 2s materialize finishes — \
         if this instead returns Ok, the blocking copy ran inline on this task's own \
         poll and the timeout could not race it"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "the timeout must fire near its own 200ms bound, not the materialize's 2s delay; took {elapsed:?}"
    );
}

/// (e) A folder switch (or close) removes the folder's session from the
/// registry as its first teardown step — a real app's workspace-close
/// handler does this before anything else runs — so `settle` must complete
/// a pending seal using ITS OWN `Arc<FolderSession>`
/// clone (carried in `PendingSeal` since the per-build half handed off),
/// never a fresh registry lookup. Otherwise "complete the pending seal rather
/// than abandon it" would silently regress into abandoning it the moment a
/// real switch handler called it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settle_completes_a_pending_seal_even_after_the_folder_session_is_deregistered() {
    let vault = Vault::new();
    vault.build().await;
    vault.drained().await;

    // The switch/close handler's own first step, simulated here directly.
    crate::system::folder_session::registry().remove(&vault.folder_key);
    assert!(
        crate::system::folder_session::registry().get(&vault.folder_key).is_none(),
        "sanity: the session must actually be gone from the registry"
    );

    crate::build::seal_phase::settle(&vault.mp).await;

    assert!(
        vault.mp.current_ptr().exists(),
        "settle must still materialize and promote the pending generation — completing \
         it, not abandoning it, is the whole point of calling settle from a switch/close path"
    );
}

/// (g) Ending a folder session — settled first, then shut down, as the
/// desktop close and switch paths do — must drop its debounce lane. Keyed by
/// folder rather than owned by the session, the lane would otherwise keep a
/// parked background task alive for every folder ever opened.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shut_down_folder_session_leaves_no_debounce_lane_behind() {
    let vault = Vault::new();
    vault.build().await;
    vault.drained().await;
    assert!(LANES.has_lane(vault.mp.root()), "sanity: a build must have opened a lane for this folder");

    settle(&vault.mp).await;
    vault.session.clone().shutdown(std::time::Duration::from_millis(50)).await;

    assert!(!LANES.has_lane(vault.mp.root()), "shutting the session down must remove the folder's lane");
}

/// (h) A seal that finishes its copy removes the generation's write-lock
/// file, so the generation gets ordinary retention once it is no longer
/// current. A lock file left behind would mark it abandoned, and the next
/// seal's GC would delete a good generation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_generation_survives_the_next_seals_gc() {
    let vault = Vault::new();
    vault.build().await;
    vault.drained().await;
    settle(&vault.mp).await;
    let first = vault.mp.current_generation_id().expect("build 1 must have promoted");

    vault.edit("v1");
    vault.build().await;
    vault.drained().await;
    settle(&vault.mp).await;
    assert_ne!(vault.mp.current_generation_id().ok().as_deref(), Some(first.as_str()), "sanity: build 2 promoted");

    assert!(vault.generations().contains(&first), "a finished generation inside the retention window must survive GC");
}

/// How many of `before`'s files are gone or were replaced in `after`.
#[cfg(unix)]
fn rewritten(
    before: &std::collections::BTreeMap<std::path::PathBuf, u64>,
    after: &std::collections::BTreeMap<std::path::PathBuf, u64>,
) -> usize {
    before.iter().filter(|(path, ino)| after.get(*path) != Some(*ino)).count()
}

/// (i) A save that changes nothing re-derives the generation `current`
/// already serves. Its seal must rewrite none of that generation's files —
/// each one is a file-provider event in a cloud-synced vault, paid while the
/// stage-write lock holds back the next edit's rebuild — and must still
/// complete: `settle` returns and deploy is handed the manifest.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resealing_the_current_generation_rewrites_none_of_its_files() {
    let vault = Vault::new();
    let gen = vault.promote().await;
    let before = vault.inodes(&gen);
    let pointer = || std::os::unix::fs::MetadataExt::ino(&std::fs::symlink_metadata(vault.mp.current_ptr()).unwrap());
    let pointer_before = pointer();

    let mut host = vault.host();
    let captured = capture_seal(&mut host);
    vault.build_with(host).await;
    vault.drained().await;
    assert!(LANES.is_pending(vault.mp.root()), "sanity: the unchanged build handed a seal to the lane");
    settle(&vault.mp).await;

    assert!(!LANES.is_pending(vault.mp.root()), "settle must complete the pending seal");
    let sealed = captured.lock().unwrap().take().expect("deploy must still be handed the manifest");
    assert_eq!(sealed.generation_id(), gen, "sanity: an unchanged rebuild re-derives the same generation");
    assert_eq!(vault.mp.current_generation_id().unwrap(), gen, "`current` must not move");
    assert_eq!(pointer(), pointer_before, "nor be rewritten in place");
    let rewritten = rewritten(&before, &vault.inodes(&gen));
    assert_eq!(rewritten, 0, "the seal rewrote {rewritten} of the {} files `current` already served", before.len());
}

/// (j) An edit and its undo return to a generation still on disk. Promoting
/// it again needs no copy: the directory already holds exactly those bytes.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_undone_edit_promotes_the_earlier_generation_without_copying_it() {
    let vault = Vault::new();
    let first = vault.promote().await;
    let before = vault.inodes(&first);
    vault.edit("v1");
    assert_ne!(vault.promote().await, first, "sanity: the edit promoted a new generation");

    vault.edit("v0");
    assert_eq!(vault.promote().await, first, "the undo must promote the earlier generation");
    assert_eq!(rewritten(&before, &vault.inodes(&first)), 0, "and must not have rewritten any of its files");
}

/// (k) A generation directory is reused only when it is whole. One missing a
/// file, or whose copy was cut off (its lock file left behind), is copied
/// again before `current` points at it.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_earlier_generation_that_is_not_whole_is_copied_again() {
    let vault = Vault::new();
    let first = vault.promote().await;
    let page = vault.mp.generation_dir(&first).join("index.html");

    vault.edit("v1");
    vault.promote().await;
    std::fs::remove_file(&page).unwrap();
    vault.edit("v0");
    assert_eq!(vault.promote().await, first);
    assert!(page.is_file(), "a generation missing a file must be copied again");

    vault.edit("v1");
    vault.promote().await;
    let before = vault.inodes(&first);
    std::fs::write(vault.mp.generations_dir().join(format!(".{first}.writing")), b"").unwrap();
    vault.edit("v0");
    assert_eq!(vault.promote().await, first);
    assert_eq!(
        rewritten(&before, &vault.inodes(&first)),
        before.len(),
        "a generation whose copy was cut off must be copied again in full"
    );
}

/// The seal is derived work: while nobody is looking a sealed build stays
/// pending behind the folder's gate, and a forced settle (a publish, a quit, a
/// folder switch) still materializes it at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hidden_window_holds_the_seal_behind_the_gate_and_settle_still_runs_it() {
    use crate::ops::watch::cadence::Cadence;
    let vault = Vault::new();
    let (_cadence, rx) = tokio::sync::watch::channel(Cadence::Background);
    vault.session.follow_cadence(rx);

    vault.build().await;
    vault.drained().await;
    let gate = LANES.pending_gate(vault.mp.root()).expect("the seal must be pending");
    assert!(gate.is_closed(), "a hidden window must hold the seal behind the folder's gate");

    settle(&vault.mp).await;
    assert!(vault.mp.current_generation_id().is_ok(), "settle must materialize whatever the gate says");
}

/// Once the window is visible again, the held seal runs and its own tail asks
/// search for exactly one index of the generation it promoted.
///
/// On a current-thread runtime so the 20 s idle window can be skipped on the
/// paused clock; the builds themselves run on real time.
#[tokio::test(flavor = "current_thread")]
async fn the_flush_on_live_runs_the_held_seal_then_one_index() {
    use crate::build::feeds::search_lane::PageSet;
    use crate::ops::watch::cadence::Cadence;
    let _serialize = crate::build::feeds::search::lock_index_counter();
    let vault = Vault::new();
    std::fs::create_dir_all(vault.mp.root()).unwrap();
    std::fs::write(vault.mp.config(), "[site]\nsearch = true\n").unwrap();
    let (cadence, rx) = tokio::sync::watch::channel(Cadence::Background);
    vault.session.follow_cadence(rx);

    vault.build().await;
    vault.drained().await;
    tokio::time::pause();
    tokio::time::advance(IDLE + std::time::Duration::from_secs(1)).await;
    tokio::time::resume();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(vault.generations().is_empty(), "a hidden window must hold the seal past its idle window");

    let hashes: crate::types::content::SiteHashes =
        serde_json::from_str(&std::fs::read_to_string(vault.mp.hashes()).unwrap()).unwrap();
    let want = format!("{:?}", PageSet::of(&hashes.files).fp);
    let receipt_fp = || {
        std::fs::read_to_string(vault.mp.index_receipt())
            .ok()
            .and_then(|raw| serde_json::from_str::<crate::build::feeds::search_lane::BundleReceipt>(&raw).ok())
            .and_then(|r| u64::from_str_radix(&r.fp, 16).ok())
            .map(|fp| format!("PageSetFp({fp})"))
    };
    let before = crate::build::feeds::search::index_build_count();
    cadence.send_replace(Cadence::Live);
    for _ in 0..500 {
        if receipt_fp().as_deref() == Some(want.as_str()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(vault.generations().len(), 1, "Live must materialize the held seal");
    assert_eq!(receipt_fp().as_deref(), Some(want.as_str()), "and index the generation it promoted");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(crate::build::feeds::search::index_build_count() - before, 1, "exactly once");
}

/// A first publish to a new host compares against nothing of its own. The
/// older host's record lists a feed this build does not make, and the seal
/// must still say that address is going away.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_publish_to_a_new_target_still_reports_what_another_target_served() {
    use crate::build::manifest::change_set::{PublishedSnapshot, RemovalReason};
    let vault = Vault::new();
    std::fs::create_dir_all(vault.folder.join(".moss")).unwrap();
    std::fs::write(vault.folder.join(".moss/state.toml"), "[deployment]\nsite_id = \"new-site\"\n").unwrap();
    crate::build::manifest::published_record::save(
        &vault.mp,
        &PublishedSnapshot {
            generation_id: "old".into(),
            target: "moss:old-site".into(),
            published_at: "2026-08-20T00:00:00Z".into(),
            files: [("legacy-feed.xml".to_string(), "100644:abc".to_string())].into(),
            asset_source_to_output: Some(Default::default()),
            ..Default::default()
        },
    )
    .unwrap();

    let mut host = vault.host();
    let seen = crate::deploy::one_shot::capture(&mut host).change_set;
    vault.build_with(host).await;
    vault.drained().await;
    settle(&vault.mp).await;

    let set = seen.lock().unwrap().take().expect("the seal published a change set");
    assert!(!set.classified, "verbs stay target-strict: the new target has no record of its own");
    assert_eq!(
        set.removed.iter().map(|r| (r.path.as_str(), r.reason)).collect::<Vec<_>>(),
        [("legacy-feed.xml", RemovalReason::Unexplained)]
    );
}

/// The record's folder-file map is read off a real seal: a file the author put
/// in the folder is recorded against the address it ships at.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_seal_records_which_folder_file_shipped_at_which_address() {
    let vault = Vault::new();
    std::fs::create_dir_all(vault.folder.join("docs")).unwrap();
    std::fs::write(vault.folder.join("docs/Guide.pdf"), b"%PDF-1.4 fixture").unwrap();
    let mut host = vault.host();
    let captured = capture_seal(&mut host);
    vault.build_with(host).await;
    vault.drained().await;
    settle(&vault.mp).await;

    let sealed = captured.lock().unwrap().take().expect("a sealed manifest");
    let record = crate::build::manifest::change_set::PublishedSnapshot::from_sealed(&sealed, "moss:s", String::new());
    let assets = record.asset_source_to_output.expect("a sealed record knows its folder files");
    assert_eq!(assets.get("docs/Guide.pdf").map(String::as_str), Some("docs/Guide.pdf"), "{assets:?}");
}

impl Vault {
    /// One build and seal, returning the manifest and the change set the seal
    /// published.
    async fn seal_and_tap(
        &self,
    ) -> (crate::build::manifest::SealedManifest, crate::build::manifest::change_set::ChangeSet) {
        let mut host = self.host();
        let captured_both = crate::deploy::one_shot::capture(&mut host);
        let (captured, seen) = (captured_both.sealed, captured_both.change_set);
        self.build_with(host).await;
        self.drained().await;
        settle(&self.mp).await;
        let sealed = captured.lock().unwrap().take().expect("a sealed manifest");
        let set = seen.lock().unwrap().take().expect("a change set");
        (sealed, set)
    }

    /// Record `sealed` as live on the target this folder publishes to.
    fn publish(&self, sealed: &crate::build::manifest::SealedManifest) {
        std::fs::create_dir_all(self.folder.join(".moss")).unwrap();
        std::fs::write(self.folder.join(".moss/state.toml"), "[deployment]\nsite_id = \"s1\"\n").unwrap();
        let record = crate::build::manifest::change_set::PublishedSnapshot::from_sealed(
            sealed,
            "moss:s1",
            "2026-09-01T00:00:00Z".into(),
        );
        crate::build::manifest::published_record::save(&self.mp, &record).unwrap();
    }
}

fn removed_pairs(
    set: &crate::build::manifest::change_set::ChangeSet,
) -> Vec<(String, crate::build::manifest::change_set::RemovalReason)> {
    set.removed.iter().map(|r| (r.path.clone(), r.reason)).collect()
}

/// Publish, delete a folder file, build again: the author removed it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_folder_file_deleted_after_a_publish_reads_author_removed() {
    use crate::build::manifest::change_set::RemovalReason::AuthorRemoved;
    let vault = Vault::new();
    std::fs::write(vault.folder.join("guide.pdf"), b"%PDF-1.4 fixture").unwrap();
    let (first, _) = vault.seal_and_tap().await;
    vault.publish(&first);

    std::fs::remove_file(vault.folder.join("guide.pdf")).unwrap();
    let (_, set) = vault.seal_and_tap().await;

    assert!(set.classified);
    assert_eq!(removed_pairs(&set), [("guide.pdf".to_string(), AuthorRemoved)]);
}

/// Unreadable authored bytes retain the prior page and refuse this seal for
/// publication; only verified absence can remove its output.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreadable_page_keeps_its_published_address_until_a_fresh_render() {
    use std::os::unix::fs::PermissionsExt;
    for unreadable in [false, true] {
        let vault = Vault::new();
        let path = vault.folder.join("about.md");
        std::fs::write(&path, "---\ntitle: About\n---\n\nhello\n").unwrap();
        let (first, _) = vault.seal_and_tap().await;
        assert!(first.source_to_output().contains_key("about.md"), "sanity: the page built");
        vault.publish(&first);
        let prior_page = std::fs::read(vault.mp.current_ptr().join("about/index.html")).unwrap();

        if unreadable {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        } else {
            std::fs::write(&path, b"---\ntitle: About\n---\n\n\xff\xfe\n").unwrap();
        }
        let mut host = vault.host();
        let captured = crate::deploy::one_shot::capture(&mut host);
        vault.build_with(host).await;
        vault.drained().await;
        settle(&vault.mp).await;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(captured.sealed.lock().unwrap().is_none(), "incomplete attempt must not replace the selected seal");
        assert_eq!(std::fs::read(vault.mp.current_ptr().join("about/index.html")).unwrap(), prior_page);
        let projection = crate::system::build_records::records()
            .publish_preflight(vault.folder.to_str().unwrap()).expect("latest attempt recorded");
        assert!(projection.unresolved_inputs.iter().any(|source| source == "about.md"));
        assert!(crate::deploy::refuse_publish(vault.folder.to_str().unwrap()).is_err());

        std::fs::write(&path, "---\ntitle: About\n---\n\nrestored\n").unwrap();
        let (recovered, set) = vault.seal_and_tap().await;
        assert!(recovered.unresolved_inputs().is_empty());
        assert!(removed_pairs(&set).is_empty());
        assert!(crate::system::build_records::records()
            .publish_preflight(vault.folder.to_str().unwrap()).unwrap().unresolved_inputs.is_empty());
    }
}


#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_original_receipt_flows_from_headless_render_through_promote() {
    let vault = Vault::new();
    let source = "Pictures/Portrait, Tomorrow/assets/cover.png";
    let source_path = vault.folder.join(source);
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    let image = image::RgbImage::from_fn(800, 600, |x, y| {
        image::Rgb([(x.wrapping_mul(17) ^ y.wrapping_mul(53)) as u8,
            (x.wrapping_mul(71) ^ y.wrapping_mul(11)) as u8, (x ^ y) as u8])
    });
    image.save(&source_path).unwrap();
    std::fs::write(vault.folder.join("Pictures/Portrait, Tomorrow/Portrait, Tomorrow.md"),
        "---\ntitle: Portrait\nurl: portrait-display\n---\n\n![portrait](assets/cover.png)\n").unwrap();
    vault.edit("![portrait](<Pictures/Portrait, Tomorrow/assets/cover.png>)");
    let generation = vault.promote().await;
    let path = crate::build::manifest::preview_originals::receipt_path(&vault.mp.generations_dir(), &generation);
    let receipt: crate::build::manifest::preview_originals::Originals =
        serde_json::from_slice(&std::fs::read(&path).expect("render and promotion persisted originals")).unwrap();
    let original = receipt.images.get("pictures/portrait-display/assets/cover.webp")
        .unwrap_or_else(|| panic!("canonical URL override missing from {:?}", receipt.images.keys().collect::<Vec<_>>()));
    assert_eq!(original.source, source);
    assert_eq!(original.oid, crate::build::cache::ObjectStore::hash_file(&source_path).unwrap());
    assert!(!vault.mp.current_ptr().join(path.file_name().unwrap()).exists(), "receipt is outside the served tree");
}
