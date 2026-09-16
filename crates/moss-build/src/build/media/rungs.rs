//! Responsive-ladder rung encode unit.
//!
//! Extracted from `build/media/image.rs` per
//! `docs/reference/target/MIGRATION-STATE.md` (the `build/media/image.rs`
//! debt row) / the responsive-image-variants plan, Task 10.5 — a pure code
//! move, zero behavior change. Owns the rung-specific encode path
//! (`encode_rungs`), the shared EXIF-aware decode front half
//! (`decode_oriented`), the per-rung result carrier (`RungOutcome`), and the
//! user-file collision map (`rung_collision_map`). The dispatch-side rung loop
//! (in `convert_single_image` / `run_image_conversion`) and the
//! fingerprint-skip heal block stay in `image.rs` and CALL into this module.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::image::{
    apply_exif_orientation, encode_webp, flatten_alpha_to_white, read_exif_orientation,
    validate_webp_output, ImageCompressionConfig,
};

/// One ladder rung's encode result, carried inside [`ImageConversionOutcome`].
///
/// Convention mirrors the parent: `error.is_some()` ⇔ this rung failed;
/// a failure here neither rolls back the base nor stops other rungs.
///
/// [`ImageConversionOutcome`]: super::image::ImageConversionOutcome
#[derive(Debug, Clone)]
pub struct RungOutcome {
    /// Rung width in px (the `N` of the `.wN.webp` filename).
    pub width: u32,
    /// OID of the encoded rung webp in the object store. `None` on error.
    pub oid: Option<String>,
    /// Set when this rung's encode/link failed.
    pub error: Option<String>,
}

/// Dir-override-mapped path of every vault image file → the ABSOLUTE vault
/// source path of that file, for detecting collisions between generated
/// responsive-rung URLs and REAL user files.
///
/// A vault file literally named like a rung (`photo.w800.webp` beside
/// `photo.jpg`) must win over the generated variant — never clobber user
/// content (design § Guards). Callers build this map ONCE per build and
/// test each candidate rung URL for KEY membership; a hit means "skip
/// registration/write for this rung, warn", and the VALUE names the user's
/// actual file for the warning + source-passthrough. Rung URLs always end
/// in `.webp`, and scan collects every `.webp` vault file into
/// `image_files`, so the key set is a complete collision domain — except
/// passthrough-rooted files, which scan does not walk (the same
/// pre-existing blindness the base-webp path has).
///
/// Shared by blocking.rs (registration skip, Task 4) and the background
/// encode worker (write skip, Task 5). THREAD the result to the worker
/// (`BackgroundContext` → `ImageRunContext`) — don't recompute it there:
/// the worker has no `ProjectStructure`, and a divergent set means a
/// registered-but-never-encoded rung, i.e. a sealed deploy with a 404ing
/// `<picture>` candidate (ADR-013).
pub(crate) fn rung_collision_map(
    project_structure: &crate::types::content::ProjectStructure,
    dir_overrides: &HashMap<String, String>,
) -> HashMap<String, PathBuf> {
    project_structure
        .image_files
        .iter()
        .map(|m| {
            (
                crate::build::scan::page_map::resolve_path_with_overrides(
                    &m.path,
                    dir_overrides,
                ),
                Path::new(&project_structure.root_path).join(&m.path),
            )
        })
        .collect()
}

/// Decode `source_file` (format sniffed by magic bytes, allocation-capped)
/// and apply its EXIF orientation. The shared decode front half of the base
/// webp pass and the rung encodes.
///
/// The 1 GiB `max_alloc` cap is defense-in-depth against decompression bombs
/// / wrong-dimension headers: the MegapixelBudget bounds *concurrent* decoded
/// pixels, but nothing else caps a *single* decode's allocation — a tiny file
/// declaring enormous dimensions would decode to multiple GB and OOM-kill the
/// process regardless of the budget. 1 GiB ≈ 250 MP of RGBA, orders of
/// magnitude above any real photo, so legitimate images are never rejected.
pub(crate) fn decode_oriented(source_file: &Path) -> Result<image::DynamicImage, String> {
    use image::io::Reader as ImageReader;
    const DECODE_ALLOC_CEILING: u64 = 1024 * 1024 * 1024;
    let orientation = read_exif_orientation(source_file);
    let img = ImageReader::open(source_file)
        .and_then(|r| r.with_guessed_format())
        .map_err(image::ImageError::IoError)
        .and_then(|mut r| {
            let mut limits = image::io::Limits::default();
            limits.max_alloc = Some(DECODE_ALLOC_CEILING);
            r.limits(limits);
            r.decode()
        })
        .map_err(|e| format!("Failed to open image: {}", e))?;
    Ok(apply_exif_orientation(img, orientation))
}

/// Encode the responsive ladder rungs for one source image.
///
/// Runs AFTER the base webp landed (on both the cache-hit fast path and the
/// full encode path of [`convert_single_image`]) so a base failure never
/// yields rung-only output. Per `ladder_rungs` width — warm: a cached
/// `image/webp-w{N}` blob is linked to staging (no decode); cold:
/// `resize_exact` from the decoded ORIENTED original to EXACTLY
/// `(rung, h·rung/w)` — the u64 mul/div `.max(1)` integer family of
/// `asset_paths::deployed_width` — so the emitted `w`-descriptor and encoded
/// pixels agree. Bounding-box `resize()` is FORBIDDEN: it can undershoot the
/// target width by 1px on adversarial ratios; `validate_webp_output` pins the
/// EXACT dims.
///
/// `decoded` is `Some` on the full-encode path (reusing the base pass's
/// image); `None` on the cache-hit path, which decodes LAZILY only if a rung
/// misses the cache — the all-warm rebuild stays decode-free. Ladder
/// membership derives from oriented dims: decoded dims, else header dims +
/// EXIF swap (what scan stores, so membership byte-matches registration).
///
/// Collision-mapped rung paths (a USER file named like a rung) are encoded
/// and cached but NEVER written: the user's file wins there (registration
/// skipped its promise too — it warned; we only debug-log). The encode still
/// runs so a singleflight WAITER — duplicate content at a non-colliding path
/// — can link the shared blob; skipping would strand its REGISTERED rung (ADR-013).
///
/// Failures are per-rung (`RungOutcome::error`): one bad rung neither rolls
/// back the base nor stops the rest. NO outcome-based skipping (keep-smaller,
/// APNG verbatim-keep, …) — a registered rung MUST be encoded or a sealed
/// deploy ships a 404ing `<picture>` candidate (see the WARNING on
/// `asset_paths::ladder_rungs`). Scan now stores the ORIENTED dims for every
/// ladder source (EXIF-oriented png/webp included — the scan-swap fix, design
/// follow-up #1), so scan's ladder and this unit's oriented ladder agree by
/// construction. Were a future change to reintroduce a scan-vs-decode divergence,
/// only ONE direction is benign: when decode's ladder is a SUPERSET of scan's (or
/// scan-dims-None registered/emitted nothing), the extra encoded rungs are
/// unreferenced — no 404 risk, stale-output cleanup sweeps them; never "fix" that
/// by narrowing the encode side. The NON-superset direction would strand an
/// emission/registration-promised rung the encode never produces → publish-404
/// — the class the scan-swap fix closed; keep scan and encode on the same
/// oriented dims rather than narrowing this unit.
/// `base_webp_len` is the base webp's encoded byte count (cache-record size on
/// the warm base path, fresh encode length on the cold path — both already in
/// hand at the call sites, no extra I/O). A rung whose bytes are >= the base's
/// is logged at info (design doc § Guards): it is EXPECTED near the ladder cap
/// (e.g. a w1600 rung of a 1603w source) and still served per ADR-013, so this
/// is observability only — never a skip.
///
/// [`convert_single_image`]: super::image::convert_single_image
#[allow(clippy::too_many_arguments)]
pub(crate) fn encode_rungs(
    source_file: &Path,
    source_oid: &str,
    relative_webp: &str,
    staging_dir: &Path,
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    config: &ImageCompressionConfig,
    rung_collisions: &HashMap<String, PathBuf>,
    decoded: Option<&image::DynamicImage>,
    base_webp_len: u64,
) -> Vec<RungOutcome> {
    use crate::build::cache::{TransformEntry, TransformRecord};
    use moss_core::asset_paths;

    // Oriented dims: from the decoded image when we have it (post-EXIF by
    // construction), else header dims + EXIF swap — exactly what scan stores,
    // so the ladder agrees with what registration promised. If even the
    // header is unreadable (practically unreachable behind a cache hit for
    // this very content), fall back to a full decode: speed is never worth a
    // registered-but-never-encoded rung.
    let mut lazy: Option<image::DynamicImage> = None;
    let dims = match decoded {
        Some(img) => Some((img.width(), img.height())),
        None => match image::image_dimensions(source_file) {
            Ok((w, h)) => {
                if crate::build::scan::scan::should_swap_dimensions(read_exif_orientation(
                    source_file,
                )) {
                    Some((h, w))
                } else {
                    Some((w, h))
                }
            }
            Err(_) => match decode_oriented(source_file) {
                Ok(img) => {
                    let d = (img.width(), img.height());
                    lazy = Some(img);
                    Some(d)
                }
                Err(e) => {
                    log::warn!(
                        "[image] cannot determine dimensions for rung encode of {}: {}",
                        relative_webp, e
                    );
                    None
                }
            },
        },
    };
    // Empty return = no rungs RESOLVED, not "no rungs registered": the
    // worker's unresolved-promise sweep (run_image_conversion success arm)
    // compares this against the registration-side ladder and set_failed's
    // any promise left uncovered, so these bail-outs can't strand a
    // registered rung in Pending.
    let Some((w, h)) = dims else { return Vec::new() };
    if w == 0 || h == 0 {
        return Vec::new();
    }

    let params = config.to_params();
    // `false` literal (webp reaches here only when non-animated — should_skip
    // filters animated webp; png/jpg/jpeg are never animated here): rationale +
    // the EXIF-oriented dims agreement live on asset_paths::ladder_rungs.
    let ladder = asset_paths::ladder_rungs(w, h, false);
    let mut outcomes = Vec::with_capacity(ladder.len());
    for &rung in ladder {
        let rung_rel = asset_paths::to_webp_rung(relative_webp, rung);
        // Same membership test as registration (blocking.rs) — the user's
        // file wins at a collided path; never write there. Warn already
        // fired at registration, so debug here.
        let write_allowed = !rung_collisions.contains_key(&rung_rel);
        if !write_allowed {
            log::debug!(
                "[image] rung {} collides with a user file — encoding to cache only, \
                 not writing the generated variant",
                rung_rel
            );
        }
        let kind = format!("image/webp-w{}", rung);
        let out_path = staging_dir.join(&rung_rel);

        // Design doc § Guards: a rung at least as heavy as the base webp is
        // an anomaly worth one log line — debug, not warn, because it is
        // expected near the ladder cap (e.g. w1600 with a 1603w base). The
        // rung is still cached/linked per ADR-013 (a registered rung must
        // exist), so this is observability only. DEBUG rather than INFO
        // because it fires per image PER RUNG: an expected outcome multiplied
        // by the ladder size is a flood, not a signal.
        let log_rung_anomaly = |rung_len: u64| {
            if base_webp_len > 0 && rung_len >= base_webp_len {
                log::debug!(
                    "[image] rung {} is {} bytes, not smaller than the base webp's {} \
                     bytes — expected near the ladder cap; serving it anyway (ADR-013)",
                    rung_rel, rung_len, base_webp_len
                );
            }
        };

        // ---- Warm path: reuse the cached rung blob. ----
        if let Some(cached) = transforms.find_cached_output(source_oid, &kind, &params) {
            // Cheap 0-byte guard (iCloud-eviction class); a bad blob falls
            // through to re-encode, self-healing the cache entry. The stat's
            // length doubles as the anomaly-log input — no extra I/O.
            let blob_len = objects
                .get_path(&cached)
                .and_then(|p| fs::metadata(p).ok())
                .map(|m| m.len())
                .unwrap_or(0);
            if blob_len > 0 {
                log_rung_anomaly(blob_len);
                let linked = if write_allowed {
                    objects.link_to(&cached, &out_path)
                } else {
                    Ok(())
                };
                outcomes.push(match linked {
                    Ok(()) => RungOutcome { width: rung, oid: Some(cached), error: None },
                    Err(e) => RungOutcome {
                        width: rung,
                        oid: None,
                        error: Some(format!("link_to staging failed: {}", e)),
                    },
                });
                continue;
            }
        }

        // ---- Cold path: resize_exact from the decoded ORIENTED original. ----
        let img: &image::DynamicImage = match decoded {
            Some(i) => i,
            None => {
                if lazy.is_none() {
                    match decode_oriented(source_file) {
                        Ok(i) => lazy = Some(i),
                        Err(e) => {
                            outcomes.push(RungOutcome {
                                width: rung,
                                oid: None,
                                error: Some(e),
                            });
                            continue;
                        }
                    }
                }
                lazy.as_ref().unwrap()
            }
        };
        // EXACT target dims: same integer family as `deployed_width` (u64
        // mul, truncating div, floor at 1).
        let target_h = ((h as u64 * rung as u64 / w as u64) as u32).max(1);
        let resized = img.resize_exact(rung, target_h, image::imageops::FilterType::Lanczos3);
        let resized = flatten_alpha_to_white(resized);
        let webp_bytes = match encode_webp(&resized, config.quality, None) {
            Ok(b) => b,
            Err(e) => {
                outcomes.push(RungOutcome { width: rung, oid: None, error: Some(e) });
                continue;
            }
        };
        if let Err(e) = validate_webp_output(&webp_bytes, (rung, target_h)) {
            outcomes.push(RungOutcome { width: rung, oid: None, error: Some(e) });
            continue;
        }
        log_rung_anomaly(webp_bytes.len() as u64);
        let oid = match objects.store_bytes(&webp_bytes) {
            Ok(o) => o,
            Err(e) => {
                outcomes.push(RungOutcome {
                    width: rung,
                    oid: None,
                    error: Some(format!("CAS store failed: {}", e)),
                });
                continue;
            }
        };
        // Staging write via the CAS COW-copy path (`link_to` = fs::copy /
        // fclonefileat reflink — NEVER fs::hard_link; hardlink_invariant_test
        // enforces the iCloud-eviction rule).
        if write_allowed {
            if let Err(e) = objects.link_to(&oid, &out_path) {
                outcomes.push(RungOutcome {
                    width: rung,
                    oid: None,
                    error: Some(format!("link_to staging failed: {}", e)),
                });
                continue;
            }
        }
        // Merge-preserve transform record (same read-modify-write idiom as
        // the base and the sized-raster pass).
        let mut record = transforms.get(source_oid).unwrap_or(TransformRecord {
            source_oid: source_oid.to_string(),
            source_size: fs::metadata(source_file).map(|m| m.len()).unwrap_or(0),
            transforms: std::collections::HashMap::new(),
        });
        record.transforms.insert(
            kind,
            TransformEntry {
                oid: oid.clone(),
                size: webp_bytes.len() as u64,
                params: params.clone(),
            },
        );
        if let Err(e) = transforms.put(&record) {
            log::warn!("[image] failed to write rung transform record: {}", e);
        }
        outcomes.push(RungOutcome { width: rung, oid: Some(oid), error: None });
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::media::image::tests::build_project_with_images;

    // ----- rung_collision_map (Task 4 collision guard) -----

    #[test]
    fn rung_collision_map_flags_vault_file_named_like_a_rung() {
        // A user's real file named exactly like a generated rung
        // (photo.w800.webp beside photo.jpg) must be detected so the
        // pipeline keeps the user's file (design § Guards). The value is
        // the ABSOLUTE vault path of that file — used for the warning and
        // the source-passthrough that serves the user's bytes.
        let (structure, _tmp) = build_project_with_images(&[
            ("assets/photo.jpg", "jpg", None, false),
            ("assets/photo.w800.webp", "webp", None, false),
        ]);
        let map = rung_collision_map(&structure, &HashMap::new());

        // Rung 800 of assets/photo.jpg collides with the user's file …
        let rung_800 = moss_core::asset_paths::to_webp_rung("assets/photo.jpg", 800);
        let user_file = map.get(&rung_800).unwrap_or_else(|| panic!("{rung_800} must collide"));
        assert_eq!(
            *user_file,
            _tmp.path().join("assets/photo.w800.webp"),
            "value must be the absolute vault path of the USER'S colliding file"
        );
        // … rung 1600 does not.
        let rung_1600 = moss_core::asset_paths::to_webp_rung("assets/photo.jpg", 1600);
        assert!(!map.contains_key(&rung_1600), "{rung_1600} must not collide");
    }

    #[test]
    fn rung_collision_map_compares_dir_override_mapped_paths() {
        // Both sides of the comparison are dir-override-mapped: the vault
        // file 视频/photo.w800.webp maps to video/photo.w800.webp, which is
        // exactly the rung URL derived from the MAPPED source path — same
        // derivation blocking.rs uses for the registry key. The VALUE stays
        // the file-tree (unmapped) absolute path — that's where the user's
        // file actually lives on disk.
        let (structure, _tmp) =
            build_project_with_images(&[("视频/photo.w800.webp", "webp", None, false)]);
        let overrides: HashMap<String, String> =
            [("视频".to_string(), "video".to_string())].into_iter().collect();
        let map = rung_collision_map(&structure, &overrides);

        let rung = moss_core::asset_paths::to_webp_rung("video/photo.jpg", 800);
        let user_file = map.get(&rung).unwrap_or_else(|| panic!("{rung} must collide via the mapped path"));
        assert_eq!(
            *user_file,
            _tmp.path().join("视频/photo.w800.webp"),
            "value must be the user's real on-disk (file-tree) path, not the mapped one"
        );
    }
}
