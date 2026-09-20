//! Folder analysis and project structure detection for moss build
//!
//! This module handles the first phase of website build: analyzing a directory
//! to understand its content and structure. It scans folders recursively, categorizes
//! files by type, and makes intelligent decisions about how the site should be organized.
//!
//! ## Media Metadata Extraction (ADR-006)
//!
//! For image and video files, we extract additional metadata:
//! - **Dimensions**: Read from image headers (fast, ~1ms per file)
//! - **Dominant color**: Extracted from 100x100 thumbnail for speed
//! - **EXIF orientation**: Used to swap dimensions for rotated images
//!
//! This metadata enables ADR-002 (dynamic SVG placeholders) during page rendering.

use crate::types::content::{FileInfo, MediaMetadata, ProjectStructure};
use crate::build::cache::{CachedMediaMeta, FileStat, HashIndex, ObjectStore, TransformCache, TransformEntry, TransformRecord};
use super::classify::{classify_extension, is_excluded_dir_name, skip_root_agent_config, ScanBucket};
use crate::build::media::ffmpeg::FFmpegManager;
use walkdir::WalkDir;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

/// Internal event variants emitted during the scan walk.
/// The shell maps them onto its own events in `crate::events`; unit tests
/// capture them directly.
pub enum ScanEvent {
    Progress { found: usize, evicted: usize },
    Complete {
        total_files: usize,
        evicted_count: usize,
        cloud_provider: Option<String>,
    },
}

/// Abstraction over "where scan events go", plus the throttle that keeps a
/// fast local scan from flooding whatever is listening.
pub struct ScanEventEmitter {
    inner: Box<dyn Fn(ScanEvent) + Send + Sync>,
    last_progress_at: std::sync::Mutex<Option<Instant>>,
}

impl ScanEventEmitter {
    /// Throttle interval for `scan-progress` emits. Chosen so UI updates
    /// feel live without flooding the IPC bus during fast local scans.
    const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

    /// Wrap a sink. The shell's is `crate::events::scan_event_emitter`; tests
    /// pass a closure that captures what was emitted.
    pub fn new(f: Box<dyn Fn(ScanEvent) + Send + Sync>) -> Self {
        Self {
            inner: f,
            last_progress_at: std::sync::Mutex::new(None),
        }
    }

    /// Emit a `Progress` event, throttled to at most one per
    /// `PROGRESS_INTERVAL`. Always lets the first event through.
    fn emit_progress_throttled(&self, found: usize, evicted: usize) {
        let mut guard = self.last_progress_at.lock().unwrap();
        let now = Instant::now();
        let should_emit = match *guard {
            None => true,
            Some(t) => now.duration_since(t) >= Self::PROGRESS_INTERVAL,
        };
        if should_emit {
            *guard = Some(now);
            drop(guard);
            (self.inner)(ScanEvent::Progress { found, evicted });
        }
    }

    fn emit_complete(
        &self,
        total_files: usize,
        evicted_count: usize,
        cloud_provider: Option<String>,
    ) {
        (self.inner)(ScanEvent::Complete {
            total_files,
            evicted_count,
            cloud_provider,
        });
    }
}

// =========================================================================
// Media Metadata Extraction Functions (ADR-006)
// =========================================================================

/// Extract image dimensions from file header.
///
/// ADR-006: Uses image::image_dimensions() which reads header only (~1ms per file)
/// This is much faster than loading the entire image into memory.
///
/// # Arguments
/// * `path` - Path to the image file
///
/// # Returns
/// * `Some((width, height))` - Dimensions if successfully read
/// * `None` - If file cannot be read or is not a valid image
pub fn extract_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    // ADR-006: Use image_dimensions() which reads header only (~1ms per file)
    match image::image_dimensions(path) {
        Ok((width, height)) => {
            // EXIF orientation 5-8 (a 90°/270° rotation) swaps the DISPLAY
            // dimensions relative to the stored pixel grid. Read the tag for
            // every responsive-ladder source (png/jpg/jpeg/webp — the raster
            // formats that both carry an EXIF orientation tag AND participate in
            // the ladder) through the SAME reader the encode side uses
            // (`decode_oriented` → `read_exif_orientation`, build/media/image.rs).
            // Going through the identical function makes scan's stored dims agree
            // BYTE-FOR-BYTE with encode's oriented dims, so emission/registration
            // and encode derive the SAME ladder — no emitted-but-never-encoded
            // rung (publish-404) — and the `<img width height>` attribute is
            // correct (no CLS) for EXIF-oriented png/webp too, not just jpeg.
            //
            // Gated to ladder sources on purpose: gif/svg/heic/bmp/tiff/avif are
            // NOT ladder sources (the encode side derives no ladder from them), so
            // they keep their stored dims — matching prior behavior — and only the
            // four ladder extensions pay the bounded second header read (EXIF sits
            // in the first few KB), keeping first-paint scan cost unchanged for
            // everything else. Deriving the gate from `is_ladder_source_ext`
            // single-sources the "which formats agree with encode's ladder?"
            // decision instead of hardcoding a parallel extension list.
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if moss_core::asset_paths::is_ladder_source_ext(ext)
                && should_swap_dimensions(crate::build::media::image::read_exif_orientation(path))
            {
                return Some((height, width));
            }
            Some((width, height))
        }
        Err(e) => {
            log::debug!("Could not read image dimensions for {:?}: {}", path, e);
            None
        }
    }
}

/// Determine if dimensions should be swapped based on EXIF orientation.
///
/// Orientations 5-8 indicate 90 or 270 degree rotation, which means
/// the stored dimensions are swapped from the visual dimensions.
///
/// # EXIF Orientation Values:
/// - 1: Normal
/// - 2: Flipped horizontally
/// - 3: Rotated 180 degrees
/// - 4: Flipped vertically
/// - 5: Rotated 90 CW then flipped horizontally
/// - 6: Rotated 90 CW
/// - 7: Rotated 90 CCW then flipped horizontally
/// - 8: Rotated 90 CCW
pub fn should_swap_dimensions(orientation: u32) -> bool {
    // Orientations 5-8 involve 90/270 degree rotation
    (5..=8).contains(&orientation)
}

/// Extract dominant color and LQIP from an image in a single load.
///
/// Opens the image once, then:
/// 1. 100x100 thumbnail → average color as hex "#RRGGBB"
/// 2. ~20px thumbnail → JPEG quality 20 → base64 data URI
///
/// Returns `(dominant_color, lqip_data_uri)` — both are `None` if the
/// image cannot be loaded (SVG, corrupt, etc.).
pub fn extract_color_and_lqip(path: &Path) -> (Option<String>, Option<String>) {
    let img = match image::open(path) {
        Ok(img) => img,
        Err(_) => return (None, None),
    };
    compute_color_and_lqip_from_image(&img)
}

/// Compute `(dominant_color, lqip_data_uri)` from an already-decoded image.
///
/// Split out from [`extract_color_and_lqip`] so the background media phase can
/// reuse the pixels it already decoded for WebP conversion instead of paying a
/// second full decode. The blocking scan no longer calls this for images (it
/// reads dimensions only); the background computes color/LQIP here and caches
/// them under the scan's stat key. See `extract_media_metadata_cached`.
pub(crate) fn compute_color_and_lqip_from_image(
    img: &image::DynamicImage,
) -> (Option<String>, Option<String>) {
    // --- Dominant color from 100x100 thumbnail ---
    let thumbnail = img.thumbnail(100, 100);
    let rgb = thumbnail.to_rgb8();

    let (mut r_sum, mut g_sum, mut b_sum) = (0u64, 0u64, 0u64);
    let mut count = 0u64;

    for pixel in rgb.pixels() {
        r_sum += pixel[0] as u64;
        g_sum += pixel[1] as u64;
        b_sum += pixel[2] as u64;
        count += 1;
    }

    let dominant_color = if count > 0 {
        let r = (r_sum / count) as u8;
        let g = (g_sum / count) as u8;
        let b = (b_sum / count) as u8;
        Some(format!("#{:02X}{:02X}{:02X}", r, g, b))
    } else {
        None
    };

    // --- LQIP from ~20px thumbnail → JPEG → base64 data URI ---
    let lqip_thumb = img.thumbnail(20, 20);
    let lqip_rgb = lqip_thumb.to_rgb8();
    let mut jpeg_buf = std::io::Cursor::new(Vec::new());
    let lqip_data_uri = if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, 20)
        .encode(
            lqip_rgb.as_raw(),
            lqip_rgb.width(),
            lqip_rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .is_ok()
    {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(jpeg_buf.into_inner());
        Some(format!("data:image/jpeg;base64,{}", b64))
    } else {
        None
    };

    (dominant_color, lqip_data_uri)
}

/// Sniff whether an image file is animated, gated by extension so ONLY
/// gif/webp pay the bounded (≤4 KB) header read. Every other extension —
/// including videos — returns `false` without touching the file, keeping the
/// blocking scan's first-paint cost unchanged.
fn sniff_is_animated(path: &Path, extension: &str) -> bool {
    match extension {
        "gif" => crate::build::media::sniff::is_animated_gif(path),
        "webp" => crate::build::media::sniff::is_animated_webp(path),
        _ => false,
    }
}

/// Extract complete media metadata for an image or video file.
///
/// ADR-006: Combines dimension extraction, EXIF handling, and dominant color
/// into a single MediaMetadata struct for efficient scanning.
///
/// # Arguments
/// * `path` - Absolute path to the media file
/// * `relative_path` - Relative path from scan root
/// * `extension` - File extension in lowercase
/// * `size` - File size in bytes
/// * `modified` - Modification timestamp as string
///
/// # Returns
/// MediaMetadata struct with all available fields populated
pub fn extract_media_metadata(
    path: &Path,
    relative_path: &str,
    extension: &str,
    size: u64,
    modified: Option<String>,
    ffmpeg: Option<&FFmpegManager>,
) -> MediaMetadata {
    let is_image = matches!(
        extension,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "avif"
    );

    // A cloud-evicted source cannot be probed. Image decoding reads bytes, and
    // ffprobe is a subprocess whose failure arrives as a non-zero exit status
    // rather than an io::Error anything here could classify — so an evicted
    // video would record "no dimensions, no colour" as if that were the truth
    // about the file. Ask for it back and leave the fields empty; the watcher
    // rebuilds once it lands.
    // The ask is deliberately unconditional but *not* unbounded: the scan visits
    // every media file, and media is the bulk of an evicted vault, so a cold
    // open of a 700-photo folder asks 700 times. `request_download` holds every
    // build caller to a global burst cap for exactly this reason — and the
    // supervisor, which asks in Home-first order, bypasses it so this can never
    // starve the file the gate is waiting on.
    let evicted = crate::build::icloud::is_evicted(path);
    if evicted {
        crate::build::cloud_readiness::request_download(path);
    }

    // Extract dimensions for images (native) or videos (via ffprobe)
    let dimensions = if evicted {
        None
    } else if is_image {
        extract_image_dimensions(path)
    } else if let Some(mgr) = ffmpeg {
        mgr.get_dimensions(path).ok()
    } else {
        None
    };

    // Extract dominant color + LQIP for images, or just color for videos
    let (dominant_color, lqip_data_uri) = if evicted {
        (None, None)
    } else if is_image {
        extract_color_and_lqip(path)
    } else if let Some(mgr) = ffmpeg {
        (crate::build::media::dimensions::extract_video_dominant_color(mgr, path), None)
    } else {
        (None, None)
    };

    MediaMetadata {
        path: relative_path.to_string(),
        file_type: extension.to_string(),
        size,
        modified,
        dimensions,
        dominant_color,
        lqip_data_uri,
        is_animated: !evicted && sniff_is_animated(path, extension),
    }
}

/// Transform name used for cached media metadata in the TransformCache.
const MEDIA_META_TRANSFORM: &str = "media/meta";

/// Stat-based cache key for an image's placeholder metadata (dimensions +
/// dominant color + LQIP), shared by the blocking scan and the background media
/// phase so they read/write the SAME `media/meta` entry.
///
/// The blocking scan writes this key with dimensions only (color/LQIP `None`);
/// the background media phase, which decodes each image for WebP anyway,
/// enriches the same key with the dominant color + LQIP it computes from those
/// pixels. A later (warm) scan then returns the full placeholder metadata.
/// `(relative_path, size, mtime)` uniquely identifies a file version — any edit
/// changes size or mtime and invalidates the entry. Must be kept byte-identical
/// on both sides; that is exactly why it lives in one function.
pub(crate) fn image_meta_stat_key(relative_path: &str, size: u64, mtime: u64) -> String {
    format!("stat:{}:{}:{}", relative_path, size, mtime)
}

/// Resolve the content hash for a file, using the hash index for speed.
///
/// If the hash index has an entry that still matches the file's full stat record
/// (`HashIndex::lookup`), that entry is carried into the new index and its hash
/// returned without reading the file.  Otherwise, the file is hashed via
/// `ObjectStore::hash_file` and the *new* index is updated.
fn resolve_content_hash(
    abs_path: &Path,
    relative_path: &str,
    stat: &FileStat,
    old_index: &HashIndex,
    new_index: &mut HashIndex,
) -> Option<String> {
    // Check old index for a stat-matching entry.
    if let Some(cached_hash) = old_index.lookup(relative_path, stat) {
        let hash = cached_hash.to_string();
        new_index.carry_forward(old_index, relative_path);
        return Some(hash);
    }

    // Stat miss — re-hash the file.
    match ObjectStore::hash_file(abs_path) {
        Ok(hash) => {
            new_index.update(relative_path.to_string(), stat, hash.clone());
            Some(hash)
        }
        Err(e) => {
            log::warn!("Failed to hash {}: {}", relative_path, e);
            None
        }
    }
}

/// Try to read cached media metadata from the TransformCache.
///
/// Returns `Some(CachedMediaMeta)` on cache hit, `None` on miss.
///
/// `pub(crate)` so the background media phase can read back `is_animated`
/// before it enriches (and thereby overwrites) a stat-keyed entry with the
/// dominant color + LQIP it computes — see `run_image_conversion`'s call in
/// `build/media/image.rs`.
pub(crate) fn read_cached_meta(
    transform_cache: &TransformCache,
    objects: &ObjectStore,
    content_hash: &str,
) -> Option<CachedMediaMeta> {
    let params = serde_json::json!({});
    let meta_oid = transform_cache.find_cached_output(content_hash, MEDIA_META_TRANSFORM, &params)?;
    let blob_path = objects.get_path(&meta_oid)?;
    let raw = std::fs::read(blob_path).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// Store media metadata in the ObjectStore + TransformCache.
///
/// `pub(crate)` so the background media phase can enrich an image's stat-key
/// entry with the dominant color + LQIP it computes during WebP conversion.
pub(crate) fn write_cached_meta(
    objects: &ObjectStore,
    transform_cache: &TransformCache,
    content_hash: &str,
    source_size: u64,
    meta: &CachedMediaMeta,
) {
    let json_bytes = match serde_json::to_vec(meta) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("Failed to serialize media meta: {}", e);
            return;
        }
    };

    let meta_oid = match objects.store_bytes(&json_bytes) {
        Ok(oid) => oid,
        Err(e) => {
            log::warn!("Failed to store media meta blob: {}", e);
            return;
        }
    };

    let params = serde_json::json!({});
    let new_entry = TransformEntry {
        oid: meta_oid,
        size: json_bytes.len() as u64,
        params,
    };

    // Merge with existing record to preserve other transforms (video/mp4, video/thumbnail).
    // Without this, writing media/meta would overwrite the entire TransformRecord,
    // destroying previously cached video conversion results.
    let mut record = transform_cache
        .get(content_hash)
        .unwrap_or_else(|| TransformRecord {
            source_oid: content_hash.to_string(),
            source_size,
            transforms: HashMap::new(),
        });
    record.transforms.insert(MEDIA_META_TRANSFORM.to_string(), new_entry);

    if let Err(e) = transform_cache.put(&record) {
        log::warn!("Failed to write media meta transform record: {}", e);
    }
}

/// Extract media metadata with two-layer caching.
///
/// Layer 1 (HashIndex): avoids re-hashing unchanged files using stat fields.
/// Layer 2 (TransformCache): avoids re-running ffprobe/ffmpeg when content
/// hash is already known to have cached metadata.
///
/// On cache miss, falls back to the original `extract_media_metadata` path,
/// then stores the result for next time.
///
/// ## Video-specific optimization
///
/// Video files are typically multi-GB; SHA-256 hashing takes 14-28s per file,
/// totaling ~108s for 6 videos. This dominates the blocking scan phase. The
/// scan only needs dimensions (for placeholder SVGs), not content verification.
///
/// For videos, we skip hashing when the hash index has no stat match (first
/// scan). Instead, we extract metadata directly via ffprobe (~50ms) and cache
/// under a stat-based key `stat:<path>:<size>:<mtime>`. On subsequent scans
/// (when the hash index has a stat match), the existing hash-based path is
/// used.
///
/// Raster image files: in preview/server mode (`defer_placeholders = true`) they
/// take the stat-key dimensions-only path (Step 2c) — the full decode for
/// dominant color + LQIP is deferred to the background media phase so first paint
/// is independent of image count. In build/deploy mode (`defer_placeholders =
/// false`) they take the synchronous full-extract path so the built HTML bakes
/// complete color + LQIP placeholders.
fn extract_media_metadata_cached(
    abs_path: &Path,
    relative_path: &str,
    extension: &str,
    stat: &FileStat,
    modified: Option<String>,
    ffmpeg: Option<&FFmpegManager>,
    old_index: &HashIndex,
    new_index: &mut HashIndex,
    objects: &ObjectStore,
    transform_cache: &TransformCache,
    metadata_dedup: Option<&crate::build::cache::Singleflight<MediaMetadata>>,
    // When true (live preview / server mode) images are resolved dimensions-only
    // and their dominant color + LQIP are deferred to the background so first
    // paint is instant. When false (build / deploy) they are extracted
    // synchronously so the built HTML bakes the LQIP + color placeholders — the
    // deployed site must be complete, not progressive. See `scan_folder_with_dedup_emit`.
    defer_placeholders: bool,
) -> MediaMetadata {
    let (size, mtime) = (stat.size, stat.mtime);
    let is_video = matches!(extension, "mov" | "mp4" | "webm" | "avi" | "mkv" | "m4v");
    // Raster images whose dominant color / LQIP require a full pixel decode.
    // SVG is excluded — it is a vector format (`image::open` can't decode it, so
    // it carries no decode cost) and its dimensions take the ordinary path.
    // We only DEFER these (dimensions-only + stat-key) in preview/server mode;
    // in build/deploy mode `defer_this_image` is false and images take the full
    // synchronous extract below, exactly as before this optimization.
    let is_image = matches!(extension, "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif");
    let defer_this_image = is_image && defer_placeholders;

    // Step 1: Resolve content hash.
    //
    // For videos: only use the fast stat-match path in the hash index. If the
    // hash index has no stat match (first scan), we skip hashing entirely —
    // reading multi-GB files for SHA-256 takes 14-28s per file and blocks the
    // preview from loading.
    //
    // For iCloud-evicted files: same treatment as videos. Reading an evicted
    // file triggers a kernel-level download via fileproviderd, blocking the
    // entire scan. With hundreds of evicted files this delays preview by 15s+.
    // Use stat-based cache key instead; the file watcher will trigger a rebuild
    // when iCloud materializes the file.
    //
    // For raster images: ALSO skip the content read on the blocking scan. Both
    // the full-file SHA-256 AND the full pixel decode (dominant color + LQIP)
    // are deferred to the background media phase, which already decodes every
    // image for WebP conversion. The blocking scan reads only the dimensions
    // (image header, ~1ms) so first paint stays independent of image count — a
    // folder of 1000+ large photos previously spent minutes here, because a
    // full decode is ~10× a header read and it ran sequentially per image. See
    // the image stat-miss branch below.
    // Held for the whole function: an evicted source cannot answer any of the
    // questions below, and — crucially — its non-answers must not be written to
    // the stat-keyed caches. Materialization on Sonoma+ preserves size AND
    // mtime, so the arrival leaves `stat:<path>:<size>:<mtime>` byte-identical:
    // a cached `dimensions: None` would be read back as authoritative on every
    // later build, and videos never enter `HashIndex`, so nothing else would
    // ever invalidate it. The image would be permanently dimensionless.
    let source_is_evicted = crate::build::icloud::is_evicted(abs_path);
    if source_is_evicted {
        // Burst-capped inside `request_download`, same as above.
        crate::build::cloud_readiness::request_download(abs_path);
    }
    let skip_content_read = is_video || defer_this_image || source_is_evicted;
    let content_hash = if skip_content_read {
        // Videos / images / evicted files: only use a cached hash from the
        // index; never read file content on the blocking scan. A video is matched
        // by the whole-second rule its own worker records under (it can never
        // fall back to hashing a multi-GB file); anything else by the full stat
        // record, so a hit here is one `collect_images_for_conversion` will also
        // take. The hit is carried forward as recorded, never re-stamped.
        let hit = if is_video {
            old_index.lookup_whole_second(relative_path, size, mtime)
        } else {
            old_index.lookup(relative_path, stat)
        };
        hit.map(|h| {
            let hash = h.to_string();
            new_index.carry_forward(old_index, relative_path);
            hash
        })
    } else {
        resolve_content_hash(abs_path, relative_path, stat, old_index, new_index)
    };

    // Step 2: If we have a content hash, check the transform cache.
    if let Some(ref hash) = content_hash {
        if let Some(cached) = read_cached_meta(transform_cache, objects, hash) {
            log::trace!("Media meta cache hit for {}", relative_path);
            return MediaMetadata {
                path: relative_path.to_string(),
                file_type: extension.to_string(),
                size,
                modified,
                dimensions: cached.dimensions,
                dominant_color: cached.dominant_color,
                lqip_data_uri: cached.lqip_data_uri,
                is_animated: cached.is_animated,
            };
        }
    }

    // Step 2b: For videos without a content hash (first scan, stat miss),
    // check the transform cache using a stat-based key instead.
    // The stat key (path + size + mtime) uniquely identifies a file version.
    // If the file changes, stat will change and we'll re-extract.
    if is_video && content_hash.is_none() {
        let stat_key = image_meta_stat_key(relative_path, size, mtime);
        if let Some(cached) = read_cached_meta(transform_cache, objects, &stat_key) {
            log::debug!("Media meta stat-cache hit for {}", relative_path);
            return MediaMetadata {
                path: relative_path.to_string(),
                file_type: extension.to_string(),
                size,
                modified,
                dimensions: cached.dimensions,
                dominant_color: cached.dominant_color,
                lqip_data_uri: cached.lqip_data_uri,
                is_animated: cached.is_animated,
            };
        }

        // Stat cache miss — extract metadata directly. This is fast because
        // ffprobe reads only the container header (~50ms), not the full file.
        // Skip singleflight (which is hash-keyed, not applicable here). A
        // miss is the routine first-scan case, not a problem to flag — TRACE
        // like the cache-hit branch above, not DEBUG (measured ~520
        // lines/session in a real upload, 2026-09-15,
        // docs/archive/2026-09-15-open-feedback-design.md).
        log::trace!("Media meta stat-cache miss for {} (skipping hash)", relative_path);
        let meta = extract_media_metadata(
            abs_path, relative_path, extension, size, modified, ffmpeg,
        );

        // Cache under stat key so subsequent calls in this session are fast.
        let cached = CachedMediaMeta {
            dimensions: meta.dimensions,
            dominant_color: meta.dominant_color.clone(),
            lqip_data_uri: meta.lqip_data_uri.clone(),
            is_animated: meta.is_animated,
        };
        // See `source_is_evicted` above: caching a non-answer under a key the
        // arrival does not change makes it permanent.
        if !source_is_evicted {
            write_cached_meta(objects, transform_cache, &stat_key, size, &cached);
        }

        return meta;
    }

    // Step 2c: Raster images are ALWAYS resolved here via the stat key — never
    // by content hash and never via the full-decode fallback (Step 3). Read ONLY
    // the dimensions on the blocking scan and cache them under a stat-based key.
    // Dominant color + LQIP are left `None`; the background media phase computes
    // them from the pixels it decodes for WebP and enriches this same stat-key
    // entry, so a later rebuild picks them up. Until then the grid card /
    // placeholder renders with a graceful fallback color and the real image
    // morphs in via the AssetRegistry. This keeps first paint near-instant
    // regardless of image count.
    //
    // The condition is `defer_this_image` (not `content_hash.is_none()`): on a
    // warm preview build the content hash is `Some` (preserved into `new_index`
    // above so the background's `collect_images_for_conversion` can stat-match
    // it), but we must STILL take the stat-key path — otherwise a warm image
    // would fall through to Step 3 and pay the full decode we worked to defer.
    // In build/deploy mode `defer_this_image` is false, so images fall through
    // to Step 2/Step 3 and are extracted synchronously (LQIP + color baked).
    if defer_this_image {
        let stat_key = image_meta_stat_key(relative_path, size, mtime);
        if let Some(cached) = read_cached_meta(transform_cache, objects, &stat_key) {
            log::trace!("Media meta stat-cache hit for {}", relative_path);
            return MediaMetadata {
                path: relative_path.to_string(),
                file_type: extension.to_string(),
                size,
                modified,
                dimensions: cached.dimensions,
                dominant_color: cached.dominant_color,
                lqip_data_uri: cached.lqip_data_uri,
                is_animated: cached.is_animated,
            };
        }

        // Cold: header-only dimension read (~1ms). No SHA-256, no pixel decode.
        // is_animated IS sniffed here (bounded ≤4 KB, gif/webp only) — this is
        // the one true first-write path, since extract_media_metadata isn't
        // called on this branch. The result is cached below so every
        // subsequent scan of this unchanged file hits the branch above.
        // Same reasoning as the video branch above: routine, not DEBUG.
        log::trace!(
            "Media meta stat-cache miss for {} (dimensions only; color/LQIP deferred)",
            relative_path
        );
        let dimensions = extract_image_dimensions(abs_path);
        let is_animated = sniff_is_animated(abs_path, extension);
        let cached = CachedMediaMeta {
            dimensions,
            dominant_color: None,
            lqip_data_uri: None,
            is_animated,
        };
        // See `source_is_evicted` above.
        if !source_is_evicted {
            write_cached_meta(objects, transform_cache, &stat_key, size, &cached);
        }

        return MediaMetadata {
            path: relative_path.to_string(),
            file_type: extension.to_string(),
            size,
            modified,
            dimensions,
            dominant_color: None,
            lqip_data_uri: None,
            is_animated,
        };
    }

    // Step 3: Cache miss — run the real extraction.
    // If singleflight dedup is available and we have a content hash, wrap the
    // extraction so concurrent scans for the same file share the result.
    log::debug!("Media meta cache miss for {}", relative_path);

    if let (Some(dedup), Some(ref hash)) = (metadata_dedup, &content_hash) {
        let dedup_key = format!("meta:{}", hash);
        // Clone data into owned values for the closure (FnOnce + Send).
        let abs_path_owned = abs_path.to_path_buf();
        let relative_path_owned = relative_path.to_string();
        let extension_owned = extension.to_string();
        let modified_clone = modified.clone();
        // Capture FFmpeg bin_path string so we can reconstruct inside closure.
        let ffmpeg_bin = ffmpeg.map(|f| f.bin_path().to_string());
        // Capture cache paths for reconstruction inside the closure.
        let objects_dir = objects.root().to_path_buf();
        let hash_clone = hash.clone();

        let (result, shared) = dedup.do_work(&dedup_key, move || {
            // Reconstruct FFmpegManager from bin_path if available.
            let ffmpeg_mgr = ffmpeg_bin.map(FFmpegManager::from_bin_path);
            let meta = extract_media_metadata(
                &abs_path_owned,
                &relative_path_owned,
                &extension_owned,
                size,
                modified_clone,
                ffmpeg_mgr.as_ref(),
            );

            // Store the result in the transform cache for next time.
            // Reconstruct cache infrastructure from paths because ObjectStore/TransformCache
            // are borrowed from the enclosing scope and cannot be captured by reference in
            // a FnOnce + Send closure. These types are stateless path wrappers, so
            // reconstruction is safe. Layout: .moss/cache/objects/ → sibling transforms/
            let cache_dir = objects_dir.parent().unwrap_or(&objects_dir).to_path_buf();
            let objects = ObjectStore::new(objects_dir);
            let transform_cache = TransformCache::new(
                cache_dir.join("transforms"),
                ObjectStore::new(objects.root().to_path_buf()),
            );
            let cached = CachedMediaMeta {
                dimensions: meta.dimensions,
                dominant_color: meta.dominant_color.clone(),
                lqip_data_uri: meta.lqip_data_uri.clone(),
                is_animated: meta.is_animated,
            };
            write_cached_meta(&objects, &transform_cache, &hash_clone, size, &cached);

            meta
        });

        if let Some(meta) = result {
            if shared {
                log::debug!("Media meta singleflight hit for {}", hash);
            }
            return meta;
        }
        // If do_work returned None (panic in work fn), fall through to direct extraction.
        log::warn!("Singleflight returned None for {}, falling back to direct extraction", hash);
    }

    let meta = extract_media_metadata(
        abs_path, relative_path, extension, size, modified.clone(), ffmpeg,
    );

    // Step 4: Store the result in the transform cache for next time.
    if let Some(ref hash) = content_hash {
        let cached = CachedMediaMeta {
            dimensions: meta.dimensions,
            dominant_color: meta.dominant_color.clone(),
            lqip_data_uri: meta.lqip_data_uri.clone(),
            is_animated: meta.is_animated,
        };
        write_cached_meta(objects, transform_cache, hash, size, &cached);
    }

    meta
}

/// Identifies the most likely homepage file from a collection of files.
///
/// Uses a priority-based detection algorithm to find the main entry point
/// for the website. Prioritizes conventional homepage filenames and falls
/// back to alphabetical ordering for ambiguous cases.
///
/// # Priority Order
/// 1. Index stems × `.md` (index, readme, _index, main — in priority order)
/// 2. `index.pages` - macOS Pages document
/// 3. `index.docx` - Microsoft Word document
/// 4. First document file alphabetically
///
/// # Arguments
/// * `files` - Complete list of files discovered in the project
///
/// # Returns
/// * `Some(String)` - Relative path to the detected homepage file
/// * `None` - No suitable homepage file found
// Not cfg(test): the app crate's build_tests call this across the crate
// boundary, where a cfg(test) item would be configured out.
pub fn detect_homepage_file(files: &[FileInfo]) -> Option<String> {
    detect_homepage_file_in_folder(files, "")
}

/// Detect homepage file with folder-name awareness for self-named folder notes.
///
/// Delegates to `moss_core::home::detect_home_file_in_folder` for consistent
/// priority logic across the entire codebase.
pub fn detect_homepage_file_in_folder(files: &[FileInfo], folder_name: &str) -> Option<String> {
    let root_files: Vec<&FileInfo> = files.iter()
        .filter(|f| !f.path.contains('/')) // Only files in root directory
        .collect();

    let filenames: Vec<&str> = root_files.iter().map(|f| f.path.as_str()).collect();
    moss_core::home::detect_home_file_in_folder(&filenames, folder_name).map(|s| s.to_string())
}

/// [`home_marker_of`], folding "I could not read it" into `false`. The build
/// wants that fold — a page it cannot read it cannot render, so it is not this
/// build's home either way. The **editor** must not fold it.
pub fn file_has_home_marker(path: &std::path::Path) -> bool {
    home_marker_of(path).unwrap_or(false)
}

/// Does this file carry `home: true`? `None` when the answer is unknowable
/// right now because the bytes are still in the cloud.
///
/// The three-valued form is the editor's, and load-bearing there: a home that
/// is neither index-named nor self-named is provable ONLY by reading it, and
/// under the dataless fail-fast policy that read fails on an evicted file.
/// Folded to `false`, "I could not check" becomes "it is not the home" — the
/// root resolves to nothing and the editor offers to create a home page over
/// the one that exists (moss#1062).
///
/// `is_offline_not_absent` is the classifier — the same `outcome::io_stop` and
/// `cloud_readiness` consult. Any other read failure (permissions, bad I/O) is
/// a definite `Some(false)`: waiting will not fix those. Bounded head read.
pub fn home_marker_of(path: &std::path::Path) -> Option<bool> {
    head_frontmatter_of(path).map(|fm| fm.home == Some(true))
}

/// Bounded-head frontmatter read: the first 8KB of the file, parsed through the
/// renderer's own parser selection (page_map::compute_home_overrides) —
/// traditional `---` frontmatter vs simplified (no leading `---`). Using only
/// the simplified parser silently misses fields on real `---` files.
///
/// `None` means the bytes are still in the cloud (`is_offline_not_absent`) and
/// the answer is unknowable right now; the caller decides what that folds to.
/// This NEVER materializes an evicted file — it is safe on interactive paths.
/// Any other read failure (permissions, bad I/O) parses as empty frontmatter:
/// waiting will not fix those.
pub fn head_frontmatter_of(
    path: &std::path::Path,
) -> Option<moss_core::frontmatter_typed::FrontMatter> {
    use std::io::Read;
    let mut buf = [0u8; 8192];
    let n = match std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => n,
        Err(e) if crate::build::icloud::is_offline_not_absent(path, &e) => return None,
        Err(_) => 0,
    };
    let head = String::from_utf8_lossy(&buf[..n]);
    Some(if crate::build::markdown::is_simplified_frontmatter(&head) {
        crate::build::markdown::parse_simplified_frontmatter(&head).0
    } else {
        crate::build::markdown::parse_typed_frontmatter(&head)
    })
}

/// Marker-aware [`detect_homepage_file_in_folder`]: a root file carrying the
/// `home: true` marker wins over filename rules (matches the renderer and
/// survives a folder rename). `root` is the folder's absolute path, used to read
/// each root markdown file's frontmatter.
pub fn detect_homepage_file_in_folder_marked(
    files: &[FileInfo],
    folder_name: &str,
    root: &std::path::Path,
) -> Option<String> {
    let filenames: Vec<&str> = files
        .iter()
        .filter(|f| !f.path.contains('/'))
        .map(|f| f.path.as_str())
        .collect();
    let marked: Vec<String> = files
        .iter()
        .filter(|f| !f.path.contains('/') && f.file_type == "md")
        .filter(|f| file_has_home_marker(&root.join(&f.path)))
        .map(|f| f.path.clone())
        .collect();
    let marked_refs: Vec<&str> = marked.iter().map(|s| s.as_str()).collect();
    moss_core::home::detect_home_file_in_folder_marked(&filenames, folder_name, &marked_refs)
        .map(|s| s.to_string())
}

/// Performs recursive directory analysis for static site generation.
///
/// Walks through the specified folder and all subdirectories to build
/// a complete inventory of files, categorized by type and purpose.
/// Also performs content analysis to determine the optimal site structure.
///
/// # File Categorization
/// * **Markdown**: `.md`, `.markdown`, `.mdown`, `.mkd`
/// * **HTML**: `.html`, `.htm`
/// * **Images**: `.jpg`, `.jpeg`, `.png`, `.gif`, `.svg`, `.webp`
/// * **Documents**: `.pages`, `.docx`, `.doc` (stored in other_files)
/// * **Other**: All remaining file types
///
/// # Content Analysis
/// Automatically detects:
/// - Homepage file candidates
/// - Content organization folders
/// - Recommended site structure type
///
/// # Arguments
/// * `folder_path` - Absolute path to the directory to analyze
///
/// # Returns
/// * `Ok(ProjectStructure)` - Complete analysis with file inventory and recommendations
/// * `Err(String)` - Error message if folder is inaccessible or invalid
///
/// # Errors
/// - Folder does not exist
/// - Path is not a directory
/// - Permission denied during scanning
pub fn scan_folder(folder_path: &str) -> Result<ProjectStructure, String> {
    scan_folder_with_dedup(folder_path, None)
}

/// Build/deploy default: extract image placeholders synchronously (no deferral).
/// Preview callers use `scan_folder_with_dedup_emit(.., defer_placeholders=true)`.

/// Scan a folder with optional singleflight dedup for metadata extraction.
///
/// When `metadata_dedup` is `Some`, concurrent calls that hit a TransformCache
/// miss for the same file content will share the extraction result instead of
/// duplicating work (image dimensions, dominant color, ffprobe).
///
/// When `metadata_dedup` is `None`, behaves identically to `scan_folder()`.
pub fn scan_folder_with_dedup(
    folder_path: &str,
    metadata_dedup: Option<&crate::build::cache::Singleflight<MediaMetadata>>,
) -> Result<ProjectStructure, String> {
    scan_folder_with_dedup_emit(folder_path, metadata_dedup, None, false)
}

/// Like `scan_folder_with_dedup`, but threads a `ScanEventEmitter` through
/// the walk so callers can observe progress. Pass `None` to opt out
/// (e.g. CLI, tests that don't care about events).
pub fn scan_folder_with_dedup_emit(
    folder_path: &str,
    metadata_dedup: Option<&crate::build::cache::Singleflight<MediaMetadata>>,
    emitter: Option<&ScanEventEmitter>,
    // Preview/server mode: defer image dominant-color + LQIP to the background so
    // first paint is instant (see `extract_media_metadata_cached`). Build/deploy
    // passes `false` to extract them synchronously so the built HTML is complete.
    defer_placeholders: bool,
) -> Result<ProjectStructure, String> {
    let path = Path::new(folder_path);

    if !path.exists() {
        return Err(format!("Folder does not exist: {}", folder_path));
    }

    if !path.is_dir() {
        return Err(format!("Path is not a directory: {}", folder_path));
    }

    // Lazy FFmpeg resolution: only download/detect when the first video file is found.
    // Avoids ~5-10s download on builds with no video files (Task 4).
    let ffmpeg_cell: OnceCell<Option<FFmpegManager>> = OnceCell::new();

    // Set up cache infrastructure (derived from folder_path).
    let moss_cache_dir = path.join(".moss").join("build").join("cache");
    let hash_index_path = moss_cache_dir.join("hash-index.json");
    let old_index = HashIndex::load(&hash_index_path);
    let mut new_index = HashIndex::new();

    let objects = ObjectStore::new(moss_cache_dir.join("objects"));
    let transform_cache = TransformCache::new(
        moss_cache_dir.join("transforms"),
        ObjectStore::new(moss_cache_dir.join("objects")),
    );

    // Raster-image metadata extraction (header read + dominant-color/LQIP or
    // stat-key cache I/O) is ~99% of a cold scan and is independent per file, so
    // it runs in a parallel phase 2 after the serial walk. The walk collects an
    // ImageEntry per raster image; videos + non-media are handled inline (video
    // uses a !Sync ffmpeg OnceCell, so it stays serial).
    struct ImageEntry {
        abs_path: std::path::PathBuf,
        relative_path: String,
        extension: String,
        stat: FileStat,
        modified: Option<String>,
    }

    let mut markdown_files = Vec::new();
    let mut html_files = Vec::new();
    let mut image_files: Vec<MediaMetadata> = Vec::new();
    let mut image_entries: Vec<ImageEntry> = Vec::new();
    let mut video_files = Vec::new();
    let mut notebook_files = Vec::new();
    let mut other_files = Vec::new();
    // Every non-excluded content directory (project-relative, raw path). Lets
    // the renderer emit an index page for EVERY folder, including empty ones.
    let mut dirs: Vec<String> = Vec::new();
    let mut evicted_count: usize = 0;
    let mut evicted_paths: Vec<std::path::PathBuf> = Vec::new();
    let mut found_count: usize = 0;

    let scan_walk_start = std::time::Instant::now();

    // Nested moss sites pruned by the walk below, reported once after it.
    let mut nested_boundaries: Vec<std::path::PathBuf> = Vec::new();

    // Walk through the directory recursively. `is_excluded_dir_name` applies to
    // DIRECTORY entries only — a file named `_43A2045.jpg` must not be filtered
    // by it. Files get one rule of their own: `skip_root_agent_config`.
    for entry in WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            if !e.file_type().is_dir() {
                return !skip_root_agent_config(&name, e.depth());
            }
            if is_excluded_dir_name(&name) {
                return false;
            }
            // Nested-vault boundary (2026-08-19 design §4): a descendant
            // owning its own `.moss/` is a different site — the outer build
            // goes around it rather than absorbing its pages.
            if e.depth() > 0 && e.path().join(".moss").is_dir() {
                nested_boundaries.push(e.path().to_path_buf());
                return false;
            }
            true
        }) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::warn!("Failed to read entry: {}", e);
                continue;
            }
        };

        // Skip directories, only process files — but first record the
        // directory path so empty/childless folders still get an index page.
        // The WalkDir `filter_entry` above already pruned excluded dirs
        // (`.moss`, `.git`, dotfiles, `node_modules`), so any directory entry
        // reaching here is an allowed content folder — this mirrors the file
        // exclusions exactly. Skip the scanned root itself (depth 0), whose
        // page is the homepage rather than a folder index. Symlinks and other
        // non-file, non-dir entries fall through unrecorded.
        if !entry.file_type().is_file() {
            if entry.file_type().is_dir() && entry.depth() > 0 {
                let dir_path = entry.path();
                let dir_rel = match dir_path.strip_prefix(path) {
                    Ok(rel) => moss_core::slug::normalize_separators(&rel.to_string_lossy()),
                    Err(_) => moss_core::slug::normalize_separators(&dir_path.to_string_lossy()),
                };
                if !dir_rel.is_empty() {
                    dirs.push(dir_rel);
                }
            }
            continue;
        }

        let file_path = entry.path();
        if crate::build::icloud::is_evicted(file_path) {
            evicted_count += 1;
            evicted_paths.push(file_path.to_path_buf());
        } else if let Some(real) = crate::build::icloud::icloud_stub_target(file_path) {
            // Pre-Sonoma eviction (macOS 12–13, still a supported minimum)
            // swaps the file out for this hidden `.name.icloud` placeholder, so
            // the real name never appears in this walk. Record what it stands
            // for: without it the page reads as deleted rather than offline,
            // and the build would go on to delete its still-good HTML.
            evicted_count += 1;
            evicted_paths.push(real);
        }
        // Normalize `\`→`/` so FileInfo.path (which flows into URLs, manifest keys,
        // and path comparisons) is OS-independent. On Windows strip_prefix yields
        // backslash-separated relative paths; left raw they collapse nested URLs
        // and pollute manifest keys. moss treats `\` as a separator everywhere.
        let relative_path = match file_path.strip_prefix(path) {
            Ok(rel_path) => moss_core::slug::normalize_separators(&rel_path.to_string_lossy()),
            Err(_) => moss_core::slug::normalize_separators(&file_path.to_string_lossy()),
        };

        // Get file metadata
        let metadata = match entry.metadata() {
            Ok(meta) => meta,
            Err(e) => {
                log::warn!("Failed to read metadata for {}: {}", relative_path, e);
                continue;
            }
        };

        let size = metadata.len();
        let mtime_secs = metadata.modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let modified = if mtime_secs > 0 {
            Some(mtime_secs.to_string())
        } else {
            None
        };

        // Determine file type based on extension
        let extension = file_path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Categorize files by extension. `classify_extension` is the single
        // definition of this match (moss#1087) — a caller outside this crate
        // that needs the same verdict calls it directly instead of
        // hand-maintaining its own extension list that can drift from this one.
        // ADR-006: Images and videos use MediaMetadata for dimensions and dominant color
        match classify_extension(&extension) {
            ScanBucket::Page => {
                markdown_files.push(FileInfo {
                    path: relative_path,
                    file_type: extension.clone(),
                    size,
                    modified,
                });
            }
            ScanBucket::Html => {
                html_files.push(FileInfo {
                    path: relative_path,
                    file_type: extension.clone(),
                    size,
                    modified,
                });
            }
            ScanBucket::Image => {
                // ADR-006 metadata (dimensions + dominant color/LQIP or stat-key
                // cache I/O) is the dominant cold-scan cost; defer it to the
                // parallel phase 2 below and just record the entry here (walk
                // order preserved). The synthesizer always emits `<picture>` for
                // raster originals; the preview server's AssetRegistry handles
                // the placeholder lifecycle at request time.
                image_entries.push(ImageEntry {
                    abs_path: file_path.to_path_buf(),
                    relative_path,
                    extension: extension.clone(),
                    stat: FileStat::of(&metadata),
                    modified,
                });
            }
            ScanBucket::Video => {
                // ADR-006: Videos use FFmpeg for dimensions and dominant color, with caching.
                // Lazy resolution (Task 4): FFmpeg is only downloaded/detected when
                // the first video file is encountered during the directory walk.
                let ffmpeg = ffmpeg_cell.get_or_init(|| {
                    match FFmpegManager::get_or_download(None) {
                        Ok(mgr) => Some(mgr),
                        Err(e) => {
                            log::debug!("FFmpeg not available during scan: {}", e);
                            None
                        }
                    }
                });
                let media_meta = extract_media_metadata_cached(
                    file_path,
                    &relative_path,
                    &extension,
                    &FileStat::of(&metadata),
                    modified,
                    ffmpeg.as_ref(),
                    &old_index,
                    &mut new_index,
                    &objects,
                    &transform_cache,
                    metadata_dedup,
                    defer_placeholders,
                );
                video_files.push(media_meta);
            }
            // Jupyter notebooks: detected here, processed in background phase.
            // No download or processing during scan — just categorization.
            // JupyterLite assets are lazy-downloaded during build's background
            // phase when notebook_files is non-empty (see build.rs).
            ScanBucket::Notebook => {
                notebook_files.push(FileInfo {
                    path: relative_path,
                    file_type: extension.clone(),
                    size,
                    modified,
                });
            }
            ScanBucket::Document | ScanBucket::Other => {
                other_files.push(FileInfo {
                    path: relative_path,
                    file_type: extension.clone(),
                    size,
                    modified,
                });
            }
        }

        // Bump the running found counter once per file that made it past the
        // directory filter, then let the emitter decide whether to publish
        // a throttled scan-progress event.
        found_count += 1;
        if let Some(e) = emitter {
            e.emit_progress_throttled(found_count, evicted_count);
        }
    }

    // One warning per scan naming every pruned nested site, so the exclusion
    // is never silent (2026-08-19 design §4).
    if !nested_boundaries.is_empty() {
        let names: Vec<String> = nested_boundaries
            .iter()
            .map(|p| {
                p.strip_prefix(path)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        log::warn!(
            "Skipped {} nested moss site(s) inside this folder: {} — each builds as its own site",
            names.len(),
            names.join(", ")
        );
    }

    let walk_elapsed = scan_walk_start.elapsed();

    // Phase 2: extract raster-image metadata in PARALLEL. Each task uses a fresh
    // local HashIndex (extract_media_metadata_cached needs `&mut`; on a warm
    // build it records the file's carried-forward content hash) — merged into
    // new_index in walk order afterward. `old_index` is read-only shared;
    // ObjectStore/TransformCache are path-only structs (Sync) whose writes go
    // through content-addressed / stat-keyed atomic temp+rename, so concurrent
    // writes to distinct keys don't conflict. Videos are NOT here — their ffmpeg
    // resolution uses a !Sync std OnceCell and stays on the serial walk.
    let img_phase_start = std::time::Instant::now();
    let img_n = image_entries.len();
    if defer_placeholders {
        // PREVIEW path — dimensions only (header read, ~1ms/image), the dominant
        // cold-scan cost. Parallelize with RAW OS THREADS, not rayon: a full JPEG
        // decode goes through jpeg-decoder, which itself parallelizes with rayon,
        // so calling it from inside a rayon `par_iter` NESTS rayon on the same
        // pool and non-deterministically corrupts decodes ("not an SOI marker" on
        // valid files). std::thread keeps any jpeg-decoder rayon at top level.
        // (Header-only dimension reads don't full-decode, so this path is doubly
        // safe; the build/deploy branch below, which DOES full-decode for
        // color+LQIP, must stay serial.)
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Mutex;

        let n_entries = image_entries.len();
        let next = AtomicUsize::new(0);
        // Indexed result slots preserve walk order regardless of completion order.
        let results: Vec<Mutex<Option<(MediaMetadata, HashIndex)>>> =
            (0..n_entries).map(|_| Mutex::new(None)).collect();

        // Only RASTER images run on worker threads. They take the dimensions-only
        // (header read) deferred path and never full-decode. SVG is NOT raster —
        // `is_image` in extract_media_metadata_cached excludes it, so it falls
        // through to the full-extract path that calls `image::open`. That must
        // never run on a worker (image::open decodes via jpeg-decoder, which
        // nests rayon → corruption). Today image 0.24 can't decode SVG so it's
        // inert, but keep the parallel path structurally decode-free: process
        // SVG (and any future non-raster) SERIALLY, into the same indexed slots.
        let is_raster =
            |ext: &str| matches!(ext, "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif");
        let extract_one = |e: &ImageEntry| -> (MediaMetadata, HashIndex) {
            let mut local_index = HashIndex::new();
            let meta = extract_media_metadata_cached(
                &e.abs_path,
                &e.relative_path,
                &e.extension,
                &e.stat,
                e.modified.clone(),
                None, // images don't need FFmpeg
                &old_index,
                &mut local_index,
                &objects,
                &transform_cache,
                metadata_dedup,
                defer_placeholders,
            );
            (meta, local_index)
        };

        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(n_entries.max(1));
        std::thread::scope(|scope| {
            for _ in 0..n_threads {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n_entries {
                        break;
                    }
                    let e = &image_entries[i];
                    if !is_raster(&e.extension) {
                        continue; // non-raster handled serially below
                    }
                    *results[i].lock().unwrap() = Some(extract_one(e));
                });
            }
        });
        // Serial pass for non-raster (SVG) entries — decode-free guarantee.
        for (i, e) in image_entries.iter().enumerate() {
            if is_raster(&e.extension) {
                continue;
            }
            *results[i].lock().unwrap() = Some(extract_one(e));
        }
        for slot in results {
            if let Some((meta, local_index)) = slot.into_inner().unwrap() {
                new_index.entries.extend(local_index.entries);
                image_files.push(meta);
            }
        }
    } else {
        // BUILD/DEPLOY path — full pixel decode for dominant color + LQIP. Runs
        // SERIALLY: jpeg-decoder's internal rayon makes concurrent full decodes
        // non-deterministic (verified: a colliding decode yields varying color/
        // dims across runs). This path is not latency-critical (deploy, not the
        // preview first paint), so correctness wins.
        for e in &image_entries {
            let meta = extract_media_metadata_cached(
                &e.abs_path,
                &e.relative_path,
                &e.extension,
                &e.stat,
                e.modified.clone(),
                None,
                &old_index,
                &mut new_index,
                &objects,
                &transform_cache,
                metadata_dedup,
                defer_placeholders,
            );
            image_files.push(meta);
        }
    }

    log::debug!(target: "timing", "[scan] walk={:?} image_meta(parallel, {} imgs)={:?}",
        walk_elapsed, img_n, img_phase_start.elapsed());

    // Save the new hash index (only entries seen this scan — auto-prunes stale).
    if let Err(e) = new_index.save(&hash_index_path) {
        log::warn!("Failed to save hash index: {}", e);
    }

    let total_files = markdown_files.len() + html_files.len() + image_files.len() + video_files.len() + notebook_files.len() + other_files.len();

    // Combine non-media files for homepage analysis
    let all_files: Vec<FileInfo> = markdown_files.iter()
        .chain(html_files.iter())
        .chain(other_files.iter())
        .cloned()
        .collect();

    // Detect content patterns — pass folder name for self-named folder note support.
    // The root name decides whether a self-named note is this folder's home. Routed
    // through the owner rather than re-derived: `scan_folder` keeps its `&str` argument
    // (68 call sites, mostly tests) and `VaultRoot::resolve` is I/O-free for the absolute
    // paths every production caller passes.
    let root = crate::vault::paths::VaultRoot::resolve(folder_path);
    let homepage_file = detect_homepage_file_in_folder_marked(&all_files, root.name(), path);

    // Extract resolved ffmpeg path from the OnceCell (if it was ever initialized).
    // This is None when no video files were found (ffmpeg was never needed).
    let ffmpeg_bin_path = ffmpeg_cell
        .get()
        .and_then(|opt| opt.as_ref())
        .map(|mgr| mgr.bin_path().to_string());

    // Detect content folders: root-level non-excluded directories containing .md/.html files.
    // Uses the same exclusion logic as the directory walk (single source of truth).
    let has_content_folders = markdown_files.iter().chain(html_files.iter()).any(|f| {
        let parts: Vec<&str> = f.path.splitn(2, '/').collect();
        if parts.len() < 2 { return false; }
        !crate::build::scan::classify::is_excluded_dir_name(parts[0])
    });

    // Detect language-prefix subtrees: any content file under a top-level folder
    // named with a known language code (e.g. `en/`, `zh-hans/`). `f.path` is
    // root-relative, so `lang_tree_prefix` keys on the first path component.
    // `markdown_files`/`html_files` are already exclusion-filtered by the walk (same
    // input as `has_content_folders` above), so no `is_excluded_dir_name` re-check is
    // needed here. Recognized codes are `moss_core::home::KNOWN_LANG_SUFFIXES`; a tree
    // using a code outside that list silently won't be scoped — extend the list, or
    // adopt the filed explicit-`languages`-config direction, to cover it.
    // Gates the root homepage's default-language-tree listing scope (the root home
    // lists only the default tree on a multilingual site). See
    // docs/archive/2026-06-06-multilingual-children-scoping-design.md.
    let has_language_trees = markdown_files.iter().chain(html_files.iter())
        .any(|f| moss_core::home::lang_tree_prefix(&f.path).is_some());

    if let Some(e) = emitter {
        let cloud_provider = if evicted_count > 0 {
            crate::build::cloud_provider::detect_from_path(path)
                .and_then(|p| p.display_name())
                .map(|s| s.to_string())
        } else {
            None
        };
        e.emit_complete(found_count, evicted_count, cloud_provider);
    }

    // Compute passthrough subtree roots: auto-detect dirs with index.html, apply [build].passthrough overrides.
    let config_passthrough = crate::build::site_config::get_build_passthrough(folder_path)
        .unwrap_or_else(|e| {
            log::warn!("[scan] failed to read [build].passthrough from config.toml: {}", e);
            vec![]
        });
    let passthrough_roots = crate::build::scan::classify::compute_passthrough_roots(
        &html_files, &config_passthrough,
    );
    if !passthrough_roots.is_empty() {
        log::info!("[scan] {} passthrough subtree(s) detected", passthrough_roots.len());
        // Remove passthrough media from the conversion pipelines so a bundled
        // web app's own images/videos are copied byte-for-byte (WebP/transcode
        // would rewrite filenames the app's HTML/CSS/JS already reference).
        // copy_deferred_assets still copies them verbatim via its own WalkDir.
        //
        // We deliberately DO NOT filter `html_files`: its only consumer is the
        // folder-embed iframe lookup (folder_embed.rs), so keeping a passthrough
        // `index.html` here lets `![[my-app]]` embed the app as an iframe. The
        // blocking phase never reads `html_files`, and SPA meta-injection is
        // guarded separately by `passthrough_roots` in copy_deferred_assets, so
        // retaining these entries causes no rewriting.
        //
        // We also DO NOT filter `markdown_files` / `notebook_files`. The
        // motivating case (a JS/HTML/CSS/image SPA) ships neither, and
        // copy_deferred_assets skips `.md`/`.ipynb` extensions — filtering them
        // out of these lists would drop the file entirely rather than copy it
        // verbatim. True raw passthrough of `.md`/`.ipynb` is a deliberate
        // follow-up (would require copy_deferred_assets to copy passthrough-path
        // docs despite their extension).
        image_files.retain(|f| !crate::build::scan::classify::is_in_passthrough(&f.path, &passthrough_roots));
        video_files.retain(|f| !crate::build::scan::classify::is_in_passthrough(&f.path, &passthrough_roots));
    }

    // Narrow `dirs` to what it is FOR — see `classify::gets_index_page`.
    let attachment_folder = crate::build::site_config::load_attachment_folder(folder_path);
    dirs.retain(|d| crate::build::scan::classify::gets_index_page(d, &passthrough_roots, &attachment_folder));

    Ok(ProjectStructure {
        root_path: folder_path.to_string(),
        markdown_files,
        html_files,
        image_files,
        video_files,
        notebook_files,
        other_files,
        total_files,
        homepage_file,
        ffmpeg_bin_path,
        evicted_count,
        evicted_paths,
        has_content_folders,
        has_language_trees,
        passthrough_roots,
        dirs,
    })
}

// =========================================================================
// ContentGraph population (Task 10)
// =========================================================================

use moss_core::content_graph::{ContentGraph, ContentGraphBuilder, generate_slug};

/// Build a [`ContentGraph`] from the scanned project structure.
///
/// Indexes all files (markdown, images, videos, other) for fuzzy path
/// resolution.
///
/// # Arguments
/// * `project_structure` - The scanned folder analysis
pub fn build_content_graph(project_structure: &ProjectStructure) -> ContentGraph {
    let mut builder = ContentGraphBuilder::new();

    // 1. Index files from ProjectStructure (content directories).
    for file_info in &project_structure.markdown_files {
        let slug = generate_slug(&file_info.path);
        builder.add_file(&file_info.path, &slug);
    }
    for media in &project_structure.image_files {
        let slug = generate_slug(&media.path);
        builder.add_file(&media.path, &slug);
    }
    for media in &project_structure.video_files {
        let slug = generate_slug(&media.path);
        builder.add_file(&media.path, &slug);
    }
    for file_info in &project_structure.notebook_files {
        let slug = generate_slug(&file_info.path);
        builder.add_file(&file_info.path, &slug);
    }
    for file_info in &project_structure.html_files {
        let slug = generate_slug(&file_info.path);
        builder.add_file(&file_info.path, &slug);
    }
    for file_info in &project_structure.other_files {
        let slug = generate_slug(&file_info.path);
        builder.add_file(&file_info.path, &slug);
    }

    // The main scan now walks formerly-excluded asset folders (assets/, images/,
    // etc.) so their files are already indexed above. Bare-filename wikilink
    // resolution like `![](photo.jpg)` works through the same index.

    builder.build()
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
