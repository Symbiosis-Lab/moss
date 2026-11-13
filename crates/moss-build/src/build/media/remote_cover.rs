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
//! fetch): the pending manifest is per generation, so every cover must be
//! registered again even when nothing new was fetched. A cover whose
//! dimensions are known is first staged from receipts, as an unchanged vault
//! image is: its transform record, under the current encode parameters, names
//! the blob for the webp and each rung the page will ask for, and the staged
//! links record proves with one stat that the file at each path is that blob
//! (or relinks it from the store). Only when one is missing is the picture
//! decoded and encoded again.

use crate::build::cache::{ObjectStore, TransformCache};
use super::image::{convert_single_image, ImageCompressionConfig};
use crate::build::io_utils;
use crate::build::lifecycle::cas_heal::{rematerialize_with_record, HealOutcome, Placement, StagedLinks};
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::page::link_meta::LinkMeta;
use crate::build::served_path::ServedPath;
use crate::moss_paths::MossPaths;
use std::collections::HashMap;
use std::path::Path;

/// Materialize every URL's downloaded cover (if any) into `output_dir`,
/// updating `link_meta` in place and writing the update back to the
/// on-disk cache so the next reader (this build's own render, or the next
/// build) sees it. Returns the number of covers actually (re-)encoded, not
/// staged from receipts — informational only, callers don't need to act on it.
pub(crate) fn materialize_remote_covers(
    link_meta: &mut HashMap<String, LinkMeta>,
    moss_dir: &Path,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> usize {
    let paths = MossPaths::from_moss_dir(moss_dir.to_path_buf());
    let scratch_dir = paths.cache_tmp().join("remote-cover");
    if let Err(e) = io_utils::create_output_dir_all(&scratch_dir) {
        log::info!(target: "remote-cover", "could not create {:?}: {}", scratch_dir, e);
        return 0;
    }
    let mut stage = Stage {
        objects: ObjectStore::for_site(&paths),
        transforms: TransformCache::for_site(&paths),
        params: ImageCompressionConfig::default().to_params(),
        staged: StagedLinks::load(&paths, "remote-covers", output_dir),
        output_dir,
    };

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
        // The hash that names this asset is the CONTENT hash (the oid), not the
        // URL: two pages linking the same image share one cover. 16 hex chars,
        // matching `ServedPath::for_remote_cover`'s (and og_card's) convention.
        let Ok(original) = ServedPath::for_remote_cover(&oid[..oid.len().min(16)], ext) else { continue };
        let served = Some(original.as_str().to_string());
        if let Some(dims) = meta.cover_width.zip(meta.cover_height) {
            if let Some(Placed { outputs, complete: true }) = stage.place(oid, ext, &cover, &original, dims) {
                register(output_dir, &outputs, pending);
                if meta.cover_served_path != served {
                    meta.cover_served_path = served;
                    crate::build::page::link_meta::save_derived(moss_dir, meta);
                }
                continue;
            }
        }
        let encoded = materialize_one(oid, ext, &cover, &original, &stage.objects, &stage.transforms, &scratch_dir, output_dir);
        // Rungs that did not land are left out, as an encode failure of one
        // rung always was; the next build sees the set incomplete and tries again.
        match encoded.and_then(|e| Some((stage.place(oid, ext, &cover, &original, (e.width, e.height))?, e))) {
            Some((placed, result)) => {
                register(output_dir, &placed.outputs, pending);
                meta.cover_served_path = served;
                meta.cover_width = Some(result.width);
                meta.cover_height = Some(result.height);
                meta.cover_lqip = result.lqip;
                meta.cover_color = result.color;
                crate::build::page::link_meta::save_derived(moss_dir, meta);
                materialized += 1;
            }
            // Nothing this build registered backs the card, and sealing drops
            // every path it did not register, so files an earlier build left
            // in place are not served.
            None => {
                if meta.cover_served_path.take().is_some() {
                    crate::build::page::link_meta::save_derived(moss_dir, meta);
                }
            }
        }
    }
    stage.staged.save();
    materialized
}

/// What placing a cover's outputs consults: the encode records, the store
/// they name, and which blob each staged file was last linked from.
struct Stage<'a> {
    objects: ObjectStore,
    transforms: TransformCache,
    params: serde_json::Value,
    staged: StagedLinks,
    output_dir: &'a Path,
}

impl Stage<'_> {
    /// Puts every file a `width`×`height` cover is served from in staging, each
    /// one the exact bytes it must hold: the original copied from `cover`, and
    /// the webp and every rung the page will ask for, each the blob its
    /// transform record names under the current parameters. Returns what it
    /// placed and whether that was every rung; `None` without the original or
    /// the webp, since then there is nothing to serve.
    fn place(&mut self, oid: &str, ext: &str, cover: &Path, original: &ServedPath, (width, height): (u32, u32)) -> Option<Placed> {
        let record = self.transforms.get_with(oid, crate::build::cache::RecordMode::Wait);
        // `for_jupyterlite_asset` is `ServedPath`'s no-validation passthrough,
        // safe here because every path is derived from the validated `original`.
        let served = |path: &str| ServedPath::for_jupyterlite_asset(Path::new(path));

        let webp = moss_core::asset_paths::to_webp(original.as_str());
        if !self.heal(record.as_ref(), "image/webp", &webp) {
            return None;
        }
        // A webp source is served from the encoded webp itself, at the same
        // path: the source bytes are never staged there.
        let mut outputs = if webp == original.as_str() {
            vec![(original.clone(), HashBucket::ImageOutputs)]
        } else {
            let (Placement::Held | Placement::Linked) = self.staged.link(&self.objects, oid, &self.output_dir.join(original.as_str()), Some(cover)) else { return None };
            vec![(original.clone(), HashBucket::ImageOutputs), (served(&webp), HashBucket::ImageVariants)]
        };
        let rungs = if moss_core::asset_paths::is_ladder_source_ext(ext) { moss_core::asset_paths::ladder_rungs(width, height, false) } else { &[] };
        let mut complete = true;
        for w in rungs {
            let path = moss_core::asset_paths::to_webp_rung(&webp, *w);
            if self.heal(record.as_ref(), &format!("image/webp-w{w}"), &path) {
                outputs.push((served(&path), HashBucket::ImageVariants));
            } else {
                complete = false;
            }
        }
        Some(Placed { outputs, complete })
    }

    /// Whether `path` now holds the blob `record` names for `transform`.
    fn heal(&mut self, record: Option<&crate::build::cache::TransformRecord>, transform: &str, path: &str) -> bool {
        let outcome = rematerialize_with_record(&self.objects, record, &self.params, &mut self.staged, &self.output_dir.join(path), transform);
        matches!(outcome, HealOutcome::AlreadyPresent { .. } | HealOutcome::Healed { .. })
    }
}

/// The files [`Stage::place`] put in staging, original first.
struct Placed {
    outputs: Vec<(ServedPath, HashBucket)>,
    complete: bool,
}

/// Registers each placed file under the hash of the bytes now on disk, never a
/// previous entry: a build that died between staging and sealing leaves a
/// manifest that describes other bytes.
fn register(output_dir: &Path, outputs: &[(ServedPath, HashBucket)], pending: &mut PendingManifest) {
    for (path, bucket) in outputs {
        match std::fs::read(output_dir.join(path.as_str())) {
            Ok(bytes) => pending.register(path, &bytes, *bucket),
            Err(e) => log::info!(target: "remote-cover", "could not read staged {}: {}", path.as_str(), e),
        }
    }
}

struct Encoded {
    width: u32,
    height: u32,
    lqip: Option<String>,
    color: Option<String>,
}

/// Encodes the cover's webp and rungs into the store and reads its size,
/// colour and placeholder. Stages nothing: [`Stage::place`] does.
fn materialize_one(
    oid: &str,
    ext: &str,
    cover: &Path,
    original: &ServedPath,
    objects: &ObjectStore,
    transforms: &TransformCache,
    scratch_dir: &Path,
    output_dir: &Path,
) -> Option<Encoded> {
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

    let len = std::fs::metadata(&scratch_source).ok()?.len();
    if super::image::should_skip(
        &scratch_source,
        ext,
        len,
        &ImageCompressionConfig::default(),
        transforms,
        oid,
        false, // a freshly-materialized scratch file is never cloud-dataless
    )
    .is_some()
    {
        return None;
    }

    let relative_webp = moss_core::asset_paths::to_webp(original.as_str());
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
    outcome.error.is_none().then_some(outcome.webp_oid).flatten()?;

    let dims = crate::build::scan::scan::extract_image_dimensions(&scratch_source);
    let (color, lqip) = crate::build::scan::scan::extract_color_and_lqip(&scratch_source);
    let (width, height) = dims.unwrap_or((800, 600));
    Some(Encoded { width, height, lqip, color })
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

    /// One card for a picture stored as `ext`, built once; returns the root,
    /// the card map, the sealed hashes and the staged original's path.
    fn first_generation(label: &str, bytes: &[u8], ext: &str) -> (std::path::PathBuf, HashMap<String, LinkMeta>, SiteHashes, String) {
        let root = fresh_moss_dir(label);
        let moss_dir = root.join(".moss");
        std::fs::create_dir_all(root.join("site")).unwrap();
        let mut meta = blank_link_meta("https://example.com/post");
        meta.cover = crate::build::page::link_snapshot::store_cover(&moss_dir, bytes, ext);
        let mut link_meta = HashMap::from([(meta.url.clone(), meta)]);
        let mut pending = PendingManifest::new(SiteHashes::default());
        assert_eq!(materialize_remote_covers(&mut link_meta, &moss_dir, &root.join("site"), &mut pending), 1);
        let original = link_meta["https://example.com/post"].cover_served_path.clone().unwrap();
        (root, link_meta, pending.hashes_clone(), original)
    }

    /// Builds again on `hashes`; returns the encode count and the sealed files.
    fn next_generation(root: &Path, link_meta: &mut HashMap<String, LinkMeta>, hashes: SiteHashes) -> (usize, HashMap<String, String>) {
        let mut pending = PendingManifest::new(hashes);
        let encoded = materialize_remote_covers(link_meta, &root.join(".moss"), &root.join("site"), &mut pending);
        (encoded, pending.seal().files().into_owned())
    }

    /// The entry the file now at `path` would be registered under.
    fn entry_on_disk(root: &Path, path: &str) -> String {
        let bytes = std::fs::read(root.join("site").join(path)).unwrap();
        let mut probe = PendingManifest::new(SiteHashes::default());
        probe.register(&ServedPath::from_cached(path).unwrap(), &bytes, HashBucket::ImageVariants);
        probe.files()[path].clone()
    }

    fn image_bytes(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
        let img = image::RgbImage::from_fn(width, height, |x, y| image::Rgb([(x % 251) as u8, (y % 241) as u8, 90]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut buf, format).unwrap();
        buf.into_inner()
    }

    #[test]
    fn a_staged_cover_is_registered_again_without_encoding() {
        let (root, mut link_meta, hashes, original) = first_generation("second_generation", &tiny_png_bytes(), "png");
        let webp = moss_core::asset_paths::to_webp(&original);
        let first = hashes.files.clone();
        let (encoded, second) = next_generation(&root, &mut link_meta, hashes);
        assert_eq!(encoded, 0, "a cover whose receipts hold is not encoded again");
        assert_eq!(second.get(&original), first.get(&original));
        assert_eq!(second.get(&webp), first.get(&webp));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_staged_file_that_went_missing_is_relinked_from_the_store() {
        let (root, mut link_meta, hashes, original) = first_generation("missing_file", &tiny_png_bytes(), "png");
        let webp = moss_core::asset_paths::to_webp(&original);
        std::fs::remove_file(root.join("site").join(&webp)).unwrap();
        let (encoded, files) = next_generation(&root, &mut link_meta, hashes);
        assert_eq!(encoded, 0);
        assert_eq!(files.get(&webp), Some(&entry_on_disk(&root, &webp)));
        let _ = std::fs::remove_dir_all(root);
    }

    /// A webp picture is served from its encoded webp at the picture's own
    /// path. Bytes left there by a build that stopped before sealing, here the
    /// source itself, are replaced by the encoded blob and registered as what
    /// is on disk, not under the last sealed entry.
    #[test]
    fn a_webp_cover_overwritten_before_a_crash_is_restaged_and_registered_as_it_is() {
        let source = image_bytes(900, 500, image::ImageFormat::WebP);
        let (root, mut link_meta, hashes, served) = first_generation("crash", &source, "webp");
        assert_eq!(moss_core::asset_paths::to_webp(&served), served);
        let (_, sealed) = next_generation(&root, &mut link_meta, hashes.clone());
        let encoded = std::fs::read(root.join("site").join(&served)).unwrap();
        assert!(encoded != source, "the fixture needs an encode that changes the bytes");

        std::fs::write(root.join("site").join(&served), &source).unwrap();
        let mut hashes = hashes;
        hashes.files = sealed;
        let (_, files) = next_generation(&root, &mut link_meta, hashes);
        assert!(std::fs::read(root.join("site").join(&served)).unwrap() == encoded, "the encoded webp must be back in place");
        assert_eq!(files.get(&served), Some(&entry_on_disk(&root, &served)));
        let _ = std::fs::remove_dir_all(root);
    }

    /// A rung that did not land leaves the cover incomplete: the next build
    /// puts it in place instead of reusing the rest without it.
    #[test]
    fn a_rung_missing_from_the_last_build_is_put_in_place() {
        let root = fresh_moss_dir("partial");
        let moss_dir = root.join(".moss");
        let output_dir = root.join("site");
        let mut meta = blank_link_meta("https://example.com/post");
        meta.cover = crate::build::page::link_snapshot::store_cover(&moss_dir, &image_bytes(900, 500, image::ImageFormat::Png), "png");
        let oid = meta.cover.clone().unwrap();
        let rung = format!("_moss/link/{}.w800.webp", &oid[..16]);
        // A directory where the rung belongs makes its link fail.
        std::fs::create_dir_all(output_dir.join(&rung)).unwrap();
        let mut link_meta = HashMap::from([(meta.url.clone(), meta)]);
        let mut pending = PendingManifest::new(SiteHashes::default());
        assert_eq!(materialize_remote_covers(&mut link_meta, &moss_dir, &output_dir, &mut pending), 1);
        assert!(!pending.files().contains_key(&rung), "the fixture needs the rung to fail");

        std::fs::remove_dir_all(output_dir.join(&rung)).unwrap();
        let (_, files) = next_generation(&root, &mut link_meta, pending.hashes_clone());
        assert!(output_dir.join(&rung).is_file());
        assert_eq!(files.get(&rung), Some(&entry_on_disk(&root, &rung)));
        let _ = std::fs::remove_dir_all(root);
    }

    /// Outputs encoded under other parameters (an older moss's settings) are
    /// encoded again, not reused because their files are still in place.
    #[test]
    fn a_cover_encoded_under_other_parameters_is_encoded_again() {
        let (root, mut link_meta, hashes, original) = first_generation("params", &tiny_png_bytes(), "png");
        let oid = link_meta["https://example.com/post"].cover.clone().unwrap();
        let oid = oid.split_once('.').unwrap().0;
        let transforms = TransformCache::for_site(&MossPaths::from_moss_dir(root.join(".moss")));
        let mut record = transforms.get_with(oid, crate::build::cache::RecordMode::Wait).unwrap();
        for entry in record.transforms.values_mut() {
            entry.params = serde_json::json!({ "quality": 1 });
        }
        transforms.put(&record, crate::build::cache::RecordMode::Wait).unwrap();

        let (encoded, files) = next_generation(&root, &mut link_meta, hashes);
        assert_eq!(encoded, 1);
        let current = ImageCompressionConfig::default().to_params();
        let record = transforms.get_with(oid, crate::build::cache::RecordMode::Wait).unwrap();
        assert_eq!(record.transforms["image/webp"].params, current);
        let webp = moss_core::asset_paths::to_webp(&original);
        assert_eq!(files.get(&webp), Some(&entry_on_disk(&root, &webp)));
        let _ = std::fs::remove_dir_all(root);
    }

    /// A cover that can be neither staged from receipts nor encoded this build
    /// is not registered, so the card must not keep pointing at its old files:
    /// sealing drops every path this build did not register.
    #[test]
    fn a_cover_this_build_could_not_register_is_not_referenced() {
        let (root, mut link_meta, hashes, original) = first_generation("unregistered", &tiny_png_bytes(), "png");
        let name = link_meta["https://example.com/post"].cover.clone().unwrap();
        let oid = name.split_once('.').unwrap().0;
        TransformCache::for_site(&MossPaths::from_moss_dir(root.join(".moss"))).remove(oid).unwrap();
        let cover = crate::build::page::link_snapshot::cover_path(&root.join(".moss"), &name);
        std::fs::write(&cover, b"no longer a picture").unwrap();
        assert!(root.join("site").join(&original).exists(), "the fixture needs the old files left in place");

        let (_, files) = next_generation(&root, &mut link_meta, hashes);
        let served = link_meta["https://example.com/post"].cover_served_path.clone();
        assert!(served.as_ref().is_none_or(|path| files.contains_key(path)), "{served:?} is referenced but not in the manifest");
        let _ = std::fs::remove_dir_all(root);
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

    /// What staging from receipts saves per build, against encoding every card
    /// again from a warm cache as each build did before. Run it with
    /// `cargo test -p moss-build --lib --release remote_cover::tests::bench -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing, not a check"]
    fn bench_thirty_cards_staged_against_encoded_again() {
        let root = fresh_moss_dir("bench");
        let (moss_dir, output_dir) = (root.join(".moss"), root.join("site"));
        let mut link_meta: HashMap<String, LinkMeta> = (0..30u8)
            .map(|i| {
                let mut meta = blank_link_meta(&format!("https://example.com/{i}"));
                let img = image::RgbImage::from_fn(1200, 630, |x, y| image::Rgb([(x % 251) as u8 ^ i, (y % 241) as u8, i.wrapping_mul(7)]));
                let mut buf = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgb8(img).write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
                meta.cover = crate::build::page::link_snapshot::store_cover(&moss_dir, buf.get_ref(), "jpg");
                (meta.url.clone(), meta)
            })
            .collect();
        let mut hashes = SiteHashes::default();
        let mut run = |link_meta: &mut HashMap<String, LinkMeta>, forget_sizes: bool| {
            if forget_sizes {
                link_meta.values_mut().for_each(|m| m.cover_width = None);
            }
            let mut pending = PendingManifest::new(std::mem::take(&mut hashes));
            let start = std::time::Instant::now();
            let encoded = materialize_remote_covers(link_meta, &moss_dir, &output_dir, &mut pending);
            let took = start.elapsed();
            hashes = pending.hashes_clone();
            (encoded, took)
        };
        let (cold, _) = run(&mut link_meta, false);
        assert_eq!(cold, 30);
        let median = |mut v: Vec<std::time::Duration>| { v.sort(); v[v.len() / 2] };
        let again: Vec<_> = (0..5).map(|_| run(&mut link_meta, true)).inspect(|(n, _)| assert_eq!(*n, 30)).map(|(_, t)| t).collect();
        let staged: Vec<_> = (0..5).map(|_| run(&mut link_meta, false)).inspect(|(n, _)| assert_eq!(*n, 0)).map(|(_, t)| t).collect();
        println!("30 cards, median of 5: encoded again from a warm cache {:?}, staged from receipts {:?}", median(again), median(staged));
        let _ = std::fs::remove_dir_all(root);
    }
}
