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

/// Merge transform entries into a source's record, preserving everything else
/// already in it — scan metadata, and outputs written by an earlier step of the
/// same conversion.
fn record_transforms(
    transforms: &crate::build::cache::TransformCache,
    source_oid: &str,
    source_file: &Path,
    entries: impl IntoIterator<Item = (String, crate::build::cache::TransformEntry)>,
) {
    use crate::build::cache::TransformRecord;
    let mut record = transforms.get(source_oid).unwrap_or_else(|| TransformRecord {
        source_oid: source_oid.to_string(),
        source_size: fs::metadata(source_file).map(|m| m.len()).unwrap_or(0),
        transforms: std::collections::HashMap::new(),
    });
    record.transforms.extend(entries);
    if let Err(e) = transforms.put(&record) {
        log::warn!("Failed to write transform record: {}", e);
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
/// Convention: `error.is_some()` means the conversion failed. Success carries
/// nothing — the outputs are on disk and in the transform record by the time
/// this returns, and the OIDs this once also carried were read by no one.
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
            // Recorded here rather than with the MP4's own outputs: four
            // early returns sit between this point and that write, and a
            // ladder that reached the object store without reaching the record
            // is seventeen blobs re-encoded on every build.
            record_transforms(transforms, source_oid, source_file, entries);
        }
        Ok(None) => log::debug!("No HLS ladder for {}: source fills one rung", filename),
        Err(ref e) if e == "Cancelled" => {
            return VideoConversionOutcome {
                error: Some("Cancelled".to_string()),
                hls_rungs: 0,
                poster: false,
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
                };
            }
            return VideoConversionOutcome {
                error: None,
                hls_rungs,
                poster: thumb_linked,
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
                let _ = fs::remove_file(&temp_thumb);
            }
        }
        Ok(false) => {}
        Err(ref e) if e == "Cancelled" => {
            let _ = fs::remove_file(&temp_thumb);
            return VideoConversionOutcome {
                error: Some("Cancelled".to_string()),
                hls_rungs,
                poster: false,
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
            let _ = fs::remove_file(&temp_mp4);
            return VideoConversionOutcome {
                error: Some("Cancelled".to_string()),
                hls_rungs,
                poster: false,
            };
        }
        Err(e) => {
            let _ = fs::remove_file(&temp_mp4);
            return VideoConversionOutcome {
                error: Some(e),
                hls_rungs,
                poster: false,
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
            let _ = fs::remove_file(&temp_mp4);
            return VideoConversionOutcome {
                error: Some(format!("Validation failed for {}", filename)),
                hls_rungs,
                poster: false,
            };
        }

        stage_mp4(objects, &temp_mp4, &output_mp4)
    };
    let _ = fs::remove_file(&temp_mp4);
    let mp4_oid_result = match staged {
        Ok(oid) => Some(oid),
        Err(e) => {
            return VideoConversionOutcome {
                error: Some(format!("could not stage {}: {}", filename, e)),
                hls_rungs,
                poster: false,
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
    }
}

/// Resolve the content hash for a source file, using HashIndex as a cache.
///
/// If the file's (size, mtime) match the cached entry, returns the cached hash
/// without reading the file. Otherwise, hashes the file via `ObjectStore::hash_file()`
/// and updates the index.
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
    if let Some(cached_hash) = hash_index.lookup(relative_path, size, mtime) {
        return Ok(cached_hash.to_string());
    }

    // Cache miss: hash the file and update index
    let hash = ObjectStore::hash_file(source_file)?;
    hash_index.update(relative_path.to_string(), size, mtime, hash.clone());
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
    fn shipped_original(
        source_file: &Path,
        staging_dir: &Path,
        mapped_source: &str,
        advisory: Option<Advisory>,
    ) -> Self {
        let url = moss_core::asset_paths::to_mp4(mapped_source);
        let mut advisories: Vec<Advisory> = advisory.into_iter().collect();
        let delivered = match crate::build::media::ffmpeg::copy_video_as_fallback(
            source_file,
            &staging_dir.join(&url),
        ) {
            Ok(()) => vec![Delivery { url, kind: DeliveryKind::Video }],
            Err(e) => {
                log::warn!("Fallback copy failed for {}: {}", mapped_source, e);
                // The caller's advisory says why moss could not OPTIMIZE the
                // video; only this arm knows the worse fact, that the page has
                // no video at all.
                advisories.push(Advisory::blocking_file(
                    mapped_source,
                    crate::infra::app_advisory::fmt("video_not_published", &[("err", &e)]),
                ));
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
    produced: &mut Vec<String>,
    delivered: Vec<Delivery>,
) {
    for Delivery { url, kind } in delivered {
        produced.push(url.clone());
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
    produced_video_paths: &[String],
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
) {
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
        return;
    }

    let mut advisories: Vec<Advisory> = vec![];
    // Count of videos this run ACTUALLY converted (ran FFmpeg / produced a fresh
    // mp4). Cache-skipped videos do NOT increment it — so a content-only rebuild
    // on a video site ends with `converted_count == 0`, which (with zero advisories)
    // gates out the media child + parent Jobs (FIX 1b, invariant #6).
    let mut converted_count: u32 = 0;
    // Every output path this run put on disk — the census `video_outputs` is
    // built from. Written only by `record_deliveries`.
    let mut produced_video_paths: Vec<String> = Vec::new();

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
        Some(mb) => VideoCompressionConfig { max_size_mb: mb, ..Default::default() },
        None => VideoCompressionConfig::default(),
    };

    use crate::build::cache::{HashIndex, ObjectStore};
    let vid_paths = MossPaths::from_moss_dir(ctx.moss_dir.clone());
    let objects = ObjectStore::new(vid_paths.cache_objects());
    let transforms = crate::build::cache::TransformCache::new(
        vid_paths.cache_transforms(),
        ObjectStore::new(vid_paths.cache_objects()),
    );
    let mp4_params = compression_config.to_params();
    let thumb_params = serde_json::json!({});

    // Load HashIndex for fast hash lookups (avoids re-reading large video files)
    let hash_index_path = vid_paths.cache_hash_index();
    let mut hash_index = HashIndex::load(&hash_index_path);

    // Clean temp directory at build start (defense-in-depth against unbounded growth)
    let temp_dir = vid_paths.cache_tmp();
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).ok();

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
                break 'item ItemStep::advise(Advisory {
                    scope: Scope::File,
                    severity: Severity::NeedsAction,
                    item: Some(filename.clone()),
                    what: crate::infra::app_advisory::t("not_found"),
                    action: Action::None,
                });
            }

            // No encoder: the original bytes ARE the deliverable. Decided here,
            // inside the item body, so this ships through the same owner, census
            // and budget as every other outcome — and so a superseded or
            // cancelled run still stops instead of copying the whole vault.
            let Some(ffmpeg) = ffmpeg.as_ref() else {
                // No advisory: this one is the environment's, pushed once above.
                break 'item ItemStep::shipped_original(
                    &source_file,
                    &ctx.staging_dir,
                    &mapped_source,
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
                        &source_file,
                        &ctx.staging_dir,
                        &mapped_source,
                        Some(Advisory {
                            scope: Scope::File,
                            // Transient and self-resolving (no user action): the original
                            // ships unoptimized and the next preview re-optimizes it. Use
                            // the calm "shipped, not optimal" tier, not NeedsAction — this
                            // is a soft notice, not a failure.
                            severity: Severity::ShippedDegraded,
                            item: Some(filename.clone()),
                            what: crate::infra::app_advisory::t("still_downloading_icloud"),
                            action: Action::None,
                        }),
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
                        &source_file,
                        &ctx.staging_dir,
                        &mapped_source,
                        Some(Advisory {
                            scope: Scope::File,
                            severity: Severity::NeedsAction,
                            item: Some(filename.clone()),
                            what: crate::infra::app_advisory::t("could_not_read_file"),
                            action: Action::None,
                        }),
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
                    &source_file,
                    &ctx.staging_dir,
                    &mapped_source,
                    Some(Advisory {
                        scope: Scope::File,
                        severity: Severity::ShippedDegraded,
                        item: Some(filename.clone()),
                        what: crate::infra::app_advisory::fmt("shipped_without_optimizing", &[("err", err_detail)]),
                        action: Action::None,
                    }),
                );
            };

            // The poster and the video, then the ladder's rungs. Registered only
            // now, with the files on disk: a <video> advances past a failed
            // <source> only until HAVE_METADATA, after which the chosen resource
            // is final (incident 8690101ac), so the emitter learns about a rung
            // on the build AFTER the one that encoded it. The master playlist
            // marks the stem as laddered; the rest are registered to resolve.
            let mut delivered = vec![Delivery { url: relative_mp4.clone(), kind: DeliveryKind::Video }];
            if outcome.poster {
                delivered.push(Delivery {
                    url: asset_paths::to_thumb(&mapped_source),
                    kind: DeliveryKind::Poster,
                });
            }
            if let Some(rungs) = asset_paths::video_ladder_rungs_by_count(outcome.hls_rungs) {
                delivered.extend(
                    asset_paths::hls_outputs(&mapped_source, rungs)
                        .into_iter()
                        .map(|url| Delivery { url, kind: DeliveryKind::Video }),
                );
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
                    .map(|len| Advisory {
                        scope: Scope::File,
                        severity: Severity::ShippedDegraded,
                        item: Some(filename.clone()),
                        what: crate::infra::app_advisory::fmt(
                            "video_exceeds_size_target",
                            &[
                                ("size", &format!("{:.0}", len as f64 / 1024.0 / 1024.0)),
                                ("cap", &compression_config.max_size_mb.to_string()),
                            ],
                        ),
                        action: Action::None,
                    })
                    .into_iter()
                    .collect(),
                // Successful conversion (either by us or shared from another task)
                // — real work, so it counts toward the media-Job gate (FIX 1b).
                encoded: true,
            }
        };

        match step {
            ItemStep::Handled { delivered, advisories: item_advisories, encoded } => {
                record_deliveries(services, &mut produced_video_paths, delivered);
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
                return;
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
}

/// Send produced video paths to the manifest coordinator, one
/// `HashBucket::VideoOutputs` message each. `blocking_send`, because this runs
/// inside `Spawner::spawn_blocking`. No-op when `paths` is empty.
fn emit_video_outputs_via_channel(
    tx: &Option<mpsc::Sender<EmitMessage>>,
    paths: &[String],
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
    for path in paths {
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

/// A SHA-256 over sorted `(path, size, mtime)` tuples plus the compression
/// config. Matching the previous dispatch's fingerprint means the video set is
/// unchanged and there is no reason to cancel and restart. The compression
/// params are in it so a config change (`video_max_size_mb` from a deploy
/// plugin) re-dispatches even when no video file moved.
pub(crate) fn compute_video_set_fingerprint(
    source_path: &str,
    video_items: &[String],
    compression_config: &crate::build::media::ffmpeg::VideoCompressionConfig,
) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();

    // Collect (path, size, mtime) tuples and sort by path for deterministic ordering
    let mut entries: Vec<(String, u64, u64)> = video_items
        .iter()
        .filter_map(|item| {
            let source = Path::new(source_path).join(&item);
            let meta = fs::metadata(&source).ok()?;
            let mtime = meta
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            Some((item.clone(), meta.len(), mtime))
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (path, size, mtime) in &entries {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(size.to_le_bytes());
        hasher.update(mtime.to_le_bytes());
    }

    // Include compression params so config changes trigger re-dispatch
    let params = compression_config.to_params();
    hasher.update(params.to_string().as_bytes());

    format!("{:x}", hasher.finalize())
}

/// Spawn the video worker — one owner for both the first-build and rebuild
/// paths in `build_inner()`.
///
/// Before cancelling an in-progress conversion it compares the video set's
/// fingerprint with the previous dispatch's; on a match the existing conversion
/// keeps running (the user changed markdown, not video), and the carry-forward
/// output keys are re-registered so seal does not prune them.
pub(crate) fn dispatch_video_conversions(
    services: Option<&BuildServices>,
    background_ctx: BackgroundContext,
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
                Some(mb) => crate::build::media::ffmpeg::VideoCompressionConfig {
                    max_size_mb: mb,
                    ..Default::default()
                },
                None => crate::build::media::ffmpeg::VideoCompressionConfig::default(),
            };
            let fingerprint = compute_video_set_fingerprint(
                &background_ctx.source_path,
                &background_ctx.video_items,
                &compression_config,
            );

            // Check fingerprint: if video set unchanged AND all canonical
            // outputs are present + non-zero, let existing conversion continue.
            //
            // The output check is the self-heal seam — without it, a previous
            // dispatch killed mid-`link_to` leaves 0-byte stubs in canonical
            // and every subsequent dispatch with the same input set skips,
            // so the user sees a permanent loader on the affected card. The
            // per-video fast-path inside run_video_conversion already heals
            // 0-byte canonical outputs, but it never runs because the dispatch
            // is skipped here.
            //
            // Note: when this path forces re-dispatch, in_flight_videos.clear()
            // below kills any in-flight conversion. That's intended — if outputs
            // are bad, restart beats letting a possibly-stuck conversion run.
            if svc.cancellation.check_and_update_fingerprint(&fingerprint) {
                // Fingerprint matched: video set unchanged. Re-register every
                // key the encode path delivers, so seal()'s
                // video_outputs.retain keeps them — an untouched key is pruned
                // at seal and the stale sweep then deletes a physically
                // present file → live 404. emit_video_outputs_via_channel's
                // existence check drops the keys whose staging file is absent,
                // which is the self-heal: they fall through to the dispatch
                // below on the next rebuild cycle.
                log::info!(
                    "Video set unchanged ({} videos), skipping re-dispatch — re-registering carry-forward output keys",
                    background_ctx.video_items.len()
                );
                use crate::build::render::resolve_path_with_overrides;
                use moss_core::asset_paths;
                let dir_overrides = &background_ctx.dir_overrides;
                let mut skip_paths: Vec<String> = Vec::new();
                for item in &background_ctx.video_items {
                    let mapped = resolve_path_with_overrides(&item, &dir_overrides);
                    skip_paths.push(asset_paths::to_mp4(&mapped));
                    skip_paths.push(asset_paths::to_thumb(&mapped));
                    // The full ladder as a candidate set: video_ladder_rungs
                    // truncates from the top only, so any real ladder is a
                    // prefix of it and the existence check filters the rest.
                    skip_paths.extend(asset_paths::hls_outputs(&mapped, &asset_paths::VIDEO_LADDER));
                }
                emit_video_outputs_via_channel(&tx, &skip_paths, &background_ctx.staging_dir);
                return;
            }

            // Bump the epoch so stale tasks exit on their next epoch check — the sole
            // halt signal for a prior epoch, and what keeps concurrent FFmpeg
            // processes from piling up when rebuilds land during a conversion
            // (common on iCloud Drive). Clearing singleflight lets the cancelled
            // videos be re-dispatched while the old task drains.
            svc.in_flight_videos.clear();
            let epoch = svc.cancellation.start_new_conversion();

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
            spawner.spawn_blocking(Box::new(move || {
                run_video_conversion(&services_arc, &background_ctx, epoch, tx);
            }));
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
            run_video_conversion(svc, &background_ctx, 0, tx);
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
        if let Err(e) = fs::remove_dir_all(&legacy_video_cache) {
            log::warn!("Failed to remove legacy video cache: {}", e);
        } else {
            log::info!("Removed legacy path-based video cache (.moss/build/cache/videos/)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A second build of a settled video site keeps every file the first one
    /// staged. The skip branch is the only thing that re-registers them, so this
    /// drives the real seam: dispatch → seal → the stale sweep `advertise_sealed`
    /// runs (`build.rs`, step 4). Subsumes the emit-level positive case, which
    /// asserted the mp4 and poster keys reached the sealed manifest and could not
    /// see either the dispatch that produces them or the deletion that follows.
    #[tokio::test]
    async fn a_skip_dispatch_keeps_the_hls_ladder_through_stale_cleanup() {
        use crate::build::coordinator::test_utils;
        use crate::build::media::pipeline::{compute_expected_dirs, remove_stale_dirs, remove_stale_files};
        use crate::types::content::SiteHashes;
        use crate::types::services::BackgroundContext;
        use moss_core::asset_paths;

        let tmp = portable_tmpdir();
        let vault = tmp.path().join("vault");
        let staging = tmp.path().join("stage");

        let item = "videos/talk.mov".to_string();
        std::fs::create_dir_all(vault.join("videos")).unwrap();
        std::fs::write(vault.join(&item), b"source bytes").unwrap();

        // Build N's outputs, on disk: mp4, poster, and a 3-rung ladder (a
        // 640-wide source — deliberately shorter than VIDEO_LADDER).
        let rungs = asset_paths::video_ladder_rungs(640);
        let ladder = asset_paths::hls_outputs(&item, rungs);
        let mut staged = vec![asset_paths::to_mp4(&item), asset_paths::to_thumb(&item)];
        staged.extend(ladder.clone());
        for key in &staged {
            let abs = staging.join(key);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(&abs, b"x").unwrap();
        }

        let mut svc = BuildServices::headless();
        svc.spawner = Some(std::sync::Arc::new(crate::build::ports::spawner::TokioSpawner));
        // Prime the fingerprint with build N's, so build N+1 takes the skip branch.
        let fingerprint = compute_video_set_fingerprint(
            &vault.display().to_string(),
            std::slice::from_ref(&item),
            &crate::build::media::ffmpeg::VideoCompressionConfig::default(),
        );
        assert!(!svc.cancellation.check_and_update_fingerprint(&fingerprint));

        let ctx = BackgroundContext {
            video_items: vec![item.clone()],
            source_path: vault.display().to_string(),
            staging_dir: staging.clone(),
            ..BackgroundContext::for_test()
        };
        let (tx, rx) = test_utils::build_test_coordinator();
        // blocking_send requires a non-async thread — as in production, where the
        // dispatcher is called from the blocking render phase.
        tokio::task::spawn_blocking(move || {
            dispatch_video_conversions(Some(&svc), ctx, Some(tx));
        })
        .await
        .unwrap();
        let sealed = test_utils::drain_into_sealed(rx, SiteHashes::default()).await;

        let view = sealed.site_hashes_view();
        remove_stale_files(&staging, view, "test");
        remove_stale_dirs(&staging, &compute_expected_dirs(view));

        for key in &staged {
            assert!(
                staging.join(key).exists(),
                "'{}' was deleted by the stale sweep after a skip dispatch — \
                 the deploy would remove a physically-present file and the page's \
                 <source> would 404; sealed video_outputs: {:?}",
                key,
                view.video_outputs
            );
        }
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
        let paths = vec![mp4_key.clone(), thumb_key.clone()];
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
        let paths = vec![mp4_key.clone(), thumb_key.clone()];
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
        let paths_b = vec![mp4_key.clone(), thumb_key.clone()];
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
                assert_eq!(advisory.item.as_deref(), Some("clip.mov"));
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
}
