//! Responsive-ladder rung encode unit.
//!
//! Extracted from `build/media/image.rs` as part of the responsive-image-variants
//! plan — a pure code move, zero behavior change. Owns the rung-specific encode path
//! (`encode_rungs`), the shared EXIF-aware decode front half
//! (`decode_oriented`), the per-rung result carrier (`RungOutcome`), and the
//! user-file collision map (`rung_collision_map`). The dispatch-side rung loop
//! (in `convert_single_image` / `run_image_conversion`) and the
//! fingerprint-skip heal block stay in `image.rs` and CALL into this module.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::image::{
    apply_exif_orientation, encode_webp, read_exif_orientation,
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

/// A source picture's pixel size after orientation, from its header alone:
/// what scan stores, without decoding.
pub(super) fn sniffed_oriented_dims(source_file: &Path) -> Option<(u32, u32)> {
    crate::build::scan::scan::extract_image_dimensions(source_file)
}

/// The cached base picture of `source_oid`: made with today's recipe, or with
/// an older one whose pixels are still what today's resize gives.
pub(super) fn kept_base(
    transforms: &crate::build::cache::TransformCache,
    source_oid: &str,
    params: &serde_json::Value,
    source_file: &Path,
    max_edge: u32,
) -> Option<crate::build::cache::TransformEntry> {
    let today = || {
        let (w, h) = sniffed_oriented_dims(source_file)?;
        Some(crate::build::cache::Need::Scaled { source: (w, h), bound: moss_core::asset_paths::deployed_long_edge(w, h, max_edge) })
    };
    transforms.find_kept_entry(source_oid, "image/webp", params, crate::build::cache::RecordMode::Request, crate::build::cache::Fit::Kept(&today))
}

/// Which rungs each picture in this build offers, decided once in blocking.rs
/// and threaded to the worker (`BackgroundContext` → `ImageRunContext`): the
/// worker has no `ProjectStructure` and must never recompute it, since a
/// divergent set is a registered-but-never-encoded rung, a sealed deploy with a
/// 404ing `<picture>` candidate.
#[derive(Debug, Clone, Default)]
pub struct RungPlan {
    /// See [`rung_collision_map`]: a rung URL found here is a user's own file.
    pub collisions: HashMap<String, PathBuf>,
    pub kept: KeptRungs,
}

impl RungPlan {
    /// The rung widths of the picture whose base webp is served at `base_webp`,
    /// from its oriented `w`×`h`. Every pipeline site asks this, as emission
    /// asks `offered_rungs` with the snapshot's kept widths.
    pub fn widths(&self, base_webp: &str, w: u32, h: u32) -> Vec<u32> {
        moss_core::asset_paths::offered_rungs(w, h, false, self.kept.get(base_webp))
    }

    /// The singleflight key of the encode of `source_oid` at `base_webp`.
    /// Duplicate bytes share an encode only when they offer the same rungs: a
    /// published copy keeps its widths while a new copy gets today's ladder, and
    /// each stages the shared outcome's rungs.
    pub fn flight(&self, source_oid: &str, base_webp: &str) -> String {
        match self.kept.get(base_webp) {
            Some(kept) => format!("{source_oid} {kept:?}"),
            None => source_oid.to_string(),
        }
    }
}

/// The rung widths the live deploy record lists for each published picture,
/// keyed by its served base webp. Read from the record because that is folder
/// state: a build cache would make the srcset depend on one machine's history.
#[derive(Debug, Clone, Default)]
pub struct KeptRungs(std::sync::Arc<HashMap<String, Vec<u32>>>);

impl KeptRungs {
    /// The record the removed-address check reads. No record, or an unreadable
    /// one, keeps nothing: every picture offers today's ladder.
    pub fn load(mp: &crate::moss_paths::MossPaths) -> Self {
        let target = crate::build::manifest::backfill::publish_target(mp);
        crate::build::manifest::published_record::load_address_baseline(mp, target.as_deref())
            .map(|record| Self::from_files(record.files.keys().map(String::as_str)))
            .unwrap_or_default()
    }

    /// From a record's served paths: every webp is a base that keeps the
    /// widths of the `<stem>.wN.webp` files beside it.
    pub fn from_files<'a>(files: impl IntoIterator<Item = &'a str>) -> Self {
        let mut kept: HashMap<String, Vec<u32>> = HashMap::new();
        let mut rungs = Vec::new();
        for file in files {
            let Some(stem) = file.strip_suffix(".webp") else { continue };
            kept.entry(file.to_string()).or_default();
            if let Some((base, width)) = stem.rsplit_once(".w").and_then(|(base, w)| Some((base, w.parse::<u32>().ok()?))) {
                rungs.push((format!("{base}.webp"), width));
            }
        }
        for (base, width) in rungs {
            if let Some(widths) = kept.get_mut(&base) {
                widths.push(width);
            }
        }
        for widths in kept.values_mut() {
            widths.sort_unstable();
        }
        Self(std::sync::Arc::new(kept))
    }

    /// The widths kept for the picture served at `base_webp`; `None` when the
    /// record does not list it.
    pub fn get(&self, base_webp: &str) -> Option<&[u32]> {
        self.0.get(base_webp).map(Vec::as_slice)
    }
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
/// pre-existing blindness the base-webp path has). It rides [`RungPlan`].
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
                crate::build::scan::path_collision::on_disk(Path::new(&project_structure.root_path), &m.path),
            )
        })
        .collect()
}

/// Decode `source_file` (format sniffed by magic bytes, allocation-capped —
/// see `media::decode::sniff_decode`) and apply its EXIF orientation. The
/// shared decode front half of the base webp pass and the rung encodes.
pub(crate) fn decode_oriented(source_file: &Path) -> Result<image::DynamicImage, String> {
    let orientation = read_exif_orientation(source_file);
    let img = super::decode::sniff_decode(source_file)
        .map_err(|e| format!("Failed to open image: {}", e))?;
    Ok(apply_exif_orientation(img, orientation))
}

/// Encode the responsive ladder rungs for one source image.
///
/// Runs AFTER the base webp landed (on both the cache-hit fast path and the
/// full encode path of [`convert_single_image`]) so a base failure never
/// yields rung-only output. Per `ladder_rungs` width — warm: a cached
/// `image/webp-w{N}` blob is linked to staging (no decode); cold:
/// [`super::scale::scale_exact`] from the decoded ORIENTED original to EXACTLY
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
/// — can link the shared blob; skipping would strand its REGISTERED rung.
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
/// (e.g. a w1600 rung of a 1603w source) and still served, so this
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
    rung_plan: &RungPlan,
    decoded: Option<&image::DynamicImage>,
    base_webp_len: u64,
) -> Vec<RungOutcome> {
    use crate::build::cache::{Fit, TransformEntry};
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
        None => match sniffed_oriented_dims(source_file) {
            Some(dims) => Some(dims),
            None => match decode_oriented(source_file) {
                Ok(img) => {
                    let d = (img.width(), img.height());
                    lazy = Some(img);
                    Some(d)
                }
                Err(e) => {
                    log::info!(
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
    let ladder = rung_plan.widths(relative_webp, w, h);
    let mut outcomes = Vec::with_capacity(ladder.len());
    for rung in ladder {
        let rung_rel = asset_paths::to_webp_rung(relative_webp, rung);
        // Same membership test as registration (blocking.rs) — the user's
        // file wins at a collided path; never write there. Warn already
        // fired at registration, so debug here.
        let write_allowed = !rung_plan.collisions.contains_key(&rung_rel);
        if !write_allowed {
            log::debug!(
                "[image] rung {} collides with a user file — encoding to cache only, \
                 not writing the generated variant",
                rung_rel
            );
        }
        let kind = format!("image/webp-w{}", rung);
        let out_path = staging_dir.join(&rung_rel);
        // EXACT target dims: same integer family as `deployed_width` (u64
        // mul, truncating div, floor at 1).
        let target_h = ((h as u64 * rung as u64 / w as u64) as u32).max(1);

        // Design doc § Guards: a rung at least as heavy as the base webp is
        // an anomaly worth one log line — debug, not warn, because it is
        // expected near the ladder cap (e.g. w1600 with a 1603w base). The
        // rung is still cached/linked (a registered rung must
        // exist), so this is observability only. DEBUG rather than INFO
        // because it fires per image PER RUNG: an expected outcome multiplied
        // by the ladder size is a flood, not a signal.
        let log_rung_anomaly = |rung_len: u64| {
            if base_webp_len > 0 && rung_len >= base_webp_len {
                log::debug!(
                    "[image] rung {} is {} bytes, not smaller than the base webp's {} \
                     bytes — expected near the ladder cap; serving it anyway",
                    rung_rel, rung_len, base_webp_len
                );
            }
        };

        // ---- Warm path: reuse the cached rung blob. ----
        let today = || Some(crate::build::cache::Need::Dims(rung, target_h));
        let kept = transforms.find_kept_entry(source_oid, &kind, &params, crate::build::cache::RecordMode::Request, Fit::Kept(&today));
        if let Some(cached) = kept.map(|entry| entry.oid) {
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

        // ---- Cold path: scale from the decoded ORIENTED original. ----
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
        let resized = super::scale::scale_exact(img, rung, target_h);
        // No alpha flatten: `encode_webp` encodes alpha directly (`from_rgba`);
        // see `image.rs::flatten_alpha_to_white`'s doc comment for why this
        // rung pass never needed it. `params` above is `config.to_params()`,
        // whose cache-key rename (`alpha_preserved`) also forces a cached
        // pre-fix (flattened) rung blob to re-encode rather than be reused.
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
        let source_size = fs::metadata(source_file).map(|m| m.len()).unwrap_or(0);
        let merged = transforms.merge(source_oid, source_size, crate::build::cache::RecordMode::Request, |record| {
            record.transforms.insert(
                kind,
                TransformEntry {
                    kept_for: None, validation: None,
                    oid: oid.clone(),
                    size: webp_bytes.len() as u64,
                    params: params.clone(),
                },
            );
        });
        if let Err(e) = merged {
            log::info!("[image] failed to write rung transform record: {}", e);
        }
        outcomes.push(RungOutcome { width: rung, oid: Some(oid), error: None });
    }
    outcomes
}

/// A short memory of responsive sizes a re-dispatch failed to produce.
///
/// An unchanged image whose responsive size is missing from the cache is sent
/// back to the conversion worker to encode it. When the cause persists (the
/// store refuses the blob, the link into the output fails, the encoder declines
/// the size) that would repeat on every build, so a re-dispatch that did not
/// produce its size is noted here, and the skip decision leaves that size alone
/// for [`WINDOW`]. The note lives in memory only: a new process tries again.
pub(crate) mod redispatch_note {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, LazyLock, Mutex};
    use std::time::{Duration, Instant};

    /// How long a size that was not produced is left alone.
    pub(crate) const WINDOW: Duration = Duration::from_secs(10 * 60);

    /// The clock, injected so tests move it instead of sleeping.
    pub(crate) type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

    pub(crate) fn system_clock() -> Clock {
        Arc::new(Instant::now)
    }

    /// (cache root, source oid, output path of the size) -> when it was not produced.
    static NOTES: LazyLock<Mutex<HashMap<(PathBuf, String, String), Instant>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

    fn key(root: &Path, oid: &str, rung_rel: &str) -> (PathBuf, String, String) {
        (root.to_path_buf(), oid.to_string(), rung_rel.to_string())
    }

    fn is_fresh(root: &Path, oid: &str, rung_rel: &str, now: Instant) -> bool {
        let Ok(notes) = NOTES.lock() else { return false };
        notes.get(&key(root, oid, rung_rel)).is_some_and(|at| now.saturating_duration_since(*at) < WINDOW)
    }

    /// The first of the missing sizes whose last re-dispatch is not recent: the reason to send the image back.
    pub(crate) fn to_retry<'a>(root: &Path, oid: &str, missing: &[&'a str], now: Instant) -> Option<&'a str> {
        missing.iter().copied().find(|rel| {
            let held = is_fresh(root, oid, rel, now);
            if held {
                log::debug!("{rel} was not produced by the last re-dispatch; not retrying yet");
            }
            !held
        })
    }

    /// After a re-dispatch has run: a size now in staging is forgotten, one still
    /// missing is noted, and notes past their window are dropped. A cancelled run
    /// never reached some images, so it forgets what is present but notes nothing.
    /// Presence is probed before the lock is taken: the staging directory may be
    /// slow, and the lock is shared by every site's dispatch decision.
    pub(crate) fn settle(root: &Path, staging_dir: &Path, redispatched: &[(String, String)], now: Instant, cancelled: bool) {
        let probed: Vec<_> = redispatched
            .iter()
            .map(|(oid, rung_rel)| (key(root, oid, rung_rel), crate::build::io_utils::output_present(&staging_dir.join(rung_rel))))
            .collect();
        let Ok(mut notes) = NOTES.lock() else { return };
        notes.retain(|_, at| now.saturating_duration_since(*at) < WINDOW);
        for (k, present) in probed {
            if present {
                notes.remove(&k);
            } else if !cancelled {
                notes.insert(k, now);
            }
        }
    }
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
