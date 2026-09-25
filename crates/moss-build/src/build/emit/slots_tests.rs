//! Rendered HTML ships by CAS object id, not by re-reading the shared stage.
//!
//! Every test here drives the real pipeline (`pipeline::run`, awaited to its
//! seal) and asserts on the `SealedManifest` it hands back — a hand-built
//! manifest never acquires an oid, which is the wiring these tests are about.
//! `build_test_full` cannot serve: it pins `IncrementalGates::default()`, so no
//! test build through it has ever carried a page, and a watch rebuild — the only
//! place two builds of one folder overlap — is almost entirely carried pages.

use super::*;
use crate::build::assets::paths::compute_binary_hash;
use crate::build::cache::ObjectStore;
use crate::build::manifest::SealedManifest;
use crate::build::pipeline::{run, PipelineRunOutput};
use crate::build::render::IncrementalGates;
use crate::build::scan::scan::scan_folder;
use crate::build::ship::{apply_transform, transform_for};
use std::path::PathBuf;
use tempfile::TempDir;

const CARRY: IncrementalGates = IncrementalGates { render_skip: true, parse_cache: false };

/// A vault in a temp dir, built the way the test harness builds everything.
struct Site {
    dir: TempDir,
}

impl Site {
    fn new() -> Self {
        // Not `TempDir::new()`: its `.tmpXXXX` name is a hidden directory, which
        // the scan skips wholesale — the build then renders an empty vault.
        Site { dir: tempfile::Builder::new().prefix("moss_build_test_").tempdir().unwrap() }
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn remove(&self, rel: &str) {
        std::fs::remove_file(self.dir.path().join(rel)).unwrap();
    }

    fn paths(&self) -> MossPaths {
        MossPaths::new(self.dir.path())
    }

    fn stage(&self, rel: &str) -> PathBuf {
        self.paths().staging_dir().join(rel)
    }

    fn objects(&self) -> ObjectStore {
        ObjectStore::new(self.paths().cache_objects())
    }

    /// One real build, awaited to its seal, persisted so the next `build` sees
    /// this one as its predecessor.
    fn build(&self, gates: IncrementalGates) -> SealedManifest {
        let folder = self.dir.path().to_str().unwrap();
        let ps = scan_folder(folder).expect("scan");
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let root = crate::vault::paths::VaultRoot::resolve(folder);
        let port = crate::build::ports::port_of_this_build(None, None);
        let keys = crate::build::ports::CacheKeyInputs { builder: "test-builder-fingerprint".to_string() };
        rt.block_on(async {
            let PipelineRunOutput { bg_handle, .. } = run(
                &root,
                None,
                None,
                &port,
                None,
                Some(Box::new(|_, _, _| Ok(ResolvedSlots::empty()))),
                &ps,
                None,
                gates,
                crate::build::feeds::search_lane::Freshness::Now,
                &keys,
            )
            .map_err(crate::build::outcome::BuildStopped::into_message)
            .expect("build must succeed");
            let (sealed, _lease) = bg_handle
                .expect("every build returns a handle to seal through")
                .await_completion()
                .await
                .expect("seal");
            sealed.write_to_disk(&self.paths().hashes()).unwrap();
            sealed
        })
    }
}

/// The hash the manifest holds for bytes: the shipped (stripped) form, mode-tagged.
fn entry_for(key: &str, bytes: &[u8]) -> String {
    format!("100644:{}", compute_binary_hash(&apply_transform(transform_for(key), bytes)))
}

/// The invariant a rendered page must satisfy once the slot pass has run: it
/// names a CAS blob, the blob holds exactly what the stage holds, and the
/// manifest's hash is that blob's shipped hash — so `ship_phase` reading the blob
/// reproduces the hash whatever happens to the stage afterwards.
fn assert_ships_by_oid(site: &Site, sealed: &SealedManifest, key: &str) {
    let oid = sealed.staged_oid(key).unwrap_or_else(|| {
        panic!(
            "{key} has no CAS source, so it ships from the mutable stage. files: {:?}",
            sealed.files().keys().collect::<Vec<_>>()
        )
    });
    let blob_path = site.objects().get_path(oid).unwrap_or_else(|| panic!("{key}: oid {oid} names no blob"));
    let blob = std::fs::read(blob_path).unwrap();
    assert_eq!(
        blob,
        std::fs::read(site.stage(key)).unwrap(),
        "{key}: the blob must hold exactly the bytes in the stage"
    );
    assert_eq!(
        sealed.files().get(key),
        Some(&entry_for(key, &blob)),
        "{key}: the manifest hash must be the shipped hash of the blob's bytes"
    );
}

fn empty_site_result() -> crate::types::content::SiteResult {
    crate::types::content::SiteResult {
        page_count: 0,
        site_build_dir: String::new(),
        site_title: String::new(),
        hashes: crate::types::content::SiteHashes::default(),
        deferred_paths: Vec::new(),
        missing_references: Vec::new(),
    }
}

fn touch_apart() {
    // mtimes are compared between builds; keep them off the same tick.
    std::thread::sleep(std::time::Duration::from_millis(30));
}

// ---------------------------------------------------------------------------

/// Pages with slot markers, the ordinary case: cold, then warm (every page
/// re-rendered, the slot pass answering from its cache).
#[test]
fn rendered_pages_ship_by_oid_cold_and_warm() {
    let site = Site::new();
    site.write("index.md", "# Home\n\nWelcome.");
    site.write("about.md", "# About\n\nWho we are.");

    // Build one is cold. Build two re-renders every page, and the pass answers
    // from the cache build one filled.
    for _ in 0..2 {
        let sealed = site.build(IncrementalGates::default());
        for key in ["index.html", "about/index.html"] {
            assert_ships_by_oid(&site, &sealed, key);
        }
    }
}

/// A page with NO slot marker — a redirect stub — is the slot pass's no-op arm:
/// nothing to inject, so nothing forces a blob into the CAS unless the arm mints
/// one itself. A marked page stays green even if that arm is reverted, so this
/// is the page that pins it. Cold, then warm (the cached no-op).
#[test]
fn a_page_with_no_slot_marker_ships_by_oid_cold_and_warm() {
    let site = Site::new();
    site.write("index.md", "# Home");
    site.write("about.md", "# About\n\nNot a redirect target.");
    // Seeds `emit_redirect_stubs`: the old URL is not a live page, so the stub
    // survives the merge and is emitted as `old-about/index.html`.
    site.write(".moss/data/redirects.json", r#"{"old-about/": "about/"}"#);

    let cold = site.build(IncrementalGates::default());
    let stub = std::fs::read_to_string(site.stage("old-about/index.html")).expect("the redirect stub is staged");
    assert!(!stub.contains("<!-- slot:"), "the fixture must be a page without markers");
    assert_ships_by_oid(&site, &cold, "old-about/index.html");

    let warm = site.build(IncrementalGates::default());
    assert_ships_by_oid(&site, &warm, "old-about/index.html");
}

/// Watch mode: the second build carries every page nothing moved for, so most
/// pages reach the slot pass as the PREVIOUS build's already-injected bytes — a
/// cold no-op the first time, a cached one after. Two real sequential builds
/// with `render_skip` on, because that is the only path that carries.
#[test]
fn carried_pages_ship_by_oid_and_keep_their_manifest_entry() {
    let site = Site::new();
    site.write("index.md", "# Home");
    site.write("a.md", "# A\n\nfirst");
    site.write("b.md", "# B\n\nstable");
    site.write("c.md", "# C\n\nstable");

    let first = site.build(CARRY);
    let b_before = std::fs::metadata(site.stage("b/index.html")).unwrap().modified().unwrap();
    let a_before = std::fs::metadata(site.stage("a/index.html")).unwrap().modified().unwrap();
    touch_apart();

    site.write("a.md", "# A\n\nedited");
    let second = site.build(CARRY);

    // The gate is what makes this test about carrying rather than about a
    // second full render: an edited page is rewritten, an untouched one is not.
    assert_ne!(
        std::fs::metadata(site.stage("a/index.html")).unwrap().modified().unwrap(),
        a_before,
        "the edited page must have been re-rendered"
    );
    assert_eq!(
        std::fs::metadata(site.stage("b/index.html")).unwrap().modified().unwrap(),
        b_before,
        "the untouched page must have been carried, not re-rendered"
    );
    for key in ["b/index.html", "c/index.html", "index.html", "a/index.html"] {
        assert_ships_by_oid(&site, &second, key);
    }
    // A carried page keeps the entry the previous build sealed for it.
    assert_eq!(second.files().get("b/index.html"), first.files().get("b/index.html"));

    // And once more, no edit: every page carried, the no-op now answered from
    // the cache the previous build wrote.
    touch_apart();
    let third = site.build(CARRY);
    for key in ["b/index.html", "c/index.html", "a/index.html", "index.html"] {
        assert_ships_by_oid(&site, &third, key);
    }
    assert_eq!(third.files().get("b/index.html"), first.files().get("b/index.html"));
}

/// The stage keeps what the previous build left there, and the slot pass walks
/// all of it. A page this build did not render is not this build's to register:
/// registering it would keep a deleted page in the manifest, and its CAS blob
/// would then answer "present" to the presence pass — the page would ship from
/// the CAS for good. One folder-index page and one non-index page, because the
/// stale-HTML sweep that runs later only knows about `index*.html`.
#[test]
fn a_deleted_page_left_in_the_stage_is_not_registered_by_the_slot_pass() {
    let site = Site::new();
    site.write("index.md", "# Home");
    site.write("gone.md", "# Gone\n\nAbout to be deleted.");
    site.write("sketch.html", "<html><body>an interactive sketch</body></html>");
    let first = site.build(IncrementalGates::default());
    assert!(first.files().contains_key("gone/index.html"), "fixture: the page exists in build one");
    assert!(first.files().contains_key("sketch.html"), "fixture: the sketch exists in build one");

    site.remove("gone.md");
    site.remove("sketch.html");
    let second = site.build(IncrementalGates::default());

    let deleted = ["gone/index.html", "sketch.html"];
    let leaked: Vec<&str> = deleted.into_iter().filter(|key| second.files().contains_key(*key)).collect();
    assert!(leaked.is_empty(), "deleted from the vault but still in the generation: {leaked:?}");
    for key in deleted {
        assert_eq!(second.staged_oid(key), None, "{key}: no CAS source for a deleted page");
    }
}

/// A page a LATER step re-registers after the slot pass gave it an oid must lose
/// that oid: the oid names the bytes of the earlier registration. The notebook
/// step does this to its viewer pages, which are written after the pass. Isolates
/// the last registration winning: the render phase registered the page, so the
/// pass's ownership check lets it through and only `register_with_hash` is left
/// to drop the oid. `a_deleted_page_left_in_the_stage_is_not_registered_by_the_slot_pass`
/// is the test for the ownership check itself.
#[test]
fn a_page_re_registered_after_the_slot_pass_loses_the_passes_oid() {
    use crate::build::manifest::{HashBucket, PendingManifest};
    use crate::build::served_path::ServedPath;

    let tmp = TempDir::new().unwrap();
    let paths = MossPaths::new(tmp.path());
    let stage = paths.staging_dir();
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join("page.html"), "<html>rendered</html>").unwrap();

    let page = ServedPath::from_source("page.html").unwrap();
    let mut pending = PendingManifest::new(crate::types::content::SiteHashes::default());
    // The render phase registered it: this build owns the page.
    pending.register(&page, b"<html>rendered</html>", HashBucket::Files);
    let mut site_result = empty_site_result();
    apply_to_stage_and_manifest(&paths, &stage, &ResolvedSlots::empty(), &mut pending, &mut site_result, None)
        .expect("slot pass");

    pending.register_hashed(&page, "0123456789abcdef", HashBucket::Files);

    assert_eq!(pending.seal().staged_oid("page.html"), None);
}

/// The pipeline compares `site_result.hashes` with the previous build's manifest
/// to decide whether anything changed, and hands a copy to the watcher. It holds
/// the render phase's entries, which are the hashes of the pages BEFORE injection;
/// the pass has to bring them to the entries it gave the manifest, or every page
/// that injection rewrites reads as changed against a manifest that recorded the
/// injected bytes. No other test looks at `site_result` after the pass, which is
/// how dropping that update went unnoticed by the whole suite.
///
/// Twice: once with a store that takes the blob, once with one that cannot, since
/// the page without an oid takes its own branch of the receipt loop.
#[test]
fn the_slot_pass_brings_the_change_detection_hashes_to_the_manifests_entries() {
    use crate::build::manifest::{HashBucket, PendingManifest};
    use crate::build::served_path::ServedPath;

    for store_takes_blobs in [true, false] {
        let tmp = TempDir::new().unwrap();
        let paths = MossPaths::new(tmp.path());
        let stage = paths.staging_dir();
        std::fs::create_dir_all(&stage).unwrap();
        if !store_takes_blobs {
            // A file where the object store's directory should be.
            std::fs::create_dir_all(paths.cache_objects().parent().unwrap()).unwrap();
            std::fs::write(paths.cache_objects(), "not a directory").unwrap();
        }
        let rendered = "<html><head><!-- slot:head-end --></head><body></body></html>";
        std::fs::write(stage.join("page.html"), rendered).unwrap();

        let page = ServedPath::from_source("page.html").unwrap();
        let mut pending = PendingManifest::new(crate::types::content::SiteHashes::default());
        pending.register(&page, rendered.as_bytes(), HashBucket::Files);
        // What `generate_blocking_content` leaves in `SiteResult`: the manifest as
        // the render phase saw it.
        let mut site_result = empty_site_result();
        site_result.hashes = pending.as_parts_clone().0;
        let render_phase_entry = site_result.hashes.files["page.html"].clone();

        apply_to_stage_and_manifest(&paths, &stage, &ResolvedSlots::empty(), &mut pending, &mut site_result, None)
            .expect("slot pass");

        let final_entry = entry_for("page.html", &std::fs::read(stage.join("page.html")).unwrap());
        assert_ne!(render_phase_entry, final_entry, "fixture: injection changed the page's bytes");
        assert_eq!(
            pending.files().get("page.html"),
            Some(&final_entry),
            "[store takes blobs: {store_takes_blobs}] the manifest holds the final entry"
        );
        assert_eq!(
            site_result.hashes.files.get("page.html"),
            Some(&final_entry),
            "[store takes blobs: {store_takes_blobs}] and so does the copy the pipeline compares against the previous build"
        );
    }
}

/// A full or unwritable object store must not fail a build that succeeds today:
/// a page loses its immutable copy, not its place in the generation. The pages
/// fall back to the stage-path fingerprint `advertise_sealed` stamps after seal.
///
/// Losing the oid must not cost the page its final hash. Such a page ships from
/// the stage, and the stage holds the injected bytes, so a manifest still carrying
/// the render phase's hash would seal a generation whose bytes match no entry.
#[test]
fn an_unwritable_object_store_costs_the_pages_their_oid_not_the_build() {
    let site = Site::new();
    site.write("index.md", "# Home");
    site.write("about.md", "# About");
    // A file where the object store's directory should be: every write beneath
    // it fails, on every platform, whoever runs the test.
    let objects = site.paths().cache_objects();
    std::fs::create_dir_all(objects.parent().unwrap()).unwrap();
    std::fs::write(&objects, "not a directory").unwrap();

    let mut sealed = site.build(IncrementalGates::default());

    for key in ["index.html", "about/index.html"] {
        assert!(sealed.files().contains_key(key), "{key} must still be in the generation");
        assert_eq!(sealed.staged_oid(key), None, "{key}: nothing to name");
        let staged = std::fs::read(site.stage(key)).unwrap();
        assert_eq!(
            sealed.files().get(key),
            Some(&entry_for(key, &staged)),
            "{key}: without an oid it ships the staged bytes, so the entry must hash them"
        );
    }
    sealed.stamp_all_ship_fingerprints(&site.paths().staging_dir());
    for key in ["index.html", "about/index.html"] {
        assert!(sealed.ship_fingerprint(key).is_some(), "{key} must fall back to a fingerprint");
    }
}

/// The property ship-by-oid exists for, on a rendered page, through the real
/// `materialize_and_promote`. A second build finishes in the window between this
/// build's seal and its ship and rewrites the shared stage copy; the generation
/// must still get the bytes this build sealed a hash for.
///
/// Drives `materialize_and_promote`, never `ship_phase` with a hand-built store:
/// the `Some(&object_store)` that reaches `ship_phase` is wired inside
/// `materialize_and_promote`, and a test that supplies its own store passes
/// whether or not that wiring exists.
#[test]
fn materialize_and_promote_ships_the_sealed_bytes_of_a_page_the_stage_no_longer_holds() {
    let site = Site::new();
    site.write("index.md", "# Home");
    site.write("about.md", "# About\n\nThe bytes this build sealed.");
    let sealed = site.build(IncrementalGates::default());

    let key = "about/index.html";
    let sealed_bytes = std::fs::read(site.stage(key)).unwrap();
    std::fs::write(site.stage(key), "<html><body>SECOND BUILD</body></html>").unwrap();

    let mp = site.paths();
    crate::build::ship::materialize_and_promote(
        &sealed,
        &mp,
        &mp.staging_dir(),
        None,
        crate::build::ship::next_promotion_epoch(),
        None,
        crate::build::ship::ShipVerdict::Ship,
    )
    .expect("materialize");

    let shipped = std::fs::read(mp.generation_dir(sealed.generation_id()).join(key)).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&shipped),
        String::from_utf8_lossy(&apply_transform(transform_for(key), &sealed_bytes)),
        "the generation must hold the bytes this build sealed, not what the stage holds now"
    );
    assert_eq!(
        sealed.files().get(key),
        Some(&entry_for(key, &shipped)),
        "and what shipped must hash to what the manifest says, or deploy refuses the generation"
    );
}
