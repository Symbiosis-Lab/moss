use super::*;

use crate::build::manifest::PendingManifest;
use crate::moss_paths::MossPaths;

/// A project root with `.moss/` laid out and one indexable page in staging.
fn project(page: &str) -> (tempfile::TempDir, MossPaths) {
    let root = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(root.path());
    mp.ensure_dirs().unwrap();
    let stage = mp.staging_dir();
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join("index.html"), page).unwrap();
    (root, mp)
}

const PAGE: &str = "<html lang=\"en\"><body><h1>Moss</h1>\
                    <p>A preview and publish tool for static websites.</p></body></html>";

fn bundle_keys(pending: &PendingManifest) -> Vec<String> {
    let mut keys: Vec<String> = pending
        .files()
        .keys()
        .filter(|k| k.starts_with("_moss/pagefind/"))
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// Adopt as an app build does: a lane will index the frozen generation later.
fn adopt(mp: &MossPaths, pending: &mut PendingManifest) -> Adoption {
    adopt_into(mp, &mp.staging_dir(), pending, true, Freshness::Lane)
}

// ── The gate ────────────────────────────────────────────────────────────────

/// With search off, adoption must register nothing and index nothing — the
/// nav button and the bundle must never disagree about whether search exists.
#[test]
fn disabled_search_registers_nothing_and_indexes_nothing() {
    let (_root, mp) = project(PAGE);
    let mut pending = PendingManifest::new(Default::default());

    let outcome = adopt_into(&mp, &mp.staging_dir(), &mut pending, false, Freshness::Lane);

    assert_eq!(outcome, Adoption::Disabled);
    assert!(bundle_keys(&pending).is_empty());
    assert!(!mp.index_receipt().exists());
    assert!(!mp.staging_dir().join("_moss/pagefind").exists());
}

// ── Nothing to adopt (ADR-045's first hole) ─────────────────────────────────

/// The first build after search is enabled has no receipt to adopt. Registering
/// nothing would be a silent bug rather than a slow path: mark-and-sweep would
/// prune the bucket, `remove_stale_files` would delete the bundle, and — the
/// part that reaches users — a no-op republish would have the deploy diff read
/// the index as *removed* and delete it off the live site. Under the old ticket
/// scheme a newer worker was always queued; under adoption none is. So this
/// must fall through to a synchronous index.
#[test]
fn a_first_build_with_no_receipt_indexes_synchronously_and_registers() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut pending = PendingManifest::new(Default::default());

    let outcome = adopt(&mp, &mut pending);

    let n = match outcome {
        Adoption::Indexed(n) => n,
        other => panic!("expected a synchronous index, got {:?}", other),
    };
    assert!(n > 0);
    let keys = bundle_keys(&pending);
    assert!(
        keys.iter().any(|k| k == "_moss/pagefind/pagefind.js"),
        "bundle entrypoint missing from the manifest: {:?}",
        keys
    );
    for key in &keys {
        assert!(
            mp.staging_dir().join(key).is_file(),
            "{key} registered but not laid into staging"
        );
    }
    assert!(mp.index_receipt().is_file(), "a receipt must be published");
}

// ── Steady state ────────────────────────────────────────────────────────────

/// Every subsequent build adopts the same receipt — same path set, byte for
/// byte — without running Pagefind again. This is the whole of the Finding 3
/// saving: the registered set is unchanged and the ~5.2 s is gone.
#[test]
fn later_builds_adopt_the_same_path_set_without_reindexing() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut first = PendingManifest::new(Default::default());
    adopt(&mp, &mut first);
    let expected = bundle_keys(&first);
    assert!(!expected.is_empty());

    let before = search::index_build_count();
    let mut second = PendingManifest::new(Default::default());
    let outcome = adopt(&mp, &mut second);

    assert!(matches!(outcome, Adoption::Adopted(_)), "got {:?}", outcome);
    assert_eq!(
        search::index_build_count(),
        before,
        "adoption must not run Pagefind"
    );
    assert_eq!(
        bundle_keys(&second),
        expected,
        "the registered path set must stay byte-identical across builds"
    );
}

/// Adoption re-lays the bundle when staging has lost it — a fresh staging tree
/// (or a sweep that ran before the receipt existed) must not leave the manifest
/// naming paths that are not on disk, which is the deploy error `manifest.rs`
/// documents.
#[test]
fn adoption_relays_files_that_staging_no_longer_has() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut first = PendingManifest::new(Default::default());
    adopt(&mp, &mut first);
    std::fs::remove_dir_all(mp.staging_dir().join("_moss/pagefind")).unwrap();

    let mut second = PendingManifest::new(Default::default());
    let outcome = adopt(&mp, &mut second);

    assert!(matches!(outcome, Adoption::Adopted(_)), "got {:?}", outcome);
    for key in bundle_keys(&second) {
        assert!(
            mp.staging_dir().join(&key).is_file(),
            "{key} registered but missing from staging"
        );
    }
}

// ── Freshness: a process nothing runs after ─────────────────────────────────

/// `lane::request` is issued only from the app-context seal tail. A CLI
/// `moss build` (or `build_sync`, or the snapshot harness) therefore has no
/// lane, and adopting its own earlier receipt verbatim froze the index at
/// whatever the *first* such build produced: add a page and it was never
/// searchable, delete one and search still returned it. `Freshness::Now` says
/// "nothing indexes after me", so every such build indexes.
#[test]
fn a_build_with_no_lane_reindexes_rather_than_adopting_its_own_stale_receipt() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut first = PendingManifest::new(Default::default());
    adopt_into(&mp, &mp.staging_dir(), &mut first, true, Freshness::Now);
    assert!(!bundle_keys(&first).is_empty());

    // The page set changes — the case the frozen receipt got wrong.
    std::fs::write(
        mp.staging_dir().join("second.html"),
        "<html lang=\"en\"><body><p>A second page nobody could find.</p></body></html>",
    )
    .unwrap();

    let before = search::index_build_count();
    let mut second = PendingManifest::new(Default::default());
    let outcome = adopt_into(&mp, &mp.staging_dir(), &mut second, true, Freshness::Now);

    assert!(matches!(outcome, Adoption::Indexed(_)), "got {:?}", outcome);
    assert_eq!(
        search::index_build_count() - before,
        1,
        "a build with no lane behind it must index its own output"
    );
    assert_eq!(
        read_receipt(&mp.index_dir()).unwrap().pages,
        2,
        "the fresh index must cover both pages"
    );
}

// ── Receipt/disk divergence (ADR-045's second hole) ─────────────────────────

/// An external delete or a cloud eviction of the holding area leaves a receipt
/// naming files that are gone. Registering them would put a path in the
/// manifest that is not on disk. Register nothing, drop the receipt — and the
/// next build takes the synchronous path, so it self-heals in one cycle.
#[test]
fn a_receipt_whose_files_are_gone_registers_nothing_and_is_dropped() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut first = PendingManifest::new(Default::default());
    adopt(&mp, &mut first);
    let receipt = read_receipt(&mp.index_dir()).expect("receipt published");
    std::fs::remove_dir_all(holding_dir(&mp.index_dir(), receipt.fp())).unwrap();

    let mut second = PendingManifest::new(Default::default());
    let outcome = adopt(&mp, &mut second);

    assert_eq!(outcome, Adoption::Diverged);
    assert!(
        bundle_keys(&second).is_empty(),
        "a diverged bundle must register nothing at all, not a partial set"
    );
    assert!(!mp.index_receipt().exists(), "the bad receipt must be dropped");

    let mut third = PendingManifest::new(Default::default());
    assert!(
        matches!(adopt(&mp, &mut third), Adoption::Indexed(_)),
        "divergence must self-heal on the next build"
    );
}

/// A copy that fails partway must register **nothing**. Registering the k
/// entries copied so far is worse than registering none: the sweep deletes the
/// other n−k out of staging and the deploy diff deletes them off the live site,
/// leaving `pagefind.js` fetching shards that 404 — a corrupt index rather than
/// an absent one.
#[test]
fn a_copy_that_fails_partway_registers_nothing() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut first = PendingManifest::new(Default::default());
    adopt(&mp, &mut first);
    let receipt = read_receipt(&mp.index_dir()).expect("receipt published");
    assert!(receipt.files.len() > 1, "need several files to fail partway");

    // Wipe staging's copy so every entry is re-laid, and make the LAST
    // destination a directory so its copy — and only its copy — fails.
    std::fs::remove_dir_all(mp.staging_dir().join("_moss/pagefind")).unwrap();
    let (last, _) = receipt.files.last().unwrap();
    std::fs::create_dir_all(mp.staging_dir().join("_moss/pagefind").join(last)).unwrap();

    let mut second = PendingManifest::new(Default::default());
    let outcome = adopt(&mp, &mut second);

    assert_eq!(outcome, Adoption::Diverged);
    assert!(
        bundle_keys(&second).is_empty(),
        "a failed lay must register nothing, not the files it managed to copy"
    );
}

/// The divergence branch deletes `receipt.json` by path. It must first check
/// that the file still names the bundle that diverged — otherwise a receipt
/// published in the meantime (by a second moss instance on the same folder) is
/// destroyed, and the next build pays a full synchronous index for it.
#[test]
fn the_divergence_branch_does_not_delete_a_receipt_it_never_read() {
    let (_root, mp) = project(PAGE);
    let index_dir = mp.index_dir();
    let fresh = BundleReceipt { fp: "beef".into(), pages: 1, files: vec![] };
    std::fs::create_dir_all(&index_dir).unwrap();
    std::fs::write(mp.index_receipt(), serde_json::to_vec(&fresh).unwrap()).unwrap();

    let stale = BundleReceipt { fp: "dead".into(), pages: 1, files: vec![] };
    with_holding(&index_dir, |h| drop_receipt_if_still(h, &index_dir, &stale));

    assert_eq!(
        read_receipt(&index_dir).map(|r| r.fp),
        Some("beef".to_string()),
        "a receipt this build never read must survive"
    );
}

// ── The coverage guard (a partial index is never authoritative) ─────────────

/// The failure this guard exists for: `publish_bundle` is handed the
/// fingerprint of the **complete** page set, and nothing in the indexer's
/// return value proves the bundle covers it — an iCloud-evicted page, a GC'd
/// generation and an empty site all arrive as "0 pages". Stamped with a real
/// fingerprint, an empty bundle is absorbing: the lane skips every later
/// request for that page set, `settle_for_publish`'s `intact` test is vacuously
/// true over an empty file list, adoption registers nothing, and the deploy
/// diff deletes the search index off the live site. Refuse the stamp instead.
#[test]
fn a_bundle_that_covers_fewer_pages_than_its_page_set_is_refused() {
    let (_root, mp) = project(PAGE);
    let index_dir = mp.index_dir();
    let want = PageSet { fp: PageSetFp(0xfeed), pages: 3 };

    // Nothing could be read at all — a generation GC'd or evicted mid-walk.
    let nothing = search::SearchIndex { files: vec![], pages: 0, skipped: 0 };
    let err = with_holding(&index_dir, |h| publish_bundle(h, &index_dir, want, &nothing))
        .expect_err("a zero-page bundle must not be stamped with a 3-page fingerprint");
    assert!(err.contains("0 of 3"), "unhelpful refusal: {err}");
    assert!(!mp.index_receipt().exists(), "a refused publish writes no receipt");

    // Two of three pages read — the invisible variant: search silently returns
    // no hits for the third and no later build re-indexes it.
    let partial = search::SearchIndex {
        files: vec![search::SearchIndexFile { rel_path: "pagefind.js".into(), bytes: vec![1] }],
        pages: 2,
        skipped: 1,
    };
    assert!(
        with_holding(&index_dir, |h| publish_bundle(h, &index_dir, want, &partial)).is_err(),
        "a bundle missing one page must not be stamped with the whole page set"
    );
    assert!(!mp.index_receipt().exists());
}

/// A refusal must not disturb the bundle already published: the previous
/// receipt stays adoptable, so the site keeps the index it has until a pass
/// succeeds.
#[test]
fn a_refused_publish_leaves_the_previous_bundle_adoptable() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let index_dir = mp.index_dir();
    let good = search::build_search_index(&mp.staging_dir()).unwrap();
    let one = PageSet { fp: PageSetFp(0x1111), pages: 1 };
    with_holding(&index_dir, |h| publish_bundle(h, &index_dir, one, &good)).unwrap();

    let nothing = search::SearchIndex { files: vec![], pages: 0, skipped: 0 };
    let two = PageSet { fp: PageSetFp(0x2222), pages: 2 };
    assert!(with_holding(&index_dir, |h| publish_bundle(h, &index_dir, two, &nothing)).is_err());

    let mut pending = PendingManifest::new(Default::default());
    assert!(matches!(adopt(&mp, &mut pending), Adoption::Adopted(_)));
    assert!(!bundle_keys(&pending).is_empty(), "the good bundle must survive a refusal");
}

/// A page set with nothing to index publishes an *empty* receipt rather than
/// none. Without it the fallback would re-run a whole index on every build of a
/// site that has no indexable content, forever. This is the one case where an
/// empty bundle is authoritative, and the receipt records the measurement that
/// makes it so.
#[test]
fn an_unindexable_site_publishes_an_empty_receipt_instead_of_reindexing() {
    let _serialize = search::lock_index_counter();
    let root = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(root.path());
    mp.ensure_dirs().unwrap();

    let mut first = PendingManifest::new(Default::default());
    assert_eq!(adopt(&mp, &mut first), Adoption::Empty);
    assert_eq!(read_receipt(&mp.index_dir()).unwrap().pages, 0);

    let before = search::index_build_count();
    let mut second = PendingManifest::new(Default::default());
    assert_eq!(adopt(&mp, &mut second), Adoption::Empty);
    assert_eq!(
        search::index_build_count(),
        before,
        "an empty receipt must stop the fallback from re-indexing every build"
    );
    assert!(bundle_keys(&second).is_empty());
}

// ── PageSet ─────────────────────────────────────────────────────────────────

/// The fingerprint is HTML-only on purpose: a re-encoded `.webp` moves the
/// manifest but not one word of indexable text, and re-indexing 386 pages for
/// it would put the corpus cost back on every image edit. The page *count*
/// follows the same filter, or the coverage guard would demand pages that are
/// not pages.
#[test]
fn the_page_set_counts_and_fingerprints_html_entries_only() {
    let mut files = HashMap::new();
    files.insert("index.html".to_string(), "100644:aaa".to_string());
    let html_only = PageSet::of(&files);
    assert_eq!(html_only.pages, 1);

    files.insert("img/cover.webp".to_string(), "100644:bbb".to_string());
    assert_eq!(PageSet::of(&files), html_only, "an asset moved the page set");

    files.insert("about/index.html".to_string(), "100644:ccc".to_string());
    let two = PageSet::of(&files);
    assert_ne!(two.fp, html_only.fp, "a new page did not move the fingerprint");
    assert_eq!(two.pages, 2);
}

/// Same content, same fingerprint, regardless of `HashMap` iteration order —
/// the property `compute_manifest_generation_id` needed a `BTreeMap` for, and
/// the reason a lane keyed on it can skip a no-op save.
#[test]
fn the_page_set_fingerprint_is_order_independent() {
    let mut a = HashMap::new();
    let mut b = HashMap::new();
    for i in 0..64 {
        a.insert(format!("p{i}/index.html"), format!("100644:{i:04x}"));
    }
    for i in (0..64).rev() {
        b.insert(format!("p{i}/index.html"), format!("100644:{i:04x}"));
    }
    assert_eq!(PageSet::of(&a), PageSet::of(&b));
}

// ── The publish sync point ──────────────────────────────────────────────────

/// `settle_for_publish` must be free when the lane has already published a
/// receipt for exactly the page set on disk — otherwise every publish would
/// pay for a re-index and a rebuild it does not need.
#[test]
fn settle_is_a_no_op_when_the_receipt_already_covers_the_published_page_set() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    // Publish a bundle stamped with the fingerprint of the manifest on disk.
    let mut hashes = crate::types::content::SiteHashes::default();
    hashes.files.insert("index.html".to_string(), "100644:abc".to_string());
    std::fs::write(mp.hashes(), serde_json::to_string(&hashes).unwrap()).unwrap();
    let want = PageSet::of(&hashes.files);
    let produced = search::build_search_index(&mp.staging_dir()).unwrap();
    with_holding(&mp.index_dir(), |h| publish_bundle(h, &mp.index_dir(), want, &produced))
        .unwrap();

    let before = search::index_build_count();
    assert!(!settle_for_publish(&mp, true), "a fresh index must not force a rebuild");
    assert_eq!(search::index_build_count(), before);
}

/// …and must re-index (and say so) when it does not. A publish that shipped the
/// previous page set's bundle would hand readers a search box that cannot find
/// the page they just published.
#[test]
fn settle_reindexes_when_the_receipt_is_for_an_older_page_set() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut pending = PendingManifest::new(Default::default());
    adopt(&mp, &mut pending); // receipt stamped UNKNOWN
    // `current` must resolve for the settle to have something to index.
    let gen = mp.generation_dir("test");
    std::fs::create_dir_all(&gen).unwrap();
    std::fs::write(gen.join("index.html"), PAGE).unwrap();
    mp.set_current_ptr("test").unwrap();

    let mut hashes = crate::types::content::SiteHashes::default();
    hashes.files.insert("index.html".to_string(), "100644:abc".to_string());
    std::fs::write(mp.hashes(), serde_json::to_string(&hashes).unwrap()).unwrap();

    assert!(settle_for_publish(&mp, true), "a stale index must force a rebuild");
    assert_eq!(
        read_receipt(&mp.index_dir()).unwrap().fp(),
        PageSetFp::of(&hashes.files),
        "settle must leave a receipt for the page set being published"
    );
}

/// An empty bundle must not satisfy a page set that has pages. `all` over an
/// empty file list is vacuously true, so without the recorded page count the
/// intactness test would pass and a publish would ship no index at all.
#[test]
fn settle_does_not_accept_an_empty_bundle_for_a_page_set_that_has_pages() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut hashes = crate::types::content::SiteHashes::default();
    hashes.files.insert("index.html".to_string(), "100644:abc".to_string());
    std::fs::write(mp.hashes(), serde_json::to_string(&hashes).unwrap()).unwrap();

    // A receipt carrying the right fingerprint but nothing behind it.
    let empty = BundleReceipt {
        fp: PageSetFp::of(&hashes.files).hex(),
        pages: 0,
        files: vec![],
    };
    std::fs::create_dir_all(mp.index_dir()).unwrap();
    std::fs::write(mp.index_receipt(), serde_json::to_vec(&empty).unwrap()).unwrap();

    let gen = mp.generation_dir("g");
    std::fs::create_dir_all(&gen).unwrap();
    std::fs::write(gen.join("index.html"), PAGE).unwrap();
    mp.set_current_ptr("g").unwrap();

    assert!(settle_for_publish(&mp, true), "an empty bundle must not pass for a real page set");
    let after = read_receipt(&mp.index_dir()).unwrap();
    assert_eq!(after.pages, 1);
    assert!(!after.files.is_empty());
}

/// Search off: no read, no index, no forced rebuild.
#[test]
fn settle_is_inert_when_search_is_disabled() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let before = search::index_build_count();
    assert!(!settle_for_publish(&mp, false));
    assert_eq!(search::index_build_count(), before);
}

// ── The lane ────────────────────────────────────────────────────────────────

/// A burst of saves must collapse to one index, and a request for a page set
/// that is already published must run none at all — the no-op save case the
/// whole of Finding 3 is about.
#[tokio::test(flavor = "multi_thread")]
async fn a_burst_of_requests_indexes_once_and_a_no_op_save_indexes_not_at_all() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let gen = mp.generation_dir("g1");
    std::fs::create_dir_all(&gen).unwrap();
    std::fs::write(gen.join("index.html"), PAGE).unwrap();
    let want = PageSet { fp: PageSetFp(0xfeed), pages: 1 };

    let before = search::index_build_count();
    for _ in 0..8 {
        request(&mp, "g1", want);
    }
    assert!(is_indexing(&mp, "g1"), "the lane's generation must be pinned against GC");

    // IDLE + one index + margin. The lane is level-triggered, so the eight
    // requests are one value, not a queue of eight.
    tokio::time::sleep(IDLE + Duration::from_secs(8)).await;

    assert_eq!(
        search::index_build_count() - before,
        1,
        "eight requests for one page set must run one index"
    );
    let receipt = read_receipt(&mp.index_dir()).expect("the lane published a receipt");
    assert_eq!(receipt.fp(), want.fp);
    assert_eq!(receipt.pages, 1);
    assert!(!receipt.files.is_empty());

    // The no-op save: same page set, already published.
    let before = search::index_build_count();
    request(&mp, "g1", want);
    tokio::time::sleep(IDLE + Duration::from_secs(2)).await;
    assert_eq!(
        search::index_build_count(),
        before,
        "a request for the published page set must run zero work"
    );

    // A newer generation releases the older one: the pin exists to stop GC
    // deleting a tree the lane is about to read, not to retain it forever.
    let gen2 = mp.generation_dir("g2");
    std::fs::create_dir_all(&gen2).unwrap();
    std::fs::write(gen2.join("index.html"), PAGE).unwrap();
    request(&mp, "g2", PageSet { fp: PageSetFp(0xf00d), pages: 1 });
    tokio::time::sleep(IDLE + Duration::from_secs(8)).await;
    assert!(!is_indexing(&mp, "g1"), "a superseded generation must be released");
    assert!(is_indexing(&mp, "g2"), "the outstanding request stays pinned");
}

/// The lane must **park** between requests. An earlier shape always broke out
/// of its debounce on the `IDLE` timer and re-read the same request, which on a
/// healthy folder was a wakeup plus a ~440-entry JSON parse every 2 s for the
/// life of the app — and on a folder whose index kept failing, a whole-corpus
/// pagefind run every ~7 s forever. Both are invisible to a test that counts
/// index builds, so this counts passes; the failing-index half is covered by
/// the request below, whose generation does not exist.
#[tokio::test(flavor = "multi_thread")]
async fn a_lane_whose_index_cannot_be_published_retries_once_per_request_not_forever() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    // A generation the GC already removed: the walk finds nothing, and the
    // coverage guard refuses to stamp an empty bundle on a one-page set.
    let want = PageSet { fp: PageSetFp(0xbeef), pages: 1 };

    let builds = search::index_build_count();
    request(&mp, "gone", want);
    tokio::time::sleep(IDLE + Duration::from_secs(3)).await;

    assert_eq!(
        search::index_build_count() - builds,
        1,
        "one request must produce one attempt, not a retry loop"
    );
    assert!(
        !mp.index_receipt().exists(),
        "an index that covered no pages must not be published as authoritative"
    );

    let passes = lane_passes();
    tokio::time::sleep(IDLE * 3).await;
    assert_eq!(
        lane_passes(),
        passes,
        "an idle lane must be parked on its channel, not spinning on a timer"
    );
}
