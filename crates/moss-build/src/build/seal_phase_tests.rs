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
        Self { _tmp: tmp, folder, folder_key, mp, session, served: Arc::new(RwLock::new(std::path::PathBuf::new())), _record: record }
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
