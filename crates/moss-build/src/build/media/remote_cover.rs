//! Materializing a downloaded external-link cover (an og:image / twitter:image
//! `build::page::link_meta` fetched and kept in the site folder, see
//! `page::link_snapshot`) as a real local asset in this build's output,
//! through the SAME encoder the vault's own images use.
//!
//! `link_meta`'s fetch step already downloaded the bytes, decode-verified
//! them, and stored them under their SHA-256 (`LinkMeta::cover`). What's
//! left here: copy that file into scratch (the encoder needs a path, not
//! bytes), run it through
//! `media::image::convert_single_image` exactly as a vault image would be,
//! stage the fallback original alongside the webp it produces, and record
//! the result (served path, dimensions, LQIP, dominant color) back onto the
//! `LinkMeta` cache entry so `render::grid_cells` can build a one-entry
//! `MediaDimensionLookup` and render the cover exactly like a local one —
//! no new parameter threaded through the render call chain, the same way
//! `link_meta`'s own cache-only render-side read works.
//!
//! Runs on EVERY build (not gated on `exits_after_build`, unlike the text
//! fetch): the output directory is fresh each build, so a cover fetched on
//! a previous build still needs its files linked into THIS build's
//! staging even when nothing new was fetched. On a warm cache
//! (`TransformCache` hit) this is a cheap copy, the same as any other
//! carried-forward asset.

use crate::build::cache::{ObjectStore, TransformCache};
use super::image::{convert_single_image, ImageCompressionConfig};
use crate::build::io_utils;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::page::link_meta::LinkMeta;
use crate::build::served_path::ServedPath;
use crate::moss_paths::MossPaths;
use std::collections::HashMap;
use std::path::Path;

/// Materialize every URL's downloaded cover (if any) into `output_dir`,
/// updating `link_meta` in place and writing the update back to the
/// on-disk cache so the next reader (this build's own render, or the next
/// build) sees it. Returns the number of covers actually (re-)encoded —
/// informational only, callers don't need to act on it.
pub(crate) fn materialize_remote_covers(
    link_meta: &mut HashMap<String, LinkMeta>,
    moss_dir: &Path,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> usize {
    let paths = MossPaths::from_moss_dir(moss_dir.to_path_buf());
    let objects = ObjectStore::for_site(&paths);
    let transforms = TransformCache::for_site(&paths);
    let scratch_dir = paths.cache_tmp().join("remote-cover");
    if let Err(e) = io_utils::create_output_dir_all(&scratch_dir) {
        log::info!(target: "remote-cover", "could not create {:?}: {}", scratch_dir, e);
        return 0;
    }

    let mut materialized = 0usize;
    for meta in link_meta.values_mut() {
        let Some(name) = meta.cover.clone() else { continue };
        let Some((oid, ext)) = name.split_once('.') else { continue };
        let cover = crate::build::page::link_snapshot::cover_path(moss_dir, &name);
        if crate::build::outcome::not_yet(moss_dir.parent().unwrap_or(moss_dir), &cover) {
            crate::build::page::link_snapshot::hold(&cover, pending.input_evidence().as_ref());
            crate::build::cli_output::log_note!("The picture for the link card for {} is still in the cloud — building without it; the download will trigger a rebuild", meta.url);
            meta.cover_served_path = None;
            continue;
        }
        if !cover.exists() {
            // Never fetched again here: a fetch would let two machines draw two cards.
            crate::build::cli_output::log_warn_problem!(target: "link-meta",
                "The link card for {} has no picture: {} is missing from the site folder. Delete the card's entry beside it to fetch the card again.",
                meta.url, cover.display());
            meta.cover_served_path = None;
            continue;
        }
        // Re-run the materializer even when the original is already staged:
        // the pending manifest is per generation, and an unregistered asset is
        // swept from the new one. A warm transform cache makes this a relink.
        match materialize_one(oid, ext, &cover, &objects, &transforms, &scratch_dir, output_dir, pending) {
            Some(result) => {
                meta.cover_served_path = Some(result.served_path);
                meta.cover_width = Some(result.width);
                meta.cover_height = Some(result.height);
                meta.cover_lqip = result.lqip;
                meta.cover_color = result.color;
                crate::build::page::link_meta::save_derived(moss_dir, meta);
                materialized += 1;
            }
            None => {
                // Encode failed this time. Leave any PREVIOUSLY-materialized
                // fields alone if the files backing them still exist;
                // otherwise there is nothing to serve, so clear the pointer
                // rather than reference a file that was never written.
                if !meta
                    .cover_served_path
                    .as_ref()
                    .is_some_and(|sp| io_utils::output_present(&output_dir.join(sp)))
                    && meta.cover_served_path.is_some()
                {
                    meta.cover_served_path = None;
                    crate::build::page::link_meta::save_derived(moss_dir, meta);
                }
            }
        }
    }
    materialized
}

struct MaterializedCover {
    served_path: String,
    width: u32,
    height: u32,
    lqip: Option<String>,
    color: Option<String>,
}

fn materialize_one(
    oid: &str,
    ext: &str,
    cover: &Path,
    objects: &ObjectStore,
    transforms: &TransformCache,
    scratch_dir: &Path,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> Option<MaterializedCover> {
    // The hash that names this asset is the CONTENT hash (the oid), not the
    // URL: two pages linking the same image share one cover. 16 hex chars,
    // matching `ServedPath::for_remote_cover`'s (and og_card's) convention.
    let hash16 = &oid[..oid.len().min(16)];
    let original_sp = ServedPath::for_remote_cover(hash16, ext).ok()?;

    let scratch_source = scratch_dir.join(format!("{oid}.{ext}"));
    // `scratch_source`'s name is content-addressed (oid + ext), not a fresh
    // UUID, so a leftover from a prior run can already sit at this exact
    // path — unlike every other scratch file in this crate, this one is not
    // guaranteed absent. `cache_tmp()` is `.moss/build.nosync/cache/tmp/`,
    // IS the regenerable output tree, so a raw `fs::copy`'s O_TRUNC open
    // here is exactly the EDEADLK hazard `io_utils` exists to route around.
    // `copy_output` lands in a temp sibling and `rename`s it into place, so
    // truncating a leftover (dataless or merely unwritable) never happens.
    if io_utils::copy_output(cover, &scratch_source).is_err() {
        return None;
    }

    let bytes = std::fs::read(&scratch_source).ok()?;
    if super::image::should_skip(
        &scratch_source,
        ext,
        bytes.len() as u64,
        &ImageCompressionConfig::default(),
        transforms,
        oid,
        false, // a freshly-materialized scratch file is never cloud-dataless
    )
    .is_some()
    {
        return None;
    }

    // Stage the fallback original — the `<img>`/`<source>` `src` browsers
    // without webp support fall back to. For a webp source this is the
    // exact file `convert_single_image` is about to (re-)encode at the
    // same relative path below; for jpg/png it's a distinct file.
    io_utils::write_output(&output_dir.join(original_sp.as_str()), &bytes).ok()?;
    pending.register(&original_sp, &bytes, HashBucket::ImageOutputs);

    let relative_webp = moss_core::asset_paths::to_webp(original_sp.as_str());
    let outcome = convert_single_image(
        &scratch_source,
        oid,
        &relative_webp,
        scratch_dir,
        output_dir,
        objects,
        transforms,
        &ImageCompressionConfig::default(),
        None,
        None,
        &HashMap::new(),
    );
    let webp_oid = outcome.error.is_none().then_some(outcome.webp_oid).flatten()?;
    let webp_bytes = objects.ready_blob(&webp_oid).and_then(|p| std::fs::read(p).ok())?;
    pending.register(
        &ServedPath::for_remote_cover(hash16, "webp").ok()?,
        &webp_bytes,
        HashBucket::ImageVariants,
    );
    for rung in &outcome.rungs {
        if rung.error.is_some() {
            continue;
        }
        let Some(rung_oid) = &rung.oid else { continue };
        let Some(rung_path) = objects.ready_blob(rung_oid) else { continue };
        let Ok(rung_bytes) = std::fs::read(&rung_path) else { continue };
        // `to_webp_rung` names the rung file relative to the SAME base the
        // synthesizer will look for it at (`asset_paths::to_webp_rung`).
        // `for_jupyterlite_asset` is `ServedPath`'s one no-validation
        // passthrough constructor — safe here because the path it wraps
        // is derived entirely from `original_sp`, which was already
        // validated by `for_remote_cover`.
        let rung_relative = moss_core::asset_paths::to_webp_rung(&relative_webp, rung.width);
        let rung_sp = ServedPath::for_jupyterlite_asset(Path::new(&rung_relative));
        pending.register(&rung_sp, &rung_bytes, HashBucket::ImageVariants);
    }

    let dims = crate::build::scan::scan::extract_image_dimensions(&scratch_source);
    let (color, lqip) = crate::build::scan::scan::extract_color_and_lqip(&scratch_source);
    let (width, height) = dims.unwrap_or((800, 600));

    Some(MaterializedCover {
        served_path: original_sp.as_str().to_string(),
        width,
        height,
        lqip,
        color,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::manifest::PendingManifest;
    use crate::types::content::SiteHashes;

    fn fresh_moss_dir(label: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!(
            "moss_remote_cover_{label}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    fn tiny_png_bytes() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(3, 2, image::Rgb([10, 200, 30]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    fn blank_link_meta(url: &str) -> LinkMeta {
        LinkMeta { url: url.to_string(), ..Default::default() }
    }

    /// Stores a picture where a fetch keeps it and returns its `cover` name.
    fn stored_cover(moss_dir: &Path, bytes: &[u8]) -> String {
        crate::build::page::link_snapshot::store_cover(moss_dir, bytes, "png").unwrap()
    }

    /// A card whose picture has not reached this machine shows none and
    /// fetches nothing.
    #[test]
    fn a_missing_cover_leaves_the_card_without_a_picture() {
        let root = fresh_moss_dir("missing_cover");
        let moss_dir = root.join(".moss");
        let output_dir = root.join("site");
        std::fs::create_dir_all(&output_dir).unwrap();
        let mut meta = blank_link_meta("https://example.com/post");
        meta.cover = Some(format!("{}.png", "ab".repeat(32)));
        // An earlier build's file is still in the output, which an encode
        // failure would keep serving; a missing picture must not.
        let earlier = "_moss/link/abababababababab.png";
        std::fs::create_dir_all(output_dir.join("_moss/link")).unwrap();
        std::fs::write(output_dir.join(earlier), b"earlier").unwrap();
        meta.cover_served_path = Some(earlier.into());
        let mut link_meta = HashMap::from([(meta.url.clone(), meta)]);
        let mut pending = PendingManifest::new(SiteHashes::default());
        assert_eq!(materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut pending), 0);
        assert!(link_meta["https://example.com/post"].cover_served_path.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn materializes_a_downloaded_image_into_a_local_webp_and_original() {
        let root = fresh_moss_dir("ok");
        let moss_dir = root.join(".moss");
        std::fs::create_dir_all(&moss_dir).unwrap();
        let output_dir = root.join("site");
        std::fs::create_dir_all(&output_dir).unwrap();

        let cover = stored_cover(&moss_dir, &tiny_png_bytes());

        let mut meta = blank_link_meta("https://example.com/post");
        meta.cover = Some(cover.clone());
        let mut link_meta = HashMap::new();
        link_meta.insert(meta.url.clone(), meta);

        let mut pending = PendingManifest::new(SiteHashes::default());
        let count = materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut pending);
        assert_eq!(count, 1);

        let result = &link_meta["https://example.com/post"];
        let served_path = result.cover_served_path.as_deref().expect("cover must be materialized");
        assert!(served_path.starts_with("_moss/link/"), "got: {served_path}");
        assert!(served_path.ends_with(".png"), "got: {served_path}");
        assert!(output_dir.join(served_path).exists(), "original file must exist: {served_path}");

        let webp_path = moss_core::asset_paths::to_webp(served_path);
        assert!(output_dir.join(&webp_path).exists(), "webp variant must exist: {webp_path}");

        assert_eq!(result.cover_width, Some(3));
        assert_eq!(result.cover_height, Some(2));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn re_registers_cached_cover_files_in_a_second_generation() {
        let root = fresh_moss_dir("second_generation");
        let moss_dir = root.join(".moss");
        std::fs::create_dir_all(&moss_dir).unwrap();
        let output_dir = root.join("site");
        std::fs::create_dir_all(&output_dir).unwrap();

        let cover = stored_cover(&moss_dir, &tiny_png_bytes());
        let mut meta = blank_link_meta("https://example.com/post");
        meta.cover = Some(cover.clone());
        let mut link_meta = HashMap::from([(meta.url.clone(), meta)]);

        let mut first_pending = PendingManifest::new(SiteHashes::default());
        assert_eq!(
            materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut first_pending),
            1
        );
        let first_hashes = first_pending.hashes_clone();
        let first = first_pending.seal();
        let original = link_meta["https://example.com/post"].cover_served_path.clone().unwrap();
        let webp = moss_core::asset_paths::to_webp(&original);
        assert!(first.files().contains_key(&original));
        assert!(first.files().contains_key(&webp));

        let mut second_pending = PendingManifest::new(first_hashes);
        assert_eq!(
            materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut second_pending),
            1,
            "a warm cached cover must still be registered for the new generation"
        );
        let second = second_pending.seal();
        assert!(second.files().contains_key(&original));
        assert!(second.files().contains_key(&webp));
        assert!(output_dir.join(&original).exists());
        assert!(output_dir.join(&webp).exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_cloud_only_cover_is_requested_without_delaying_the_page() {
        let root = fresh_moss_dir("cloud_only_cover");
        let moss_dir = root.join(".moss");
        std::fs::create_dir_all(&moss_dir).unwrap();
        let output_dir = root.join("site");
        std::fs::create_dir_all(&output_dir).unwrap();
        let cover = stored_cover(&moss_dir, &tiny_png_bytes());
        let blob = crate::build::page::link_snapshot::cover_path(&moss_dir, &cover);
        let _cloud = crate::build::icloud::pretend::evicted_until_requested(&blob);
        let mut meta = blank_link_meta("https://example.com/post");
        meta.cover = Some(cover.clone());
        let mut link_meta = HashMap::from([(meta.url.clone(), meta)]);
        let mut pending = PendingManifest::new(SiteHashes::default());
        let before = crate::build::cloud_readiness::WAITS_RUN.with(|n| n.get());
        assert_eq!(materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut pending), 0);
        assert_eq!(crate::build::cloud_readiness::WAITS_RUN.with(|n| n.get()), before);
        assert_eq!(crate::build::icloud::pretend::requests_for(&blob), 1);
        assert!(link_meta["https://example.com/post"].cover_served_path.is_none());

        let mut later = PendingManifest::new(SiteHashes::default());
        assert_eq!(materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut later), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    /// A picture in the cloud is requested and held from publish, not reported
    /// missing, and the card drops an earlier build's file this generation lacks.
    #[test]
    fn a_cover_in_the_cloud_is_requested_and_holds_the_publish() {
        let root = fresh_moss_dir("cover_in_cloud");
        let moss_dir = root.join(".moss");
        let output_dir = root.join("site");
        let earlier = "_moss/link/abababababababab.png";
        std::fs::create_dir_all(output_dir.join("_moss/link")).unwrap();
        std::fs::write(output_dir.join(earlier), b"earlier").unwrap();
        let cover = stored_cover(&moss_dir, &tiny_png_bytes());
        let blob = crate::build::page::link_snapshot::cover_path(&moss_dir, &cover);
        let _cloud = crate::build::icloud::pretend::evicted(&blob);
        let mut meta = blank_link_meta("https://example.com/post");
        (meta.cover, meta.cover_served_path) = (Some(cover.clone()), Some(earlier.into()));
        let mut link_meta = HashMap::from([(meta.url.clone(), meta)]);
        let evidence = crate::build::cloud_ledger::InputEvidence::new(&root);
        let mut pending = PendingManifest::for_build(SiteHashes::default(), evidence.clone());
        assert_eq!(materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut pending), 0);
        assert_eq!(crate::build::icloud::pretend::requests_for(&blob), 1);
        assert_eq!(evidence.unresolved_structural(), [format!(".moss/data/social/links/covers/{cover}")]);
        assert!(link_meta["https://example.com/post"].cover_served_path.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_preexisting_unwritable_scratch_file_does_not_block_materialization() {
        // `scratch_source` is named `{oid}.{ext}` — content-addressed, not a
        // fresh UUID — so a leftover from a prior run can already occupy
        // that exact path. Simulate the class of destination a raw
        // `fs::copy` cannot truncate (a dataless cloud placeholder is the
        // real-world case; a read-only file is the portable, no-root stand-in
        // with the same "cannot open O_TRUNC" shape): `rename(2)` replaces it
        // regardless, `fs::copy`'s O_TRUNC open does not.
        let root = fresh_moss_dir("readonly_scratch");
        let moss_dir = root.join(".moss");
        std::fs::create_dir_all(&moss_dir).unwrap();
        let output_dir = root.join("site");
        std::fs::create_dir_all(&output_dir).unwrap();

        let cover = stored_cover(&moss_dir, &tiny_png_bytes());

        let scratch_dir = MossPaths::from_moss_dir(moss_dir.clone()).cache_tmp().join("remote-cover");
        std::fs::create_dir_all(&scratch_dir).unwrap();
        let scratch_source = scratch_dir.join(&cover);
        std::fs::write(&scratch_source, b"stale leftover from an earlier run").unwrap();
        let mut perms = std::fs::metadata(&scratch_source).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o444);
        std::fs::set_permissions(&scratch_source, perms).unwrap();

        let mut meta = blank_link_meta("https://example.com/post");
        meta.cover = Some(cover.clone());
        let mut link_meta = HashMap::new();
        link_meta.insert(meta.url.clone(), meta);

        let mut pending = PendingManifest::new(SiteHashes::default());
        let count = materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut pending);
        assert_eq!(count, 1, "a stale unwritable scratch file must not block materialization");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn leaves_no_cover_when_there_is_nothing_downloaded() {
        let root = fresh_moss_dir("none");
        let moss_dir = root.join(".moss");
        std::fs::create_dir_all(&moss_dir).unwrap();
        let output_dir = root.join("site");
        std::fs::create_dir_all(&output_dir).unwrap();

        let mut link_meta = HashMap::new();
        link_meta.insert("https://example.com/post".to_string(), blank_link_meta("https://example.com/post"));

        let mut pending = PendingManifest::new(SiteHashes::default());
        let count = materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut pending);
        assert_eq!(count, 0);
        assert!(link_meta["https://example.com/post"].cover_served_path.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_corrupt_stored_blob_falls_back_to_no_cover_without_panicking() {
        // The oid points at bytes that are no longer a real image (defensive:
        // decode-verification happened at download time, but this proves the
        // materialize step degrades safely rather than trusting that check
        // forever).
        let root = fresh_moss_dir("corrupt");
        let moss_dir = root.join(".moss");
        std::fs::create_dir_all(&moss_dir).unwrap();
        let output_dir = root.join("site");
        std::fs::create_dir_all(&output_dir).unwrap();

        let cover = stored_cover(&moss_dir, b"not actually a png");

        let mut meta = blank_link_meta("https://example.com/post");
        meta.cover = Some(cover.clone());
        let mut link_meta = HashMap::new();
        link_meta.insert(meta.url.clone(), meta);

        let mut pending = PendingManifest::new(SiteHashes::default());
        let count = materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut pending);
        assert_eq!(count, 0);
        assert!(link_meta["https://example.com/post"].cover_served_path.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }
}
