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

    repair_staged_html(&mp, &stage, &mut sealed, HashSet::new());

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
        crate::build::ship::transform_for("index.html"),
        on_disk.as_bytes(),
    );
    assert!(!String::from_utf8_lossy(&shipped).contains("data-moss-preview"));
    let expected = crate::build::assets::paths::compute_binary_hash(&shipped);
    assert_eq!(
        sealed.files().get("index.html").map(String::as_str),
        Some(format!("100644:{expected}").as_str()),
    );
}

