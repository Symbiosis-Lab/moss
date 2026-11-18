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

/// Adopt as a watch-session build does: a reader settles the index later.
fn adopt(mp: &MossPaths, pending: &mut PendingManifest) -> Adoption {
    adopt_into(mp, &mp.staging_dir(), pending, true, Freshness::OnRead)
}

/// Publish a bundle of the staged pages, as a settle does, then adopt it.
fn settle_and_adopt(mp: &MossPaths, pending: &mut PendingManifest) -> Adoption {
    let produced = search::build_search_index(&mp.staging_dir()).unwrap();
    with_holding(&mp.index_dir(), |h| publish_bundle(h, &mp.index_dir(), PageSet::UNKNOWN, None, &produced)).unwrap();
    adopt(mp, pending)
}

// ── The gate ────────────────────────────────────────────────────────────────

/// With search off, adoption must register nothing and index nothing — the
/// nav button and the bundle must never disagree about whether search exists.
#[test]
fn disabled_search_registers_nothing_and_indexes_nothing() {
    let (_root, mp) = project(PAGE);
    let mut pending = PendingManifest::new(Default::default());

    let outcome = adopt_into(&mp, &mp.staging_dir(), &mut pending, false, Freshness::OnRead);

    assert_eq!(outcome, Adoption::Disabled);
    assert!(bundle_keys(&pending).is_empty());
    assert!(!mp.index_receipt().exists());
    assert!(!mp.staging_dir().join("_moss/pagefind").exists());
}

// ── Nothing to adopt ────────────────────────────────────────────────────────

/// A build with no receipt indexes nothing: the index is built when search or
/// a publish reads it, and both settle before they do.
#[test]
fn a_build_with_no_receipt_registers_nothing_and_indexes_nothing() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut pending = PendingManifest::new(Default::default());
    let before = search::index_build_count();

    assert_eq!(adopt(&mp, &mut pending), Adoption::Unindexed);
    assert!(bundle_keys(&pending).is_empty());
    assert_eq!(search::index_build_count(), before);
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
    settle_and_adopt(&mp, &mut first);
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
    settle_and_adopt(&mp, &mut first);
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

/// Only a preview read or a publish settles the index. A CLI `moss build` (or
/// `build_sync`, or the snapshot harness) has neither after it, and adopting
/// its own earlier receipt verbatim would freeze the index at whatever the
/// *first* such build produced: add a page and it is never searchable, delete
/// one and search still returns it. `Freshness::Now` says "nothing indexes
/// after me", so every such build indexes.
#[test]
fn a_one_shot_build_reindexes_rather_than_adopting_its_own_stale_receipt() {
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
        "a build with no reader after it must index its own output"
    );
    assert_eq!(
        read_receipt(&mp.index_dir()).unwrap().pages,
        2,
        "the fresh index must cover both pages"
    );
}

/// The other half: a one-shot build over the same staged pages would index them
/// into the same bytes, so it serves the bundle it holds. A bundle a settle
/// indexed from a generation is never taken for one.
#[test]
fn a_one_shot_build_serves_the_bundle_it_indexed_from_the_same_pages() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut settled = PendingManifest::new(Default::default());
    settle_and_adopt(&mp, &mut settled);
    let mut first = PendingManifest::new(Default::default());
    let before = search::index_build_count();
    assert!(matches!(adopt_into(&mp, &mp.staging_dir(), &mut first, true, Freshness::Now), Adoption::Indexed(_)));
    assert_eq!(search::index_build_count() - before, 1, "a settle's receipt is not a one-shot build's own");

    let mut second = PendingManifest::new(Default::default());
    let outcome = adopt_into(&mp, &mp.staging_dir(), &mut second, true, Freshness::Now);

    assert!(matches!(outcome, Adoption::Adopted(_)), "got {:?}", outcome);
    assert_eq!(search::index_build_count() - before, 1, "the same pages are not indexed twice");
    assert_eq!(bundle_keys(&second), bundle_keys(&first));
    assert_eq!(second.files(), first.files());
}

// ── Receipt/disk divergence (the rule's second hole) ─────────────────────────

/// An external delete or a cloud eviction of the holding area leaves a receipt
/// naming files that are gone. Registering them would put a path in the
/// manifest that is not on disk. Register nothing and drop the receipt; the
/// next settle re-indexes.
#[test]
fn a_receipt_whose_files_are_gone_registers_nothing_and_is_dropped() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut first = PendingManifest::new(Default::default());
    settle_and_adopt(&mp, &mut first);
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
        adopt(&mp, &mut third) == Adoption::Unindexed,
        "with the receipt dropped, the next settle re-indexes"
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
    settle_and_adopt(&mp, &mut first);
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
    let fresh = BundleReceipt { fp: "beef".into(), pages: 1, left_out: 0, files: vec![], staged: None };
    std::fs::create_dir_all(&index_dir).unwrap();
    std::fs::write(mp.index_receipt(), serde_json::to_vec(&fresh).unwrap()).unwrap();

    let stale = BundleReceipt { fp: "dead".into(), pages: 1, left_out: 0, files: vec![], staged: None };
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
/// fingerprint, an empty bundle is absorbing: every later settle skips that
/// page set, its `intact` test is vacuously
/// true over an empty file list, adoption registers nothing, and the deploy
/// diff deletes the search index off the live site. Refuse the stamp instead.
#[test]
fn a_bundle_that_covers_fewer_pages_than_its_page_set_is_refused() {
    let (_root, mp) = project(PAGE);
    let index_dir = mp.index_dir();
    let want = PageSet { fp: PageSetFp(0xfeed), pages: 3 };

    // Nothing could be read at all — a generation GC'd or evicted mid-walk.
    let nothing = search::SearchIndex { files: vec![], pages: 0, skipped: 0, left_out: 0 };
    let err = with_holding(&index_dir, |h| publish_bundle(h, &index_dir, want, None, &nothing))
        .expect_err("a zero-page bundle must not be stamped with a 3-page fingerprint");
    assert!(err.contains("0 of 3"), "unhelpful refusal: {err}");
    assert!(!mp.index_receipt().exists(), "a refused publish writes no receipt");

    // Two of three pages read — the invisible variant: search silently returns
    // no hits for the third and no later build re-indexes it.
    let partial = search::SearchIndex {
        files: vec![search::SearchIndexFile { rel_path: "pagefind.js".into(), bytes: vec![1] }],
        pages: 2,
        skipped: 1,
        left_out: 0,
    };
    assert!(
        with_holding(&index_dir, |h| publish_bundle(h, &index_dir, want, None, &partial)).is_err(),
        "a bundle missing one page must not be stamped with the whole page set"
    );
    assert!(!mp.index_receipt().exists());
}

/// The pages the walk leaves out on purpose, an old address's redirect stub and
/// the not-found page at both its addresses, are in the page set but are not
/// missing from the bundle. A site with renamed pages or a not-found page would
/// otherwise have every bundle of its generations refused.
#[test]
fn pages_left_out_on_purpose_do_not_make_a_bundle_partial() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let stage = mp.staging_dir();
    std::fs::create_dir_all(stage.join("old-name")).unwrap();
    std::fs::write(stage.join("old-name/index.html"), crate::build::feeds::redirects::generate_redirect_html("/")).unwrap();
    std::fs::write(stage.join(crate::build::served_path::NOT_FOUND_FILE), PAGE).unwrap();
    let files: HashMap<String, String> = ["index.html", "old-name/index.html", crate::build::served_path::NOT_FOUND_FILE]
        .into_iter()
        .map(|p| (p.to_string(), "hash".to_string()))
        .collect();
    let want = PageSet::of(&files);
    let index_dir = mp.index_dir();

    let produced = search::build_search_index(&stage).unwrap();
    with_holding(&index_dir, |h| publish_bundle(h, &index_dir, want, None, &produced))
        .expect("a stub and the not-found page are left out, not missing");
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
    with_holding(&index_dir, |h| publish_bundle(h, &index_dir, one, None, &good)).unwrap();

    let nothing = search::SearchIndex { files: vec![], pages: 0, skipped: 0, left_out: 0 };
    let two = PageSet { fp: PageSetFp(0x2222), pages: 2 };
    assert!(with_holding(&index_dir, |h| publish_bundle(h, &index_dir, two, None, &nothing)).is_err());

    let mut pending = PendingManifest::new(Default::default());
    assert!(matches!(adopt(&mp, &mut pending), Adoption::Adopted(_)));
    assert!(!bundle_keys(&pending).is_empty(), "the good bundle must survive a refusal");
}

/// A page set with nothing to index publishes an *empty* receipt rather than
/// none. Without it every settle of a site that has no indexable content would
/// re-run a whole index. This is the one case where an
/// empty bundle is authoritative, and the receipt records the measurement that
/// makes it so.
#[test]
fn an_unindexable_site_publishes_an_empty_receipt_instead_of_reindexing() {
    let _serialize = search::lock_index_counter();
    let root = tempfile::tempdir().unwrap();
    let mp = MossPaths::new(root.path());
    mp.ensure_dirs().unwrap();

    let mut first = PendingManifest::new(Default::default());
    assert_eq!(settle_and_adopt(&mp, &mut first), Adoption::Empty);
    assert_eq!(read_receipt(&mp.index_dir()).unwrap().pages, 0);

    let before = search::index_build_count();
    let mut second = PendingManifest::new(Default::default());
    assert_eq!(adopt(&mp, &mut second), Adoption::Empty);
    assert_eq!(
        search::index_build_count(),
        before,
        "an empty receipt is adopted, never re-indexed"
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
/// the reason a settle keyed on it can skip a no-op save.
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

/// A bundle published for a one-page set, and `hashes.json` for that page,
/// naming the bundle's files too when `seal_the_bundle`.
fn sealed_with_a_covering_receipt(mp: &MossPaths, seal_the_bundle: bool) {
    let mut hashes = crate::types::content::SiteHashes::default();
    hashes.files.insert("index.html".to_string(), "100644:abc".to_string());
    let want = PageSet::of(&hashes.files);
    let produced = search::build_search_index(&mp.staging_dir()).unwrap();
    let receipt = with_holding(&mp.index_dir(), |h| publish_bundle(h, &mp.index_dir(), want, None, &produced)).unwrap();
    if seal_the_bundle {
        for (rel, hash) in &receipt.files {
            let sp = ServedPath::for_search_asset(rel).unwrap();
            hashes.files.insert(sp.as_str().to_string(), crate::types::content::file_entry(hash));
        }
    }
    std::fs::write(mp.hashes(), serde_json::to_string(&hashes).unwrap()).unwrap();
}

/// A settle must be free when the receipt covers the page set `current` holds
/// and `current` already ships its bundle — otherwise every publish would pay
/// for a re-index and a rebuild it does not need.
#[test]
fn settle_is_a_no_op_when_current_ships_the_covering_bundle() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    sealed_with_a_covering_receipt(&mp, true);

    let before = search::index_build_count();
    assert!(!settle_index(&mp), "a fresh, shipped index must not force a rebuild");
    assert_eq!(search::index_build_count(), before);
}

/// A search read since the last build published a covering bundle that build
/// never adopted: a publish must rebuild to ship it, without indexing again.
#[test]
fn settle_asks_for_a_rebuild_when_current_ships_another_bundle() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    sealed_with_a_covering_receipt(&mp, false);

    let before = search::index_build_count();
    assert!(settle_index(&mp), "current does not ship the receipt's bundle");
    assert_eq!(search::index_build_count(), before, "the receipt already covers the page set");
}

/// …and must re-index (and say so) when it does not. A publish that shipped the
/// previous page set's bundle would hand readers a search box that cannot find
/// the page they just published.
#[test]
fn settle_reindexes_when_the_receipt_is_for_an_older_page_set() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut pending = PendingManifest::new(Default::default());
    settle_and_adopt(&mp, &mut pending); // receipt stamped UNKNOWN
    // `current` must resolve for the settle to have something to index.
    let gen = mp.generation_dir("test");
    std::fs::create_dir_all(&gen).unwrap();
    std::fs::write(gen.join("index.html"), PAGE).unwrap();
    mp.set_current_ptr("test").unwrap();

    let mut hashes = crate::types::content::SiteHashes::default();
    hashes.files.insert("index.html".to_string(), "100644:abc".to_string());
    std::fs::write(mp.hashes(), serde_json::to_string(&hashes).unwrap()).unwrap();

    assert!(settle_index(&mp), "a stale index must force a rebuild");
    assert_eq!(
        read_receipt(&mp.index_dir()).unwrap().fp(),
        PageSetFp::of(&hashes.files),
        "settle must leave a receipt for the page set being published"
    );
}

/// A page set with a page the indexer leaves out on purpose (here the
/// not-found page) is covered by the bundle that left it out: a second settle
/// of the same generation must not index again.
#[test]
fn a_second_settle_of_a_page_set_with_a_page_left_out_indexes_nothing() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let gen = mp.generation_dir("g");
    std::fs::create_dir_all(&gen).unwrap();
    std::fs::write(gen.join("index.html"), PAGE).unwrap();
    std::fs::write(gen.join(crate::build::served_path::NOT_FOUND_FILE), PAGE).unwrap();
    mp.set_current_ptr("g").unwrap();
    let mut hashes = crate::types::content::SiteHashes::default();
    for page in ["index.html", crate::build::served_path::NOT_FOUND_FILE] {
        hashes.files.insert(page.to_string(), "100644:abc".to_string());
    }
    std::fs::write(mp.hashes(), serde_json::to_string(&hashes).unwrap()).unwrap();

    settle_index(&mp);
    let before = search::index_build_count();
    settle_index(&mp);
    assert_eq!(search::index_build_count(), before, "the first settle's bundle covers this page set");
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
        left_out: 0,
        files: vec![],
        staged: None,
    };
    std::fs::create_dir_all(mp.index_dir()).unwrap();
    std::fs::write(mp.index_receipt(), serde_json::to_vec(&empty).unwrap()).unwrap();

    let gen = mp.generation_dir("g");
    std::fs::create_dir_all(&gen).unwrap();
    std::fs::write(gen.join("index.html"), PAGE).unwrap();
    mp.set_current_ptr("g").unwrap();

    assert!(settle_index(&mp), "an empty bundle must not pass for a real page set");
    let after = read_receipt(&mp.index_dir()).unwrap();
    assert_eq!(after.pages, 1);
    assert!(!after.files.is_empty());
}

/// An edit's build must not wait out a search's index. `begin` takes the
/// holding lock on every build, so a settle may hold it only to read the
/// receipt and to publish, never across the index.
#[test]
fn a_build_adopts_while_a_settle_indexes() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let gen = mp.generation_dir("g");
    std::fs::create_dir_all(&gen).unwrap();
    std::fs::write(gen.join("index.html"), PAGE).unwrap();
    mp.set_current_ptr("g").unwrap();
    let mut hashes = crate::types::content::SiteHashes::default();
    hashes.files.insert("index.html".to_string(), "100644:abc".to_string());
    std::fs::write(mp.hashes(), serde_json::to_string(&hashes).unwrap()).unwrap();

    let adopted_mid_index = Arc::new(Mutex::new(None));
    let (build, record) = (mp.clone(), Arc::clone(&adopted_mid_index));
    *WHILE_INDEXING.lock().unwrap() = Some(Box::new(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut pending = PendingManifest::new(Default::default());
            let _ = tx.send(adopt(&build, &mut pending));
        });
        *record.lock().unwrap() = rx.recv_timeout(std::time::Duration::from_secs(5)).ok();
    }));

    assert!(settle_index(&mp));
    assert_eq!(
        *adopted_mid_index.lock().unwrap(),
        Some(Adoption::Unindexed),
        "a build's adoption waited for the settle's index"
    );
    assert_eq!(read_receipt(&mp.index_dir()).unwrap().fp(), PageSetFp::of(&hashes.files));
}

/// The settle indexes with no lock against a promote. A promote to another
/// generation with as many pages, after the manifest is read, must publish
/// nothing: a bundle of either generation's bytes under the page set read
/// before it would later "cover" that page set again (an edit undone) and
/// ship with pages it does not index.
#[test]
fn a_promote_between_the_manifest_read_and_the_page_read_publishes_nothing() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut hashes = crate::types::content::SiteHashes::default();
    hashes.files.insert("index.html".to_string(), "100644:abc".to_string());
    for (id, page) in [("a", PAGE), ("b", "<html lang=\"en\"><body><h1>Lichen</h1></body></html>")] {
        let gen = mp.generation_dir(id);
        std::fs::create_dir_all(&gen).unwrap();
        std::fs::write(gen.join("index.html"), page).unwrap();
    }
    mp.set_current_ptr("a").unwrap();
    std::fs::write(mp.hashes(), serde_json::to_string(&hashes).unwrap()).unwrap();

    let promote = mp.clone();
    *WHILE_INDEXING.lock().unwrap() = Some(Box::new(move || {
        let mut b = crate::types::content::SiteHashes::default();
        b.files.insert("index.html".to_string(), "100644:def".to_string());
        promote.set_current_ptr("b").unwrap();
        std::fs::write(promote.hashes(), serde_json::to_string(&b).unwrap()).unwrap();
    }));

    assert!(!settle_index(&mp), "nothing was indexed for a publish to ship");
    assert!(read_receipt(&mp.index_dir()).is_none(), "a bundle was published for a page set current no longer holds");
}

/// Search off: no read, no index, no forced rebuild.
#[test]
fn settle_is_inert_when_search_is_disabled() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let before = search::index_build_count();
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    assert!(!runtime.block_on(settle(&mp, false)));
    assert_eq!(search::index_build_count(), before);
}

// ── A one-shot build's index runs beside the background phase ──────────────

/// The pages are read on the build thread, inside the stage-write span; the
/// index is computed on a worker beside the media and the asset walk, and the
/// seal still carries the bundle into the generation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_one_shot_build_indexes_beside_the_background_phase() {
    use crate::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    let _serialize = search::lock_index_counter();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mp = MossPaths::new(root);
    std::fs::create_dir_all(mp.root()).unwrap();
    std::fs::write(mp.config(), "[site]\nsearch = true\n").unwrap();
    std::fs::write(root.join("index.md"), "---\ntitle: Garden\n---\n\n苔藓花园里的一条小路。\n").unwrap();
    std::fs::write(root.join("notes.md"), "---\ntitle: Notes\n---\n\nGrowing seeds.\n").unwrap();

    run_pipeline(PipelineConfig {
        root: crate::vault::paths::VaultRoot::resolve(root),
        progress: std::sync::Arc::new(crate::build::ports::reporter::NullReporter), plugins: PluginMode::Skip,
        watch: false, start_server: false,
        host: crate::build::ports::host::test_host_ports(),
        trigger: BuildTrigger::Full, exits_after_build: true,
        site_url_override: None, server_port: None, admission_epoch: None, live_port: None, rename_pairs: Vec::new(),
    }).await.expect("fixture builds");

    let text = std::fs::read_to_string(mp.profile()).unwrap();
    let record: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    let phases = record["phases"].as_array().unwrap();
    let entry = |name: &str| phases.iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("missing {name}: {record}"));
    assert!(entry("search_read").get("beside").is_none(), "the pages are read on the build thread: {record}");
    assert_eq!(entry("search_index")["beside"], true, "the index runs beside the background phase: {record}");
    assert_eq!(entry("search_key")["parent"], "search_read", "{record}");
    for name in ["search_publish", "search_gc", "search_lay"] {
        assert_eq!(entry(name)["parent"], "search_index", "the bundle's own IO is timed: {name}: {record}");
    }

    let hashes: SiteHashes = serde_json::from_str(&std::fs::read_to_string(mp.hashes()).unwrap()).unwrap();
    assert!(hashes.files.contains_key("_moss/pagefind/pagefind.js"), "the seal carries the bundle: {:?}", hashes.files.keys().collect::<Vec<_>>());
    let generation = mp.generation_dir(&mp.current_generation_id().unwrap());
    assert!(generation.join("_moss/pagefind/pagefind.js").is_file(), "and the generation ships it");
}

/// A one-page edit rewrites a handful of a bundle's files. Publishing and
/// laying it touches only those: every other file keeps its inode in the
/// holding area and in staging, and nothing the new receipt does not name is
/// left in the holding area.
#[cfg(unix)]
#[test]
fn a_one_shot_rebuild_writes_and_lays_only_the_files_that_changed() {
    use std::os::unix::fs::MetadataExt;
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    for n in 0..12 {
        std::fs::write(mp.staging_dir().join(format!("page{n}.html")), format!("<html lang=\"en\"><body><p>Page {n} about lichen number {n}.</p></body></html>")).unwrap();
    }
    let mut first = PendingManifest::new(Default::default());
    assert!(matches!(adopt_into(&mp, &mp.staging_dir(), &mut first, true, Freshness::Now), Adoption::Indexed(_)));
    let receipt = read_receipt(&mp.index_dir()).unwrap();
    let holding = holding_dir(&mp.index_dir(), receipt.fp());
    let inodes = |root: &Path| -> HashMap<String, u64> {
        receipt.files.iter().map(|(rel, _)| (rel.clone(), std::fs::metadata(root.join(rel)).unwrap().ino())).collect()
    };
    let staged_root = mp.staging_dir().join("_moss/pagefind");
    let (held_before, staged_before) = (inodes(&holding), inodes(&staged_root));

    std::fs::write(mp.staging_dir().join("page3.html"), "<html lang=\"en\"><body><p>Page 3 now about mosses.</p></body></html>").unwrap();
    let mut second = PendingManifest::new(Default::default());
    assert!(matches!(adopt_into(&mp, &mp.staging_dir(), &mut second, true, Freshness::Now), Adoption::Indexed(_)));

    let after = read_receipt(&mp.index_dir()).unwrap();
    let unchanged: Vec<&String> = after.files.iter().filter(|f| receipt.files.contains(f)).map(|(rel, _)| rel).collect();
    assert!(unchanged.len() * 2 > after.files.len(), "most files survive a one-page edit: {} of {}", unchanged.len(), after.files.len());
    for rel in &unchanged {
        assert_eq!(std::fs::metadata(holding.join(rel)).unwrap().ino(), held_before[*rel], "rewritten in the holding area: {rel}");
        assert_eq!(std::fs::metadata(staged_root.join(rel)).unwrap().ino(), staged_before[*rel], "re-laid into staging: {rel}");
    }
    let on_disk = walkdir::WalkDir::new(&holding).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file()).count();
    assert_eq!(on_disk, after.files.len(), "the holding area keeps only what the receipt names");
    let bytes = |root: &Path| -> Vec<(String, Vec<u8>)> { after.files.iter().map(|(rel, _)| (rel.clone(), std::fs::read(root.join(rel)).unwrap())).collect() };
    assert_eq!(bytes(&staged_root), bytes(&holding), "staging serves the bundle the receipt names");
}

/// A staged file's presence never proves which bytes it holds. A laid bundle
/// file rewritten in place at the same size is laid again on the next
/// adoption, because its stat no longer vouches for the copy.
#[test]
fn a_laid_file_rewritten_in_place_is_laid_again() {
    let _serialize = search::lock_index_counter();
    let (_root, mp) = project(PAGE);
    let mut first = PendingManifest::new(Default::default());
    assert!(matches!(adopt_into(&mp, &mp.staging_dir(), &mut first, true, Freshness::Now), Adoption::Indexed(_)));
    let receipt = read_receipt(&mp.index_dir()).unwrap();
    let (rel, _) = receipt.files.iter().find(|(rel, _)| rel.starts_with("fragment/")).unwrap();
    let staged = mp.staging_dir().join("_moss/pagefind").join(rel);
    let good = std::fs::read(&staged).unwrap();
    let laid_at = std::fs::metadata(&staged).unwrap().modified().unwrap();
    std::fs::write(&staged, vec![b'x'; good.len()]).unwrap();
    crate::build::stat::FileStat::stamp_in_the_second_of(&staged, laid_at);

    let mut second = PendingManifest::new(Default::default());
    assert!(matches!(adopt_into(&mp, &mp.staging_dir(), &mut second, true, Freshness::Now), Adoption::Adopted(_)), "the same pages serve the held bundle");

    assert_eq!(std::fs::read(&staged).unwrap(), good, "staging serves the receipt's bytes again");
}
