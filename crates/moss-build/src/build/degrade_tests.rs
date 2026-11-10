use super::*;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::ServedPath;
use crate::types::content::SiteHashes;

#[test]
fn noop_when_nothing_is_unshippable() {
    let dir = tempfile::tempdir().unwrap();
    let html = r#"<picture><source srcset="a.webp" type="image/webp"><img src="a.jpg"></picture>"#;
    std::fs::write(dir.path().join("index.html"), html).unwrap();

    let mut pending = PendingManifest::new(SiteHashes::default());
    let sp = ServedPath::from_source("index.html").unwrap();
    pending.register(&sp, html.as_bytes(), HashBucket::Files);
    let mut sealed = pending.seal();
    let before_gen = sealed.generation_id().to_string();

    apply_to_staging(dir.path(), &mut sealed, &std::collections::HashSet::new());

    assert_eq!(sealed.generation_id(), before_gen);
    assert_eq!(std::fs::read_to_string(dir.path().join("index.html")).unwrap(), html);
}

// ── The whole tail, including the fourth unshippable source ──

/// Stage a page and a manifest, then run the seal tail through the one
/// function `build.rs` calls — so that dropping a pass out of it turns this
/// red, which composing the passes here by hand could never do.
///
/// The hole this closes has no reachable fixture through a real code path on
/// Linux — every image route to "this will not exist" now settles `Failed`,
/// and the routes that stay `Pending` need cloud eviction, which
/// `icloud::is_evicted` hardcodes to false off macOS. So the tail is driven
/// directly, as `video.rs`'s HLS stale-cleanup test drives its own seam,
/// rather than by adding a fault-injection seam to production code.
#[test]
fn the_tail_strips_a_source_whose_variant_has_no_manifest_entry() {
    let vault = tempfile::tempdir().unwrap();
    let mp = crate::moss_paths::MossPaths::new(vault.path());
    let stage = mp.staging_dir();
    std::fs::create_dir_all(stage.join("assets")).unwrap();

    // `data-moss-preview` is the reason the hash below is of the SHIPPED
    // bytes: staging keeps it, the generation does not.
    let html = concat!(
        "<html><body data-moss-preview>\n",
        r#"<picture><source srcset="assets/gone.webp" type="image/webp">"#,
        r#"<img src="assets/gone.jpg"></picture>"#,
        "\n",
        r#"<picture><source srcset="assets/kept.webp" type="image/webp">"#,
        r#"<img src="assets/kept.jpg"></picture>"#,
        "\n",
        r#"<picture><source srcset="assets/evicted.webp" type="image/webp">"#,
        r#"<img src="assets/evicted.jpg"></picture>"#,
        "\n</body></html>\n"
    );
    std::fs::write(stage.join("index.html"), html).unwrap();
    // Registered AND on disk AND referenced — the control arm. Nothing in
    // this tail may touch it, or the pass is stripping live variants.
    std::fs::write(stage.join("assets/kept.webp"), b"kept webp bytes").unwrap();
    // On disk and registered, but referenced by no page: the orphan prune's
    // arm. Deleting the prune from the tail leaves this entry in the manifest.
    std::fs::write(stage.join("assets/orphan.webp"), b"orphan webp bytes").unwrap();
    // `assets/gone.webp` is deliberately never written and never registered:
    // only the unregistered-reference pass can see it.
    // `assets/evicted.webp` is registered but never written — the presence
    // pass's arm, and invisible to the unregistered-reference pass, which
    // requires the manifest entry to be gone already.

    let mut pending = PendingManifest::new(SiteHashes::default());
    pending.register(
        &ServedPath::from_source("index.html").unwrap(),
        html.as_bytes(),
        HashBucket::Files,
    );
    pending.register(
        &ServedPath::from_source("assets/kept.webp").unwrap(),
        b"kept webp bytes",
        HashBucket::ImageVariants,
    );
    pending.register(
        &ServedPath::from_source("assets/orphan.webp").unwrap(),
        b"orphan webp bytes",
        HashBucket::ImageVariants,
    );
    pending.register(
        &ServedPath::from_source("assets/evicted.webp").unwrap(),
        b"evicted webp bytes",
        HashBucket::ImageVariants,
    );
    let mut sealed = pending.seal();

    let (_, rewritten) = repair_staged_html(&mp, &stage, &mut sealed, HashSet::new());
    assert_eq!(rewritten, ["index.html"], "the page the preview is serving was rewritten, and the caller is told");

    let on_disk = std::fs::read_to_string(stage.join("index.html")).unwrap();
    assert!(
        !on_disk.contains("gone.webp"),
        "a <source> for a variant with no manifest entry ships a live 404 that \
         <picture> renders blank instead of falling back: {on_disk}"
    );
    assert!(
        on_disk.contains(r#"<img src="assets/gone.jpg""#),
        "the sibling <img> is the fallback the strip exists to reach: {on_disk}"
    );
    assert!(
        on_disk.contains("assets/kept.webp") && stage.join("assets/kept.webp").exists(),
        "a registered, present, referenced variant must survive the whole tail: {on_disk}"
    );
    assert!(
        !on_disk.contains("evicted.webp"),
        "a manifest entry with no bytes behind it leaves the site at the generation \
         swap, so the presence pass must condemn its <source> too: {on_disk}"
    );
    assert!(
        !sealed.files().contains_key("assets/orphan.webp"),
        "an unreferenced variant is the orphan prune's to drop from the manifest, \
         which is the whole of not shipping it — ship_phase copies sealed.files()"
    );
    assert!(
        stage.join("assets/orphan.webp").exists(),
        "and the bytes must stay: staging is what the preview server reads while \
         this tail runs, so unlinking here is a live 404. `pipeline::sweep_staging` \
         takes them at the next build's start"
    );

    // The manifest records the bytes the SITE serves, not staging's.
    // Hashing the staged bytes made deploy reject the entire upload, and a
    // hash mismatch is not a 404 — the parity gate cannot see it.
    let shipped = crate::build::ship::apply_transform(
        crate::build::ship::transform_for("index.html"), &Default::default(),
        on_disk.as_bytes(),
    );
    assert!(!String::from_utf8_lossy(&shipped).contains("data-moss-preview"));
    let expected = crate::build::assets::paths::compute_binary_hash(&shipped);
    assert_eq!(
        sealed.files().get("index.html").map(String::as_str),
        Some(format!("100644:{expected}").as_str()),
    );
}

/// The presence pass asks every CAS-backed entry of its blob, and a blob is
/// hashed unless something vouches for it at its current stat. Each build is a
/// new process with a new store, so only the index the last seal saved can.
#[test]
fn the_next_seal_reads_no_blob_the_last_one_verified() {
    let vault = tempfile::tempdir().unwrap();
    let mp = crate::moss_paths::MossPaths::new(vault.path());
    let stage = mp.staging_dir();
    std::fs::create_dir_all(&stage).unwrap();
    let oid = crate::build::cache::ObjectStore::for_site(&mp)
        .store_bytes(b"stable bytes", crate::build::cache::RecordMode::Request)
        .unwrap();
    let seal = || {
        let mut pending = PendingManifest::new(SiteHashes::default());
        pending.apply_message("asset.bin".to_string(), "cafefacecafeface", HashBucket::Files, Some(oid.clone()));
        pending.seal()
    };
    let reads = || crate::build::cache::blob_residency::HASH_READS.with(|count| count.get());

    let before = reads();
    repair_staged_html(&mp, &stage, &mut seal(), HashSet::new());
    assert_eq!(reads() - before, 1, "the first seal checks the blob");

    let mut sealed = seal();
    repair_staged_html(&mp, &stage, &mut sealed, HashSet::new());
    assert_eq!(reads() - before, 1, "the second seal trusts the first one's check");
    assert!(sealed.files().contains_key("asset.bin"));
}

/// The tail runs detached, so an overlapping build can render over the stage
/// before it: here the stage's page names `b.webp` while the page this
/// generation sealed, held in the CAS, names `a.webp`. The prune must judge
/// `a.webp` by the page this generation ships, or the page it ships names a
/// variant its own generation dropped.
#[test]
fn the_prune_reads_the_page_this_generation_ships_not_the_stage() {
    let vault = tempfile::tempdir().unwrap();
    let mp = crate::moss_paths::MossPaths::new(vault.path());
    let stage = mp.staging_dir();
    std::fs::create_dir_all(stage.join("assets")).unwrap();

    let sealed_page = r#"<picture><img src="assets/a.jpg" srcset="assets/a.webp"></picture>"#;
    let oid = crate::build::cache::ObjectStore::for_site(&mp)
        .store_bytes(sealed_page.as_bytes(), crate::build::cache::RecordMode::Request)
        .unwrap();
    std::fs::write(stage.join("assets/a.webp"), b"a webp bytes").unwrap();

    let mut pending = PendingManifest::new(SiteHashes::default());
    let hash = crate::build::assets::paths::compute_binary_hash(sealed_page.as_bytes());
    pending.apply_message("index.html".to_string(), &hash, HashBucket::Files, Some(oid));
    pending.register(&ServedPath::from_source("assets/a.webp").unwrap(), b"a webp bytes", HashBucket::ImageVariants);
    let mut sealed = pending.seal();

    // The overlapping build's render, after this one sealed.
    std::fs::write(stage.join("index.html"), r#"<picture><img src="assets/b.jpg" srcset="assets/b.webp"></picture>"#).unwrap();

    repair_staged_html(&mp, &stage, &mut sealed, HashSet::new());

    assert!(
        sealed.files().contains_key("assets/a.webp"),
        "the sealed page names assets/a.webp, so the generation must ship it: {:?}",
        sealed.files().keys().collect::<Vec<_>>()
    );
}

/// This generation stopped showing `a.webp`; the overlapping build put it
/// back. The prune drops it from this generation, and must not strip it from
/// the newer build's staged page: a pruned key is one no page of this
/// generation names, so stripping it only ever rewrites another generation's
/// page, which this generation would then ship as its own.
#[test]
fn a_pruned_variant_is_not_stripped_from_the_next_builds_page() {
    let vault = tempfile::tempdir().unwrap();
    let mp = crate::moss_paths::MossPaths::new(vault.path());
    let stage = mp.staging_dir();
    std::fs::create_dir_all(stage.join("assets")).unwrap();

    let sealed_page = r#"<p>no picture</p>"#;
    let oid = crate::build::cache::ObjectStore::for_site(&mp)
        .store_bytes(sealed_page.as_bytes(), crate::build::cache::RecordMode::Request)
        .unwrap();
    std::fs::write(stage.join("assets/a.webp"), b"a webp bytes").unwrap();

    let mut pending = PendingManifest::new(SiteHashes::default());
    let hash = crate::build::assets::paths::compute_binary_hash(sealed_page.as_bytes());
    pending.apply_message("index.html".to_string(), &hash, HashBucket::Files, Some(oid));
    pending.register(&ServedPath::from_source("assets/a.webp").unwrap(), b"a webp bytes", HashBucket::ImageVariants);
    let mut sealed = pending.seal();
    let sealed_entry = sealed.files().get("index.html").cloned();

    let next_page = r#"<picture><img src="assets/a.jpg" srcset="assets/a.webp"></picture>"#;
    std::fs::write(stage.join("index.html"), next_page).unwrap();

    repair_staged_html(&mp, &stage, &mut sealed, HashSet::new());

    assert!(!sealed.files().contains_key("assets/a.webp"), "no page this generation ships names it");
    assert_eq!(std::fs::read_to_string(stage.join("index.html")).unwrap(), next_page, "the next build's page is its own");
    assert_eq!(sealed.files().get("index.html").cloned(), sealed_entry, "this generation ships the page it sealed");
}

/// The tail's scan reads an unchanged page once, whatever the stage walk read
/// in between: each remembers its own files, or every build would read the
/// whole site twice. Two pages with the same bytes ship one CAS blob, and each
/// is remembered under its own address.
#[test]
fn the_tail_scan_rereads_no_unchanged_page() {
    let vault = tempfile::tempdir().unwrap();
    let mp = crate::moss_paths::MossPaths::new(vault.path());
    let stage = mp.staging_dir();
    std::fs::create_dir_all(stage.join("twin")).unwrap();
    let page = r#"<img src="assets/a.jpg">"#;
    let store = crate::build::cache::ObjectStore::for_site(&mp);
    let oid = store.store_bytes(page.as_bytes(), crate::build::cache::RecordMode::Request).unwrap();
    let hash = crate::build::assets::paths::compute_binary_hash(page.as_bytes());
    let mut pending = PendingManifest::new(SiteHashes::default());
    for key in ["index.html", "twin/index.html"] {
        std::fs::write(stage.join(key), page).unwrap();
        pending.apply_message(key.to_string(), &hash, HashBucket::Files, Some(oid.clone()));
    }
    let sealed = pending.seal();
    let reads = || crate::build::media::orphan_prune::FILES_READ.with(|n| n.get());

    let scan = shipped_references(&stage, &sealed, &store, None);
    assert!(scan.resolved.contains("assets/a.jpg") && scan.resolved.contains("twin/assets/a.jpg"));
    crate::build::media::orphan_prune::scan_stage(&stage, None);
    let before = reads();
    let scan = shipped_references(&stage, &sealed, &store, None);
    assert_eq!(reads() - before, 0, "both pages are unchanged since the tail last read them");
    assert!(scan.resolved.contains("assets/a.jpg") && scan.resolved.contains("twin/assets/a.jpg"), "remembered pages still count their references");
}

/// A new process's tail scan starts from the one the last process kept in the
/// folder's memo directory, so a one-shot build reads no unchanged page.
#[test]
fn a_new_process_tail_scan_reads_no_unchanged_page() {
    let vault = tempfile::tempdir().unwrap();
    let mp = crate::moss_paths::MossPaths::new(vault.path());
    let stage = mp.staging_dir();
    std::fs::create_dir_all(&stage).unwrap();
    let page = r#"<img src="assets/a.jpg">"#;
    std::fs::write(stage.join("index.html"), page).unwrap();
    let store = crate::build::cache::ObjectStore::for_site(&mp);
    let oid = store.store_bytes(page.as_bytes(), crate::build::cache::RecordMode::Request).unwrap();
    let hash = crate::build::assets::paths::compute_binary_hash(page.as_bytes());
    let sealed = || {
        let mut pending = PendingManifest::new(SiteHashes::default());
        pending.apply_message("index.html".to_string(), &hash, HashBucket::Files, Some(oid.clone()));
        pending.seal()
    };
    let reads = || crate::build::media::orphan_prune::FILES_READ.with(|n| n.get());

    let before = reads();
    repair_staged_html(&mp, &stage, &mut sealed(), HashSet::new());
    assert_eq!(reads() - before, 1, "the first process reads the page");
    crate::build::media::orphan_prune::new_process(&stage);
    let before = reads();
    repair_staged_html(&mp, &stage, &mut sealed(), HashSet::new());
    assert_eq!(reads() - before, 0, "the next process starts from the kept scan");
}
