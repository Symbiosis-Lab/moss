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
