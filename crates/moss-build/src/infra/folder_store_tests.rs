use super::*;
use crate::identity::keypair::Identity;
use tempfile::TempDir;

fn volume_a(_: &Path) -> Option<Volume> {
    Some(Volume { id: "volume-a".into(), root: PathBuf::from("/") })
}

fn volume_b(_: &Path) -> Option<Volume> {
    Some(Volume { id: "volume-b".into(), root: PathBuf::from("/") })
}

fn unidentified(_: &Path) -> Option<Volume> {
    None
}

struct World {
    tmp: TempDir,
}

impl World {
    fn new() -> Self {
        Self { tmp: TempDir::new().unwrap() }
    }

    fn stores(&self, lookup: fn(&Path) -> Option<Volume>) -> FolderStores {
        FolderStores::with_volume_lookup(self.tmp.path().join("records"), self.tmp.path().join("cache"), lookup)
    }

    fn folder(&self, name: &str) -> PathBuf {
        let dir = self.tmp.path().join("sites").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn site(&self, name: &str) -> PathBuf {
        let dir = self.folder(name);
        Identity::generate().unwrap().save(&dir).unwrap();
        dir
    }
}

fn mark(store: &FolderStore) {
    std::fs::create_dir_all(&store.cache_dir).unwrap();
    std::fs::write(store.cache_dir.join("hash-index.json"), "{}").unwrap();
}

fn marked(store: &FolderStore) -> bool {
    store.cache_dir.join("hash-index.json").exists()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn the_same_folder_resolves_to_the_same_store() {
    let w = World::new();
    let site = w.site("blog");
    let stores = w.stores(volume_a);
    assert_eq!(stores.resolve(&site).unwrap(), stores.resolve(&site.join(".")).unwrap());
}

#[test]
fn a_renamed_folder_keeps_its_store_and_id() {
    let w = World::new();
    let stores = w.stores(volume_a);
    let site = w.site("blog");
    let before = stores.resolve(&site).unwrap();
    mark(&before);
    std::fs::write(before.record_dir.join("comment-sync-state.json"), "{}").unwrap();
    let renamed = site.with_file_name("journal");
    std::fs::rename(&site, &renamed).unwrap();
    let after = stores.resolve(&renamed).unwrap();
    assert_eq!(after.id, before.id);
    assert!(marked(&after), "the cache moved with the record");
    assert!(after.record_dir.join("comment-sync-state.json").exists(), "a kept fact moved with the record");
    assert!(!before.cache_dir.exists());
}

#[test]
fn a_copy_beside_its_original_gets_its_own_id() {
    let w = World::new();
    let stores = w.stores(volume_a);
    let site = w.site("blog");
    let original = stores.resolve(&site).unwrap();
    mark(&original);
    let copy = site.with_file_name("blog copy");
    copy_dir(&site, &copy);
    let copied = stores.resolve(&copy).unwrap();
    assert_ne!(copied.id, original.id, "a copy is asked again, never trusted as its original");
    assert!(!marked(&copied));
    assert_eq!(stores.resolve(&site).unwrap(), original);
}

#[test]
fn a_folder_carrying_only_public_json_gets_its_own_id() {
    let w = World::new();
    let stores = w.stores(volume_a);
    let site = w.site("blog");
    let original = stores.resolve(&site).unwrap();
    let copy = site.with_file_name("lookalike");
    copy_dir(&site, &copy);
    std::fs::remove_file(copy.join(".moss/identity/secret-key")).unwrap();
    std::fs::remove_dir_all(&site).unwrap();
    assert_ne!(stores.resolve(&copy).unwrap().id, original.id);
}

#[test]
fn a_rotated_key_keeps_the_store_through_a_rename() {
    let w = World::new();
    let stores = w.stores(volume_a);
    let site = w.site("blog");
    let before = stores.resolve(&site).unwrap();
    mark(&before);
    // Saving a different key retires the old one into previous-keys.jsonl.
    Identity::generate().unwrap().save(&site).unwrap();
    assert_eq!(stores.resolve(&site).unwrap().id, before.id);
    let renamed = site.with_file_name("journal");
    std::fs::rename(&site, &renamed).unwrap();
    let after = stores.resolve(&renamed).unwrap();
    assert_eq!(after.id, before.id);
    assert!(marked(&after));
}

#[test]
fn a_rename_whose_old_key_left_no_chain_is_a_fresh_store() {
    let w = World::new();
    let site = w.site("blog");
    let before = w.stores(volume_a).resolve(&site).unwrap();
    std::fs::remove_dir_all(site.join(".moss/identity")).unwrap();
    Identity::generate().unwrap().save(&site).unwrap();
    let renamed = site.with_file_name("journal");
    std::fs::rename(&site, &renamed).unwrap();
    assert_ne!(w.stores(volume_a).resolve(&renamed).unwrap().id, before.id);
}

#[test]
fn a_copy_on_another_volume_never_takes_the_store_of_an_original_whose_volume_is_away() {
    let w = World::new();
    let site = w.site("blog");
    let original = w.stores(volume_a).resolve(&site).unwrap();
    // The original's drive is away (its path is gone) and a copy is opened
    // from another drive.
    let copy = w.tmp.path().join("other-drive").join("blog");
    copy_dir(&site, &copy);
    std::fs::remove_dir_all(&site).unwrap();
    assert_ne!(w.stores(volume_b).resolve(&copy).unwrap().id, original.id);
}

#[test]
fn another_volume_mounted_under_the_same_name_is_not_the_original() {
    let w = World::new();
    let site = w.site("blog");
    let original = w.stores(volume_a).resolve(&site).unwrap();
    // Both report the same mount point; only the volume ids differ.
    let copy = site.with_file_name("journal");
    std::fs::rename(&site, &copy).unwrap();
    assert_ne!(w.stores(volume_b).resolve(&copy).unwrap().id, original.id);
}

#[test]
fn an_unidentified_volume_never_follows_a_rename() {
    let w = World::new();
    let stores = w.stores(unidentified);
    let site = w.site("blog");
    let before = stores.resolve(&site).unwrap();
    let renamed = site.with_file_name("journal");
    std::fs::rename(&site, &renamed).unwrap();
    assert_ne!(stores.resolve(&renamed).unwrap().id, before.id);
}

#[test]
fn a_known_folder_is_not_asked_its_volume_again() {
    static LOOKUPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    fn counted(p: &Path) -> Option<Volume> {
        LOOKUPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        volume_a(p)
    }
    let w = World::new();
    let stores = w.stores(counted);
    let site = w.site("blog");
    let first = stores.resolve(&site).unwrap();
    assert_eq!(stores.resolve(&site).unwrap(), first);
    assert_eq!(LOOKUPS.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn a_folder_first_seen_without_a_volume_follows_a_rename_once_it_has_one() {
    let w = World::new();
    let site = w.site("blog");
    let before = w.stores(unidentified).resolve(&site).unwrap();
    w.stores(volume_a).resolve(&site).unwrap();
    let renamed = site.with_file_name("journal");
    std::fs::rename(&site, &renamed).unwrap();
    assert_eq!(w.stores(volume_a).resolve(&renamed).unwrap().id, before.id);
}

#[test]
fn a_stale_recorded_volume_costs_a_fresh_store_after_a_rename_and_nothing_else() {
    let w = World::new();
    let site = w.site("blog");
    let before = w.stores(volume_a).resolve(&site).unwrap();
    // The folder now reports another volume; the record keeps the first one.
    assert_eq!(w.stores(volume_b).resolve(&site).unwrap(), before);
    let renamed = site.with_file_name("journal");
    std::fs::rename(&site, &renamed).unwrap();
    assert_ne!(w.stores(volume_b).resolve(&renamed).unwrap().id, before.id);
    assert!(keypair::proven_pubkey(&renamed).is_some(), "the folder itself is untouched");
}

#[test]
fn a_folder_replaced_in_place_never_sees_the_old_store() {
    let w = World::new();
    let stores = w.stores(volume_a);
    let site = w.site("blog");
    let before = stores.resolve(&site).unwrap();
    mark(&before);
    std::fs::remove_dir_all(&site).unwrap();
    let site = w.site("blog");
    let after = stores.resolve(&site).unwrap();
    assert_ne!(after.id, before.id);
    assert!(!marked(&after), "the old cache was set aside, not reused");
    let aside = std::fs::read_dir(w.tmp.path().join("cache")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".replaced-")).count();
    assert_eq!(aside, 1, "left for the collector");
}

#[test]
fn a_folder_without_a_key_keeps_its_id_when_it_gets_one() {
    let w = World::new();
    let stores = w.stores(volume_a);
    let folder = w.folder("draft");
    let before = stores.resolve(&folder).unwrap();
    assert_eq!(stores.resolve(&folder).unwrap().id, before.id, "stable while keyless");
    Identity::generate().unwrap().save(&folder).unwrap();
    assert_eq!(stores.resolve(&folder).unwrap().id, before.id);
}

#[test]
fn concurrent_resolves_of_a_new_folder_agree_on_its_id() {
    let w = World::new();
    let stores = std::sync::Arc::new(w.stores(volume_a));
    for round in 0..20 {
        let site = w.site(&format!("site-{round}"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let ids: Vec<String> = (0..8)
            .map(|_| {
                let (stores, site, barrier) = (stores.clone(), site.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    stores.resolve(&site).unwrap().id
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();
        assert!(ids.windows(2).all(|p| p[0] == p[1]), "round {round}: {ids:?}");
    }
}

#[test]
fn the_collector_removes_a_gone_folders_stores_and_keeps_the_rest() {
    let w = World::new();
    let stores = w.stores(volume_a);
    let gone = w.folder("gone");
    let kept = w.folder("kept");
    let (gone_store, kept_store) = (stores.resolve(&gone).unwrap(), stores.resolve(&kept).unwrap());
    mark(&gone_store);
    mark(&kept_store);
    std::fs::remove_dir_all(&gone).unwrap();

    assert_eq!(stores.collect(std::time::Duration::from_secs(3600)).unwrap(), 0, "a recently used store waits out its grace");
    assert!(marked(&gone_store));

    assert_eq!(stores.collect(std::time::Duration::ZERO).unwrap(), 1);
    assert!(!gone_store.cache_dir.exists() && !gone_store.record_dir.exists());
    assert!(marked(&kept_store), "a folder that is still there keeps its store");
}

#[test]
fn a_disk_short_of_room_takes_gone_stores_before_their_grace_longest_unused_first() {
    let w = World::new();
    let stores = w.stores(volume_a);
    let (a, b, kept) = (w.folder("a"), w.folder("b"), w.folder("kept"));
    let [a_store, b_store, kept_store] = [&a, &b, &kept].map(|f| stores.resolve(f).unwrap());
    for store in [&a_store, &b_store, &kept_store] {
        mark(store);
    }
    // The store the listing gives last is the one unused longest, so the order
    // cannot come from the listing.
    let listed: Vec<_> = std::fs::read_dir(&stores.record_root).unwrap().flatten().map(|e| e.path()).collect();
    let (older, newer) = if listed.iter().position(|p| *p == a_store.record_dir) > listed.iter().position(|p| *p == b_store.record_dir) {
        (&a_store, &b_store)
    } else {
        (&b_store, &a_store)
    };
    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::open(older.record_dir.join(RECORD)).unwrap().set_modified(hour_ago).unwrap();
    std::fs::remove_dir_all(&a).unwrap();
    std::fs::remove_dir_all(&b).unwrap();
    let grace = std::time::Duration::from_secs(30 * 24 * 3600);

    assert_eq!(stores.collect_within(grace, &|| false).unwrap(), 0, "with room, a gone store waits out its grace");
    let room_after = std::cell::Cell::new(1u32);
    let short = || room_after.replace(room_after.get().saturating_sub(1)) > 0;
    assert_eq!(stores.collect_within(grace, &short).unwrap(), 1, "the sweep stops once there is room");
    assert!(!older.cache_dir.exists(), "the store unused longest goes first");
    assert!(marked(newer));

    assert_eq!(stores.collect_within(grace, &|| true).unwrap(), 1);
    assert!(!newer.cache_dir.exists());
    assert!(marked(&kept_store), "a folder that is still there keeps its store however short the disk");
}

#[test]
fn a_cache_written_for_a_folder_that_never_came_goes_after_a_day() {
    let w = World::new();
    let stores = w.stores(volume_a);
    let kept = stores.resolve(&w.folder("kept")).unwrap();
    mark(&kept);
    let orphan = stores.to_be(&w.tmp.path().join("never-made")).cache_dir;
    std::fs::create_dir_all(orphan.join("build")).unwrap();

    assert_eq!(stores.collect(std::time::Duration::ZERO).unwrap(), 0, "a folder made just after it asked keeps its cache");
    let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 3600);
    std::fs::File::open(&orphan).unwrap().set_modified(two_days_ago).unwrap();
    assert_eq!(stores.collect(std::time::Duration::ZERO).unwrap(), 1);
    assert!(!orphan.exists());
    assert!(marked(&kept));
}

#[test]
fn the_collector_keeps_the_stores_of_a_folder_whose_volume_is_away() {
    let w = World::new();
    let stores = w.stores(volume_a);
    let folder = w.folder("away/site");
    let store = stores.resolve(&folder).unwrap();
    mark(&store);
    // The whole mount is gone, parent included; what exists above it is another volume.
    std::fs::remove_dir_all(folder.parent().unwrap()).unwrap();

    assert_eq!(w.stores(volume_b).collect(std::time::Duration::ZERO).unwrap(), 0);
    assert!(marked(&store));
    assert_eq!(w.stores(unidentified).collect(std::time::Duration::ZERO).unwrap(), 0);
    assert!(marked(&store));

    assert_eq!(w.stores(volume_a).collect(std::time::Duration::ZERO).unwrap(), 1, "the volume is mounted and the folder is not on it");
}

#[test]
fn a_folder_not_made_yet_reads_the_store_it_will_have() {
    let tmp = TempDir::new().unwrap();
    let folder = tmp.path().join("not-yet").join("site");
    let before = for_folder(&folder);
    assert!(!before.cache_dir.starts_with(&folder) && !before.record_dir.starts_with(&folder));
    assert!(!before.record_dir.exists(), "asking writes nothing");
    std::fs::create_dir_all(&folder).unwrap();
    let after = for_folder(&folder);
    assert!(!after.id.is_empty(), "the miss is not remembered: the folder gets its record once it exists");
    assert_eq!((&after.cache_dir, &after.record_dir), (&before.cache_dir, &before.record_dir));
    assert_eq!(for_folder(&folder), after);
}

#[test]
fn an_exited_process_scratch_store_is_swept_and_a_running_one_kept() {
    let tmp = TempDir::new().unwrap();
    let gone = tmp.path().join(format!("{UNSTORED_PREFIX}4000000000"));
    let ours = tmp.path().join(format!("{UNSTORED_PREFIX}{}", std::process::id()));
    let other = tmp.path().join(format!("{UNSTORED_PREFIX}notapid"));
    for dir in [&gone, &ours, &other] {
        std::fs::create_dir_all(dir.join("site/cache")).unwrap();
    }
    sweep_unstored(tmp.path());
    assert!(!gone.exists());
    assert!(ours.exists() && other.exists());
}

/// A build asks where its output goes thousands of times; the folder's store
/// is resolved, and its record written, once.
#[test]
fn a_build_resolves_its_folders_store_once() {
    use crate::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    let site = tempfile::tempdir().unwrap();
    std::fs::write(site.path().join("index.md"), "---\ntitle: Orchard\n---\n\nPlum trees.\n").unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: crate::vault::paths::VaultRoot::resolve(site.path()),
        progress: crate::build::null_sink(), plugins: PluginMode::Skip,
        watch: false, start_server: false,
        host: crate::build::ports::host::test_host_ports(),
        trigger: BuildTrigger::Full, reason: None, exits_after_build: true,
        site_url_override: None, server_port: None, admission_epoch: None, live_port: None, rename_pairs: Vec::new(),
    })).expect("fixture builds");
    assert!(crate::moss_paths::MossPaths::new(site.path()).has_servable_generation());
    let writes = RECORD_WRITES.lock().unwrap().get(&name_for(site.path())).copied();
    assert_eq!(writes, Some(1));
}

/// `f` on its own thread, or `None` if it has not answered within five seconds.
fn within_five_seconds<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(f()));
    rx.recv_timeout(std::time::Duration::from_secs(5)).ok()
}

#[test]
fn the_sweep_waits_for_no_resolve_while_it_reads_stores_that_stay() {
    let w = World::new();
    let stores = std::sync::Arc::new(w.stores(volume_a));
    stores.resolve(&w.folder("kept")).unwrap();
    let held = stores.lock().unwrap();
    let sweeping = stores.clone();
    let swept = within_five_seconds(move || sweeping.collect(std::time::Duration::ZERO).unwrap());
    drop(held);
    assert_eq!(swept, Some(0), "the sweep finished while another process held the stores");
}

#[test]
fn a_known_folder_answers_while_another_folder_is_still_resolving() {
    let memo: &'static Memo = Box::leak(Box::default());
    let (known, new) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let store = |path: &Path| FolderStore { id: String::new(), cache_dir: path.join("cache"), record_dir: path.join("record") };
    let known_store = memoized(memo, known.path(), |p| Ok(store(p))).unwrap();
    let (started, resolving) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let new_path = new.path().to_path_buf();
    let slow = std::thread::spawn(move || {
        memoized(memo, &new_path, |p| {
            started.send(()).unwrap();
            released.recv().unwrap();
            Ok(store(p))
        })
    });
    resolving.recv().unwrap();
    let known_path = known.path().to_path_buf();
    let answered = within_five_seconds(move || memoized(memo, &known_path, |_| panic!("a known folder is not resolved again")).unwrap().record_dir);
    release.send(()).unwrap();
    assert!(slow.join().unwrap().is_ok());
    assert_eq!(answered, Some(known_store.record_dir), "the known folder waited for another folder's resolve");
}

/// `store`'s build output with generations `ids`, each one 1000-byte file, the
/// last of them current, staging present, and the size a seal would record.
fn sealed_store(store: &FolderStore, ids: &[&str]) {
    let build = store.cache_dir.join("build");
    let gens = build.join("generations");
    for id in ids {
        let dir = gens.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), vec![b'x'; 1000]).unwrap();
        let files = std::collections::HashMap::from([("index.html".to_string(), "100644:x".to_string())]);
        crate::build::store_gc::WholeGeneration::record(&dir, id, &files, &Default::default()).unwrap().save(&gens);
    }
    std::fs::write(build.join("current.generation"), ids.last().unwrap()).unwrap();
    std::fs::create_dir_all(build.join("staging")).unwrap();
    std::fs::write(build.join("staging").join("index.html"), "x").unwrap();
    crate::build::store_gc::record_store_size(&build);
}

fn generations(store: &FolderStore) -> Vec<String> {
    let mut ids = crate::build::store_gc::list_generations(&store.cache_dir.join("build").join("generations"));
    ids.sort();
    ids
}

#[test]
fn over_budget_a_live_store_keeps_only_its_current_generation_and_stage() {
    let w = World::new();
    let _home = crate::infra::home::thread_moss_home(&w.tmp.path().join("home"));
    let stores = w.stores(volume_a);
    let store = stores.resolve(&w.folder("live")).unwrap();
    sealed_store(&store, &["a", "b", "c"]);

    assert_eq!(stores.trim_live(10_000).unwrap(), 0, "under budget nothing goes");
    assert_eq!(generations(&store), ["a", "b", "c"]);
    assert_eq!(stores.trim_live(1).unwrap(), 1);
    assert_eq!(generations(&store), ["c"]);
    assert!(store.cache_dir.join("build/staging/index.html").exists());
    assert_eq!(crate::build::store_gc::recorded_store_size(&store.cache_dir.join("build")), Some(2000), "the size is recorded again");
}

#[test]
fn a_store_whose_folder_is_building_keeps_its_generations() {
    let w = World::new();
    let _home = crate::infra::home::thread_moss_home(&w.tmp.path().join("home"));
    let stores = w.stores(volume_a);
    let folder = w.folder("building");
    let store = stores.resolve(&folder).unwrap();
    sealed_store(&store, &["a", "b"]);

    let build = crate::infra::folder_lock::try_acquire_named(&folder, "build").unwrap().unwrap();
    assert_eq!(stores.trim_live(1).unwrap(), 0);
    assert_eq!(generations(&store), ["a", "b"]);
    drop(build);
    assert_eq!(stores.trim_live(1).unwrap(), 1);
}

#[test]
fn a_store_a_publish_has_pinned_keeps_its_generations() {
    let w = World::new();
    let _home = crate::infra::home::thread_moss_home(&w.tmp.path().join("home"));
    let stores = w.stores(volume_a);
    let folder = w.folder("publishing");
    let store = stores.resolve(&folder).unwrap();
    sealed_store(&store, &["a", "b"]);

    let pin = crate::infra::folder_lock::hold_publish_pin(&folder).unwrap();
    assert_eq!(stores.trim_live(1).unwrap(), 0);
    assert_eq!(generations(&store), ["a", "b"]);
    drop(pin);
    assert_eq!(stores.trim_live(1).unwrap(), 1);
}

#[test]
fn a_store_whose_folder_has_a_serve_owner_is_not_evicted() {
    let w = World::new();
    let _home = crate::infra::home::thread_moss_home(&w.tmp.path().join("home"));
    let stores = w.stores(volume_a);
    let folder = w.folder("served");
    let store = stores.resolve(&folder).unwrap();
    sealed_store(&store, &["a", "b"]);

    let owner = crate::infra::folder_lock::lock_path_named(&folder, "owner").unwrap();
    std::fs::create_dir_all(owner.parent().unwrap()).unwrap();
    std::fs::write(&owner, "").unwrap();
    assert_eq!(stores.trim_live(1).unwrap(), 0);
    assert_eq!(generations(&store), ["a", "b"]);
}

#[test]
fn the_store_unused_longest_is_trimmed_before_a_more_recent_one() {
    let w = World::new();
    let _home = crate::infra::home::thread_moss_home(&w.tmp.path().join("home"));
    let stores = w.stores(volume_a);
    let (x, y) = (stores.resolve(&w.folder("x")).unwrap(), stores.resolve(&w.folder("y")).unwrap());
    for store in [&x, &y] {
        sealed_store(store, &["a", "b", "c"]);
    }
    // The listing gives the older one last, so the order cannot come from it.
    let listed: Vec<_> = std::fs::read_dir(&stores.record_root).unwrap().flatten().map(|e| e.path()).collect();
    let (older, newer) = if listed.iter().position(|p| *p == x.record_dir) > listed.iter().position(|p| *p == y.record_dir) { (&x, &y) } else { (&y, &x) };
    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::open(older.record_dir.join(RECORD)).unwrap().set_modified(hour_ago).unwrap();

    // 8000 recorded; one store trimmed leaves 6000.
    assert_eq!(stores.trim_live(6000).unwrap(), 1);
    assert_eq!(generations(older), ["c"]);
    assert_eq!(generations(newer), ["a", "b", "c"]);
}
