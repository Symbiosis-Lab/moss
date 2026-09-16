//! The promise half of the image pipeline: every variant URL the synthesizer
//! will emit is registered before first paint, and a source this build cannot
//! encode is settled instead of left unregistered.
//!
//! Extracted from `render/blocking.rs` on 2026-09-05 (MIGRATION-STATE: a
//! feature touching that file extracts before it lands) together with the
//! change that made a never-encoded variant settle `Failed`; the registration
//! loop itself is a pure move.
//!
//! Mirrors the video pattern in `render/blocking.rs` for image webp
//! variants. After registration, the preview server's
//! handle_asset_request will return a 200 LQIP-bytes response for
//! any registered .webp URL that hasn't been materialized yet,
//! instead of falling through to ServeDir's 404 — which is
//! non-recoverable inside <picture> per the HTML spec
//! (a chosen <source> 404 sets image state to broken and fires
//! error; the browser does NOT walk back to the inner <img>).
//!
//! Pattern: explicit promise model (GraphQL @defer, Bazel
//! ActionResult). See docs/archive/2026-05-20-image-variant-honest-
//! mirror.md (Layer 1). Closes the catastrophic 404 verified at
//! ~/Library/Logs/host.moss.publisher/moss.log L2006/L2446/L2450
//! on 2026-05-19.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::build::media::image::{ImageConversionItem, SkipReason};
use crate::types::assets::AssetRegistry;
use crate::types::content::ProjectStructure;

/// Register every collected item's variant URLs with `registry` (when the
/// build has one) and return the items the encoder should run — the ones
/// whose bytes are here and whose verdict is "encode".
///
/// `rung_collisions` is `rungs::rung_collision_map`, built once per build and
/// also threaded to the encode worker; `vault_root` resolves each source to
/// the absolute file the preview serves through the passthrough.
pub(crate) fn promise_image_variants(
    registry: Option<&AssetRegistry>,
    items: Vec<ImageConversionItem>,
    project_structure: &ProjectStructure,
    dir_overrides: &HashMap<String, String>,
    rung_collisions: &HashMap<String, PathBuf>,
    vault_root: &str,
) -> Vec<ImageConversionItem> {
    if let Some(registry) = registry {
        for item in &items {
            // Map source path through dir_overrides, then derive
            // the .webp URL. This ensures the registry key matches
            // exactly what the synthesizer emits as
            // <source srcset="./assets/X.webp">. The preview
            // server's normalized_path strips the leading slash;
            // the synthesizer's leading `./` does not survive the
            // browser's URL resolution. So both sides settle on
            // `assets/X.webp` as the registry key.
            let mapped = crate::build::scan::page_map::resolve_path_with_overrides(
                &item.source_path.to_string_lossy(),
                dir_overrides,
            );
            let webp_path = moss_core::asset_paths::to_webp(&mapped);

            // Look up the source's MediaMetadata to carry
            // dimensions / dominant_color into the registry. NOT the
            // LQIP: a blur is a published-site technique for a visitor
            // on a network, and in the preview it would be a stand-in
            // the author could mistake for their own photo. The preview
            // serves the full original instead — see
            // `set_source_passthrough` below.
            let img_meta = project_structure
                .image_files
                .iter()
                .find(|m| std::path::Path::new(&m.path) == item.source_path);

            let dimensions = img_meta.and_then(|m| m.dimensions);
            let dominant_color = img_meta.and_then(|m| m.dominant_color.clone());

            // The variant URLs the synthesizer has already emitted for this
            // source: the base webp and, for a ladder source, its rungs. The
            // registry key must EXACTLY equal the emitted URL: same `mapped`
            // path + same to_webp_rung + same ladder_rungs(w, h, false) over
            // the same scan dims. (ladder_rungs is height-aware — pass BOTH
            // scan dims, NEVER `(w, w, false)`.) The `false` literal, the
            // animated-flag proof, and the EXIF-oriented png/webp dims
            // agreement are documented canonically on asset_paths::ladder_rungs.
            // `is_ladder_source_ext` covers png/jpg/jpeg AND webp, so a webp
            // source registers its rung URLs here too (its base URL is the
            // source itself: to_webp(webp) == webp).
            let mut variants = vec![webp_path];
            if moss_core::asset_paths::is_ladder_source_ext(&item.ext) {
                if let Some((w, h)) = dimensions {
                    for rung in moss_core::asset_paths::ladder_rungs(w, h, false) {
                        let rung_url = moss_core::asset_paths::to_webp_rung(&mapped, *rung);
                        if let Some(user_file) = rung_collisions.get(&rung_url) {
                            log::warn!(
                                "'{}' is named like a generated responsive variant of '{}'; keeping your file — rename '{}' to avoid the reserved '.w{}.webp' naming pattern if you meant it to be a different image",
                                user_file.display(),
                                item.source_path.display(),
                                user_file.display(),
                                rung
                            );
                            // Deferred-window guard: a small colliding
                            // file may never register its own promise
                            // (it converts to photo.w800.webp only if
                            // IT is a convertible raster), yet the
                            // emitted srcset references this URL before
                            // asset copy lands the file. Passthrough
                            // the USER'S file so the preview serves its
                            // real bytes in that window instead of 404.
                            registry.set_source_passthrough(rung_url, user_file.clone());
                            continue;
                        }
                        variants.push(rung_url);
                    }
                }
            }

            // Source passthrough: the original's own resolved URL (the inner
            // <img>) and, while the encoder is still working, every variant
            // URL map to the ABSOLUTE source file so the preview serves the
            // FULL ORIGINAL bytes (sharp). Resolve the source the same way
            // run_image_conversion does (project_root.join(source_path)).
            let source_abs = std::path::Path::new(vault_root).join(&item.source_path);
            registry.set_source_passthrough(mapped.clone(), source_abs.clone());

            // A variant this build will never encode settles `Failed`
            // instead, so the post-seal degrade pass drops its `<source>`
            // and the page keeps the original `<img>` — see
            // `ImageConversionItem::skip`.
            let never = item.skip.as_ref().and_then(SkipReason::failure_message);
            for url in variants {
                match never {
                    Some(why) => registry.set_failed(url, why.to_string()),
                    None => {
                        registry.set_source_passthrough(url.clone(), source_abs.clone());
                        registry.set_pending(url, dimensions, dominant_color.clone());
                    }
                }
            }
        }
    }

    // Registration is over ALL collected items; the ENCODER gets only the
    // ones it can encode (moss#982).
    //
    // An item carrying a `skip` verdict has just had its variant URLs
    // settled above — Pending with a passthrough for a source still in the
    // cloud, Failed for one that will never encode — which is the whole
    // reason it was collected (`ImageConversionItem::skip`). Handing it to
    // the encoder as well would read the source, take an `EDEADLK`, and
    // `set_failed` the variant, replacing "still downloading" with a
    // "⚠ asset failed" tile for a file that is on its way down. Dropping
    // it here also keeps the background progress total honest: it counts
    // work this build can do.
    items.into_iter().filter(|i| i.skip.is_none()).collect()
}

#[cfg(test)]
#[path = "promise_tests.rs"]
mod tests;
