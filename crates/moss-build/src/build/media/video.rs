//! Video transcoding and conversion pipeline.
//!
//! ```text
//! Source (.mov)  →  Cache (.mp4)  →  Output (staging_dir + canonical_dir)
//! ```
//!
//! `convert_single_video()` runs one video through cache lookup, thumbnail,
//! MP4 conversion and CAS storage. `dispatch_video_conversions()` is the batch
//! entry point — background tasks in GUI mode, synchronous headlessly — and
//! `run_video_conversion()` is the loop it runs. What happens to one video in
//! that loop is an [`ItemStep`], and everything derived from it (the census,
//! `AssetReady`, the registry, the UiBound budget) is applied in one place.

use crate::moss_paths::MossPaths;
use crate::types::services::{BackgroundContext, BuildServices};
use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::LazyLock;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;

use crate::build::coordinator::EmitMessage;
use crate::build::lifecycle::cas_heal::{rematerialize, HashPolicy, HealOutcome};
use crate::build::manifest::HashBucket;
use crate::advisory::{Action, Advisory, Scope, Severity};
use crate::build::progress::{format_progress_message, spawn_media_child_job, PipelineEvent};

/// Max ffmpeg conversions running at once across ALL dispatch epochs and
/// folder sessions. Each dispatch worker is sequential; overlap comes from
/// stale epochs finishing their current encode. 2 = one fresh + one draining.
const VIDEO_ENCODE_MAX_CONCURRENT: usize = 2;

/// Global semaphore bounding concurrent video encodes. Process-global on
/// purpose: `BuildServices` is re-constructed per dispatch, but the contention
/// is cross-epoch (a stale worker draining its current encode overlaps the
/// fresh one). Pattern: `DOWNLOAD_SEMAPHORE` in plugins/runtime/download.rs.
static VIDEO_ENCODE_SEMAPHORE: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(VIDEO_ENCODE_MAX_CONCURRENT));

/// Cancellation poll cadence while waiting for an encode slot.
const PERMIT_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// Poll-acquire an encode permit from a sync (spawn_blocking) thread.
/// Returns None if `is_cancelled` fires while waiting. Never blocks the
/// async runtime (no block_on); fairness is best-effort, fine for 2 permits.
pub(crate) fn acquire_encode_permit<'a>(
    sem: &'a Semaphore,
    poll: std::time::Duration,
    is_cancelled: &dyn Fn() -> bool,
) -> Option<tokio::sync::SemaphorePermit<'a>> {
    loop {
        if is_cancelled() {
            return None;
        }
        match sem.try_acquire() {
            Ok(p) => return Some(p),
            Err(_) => std::thread::sleep(poll),
        }
    }
}

/// A source's existing transform record, or a fresh empty one keyed to
/// `source_file`'s current size. Shared load step for both `record_transforms`
/// and `record_ladder` below, which differ only in what they do to
/// `record.transforms` before writing it back.
fn load_transform_record(
    transforms: &crate::build::cache::TransformCache,
    source_oid: &str,
    source_file: &Path,
) -> crate::build::cache::TransformRecord {
    use crate::build::cache::TransformRecord;
    transforms.get(source_oid).unwrap_or_else(|| TransformRecord {
        source_oid: source_oid.to_string(),
        source_size: fs::metadata(source_file).map(|m| m.len()).unwrap_or(0),
        transforms: std::collections::HashMap::new(),
    })
}

/// Merge transform entries into a source's record, preserving everything else
/// already in it — scan metadata, and outputs written by an earlier step of the
/// same conversion.
fn record_transforms(
    transforms: &crate::build::cache::TransformCache,
    source_oid: &str,
    source_file: &Path,
    entries: impl IntoIterator<Item = (String, crate::build::cache::TransformEntry)>,
) {
    let mut record = load_transform_record(transforms, source_oid, source_file);
    record.transforms.extend(entries);
    if let Err(e) = transforms.put(&record) {
        log::warn!("Failed to write transform record: {}", e);
    }
}

/// Record a freshly produced HLS ladder, replacing whatever `video/hls/` keys
/// the record already carries — a ladder's rungs are a SET, not entries that
/// only ever accumulate.
///
/// `record_transforms`' plain merge is wrong here specifically: `cached_ladder`
/// (hls.rs) infers a cached ladder's rung count by counting `video/hls/v*.m3u8`
/// keys in the record, so when a ladder SHRINKS — a tighter per-file budget or
/// a table edit dropping the top rung — a merge leaves the old top rung's
/// `video/hls/v5.*` keys behind. `cached_ladder` then counts one rung too many,
/// looks the extra one up under today's params, misses, and re-encodes the
/// whole ladder on every subsequent build forever, not just once. Dropping
/// every `video/hls/` key before inserting the fresh set is what keeps a
/// shrunk ladder a cache hit.
///
/// `pub(crate)`, not private: `hls_tests.rs` exercises this against
/// `hls::cached_ladder` directly to prove the shrink-stays-a-hit property,
/// rather than through a full `produce_ladder` encode.
pub(crate) fn record_ladder(
    transforms: &crate::build::cache::TransformCache,
    source_oid: &str,
    source_file: &Path,
    entries: impl IntoIterator<Item = (String, crate::build::cache::TransformEntry)>,
) {
    let mut record = load_transform_record(transforms, source_oid, source_file);
    record.transforms.retain(|k, _| !k.starts_with(crate::build::media::hls::HLS_TRANSFORM_PREFIX));
    record.transforms.extend(entries);
    if let Err(e) = transforms.put(&record) {
        log::warn!("Failed to write transform record: {}", e);
    }
}

/// Drop a source's `video/hls/` keys after `produce_ladder` returns `Ok(None)`
/// — the rung count collapsed below 2 (a tighter per-file budget, a shorter
/// `video_max_size_mb` from a deploy plugin, or a table edit), so no ladder is
/// owed this build, but a record from an EARLIER build may still carry the
/// old one's entries.
///
/// Left in place, those stale entries are wrong in two ways at once. The
/// vacuum pass (`cache.rs`) treats every oid a surviving record names as
/// reachable, so the stale blobs — including the very over-cap file
/// `record_ladder`'s budget exists to stop shipping — are never collected.
/// And `cached_ladder` (hls.rs) keeps counting the stale rungs on every later
/// build, misses looking the extra one up under today's params, and pays a
/// `probe_source` it would otherwise skip, forever — the same failure mode
/// `record_ladder` fixes for a SHRUNK ladder, here for a ladder that
/// disappeared entirely.
///
/// A no-op, with no record write, when the record has no `video/hls/` keys to
/// clear: most sources never had a ladder, this runs on every build's
/// `Ok(None)` arm, and a write nothing needs is still a write.
fn clear_stale_ladder(
    transforms: &crate::build::cache::TransformCache,
    source_oid: &str,
    source_file: &Path,
) {
    let Some(mut record) = transforms.get(source_oid) else {
        return;
    };
    if !record.transforms.keys().any(|k| k.starts_with(crate::build::media::hls::HLS_TRANSFORM_PREFIX)) {
        return;
    }
    record.transforms.retain(|k, _| !k.starts_with(crate::build::media::hls::HLS_TRANSFORM_PREFIX));
    if let Err(e) = transforms.put(&record) {
        log::warn!("Failed to clear stale HLS ladder keys for {}: {}", source_file.display(), e);
    }
}

/// Store one MP4 and link it into staging. Staging is the page's copy, so a
/// store or link that failed is a failed conversion, not a warning.
fn stage_mp4(
    objects: &crate::build::cache::ObjectStore,
    file: &Path,
    output_mp4: &Path,
) -> Result<String, String> {
    let oid = objects.store_file(file)?;
    objects.link_to(&oid, output_mp4)?;
    Ok(oid)
}

/// The ladder's slice of one video's progress bar. Measured on the ladder's own
/// fixture, it costs about 1.65x the two-pass MP4 wall, and it runs first.
const HLS_PROGRESS_SHARE: f64 = 0.6;

/// Within the MP4's own slice, `run_two_pass_encode` gives pass 1 — the
/// analysis pass — the first 0.3.
const MP4_ANALYSIS_SHARE: f64 = 0.3;

/// Outcome of converting a single video file.
///
/// This struct is the value carried through the singleflight dedup map so all
/// concurrent callers sharing a single conversion receive the full result —
/// including any error — without needing an out-of-band side channel.
///
/// Convention: `error.is_some()` means the conversion failed and the caller
/// (`run_video_conversion`) discards the whole outcome for the
/// `shipped_original` fallback, so `mp4_oid`/`thumb_oid`/`hls_entries` are
/// never read on that path and every early-return leaves them at their
/// zero value rather than bothering to thread a real one through.
///
/// Success carries the just-staged outputs' CAS oids: they were once on this
/// struct and removed because nothing read them (see git history) — ship-by-
/// OID (moss#867-adjacent) is the real reader this reintroduces them for, so
/// `run_video_conversion` can hand `ship_phase` an immutable blob instead of
/// the mutable stage path a concurrent build can rewrite.
#[derive(Clone, Debug)]
pub struct VideoConversionOutcome {
    /// Set when conversion failed. Carried inside the outcome so singleflight
    /// waiters receive the error message rather than an opaque sentinel.
    pub(crate) error: Option<String>,
    /// How many rungs the HLS ladder has, or 0 for no ladder. The caller
    /// registers the ladder's URLs from this — a ladder truncates from the top
    /// only, so its length names its files.
    pub(crate) hls_rungs: usize,
    /// The poster frame reached staging. Absent, it is not delivered: a poster
    /// is optional, a promised URL is not.
    pub(crate) poster: bool,
    /// The CAS oid backing the exact bytes just linked to the `.mp4`'s
    /// staging path — set on both the cache-hit and the freshly-encoded
    /// success path, `None` everywhere else (including every early-return,
    /// where it is never read).
    pub(crate) mp4_oid: Option<String>,
    /// The CAS oid backing the poster's staged bytes, `Some` only when
    /// `poster` is true — a store that succeeded but whose `link_to` failed
    /// leaves both `false`/`None`, since no bytes actually reached staging.
    pub(crate) thumb_oid: Option<String>,
    /// `(member filename, CAS oid)` for every file the HLS ladder staged this
    /// call, filename bare (e.g. `"v0.m3u8"`, no `video/hls/` prefix) so it
    /// matches `asset_paths::hls_members`' own naming. Populated whenever
    /// `produce_ladder` returns entries — cache hit or fresh encode alike,
    /// since that function's own cache check runs independently of the
    /// mp4/thumb one above it.
    pub(crate) hls_entries: Vec<(String, String)>,
}

/// The whole pipeline for one video: cache lookup, thumbnail, MP4, CAS store.
///
/// A cache hit validates the cached blob and links it to the output; a blob that
/// fails validation is evicted and re-converted. Either way the transform record
/// is written, so the next build hits the cache.
///
/// Output paths come from `relative_mp4` / `relative_thumb` — the caller's own
/// derivation, passed in rather than re-derived here, so there is one spelling
/// of where a video's outputs live. The optional callbacks are GUI-only
/// (progress, cancellation) and `None` headlessly.
pub(crate) fn convert_single_video(
    ffmpeg: &crate::build::media::ffmpeg::FFmpegManager,
    source_file: &Path,
    source_oid: &str,
    relative_mp4: &str,
    relative_thumb: &str,
    temp_dir: &Path,
    staging_dir: &Path,
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    compression_config: &crate::build::media::ffmpeg::VideoCompressionConfig,
    mp4_params: &serde_json::Value,
    thumb_params: &serde_json::Value,
    // GUI-specific, None for headless:
    registry: Option<&crate::types::runtime::ChildProcessRegistry>,
    cancel_flag: Option<&std::sync::atomic::AtomicBool>,
    progress_callback: Option<&dyn Fn(f64)>,
) -> VideoConversionOutcome {
    use crate::build::cache::TransformEntry;

    let filename = Path::new(relative_mp4)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(relative_mp4);

    let output_mp4 = staging_dir.join(relative_mp4);
    let output_thumb = staging_dir.join(relative_thumb);

    // Two encodes share one progress bar, so each reports a slice of it, in
    // the order they actually run: the ladder first, then the MP4.
    let hls_progress = progress_callback.map(|cb| move |f: f64| cb(f * HLS_PROGRESS_SHARE));
    let mp4_progress = progress_callback
        .map(|cb| move |f: f64| cb(HLS_PROGRESS_SHARE + f * (1.0 - HLS_PROGRESS_SHARE)));

    // The adaptive ladder, beside the progressive MP4 rather than instead of
    // it, and BEFORE the MP4's own cache check: `produce_ladder` caches itself,
    // and a video whose MP4 is a cache hit still needs its ladder linked into
    // staging. A failure here is not a failed conversion — the MP4 is still the
    // page's video and the emitter simply has no ladder to offer.
    let mut hls_rungs = 0usize;
    // (bare member filename, CAS oid) for every ladder file `produce_ladder`
    // just linked — same census `entries` carries, minus the `video/hls/`
    // transform-cache prefix, so it lines up with `asset_paths::hls_members`'
    // own naming for the caller that builds `Delivery`s from it.
    let mut hls_entries: Vec<(String, String)> = Vec::new();
    match crate::build::media::hls::produce_ladder(
        ffmpeg,
        source_file,
        source_oid,
        &staging_dir.join(moss_core::asset_paths::to_hls_dir(relative_mp4)),
        temp_dir,
        transforms,
        compression_config,
        hls_progress.as_ref().map(|f| f as &dyn Fn(f64)),
        registry,
        cancel_flag,
    ) {
        Ok(Some(entries)) => {
            // One playlist per rung, plus one segment each — see `hls_members`.
            hls_rungs = entries
                .iter()
                .filter(|(n, _)| n.starts_with("video/hls/v") && n.ends_with(".m3u8"))
                .count();
            hls_entries = entries
                .iter()
                .filter_map(|(name, entry)| {
                    name.strip_prefix(crate::build::media::hls::HLS_TRANSFORM_PREFIX)
                        .map(|bare| (bare.to_string(), entry.oid.clone()))
                })
                .collect();
            // Recorded here rather than with the MP4's own outputs: four
            // early returns sit between this point and that write, and a
            // ladder that reached the object store without reaching the record
            // is seventeen blobs re-encoded on every build. `record_ladder`,
            // not `record_transforms`: see its own doc for why a plain merge
            // would strand a shrunk ladder's dropped rung keys.
            record_ladder(transforms, source_oid, source_file, entries);
        }
        Ok(None) => {
            log::debug!("No HLS ladder for {}: source fills one rung", filename);
            // A record from an earlier build may still carry a ladder this
            // one no longer owes — see `clear_stale_ladder`'s own doc for
            // what leaving it behind costs.
            clear_stale_ladder(transforms, source_oid, source_file);
        }
        Err(ref e) if e == "Cancelled" => {
            return VideoConversionOutcome {
                error: Some("Cancelled".to_string()),
                hls_rungs: 0,
                poster: false,
                mp4_oid: None,
                thumb_oid: None,
                hls_entries: Vec::new(),
            };
        }
        Err(e) => log::warn!("HLS ladder failed for {} (MP4 still served): {}", filename, e),
    }

    // Step 1: Check transform cache for both MP4 and thumbnail
    let cached_mp4 = transforms.find_cached_output(source_oid, "video/mp4", mp4_params);
    let cached_thumb = transforms.find_cached_output(source_oid, "video/thumbnail", thumb_params);

    // Step 2: On cache hit, validate and link. Both outputs are required for a
    // hit — the worker owns the video's bytes as well as its poster, so a
    // cached thumbnail alone leaves the page with no video to play.
    if let (Some(ref mp4_oid), Some(ref thumb_oid)) = (&cached_mp4, &cached_thumb) {
        // Self-healing: validate cached MP4 blob via ffprobe
        let mut mp4_valid = false;
        if let Some(blob_path) = objects.get_path(mp4_oid) {
            if ffmpeg.validate_encoded_video(&blob_path).unwrap_or(false) {
                mp4_valid = true;
            } else {
                log::warn!("Cached video failed validation, evicting: {}", mp4_oid);
                // Evict the transform record so re-conversion happens.
                // The corrupt blob itself will be self-healed by store_file()
                // via validate_blob() when the re-converted output is stored.
                transforms.remove(source_oid).ok();
                // Fall through to cache-miss path below
            }
        }

        if mp4_valid {
            log::debug!("Cache hit for {} (content-addressed)", filename);

            // Link thumbnail to staging. Optional: a failed link just means no
            // poster ships, tracked below so the caller doesn't promise a URL
            // for bytes that never landed.
            let thumb_linked = match objects.link_to(thumb_oid, &output_thumb) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("Failed to link cached thumbnail to output: {}", e);
                    false
                }
            };

            // The cached MP4 is a hit only once it is in staging, the page's copy.
            if let Err(e) = objects.link_to(mp4_oid, &output_mp4) {
                return VideoConversionOutcome {
                    error: Some(format!("could not stage {}: {}", filename, e)),
                    hls_rungs,
                    poster: false,
                    mp4_oid: None,
                    thumb_oid: None,
                    hls_entries: Vec::new(),
                };
            }
            return VideoConversionOutcome {
                error: None,
                hls_rungs,
                poster: thumb_linked,
                mp4_oid: Some(mp4_oid.clone()),
                // Only when the bytes actually reached staging — a store hit
                // whose link failed ships nothing at this URL, so no oid
                // should claim to back it either.
                thumb_oid: thumb_linked.then(|| thumb_oid.clone()),
                hls_entries,
            };
        }
    }

    // Step 3: Cache miss -- run ffmpeg pipeline
    log::info!("Converting video: {}", filename);

    let mut thumb_oid_result: Option<String> = None;
    // True only once the thumbnail is both stored AND linked into staging —
    // `thumb_oid_result` alone would count a store whose link then failed,
    // leaving nothing at the URL the caller is about to promise.
    let mut thumb_linked = false;

    // 3a. Thumbnail FIRST (preserve fast preview swap -- 2s vs 30s)
    let temp_thumb = temp_dir.join(format!("{}-{}.thumb.jpg", filename, uuid::Uuid::new_v4()));
    log::debug!("Generating thumbnail first for fast preview: {}", filename);
    match ffmpeg.generate_thumbnail(
        source_file,
        &temp_thumb,
        registry,
        cancel_flag,
    ) {
        Ok(true) => {
            // Validate thumbnail: reject 0-byte files
            if fs::metadata(&temp_thumb).map(|m| m.len()).unwrap_or(0) == 0 {
                log::warn!("Thumbnail is 0 bytes, skipping CAS storage: {}", filename);
                // allow:unlink an encode temp this call wrote under cache/tmp
                let _ = fs::remove_file(&temp_thumb);
            } else {
                log::debug!("Generated thumbnail for {}", filename);
                match objects.store_file(&temp_thumb) {
                    Ok(oid) => {
                        match objects.link_to(&oid, &output_thumb) {
                            Ok(()) => thumb_linked = true,
                            Err(e) => log::warn!("Failed to link thumbnail to output: {}", e),
                        }
                        thumb_oid_result = Some(oid);
                    }
                    Err(e) => log::warn!("Failed to store thumbnail in CAS: {}", e),
                }
                // allow:unlink an encode temp this call wrote under cache/tmp
                let _ = fs::remove_file(&temp_thumb);
            }
        }
        Ok(false) => {}
        Err(ref e) if e == "Cancelled" => {
            // allow:unlink an encode temp this call wrote under cache/tmp
            let _ = fs::remove_file(&temp_thumb);
            return VideoConversionOutcome {
                error: Some("Cancelled".to_string()),
                hls_rungs,
                poster: false,
                mp4_oid: None,
                thumb_oid: None,
                hls_entries: Vec::new(),
            };
        }
        Err(e) => log::warn!("Thumbnail generation failed for {}: {}", filename, e),
    }

    // 3b. The video's bytes. Every video passes through here — `plan_video_encode`
    // decides inside `convert_to_mp4_with_config` whether that means a re-encode
    // or the source bytes unchanged, and either way the output is this worker's.
    let temp_mp4 = temp_dir.join(format!("{}-{}.mp4", filename, uuid::Uuid::new_v4()));
    let conversion_result = ffmpeg.convert_to_mp4_with_config(
        source_file,
        &temp_mp4,
        compression_config,
        mp4_progress.as_ref().map(|f| f as &dyn Fn(f64)),
        registry,
        cancel_flag,
    );

    let mut kept_original = false;
    match conversion_result {
        Ok(crate::build::media::ffmpeg::EncodingResult::TwoPassSuccess { size_bytes }) => {
            let size_mb = size_bytes as f64 / 1024.0 / 1024.0;
            log::info!("Converted with two-pass ({:.1} MB): {}", size_mb, filename);
        }
        Ok(crate::build::media::ffmpeg::EncodingResult::KeptOriginal) => {
            kept_original = true;
        }
        Err(ref e) if e == "Cancelled" => {
            // allow:unlink an encode temp this call wrote under cache/tmp
            let _ = fs::remove_file(&temp_mp4);
            return VideoConversionOutcome {
                error: Some("Cancelled".to_string()),
                hls_rungs,
                poster: false,
                mp4_oid: None,
                thumb_oid: None,
                hls_entries: Vec::new(),
            };
        }
        Err(e) => {
            // allow:unlink an encode temp this call wrote under cache/tmp
            let _ = fs::remove_file(&temp_mp4);
            return VideoConversionOutcome {
                error: Some(e),
                hls_rungs,
                poster: false,
                mp4_oid: None,
                thumb_oid: None,
                hls_entries: Vec::new(),
            };
        }
    }

    let staged = if kept_original {
        // Unachievable size target on an already-web-ready source (long
        // film): store the ORIGINAL as its own transform output. The
        // seal/registry flow and future cache hits are then identical to
        // a real encode — rebuilds link the original without re-probing.
        stage_mp4(objects, source_file, &output_mp4)
    } else {
        // 3c. Validate converted MP4 via ffprobe before storing in CAS
        if !ffmpeg.validate_encoded_video(&temp_mp4).unwrap_or(false) {
            log::warn!("Converted video failed validation, not storing: {}", filename);
            // allow:unlink an encode temp this call wrote under cache/tmp
            let _ = fs::remove_file(&temp_mp4);
            return VideoConversionOutcome {
                error: Some(format!("Validation failed for {}", filename)),
                hls_rungs,
                poster: false,
                mp4_oid: None,
                thumb_oid: None,
                hls_entries: Vec::new(),
            };
        }

        stage_mp4(objects, &temp_mp4, &output_mp4)
    };
    // allow:unlink an encode temp this call wrote under cache/tmp
    let _ = fs::remove_file(&temp_mp4);
    let mp4_oid_result = match staged {
        Ok(oid) => Some(oid),
        Err(e) => {
            return VideoConversionOutcome {
                error: Some(format!("could not stage {}: {}", filename, e)),
                hls_rungs,
                poster: false,
                mp4_oid: None,
                thumb_oid: None,
                hls_entries: Vec::new(),
            }
        }
    };

    // Step 4: Write transform record (if we have at least one output)
    let stored_entry = |oid: &String, params: &serde_json::Value| TransformEntry {
        oid: oid.clone(),
        size: objects
            .get_path(oid)
            .and_then(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0),
        params: params.clone(),
    };
    let entries: Vec<_> = [
        mp4_oid_result
            .as_ref()
            .map(|oid| ("video/mp4".to_string(), stored_entry(oid, mp4_params))),
        thumb_oid_result
            .as_ref()
            .map(|oid| ("video/thumbnail".to_string(), stored_entry(oid, thumb_params))),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !entries.is_empty() {
        record_transforms(transforms, source_oid, source_file, entries);
    }

    VideoConversionOutcome {
        error: None,
        hls_rungs,
        poster: thumb_linked,
        mp4_oid: mp4_oid_result,
        // As above: a stored-but-unlinked thumbnail has no bytes at the URL
        // `poster`/`thumb_linked` says nothing shipped for, so it gets no oid.
        thumb_oid: if thumb_linked { thumb_oid_result } else { None },
        hls_entries,
    }
}

/// Resolve the content hash for a VIDEO source file, using HashIndex as a cache.
///
/// Matches by (size, whole-second mtime) only — the rule video keeps on purpose, see
/// [`HashIndex::lookup_whole_second`]; images take [`HashIndex::resolve`]. On a miss,
/// hashes the file via `ObjectStore::hash_file()` and updates the index.
pub(crate) fn resolve_source_hash(
    source_file: &Path,
    relative_path: &str,
    hash_index: &mut crate::build::cache::HashIndex,
) -> Result<String, String> {
    use crate::build::cache::ObjectStore;

    let meta = fs::metadata(source_file)
        .map_err(|e| format!("Failed to stat {}: {}", source_file.display(), e))?;
    let size = meta.len();
    let mtime = meta.modified()
        .map_err(|e| format!("Failed to get mtime for {}: {}", source_file.display(), e))?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Check HashIndex cache: if (size, mtime) match, use cached hash
    if let Some(cached_hash) = hash_index.lookup_whole_second(relative_path, size, mtime) {
        return Ok(cached_hash.to_string());
    }

    // Cache miss: hash the file and update index
    let hash = ObjectStore::hash_file(source_file)?;
    hash_index.update_whole_second(relative_path.to_string(), size, mtime, hash.clone());
    Ok(hash)
}

// ---------------------------------------------------------------------------
// What happened to one video
// ---------------------------------------------------------------------------

/// What the rest of the build calls a file the video worker put on disk: the
/// `.mp4` and the HLS ladder's members are `Video`, the poster frame `Poster`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeliveryKind {
    Video,
    Poster,
}

impl DeliveryKind {
    /// The `AssetReady` asset_type the preview's iframe bridge swaps on.
    fn asset_type(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Poster => "thumbnail",
        }
    }

    /// Whether the asset registry tracks this URL. Videos yes: `blocking.rs`
    /// promises the `.mp4` with `set_pending`, and the ladder's rungs — whose
    /// count is known only after the encode, so never promised at render time —
    /// are registered late by the `set_ready` here. The poster is tracked by
    /// neither; marking it ready would invent an entry nobody looks up (ADR-013).
    fn registry_tracked(self) -> bool {
        matches!(self, Self::Video)
    }
}

/// A URL whose bytes are on disk under `staging_dir`.
struct Delivery {
    url: String,
    kind: DeliveryKind,
    /// The CAS oid backing these exact bytes, when the caller already knows
    /// one — `None` for the raw fallback copy (`shipped_original`, no CAS
    /// interaction at all) and for a `Delivery` this file cannot yet name an
    /// oid for. Threaded through to `EmitMessage::File.oid` so `ship_phase`
    /// can read the immutable blob instead of the mutable stage path.
    oid: Option<String>,
}

/// What the conversion loop decided about one video.
///
/// Every arm of the per-item body returns one of these instead of running its
/// own teardown, so the census push, the `AssetReady` emit, the registry
/// `set_ready` and the UiBound release happen in exactly ONE place and a new
/// exit arm cannot skip a step. The eight `end_ui_bound(); continue;` arms
/// this replaces already had: all three fallback arms forgot the census push,
/// so `remove_stale_files` deleted the bytes they had just staged.
enum ItemStep {
    /// Handled. `delivered` names every URL now on disk (empty when nothing
    /// shipped) and `advisories` is what to tell the user about this video.
    /// `encoded` is true only when this run actually produced the video's
    /// bytes — it gates the media child Job (FIX 1b).
    Handled {
        delivered: Vec<Delivery>,
        advisories: Vec<Advisory>,
        encoded: bool,
    },
    /// A newer dispatch owns this vault now. Release the remaining budget and
    /// leave `videos_converted` alone: the receipt counter belongs to the fresh
    /// epoch, and storing ours would clobber a count that is still growing.
    Superseded,
    /// The user cancelled. Release the remaining budget and publish the count
    /// reached so far.
    Cancelled,
}

impl ItemStep {
    /// Ship the source bytes unchanged at the video's own output URL, with the
    /// advisory explaining why, if there is one to give.
    ///
    /// The one owner of the fallback that three arms used to spell out
    /// separately (no FFmpeg, the iCloud wait timing out, a failed encode).
    /// Each copy wrote the original to the SOURCE path — `videos/clip.mov` —
    /// while announcing `to_mp4`'s `videos/clip.mp4`, so a published site
    /// referenced a file that existed nowhere; preview hid it, because
    /// `set_source_passthrough` serves the original at the `.mp4` URL there.
    /// A failed copy delivers nothing, rather than promising a URL for bytes
    /// that never landed — and says so in an advisory of its own. `encoded` is
    /// false by construction: no new bytes means no media child Job (FIX 1b).
    ///
    /// A failed copy also retracts the `.mp4` promise via `set_failed`
    /// (2026-09-16 thermo review of the promise gate). Without it the key
    /// registered by `blocking.rs`'s `set_pending` stayed `Pending` forever —
    /// a rebuild hits the same copy failure every time — and the publish
    /// promise gate this arm feeds (`link_audit::dead_links_among_promises`)
    /// has no override, so the folder's publish would be blocked
    /// indefinitely. `services` is only needed for this registry write; every
    /// other outcome here (delivered bytes, or none at all) still flows
    /// through the caller's one `record_deliveries` call.
    fn shipped_original(
        services: &BuildServices,
        source_file: &Path,
        staging_dir: &Path,
        mapped_source: &str,
        item: &str,
        advisory: Option<Advisory>,
    ) -> Self {
        let url = moss_core::asset_paths::to_mp4(mapped_source);
        let mut advisories: Vec<Advisory> = advisory.into_iter().collect();
        let delivered = match crate::build::media::ffmpeg::copy_video_as_fallback(
            source_file,
            &staging_dir.join(&url),
        ) {
            // A raw fs copy of the source, not a CAS write — no oid to offer.
            Ok(()) => vec![Delivery { url, kind: DeliveryKind::Video, oid: None }],
            Err(e) => {
                log::warn!("Fallback copy failed for {}: {}", mapped_source, e);
                // The caller's advisory says why moss could not OPTIMIZE the
                // video; only this arm knows the worse fact, that the page has
                // no video at all. `item`, not `mapped_source`: the advisory
                // names the SOURCE file, and a `url:` override can make those
                // two paths diverge (mapped_source is the OUTPUT's page-tree
                // path, which need not exist anywhere on disk).
                advisories.push(Advisory::blocking_file(
                    item,
                    crate::infra::app_advisory::fmt("video_not_published", &[("err", &e)]),
                ));
                // Terminal: this key leaves `pending_keys()` so the promise
                // gate stops refusing this folder's publish over a video that
                // will never arrive (a permanently failed encode already
                // takes this same exit through `set_failed`).
                if let Some(ref registry) = services.assets {
                    registry.set_failed(url, e);
                }
                Vec::new()
            }
        };
        Self::Handled { delivered, advisories, encoded: false }
    }

    /// Nothing shipped, but the user should hear why.
    fn advise(advisory: Advisory) -> Self {
        Self::Handled { delivered: Vec::new(), advisories: vec![advisory], encoded: false }
    }
}

/// Which ending an abandoned item had. The waits inside the item body share
/// one predicate for "stop"; only the cancel flag tells a cancel from a newer
/// epoch. Naming it here is what stops the LAST item's abandonment from being
/// reported as a completed run — there is no next iteration to notice it.
fn abandonment(cancelled: bool) -> ItemStep {
    if cancelled {
        ItemStep::Cancelled
    } else {
        ItemStep::Superseded
    }
}

/// Record one item's deliveries: the census the manifest reads, the live
/// `AssetReady` the preview swaps on, and the registry promise the URL was made
/// under. The census is why this is one function — bytes staged without being
/// registered here fall outside `video_outputs`, and `remove_stale_files`
/// deletes anything in the output tree that no bucket claims.
fn record_deliveries(
    services: &BuildServices,
    produced: &mut Vec<(String, Option<String>)>,
    delivered: Vec<Delivery>,
) {
    for Delivery { url, kind, oid } in delivered {
        produced.push((url.clone(), oid));
        services.reporter.report(&PipelineEvent::AssetReady {
            path: url.clone(),
            asset_type: kind.asset_type().to_string(),
        });
        if kind.registry_tracked() {
            if let Some(ref registry) = services.assets {
                registry.set_ready(url);
            }
        }
    }
}

/// The run's one completion tail: register the census, print the CLI summary,
/// mint the media child Job, emit the completion event and publish the receipt
/// count.
///
/// Reached by every path that finishes the run, the FFmpeg-unavailable exit
/// included — that exit used to carry its own copy of four of these five steps
/// and skip the census, leaving the originals it copied unclaimed by any
/// manifest bucket. `converted_count` gates the media Job — videos this run actually re-encoded.
/// `videos_processed` is the terminal receipt — videos the encoder reached at
/// all, which is every one of them unless FFmpeg was missing entirely.
fn finish_run(
    services: &BuildServices,
    ctx: &BackgroundContext,
    tx: &Option<mpsc::Sender<EmitMessage>>,
    produced_video_paths: &[(String, Option<String>)],
    advisories: Vec<Advisory>,
    converted_count: u32,
    videos_processed: u32,
) {
    let total = ctx.video_items.len() as u32;
    emit_video_outputs_via_channel(tx, produced_video_paths, &ctx.staging_dir);

    // CLI stderr summary (headless only)
    if services.reporter.is_terminal() {
        if advisories.is_empty() {
            eprintln!("Video conversion complete ({} videos)", total);
        } else {
            eprintln!("Video conversion complete ({} videos, {} advisories)", total, advisories.len());
            for a in &advisories {
                match &a.item {
                    Some(f) => eprintln!("  Advisory: {}: {}", f, a.what),
                    None => eprintln!("  Advisory: {}", a.what),
                }
            }
        }
    }

    // Dual-emit (Step 3 Phase 4): legacy BackgroundProgress + a media child Job
    // under the Build parent. Gated on REAL work (FIX 1b): a content-only rebuild
    // cache-skips every video → `converted_count == 0` + no advisories → no Job.
    spawn_media_child_job(services, "videos", converted_count, advisories.clone());
    // Final completion (discarded on a headless build)
    services.reporter.report(&PipelineEvent::BackgroundProgress {
        task: "videos".to_string(),
        current: total,
        total,
        message: format_progress_message("videos", total, total),
        completed: true,
        advisories,
    });

    // Publish the count for the terminal receipt. Emitting it from here is what
    // this module used to do, and what `BuildTerminalBarrier` now owns — see
    // that type's doc for why a worker that may not run cannot own a receipt.
    services
        .videos_converted
        .store(videos_processed, std::sync::atomic::Ordering::Relaxed);
}

/// How one conversion run ended: how many items put bytes on disk, and how many
/// were left behind when a newer dispatch or a cancel stopped it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RunEnd {
    pub delivered: usize,
    pub abandoned: usize,
}

impl RunEnd {
    /// `video run #<e> ended: <d> delivered, <a> abandoned — rebuild requested: yes|no`
    pub(crate) fn log(self, epoch: u64, rebuild_requested: bool) {
        log::info!(
            "video run #{} ended: {} delivered, {} abandoned — rebuild requested: {}",
            epoch,
            self.delivered,
            self.abandoned,
            if rebuild_requested { "yes" } else { "no" }
        );
    }
}

/// Video conversion for both GUI and headless modes — one synchronous function,
/// run inside `spawn_blocking`.
///
/// `epoch` selects the mode. GUI passes a live epoch and the loop exits when a
/// newer dispatch bumps it; headless passes 0, so the staleness check `0 != 0`
/// is never true. Every other GUI/headless difference is an `Option` on
/// `BuildServices` that is `None` headlessly — progress events and `AssetReady`
/// are discarded, the cancel flag stays false, the dedup set is empty. Only the
/// stderr lines are gated explicitly, on `reporter.is_terminal()`.
///
/// The loop body decides and returns an [`ItemStep`]; the loop applies it.
/// Nothing inside the body releases a UiBound permit, pushes to the census or
/// emits an `AssetReady` of its own — that is what keeps the three from ever
/// getting out of step.
pub(crate) fn run_video_conversion(
    services: &BuildServices,
    ctx: &BackgroundContext,
    epoch: u64,
    tx: Option<mpsc::Sender<EmitMessage>>,
) -> RunEnd {
    use crate::build::media::ffmpeg::FFmpegManager;
    // Uses centralized asset_paths for consistent path derivation
    use moss_core::asset_paths;
    use crate::build::render::resolve_path_with_overrides;

    let is_headless = services.reporter.is_terminal();

    // Extract dir_overrides for file-tree → page-tree mapping.
    // All output paths (staging, canonical, AssetReady events, registry keys)
    // must use mapped paths so they match HTML references.
    let dir_overrides = &ctx.dir_overrides;
    let total = ctx.video_items.len() as u32;
    if total == 0 {
        return RunEnd::default();
    }
    let mut end = RunEnd::default();
    // Every exit, early returns included, takes this run's items out of
    // `running`, so a later dispatch never joins a run that is gone.
    struct RunOwnership<'a>(&'a crate::types::runtime::VideoConversionState, u64);
    impl Drop for RunOwnership<'_> {
        fn drop(&mut self) {
            self.0.end_run(self.1);
        }
    }
    let _ownership = RunOwnership(&services.cancellation, epoch);

    let mut advisories: Vec<Advisory> = vec![];
    // Count of videos this run ACTUALLY converted (ran FFmpeg / produced a fresh
    // mp4). Cache-skipped videos do NOT increment it — so a content-only rebuild
    // on a video site ends with `converted_count == 0`, which (with zero advisories)
    // gates out the media child + parent Jobs (FIX 1b, invariant #6).
    let mut converted_count: u32 = 0;
    // Every output path this run put on disk, paired with the CAS oid backing
    // it when known — the census `video_outputs` is built from. Written only
    // by `record_deliveries`.
    let mut produced_video_paths: Vec<(String, Option<String>)> = Vec::new();

    // Bridged from `FolderSession::cancel` to an AtomicBool, the signature the
    // ffmpeg progress callback takes. Headless (no session): false forever.
    let local_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(s) = services.session.as_ref() {
        s.bridge_to_atomic(local_cancel.clone());
    }
    let is_cancelled = || local_cancel.load(Ordering::SeqCst);

    // CLI stderr output (headless only)
    if is_headless {
        eprintln!("Converting {} video(s)...", total);
        log::info!("Starting headless video conversion for {} video(s)", total);
    }

    // Initial progress toast (discarded on a headless build)
    services.reporter.report(&PipelineEvent::BackgroundProgress {
        task: "videos".to_string(),
        current: 0,
        total,
        message: format_progress_message("videos", 0, total),
        completed: false,
        advisories: vec![],
    });

    // FFmpeg, or the reason there is none — reusing the path from the scan when
    // it has one (avoids a duplicate download). Missing is not fatal and is not
    // a separate run: it becomes one advisory plus a per-item outcome, so the
    // originals it ships reach the census through the same owner as every other
    // delivery. They did not, before, and the stale sweep deleted them.
    let ffmpeg = match ctx.ffmpeg_bin_path {
        Some(ref path) => Some(FFmpegManager::from_bin_path(path.clone())),
        None => match FFmpegManager::get_or_download(None) {
            Ok(f) => Some(f),
            Err(e) => {
                log::warn!("Video conversion skipped: FFmpeg not available: {}", e);
                advisories.push(Advisory {
                    scope: Scope::Environment,
                    severity: Severity::NeedsAction,
                    item: None,
                    what: crate::infra::app_advisory::fmt(
                        "shipped_unoptimized",
                        &[("err", &e.to_string())],
                    ),
                    action: Action::Command {
                        run: "brew install ffmpeg".into(),
                        label: crate::infra::app_advisory::t("copy"),
                    },
                });
                None
            }
        },
    };

    let project_root = Path::new(&ctx.source_path);

    use crate::build::media::ffmpeg::VideoCompressionConfig;
    let compression_config = match services.deploy_video_max_size_mb {
        // The plugin host's own per-file limit, so it bounds BOTH files this
        // worker can produce for one video: the progressive MP4 directly, and
        // every HLS rung/audio file `hls_max_file_mb` gates — a host that caps
        // uploads at `mb` rejects an oversized ladder rung exactly as it would
        // an oversized MP4.
        Some(mb) => VideoCompressionConfig { max_size_mb: mb, hls_max_file_mb: mb, ..Default::default() },
        None => VideoCompressionConfig::default(),
    };

    use crate::build::cache::{HashIndex, ObjectStore};
    let vid_paths = MossPaths::from_moss_dir(ctx.moss_dir.clone());
    let objects = ObjectStore::for_site(&vid_paths);
    let transforms = crate::build::cache::TransformCache::for_site(&vid_paths);
    let mp4_params = compression_config.to_params();
    let thumb_params = serde_json::json!({});

    // Load HashIndex for fast hash lookups (avoids re-reading large video files)
    let hash_index_path = vid_paths.cache_hash_index();
    let mut hash_index = HashIndex::load(&hash_index_path);

    // This run's own scratch: its temps and two-pass logs, never another run's
    // or an image batch's. Removed when the run returns.
    let scratch = crate::build::io_utils::ScratchDir::new(&vid_paths.cache_tmp(), &format!("video-{epoch}"));
    let temp_dir = scratch.path().to_path_buf();

    for (index, item) in ctx.video_items.iter().enumerate() {
        let current = (index + 1) as u32;
        let filename = Path::new(&item)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "video".to_string());

        // Map source path through dir_overrides for all OUTPUT operations.
        // source_file below still uses the raw filesystem path for READING.
        let source_file = project_root.join(&item);
        let mapped_source = resolve_path_with_overrides(&item, &dir_overrides);

        // The whole of one video's decision. Every exit is a value, never a
        // `continue` with teardown attached — see `ItemStep`.
        let step: ItemStep = 'item: {
            // Epoch staleness check: if a newer dispatch has started, this task is
            // obsolete. This prevents multiple concurrent FFmpeg processes from
            // piling up when rebuilds trigger during video conversion (common on
            // iCloud Drive folders). In headless mode: epoch=0, current_id()=0, so
            // (0 != 0) is false — no-op.
            let current_epoch = services.cancellation.current_id();
            if current_epoch != epoch {
                log::info!(
                    "[epoch {}] Superseded by epoch {} at video {}/{}, exiting",
                    epoch, current_epoch, current, total
                );
                break 'item ItemStep::Superseded;
            }

            // Check cancellation before each video.
            // In headless mode: cancel_flag stays false — this never triggers.
            if is_cancelled() {
                log::info!("[epoch {}] Video conversion cancelled at {}/{}", epoch, current, total);
                break 'item ItemStep::Cancelled;
            }

            // Update progress toast (discarded on a headless build)
            services.reporter.report(&PipelineEvent::BackgroundProgress {
                task: "videos".to_string(),
                current,
                total,
                message: format_progress_message("videos", current, total),
                completed: false,
                advisories: vec![],
            });

            if !source_file.exists() {
                log::warn!("Video not found: {}", item);
                break 'item ItemStep::advise(Advisory::for_source(
                    Scope::File,
                    Severity::NeedsAction,
                    item,
                    crate::infra::app_advisory::t("not_found"),
                    Action::None,
                ));
            }

            // No encoder: the original bytes ARE the deliverable. Decided here,
            // inside the item body, so this ships through the same owner, census
            // and budget as every other outcome — and so a superseded or
            // cancelled run still stops instead of copying the whole vault.
            let Some(ffmpeg) = ffmpeg.as_ref() else {
                // No advisory: this one is the environment's, pushed once above.
                break 'item ItemStep::shipped_original(
                    services,
                    &source_file,
                    &ctx.staging_dir,
                    &mapped_source,
                    item,
                    None,
                );
            };

            // iCloud readiness gate: large .mov sources in iCloud Drive can be dataless
            // placeholders or mid-resync even when "Keep Downloaded" (eviction is
            // prevented; the re-sync/download window is not). Non-faststart QuickTime
            // keeps `moov` at EOF, so ffprobe fails with "moov atom not found" until the
            // tail is present. Materialize first (on this background worker, never the
            // build/UI thread) instead of reporting a transient sync gap as a hard
            // failure. No-op for already-resident files (~two lstats) and off macOS.
            // Placed after the fast-path skip so cached-output files never wait.
            use crate::build::cloud_readiness::{await_ready, Settled, MATERIALIZE_DEADLINE, POLL_INTERVAL};
            let announced = || {
                services.reporter.report(&PipelineEvent::BackgroundProgress {
                    task: "videos".to_string(),
                    current,
                    total,
                    message: crate::infra::app_advisory::fmt("downloading_from_icloud", &[("name", &filename)]),
                    completed: false,
                    advisories: vec![],
                });
                if is_headless {
                    eprintln!("  [{}/{}] Downloading from iCloud: {}", current, total, filename);
                }
            };
            let superseded_or_cancelled =
                || services.cancellation.current_id() != epoch || is_cancelled();
            match await_ready(
                &source_file,
                MATERIALIZE_DEADLINE,
                POLL_INTERVAL,
                &superseded_or_cancelled,
                &announced,
            ) {
                Settled::Ready => { /* fall through to conversion */ }
                Settled::Cancelled => {
                    log::info!("[epoch {}] iCloud wait cancelled for {}", epoch, filename);
                    break 'item abandonment(is_cancelled());
                }
                Settled::TimedOut => {
                    // Still not materialized after the deadline. Do NOT run ffprobe
                    // (that would surface "moov atom not found"). Surface a soft, honest
                    // advisory and ship the original bytes.
                    log::warn!(
                        "[epoch {}] iCloud download did not complete for {} within {:?}",
                        epoch, filename, MATERIALIZE_DEADLINE
                    );
                    break 'item ItemStep::shipped_original(
                        services,
                        &source_file,
                        &ctx.staging_dir,
                        &mapped_source,
                        item,
                        Some(Advisory::for_source(
                            Scope::File,
                            // Transient and self-resolving (no user action): the original
                            // ships unoptimized and the next preview re-optimizes it. Use
                            // the calm "shipped, not optimal" tier, not NeedsAction — this
                            // is a soft notice, not a failure.
                            Severity::ShippedDegraded,
                            item,
                            crate::infra::app_advisory::t("still_downloading_icloud"),
                            Action::None,
                        )),
                    );
                }
            }

            // Hash source file (uses HashIndex cache to avoid re-reading large files)
            let source_oid = match resolve_source_hash(&source_file, &item, &mut hash_index) {
                Ok(oid) => oid,
                Err(e) => {
                    // The read failed, so there is nothing to encode — but the bytes
                    // may still be copyable (a stat/mtime failure is not a read
                    // failure), and shipping them is what every other can't-encode
                    // arm does. Before the outcome model this arm alone shipped
                    // nothing, leaving the page with no video at all.
                    log::warn!("Failed to hash video {}: {}", filename, e);
                    break 'item ItemStep::shipped_original(
                        services,
                        &source_file,
                        &ctx.staging_dir,
                        &mapped_source,
                        item,
                        Some(Advisory::for_source(
                            Scope::File,
                            Severity::NeedsAction,
                            item,
                            crate::infra::app_advisory::t("could_not_read_file"),
                            Action::None,
                        )),
                    );
                }
            };

            // Bound concurrent encodes across dispatch epochs. LOCK ORDER (global,
            // never reversed): iCloud gate -> encode permit -> singleflight -> ffmpeg.
            // - Acquired AFTER await_ready: a 90s iCloud stall must not hold a slot.
            // - Acquired BEFORE do_work: a cancelled wait must not flow through
            //   VideoConversionOutcome.error (that path ships a fallback copy).
            // - A singleflight waiter holds a permit while blocked: bounded waste,
            //   no cycle — the leader took its permit before entering do_work and
            //   never takes another.
            let encode_permit = match acquire_encode_permit(
                &VIDEO_ENCODE_SEMAPHORE,
                PERMIT_POLL,
                &superseded_or_cancelled,
            ) {
                Some(p) => p,
                None => {
                    log::info!("[epoch {}] encode-slot wait cancelled for {}", epoch, filename);
                    break 'item abandonment(is_cancelled());
                }
            };

            let relative_mp4 = asset_paths::to_mp4(&mapped_source);
            let relative_thumb = asset_paths::to_thumb(&mapped_source);
            let output_mp4 = ctx.staging_dir.join(&relative_mp4);

            // Singleflight dedup: if another task is already converting this source_oid,
            // block until it finishes and reuse its result. Only the first caller runs
            // FFmpeg — all others block until the first completes.
            //
            // The error is carried inside VideoConversionOutcome.error so all waiters
            // (shared callers) receive the full error message, not just a silent sentinel.
            let (sf_result, was_shared) = {
                services.in_flight_videos.do_work(&source_oid, || {
                    // Build progress callback closure for fine-grained encoding updates.
                    // Fires in both modes; a headless build's reporter discards.
                    let reporter = services.reporter.as_ref();
                    let progress_callback = |fraction: f64| {
                        // Only the MP4's first pass analyses; everything before it
                        // is the ladder encoding, and everything after is pass 2.
                        let analysis_end =
                            HLS_PROGRESS_SHARE + (1.0 - HLS_PROGRESS_SHARE) * MP4_ANALYSIS_SHARE;
                        let phase = if (HLS_PROGRESS_SHARE..analysis_end).contains(&fraction) {
                            "analyzing"
                        } else {
                            "encoding"
                        };
                        let overall_fraction = (index as f64 + fraction) / total as f64;
                        reporter.report(&PipelineEvent::VideoProgress {
                            filename: filename.clone(),
                            current_video: current,
                            total_videos: total,
                            phase: phase.to_string(),
                            video_fraction: fraction,
                            overall_fraction,
                        });
                    };

                    // Process registry and cancel flag: always passed. Harmless in
                    // headless (registry is empty, cancel_flag stays false).
                    convert_single_video(
                        &ffmpeg,
                        &source_file,
                        &source_oid,
                        &relative_mp4,
                        &relative_thumb,
                        &temp_dir,
                        &ctx.staging_dir,
                        &objects,
                        &transforms,
                        &compression_config,
                        &mp4_params,
                        &thumb_params,
                        Some(&*services.processes),
                        Some(&*local_cancel),
                        Some(&progress_callback),
                    )
                })
            };

            // Release the encode slot before the fallback-copy / event-emission
            // tail — only the singleflight block needs to hold it.
            drop(encode_permit);

            if was_shared {
                log::debug!("[epoch {}] Video {} shared result from concurrent conversion", epoch, filename);
            }

            let Some(outcome) = sf_result.as_ref().filter(|o| o.error.is_none()) else {
                // An abandoned run outranks failure: the error is an artifact of the
                // cancel or of a newer dispatch taking over, not a defect in the
                // video, and the run is ending either way. Checking only the cancel
                // flag was not enough — `start_new_conversion` is a bare fetch_add
                // and never sets it, so a superseded encode used to fall through to
                // the failure path and write fallback bytes into the staging tree
                // the fresh epoch was already rewriting, with a "shipped without
                // optimizing" advisory for a video being re-encoded right then.
                if superseded_or_cancelled() {
                    log::info!("[epoch {}] Video conversion abandoned during {}", epoch, filename);
                    break 'item abandonment(is_cancelled());
                }
                let err_detail = sf_result
                    .as_ref()
                    .and_then(|o| o.error.as_deref())
                    .unwrap_or("unknown error");
                if is_headless && !was_shared {
                    eprintln!("  [{}/{}] Failed: {} ({})", current, total, filename, err_detail);
                }
                log::error!("Conversion failed for {}: {}", filename, err_detail);
                // Shipped for a shared waiter too: the leader wrote ITS output
                // path, which is a different file when two sources share an oid.
                break 'item ItemStep::shipped_original(
                    services,
                    &source_file,
                    &ctx.staging_dir,
                    &mapped_source,
                    item,
                    Some(Advisory::for_source(
                        Scope::File,
                        Severity::ShippedDegraded,
                        item,
                        crate::infra::app_advisory::fmt("shipped_without_optimizing", &[("err", err_detail)]),
                        Action::None,
                    )),
                );
            };

            // The poster and the video, then the ladder's rungs. Registered only
            // now, with the files on disk: a <video> advances past a failed
            // <source> only until HAVE_METADATA, after which the chosen resource
            // is final (incident 8690101ac), so the emitter learns about a rung
            // on the build AFTER the one that encoded it. The master playlist
            // marks the stem as laddered; the rest are registered to resolve.
            let mut delivered = vec![Delivery {
                url: relative_mp4.clone(),
                kind: DeliveryKind::Video,
                oid: outcome.mp4_oid.clone(),
            }];
            if outcome.poster {
                delivered.push(Delivery {
                    url: asset_paths::to_thumb(&mapped_source),
                    kind: DeliveryKind::Poster,
                    oid: outcome.thumb_oid.clone(),
                });
            }
            if let Some(rungs) = asset_paths::video_ladder_rungs_by_count(outcome.hls_rungs) {
                // `hls_outputs` derives its URLs by prefixing `hls_members`'
                // bare names with the ladder dir — the SAME `hls_members` call
                // `outcome.hls_entries` was keyed by inside `convert_single_
                // video`, so a name found there always names one of these URLs.
                let names = asset_paths::hls_members(rungs);
                let urls = asset_paths::hls_outputs(&mapped_source, rungs);
                delivered.extend(names.into_iter().zip(urls).map(|(name, url)| {
                    let oid = outcome
                        .hls_entries
                        .iter()
                        .find(|(n, _)| *n == name)
                        .map(|(_, o)| o.clone());
                    Delivery { url, kind: DeliveryKind::Video, oid }
                }));
            }

            if !was_shared && is_headless {
                eprintln!("  [{}/{}] Done: {}", current, total, filename);
                log::info!("[{}/{}] Converted successfully: {}", current, total, filename);
            }

            // Cap-overrun visibility: KeptOriginal and the .mov floor clamp
            // deliberately exceed max_size_mb (which may be a deploy-plugin PLATFORM
            // cap). Surface the trade-off instead of letting the user's first signal
            // be a 413 at deploy time. Stat-based so cache-hit rebuilds report it too.
            let cap_bytes = compression_config.max_size_mb as u64 * 1024 * 1024;
            let over_cap = fs::metadata(&output_mp4)
                .ok()
                .map(|m| m.len())
                .filter(|len| *len > cap_bytes);

            ItemStep::Handled {
                delivered,
                advisories: over_cap
                    .map(|len| Advisory::for_source(
                        Scope::File,
                        Severity::ShippedDegraded,
                        item,
                        crate::infra::app_advisory::fmt(
                            "video_exceeds_size_target",
                            &[
                                ("size", &format!("{:.0}", len as f64 / 1024.0 / 1024.0)),
                                ("cap", &compression_config.max_size_mb.to_string()),
                            ],
                        ),
                        Action::None,
                    ))
                    .into_iter()
                    .collect(),
                // Successful conversion (either by us or shared from another task)
                // — real work, so it counts toward the media-Job gate (FIX 1b).
                encoded: true,
            }
        };

        match step {
            ItemStep::Handled { delivered, advisories: item_advisories, encoded } => {
                let delivered_any = !delivered.is_empty();
                end.delivered += usize::from(delivered_any);
                // A detached run registers nothing itself (its coordinator is
                // gone), so what it put in staging is protected from the sweep
                // until the rebuild it asks for registers it.
                let landed: Vec<String> = if tx.is_none() { delivered.iter().map(|d| d.url.clone()).collect() } else { Vec::new() };
                record_deliveries(services, &mut produced_video_paths, delivered);
                services.cancellation.end_item(item, epoch, landed, delivered_any);
                advisories.extend(item_advisories);
                if encoded {
                    converted_count += 1;
                }
                services.end_ui_bound();
            }
            // Both abandonments release this item's budget along with every item
            // after it, from the one place that knows how many are left. Neither
            // emits the census: this dispatch's coordinator may already be gone,
            // and the next build's carry-forward re-registers the previous
            // build's outputs. The one thing they do not share is the receipt —
            // a superseded run must leave it to the fresh epoch, whose count is
            // still growing, while a cancelled run publishes what it reached.
            abandoned @ (ItemStep::Superseded | ItemStep::Cancelled) => {
                for _ in index..(total as usize) {
                    services.end_ui_bound();
                }
                if matches!(abandoned, ItemStep::Cancelled) {
                    services
                        .videos_converted
                        .store(index as u32, std::sync::atomic::Ordering::Relaxed);
                }
                end.abandoned = total as usize - index;
                return end;
            }
        }
    }

    // Save HashIndex so subsequent builds can skip re-hashing unchanged videos.
    // save_merging (not save): the image worker writes the same file concurrently
    // and holds only image entries — a plain overwrite would clobber whichever
    // category was written first. Merge preserves both.
    if let Err(e) = hash_index.save_merging(&hash_index_path) {
        log::warn!("Failed to save HashIndex: {}", e);
    }

    log::info!("[epoch {}] Video conversion complete ({} videos, {} converted)", epoch, total, converted_count);
    finish_run(
        services,
        ctx,
        &tx,
        &produced_video_paths,
        advisories,
        converted_count,
        // The receipt counts videos the encoder reached; with no FFmpeg it
        // reached none, however many originals were shipped.
        if ffmpeg.is_some() { total } else { 0 },
    );
    end
}

/// Send produced video paths to the manifest coordinator, one
/// `HashBucket::VideoOutputs` message each. `blocking_send`, because this runs
/// inside `Spawner::spawn_blocking`. No-op when `paths` is empty.
fn emit_video_outputs_via_channel(
    tx: &Option<mpsc::Sender<EmitMessage>>,
    paths: &[(String, Option<String>)],
    staging_dir: &Path,
) {
    let Some(tx) = tx else { return };
    if paths.is_empty() {
        return;
    }
    // Producer-side coherence: never register a manifest entry for a file that is
    // not on disk. A path that reaches the skip branches means an upstream step
    // reported success without landing the bytes; let it resurface in the next
    // build's scan rather than poisoning the deploy manifest. Violations are
    // collected and summarized once — one root cause (iCloud evicting every
    // staged .mp4) used to log once per file.
    //
    // The hash is a REAL content hash, as in emit_image_outputs_via_channel.
    // Without one, `verify_file_bytes(body, "")` fails the integrity check on a
    // first deploy, and — because to_mp4/to_thumb are stable, not content-hashed,
    // filenames — a re-encoded video reads as "unchanged" in the deploy diff and
    // its new bytes are never uploaded.
    let mut missing: Vec<String> = Vec::new();
    let mut read_failures: Vec<(String, String)> = Vec::new();
    for (path, oid) in paths {
        let abs = staging_dir.join(path);
        if !abs.exists() {
            missing.push(abs.display().to_string());
            continue;
        }
        // Streaming hash: KeptOriginal legitimately stages multi-GB films, so
        // never load a whole video output into memory here.
        let hash = match crate::build::assets::paths::compute_binary_hash_file(&abs) {
            Ok(h) => h,
            Err(e) => {
                // Race: existed at the pre-flight check, gone (or unreadable)
                // now. Treat as the same coherence violation as the missing
                // case and skip — never register without a hash.
                read_failures.push((abs.display().to_string(), e));
                continue;
            }
        };
        let msg = EmitMessage::File {
            rel_path: path.clone(),
            hash,
            bucket: HashBucket::VideoOutputs,
            // `Some` only from the just-encoded main path's own delivered
            // set — the carry-forward skip/carry-forward paths in
            // `dispatch_video_conversions` pass `None` for every entry.
            oid: oid.clone(),
        };
        // blocking_send: safe because run_video_conversion runs inside spawn_blocking.
        // A send failure means the coordinator was dropped; log and continue.
        if let Err(e) = tx.blocking_send(msg) {
            log::warn!("[video] Failed to register video output in coordinator: {}", e);
        }
    }
    for line in summarize_video_coherence_violations(&missing, &read_failures) {
        log::error!("{}", line);
    }
}

/// Collapse per-file video coherence violations into at most one log line per
/// class (missing / unreadable), each carrying the suppressed count and a
/// representative sample — matching the image pipeline's pattern.
fn summarize_video_coherence_violations(
    missing: &[String],
    read_failures: &[(String, String)],
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(first) = missing.first() {
        lines.push(format!(
            "[video] coherence violation: staged output missing ({}× this build); \
             first: {}",
            missing.len(),
            first,
        ));
    }
    if let Some((first_path, first_err)) = read_failures.first() {
        lines.push(format!(
            "[video] coherence violation: staged output unreadable ({}× this build); \
             first: {} ({})",
            read_failures.len(),
            first_path,
            first_err,
        ));
    }
    lines
}

/// A SHA-256 over one video's `(path, size, mtime)` plus the compression
/// config. Matching the previous dispatch's fingerprint for THIS path means
/// this one video is unchanged, and — combined with its outputs still being
/// present on disk, checked separately in `dispatch_video_conversions` —
/// there is no reason to re-encode it. The compression params are folded in
/// so a config change (`video_max_size_mb` from a deploy plugin) invalidates
/// every video's fingerprint even when no file moved.
///
/// Per item, not per set (a prerequisite for sealing a build before slow
/// video encodes finish): the old scheme hashed the whole sorted video list
/// into one fingerprint, so any single added/changed/removed video
/// invalidated it and forced a full re-dispatch — cancelling and re-running
/// every OTHER, untouched video too. Same identity scheme as
/// `compute_image_item_fingerprint` (build/media/image.rs), which mirrored
/// this fix for images in turn.
///
/// Returns `None` when the source can't be stat'd (missing / unreadable) —
/// the caller must treat that as "cannot prove unchanged" and dispatch it.
pub(crate) fn compute_video_item_fingerprint(
    source_path: &str,
    item: &str,
    compression_config: &crate::build::media::ffmpeg::VideoCompressionConfig,
) -> Option<String> {
    use sha2::{Digest, Sha256};

    let source = Path::new(source_path).join(item);
    let meta = fs::metadata(&source).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();

    let mut hasher = Sha256::new();
    hasher.update(item.as_bytes());
    hasher.update(b"\0");
    hasher.update(meta.len().to_le_bytes());
    hasher.update(mtime.to_le_bytes());

    // Include compression params so config changes trigger re-dispatch
    let params = compression_config.to_params();
    hasher.update(params.to_string().as_bytes());

    Some(format!("{:x}", hasher.finalize()))
}

/// Every staging key a video's conversion delivers: mp4, poster and the full
/// HLS ladder as a candidate set. `video_ladder_rungs` truncates from the top
/// only, so any real ladder is a prefix of it; registration filters out what is
/// not on disk.
fn video_output_keys(mapped: &str) -> Vec<String> {
    use moss_core::asset_paths;
    let mut keys = vec![asset_paths::to_mp4(mapped), asset_paths::to_thumb(mapped)];
    keys.extend(asset_paths::hls_outputs(mapped, &asset_paths::VIDEO_LADDER));
    keys
}

/// What staging holds for one video's mp4 and poster.
enum StagedVideo {
    /// Both are there; `healed` when the object store had to put one back.
    Present { healed: bool },
    /// At least one is gone and the store has no copy to relink.
    Missing,
    /// A check failed with an error that is not a positive `NotFound`: nothing
    /// is known, so nothing may be dispatched, relinked or registered.
    Unverified(String),
}

/// The object store a dispatch relinks cached video outputs from. The index is
/// read, never saved: `StatOnly` only looks entries up.
struct VideoStore {
    objects: crate::build::cache::ObjectStore,
    transforms: crate::build::cache::TransformCache,
    index: crate::build::cache::HashIndex,
    mp4_params: serde_json::Value,
}

impl VideoStore {
    fn open(moss_dir: &Path, config: &crate::build::media::ffmpeg::VideoCompressionConfig) -> Self {
        let paths = MossPaths::from_moss_dir(moss_dir.to_path_buf());
        Self {
            objects: crate::build::cache::ObjectStore::for_site(&paths),
            transforms: crate::build::cache::TransformCache::for_site(&paths),
            index: crate::build::cache::HashIndex::load(&paths.cache_hash_index()),
            mp4_params: config.to_params(),
        }
    }

    /// Probe `mp4` and `thumb` in `staging`, relinking from the store whichever
    /// is absent or evicted. A cached video whose output left staging (a sweep,
    /// an eviction) comes back here in milliseconds instead of queueing behind
    /// a real encode and being cancelled with it. `StatOnly`, because this runs
    /// on the render thread; it needs no fingerprint, so it works on the first
    /// build after a relaunch.
    ///
    /// Probed, not stat'd: an output that cannot be checked is neither present
    /// nor missing. Re-encoding it would redo work whose bytes may be fine, and
    /// registering it would claim bytes nobody read.
    fn stage(&mut self, staging: &Path, source_root: &str, item: &str, mp4: &str, thumb: &str) -> StagedVideo {
        use crate::build::io_utils::{probe_output, Presence};
        let probes = [mp4, thumb].map(|key| probe_output(&staging.join(key), crate::types::content::MODE_FILE));
        if let Some(Presence::Unverified(e)) = probes.iter().find(|p| matches!(p, Presence::Unverified(_))) {
            return StagedVideo::Unverified(e.to_string());
        }
        if probes.iter().all(Presence::is_present) {
            return StagedVideo::Present { healed: false };
        }
        let source_file = Path::new(source_root).join(item);
        let thumb_params = serde_json::json!({});
        let (mut healed, mut missing) = (false, false);
        for (key, transform, params) in [(mp4, "video/mp4", &self.mp4_params), (thumb, "video/thumbnail", &thumb_params)] {
            match rematerialize(
                &self.objects,
                &self.transforms,
                params,
                &mut self.index,
                &source_file,
                item,
                &staging.join(key),
                transform,
                HashPolicy::StatOnly,
            ) {
                HealOutcome::AlreadyPresent => {}
                HealOutcome::Healed => healed = true,
                HealOutcome::NotCached => missing = true,
                HealOutcome::Unverified(e) => return StagedVideo::Unverified(e.to_string()),
            }
        }
        if missing { StagedVideo::Missing } else { StagedVideo::Present { healed } }
    }
}

/// Spawn the video worker — one owner for both the first-build and rebuild
/// paths in `build_inner()`.
///
/// The skip/dispatch decision is made per video, not for the set as a whole:
/// each video's own fingerprint (`compute_video_item_fingerprint`) is compared
/// against the fingerprint recorded when a run last delivered THAT video.
/// A video whose fingerprint matches and whose canonical outputs (mp4 +
/// poster) are still present on disk is carried forward — its output keys are
/// re-registered so seal() doesn't prune them — without ever entering
/// `run_video_conversion`. Only the changed / new / missing-output subset is
/// actually dispatched to the encoder, so one added video no longer cancels
/// and re-runs every other, untouched video (a prerequisite for sealing a
/// build before slow video encodes finish).
pub(crate) fn dispatch_video_conversions(
    services: Option<&BuildServices>,
    mut background_ctx: BackgroundContext,
    tx: Option<mpsc::Sender<EmitMessage>>,
) {
    if background_ctx.video_items.is_empty() {
        // Nothing to convert. This branch used to be the "no videos" seam for the
        // `BuildComplete` event — a seam `pipeline.rs` never reaches, because it
        // only spawns this worker when the site HAS videos. That is the bug
        // `BuildTerminalBarrier` exists to make unrepeatable: a receipt owned by
        // a worker that may not run is not a receipt.
        return;
    }

    if let Some(svc) = services {
        if let Some(spawner) = svc.spawner.clone() {
            // Fingerprint-based skip: only relevant in GUI mode where rebuilds may
            // overlap with in-progress conversions. Headless is called once per
            // invocation, so there's no previous dispatch to compare against.
            let compression_config = match svc.deploy_video_max_size_mb {
                // Mirrors the override arm in `run_video_conversion`: the
                // plugin host's per-file limit bounds both the MP4 and the
                // HLS ladder's own files.
                Some(mb) => crate::build::media::ffmpeg::VideoCompressionConfig {
                    max_size_mb: mb,
                    hls_max_file_mb: mb,
                    ..Default::default()
                },
                None => crate::build::media::ffmpeg::VideoCompressionConfig::default(),
            };

            use crate::build::render::resolve_path_with_overrides;
            use moss_core::asset_paths;
            let dir_overrides = background_ctx.dir_overrides.clone();
            let total_items = background_ctx.video_items.len();

            // Per-item decision. `skip_paths` carries forward the output keys
            // of videos staying put — still re-registered every round, or
            // seal()'s `video_outputs.retain` prunes them as untouched and the
            // stale sweep deletes a physically-present file (the same class of
            // bug 71ee43bc1c fixed for the whole set, now enforced per item).
            // `to_dispatch` is the subset that actually needs the encoder.
            //
            // Neither of these two carries a fresh oid: `store.stage`'s
            // self-heal (below) relinks a cached blob without surfacing which
            // one, and a video that is simply unchanged never calls
            // `convert_single_video` at all this round. Every push pairs its
            // path with `None`; ship-by-OID's fingerprint fallback covers it.
            let mut skip_paths: Vec<(String, Option<String>)> = Vec::new();
            // Outputs of a video that IS being re-dispatched below, but whose
            // previous mp4/poster are still sitting at that same path (the
            // encode hasn't reached its atomic `link_to` swap yet). Registered
            // as this build's own output too, same as `skip_paths`, so the
            // core seal — which no longer waits for the encode, see below —
            // does not read "not re-emitted this round" as "gone" and let the
            // stale sweep delete a video that is still serving fine.
            let mut carry_forward_paths: Vec<(String, Option<String>)> = Vec::new();
            // (path, fingerprint): what a new run would own if one is spawned.
            let mut to_dispatch: Vec<(String, Option<String>)> = Vec::new();
            // Items a run already spawned is converting from these same bytes.
            let mut joined: Vec<(String, Option<String>)> = Vec::new();
            let mut current_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
            // One summary line per dispatch, never one per video: this runs on
            // every rebuild, and a storm of them is exactly when the log matters.
            let mut carried = 0usize;
            let mut healed = 0usize;
            let mut unverified = 0usize;
            let mut queued_why: Vec<String> = Vec::new();

            let mut store = VideoStore::open(&background_ctx.moss_dir, &compression_config);

            for item in &background_ctx.video_items {
                current_paths.insert(item.clone());
                let mapped = resolve_path_with_overrides(item, &dir_overrides);
                let mp4 = asset_paths::to_mp4(&mapped);
                let thumb = asset_paths::to_thumb(&mapped);

                // A fingerprint that can't be computed (source unreadable)
                // can't be proven unchanged either — dispatch it rather than
                // risk carrying forward a stale skip.
                let seen_before = svc.cancellation.has_item_fingerprint(item);
                let fingerprint =
                    compute_video_item_fingerprint(&background_ctx.source_path, item, &compression_config);
                let fingerprint_matched = fingerprint
                    .as_deref()
                    .is_some_and(|fp| svc.cancellation.item_fingerprint_matches(item, fp));

                // Join before anything else: the running encode is converting
                // these exact bytes, so superseding it restarts minutes of work
                // for nothing, and healing its outputs would put the dispatcher's
                // link on a target the encode is about to link. Whatever an
                // earlier build left staged is still carried forward.
                if fingerprint.as_deref().is_some_and(|fp| svc.cancellation.is_running(item, fp)) {
                    let staged = |key: &String| {
                        crate::build::io_utils::output_present(&background_ctx.staging_dir.join(key))
                    };
                    if staged(&mp4) && staged(&thumb) {
                        carry_forward_paths.push((mp4, None));
                        carry_forward_paths.push((thumb, None));
                    }
                    joined.push((item.clone(), fingerprint));
                    continue;
                }

                // The output check is the self-heal seam — without it, a
                // previous dispatch killed mid-`link_to` leaves 0-byte stubs
                // in canonical and every subsequent dispatch with a matching
                // fingerprint skips, so the user sees a permanent loader on
                // the affected card. Requiring the files to actually be there
                // is what `run_video_conversion`'s per-video fast path
                // already promises, but the code must check it here too or
                // that fast path never runs.
                let (outputs_present, healed_item) =
                    match store.stage(&background_ctx.staging_dir, &background_ctx.source_path, item, &mp4, &thumb) {
                        StagedVideo::Present { healed } => (true, healed),
                        StagedVideo::Missing => (false, false),
                        // Both keys reported unverified withholds this generation.
                        StagedVideo::Unverified(detail) => {
                            unverified += 1;
                            if let Some(tx) = &tx {
                                for key in [&mp4, &thumb] {
                                    let _ = tx.blocking_send(EmitMessage::Unverified {
                                        rel_path: key.clone(),
                                        detail: detail.clone(),
                                    });
                                }
                            }
                            continue;
                        }
                    };

                if outputs_present && (fingerprint_matched || healed_item) {
                    if healed_item {
                        healed += 1;
                        // The dispatcher put this source's cached output back itself: no run will vouch for it.
                        fingerprint.iter().for_each(|fp| svc.cancellation.record_item_fingerprint(item, fp));
                    } else {
                        carried += 1;
                    }
                    // Re-register every key the encode path delivers for this
                    // video so seal() keeps them. emit_video_outputs_via_channel's
                    // existence check drops the keys whose staging file is
                    // absent (the HLS ladder's excess candidates, normally)
                    // rather than registering a lie.
                    skip_paths.extend(video_output_keys(&mapped).into_iter().map(|key| (key, None)));
                } else {
                    let why = if fingerprint_matched {
                        "no cached output"
                    } else if seen_before {
                        "changed"
                    } else {
                        "new"
                    };
                    if queued_why.len() < 3 {
                        queued_why.push(format!("{item}: {why}"));
                    }
                    // A video with no prior output (first-ever encode) has
                    // nothing to carry forward: it stays absent from this
                    // build's manifest until its own encode finishes and the
                    // follow-up rebuild (triggered below) registers it for
                    // real — never a half-written or stale entry.
                    if outputs_present {
                        carry_forward_paths.push((mp4, None));
                        carry_forward_paths.push((thumb, None));
                    }
                    to_dispatch.push((item.clone(), fingerprint));
                }
            }

            // Drop stored fingerprints for videos no longer in the current
            // set, so a removed-then-re-added video starts fresh rather than
            // replaying a stale match against bytes that may since have
            // changed. Bounds the map to the live video set.
            svc.cancellation.retain_item_fingerprints(&current_paths);

            if !skip_paths.is_empty() {
                emit_video_outputs_via_channel(&tx, &skip_paths, &background_ctx.staging_dir);
            }
            if !carry_forward_paths.is_empty() {
                emit_video_outputs_via_channel(&tx, &carry_forward_paths, &background_ctx.staging_dir);
            }
            svc.cancellation.forget_landed(
                skip_paths.iter().map(|(p, _)| p).chain(carry_forward_paths.iter().map(|(p, _)| p)),
            );

            log::info!(
                "video dispatch: {} items — {} carried, {} healed from CAS, {} joined running encode, \
                 {} unverified, {} queued{}",
                total_items,
                carried,
                healed,
                joined.len(),
                unverified,
                to_dispatch.len(),
                if queued_why.is_empty() { String::new() } else { format!(" (queued: {})", queued_why.join(", ")) }
            );
            // Nothing new: every joined item keeps its run, its epoch and its
            // singleflight entry.
            if to_dispatch.is_empty() {
                return;
            }

            // Something new does need the encoder, and a new epoch stops the
            // run in flight — so the videos it was still converting, if this
            // build still wants them, ride along into the new run. Only the
            // changed/new/missing-output subset ever enters it; unchanged
            // videos never contend for an encode permit, an iCloud
            // materialization wait, or a UiBound slot behind one slow encode.
            to_dispatch.extend(joined);

            // Bump the epoch so stale tasks exit on their next epoch check — the
            // sole halt signal for a prior epoch, and what keeps concurrent
            // FFmpeg processes from piling up. Clearing singleflight lets the
            // superseded videos be re-dispatched while the old task drains.
            // This clears the WHOLE map: narrowing it to the dispatched subset's
            // source_oids would mean hashing every video up front, which is
            // exactly the multi-GB-file cost per-item fingerprinting avoids.
            svc.in_flight_videos.clear();
            let epoch = svc.cancellation.start_new_conversion();
            svc.cancellation.begin_run(
                epoch,
                to_dispatch.iter().filter_map(|(item, fp)| {
                    let outputs = video_output_keys(&resolve_path_with_overrides(item, &dir_overrides));
                    Some((item.clone(), fp.clone()?, outputs))
                }),
            );
            background_ctx.video_items = to_dispatch.into_iter().map(|(item, _)| item).collect();

            // GUI/CLI mode with event sink: async spawn
            // Increment tracker BEFORE spawn to close the race window.
            // Without this, the CLI could check has_ui_bound() between
            // tokio::spawn scheduling the task and the task actually starting,
            // finding no active tasks, and exiting prematurely.
            let total = background_ctx.video_items.len() as u32;
            for _ in 0..total {
                svc.begin_ui_bound();
            }

            // Field-wise Arc::clone: the same counters, tokens and parent cell,
            // reached from the spawned task. See `BuildServices`'s own doc.
            let services_arc = std::sync::Arc::new(svc.clone());
            log::info!(
                "[epoch {}] Spawning background video conversion for {} videos",
                epoch,
                background_ctx.video_items.len()
            );
            // The core seal must not wait for this encode — PROVIDED someone
            // will notice when it finishes. `tx` is the manifest
            // coordinator's sender; holding it open across a multi-minute
            // two-pass encode is what used to block `seal+persist` behind
            // one slow video. Every video this build isn't re-encoding has
            // already registered its output above (skip_paths unchanged,
            // carry_forward_paths mid-re-encode), so dropping `tx` early
            // lets the coordinator close its channel as soon as every OTHER
            // background worker finishes.
            //
            // That only converges, though, if something is watching this
            // folder for the follow-up rebuild the encode asks for below —
            // `ops::watch::worker` (the same lever a file edit already
            // drives, and `trigger_media_settle_rerender` uses for an
            // in-build poster settle). `register_worker`/`ensure_worker`
            // register it at folder-open, before the first build ever runs
            // (`ops/watch.rs`), so this read is reliable for the app and for
            // `moss build --serve --watch` on every build including the
            // first. A one-shot `moss build`, `moss deploy`, or a plugin
            // install has no such worker and will exit the moment this
            // function returns — there is nobody left to pick up a later
            // "enqueue a rebuild", so a manifest sealed without this video
            // would publish (or simply finish) permanently incomplete. For
            // that case the split does not apply: the seal waits for this
            // encode exactly as it did before this change.
            let folder_path = background_ctx.source_path.clone();
            if crate::ops::watch::worker::get(&folder_path).is_some() {
                drop(tx);
                // The encode outlives this build's cache lease; its own keeps
                // a cache GC off the blobs and records it stores before the
                // run's hash index, which marks them live, is saved.
                let encode_lease = crate::build::lifecycle::encode_lease(&MossPaths::from_moss_dir(
                    background_ctx.moss_dir.clone(),
                ));
                spawner.spawn_blocking(Box::new(move || {
                    let _encode_lease = encode_lease;
                    let end = run_video_conversion(&services_arc, &background_ctx, epoch, None);
                    // Only a run that put bytes on disk has anything for a
                    // rebuild to register. A superseded or cancelled one asking
                    // anyway is what kept the storm going: its rebuild found
                    // the outputs still missing, dispatched again, superseded
                    // the next run, and so on with no user input.
                    let worker = crate::ops::watch::worker::get(&folder_path).filter(|_| end.delivered > 0);
                    end.log(epoch, worker.is_some());
                    if let Some(worker) = worker {
                        worker.enqueue(crate::ops::watch::worker::RebuildRequest::full());
                    }
                }));
            } else {
                spawner.spawn_blocking(Box::new(move || {
                    run_video_conversion(&services_arc, &background_ctx, epoch, tx).log(epoch, false);
                }));
            }
        } else {
            // Headless mode: run synchronously with epoch=0
            // Increment tracker BEFORE calling run_video_conversion — the inner loop
            // calls fetch_sub(1) per video, so the counter must start at `total`.
            let total = background_ctx.video_items.len() as u32;
            for _ in 0..total {
                svc.begin_ui_bound();
            }
            log::info!(
                "Running headless video conversion for {} videos",
                background_ctx.video_items.len()
            );
            run_video_conversion(svc, &background_ctx, 0, tx).log(0, false);
        }
    }
}

/// One-time migration: remove the legacy path-keyed video cache that CAS
/// replaced. Only once the CAS objects directory exists, so the old cache is
/// never dropped before its replacement is in place.
pub(crate) fn cleanup_legacy_video_cache(moss_dir: &Path) {
    let paths = MossPaths::from_moss_dir(moss_dir.to_path_buf());
    let legacy_video_cache = paths.cache_videos_legacy();
    if legacy_video_cache.exists() && paths.cache_objects().exists() {
        // allow:unlink the retired .moss/cache/videos tree, not staging
        if let Err(e) = fs::remove_dir_all(&legacy_video_cache) {
            log::warn!("Failed to remove legacy video cache: {}", e);
        } else {
            log::info!("Removed legacy path-based video cache (.moss/build.nosync/cache/videos/)");
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // ===========================================
    // clear_stale_ladder: the Ok(None) record cleanup
    // ===========================================

    /// A record left over from a build where this source HAD a ladder — its
    /// `video/hls/` keys must be gone after `clear_stale_ladder`, and every
    /// other key (the mp4, the thumbnail) must survive untouched.
    #[test]
    fn clear_stale_ladder_drops_only_the_hls_keys() {
        use crate::build::cache::{ObjectStore, TransformCache, TransformEntry, TransformRecord};

        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("clip.mov");
        std::fs::write(&source, b"source bytes").unwrap();
        let transforms = TransformCache::new(
            dir.path().join("transforms"),
            ObjectStore::new(dir.path().join("objects")),
        );
        let entry = |tag: &str| TransformEntry {
            oid: format!("oid-{tag}"),
            size: 1,
            params: serde_json::json!({}),
        };
        let source_oid = "oid-collapsed";
        transforms
            .put(&TransformRecord {
                source_oid: source_oid.to_string(),
                source_size: 0,
                transforms: [
                    ("video/mp4".to_string(), entry("mp4")),
                    ("video/thumbnail".to_string(), entry("thumb")),
                    ("video/hls/master.m3u8".to_string(), entry("master")),
                    ("video/hls/v0.m3u8".to_string(), entry("v0")),
                    ("video/hls/v0.m4s".to_string(), entry("v0seg")),
                ]
                .into_iter()
                .collect(),
            })
            .unwrap();

        clear_stale_ladder(&transforms, source_oid, &source);

        let after = transforms.get(source_oid).expect("the record itself must survive");
        assert!(
            after.transforms.keys().all(|k| !k.starts_with(crate::build::media::hls::HLS_TRANSFORM_PREFIX)),
            "every video/hls/ key must be gone: {:?}",
            after.transforms.keys().collect::<Vec<_>>()
        );
        assert_eq!(after.transforms.len(), 2, "the mp4 and thumbnail entries must survive untouched");
        assert!(after.transforms.contains_key("video/mp4"));
        assert!(after.transforms.contains_key("video/thumbnail"));
    }

    /// `clear_stale_ladder` runs on every `Ok(None)` — every video that was
    /// never wide enough for a ladder in the first place, on every build.
    /// Most sources never had one, so it must not pay a record write when
    /// there is nothing to clear: no record where none existed, and no
    /// rewrite of a record that already carries no `video/hls/` keys.
    #[test]
    fn clear_stale_ladder_is_a_no_op_when_there_is_nothing_to_clear() {
        use crate::build::cache::{ObjectStore, TransformCache, TransformEntry, TransformRecord};

        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("clip.mov");
        std::fs::write(&source, b"source bytes").unwrap();
        let transforms = TransformCache::new(
            dir.path().join("transforms"),
            ObjectStore::new(dir.path().join("objects")),
        );

        // No record at all — a source too narrow for a ladder on its very
        // first build. Must not fabricate one.
        let untouched_oid = "oid-never-had-a-record";
        clear_stale_ladder(&transforms, untouched_oid, &source);
        assert!(
            transforms.get(untouched_oid).is_none(),
            "a source with no record at all must still have none"
        );

        // A record exists, but carries no video/hls/ keys (mp4 already
        // converted, ladder never attempted) — must come back byte-for-byte
        // the same record, not merely an equivalent one written fresh.
        let mp4_only_oid = "oid-mp4-only";
        let seeded = TransformRecord {
            source_oid: mp4_only_oid.to_string(),
            source_size: 0,
            transforms: [(
                "video/mp4".to_string(),
                TransformEntry { oid: "oid-mp4".to_string(), size: 1, params: serde_json::json!({}) },
            )]
            .into_iter()
            .collect(),
        };
        transforms.put(&seeded).unwrap();

        clear_stale_ladder(&transforms, mp4_only_oid, &source);

        assert_eq!(
            transforms.get(mp4_only_oid).expect("the record must still be there"),
            seeded,
            "a record with no video/hls/ keys must come back exactly as seeded"
        );
    }

    // ===========================================
    // Encode-permit (bounded concurrency) tests
    // ===========================================
    // All against LOCAL semaphores, never the global static — parallel test
    // threads must not interfere with each other.

    #[test]
    fn acquire_encode_permit_bounds_concurrency_to_permit_count() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let sem = Arc::new(tokio::sync::Semaphore::new(2));
        let active = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let (sem, active, max_seen) = (sem.clone(), active.clone(), max_seen.clone());
                std::thread::spawn(move || {
                    let permit =
                        acquire_encode_permit(&sem, std::time::Duration::from_millis(5), &|| false)
                            .expect("not cancelled");
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(40));
                    active.fetch_sub(1, Ordering::SeqCst);
                    drop(permit);
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert!(max_seen.load(Ordering::SeqCst) <= 2, "more than 2 concurrent holders");
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn acquire_encode_permit_returns_none_when_cancelled_while_full() {
        let sem = tokio::sync::Semaphore::new(1);
        let _held = sem.try_acquire().expect("prime the semaphore");
        let res = acquire_encode_permit(&sem, std::time::Duration::from_millis(1), &|| true);
        assert!(res.is_none());
    }

    #[test]
    fn acquire_encode_permit_is_immediate_when_free() {
        let sem = tokio::sync::Semaphore::new(1);
        assert!(
            acquire_encode_permit(&sem, std::time::Duration::from_millis(1), &|| false).is_some()
        );
    }

    // ── Gap #2: untouched video keys survive the seal on dispatch-skip ──

    /// Helper: create a portable TempDir inside the repo's target/test-tmp.
    fn portable_tmpdir() -> tempfile::TempDir {
        let test_tmp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("test-tmp");
        std::fs::create_dir_all(&test_tmp).unwrap();
        tempfile::TempDir::new_in(&test_tmp).unwrap()
    }

    // ── Per-item dispatch: one video's fate must not follow its siblings' ──
    //
    // These four tests replace `a_skip_dispatch_keeps_the_hls_ladder_through_
    // stale_cleanup` and `a_matched_fingerprint_with_missing_outputs_is_not_
    // skipped` (both whole-set), which they subsume: the ladder-carry-forward
    // property lives on in test (a) below, and the missing-output self-heal
    // lives on in test (c) — both now proven per item instead of per set.

    /// Shared rig: writes `item`'s bytes into `vault`, stages a full output
    /// set (mp4 + poster, plus an HLS ladder when `ladder` is true) under
    /// `staging` with SENTINEL bytes that are deliberately NOT what a real
    /// encode of `source_bytes` would produce, and primes `svc`'s per-item
    /// fingerprint so a following dispatch with the SAME source bytes takes
    /// the skip branch for `item` alone.
    ///
    /// The sentinel-vs-real-bytes gap is the whole test mechanism below:
    /// every test's `ffmpeg_bin_path` points at a binary that does not
    /// exist, so a DISPATCHED item's mp4 is overwritten by
    /// `ItemStep::shipped_original`'s fallback copy of the CURRENT vault
    /// bytes (same technique as `a_video_that_could_not_even_be_copied_
    /// fails_the_media_job`). A SKIPPED item's mp4 never goes through that
    /// path, so it keeps the sentinel unchanged — the observable, disk-level
    /// proof of "was not re-dispatched" this suite is built on.
    fn stage_and_prime_video(
        svc: &BuildServices,
        vault: &Path,
        staging: &Path,
        item: &str,
        source_bytes: &[u8],
        ladder: bool,
    ) -> Vec<String> {
        use moss_core::asset_paths;

        std::fs::create_dir_all(vault.join(item).parent().unwrap()).unwrap();
        std::fs::write(vault.join(item), source_bytes).unwrap();

        let mut staged = vec![asset_paths::to_mp4(item), asset_paths::to_thumb(item)];
        if ladder {
            let rungs = asset_paths::video_ladder_rungs(640);
            staged.extend(asset_paths::hls_outputs(item, rungs));
        }
        for key in &staged {
            let abs = staging.join(key);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(&abs, b"STAGED-SENTINEL").unwrap();
        }

        let fingerprint = compute_video_item_fingerprint(
            &vault.display().to_string(),
            item,
            &crate::build::media::ffmpeg::VideoCompressionConfig::default(),
        )
        .expect("source file exists and is stat-able");
        svc.cancellation.record_item_fingerprint(item, &fingerprint);

        staged
    }

    /// Register `folder`'s rebuild-worker slot for the duration of a test
    /// unless the caller already registered one itself (found via `get`,
    /// reused rather than replaced — replacing would orphan a handle the
    /// caller is holding). Returns the handle and whether this call is the
    /// owner responsible for deregistering it.
    ///
    /// `dispatch_video_conversions` only takes the seal/video split when
    /// `ops::watch::worker::get(folder)` finds a registered worker (see its
    /// doc) — matching the app and `moss build --serve --watch`, both of
    /// which register at folder-open, before the first build. Most of this
    /// suite wants that branch exercised; the one test that deliberately
    /// does NOT (`a_build_with_no_registered_worker_waits_for_the_video_before_sealing`)
    /// skips this helper.
    fn ensure_worker_for_test(folder: &str) -> (std::sync::Arc<crate::ops::watch::worker::WorkerHandle>, bool) {
        match crate::ops::watch::worker::get(folder) {
            Some(existing) => (existing, false),
            None => (crate::ops::watch::worker::register(folder), true),
        }
    }

    /// Run `dispatch_video_conversions` against `video_items` (an ffmpeg
    /// binary that doesn't exist, so any real dispatch takes the
    /// `shipped_original` fallback path) and seal, returning the manifest.
    ///
    /// Registers `vault`'s rebuild-worker slot for the call (see
    /// `ensure_worker_for_test`) so the split applies, then deliberately does
    /// NOT wait for the spawned background conversion to finish — proving
    /// that is the point of the seal/video split this helper exercises.
    /// Wrapped in a bounded timeout rather than a bare `.await`: a regression
    /// that goes back to holding the coordinator's sender open across the
    /// encode must fail this test fast, not hang the suite. Callers that need
    /// to inspect a DISPATCHED video's on-disk result after its encode runs
    /// use `dispatch_with_controlled_spawner` below, which hands back a
    /// spawner the test drives explicitly instead of racing a real one.
    async fn dispatch_and_seal(
        svc: &BuildServices,
        vault: &Path,
        staging: &Path,
        moss_dir: &Path,
        video_items: Vec<String>,
    ) -> crate::build::manifest::SealedManifest {
        use crate::build::coordinator::test_utils;
        use crate::types::content::SiteHashes;
        use crate::types::services::BackgroundContext;

        let folder = vault.display().to_string();
        let (worker, owns_worker) = ensure_worker_for_test(&folder);

        let ctx = BackgroundContext {
            video_items,
            source_path: folder.clone(),
            staging_dir: staging.to_path_buf(),
            moss_dir: moss_dir.to_path_buf(),
            ffmpeg_bin_path: Some(moss_dir.join("no-such-ffmpeg").display().to_string()),
            ..BackgroundContext::for_test()
        };
        let (tx, rx) = test_utils::build_test_coordinator();
        // `cancellation` is Arc-shared across the clone, so the caller's
        // `svc` observes the same epoch/fingerprint state afterward.
        let svc_for_dispatch = svc.clone();
        // blocking_send requires a non-async thread — as in production, where
        // the dispatcher is called from the blocking render phase.
        tokio::task::spawn_blocking(move || {
            dispatch_video_conversions(Some(&svc_for_dispatch), ctx, Some(tx));
        })
        .await
        .unwrap();
        let sealed = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            test_utils::drain_into_sealed(rx, SiteHashes::default()),
        )
        .await
        .expect(
            "the core seal must not wait for a still-running video encode — \
             it hung instead of closing the coordinator's channel",
        );
        if owns_worker {
            crate::ops::watch::worker::deregister(&folder, &worker);
        }
        sealed
    }

    /// A `Spawner` that CAPTURES `spawn_blocking` closures instead of running
    /// them, so a test can assert on the seal's own state before a deferred
    /// video encode ever runs (the point of the seal/video split), then run
    /// the encode explicitly to observe what changes once it lands — without
    /// racing a real background thread.
    #[derive(Default)]
    struct ControlledSpawner {
        captured: std::sync::Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    }

    impl crate::build::ports::spawner::Spawner for ControlledSpawner {
        fn spawn_blocking(&self, task: Box<dyn FnOnce() + Send + 'static>) {
            self.captured.lock().unwrap().push(task);
        }
        fn spawn(&self, _task: crate::build::ports::spawner::Task) -> crate::build::ports::spawner::Joining {
            unimplemented!("the video dispatch path under test never uses the async spawn half")
        }
    }

    impl ControlledSpawner {
        /// Run every captured task, in the order they were queued, and clear
        /// the queue. Simulates "the deferred video encode finishes now".
        fn run_captured(&self) {
            let tasks: Vec<_> = std::mem::take(&mut *self.captured.lock().unwrap());
            for task in tasks {
                task();
            }
        }

        fn captured_count(&self) -> usize {
            self.captured.lock().unwrap().len()
        }
    }

    /// Like `dispatch_and_seal`, but installs a `ControlledSpawner` on `svc`
    /// and hands it back instead of letting the deferred encode run on its
    /// own. The returned manifest is exactly what the core seal produced —
    /// still true to "does not wait" — and the caller decides when (or
    /// whether) to call `.run_captured()` on the spawner to simulate the
    /// encode landing. Also registers `vault`'s rebuild-worker slot (see
    /// `ensure_worker_for_test`) so the split applies.
    async fn dispatch_with_controlled_spawner(
        svc: &mut BuildServices,
        vault: &Path,
        staging: &Path,
        moss_dir: &Path,
        video_items: Vec<String>,
    ) -> (crate::build::manifest::SealedManifest, std::sync::Arc<ControlledSpawner>) {
        use crate::build::coordinator::test_utils;
        use crate::types::content::SiteHashes;
        use crate::types::services::BackgroundContext;

        let folder = vault.display().to_string();
        let (worker, owns_worker) = ensure_worker_for_test(&folder);

        let spawner = std::sync::Arc::new(ControlledSpawner::default());
        svc.spawner = Some(spawner.clone());

        let ctx = BackgroundContext {
            video_items,
            source_path: folder.clone(),
            staging_dir: staging.to_path_buf(),
            moss_dir: moss_dir.to_path_buf(),
            ffmpeg_bin_path: Some(moss_dir.join("no-such-ffmpeg").display().to_string()),
            ..BackgroundContext::for_test()
        };
        let (tx, rx) = test_utils::build_test_coordinator();
        let svc_for_dispatch = svc.clone();
        tokio::task::spawn_blocking(move || {
            dispatch_video_conversions(Some(&svc_for_dispatch), ctx, Some(tx));
        })
        .await
        .unwrap();
        let sealed = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            test_utils::drain_into_sealed(rx, SiteHashes::default()),
        )
        .await
        .expect(
            "the core seal must not wait for a still-running video encode — \
             it hung instead of closing the coordinator's channel",
        );
        if owns_worker {
            crate::ops::watch::worker::deregister(&folder, &worker);
        }
        (sealed, spawner)
    }

    /// Put `item`'s source in the vault and its encode in the object store, as
    /// a previous process left them: mp4 and poster blobs, the transform record
    /// under today's compression params, and a saved hash-index entry for the
    /// source's current size and mtime. Staging is left untouched.
    pub(crate) fn seed_cached_video(vault: &Path, moss_dir: &Path, item: &str, mp4: &[u8], poster: &[u8]) {
        use crate::build::cache::{HashIndex, ObjectStore, TransformCache, TransformEntry, TransformRecord};

        let source = vault.join(item);
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        if !source.exists() {
            std::fs::write(&source, format!("source of {item}")).unwrap();
        }
        let paths = MossPaths::from_moss_dir(moss_dir.to_path_buf());
        let objects = ObjectStore::for_site(&paths);
        let transforms = TransformCache::for_site(&paths);
        let source_oid = ObjectStore::hash_file(&source).unwrap();
        let entry = |bytes: &[u8], params: serde_json::Value| TransformEntry {
            oid: objects.store_bytes(bytes).unwrap(),
            size: bytes.len() as u64,
            params,
        };
        let config = crate::build::media::ffmpeg::VideoCompressionConfig::default();
        transforms
            .put(&TransformRecord {
                source_oid: source_oid.clone(),
                source_size: std::fs::metadata(&source).unwrap().len(),
                transforms: [
                    ("video/mp4".to_string(), entry(mp4, config.to_params())),
                    ("video/thumbnail".to_string(), entry(poster, serde_json::json!({}))),
                ]
                .into_iter()
                .collect(),
            })
            .unwrap();
        let index_path = paths.cache_hash_index();
        let mut index = HashIndex::load(&index_path);
        let meta = std::fs::metadata(&source).unwrap();
        let mtime = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        index.update_whole_second(item.to_string(), meta.len(), mtime, source_oid);
        index.save(&index_path).unwrap();
    }

    /// A cached video whose outputs left staging is relinked from the object
    /// store by the dispatcher itself, never queued for the encoder: queued, it
    /// waits behind whatever real encode is ahead of it and is cancelled with
    /// it, which is how ten cached videos stayed "missing" through a rebuild
    /// storm. The services are fresh (no fingerprint seen), as on the first
    /// build after a relaunch; a video neither cached nor staged still queues.
    #[tokio::test]
    async fn a_cached_video_missing_from_staging_is_relinked_not_encoded() {
        use moss_core::asset_paths;

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        std::fs::create_dir_all(&staging).unwrap();
        let mut svc = BuildServices::headless();

        let cached = "videos/cached.mov".to_string();
        let running = "videos/running.mov".to_string();
        let uncached = "videos/new.mov".to_string();
        seed_cached_video(&vault, &moss_dir, &cached, b"cached mp4", b"cached poster");
        seed_cached_video(&vault, &moss_dir, &running, b"running mp4", b"running poster");
        std::fs::write(vault.join(&uncached), b"never encoded").unwrap();
        // An earlier build's run is still converting `running` from these bytes.
        let config = crate::build::media::ffmpeg::VideoCompressionConfig::default();
        let fingerprint = compute_video_item_fingerprint(&vault.display().to_string(), &running, &config).unwrap();
        svc.cancellation.begin_run(svc.cancellation.start_new_conversion(), [(running.clone(), fingerprint, vec![])]);
        let epoch_before = svc.cancellation.current_id();

        let folder = vault.display().to_string();
        let worker = crate::ops::watch::worker::register(&folder);
        let (sealed, spawner) = dispatch_with_controlled_spawner(
            &mut svc,
            &vault,
            &staging,
            &moss_dir,
            vec![running.clone(), cached.clone()],
        )
        .await;

        assert_eq!(spawner.captured_count(), 0, "a cached video must not be sent to the encoder");
        assert_eq!(svc.cancellation.current_id(), epoch_before, "nothing new, so the running run is not superseded");
        assert!(
            !staging.join(asset_paths::to_mp4(&running)).exists(),
            "a video a run is converting is joined, never healed under it"
        );
        assert!(!worker.slot_occupied(), "a relink delivers nothing that needs a follow-up rebuild");
        for (key, bytes) in [
            (asset_paths::to_mp4(&cached), &b"cached mp4"[..]),
            (asset_paths::to_thumb(&cached), &b"cached poster"[..]),
        ] {
            assert_eq!(std::fs::read(staging.join(&key)).unwrap(), bytes, "{key} relinked from the store");
            assert!(sealed.files().contains_key(&key), "{key} registered this build: {:?}", sealed.files());
        }

        let (_, spawner) =
            dispatch_with_controlled_spawner(&mut svc, &vault, &staging, &moss_dir, vec![uncached]).await;
        assert_eq!(spawner.captured_count(), 1, "missing and not in the store still queues an encode");
        crate::ops::watch::worker::deregister(&folder, &worker);
    }

    /// Every run and image batch writes its temps in its own scratch directory
    /// and removes only that. A video run used to wipe the shared `cache/tmp`
    /// on entry, taking an image batch's temps — or the temps and two-pass logs
    /// of the run it had just joined — mid-write.
    #[tokio::test]
    async fn a_video_run_removes_its_own_scratch_and_no_one_elses() {
        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        std::fs::create_dir_all(vault.join("videos")).unwrap();
        std::fs::write(vault.join("videos/clip.mov"), b"clip bytes").unwrap();
        let cache_tmp = MossPaths::from_moss_dir(moss_dir.clone()).cache_tmp();
        let image_batch = crate::build::io_utils::ScratchDir::new(&cache_tmp, "image");
        let image_temp = image_batch.path().join("photo.webp.tmp");
        std::fs::write(&image_temp, b"an image batch mid-encode").unwrap();

        let svc = std::sync::Arc::new(BuildServices::headless());
        let ctx = std::sync::Arc::new(BackgroundContext {
            video_items: vec!["videos/clip.mov".to_string()],
            source_path: vault.display().to_string(),
            staging_dir: staging.clone(),
            moss_dir: moss_dir.clone(),
            ffmpeg_bin_path: Some(moss_dir.join("no-such-ffmpeg").display().to_string()),
            ..BackgroundContext::for_test()
        });
        // One run that finishes, one superseded before its first item.
        for epoch in [svc.cancellation.current_id(), svc.cancellation.current_id() + 7] {
            let (svc, ctx) = (svc.clone(), ctx.clone());
            tokio::task::spawn_blocking(move || run_video_conversion(&svc, &ctx, epoch, None)).await.unwrap();
        }

        assert!(image_temp.exists(), "a video run must not take an image batch's temp");
        let left: Vec<_> = std::fs::read_dir(&cache_tmp).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, vec![image_batch.path().file_name().unwrap().to_owned()], "each run removes its own scratch");
    }

    /// A run that ends having delivered nothing asks for no rebuild. Asking
    /// anyway was the storm's engine: the rebuild found the outputs still
    /// missing, dispatched again, superseded the next run, and that run's end
    /// asked again — with no user input, for as long as the encode took.
    #[tokio::test]
    async fn a_run_superseded_before_it_delivers_requests_no_rebuild() {
        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        std::fs::create_dir_all(vault.join("videos")).unwrap();
        std::fs::write(vault.join("videos/clip.mov"), b"clip bytes").unwrap();
        let mut svc = BuildServices::headless();

        let folder = vault.display().to_string();
        let worker = crate::ops::watch::worker::register(&folder);
        let (_, spawner) = dispatch_with_controlled_spawner(
            &mut svc,
            &vault,
            &staging,
            &moss_dir,
            vec!["videos/clip.mov".to_string()],
        )
        .await;
        assert_eq!(spawner.captured_count(), 1);

        // A newer dispatch lands before this run reaches its first video.
        svc.cancellation.start_new_conversion();
        tokio::task::spawn_blocking(move || spawner.run_captured()).await.unwrap();

        assert!(!worker.slot_occupied(), "a run that delivered nothing must not enqueue a rebuild");
        crate::ops::watch::worker::deregister(&folder, &worker);
    }

    /// A dispatch that records the video's fingerprint has vouched for a source its
    /// run may never deliver. A run the user cancelled before it reached the video
    /// leaves the previous version's mp4 and poster in staging, where the next
    /// build's skip check finds them present under a fingerprint that matches — and
    /// carries the old video forward over a source that changed. The fingerprint is
    /// recorded when the run delivers, so that build queues the video again.
    #[tokio::test]
    async fn a_video_changed_under_a_cancelled_run_is_queued_again_by_the_next_build() {
        use moss_core::asset_paths;

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        let mut svc = BuildServices::headless();
        svc.session = Some(crate::system::folder_session::FolderSession::new(vault.clone()));

        let item = "videos/clip.mov".to_string();
        stage_and_prime_video(&svc, &vault, &staging, &item, b"clip bytes", false);
        std::fs::write(vault.join(&item), b"clip bytes, edited").unwrap();

        let (_, spawner) =
            dispatch_with_controlled_spawner(&mut svc, &vault, &staging, &moss_dir, vec![item.clone()]).await;
        assert_eq!(spawner.captured_count(), 1, "premise: a video whose source changed is queued");

        // The user cancels before the run reaches the video.
        svc.session.as_ref().unwrap().cancel.cancel();
        tokio::task::spawn_blocking(move || spawner.run_captured()).await.unwrap();
        assert_eq!(
            std::fs::read(staging.join(asset_paths::to_mp4(&item))).unwrap(),
            b"STAGED-SENTINEL",
            "premise: the run that left delivered nothing, so the old video is still staged"
        );

        let (_, spawner) =
            dispatch_with_controlled_spawner(&mut svc, &vault, &staging, &moss_dir, vec![item.clone()]).await;
        assert_eq!(spawner.captured_count(), 1, "the source changed and nothing ever delivered it: queue it again");
    }

    /// The other half: a run that delivers records the fingerprint it was dispatched under,
    /// so the next build carries the video forward instead of encoding it again. Without an
    /// ffmpeg the run ships the original as the mp4, which is a delivery.
    #[tokio::test]
    async fn a_video_a_run_delivered_is_carried_forward_by_the_next_build() {
        use moss_core::asset_paths;

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        let mut svc = BuildServices::headless();

        let item = "videos/clip.mov".to_string();
        // Staged and sourced as `stage_and_prime_video` leaves them, but no run has
        // vouched for the source yet.
        std::fs::create_dir_all(vault.join("videos")).unwrap();
        std::fs::write(vault.join(&item), b"clip bytes").unwrap();
        for key in [asset_paths::to_mp4(&item), asset_paths::to_thumb(&item)] {
            std::fs::create_dir_all(staging.join(&key).parent().unwrap()).unwrap();
            std::fs::write(staging.join(&key), b"STAGED-SENTINEL").unwrap();
        }

        let (_, spawner) =
            dispatch_with_controlled_spawner(&mut svc, &vault, &staging, &moss_dir, vec![item.clone()]).await;
        assert_eq!(spawner.captured_count(), 1, "premise: nothing has delivered this source, so it is queued");
        tokio::task::spawn_blocking(move || spawner.run_captured()).await.unwrap();
        assert_eq!(
            std::fs::read(staging.join(asset_paths::to_mp4(&item))).unwrap(),
            b"clip bytes",
            "premise: the run delivered the source as the mp4"
        );

        let (_, spawner) =
            dispatch_with_controlled_spawner(&mut svc, &vault, &staging, &moss_dir, vec![item.clone()]).await;
        assert_eq!(spawner.captured_count(), 0, "the run delivered this source: nothing to queue");
    }

    /// A video that leaves the site loses the fingerprint a run recorded for it: every
    /// dispatch prunes the records to the current set. Kept, it would vouch for the video
    /// when it returns — its outputs are still in staging, its size and second still
    /// match — though nothing watched the file while it was gone, and the store would
    /// hold a record for every video any folder ever had. So a video that left and came
    /// back is queued as the new video it is, and one that stayed is carried forward.
    #[tokio::test]
    async fn a_video_that_left_the_site_and_came_back_is_queued_again_and_one_that_stayed_is_not() {
        use moss_core::asset_paths;

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        let mut svc = BuildServices::headless();

        let (clip, other) = ("videos/clip.mov".to_string(), "videos/other.mov".to_string());
        stage_and_prime_video(&svc, &vault, &staging, &clip, b"clip bytes", false);
        stage_and_prime_video(&svc, &vault, &staging, &other, b"other bytes", false);

        let (_, spawner) =
            dispatch_with_controlled_spawner(&mut svc, &vault, &staging, &moss_dir, vec![other.clone()]).await;
        assert_eq!(spawner.captured_count(), 0, "premise: the video that stayed is unchanged, so nothing is queued");

        let (_, spawner) =
            dispatch_with_controlled_spawner(&mut svc, &vault, &staging, &moss_dir, vec![clip.clone(), other.clone()]).await;

        assert_eq!(spawner.captured_count(), 1, "the video that came back was carried forward on its old record");
        let queued = svc.cancellation.protected_outputs();
        assert!(queued.contains(&asset_paths::to_mp4(&clip)), "the video that came back is what the run converts");
        assert!(!queued.contains(&asset_paths::to_mp4(&other)), "the video that stayed was queued again");
    }

    /// A staged output that cannot be checked is neither present nor missing.
    /// Dispatching it would re-encode a video whose bytes may be fine, and
    /// carrying it forward would register bytes nobody read — so the dispatch
    /// does neither and reports both keys `Unverified`, which withholds the
    /// generation instead.
    #[cfg(unix)]
    #[tokio::test]
    async fn an_unreadable_staged_video_is_reported_not_dispatched() {
        use moss_core::asset_paths;
        use std::os::unix::fs::PermissionsExt;

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        let mut svc = BuildServices::headless();

        let item = "videos/clip.mov".to_string();
        stage_and_prime_video(&svc, &vault, &staging, &item, b"clip bytes", false);
        let videos_dir = staging.join("videos");
        std::fs::set_permissions(&videos_dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&videos_dir).is_ok() {
            std::fs::set_permissions(&videos_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            eprintln!("skipped: this process can read a 0o000 directory (running as root?)");
            return;
        }

        let (sealed, spawner) =
            dispatch_with_controlled_spawner(&mut svc, &vault, &staging, &moss_dir, vec![item.clone()]).await;
        std::fs::set_permissions(&videos_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(spawner.captured_count(), 0, "an unverifiable video must not be sent to the encoder");
        assert!(sealed.files().is_empty(), "nothing unread may be registered: {:?}", sealed.files());
        for key in [asset_paths::to_mp4(&item), asset_paths::to_thumb(&item)] {
            assert!(
                sealed.unverified().contains_key(&key),
                "{key} must be reported unverified: {:?}",
                sealed.unverified()
            );
        }
    }

    /// (a) The data-loss guard this whole change exists for: a vault with 3
    /// already-converted videos, adding a 4th, must dispatch ONLY the new
    /// one. Before per-item fingerprinting, adding any one video invalidated
    /// the WHOLE-SET fingerprint and re-dispatched all four; combined with
    /// seal()'s `video_outputs.retain` pruning untouched keys, a round that
    /// dispatches in the background could seal before re-registering the
    /// three untouched videos and the stale sweep would then delete their
    /// physically-present files. Also carries forward the HLS-ladder-survival
    /// property the whole-set predecessor test covered (video #1).
    #[tokio::test]
    async fn a_new_video_only_dispatches_the_new_one_others_survive_seal_and_stale_sweep() {
        use crate::build::media::pipeline::{compute_expected_dirs, remove_stale_dirs, remove_stale_files};
        use moss_core::asset_paths;

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");

        let mut svc = BuildServices::headless();

        let item1 = "videos/one.mov".to_string();
        let item2 = "videos/two.mov".to_string();
        let item3 = "videos/three.mov".to_string();
        let item4 = "videos/four.mov".to_string(); // brand new: never staged or primed

        let mut untouched = stage_and_prime_video(&svc, &vault, &staging, &item1, b"one-bytes", true);
        untouched.extend(stage_and_prime_video(&svc, &vault, &staging, &item2, b"two-bytes", false));
        untouched.extend(stage_and_prime_video(&svc, &vault, &staging, &item3, b"three-bytes", false));
        std::fs::write(vault.join(&item4), b"four-bytes").unwrap();

        // The core seal below must not wait for item4's encode — proven by
        // never calling `spawner.run_captured()` until after every stale-
        // sweep assertion has already run against the SEALED view.
        let (sealed, spawner) = dispatch_with_controlled_spawner(
            &mut svc,
            &vault,
            &staging,
            &moss_dir,
            vec![item1.clone(), item2.clone(), item3.clone(), item4.clone()],
        )
        .await;
        assert_eq!(
            spawner.captured_count(),
            1,
            "exactly one deferred encode (item4) must have been dispatched"
        );

        let view = sealed.site_hashes_view();
        remove_stale_files(&staging, view, "test", &crate::build::lifecycle::permit_for_test());
        remove_stale_dirs(&staging, &compute_expected_dirs(view), &crate::build::lifecycle::permit_for_test());

        for key in &untouched {
            let abs = staging.join(key);
            assert!(
                abs.exists(),
                "'{}' (untouched video output) was deleted by the stale sweep after a \
                 sibling video was added — sealed video_outputs: {:?}",
                key,
                view.video_outputs
            );
            assert_eq!(
                std::fs::read(&abs).unwrap(),
                b"STAGED-SENTINEL",
                "'{}' content changed — it was re-dispatched even though its own \
                 fingerprint and outputs were unchanged",
                key
            );
        }

        // item4 has no prior output, so the seal above shipped without it —
        // "current" is usable (every other video survived) even though this
        // one video never finished. Only now does the encode run.
        let new_mp4 = staging.join(asset_paths::to_mp4(&item4));
        assert!(!new_mp4.exists(), "the new video must not exist before its encode runs");
        spawner.run_captured();
        assert_eq!(
            std::fs::read(&new_mp4).unwrap(),
            b"four-bytes",
            "the new video must have been dispatched and produced an mp4 output"
        );
    }

    /// (b) Editing one video's bytes must dispatch only that video.
    #[tokio::test]
    async fn a_changed_videos_bytes_only_dispatches_that_video() {
        use moss_core::asset_paths;

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");

        let mut svc = BuildServices::headless();

        let item1 = "videos/one.mov".to_string();
        let item2 = "videos/two.mov".to_string();

        let staged1 = stage_and_prime_video(&svc, &vault, &staging, &item1, b"one-bytes", false);
        let staged2 = stage_and_prime_video(&svc, &vault, &staging, &item2, b"two-bytes", false);

        // A re-encode, not an add/remove: video #2's SOURCE bytes change.
        std::fs::write(vault.join(&item2), b"two-bytes-CHANGED").unwrap();

        let (sealed, spawner) = dispatch_with_controlled_spawner(
            &mut svc,
            &vault,
            &staging,
            &moss_dir,
            vec![item1.clone(), item2.clone()],
        )
        .await;

        // Video #2's OLD output is still what's on disk — its re-encode
        // hasn't run yet — and the core seal must carry it forward as this
        // build's own output too, exactly like an unchanged video, so a
        // stale sweep run against THIS seal (as the next build's
        // `pipeline::sweep_staging` does, off the `hashes.json` this seal
        // writes) does not delete a video that is still serving fine
        // mid-re-encode.
        let view = sealed.site_hashes_view();
        for key in &staged2 {
            assert!(
                view.video_outputs.contains(key),
                "'{}' (mid-re-encode video's OLD output) must be carried forward into the \
                 core seal — sealed video_outputs: {:?}",
                key,
                view.video_outputs
            );
        }
        use crate::build::media::pipeline::{compute_expected_dirs, remove_stale_dirs, remove_stale_files};
        remove_stale_files(&staging, view, "test", &crate::build::lifecycle::permit_for_test());
        remove_stale_dirs(&staging, &compute_expected_dirs(view), &crate::build::lifecycle::permit_for_test());
        for key in &staged2 {
            assert_eq!(
                std::fs::read(staging.join(key)).unwrap(),
                b"STAGED-SENTINEL",
                "'{}' (mid-re-encode video's OLD output) must survive the seal's stale-cleanup \
                 byte-identically while the new encode is still running",
                key
            );
        }
        for key in &staged1 {
            assert_eq!(
                std::fs::read(staging.join(key)).unwrap(),
                b"STAGED-SENTINEL",
                "'{}' (unchanged video) must not be re-dispatched when a SIBLING video's bytes change",
                key
            );
        }

        // Only now does the deferred encode run.
        spawner.run_captured();
        let mp4_2 = staging.join(asset_paths::to_mp4(&item2));
        assert_eq!(
            std::fs::read(&mp4_2).unwrap(),
            b"two-bytes-CHANGED",
            "the changed video must be re-dispatched and its mp4 must reflect the new bytes"
        );
    }

    /// (c) A missing required output with nothing in the object store to
    /// relink (the 71ee43bc1c gap, now per item) must re-dispatch only the
    /// video whose output vanished.
    #[tokio::test]
    async fn a_missing_output_only_dispatches_that_video() {
        use moss_core::asset_paths;

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");

        let mut svc = BuildServices::headless();

        let item1 = "videos/one.mov".to_string();
        let item2 = "videos/two.mov".to_string();

        let staged1 = stage_and_prime_video(&svc, &vault, &staging, &item1, b"one-bytes", false);
        let _staged2 = stage_and_prime_video(&svc, &vault, &staging, &item2, b"two-bytes", false);

        // Video #2's mp4 vanished (a stale sweep / crash artifact) even
        // though its source never changed.
        let mp4_2 = staging.join(asset_paths::to_mp4(&item2));
        std::fs::remove_file(&mp4_2).unwrap();

        let epoch_before = svc.cancellation.current_id();
        let (_sealed, spawner) = dispatch_with_controlled_spawner(
            &mut svc,
            &vault,
            &staging,
            &moss_dir,
            vec![item1.clone(), item2.clone()],
        )
        .await;

        assert!(
            svc.cancellation.current_id() > epoch_before,
            "a video with a matched fingerprint but a missing required output must still re-dispatch"
        );
        for key in &staged1 {
            assert_eq!(
                std::fs::read(staging.join(key)).unwrap(),
                b"STAGED-SENTINEL",
                "'{}' (unaffected video) must not be re-dispatched when a SIBLING video's output is missing",
                key
            );
        }
        spawner.run_captured();
        assert_eq!(
            std::fs::read(&mp4_2).unwrap(),
            b"two-bytes",
            "the missing output must be self-healed (recreated) by the re-dispatch"
        );
    }

    /// (d) Removing a video from the vault must clean only its own outputs;
    /// every other video's outputs survive the seal and the stale sweep.
    #[tokio::test]
    async fn a_removed_video_is_cleaned_others_survive() {
        use crate::build::media::pipeline::{compute_expected_dirs, remove_stale_dirs, remove_stale_files};

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");

        let mut svc = BuildServices::headless();
        svc.spawner = Some(std::sync::Arc::new(crate::build::ports::spawner::TokioSpawner));

        let item1 = "videos/one.mov".to_string();
        let item2 = "videos/two.mov".to_string();

        let staged1 = stage_and_prime_video(&svc, &vault, &staging, &item1, b"one-bytes", false);
        let staged2 = stage_and_prime_video(&svc, &vault, &staging, &item2, b"two-bytes", false);

        // Video #2 is gone from the vault, and therefore from this build's
        // video_items — dispatch never even sees it.
        std::fs::remove_file(vault.join(&item2)).unwrap();

        let sealed = dispatch_and_seal(&svc, &vault, &staging, &moss_dir, vec![item1.clone()]).await;
        let view = sealed.site_hashes_view();
        remove_stale_files(&staging, view, "test", &crate::build::lifecycle::permit_for_test());
        remove_stale_dirs(&staging, &compute_expected_dirs(view), &crate::build::lifecycle::permit_for_test());

        for key in &staged1 {
            assert!(
                staging.join(key).exists(),
                "'{}' (still-live video) must survive a sibling video's removal",
                key
            );
            assert_eq!(std::fs::read(staging.join(key)).unwrap(), b"STAGED-SENTINEL");
        }
        for key in &staged2 {
            assert!(
                !staging.join(key).exists(),
                "'{}' (removed video's output) must be cleaned by the stale sweep",
                key
            );
        }
    }

    // ── The seal/video split: an in-flight encode must not hold the seal ──

    /// When no rebuild worker is registered for the folder — a one-shot
    /// `moss build`, `moss deploy`, or a plugin install, none of which stick
    /// around to pick up a later "enqueue a rebuild" — the split must not
    /// apply: the seal waits for the video exactly as it did before this
    /// change, because nothing else will ever converge it. Without this
    /// guard, a one-shot publish could adopt a manifest missing (or carrying
    /// stale bytes for) a video that then never gets encoded at all in that
    /// process's lifetime — a permanently broken link on a published site.
    #[tokio::test]
    async fn a_build_with_no_registered_worker_waits_for_the_video_before_sealing() {
        // Deliberately NOT `dispatch_and_seal` / `dispatch_with_controlled_spawner`
        // — both ensure a worker is registered so the split applies, which is
        // exactly the one thing this test must not have. Inlines the same
        // dispatch-and-drain shape without that step.
        use crate::build::coordinator::test_utils;
        use crate::types::content::SiteHashes;
        use crate::types::services::BackgroundContext;
        use moss_core::asset_paths;

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");

        let mut svc = BuildServices::headless();
        svc.spawner = Some(std::sync::Arc::new(crate::build::ports::spawner::TokioSpawner));

        let item = "videos/clip.mov".to_string();
        std::fs::create_dir_all(vault.join(&item).parent().unwrap()).unwrap();
        std::fs::write(vault.join(&item), b"clip-bytes").unwrap();

        let folder = vault.display().to_string();
        assert!(
            crate::ops::watch::worker::get(&folder).is_none(),
            "precondition: no worker registered for this folder"
        );

        let ctx = BackgroundContext {
            video_items: vec![item.clone()],
            source_path: folder.clone(),
            staging_dir: staging.clone(),
            moss_dir: moss_dir.clone(),
            ffmpeg_bin_path: Some(moss_dir.join("no-such-ffmpeg").display().to_string()),
            ..BackgroundContext::for_test()
        };
        let (tx, rx) = test_utils::build_test_coordinator();
        let svc_for_dispatch = svc.clone();
        tokio::task::spawn_blocking(move || {
            dispatch_video_conversions(Some(&svc_for_dispatch), ctx, Some(tx));
        })
        .await
        .unwrap();
        // No worker was ever registered, so the fallback (unsplit) path
        // applies: this must resolve once the encode actually finishes, not
        // hang — a bounded timeout distinguishes "waited correctly" from "wedged".
        let sealed = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            test_utils::drain_into_sealed(rx, SiteHashes::default()),
        )
        .await
        .expect("with no watcher, the seal must still complete once the encode finishes");

        let mp4 = staging.join(asset_paths::to_mp4(&item));
        assert_eq!(
            std::fs::read(&mp4).unwrap(),
            b"clip-bytes",
            "with no watcher to pick up a later rebuild, the seal must wait for the encode"
        );
        assert!(
            sealed.files().contains_key(&asset_paths::to_mp4(&item)),
            "the sealed manifest must register the video's real output, not carry nothing forward"
        );
    }

    /// A build whose one video never finishes still seals and leaves a
    /// usable `current` — the whole point of this change. The deferred
    /// encode's captured task is never run; if the core seal still depended
    /// on it, `dispatch_with_controlled_spawner`'s bounded timeout would
    /// panic instead of the assertions below ever running.
    #[tokio::test]
    async fn a_video_that_never_finishes_still_seals_and_leaves_a_usable_current() {
        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");

        let mut svc = BuildServices::headless();
        let item = "videos/slow.mov".to_string();
        std::fs::create_dir_all(vault.join(&item).parent().unwrap()).unwrap();
        std::fs::write(vault.join(&item), b"slow-source-bytes").unwrap();

        let (sealed, spawner) = dispatch_with_controlled_spawner(
            &mut svc,
            &vault,
            &staging,
            &moss_dir,
            vec![item.clone()],
        )
        .await;

        // Nothing panicked and nothing hung: the seal completed. A
        // first-ever video has no prior output to carry forward, so it is
        // correctly absent from this generation rather than half-written —
        // "usable" means the rest of the site shipped, not that this one
        // brand-new video did.
        assert!(sealed.files().is_empty(), "a first-ever video has nothing to register yet");
        assert_eq!(spawner.captured_count(), 1, "the encode was dispatched, just never run");
    }

    /// The video's later completion updates the page: once the deferred
    /// encode lands, it asks for exactly one follow-up rebuild through the
    /// same lever a file edit already drives (`ops::watch::worker`) — the
    /// settle → re-render path picks it up from there, unchanged by this
    /// fix. Inspects the worker's queue slot directly rather than running a
    /// real rebuild: this test is about the SIGNAL, not the rebuild itself.
    #[tokio::test]
    async fn the_videos_later_completion_enqueues_exactly_one_rebuild() {
        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        let folder = vault.display().to_string();

        let mut svc = BuildServices::headless();
        let item = "videos/clip.mov".to_string();
        std::fs::create_dir_all(vault.join(&item).parent().unwrap()).unwrap();
        std::fs::write(vault.join(&item), b"clip-bytes").unwrap();

        let worker = crate::ops::watch::worker::register(&folder);

        let (_sealed, spawner) = dispatch_with_controlled_spawner(
            &mut svc,
            &vault,
            &staging,
            &moss_dir,
            vec![item.clone()],
        )
        .await;
        assert!(
            worker.take().is_none(),
            "the seal must not enqueue a rebuild before the deferred encode has run"
        );

        spawner.run_captured();
        assert_eq!(
            worker.take(),
            Some(crate::ops::watch::worker::RebuildRequest::full()),
            "the video's completion must enqueue exactly one follow-up rebuild"
        );

        crate::ops::watch::worker::deregister(&folder, &worker);
    }

    /// A build with no videos is unchanged: the early return at the top of
    /// `dispatch_video_conversions` fires before any of this change's
    /// machinery (carry-forward registration, the `tx` drop, the spawner)
    /// runs at all.
    #[tokio::test]
    async fn a_build_with_no_videos_is_unchanged() {
        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");

        let mut svc = BuildServices::headless();
        let (sealed, spawner) =
            dispatch_with_controlled_spawner(&mut svc, &vault, &staging, &moss_dir, Vec::new()).await;

        assert!(sealed.files().is_empty());
        assert_eq!(spawner.captured_count(), 0, "no video items means no encode is ever dispatched");
    }

    /// Negative case: when staging files are absent, keys are NOT emitted.
    #[tokio::test]
    async fn emit_video_outputs_skips_keys_when_staging_files_absent() {
        use crate::build::coordinator::test_utils;
        use crate::types::content::SiteHashes;
        use moss_core::asset_paths;

        let tmp = portable_tmpdir();
        let staging = tmp.path().to_path_buf();

        let source = "videos/missing.mov";
        let mp4_key = asset_paths::to_mp4(source);
        let thumb_key = asset_paths::to_thumb(source);
        // Deliberately do NOT create the staging files.

        let (tx, rx) = test_utils::build_test_coordinator();
        let paths = vec![(mp4_key.clone(), None), (thumb_key.clone(), None)];
        // blocking_send requires a non-async thread.
        tokio::task::spawn_blocking(move || {
            emit_video_outputs_via_channel(&Some(tx), &paths, &staging);
        })
        .await
        .unwrap();
        let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

        let files = sealed.files();
        assert!(
            !files.contains_key(&mp4_key),
            "absent mp4 must not be registered; got files: {:?}",
            files.keys().collect::<Vec<_>>()
        );
        assert!(
            !files.contains_key(&thumb_key),
            "absent thumbnail must not be registered; got files: {:?}",
            files.keys().collect::<Vec<_>>()
        );
    }

    /// Content-hash correctness: the emitted manifest hash must equal
    /// `file_entry(compute_binary_hash(bytes))` for the actual file bytes —
    /// NOT the empty-hash sentinel `"100644:"`.
    ///
    /// Also verifies that DIFFERENT bytes produce a DIFFERENT manifest hash
    /// (proves changed-video detection: a re-encoded video with the same name
    /// will be detected as changed and re-uploaded on the next deploy).
    #[tokio::test]
    async fn emit_video_outputs_hash_is_real_and_content_sensitive() {
        use crate::build::assets::paths::compute_binary_hash;
        use crate::build::coordinator::test_utils;
        use crate::types::content::{file_entry, SiteHashes};
        use moss_core::asset_paths;

        // ── First video: bytes b"AAAA" ──────────────────────────────────────
        let tmp_a = portable_tmpdir();
        let staging_a = tmp_a.path().to_path_buf();

        let source = "videos/talk.mov";
        let mp4_key = asset_paths::to_mp4(source);
        let thumb_key = asset_paths::to_thumb(source);

        let mp4_abs = staging_a.join(&mp4_key);
        std::fs::create_dir_all(mp4_abs.parent().unwrap()).unwrap();
        std::fs::write(&mp4_abs, b"AAAA").unwrap();
        let thumb_abs = staging_a.join(&thumb_key);
        std::fs::write(&thumb_abs, b"AAAA").unwrap();

        let (tx_a, rx_a) = test_utils::build_test_coordinator();
        let paths = vec![(mp4_key.clone(), None), (thumb_key.clone(), None)];
        let staging_a_clone = staging_a.clone();
        tokio::task::spawn_blocking(move || {
            emit_video_outputs_via_channel(&Some(tx_a), &paths, &staging_a_clone);
        })
        .await
        .unwrap();
        let sealed_a = test_utils::drain_into_sealed(rx_a, SiteHashes::default()).await;
        let files_a = sealed_a.files();

        let expected_hash_aaaa = file_entry(&compute_binary_hash(b"AAAA"));

        let mp4_entry_a = files_a.get(&mp4_key).expect("mp4 key must be present");
        assert_eq!(
            mp4_entry_a,
            &expected_hash_aaaa,
            "mp4 hash for b\"AAAA\" must equal file_entry(compute_binary_hash(b\"AAAA\")); \
             got {:?} — empty-hash sentinel \"100644:\" is a bug",
            mp4_entry_a
        );

        let thumb_entry_a = files_a.get(&thumb_key).expect("thumb key must be present");
        assert_eq!(
            thumb_entry_a,
            &expected_hash_aaaa,
            "thumb hash for b\"AAAA\" must equal file_entry(compute_binary_hash(b\"AAAA\")); \
             got {:?} — empty-hash sentinel \"100644:\" is a bug",
            thumb_entry_a
        );

        // ── Second video: bytes b"BBBB" — must produce a DIFFERENT hash ─────
        let tmp_b = portable_tmpdir();
        let staging_b = tmp_b.path().to_path_buf();

        let mp4_abs_b = staging_b.join(&mp4_key);
        std::fs::create_dir_all(mp4_abs_b.parent().unwrap()).unwrap();
        std::fs::write(&mp4_abs_b, b"BBBB").unwrap();
        let thumb_abs_b = staging_b.join(&thumb_key);
        std::fs::write(&thumb_abs_b, b"BBBB").unwrap();

        let (tx_b, rx_b) = test_utils::build_test_coordinator();
        let paths_b = vec![(mp4_key.clone(), None), (thumb_key.clone(), None)];
        let staging_b_clone = staging_b.clone();
        tokio::task::spawn_blocking(move || {
            emit_video_outputs_via_channel(&Some(tx_b), &paths_b, &staging_b_clone);
        })
        .await
        .unwrap();
        let sealed_b = test_utils::drain_into_sealed(rx_b, SiteHashes::default()).await;
        let files_b = sealed_b.files();

        let expected_hash_bbbb = file_entry(&compute_binary_hash(b"BBBB"));

        let mp4_entry_b = files_b.get(&mp4_key).expect("mp4 key (BBBB) must be present");
        assert_ne!(
            mp4_entry_b, mp4_entry_a,
            "different bytes (AAAA vs BBBB) must produce different manifest hashes; \
             both got {:?} — empty-hash sentinel is a bug (changed videos would never \
             be detected as changed on re-deploy)",
            mp4_entry_b
        );
        assert_eq!(
            mp4_entry_b,
            &expected_hash_bbbb,
            "mp4 hash for b\"BBBB\" must equal file_entry(compute_binary_hash(b\"BBBB\")); \
             got {:?}",
            mp4_entry_b
        );
    }

    // ── The honest mirror: a failure the author has to hear ───────────────
    // Both tests below drive the real worker against a real vault, because both
    // defects were in the WIRING — each unit they sit in already returned the
    // right value, and the call site dropped it (`docs/reference/proving-a-change.md`:
    // "watch, rebuild, cache, atomic write, path handling → Rust test + a run
    // against a real vault").

    /// The progressive MP4 is the floor under every can't-encode path. When
    /// even the fallback copy fails, nothing at all reaches the site — and that
    /// is a strictly worse fact than whatever stopped the encode. It must reach
    /// the author, which means a `Blocking` advisory (and so a *failed* media
    /// Job), not a `log::warn!` beside a Job that says it succeeded.
    #[test]
    fn a_video_that_could_not_even_be_copied_fails_the_media_job() {
        use crate::tasks::{TaskScope, TaskState, WindowId};
        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        std::fs::create_dir_all(vault.join("videos")).unwrap();
        std::fs::write(vault.join("videos/clip.mov"), b"bytes that are not a video").unwrap();
        // The fallback copies to `<staging>/videos/clip.mp4`. A plain FILE where
        // that path's parent directory belongs makes the copy fail for real —
        // the read-only-output / disk-full shape, reproducible without root.
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("videos"), b"in the way").unwrap();

        let (svc, registry) = BuildServices::with_task_registry();
        let ctx = BackgroundContext {
            video_items: vec!["videos/clip.mov".to_string()],
            source_path: vault.display().to_string(),
            staging_dir: staging.clone(),
            moss_dir: tmp.path().join(".moss"),
            // A binary that is not there: every encode fails, so the item takes
            // a can't-encode path and lands in `shipped_original`.
            ffmpeg_bin_path: Some(tmp.path().join("no-such-ffmpeg").display().to_string()),
            ..BackgroundContext::for_test()
        };
        run_video_conversion(&svc, &ctx, 0, None);

        assert!(
            !staging.join("videos/clip.mp4").exists(),
            "precondition: the fallback copy must really have failed"
        );
        let tasks = registry.tasks(&WindowId::from("main"), TaskScope::Preview);
        let child = tasks
            .iter()
            .find(|t| t.parent.is_some())
            .expect("a video that shipped nothing is real work — it must mint a media Job");
        match &child.state {
            TaskState::Failed { advisory } => {
                assert_eq!(advisory.severity, Severity::Blocking);
                // The site-relative path, not the basename: a click has to
                // resolve this against the open folder and find the real file.
                assert_eq!(advisory.item.as_deref(), Some("videos/clip.mov"));
            }
            TaskState::Succeeded { advisories, .. } => panic!(
                "no bytes reached the site and the media Job says it SUCCEEDED; \
                 the author hears only {:?}",
                advisories.iter().map(|a| &a.what).collect::<Vec<_>>()
            ),
            other => panic!("expected a failed media Job, got {other:?}"),
        }
    }

    /// Shared rig for the three "staging is obstructed" tests below:
    /// synthesise a source, populate a cache hit when `cached`, then put a
    /// plain FILE at `staging/<obstruct>` so a later `link_to`'s
    /// `create_dir_all` fails for real. `relative_thumb` lets the poster test
    /// target `staging/posters` instead of `staging/videos`. `None` means
    /// ffmpeg isn't on PATH or couldn't synthesise — callers early-return.
    fn convert_with_staging_obstructed(
        cached: bool,
        relative_thumb: &str,
        obstruct: &str,
    ) -> Option<VideoConversionOutcome> {
        use crate::build::cache::{ObjectStore, TransformCache, TransformEntry, TransformRecord};
        use crate::build::media::ffmpeg::{real_ffmpeg, synthesise, FFmpegManager, VideoCompressionConfig};

        let bin = real_ffmpeg()?;
        let tmp = portable_tmpdir();
        let source = tmp.path().join("clip.mov");
        if !synthesise(&bin, &source, "320x240") {
            return None;
        }

        let objects = ObjectStore::new(tmp.path().join("objects"));
        let transforms = TransformCache::new(
            tmp.path().join("transforms"),
            ObjectStore::new(tmp.path().join("objects")),
        );
        let config = VideoCompressionConfig::default();
        let mp4_params = config.to_params();
        let thumb_params = serde_json::json!({});

        if cached {
            // Both blobs are real and present, so `find_cached_output` takes
            // this as a hit — the failure has to come from staging.
            let mp4_oid = objects.store_file(&source).expect("store the cached mp4");
            let poster = tmp.path().join("poster.jpg");
            std::fs::write(&poster, b"not really a jpeg, just non-empty").unwrap();
            let thumb_oid = objects.store_file(&poster).expect("store the cached poster");
            transforms
                .put(&TransformRecord {
                    source_oid: "oid-clip".to_string(),
                    source_size: std::fs::metadata(&source).unwrap().len(),
                    transforms: std::collections::HashMap::from([
                        (
                            "video/mp4".to_string(),
                            TransformEntry { oid: mp4_oid, size: 1, params: mp4_params.clone() },
                        ),
                        (
                            "video/thumbnail".to_string(),
                            TransformEntry { oid: thumb_oid, size: 1, params: thumb_params.clone() },
                        ),
                    ]),
                })
                .expect("write the transform record");
        }

        let staging = tmp.path().join("stage");
        let temp = tmp.path().join("temp");
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join(obstruct), b"in the way").unwrap();

        Some(convert_single_video(
            &FFmpegManager::from_bin_path(bin),
            &source,
            "oid-clip",
            "videos/clip.mp4",
            relative_thumb,
            &temp,
            &staging,
            &objects,
            &transforms,
            &config,
            &mp4_params,
            &thumb_params,
            None,
            None,
            None,
        ))
    }

    /// A cache hit whose MP4 blob cannot reach staging is not a hit. It used
    /// to report success with nothing on disk: the registry said `Ready`, the
    /// manifest skipped the absent file, and the staged HTML kept a
    /// `<source>` pointing at nothing.
    #[test]
    fn a_cache_hit_whose_mp4_cannot_reach_staging_is_a_failed_conversion() {
        let Some(outcome) = convert_with_staging_obstructed(true, "videos/clip.thumb.jpg", "videos") else {
            eprintln!("skipping: ffmpeg unavailable or synthesis failed");
            return;
        };
        assert!(outcome.error.is_some(), "a hit that never reached staging must not be reported as success");
    }

    /// A freshly encoded MP4 that cannot reach staging is not a success
    /// either — the video only exists in a temp file the build is about to
    /// delete.
    #[test]
    fn an_encoded_mp4_that_cannot_reach_staging_is_a_failed_conversion() {
        let Some(outcome) = convert_with_staging_obstructed(false, "videos/clip.thumb.jpg", "videos") else {
            eprintln!("skipping: ffmpeg unavailable or synthesis failed");
            return;
        };
        assert!(outcome.error.is_some(), "an encode that never reached staging must not be reported as success");
    }

    /// The poster is optional, the MP4 is not: a cache hit whose MP4 links
    /// fine but whose poster cannot must still succeed, and must not claim a
    /// poster URL that never reached staging.
    #[test]
    fn a_cache_hit_whose_poster_cannot_reach_staging_still_succeeds_without_one() {
        let Some(outcome) = convert_with_staging_obstructed(true, "posters/clip.thumb.jpg", "posters") else {
            eprintln!("skipping: ffmpeg unavailable or synthesis failed");
            return;
        };
        assert!(outcome.error.is_none());
        assert!(!outcome.poster);
    }

    // ------------------------------------------------------------------
    // Phase 1b: thread the just-staged CAS oid through to the manifest
    // ------------------------------------------------------------------

    /// Mirrors `ship_phase_ships_correct_bytes_from_cas_despite_stage_dir_
    /// being_overwritten` (`ship.rs`) for the video worker's own encode path:
    /// `convert_single_video` now carries its mp4's CAS oid out to the
    /// manifest instead of discarding it (the mechanical `oid: None` this
    /// whole change closes the gap on), so `ship_phase` can read the
    /// immutable blob instead of the mutable stage path a concurrent build
    /// can rewrite between seal and ship. Driven through the real
    /// `dispatch_video_conversions` producer end to end, with a real ffmpeg
    /// encode — a hand-built manifest never acquires a `staged_oid` in the
    /// first place and would prove nothing about this fix.
    ///
    /// Deliberately does NOT register a rebuild worker (unlike
    /// `dispatch_and_seal`): with one registered, `dispatch_video_
    /// conversions` takes the seal/video split and returns before the encode
    /// finishes, and this test needs the sealed manifest to already carry the
    /// real output's oid.
    #[tokio::test]
    async fn ship_phase_ships_a_freshly_encoded_mp4_from_cas_despite_stage_overwrite() {
        use crate::build::coordinator::test_utils;
        use crate::build::media::ffmpeg::{real_ffmpeg, synthesise};
        use crate::types::content::SiteHashes;
        use crate::types::services::BackgroundContext;
        use moss_core::asset_paths;

        let Some(bin) = real_ffmpeg() else {
            eprintln!("skipping: ffmpeg unavailable");
            return;
        };

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        let item = "videos/clip.mov".to_string();
        std::fs::create_dir_all(vault.join(&item).parent().unwrap()).unwrap();
        if !synthesise(&bin, &vault.join(&item), "320x240") {
            eprintln!("skipping: ffmpeg synthesis failed");
            return;
        }

        let mut svc = BuildServices::headless();
        svc.spawner = Some(std::sync::Arc::new(crate::build::ports::spawner::TokioSpawner));

        let folder = vault.display().to_string();
        assert!(
            crate::ops::watch::worker::get(&folder).is_none(),
            "precondition: no worker registered, so the seal waits for the real encode"
        );

        let ctx = BackgroundContext {
            video_items: vec![item.clone()],
            source_path: folder.clone(),
            staging_dir: staging.clone(),
            moss_dir: moss_dir.clone(),
            ffmpeg_bin_path: Some(bin.clone()),
            ..BackgroundContext::for_test()
        };
        let (tx, rx) = test_utils::build_test_coordinator();
        let svc_for_dispatch = svc.clone();
        tokio::task::spawn_blocking(move || {
            dispatch_video_conversions(Some(&svc_for_dispatch), ctx, Some(tx));
        })
        .await
        .unwrap();
        let sealed = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            test_utils::drain_into_sealed(rx, SiteHashes::default()),
        )
        .await
        .expect("the real encode must finish within the timeout");

        let mp4_key = asset_paths::to_mp4(&item);
        let oid = sealed
            .staged_oid(&mp4_key)
            .expect("a freshly encoded mp4 must carry a live staged_oid")
            .to_string();

        let object_store = crate::build::cache::ObjectStore::new(
            MossPaths::from_moss_dir(moss_dir.clone()).cache_objects(),
        );
        let cas_bytes =
            std::fs::read(object_store.get_path(&oid).expect("the oid must name a live CAS blob")).unwrap();

        // A concurrent build rewrites the mutable stage copy after this
        // manifest's oid was sealed.
        std::fs::write(staging.join(&mp4_key), b"CONCURRENT-OVERWRITE").unwrap();

        let site = tmp.path().join("site");
        crate::build::ship::ship_phase(&staging, &site, &sealed, Some(&object_store), None)
            .expect("ship_phase should succeed");

        let shipped = std::fs::read(site.join(&mp4_key)).unwrap();
        assert_eq!(
            shipped, cas_bytes,
            "ship_phase must ship the CAS blob's bytes, not the concurrently \
             overwritten mutable stage copy"
        );
        assert_ne!(
            shipped,
            b"CONCURRENT-OVERWRITE".to_vec(),
            "premise: the overwrite really changed the stage bytes"
        );
    }

    /// Regression guard: if a future refactor drops the oid on `Delivery`'s
    /// way into `emit_video_outputs_via_channel`, this must fail loudly
    /// rather than quietly reverting every video entry to the pre-fix,
    /// fingerprint-only ship. Covers both the mp4 (`stage_mp4`'s
    /// `objects.link_to`) and the poster (`objects.store_file` + `link_to`),
    /// the two link_to call sites a fresh (non-cached) encode drives.
    #[tokio::test]
    async fn run_video_conversion_always_records_a_staged_oid_for_mp4_and_poster() {
        use crate::build::coordinator::test_utils;
        use crate::build::media::ffmpeg::{real_ffmpeg, synthesise};
        use crate::types::content::SiteHashes;
        use crate::types::services::BackgroundContext;
        use moss_core::asset_paths;

        let Some(bin) = real_ffmpeg() else {
            eprintln!("skipping: ffmpeg unavailable");
            return;
        };

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        let item = "videos/clip.mov".to_string();
        std::fs::create_dir_all(vault.join(&item).parent().unwrap()).unwrap();
        if !synthesise(&bin, &vault.join(&item), "320x240") {
            eprintln!("skipping: ffmpeg synthesis failed");
            return;
        }

        let mut svc = BuildServices::headless();
        svc.spawner = Some(std::sync::Arc::new(crate::build::ports::spawner::TokioSpawner));

        let folder = vault.display().to_string();
        let ctx = BackgroundContext {
            video_items: vec![item.clone()],
            source_path: folder,
            staging_dir: staging.clone(),
            moss_dir: moss_dir.clone(),
            ffmpeg_bin_path: Some(bin),
            ..BackgroundContext::for_test()
        };
        let (tx, rx) = test_utils::build_test_coordinator();
        let svc_for_dispatch = svc.clone();
        tokio::task::spawn_blocking(move || {
            dispatch_video_conversions(Some(&svc_for_dispatch), ctx, Some(tx));
        })
        .await
        .unwrap();
        let sealed = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            test_utils::drain_into_sealed(rx, SiteHashes::default()),
        )
        .await
        .expect("the real encode must finish within the timeout");

        let mp4_key = asset_paths::to_mp4(&item);
        let thumb_key = asset_paths::to_thumb(&item);
        assert!(sealed.files().contains_key(&mp4_key), "premise: mp4 registered at all");
        assert!(sealed.files().contains_key(&thumb_key), "premise: poster registered at all");

        let object_store = crate::build::cache::ObjectStore::new(
            MossPaths::from_moss_dir(moss_dir.clone()).cache_objects(),
        );
        for key in [&mp4_key, &thumb_key] {
            let oid = sealed
                .staged_oid(key)
                .unwrap_or_else(|| panic!("the main encode path must record a staged_oid for '{key}'"))
                .to_string();
            let cas_bytes =
                std::fs::read(object_store.get_path(&oid).expect("the oid must name a live CAS blob")).unwrap();
            assert_eq!(
                cas_bytes,
                std::fs::read(staging.join(key)).unwrap(),
                "the staged_oid for '{key}' must back exactly the bytes on disk"
            );
        }
    }

    /// The carry-forward skip path's other half: the fingerprint-matched
    /// `skip_paths` branch in `dispatch_video_conversions` never calls
    /// `convert_single_video`, so it has no fresh oid to offer — it must
    /// register with `oid: None` and lean on `ship_phase`'s fingerprint
    /// fallback, not fabricate or resurrect a stale oid. No real ffmpeg
    /// needed: a matched fingerprint plus present staged outputs never reach
    /// the encoder at all.
    #[tokio::test]
    async fn skip_path_carry_forward_registers_with_no_oid_not_a_stale_one() {
        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");
        let moss_dir = tmp.path().join(".moss");
        std::fs::create_dir_all(&staging).unwrap();

        let mut svc = BuildServices::headless();
        svc.spawner = Some(std::sync::Arc::new(crate::build::ports::spawner::TokioSpawner));

        let item = "videos/clip.mov".to_string();
        let staged = stage_and_prime_video(&svc, &vault, &staging, &item, b"clip-bytes", false);

        let sealed = dispatch_and_seal(&svc, &vault, &staging, &moss_dir, vec![item.clone()]).await;

        assert!(!staged.is_empty(), "premise: the rig staged at least mp4 + poster");
        for key in &staged {
            assert!(
                sealed.files().contains_key(key),
                "the carry-forward key '{}' must still be registered",
                key
            );
            assert!(
                sealed.staged_oid(key).is_none(),
                "'{}' took the skip path with no fresh oid at hand — it must register \
                 with no oid rather than a stale or fabricated one",
                key
            );
        }
    }
}
