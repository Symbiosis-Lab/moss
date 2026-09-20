//! Image compression pipeline (WebP variant generation).
//!
//! Mirrors the structure of `build/video.rs`: content-addressed cache,
//! async background task, per-asset error tolerance.
//!
//! ## Pipeline Overview
//!
//! ```text
//! Source (.jpg, .png) → Cache (.webp) → Output (staging_dir + canonical_dir)
//! ```
//!
//! Each image is processed through `convert_single_image()`:
//! 1. Cache lookup (return WebP from CAS if a prior encode matched current params)
//! 2. Decode via `image` crate (handles JPEG/PNG/WebP)
//! 3. Apply EXIF orientation to pixels
//! 4. Resize if larger than `max_edge`
//! 5. Encode WebP at configured quality
//! 6. Validate encoded output
//! 7. CAS store + write transform record
//!
//! WebP is always written, even when the encoded output is larger than the
//! source. The synthesizer emits `<picture><source srcset="…webp">` for every
//! raster original; a missing variant 404s the `<source>` and the browser
//! does NOT fall back to the sibling `<img>` (HTML spec § update-the-source-set).
//! Skipping the encode to save bytes broke ~95% of imported journalism covers
//! on test-sites/yi-liu in the moss-import dogfood (2026-05-21).
//!
//! ## Cross-crate caveat
//!
//! The `webp` crate (v0.3) builds against `image = 0.25`, while this crate
//! uses `image = 0.24`. We therefore do not call `webp::Encoder::from_image`
//! — instead we extract raw RGB/RGBA bytes from the 0.24 `DynamicImage` and
//! pass them to `webp::Encoder::from_rgb` / `from_rgba`. This avoids pulling
//! in a second copy of the `image` crate and sidesteps the API mismatch.
//!
//! ## ICC profile preservation
//!
//! Deferred for v1. The `webp` 0.3 crate's simple encoder API does not expose
//! WebPMux-based ICCP chunk embedding; adding it would require using
//! `libwebp-sys` directly. Wide-gamut color preservation is tracked as a
//! follow-up (see plan "Risks & follow-ups" → wide-gamut color).
//! TODO: preserve ICC profile via WebPMux

use crate::moss_paths::MossPaths;
use crate::types::services::{BackgroundContext, BuildServices};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc;

use crate::build::coordinator::EmitMessage;
use crate::build::lifecycle::cas_heal::{rematerialize, HashPolicy, HealOutcome};
use crate::build::manifest::HashBucket;
use crate::advisory::{Action, Advisory, Scope, Severity};
use crate::build::progress::{format_progress_message, PipelineEvent};
use serde::{Deserialize, Serialize};
// The rung encode unit lives in the sibling `rungs` module (extracted per
// MIGRATION-STATE's image.rs debt row, Task 10.5). The dispatch-side rung loop
// and the fingerprint-skip heal block stay here and CALL into it.
use super::rungs::{decode_oriented, encode_rungs, RungOutcome};
use super::sniff::{is_animated_gif, is_animated_webp, is_cmyk_jpeg};

// ---------------------------------------------------------------------------
// Data types (foundation from Task 1)
// ---------------------------------------------------------------------------

/// One image queued for conversion to WebP.
///
/// Populated during scan, drained by the background image conversion task.
#[derive(Debug, Clone)]
pub struct ImageConversionItem {
    /// Absolute path to the source image on disk.
    pub source_path: PathBuf,
    /// Content-addressed OID of the source file (sha256 hex).
    pub source_oid: String,
    /// Lowercase extension without dot ("jpg", "png", "webp", etc.).
    pub ext: String,
    /// Source pixel dimensions from the scan (`None` if the header read failed).
    /// Drives the background encoder's megapixels-in-flight memory budget so a
    /// handful of huge images can't decode concurrently and OOM the process.
    pub dimensions: Option<(u32, u32)>,
    /// `None`: encode. `Some`: this build will not encode the variants, but
    /// the item is collected anyway — deliberately — so it reaches the
    /// registration loop in `media/promise.rs`, which settles its variant
    /// URLs instead of leaving them unregistered. The synthesizer emits
    /// `<picture><source srcset="…webp">` for every png/jpg/jpeg from the
    /// extension alone and cannot see registration, and a chosen `<source>`
    /// that 404s is not recoverable (ADR-013).
    ///
    /// - [`SkipReason::SourceInTheCloud`] (moss#982): registered Pending with
    ///   a source passthrough, because the bytes are on their way down. It
    ///   must NOT reach the encoder, which would read the source, take an
    ///   `EDEADLK`, and mark the variant `Failed` — turning "still
    ///   downloading" into "⚠ asset failed" in the preview. See
    ///   `build::cloud_ledger` for why the absence is recorded rather than
    ///   silently handled here.
    /// - [`SkipReason::Cmyk`] / [`SkipReason::NotAnImage`]: registered
    ///   `Failed`, so the post-seal degrade pass (moss#867) drops the
    ///   `<source>` and the page falls through to the original `<img>`.
    ///   Before this a CMYK JPEG shipped a `<source>` that 404ed and the
    ///   browser showed nothing at all (zhu-da's 河上花圖, 2026-09-05).
    ///
    /// Every other verdict is a bare `<img>` on the emission side and needs
    /// no registration; those items are dropped in the collector.
    pub skip: Option<SkipReason>,
}

/// Outcome of converting a single image to WebP.
///
/// This struct is the value carried through the singleflight dedup map so all
/// concurrent callers sharing a single conversion receive the full result —
/// including any error — without needing an out-of-band side channel.
///
/// Convention: when `error.is_some()`, the conversion failed. The other fields
/// hold placeholder zeroes/None and must not be interpreted as meaningful output.
#[allow(dead_code)] // Fields read by dispatch layer and tests
#[derive(Debug, Clone)]
pub struct ImageConversionOutcome {
    /// OID of the encoded WebP in the object store. `None` only on
    /// conversion error (in which case `error` is `Some`).
    pub webp_oid: Option<String>,
    /// Size of the encoded WebP in bytes (0 on error).
    pub webp_size: u64,
    /// Size of the source image in bytes (for size-reduction reporting).
    pub original_size: u64,
    /// Set when conversion failed. All other fields are placeholder values.
    /// Carried inside the outcome so singleflight waiters receive the error
    /// message in-band.
    pub error: Option<String>,
    /// Per-rung results of the responsive ladder encode (Task 5). Empty when
    /// the source has no ladder (below threshold / non-raster gate) or when
    /// the BASE conversion failed — rungs only run after base success. The
    /// dispatch layer walks these to link/`set_ready`/register each rung for
    /// ITS OWN item paths (the outcome may be singleflight-shared across
    /// duplicate-content items at different paths).
    pub rungs: Vec<RungOutcome>,
}

/// Knobs for the WebP encode pass.
#[derive(Debug, Clone)]
pub struct ImageCompressionConfig {
    /// WebP quality 0-100. 80 is the visually-lossless sweet spot.
    pub quality: u8,
    /// Resize so max(width, height) <= max_edge. Default: `asset_paths::DEPLOY_MAX_EDGE` (2400, retina).
    pub max_edge: u32,
    /// Drop EXIF metadata (privacy + ~10-50KB savings). ICC profile is preserved separately.
    pub strip_exif: bool,
    /// Skip processing if source is smaller than this AND fits in max_edge.
    pub min_size_kb: u64,
}

impl Default for ImageCompressionConfig {
    fn default() -> Self {
        Self {
            quality: 80,
            max_edge: moss_core::asset_paths::DEPLOY_MAX_EDGE,
            strip_exif: true,
            min_size_kb: 200,
        }
    }
}

impl ImageCompressionConfig {
    /// Serialize the encoding-relevant parameters for cache keying.
    ///
    /// `min_size_kb` is intentionally omitted — it's a skip-decision input,
    /// not an encoding parameter, so changing it should not invalidate
    /// cached encodings.
    pub(crate) fn to_params(&self) -> serde_json::Value {
        serde_json::json!({
            "quality": self.quality,
            "max_edge": self.max_edge,
            "strip_exif": self.strip_exif,
            "flatten_alpha": true,
        })
    }
}

// ---------------------------------------------------------------------------
// Skip rules
// ---------------------------------------------------------------------------

/// Why `convert_single_image` was skipped for a particular source.
///
/// Each variant corresponds to a predicate in `should_skip`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SkipReason {
    /// SVG — vector, already small and not usefully rasterized to WebP here.
    Svg,
    /// Animated GIF — animation-preserving WebP is out of scope for v1.
    AnimatedGif,
    /// Animated WebP — already WebP and animation-preserving re-encode is out of scope.
    AnimatedWebp,
    /// HEIC/HEIF — requires libheif, out of scope for v1.
    Heic,
    /// CMYK JPEG — libwebp only supports RGB; colorspace conversion is lossy.
    Cmyk,
    /// Source is small AND already fits within max_edge — re-encoding wastes CPU.
    AlreadySmall,
    /// File has an image extension but its magic bytes don't match any known
    /// image format. Common cause: an HTML 404 page saved with a .png/.jpg
    /// extension (e.g. from a broken download). Silently skipped so the user
    /// doesn't see a red advisory for a file that was never a real image.
    NotAnImage,
    /// The source bytes are still in the cloud, so nothing about this file's
    /// CONTENT can be decided yet (moss#982).
    ///
    /// Unlike every other variant, this one is **never cached** — see the
    /// guard at the top of [`should_skip`]. It is also not really a skip: the
    /// caller still collects the item so its variant promise gets registered,
    /// and only the encode is skipped.
    SourceInTheCloud,
}

/// What the first bytes of a file say about it — including "they did not say".
///
/// The third case is the point. This was a `bool`, and `false` meant both "the
/// bytes are not an image" and "there were no bytes to look at", which is how a
/// perfectly good PNG became permanently cached as [`SkipReason::NotAnImage`]:
/// a verdict keyed by content OID, about content nobody read, for a file whose
/// content never changes. Reproduced end to end on a real vault — three valid
/// images, zero `.webp` variants emitted, `<source srcset>` still emitted for
/// all three, and no warning printed (moss#985).
///
/// `should_skip` has a cloud guard upstream of this, but that guard asks
/// `icloud::is_evicted`, which is macOS-only and answers about *now* while the
/// read happens a moment later. This type is what makes the write-back safe
/// without depending on either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MagicVerdict {
    /// Magic bytes matched a known raster format.
    Image,
    /// Bytes were read, and they are not any format moss knows.
    NotAnImage,
    /// The bytes could not be read at all — eviction, permissions, a vanished
    /// file. Says nothing about the content, so nothing about the content may
    /// be cached.
    Unreadable,
}

/// Classifies the first bytes of `path` against the known raster magic
/// sequences. Reads at most 12 bytes.
///
/// An I/O error is [`MagicVerdict::Unreadable`], never `NotAnImage` — see
/// [`MagicVerdict`] for why the distinction is load-bearing rather than tidy.
///
/// Recognised signatures:
/// - PNG:  `\x89PNG`
/// - JPEG: `\xFF\xD8\xFF`
/// - GIF:  `GIF8`
/// - WebP: `RIFF????WEBP` (bytes 0-3 + 8-11)
/// - BMP:  `BM`
/// - TIFF: `II\x2A\x00` (LE) or `MM\x00\x2A` (BE) — 4-byte check to avoid
///   false positives from text files whose first two bytes happen to be "II"/"MM"
/// - AVIF/HEIC: `ftyp` at offset 4 (ISO Base Media file format)
fn image_magic(path: &Path) -> MagicVerdict {
    use std::io::Read;
    let mut buf = [0u8; 12];
    let Ok(mut f) = std::fs::File::open(path) else {
        return MagicVerdict::Unreadable;
    };
    // `read`, not `unwrap_or(0)`. On a dataless file `File::open` SUCCEEDS —
    // the fail-fast policy surfaces `EDEADLK` here, at the first read — so
    // swallowing this error was the whole bug: "the download has not finished"
    // arrived as "zero bytes", and zero bytes read as "not an image".
    let Ok(n) = f.read(&mut buf) else {
        return MagicVerdict::Unreadable;
    };
    // A short read is genuinely inconclusive too: a provider can hand back a
    // prefix, and a 1-byte file is not a format moss can rule out from bytes
    // it does not have. Costs one re-probe next build; the alternative is a
    // permanent verdict.
    if n < 2 {
        return MagicVerdict::Unreadable;
    }
    // PNG
    if n >= 4 && buf[..4] == [0x89, b'P', b'N', b'G'] {
        return MagicVerdict::Image;
    }
    // JPEG
    if n >= 3 && buf[..3] == [0xFF, 0xD8, 0xFF] {
        return MagicVerdict::Image;
    }
    // GIF
    if n >= 4 && &buf[..4] == b"GIF8" {
        return MagicVerdict::Image;
    }
    // WebP: "RIFF" at 0..4, "WEBP" at 8..12
    if n >= 12 && &buf[..4] == b"RIFF" && &buf[8..12] == b"WEBP" {
        return MagicVerdict::Image;
    }
    // BMP
    if n >= 2 && &buf[..2] == b"BM" {
        return MagicVerdict::Image;
    }
    // TIFF: little-endian b"II\x2A\x00" or big-endian b"MM\x00\x2A"
    // 4-byte check avoids false positives from text starting with "II"/"MM".
    if n >= 4 && (&buf[..4] == b"II\x2A\x00" || &buf[..4] == b"MM\x00\x2A") {
        return MagicVerdict::Image;
    }
    // AVIF / HEIC / HEIF — ISO Base Media: "ftyp" box at offset 4
    if n >= 8 && &buf[4..8] == b"ftyp" {
        return MagicVerdict::Image;
    }
    MagicVerdict::NotAnImage
}

/// Transform name under which `should_skip`'s cached verdict is stored in
/// [`crate::build::cache::TransformCache`]. Not a real converted-image
/// output — the "output blob" is a tiny JSON encoding of the `Option<SkipReason>`
/// verdict itself, stored via `ObjectStore::store_bytes` so it can reuse the
/// exact same content-addressed, params-aware, existence-verified cache
/// already trusted for real conversion outputs (`image/webp`,
/// `image/sized-raster`). See
/// docs/archive/2026-07-31-image-format-probe-cache-design.md for the design
/// rationale and prior art.
const FORMAT_PROBE_TRANSFORM: &str = "format-probe";

/// Bump whenever `probe_cmyk_magic_already_small`'s decision rules change
/// (new magic signature, different CMYK heuristic, a changed
/// `raster_with_picture`/rung precedence, etc). Unlike the `image/webp`/
/// `image/sized-raster` caches — whose entire behavior is parameterized by
/// `ImageCompressionConfig`, so a config change is already a cache miss —
/// this cache's verdict also depends on Rust code that isn't expressed as a
/// config value. `.moss/build/cache/transforms/` persists across moss
/// upgrades, so without a version in the key, a future logic change would
/// silently keep serving the OLD verdict for every unchanged file, forever.
/// Folded into `params` below so a version bump is an ordinary cache miss,
/// exactly like a config change.
///
/// **1 → 2** (moss#985): an unreadable source used to be indistinguishable from
/// a file that is not an image, so a cloud-evicted PNG could be cached as
/// `NotAnImage` — permanently, since the verdict is keyed by content OID and an
/// evicted file's content never changes. The decision rule changed, so the
/// version changes with it; that also invalidates every entry a build made while
/// the bug was live, which is the cheapest possible migration for vaults nobody
/// can inspect. It costs a re-probe of each image once, not a re-encode.
const FORMAT_PROBE_VERSION: u32 = 2;

impl SkipReason {
    /// Whether a source skipped for this reason still has to be collected so
    /// the registration loop can settle the `<picture><source>` the
    /// synthesizer emits for it — see `ImageConversionItem::skip`.
    ///
    /// The promise exists only for the conversion set (png/jpg/jpeg): webp
    /// carries its ladder on `<img srcset>` whose base URL IS the source, so
    /// marking that URL `Failed` would replace the file itself with a warning.
    pub(crate) fn settles_a_promised_variant(&self, ext: &str) -> bool {
        match self {
            SkipReason::SourceInTheCloud => true,
            SkipReason::Cmyk | SkipReason::NotAnImage => {
                moss_core::asset_paths::is_ladder_source_ext(ext)
                    && !moss_core::asset_paths::is_webp_source_ext(ext)
            }
            _ => false,
        }
    }

    /// `Some` for a verdict whose variants settle `Failed` — the message is
    /// the preview's warning tile until the degrade pass removes the
    /// `<source>`. `None` for one whose variants stay Pending (the bytes are
    /// on their way down) and for every verdict that is never collected.
    pub(crate) fn failure_message(&self) -> Option<&'static str> {
        match self {
            SkipReason::Cmyk => Some(
                "CMYK JPEG: published as the original — WebP has no CMYK and the colour conversion is lossy",
            ),
            SkipReason::NotAnImage => Some("not a recognised image format"),
            _ => None,
        }
    }
}

/// Decide whether to skip encoding a given source image.
///
/// Called before any decoding work. Two tiers:
///
/// 1. Extension-only checks (`Svg`/`Heic`) and the animated-gif/webp sniff —
///    always run fresh, never cached. The animated check is deliberately
///    left uncached per `scan.rs`'s documented policy (a stale cached
///    `is_animated` could silently mis-skip a file edited in place); it's
///    also already cheap (bounded ≤4 KB reads, extension-gated).
/// 2. The magic-byte / CMYK / AlreadySmall decision — genuinely expensive
///    (opens and decodes the file) and, unlike animated-ness, purely a
///    function of (source content, `min_size_kb`, `max_edge`): the same
///    bytes under the same config knobs always produce the same verdict.
///    Cached in `TransformCache` under [`FORMAT_PROBE_TRANSFORM`], keyed by
///    `source_oid` with those two config knobs as `params` — a changed
///    source or a changed config value is automatically a cache miss.
///    Covers every outcome uniformly, including `None` ("proceed to
///    conversion"), so skip-category images benefit exactly like
///    convert-bound ones.
pub(crate) fn should_skip(
    source_path: &Path,
    ext: &str,
    file_size: u64,
    config: &ImageCompressionConfig,
    transforms: &crate::build::cache::TransformCache,
    source_oid: &str,
    // Passed in rather than probed here so this function stays testable off
    // macOS, where `icloud::is_evicted` is a compile-time `false` and the
    // regression below could not otherwise be exercised at all.
    source_in_the_cloud: bool,
) -> Option<SkipReason> {
    // A source still in the cloud gets NO verdict, and — the part that matters
    // — no CACHED verdict (moss#982).
    //
    // This must precede every branch below, including the cheap extension ones,
    // because the cache read/write straddles them. The failure it prevents:
    // `probe_cmyk_magic_already_small` calls `has_image_magic`, which cannot
    // tell an unreadable file from a file that is not an image. `File::open`
    // SUCCEEDS on a dataless file — the fail-fast policy only surfaces
    // `EDEADLK` at the first `read`, where `.unwrap_or(0)` becomes "zero bytes"
    // and `n < 2` returns `false`. Verdict: `NotAnImage`.
    //
    // That verdict was then written back to the TransformCache under the
    // source's CONTENT oid. The content of an evicted file never changes, so
    // the OID never changes, so the verdict outlives the eviction that caused
    // it — forever. The image is dropped from `image_items` on every future
    // build, so nothing calls `set_pending` and nothing registers a
    // passthrough, while the synthesizer keeps emitting
    // `<picture><source srcset="…webp">` from the extension alone. A chosen
    // `<source>` that 404s is not recoverable (ADR-013), and the image-set
    // fingerprint is stat-keyed while eviction preserves size and mtime, so the
    // heal pass never revisits it either. A transient cloud state became
    // permanent corruption of the published site.
    //
    // `scan.rs` already refuses to write non-answers into the stat cache for
    // this exact reason ("a cached `dimensions: None` would be permanent"). The
    // general rule, which this guard is one instance of: never persist a
    // verdict derived from bytes you could not read.
    if source_in_the_cloud {
        return Some(SkipReason::SourceInTheCloud);
    }

    let ext_lower = ext.to_ascii_lowercase();

    if ext_lower == "svg" {
        return Some(SkipReason::Svg);
    }
    if ext_lower == "heic" || ext_lower == "heif" {
        return Some(SkipReason::Heic);
    }
    if ext_lower == "gif" && is_animated_gif(source_path) {
        return Some(SkipReason::AnimatedGif);
    }
    if ext_lower == "webp" && is_animated_webp(source_path) {
        return Some(SkipReason::AnimatedWebp);
    }

    // `min_size_kb`/`max_edge` are the only config fields the probe below
    // reads (see the AlreadySmall branch inside `probe_cmyk_magic_already_small`).
    // Folded into the cache key so a config change can never serve a stale
    // verdict computed under different settings.
    //
    // `ext_lower` is ALSO part of the key, even though the source content is
    // what's hashed into `source_oid` — two files can share identical bytes
    // under different extensions (e.g. a small icon saved as both `.bmp` and
    // `.png`), and `probe_cmyk_magic_already_small` branches on extension in
    // ADR-013-relevant ways: `raster_with_picture` forbids `AlreadySmall` for
    // png/jpg/jpeg specifically because the synthesizer emits an unconditional
    // `<picture><source>` for those. Without `ext` in the key, probing the
    // `.bmp` copy first would cache `Some(AlreadySmall)` and the `.png` copy
    // would then inherit that verdict, skip conversion, and strand the
    // `<source>` the synthesizer still emits — the exact 2026-05-19 failure
    // class ADR-013 documents.
    let params = serde_json::json!({
        "min_size_kb": config.min_size_kb,
        "max_edge": config.max_edge,
        "ext": ext_lower,
        "v": FORMAT_PROBE_VERSION,
    });

    // Empty `source_oid` means the caller couldn't resolve one on the cheap
    // stat-match fast path (new/changed file, index miss — see
    // `collect_images_for_conversion`). Never consult or write the cache in
    // that case: a shared "" key would let one file's verdict leak into an
    // unrelated file's result if two different unresolved-OID images are
    // probed in the same build. Falling through just means this file
    // recomputes fresh, same as before this cache existed.
    if !source_oid.is_empty() {
        if let Some(cached_oid) =
            transforms.find_cached_output(source_oid, FORMAT_PROBE_TRANSFORM, &params)
        {
            if let Some(blob_path) = transforms.objects().get_path(&cached_oid) {
                if let Ok(bytes) = fs::read(&blob_path) {
                    if let Ok(verdict) = serde_json::from_slice::<Option<SkipReason>>(&bytes) {
                        return verdict;
                    }
                }
            }
            // Cached blob vanished or is corrupt — fall through and recompute
            // fresh below; the write-back at the end repairs the entry.
        }
    }

    let verdict = probe_cmyk_magic_already_small(source_path, &ext_lower, file_size, config);

    // The second half of "never persist a verdict derived from bytes you could
    // not read", and the half that does not depend on `icloud::is_evicted`.
    // The guard at the top of this function asks the provider, on macOS, about
    // the state a moment BEFORE the read; this asks the read itself, on every
    // platform, and so also covers a permissions error, a file deleted mid-build,
    // and the race between those two moments. Returned to the caller, never
    // written down.
    if verdict == Some(SkipReason::SourceInTheCloud) {
        return verdict;
    }

    // Whole-record read-modify-write, same non-atomic pattern as the
    // `image/webp`/`image/sized-raster` writers below (mirrors the WebP
    // pass) — a benign race with a concurrent writer for the same
    // `source_oid` can at worst drop one entry, forcing a re-probe/re-encode
    // next build. Self-healing, never wrong: this call's own verdict is
    // still returned immediately regardless of whether the write-back lands.
    if !source_oid.is_empty() {
        if let Ok(json_bytes) = serde_json::to_vec(&verdict) {
            if let Ok(blob_oid) = transforms.objects().store_bytes(&json_bytes) {
                let mut record = transforms.get(source_oid).unwrap_or(
                    crate::build::cache::TransformRecord {
                        source_oid: source_oid.to_string(),
                        source_size: file_size,
                        transforms: std::collections::HashMap::new(),
                    },
                );
                record.transforms.insert(
                    FORMAT_PROBE_TRANSFORM.to_string(),
                    crate::build::cache::TransformEntry {
                        oid: blob_oid,
                        size: json_bytes.len() as u64,
                        params,
                    },
                );
                if let Err(e) = transforms.put(&record) {
                    log::warn!("[format-probe] failed to write transform record: {}", e);
                }
            }
        }
    }

    verdict
}

/// The expensive, content-stable portion of `should_skip`: magic-byte match,
/// CMYK-ness, and the AlreadySmall size/dimension carve-out. Extracted so
/// `should_skip` can cache its result as a single unit — see
/// [`FORMAT_PROBE_TRANSFORM`].
fn probe_cmyk_magic_already_small(
    source_path: &Path,
    ext_lower: &str,
    file_size: u64,
    config: &ImageCompressionConfig,
) -> Option<SkipReason> {
    if (ext_lower == "jpg" || ext_lower == "jpeg") && is_cmyk_jpeg(source_path) {
        return Some(SkipReason::Cmyk);
    }

    // Magic-byte check: if the file carries an image extension but its content
    // doesn't look like any known image format, skip it immediately.  The
    // canonical example is an HTML 404 page saved as .png (e.g. from a broken
    // download or a misconfigured server).  Without this guard the PNG decoder
    // attempts to parse HTML, fails, and surfaces a red "Image conversion
    // failed" advisory — alarming for something that was never an image.
    //
    // SVG is excluded above (no magic bytes to check for text formats).
    // HEIC/HEIF is excluded above (handled by extension before we reach here).
    // For all other image extensions, a magic MISMATCH means NotAnImage —
    // an unreadable file means nothing at all, and takes the SourceInTheCloud
    // path so the caller registers the variant promise and the verdict is
    // never cached. "In the cloud" is the overwhelmingly common cause; a
    // chmod-000 file takes the same route and is re-probed next build, which
    // is the correct outcome for both.
    match image_magic(source_path) {
        MagicVerdict::Image => {}
        MagicVerdict::NotAnImage => return Some(SkipReason::NotAnImage),
        MagicVerdict::Unreadable => return Some(SkipReason::SourceInTheCloud),
    }

    // AlreadySmall: both size AND dimension constraints met. Note: if
    // image_dimensions() fails (corrupt / non-image), we do NOT skip — let
    // convert_single_image fail per-image with a warning, rather than
    // silently shipping the original.
    //
    // Restricted to the CONVERSION set (png|jpg|jpeg). The synthesizer
    // (`render/image.rs::is_raster_original`) emits `<picture><source
    // srcset="…webp">` for every png/jpg/jpeg unconditionally, so skipping the
    // encode for those would 404 the source and break the picture chain (HTML
    // spec § update-the-source-set — the browser does NOT fall back to the
    // sibling <img>). For other formats (gif, bmp, tiff) the synthesizer emits
    // a bare <img>, so AlreadySmall remains a valid CPU optimization.
    //
    // WebP is NOT in the conversion set — despite joining `is_ladder_source_ext`
    // in Phase B (Task 12). A webp SOURCE is already webp: emission gives it
    // `<img src="photo.webp" srcset=…>` where the base `src` IS the source
    // (served verbatim on a skip), so a rung-FREE small webp can still take the
    // AlreadySmall skip and avoid a wasteful webp→webp re-encode — hence the
    // `&& !is_webp_source_ext(&ext_lower)` keeping webp out of
    // `raster_with_picture` so it falls into the carve-out below.
    //
    // A webp that DOES carry rungs must encode them — skipping would strand
    // emitted-but-unencoded rung candidates (non-recoverable srcset 404,
    // ADR-013). The `webp_carries_rungs` sub-test below draws that line.
    //
    // The `is_animated=false` literal is provably correct (an animated webp
    // returned SkipReason::AnimatedWebp above and never reaches here) — full
    // rationale on asset_paths::ladder_rungs.
    //
    // `webp_carries_rungs` must ask "does the ladder carry a rung?" on the SAME
    // oriented dims scan/emission/registration/encode use — scan now swaps EXIF
    // 5-8 dims for ladder sources (design follow-up #1). Else an EXIF-rotated
    // webp whose ORIENTED ladder is non-empty but STORED ladder is empty gets
    // AlreadySmall-skipped while emission still promises its rung → stranded,
    // never-encoded rung (non-recoverable publish-404, ADR-013). Only width-
    // derived emptiness is orientation-sensitive; `w.max(h)` is invariant, and
    // the extra bounded EXIF read is webp-only.
    let raster_with_picture = moss_core::asset_paths::is_ladder_source_ext(ext_lower)
        && !moss_core::asset_paths::is_webp_source_ext(ext_lower);
    if !raster_with_picture && file_size < config.min_size_kb.saturating_mul(1024) {
        if let Ok((w, h)) = image::image_dimensions(source_path) {
            let (lw, lh) = if ext_lower == "webp"
                && crate::build::scan::scan::should_swap_dimensions(read_exif_orientation(
                    source_path,
                )) {
                (h, w)
            } else {
                (w, h)
            };
            let webp_carries_rungs = ext_lower == "webp"
                && !moss_core::asset_paths::ladder_rungs(lw, lh, false).is_empty();
            if !webp_carries_rungs && w.max(h) < config.max_edge {
                return Some(SkipReason::AlreadySmall);
            }
        }
    }

    None
}

// `compute_webp_variant_for_scan` was deleted 2026-05-20. It computed a
// snapshot of the transform cache during scan, which the synthesizer then
// consulted to decide whether to emit `<picture><source srcset>`. That
// snapshot mechanism was the parallel-oracle root cause of the broken-hero
// bug verified at user log L2006/L2446/L2450 on 2026-05-19 — the snapshot
// could disagree with the served-path reality (e.g., when fingerprint-skip
// elided manifest registration, stale-cleanup deleted the file while the
// snapshot still claimed it existed).
//
// The synthesizer now always emits `<picture>` for raster originals; the
// preview server's AssetRegistry intercept handles the placeholder
// lifecycle at request time; publish-mode synchronous encoding ensures
// production parity. See docs/archive/2026-05-20-image-variant-honest-
// mirror.md (Layer 4).

/// Collects images that need WebP conversion from the scanned project.
///
/// For each image in `image_files`, applies skip rules. Images that pass
/// (i.e., should be encoded) are returned as `ImageConversionItem`s with
/// content-addressed `source_oid` taken from the hash index (`HashIndex::lookup`).
///
/// On hash-resolution error, the item is logged and skipped.
pub(crate) fn collect_images_for_conversion(
    project_structure: &crate::types::content::ProjectStructure,
    transforms: &crate::build::cache::TransformCache,
    hash_index: &mut crate::build::cache::HashIndex,
    config: &ImageCompressionConfig,
) -> Vec<ImageConversionItem> {
    let mut items = Vec::new();

    for media_meta in &project_structure.image_files {
        let file_path = Path::new(&project_structure.root_path).join(&media_meta.path);

        // Resolve the source OID by STAT-MATCH ONLY — never hash here. This runs
        // in the blocking phase that gates first paint, and hashing hundreds of
        // MB of images (SHA-256 of every 5-15 MB photo) previously added tens of
        // seconds before the preview could appear. On a cold/changed miss we
        // leave the OID empty; the background worker (`run_image_conversion`)
        // resolves it — hashing OFF the blocking path and persisting the index
        // so the next build stat-matches. Mirrors videos' deferred hashing.
        let source_oid = std::fs::metadata(&file_path)
            .ok()
            .and_then(|m| {
                hash_index
                    .lookup(&media_meta.path, &crate::build::cache::FileStat::of(&m))
                    .map(str::to_string)
            })
            .unwrap_or_default();

        // One `lstat`, no read — `is_evicted` never materializes anything, so
        // asking is cheap even for a vault that is entirely local.
        let source_in_the_cloud = crate::build::icloud::is_evicted(&file_path);
        if source_in_the_cloud {
            crate::build::cloud_ledger::note_unavailable(&file_path);
        }

        let file_size = media_meta.size;
        if let Some(reason) = should_skip(
            &file_path,
            &media_meta.file_type,
            file_size,
            config,
            transforms,
            &source_oid,
            source_in_the_cloud,
        ) {
            // A verdict that leaves an emitted `<source>` behind does NOT drop
            // the file: the item is collected, with the verdict, so the
            // registration loop can settle the variant URLs the synthesizer
            // has already promised — see `ImageConversionItem::skip`. The
            // `<picture>` promise is only ever made for png/jpg/jpeg, so a
            // `NotAnImage` gif or a mislabelled webp still drops here.
            if reason.settles_a_promised_variant(&media_meta.file_type) {
                log::debug!(
                    "Image {}: {:?} — registering its variants, skipping the encode",
                    media_meta.path,
                    reason
                );
                items.push(ImageConversionItem {
                    source_path: PathBuf::from(&media_meta.path),
                    source_oid,
                    ext: media_meta.file_type.clone(),
                    dimensions: media_meta.dimensions,
                    skip: Some(reason),
                });
                continue;
            }
            if reason == SkipReason::NotAnImage {
                log::warn!(
                    "Skipping {}: file has an image extension but is not a recognised image format (magic bytes mismatch)",
                    media_meta.path
                );
            } else {
                log::trace!(
                    "Skipping image {}: {:?}",
                    media_meta.path, reason
                );
            }
            continue;
        }

        items.push(ImageConversionItem {
            source_path: PathBuf::from(&media_meta.path),
            source_oid,
            ext: media_meta.file_type.clone(),
            dimensions: media_meta.dimensions,
            skip: None,
        });
    }

    items
}

// ---------------------------------------------------------------------------
// EXIF orientation handling
// ---------------------------------------------------------------------------

/// Apply one of the 8 EXIF orientation transforms to a decoded image.
///
/// Reference: https://www.exif.org/Exif2-2.PDF §4.6.4 tag 0x0112.
///
/// - 1 = identity
/// - 2 = flip horizontal
/// - 3 = rotate 180°
/// - 4 = flip vertical
/// - 5 = transpose (flip-h then rotate 270° cw, i.e. rotate 90° ccw)
/// - 6 = rotate 90° cw
/// - 7 = transverse (flip-h then rotate 90° cw)
/// - 8 = rotate 90° ccw (= rotate 270° cw)
pub(crate) fn apply_exif_orientation(img: image::DynamicImage, orientation: u32) -> image::DynamicImage {
    match orientation {
        1 => img,
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.fliph().rotate90(),
        6 => img.rotate90(),
        7 => img.fliph().rotate270(),
        8 => img.rotate270(),
        _ => img, // Unknown orientation — leave untouched.
    }
}

/// Read the EXIF Orientation tag (0x0112) from the PRIMARY IFD.
///
/// Returns 1 (identity) if absent, unreadable, or not applicable.
pub(crate) fn read_exif_orientation(path: &Path) -> u32 {
    let Ok(file) = fs::File::open(path) else {
        return 1;
    };
    let mut bufreader = std::io::BufReader::new(&file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut bufreader) else {
        return 1;
    };
    if let Some(field) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
        if let exif::Value::Short(ref values) = field.value {
            if let Some(&v) = values.first() {
                return v as u32;
            }
        }
    }
    1
}

// ---------------------------------------------------------------------------
// Alpha flattening
// ---------------------------------------------------------------------------

/// Composite an image with alpha against a white background.
///
/// PNG files with transparency are served on the web without a guaranteed
/// background color. Flattening to white before WebP encode ensures a
/// consistent, opaque result regardless of the viewer's background.
///
/// Non-alpha images are returned unchanged. The returned image always has
/// color type `Rgb8` when the input had alpha, or the original type otherwise.
pub(crate) fn flatten_alpha_to_white(img: image::DynamicImage) -> image::DynamicImage {
    if !img.color().has_alpha() {
        return img;
    }
    let (w, h) = (img.width(), img.height());
    let rgba = img.to_rgba8();
    let mut out = image::RgbImage::new(w, h);
    for (x, y, px) in rgba.enumerate_pixels() {
        let a = px[3] as f32 / 255.0;
        out.put_pixel(x, y, image::Rgb([
            (px[0] as f32 * a + 255.0 * (1.0 - a)) as u8,
            (px[1] as f32 * a + 255.0 * (1.0 - a)) as u8,
            (px[2] as f32 * a + 255.0 * (1.0 - a)) as u8,
        ]));
    }
    image::DynamicImage::ImageRgb8(out)
}

// ---------------------------------------------------------------------------
// WebP encoding
// ---------------------------------------------------------------------------

/// Encode a `DynamicImage` as a WebP byte vector.
///
/// We explicitly feed raw pixel buffers to `webp::Encoder::from_rgb` /
/// `from_rgba` rather than using `from_image` — see the module-level doc
/// comment for the cross-crate version mismatch rationale.
///
/// `icc_profile` is ignored in v1 (documented TODO). When non-None, a
/// warning is logged so the caller knows color was preserved only
/// approximately.
pub(crate) fn encode_webp(
    img: &image::DynamicImage,
    quality: u8,
    icc_profile: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    if icc_profile.is_some() {
        log::debug!("[image] ICC profile present but not preserved (v1 TODO)");
    }

    // Prefer RGBA if the image has alpha; otherwise RGB. Flatten to u8 buffers.
    let has_alpha = img.color().has_alpha();
    let (width, height) = (img.width(), img.height());

    let encoded = if has_alpha {
        let rgba = img.to_rgba8();
        let encoder = webp::Encoder::from_rgba(rgba.as_raw(), width, height);
        encoder.encode(quality as f32).to_vec()
    } else {
        let rgb = img.to_rgb8();
        let encoder = webp::Encoder::from_rgb(rgb.as_raw(), width, height);
        encoder.encode(quality as f32).to_vec()
    };

    if encoded.is_empty() {
        return Err("WebP encoder returned 0 bytes".to_string());
    }
    Ok(encoded)
}

/// Parse and dimension-check a WebP byte buffer.
///
/// Catches silent encoder failures that return the wrong number of pixels
/// (e.g., truncated input, or an oversize that libwebp clipped to its
/// maximum of 16,384).
pub(crate) fn validate_webp_output(bytes: &[u8], expected_dims: (u32, u32)) -> Result<(), String> {
    if bytes.len() < 16 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(format!(
            "WebP magic missing or file too short ({} bytes)",
            bytes.len()
        ));
    }
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::WebP)
        .map_err(|e| format!("WebP decode failed: {}", e))?;
    let got = (img.width(), img.height());
    if got != expected_dims {
        return Err(format!(
            "WebP dimensions mismatch: expected {}x{}, got {}x{}",
            expected_dims.0, expected_dims.1, got.0, got.1
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Sized-raster encoding (deployed raster originals — plan 2026-07-06)
// ---------------------------------------------------------------------------

// `SIZED_JPEG_QUALITY`, `FALLBACK_MAX_EDGE`, `encode_sized_raster` and
// `sized_raster_oid_for_original` were extracted to the sibling
// `fallback_raster` module (MIGRATION-STATE image.rs debt row, same move as
// `rungs`). This file owns the WebP variant pass; that one owns the
// `<picture>` raster FALLBACK.

// ---------------------------------------------------------------------------
// Ladder rung encodes (Task 5)
// ---------------------------------------------------------------------------

// `decode_oriented`, `encode_rungs`, and `RungOutcome` were extracted to the
// sibling `rungs` module (MIGRATION-STATE image.rs debt row, Task 10.5);
// imported at the top of this file.

// ---------------------------------------------------------------------------
// Single-image conversion
// ---------------------------------------------------------------------------

/// Convert one image: cache lookup → decode → orient → resize → encode → store.
///
/// Mirrors `build::video::convert_single_video`:
///
/// 1. Cache hit with a non-empty OID matching current params → validate blob
///    size, link, return.
/// 2. Cache miss → decode, apply EXIF orientation, resize, encode, store in
///    CAS, link to staging + canonical, write record.
///
/// WebP is always written, regardless of whether the encoded output is larger
/// than the source. See the module-level doc for the picture-source 404
/// invariant that drove this choice.
///
/// The function is synchronous; the dispatch layer handles spawning.
///
/// Returns `ImageConversionOutcome` directly (never `Err`). Failures are
/// expressed as `outcome.error = Some(message)` so the outcome is
/// self-contained and singleflight waiters receive the full error.
#[allow(clippy::too_many_arguments)]
pub(crate) fn convert_single_image(
    source_file: &Path,
    source_oid: &str,
    relative_webp: &str,
    temp_dir: &Path,
    staging_dir: &Path,
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    config: &ImageCompressionConfig,
    cancel_flag: Option<&AtomicBool>,
    // When `Some`, cache the placeholder metadata (dimensions + dominant color +
    // LQIP) under this stat key after decoding. The blocking scan skips the full
    // decode and reads dimensions only; we compute color/LQIP here for free from
    // the pixels we already decode for the WebP encode. See
    // `scan::image_meta_stat_key` / `scan::extract_media_metadata_cached`.
    meta_stat_key: Option<&str>,
    // Mapped path of every vault image file → its absolute vault path,
    // threaded from registration (blocking.rs) via `ImageRunContext`. A rung
    // URL appearing as a key is a collision: `encode_rungs` never writes to
    // that path — the user's file wins there. See `rung_collision_map`.
    rung_collisions: &HashMap<String, PathBuf>,
) -> ImageConversionOutcome {
    use crate::build::cache::{TransformEntry, TransformRecord};

    let filename = Path::new(relative_webp)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| relative_webp.to_string());

    let original_size = fs::metadata(source_file).map(|m| m.len()).unwrap_or(0);
    let params = config.to_params();

    // Rung gate: png/jpg/jpeg AND webp (Phase B, Task 12) — the SAME extension
    // set the registration loop (blocking.rs) and the synthesizer gate on, so a
    // rung is encoded iff it was registered iff it was emitted (ADR-013).
    // Derived from the source extension, which is what `item.ext` carries too.
    // For a webp source the base pass below re-encodes webp→webp at the SAME
    // relative path (to_webp(webp)==webp), and the rung encodes reuse the same
    // decoded image — no special-casing needed.
    let rungs_gated = moss_core::asset_paths::is_ladder_source_ext(
        source_file.extension().and_then(|e| e.to_str()).unwrap_or(""),
    );

    // ---- Step 1: cache lookup ----
    // `find_cached_output` matches on params, so old sentinel entries (oid="",
    // params=Null) naturally miss against current params and fall through to
    // re-encode. The new record overwrites the sentinel on `transforms.put`.
    let output_webp = staging_dir.join(relative_webp);
    if let Some(cached_oid) = transforms.find_cached_output(source_oid, "image/webp", &params) {
        // Self-healing size guard (mirrors video.rs): compare cache record's
        // recorded size against actual blob size to catch iCloud-truncation
        // and similar short-write races.
        let recorded_size = transforms
            .get(source_oid)
            .and_then(|r| r.transforms.get("image/webp").map(|e| e.size))
            .unwrap_or(0);
        let blob_path = objects.get_path(&cached_oid);
        let actual_size = blob_path
            .as_ref()
            .and_then(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);
        if blob_path.is_some() && actual_size == recorded_size && recorded_size > 0 {
            // debug!, not info!: this fires once per cached image and is pure
            // per-file confirmation noise. One real "Send logs" bundle had 7,881
            // of these lines (see docs/archive/2026-06-03-send-logs-redesign.md).
            log::trace!("Image cache hit for {} ({})", filename, cached_oid);

            // Track per-link success. The previous "warn-and-return-Ok"
            // pattern silently swallowed link failures — the outer loop
            // saw outcome.webp_oid.is_some() and emitted AssetReady, but
            // the file was never actually at the served path. This caused
            // the catastrophic <picture> 404 verified at user log
            // L2006/L2446/L2450 on 2026-05-19.
            //
            // Now: if the canonical link fails, propagate via outcome.error
            // so the outer loop transitions the registry to Failed (via
            // set_failed at image.rs:1195-1201 error arm) instead of
            // claiming Ready. Staging link failure is non-fatal here —
            // staging is the intermediate location; canonical is the
            // served path that the preview server reads from.
            //
            // Pattern: Bazel's FindMissingBlobs invariant
            // (bazel.build/remote/caching). See
            // docs/archive/2026-05-20-image-variant-honest-mirror.md (Layer 5B).
            if let Err(e) = objects.link_to(&cached_oid, &output_webp) {
                log::warn!("Failed to link cached WebP to staging: {}", e);
            }
            // Rungs ride the warm path too: registered rungs must land in
            // THIS build's staging even when the base is a cache hit — on
            // the first build after upgrading to a rung-aware moss, the
            // base cache is warm while every rung is cold, and skipping
            // here would seal a deploy with 404ing srcset candidates
            // (ADR-013). `encode_rungs` decodes lazily only on a rung
            // cache miss, so the all-warm rebuild stays decode-free.
            let rungs = if rungs_gated {
                encode_rungs(
                    source_file, source_oid, relative_webp, staging_dir,
                    objects, transforms, config, rung_collisions, None,
                    actual_size,
                )
            } else {
                Vec::new()
            };
            return ImageConversionOutcome {
                webp_oid: Some(cached_oid),
                webp_size: actual_size,
                original_size,
                error: None,
                rungs,
            };
        } else {
            log::warn!(
                "Cached WebP for {} failed size check (recorded={}, actual={}) — evicting",
                filename, recorded_size, actual_size
            );
            transforms.remove(source_oid).ok();
            // Fall through to re-encode.
        }
    }

    // ---- Step 2: cancellation check ----
    if let Some(flag) = cancel_flag {
        if flag.load(Ordering::SeqCst) {
            return ImageConversionOutcome {
                webp_oid: None,
                webp_size: 0,
                original_size: 0,
                error: Some("Cancelled".to_string()),
                rungs: Vec::new(),
            };
        }
    }

    // ---- Step 3: decode ----
    // Sniff format by magic bytes (with_guessed_format) instead of image::open,
    // which picks the decoder by extension alone. Mislabeled files (e.g. an
    // Obsidian-pasted PNG saved with a `.gif` extension) decode fine but the
    // extension-based path rejects them with a "malformed Gif header" toast.
    // Decode + allocation cap + EXIF orientation live in `decode_oriented`
    // (shared with the rung encodes).
    let img = match decode_oriented(source_file) {
        Ok(i) => i,
        Err(e) => {
            return ImageConversionOutcome {
                webp_oid: None,
                webp_size: 0,
                original_size: 0,
                error: Some(e),
                rungs: Vec::new(),
            };
        }
    };

    // ---- Step 3b: placeholder metadata (deferred from the blocking scan) ----
    // Compute the dominant color + LQIP ONCE from the pixels we just decoded.
    // The blocking scan reads only the dimensions (header) and leaves color/LQIP
    // `None`; here — for free, since we already decoded for the encode — we cache
    // them under the scan's stat key so a later (warm) scan returns the real
    // values. See scan::extract_media_metadata_cached.
    //
    // The LQIP is for the PUBLISHED page: it reaches `AssetSnapshot::lqip` and
    // `moss_core::render::image` bakes it into the emitted HTML as a blur-up, so
    // a visitor waiting on the network sees something immediately. It is
    // deliberately NOT pushed into the preview's AssetRegistry. Until 2026-08-21
    // it was, to morph the preview neutral → colored → real; that morph could
    // never happen (the router's source passthrough already outranked it) and,
    // under the rule this audit settled, must not — an author judging a photo
    // cannot tell a blurred stand-in from a badly-encoded result, so the preview
    // shows the real original instead. See
    // docs/archive/2026-08-21-pending-variant-source-passthrough-audit.md.
    if let Some(key) = meta_stat_key {
        // Computed INSIDE the `if let`: the cache entry is the only consumer
        // now that `on_decoded` is gone, so a caller that passes no stat key
        // (every non-scan path) would otherwise pay for a 100x100 thumbnail
        // average plus a JPEG encode and drop both results.
        let (dominant_color, lqip_data_uri) =
            crate::build::scan::scan::compute_color_and_lqip_from_image(&img);
        let dims = (img.width(), img.height());
        {
            // The blocking scan already wrote this stat key with `is_animated`
            // sniffed (see scan::extract_media_metadata_cached, Step 2c). This
            // write REPLACES that entry's blob wholesale, so carry the prior
            // value forward instead of silently resetting it to `false`.
            let is_animated = crate::build::scan::scan::read_cached_meta(transforms, objects, key)
                .map(|prev| prev.is_animated)
                .unwrap_or(false);
            crate::build::scan::scan::write_cached_meta(
                objects,
                transforms,
                key,
                original_size,
                &crate::build::cache::CachedMediaMeta {
                    dimensions: Some(dims),
                    dominant_color: dominant_color.clone(),
                    lqip_data_uri: lqip_data_uri.clone(),
                    is_animated,
                },
            );
        }
    }

    // ---- Step 4: resize ----
    // The decoded ORIENTED original outlives the base resize when a ladder
    // exists: rung encodes (Step 10) `resize_exact` from IT, not from the
    // max_edge-bounded base (no double resampling). `Option` lets the
    // no-ladder path move `img` into `resized` exactly as before — no clone.
    let (w, h) = (img.width(), img.height());
    let ladder_len = if rungs_gated {
        moss_core::asset_paths::ladder_rungs(w, h, false).len()
    } else {
        0
    };
    let mut original: Option<image::DynamicImage> = Some(img);
    let resized = if w.max(h) > config.max_edge {
        original.as_ref().unwrap().resize(
            config.max_edge,
            config.max_edge,
            image::imageops::FilterType::Lanczos3,
        )
    } else if ladder_len > 0 {
        // Small-but-laddered source (e.g. 2000×1200 under the 2400 cap):
        // the base encodes the unresized pixels, and the original must stay
        // alive for the rung resizes. The clone is bounded — this arm's
        // images are ≤ max_edge on the long edge, ≤ ~23 MB decoded RGBA.
        original.as_ref().unwrap().clone()
    } else {
        original.take().unwrap()
    };
    let final_dims = (resized.width(), resized.height());

    // ---- Step 4b: flatten alpha to white ----
    if resized.color().has_alpha() {
        let ext = source_file.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if ext == "png" {
            log::debug!("[image] Flattening alpha channel to white for PNG: {}", filename);
        } else {
            log::warn!(
                "[image] Flattening alpha channel to white for non-PNG source ({}): {}. \
                 Transparency will be lost. Supply an opaque variant if this is unintended.",
                ext, filename
            );
        }
    }
    let resized = flatten_alpha_to_white(resized);

    // ---- Step 5: encode WebP ----
    let webp_bytes = match encode_webp(&resized, config.quality, None) {
        Ok(b) => b,
        Err(e) => {
            return ImageConversionOutcome {
                webp_oid: None,
                webp_size: 0,
                original_size: 0,
                error: Some(e),
                rungs: Vec::new(),
            };
        }
    };
    if let Err(e) = validate_webp_output(&webp_bytes, final_dims) {
        return ImageConversionOutcome {
            webp_oid: None,
            webp_size: 0,
            original_size: 0,
            error: Some(e),
            rungs: Vec::new(),
        };
    }

    let encoded_len = webp_bytes.len() as u64;

    if encoded_len >= original_size {
        log::debug!(
            "WebP ({} bytes) >= original ({} bytes) for {} — writing anyway: \
             <picture> source 404s break the fallback chain per HTML spec.",
            encoded_len, original_size, filename
        );
    }

    // ---- Step 6: store in CAS ----
    let stem = Path::new(&filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let temp_path = temp_dir.join(format!("{}-{}.webp", stem, uuid::Uuid::new_v4()));
    if let Some(parent) = temp_path.parent() {
        let _ = crate::build::io_utils::create_output_dir_all(parent);
    }
    if let Err(e) = fs::write(&temp_path, &webp_bytes) {  // allow:raw_write the temp this fn just minted, before the encode result is placed
        return ImageConversionOutcome {
            webp_oid: None,
            webp_size: 0,
            original_size: 0,
            error: Some(format!("Failed to write temp WebP: {}", e)),
            rungs: Vec::new(),
        };
    }

    let oid = match objects.store_file(&temp_path) {
        Ok(o) => o,
        Err(e) => {
            // allow:unlink the encode temp this call wrote under cache/tmp
            let _ = fs::remove_file(&temp_path);
            return ImageConversionOutcome {
                webp_oid: None,
                webp_size: 0,
                original_size: 0,
                error: Some(format!("CAS store failed: {}", e)),
                rungs: Vec::new(),
            };
        }
    };
    // allow:unlink the encode temp this call wrote under cache/tmp
    let _ = fs::remove_file(&temp_path);

    // ---- Step 8: link to staging + canonical ----
    //
    // Both link failures are fatal: if either landing site does not receive
    // the file, downstream code (manifest registration, deploy validation)
    // observes register-without-write and the deploy aborts with `"Manifest
    // claims '<path>' exists but it's missing on disk"`.
    //
    // Why fatal for BOTH legs:
    //
    // * Staging is consulted by `emit_image_outputs_via_channel` to compute
    //   the manifest hash. If staging is missing the path it never reaches
    //   the manifest in the first place (good — pre-flight existence check
    //   skips emit), but the variant is also unavailable for any other
    //   consumer (preview HTTP server, ship phase).
    //
    // * The sealed generation (the eventual `.moss/build/current` symlink) is what the deploy
    //   reads via `canonicalize(site_dir / key)`. The blocking-phase
    //   `ship_phase` runs at `build/pipeline.rs:1020`, BEFORE the deferred
    //   image worker runs, so it cannot recover a missed canonical link.
    //   `copy_deferred_assets` does not re-mirror generated variants either.
    //   If the canonical link fails after the staging link succeeded, the
    //   variant lives in staging only, the manifest registers it (because
    //   staging exists), and deploy ENOENTs. Treating canonical-link failure
    //   as success would re-introduce the exact register-without-write class
    //   of bugs the staging-link guard closes.
    //
    // CAS is the source of bytes for both links, so a successful staging
    // link almost always implies a successful canonical link (same blob,
    // same disk). A canonical-link failure here is almost always a real
    // I/O error worth surfacing.
    if let Err(e) = objects.link_to(&oid, &output_webp) {
        log::error!(
            "Failed to link WebP to staging ({}): {}",
            output_webp.display(),
            e
        );
        return ImageConversionOutcome {
            webp_oid: None,
            webp_size: 0,
            original_size,
            error: Some(format!("link_to staging failed: {}", e)),
            rungs: Vec::new(),
        };
    }
    // ---- Step 9: merge-preserve transform record ----
    let mut record = transforms.get(source_oid).unwrap_or(TransformRecord {
        source_oid: source_oid.to_string(),
        source_size: original_size,
        transforms: std::collections::HashMap::new(),
    });
    record.transforms.insert(
        "image/webp".to_string(),
        TransformEntry {
            oid: oid.clone(),
            size: encoded_len,
            params: params.clone(),
        },
    );
    if let Err(e) = transforms.put(&record) {
        log::warn!("Failed to write transform record: {}", e);
    }

    // ---- Step 10: ladder rung encodes (after base success only) ----
    // Reuses the decoded ORIENTED original held in `original` (Some whenever
    // `ladder_len > 0`). Per-rung failures are carried in each RungOutcome —
    // they must NOT roll back the base success above.
    let rungs = if ladder_len > 0 {
        encode_rungs(
            source_file,
            source_oid,
            relative_webp,
            staging_dir,
            objects,
            transforms,
            config,
            rung_collisions,
            original.as_ref(),
            encoded_len,
        )
    } else {
        Vec::new()
    };

    ImageConversionOutcome {
        webp_oid: Some(oid),
        webp_size: encoded_len,
        original_size,
        error: None,
        rungs,
    }
}

// ---------------------------------------------------------------------------
// Fingerprint (for skip-on-no-change)
// ---------------------------------------------------------------------------

/// SHA-256 fingerprint of one image's `(path, size, mtime)` plus the
/// compression config. Matching the previous dispatch's fingerprint for THIS
/// path means this one image is unchanged, and — combined with its output
/// still being present on disk, checked separately in
/// `dispatch_image_conversions` — there is no reason to re-encode it. The
/// compression params are folded in so a config change re-dispatches every
/// image's fingerprint even when no file moved.
///
/// Per item, not per set: mirrors `compute_video_item_fingerprint`
/// (build/media/video.rs, commit 5323496908) — the old scheme hashed the
/// whole sorted image list into one fingerprint, so any single added,
/// changed or removed image invalidated it and forced a full re-dispatch,
/// which drops every OTHER, untouched image out of that round's manifest if
/// the async batch doesn't finish registering them before the build seals.
///
/// Returns `None` when the source can't be stat'd (missing / unreadable) —
/// the caller must treat that as "cannot prove unchanged" and dispatch it.
pub(crate) fn compute_image_item_fingerprint(
    source_path: &str,
    item: &Path,
    config: &ImageCompressionConfig,
) -> Option<String> {
    let source = Path::new(source_path).join(item);
    image_item_fingerprint(item, &crate::build::cache::FileStat::of(&fs::metadata(&source).ok()?), config)
}

/// [`compute_image_item_fingerprint`] for a stat record already in hand.
pub(crate) fn image_item_fingerprint(
    item: &Path,
    stat: &crate::build::cache::FileStat,
    config: &ImageCompressionConfig,
) -> Option<String> {
    use sha2::{Digest, Sha256};

    // No mtime at all: nothing to tell one write from the next.
    stat.mtime_nanos?;

    let mut hasher = Sha256::new();
    hasher.update(item.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    // The whole stat record, not size + whole-second mtime: a same-size rewrite in
    // the same second must not fingerprint like the file it replaced. In-process
    // only, so the derived Debug form is a stable enough encoding — and a field
    // added to `FileStat` joins the fingerprint without anyone remembering to.
    hasher.update(format!("{stat:?}").as_bytes());
    hasher.update(b"\0");
    hasher.update(config.to_params().to_string().as_bytes());
    Some(format!("{:x}", hasher.finalize()))
}

/// Module-local fingerprint cache for image conversion (independent of
/// `VideoConversionState`, which owns the video fingerprint), keyed by each
/// image's relative source path. Per-item, not per-set — mirrors
/// `VideoConversionState::last_video_fingerprints`.
fn image_fingerprint_cell() -> &'static Mutex<HashMap<String, String>> {
    static IMAGE_FINGERPRINTS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    IMAGE_FINGERPRINTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Whether `path`'s fingerprint is the one recorded when a worker last delivered its
/// variant (`record_image_item_fingerprint`): this one image is unchanged since. Read
/// only — a dispatch that finds a change must not vouch for the change, because the
/// worker it spawns may leave (the user cancelled, the source went back to the cloud,
/// the hash failed) without encoding it, and the old variant it leaves in staging is
/// exactly what the next dispatch's skip check finds present. Independent per path.
pub(crate) fn image_item_fingerprint_matches(path: &str, fingerprint: &str) -> bool {
    image_fingerprint_cell().lock().expect("image fp mutex").get(path).map(String::as_str) == Some(fingerprint)
}

/// Record that `path`'s variant was delivered for the source this fingerprint
/// describes.
pub(crate) fn record_image_item_fingerprint(path: &str, fingerprint: String) {
    image_fingerprint_cell().lock().expect("image fp mutex").insert(path.to_string(), fingerprint);
}

/// Drop stored fingerprints for paths not in `keep` — called once per
/// dispatch with the current image set, so a removed image's entry doesn't
/// linger forever, and a removed-then-re-added image is treated as new
/// rather than replaying a stale match against bytes that may since have
/// changed. Mirrors `VideoConversionState::retain_item_fingerprints`.
pub(crate) fn retain_image_item_fingerprints(keep: &std::collections::HashSet<String>) {
    let mut map = image_fingerprint_cell().lock().expect("image fp mutex");
    map.retain(|k, _| keep.contains(k));
}

// ---------------------------------------------------------------------------
// Batch runner
// ---------------------------------------------------------------------------

/// Lightweight context passed into the image conversion runner.
///
/// `BackgroundContext` is re-used for both video and image dispatch, so we
/// can't consume it. Instead we clone the few fields we actually need into
/// this small owned struct that is `Send + 'static`.
#[derive(Debug, Clone)]
pub(crate) struct ImageRunContext {
    pub items: Vec<ImageConversionItem>,
    pub source_path: String,
    pub staging_dir: PathBuf,
    pub moss_dir: PathBuf,
    pub config: ImageCompressionConfig,
    /// File-tree → page-tree path mappings. Cloned from `ctx.dir_overrides`
    /// so the runner remaps `item.source_path` (file-tree) onto page-tree paths
    /// before computing `.webp` output paths. Mirrors `video.rs:374`.
    pub dir_overrides: HashMap<String, String>,
    /// Coordinator channel for manifest registration. When `Some`, each produced
    /// `.webp` path is sent via `EmitMessage` with `HashBucket::ImageVariants`
    /// instead of writing to `hashes.json` via `update_image_hashes`.
    /// `None` in legacy / test paths that haven't been wired to `BackgroundHandle`.
    pub tx: Option<mpsc::Sender<EmitMessage>>,
    /// Mapped path of EVERY vault image file → its absolute vault path,
    /// cloned from `BackgroundContext::rung_collisions` (see the field doc
    /// there — keys are not only rung-shaped names). The worker tests
    /// candidate rung URLs for KEY membership before writing — the same test
    /// registration ran — so a user's file named like a rung is never
    /// clobbered. Never recomputed here (ADR-013 agreement).
    pub rung_collisions: HashMap<String, PathBuf>,
}

impl ImageRunContext {
    fn from_background(ctx: &BackgroundContext, config: ImageCompressionConfig) -> Self {
        Self {
            items: ctx.image_items.clone(),
            source_path: ctx.source_path.clone(),
            staging_dir: ctx.staging_dir.clone(),
            moss_dir: ctx.moss_dir.clone(),
            config,
            dir_overrides: ctx.dir_overrides.clone(),
            tx: None,
            rung_collisions: ctx.rung_collisions.clone(),
        }
    }

    /// Attach a coordinator channel sender for manifest registration.
    ///
    /// When set, `run_image_conversion` sends `EmitMessage`s via this channel
    /// instead of calling `update_image_hashes` (which writes to disk). The
    /// coordinator drains the channel and seals the manifest after all workers
    /// complete.
    pub(crate) fn with_tx(mut self, tx: mpsc::Sender<EmitMessage>) -> Self {
        self.tx = Some(tx);
        self
    }
}

/// A megapixels-in-flight budget: a counting semaphore whose unit is source
/// megapixels, used to bound the peak memory of the parallel encode.
///
/// Each image is fully decoded to RGBA (`w*h*4` bytes) plus resize + WebP-encode
/// scratch — and, since Task 5, the ladder rung passes: a laddered mid-size
/// item transiently holds the decoded original + the base copy/resize + one
/// rung resize buffer, so the source-megapixel reservation is a ~2-3×
/// under-proxy for such items. That head-room is what the scratch multiplier
/// already absorbs (rung buffers are ≤ 1600px wide, small next to their
/// sources); no accounting change. Bounding by *worker count* is wrong — a
/// handful of huge images
/// (William Blake: max 34 MP ≈ 135 MB decoded, several × that with scratch)
/// decoding at once OOM-SIGKILLs the process. Bounding by *megapixels in flight*
/// instead means many small images pack in (full parallelism) while huge images
/// self-throttle (a 34 MP image against a 64 MP budget runs ~alone). A permit is
/// released on drop, so it's returned on a normal return or an early bail. (A
/// *panic* mid-encode aborts the whole process under the release profile's
/// `panic = "abort"` — there is no surviving budget to leak; the RAII drop only
/// matters as an unwind safety net in dev/test builds.)
struct MegapixelBudget {
    available: std::sync::Mutex<u64>,
    cv: std::sync::Condvar,
    total: u64,
}

struct MpPermit<'a> {
    budget: &'a MegapixelBudget,
    held: u64,
}

impl MegapixelBudget {
    fn new(total_mp: u64) -> Self {
        Self {
            available: std::sync::Mutex::new(total_mp),
            cv: std::sync::Condvar::new(),
            total: total_mp.max(1),
        }
    }

    /// Block until `want` megapixels are free (clamped to `[1, total]` so an
    /// image larger than the whole budget still runs — alone), then reserve them.
    fn acquire(&self, want: u64) -> MpPermit<'_> {
        let want = want.clamp(1, self.total);
        let mut available = self.available.lock().unwrap();
        while *available < want {
            available = self.cv.wait(available).unwrap();
        }
        *available -= want;
        MpPermit { budget: self, held: want }
    }
}

impl Drop for MpPermit<'_> {
    fn drop(&mut self) {
        // Best-effort: if the lock is poisoned we can't reclaim, but the encode
        // is already failing — don't compound it by panicking in a destructor.
        if let Ok(mut available) = self.budget.available.lock() {
            *available += self.held;
            self.budget.cv.notify_all();
        }
    }
}

/// The `.webp` keys this build must not put into staging, read from the last
/// build's ship-time prune rather than derived here.
///
/// Both image producers call this — the encoder below and the fingerprint-skip
/// self-heal in `dispatch_image_conversions` — which is the point: one
/// authority, neither re-deriving it from the disk scan. Rationale:
/// `orphan_prune::suppressed_variants`.
fn suppressed_variants_for(
    paths: &MossPaths,
    staging_dir: &Path,
) -> std::collections::HashSet<String> {
    let previous: crate::types::content::SiteHashes = std::fs::read_to_string(paths.hashes())
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or_default();
    crate::build::media::orphan_prune::suppressed_variants(
        staging_dir,
        &previous.pruned_image_outputs,
    )
}

// ---------------------------------------------------------------------------
// What happened to one image
// ---------------------------------------------------------------------------

/// What the conversion worker decided about one image.
///
/// Every exit of the per-item body returns one of these instead of running its
/// own teardown, so the census push, the `AssetReady` emit, the registry
/// transition, the converted-count bump, the UiBound release and the progress
/// tick happen in ONE place and a new exit cannot skip a step. The seven
/// `end_ui_bound()` sites this replaces had drifted: the hash-failure arm was
/// the only failure here that told nobody — no advisory and no `set_failed`,
/// so the base `.webp` and every rung blocking.rs had promised sat Pending
/// forever behind a placeholder that never resolved.
///
/// The distinction the type exists to keep is Failed vs Pending. A source the
/// provider has not finished downloading is *deferred* and stays Pending (the
/// LQIP placeholder); a source that cannot be read or decoded is *failed* and
/// paints a warning. Collapsing them either hides a broken image or warns over
/// bytes that are still on their way.
enum ItemStep {
    /// `delivered` names every `.webp` URL now in staging paired with the CAS
    /// oid backing those exact bytes (`ensure_staged`'s own argument, so it is
    /// always `Some` on this path — ship-by-OID's fingerprint fallback covers
    /// every OTHER registration route, e.g. the carry-forward skip path in
    /// `dispatch_image_conversions`, which never populates `delivered` at
    /// all), `failed` every promised URL that will never be produced,
    /// `advisories` what to tell the user. `encoded` gates the media child Job
    /// on real work.
    Handled {
        delivered: Vec<(String, Option<String>)>,
        failed: Vec<(String, String)>,
        advisories: Vec<Advisory>,
        encoded: bool,
    },
    /// The user cancelled. Alone among the arms it emits no progress tick: the
    /// post-loop reduce bails before the completion event, so a tick here would
    /// advance a bar that never closes.
    Cancelled,
}

impl ItemStep {
    /// Nothing shipped and nothing is coming: retract the base promise and every
    /// rung promise registration made before the worker ran, and say why. The one
    /// owner of a policy the decode-failure arm spelled out in full and the
    /// hash-failure arm not at all.
    fn base_failed(source_path: &str, webp: &str, rungs: Vec<(u32, String)>, err: String) -> Self {
        let mut failed = vec![(webp.to_string(), err.clone())];
        failed.extend(rungs.into_iter().map(|(_, rung)| (rung, err.clone())));
        Self::Handled {
            delivered: Vec::new(),
            failed,
            advisories: vec![Advisory::for_source(
                Scope::File,
                Severity::ShippedDegraded,
                source_path,
                crate::infra::app_advisory::fmt("shipped_without_optimizing", &[("err", &err)]),
                Action::None,
            )],
            encoded: false,
        }
    }

    /// Still in the cloud: ask for the bytes and leave every promise Pending,
    /// because the build their arrival triggers runs this item for real. One
    /// owner for the gate before the encode and the race back mid-encode.
    fn deferred_to_cloud(source_file: &Path, rel_source_str: &str) -> Self {
        crate::build::cloud_readiness::request_download(source_file);
        log::debug!("[image] {} is still in the cloud — deferring", rel_source_str);
        Self::nothing_shipped(vec![Advisory::for_source(
            Scope::File,
            // Transient and self-resolving: the tier the video gate's timeout
            // uses, not NeedsAction.
            Severity::ShippedDegraded,
            rel_source_str,
            crate::infra::app_advisory::t("still_downloading_cloud"),
            Action::None,
        )])
    }

    /// Handled without producing bytes and without retracting a promise — an
    /// empty `advisories` means there is nothing for the user to hear either.
    fn nothing_shipped(advisories: Vec<Advisory>) -> Self {
        Self::Handled { delivered: Vec::new(), failed: Vec::new(), advisories, encoded: false }
    }
}

use crate::build::cache;

/// Put a CAS blob at this item's staging path and say whether the bytes are
/// there. Idempotent: a singleflight-shared outcome linked only the FIRST
/// caller's paths, so a duplicate-content image at a different path has to link
/// its own or its `<source srcset>` 404s. Returning the verdict stops a URL from
/// being announced Ready for bytes that never landed — the base variant used to
/// warn and deliver anyway.
fn ensure_staged(objects: &cache::ObjectStore, oid: &str, out: &Path, url: &str) -> bool {
    if out.exists() {
        return true;
    }
    match objects.link_to(oid, out) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("Failed to link webp to staging {}: {}", url, e);
            false
        }
    }
}

/// Record one item's deliveries: the census the manifest reads, the live
/// `AssetReady` the preview swaps on, and the registry promise the URL was made
/// under. One function because the census is load-bearing — bytes staged
/// without being pushed here fall outside `image_outputs`, and the next build's
/// `remove_stale_files` deletes what no bucket claims. Base and rung used to
/// spell the triplet out separately.
fn record_deliveries(
    services: &BuildServices,
    produced: &std::sync::Mutex<Vec<(String, Option<String>)>>,
    delivered: Vec<(String, Option<String>)>,
) {
    if delivered.is_empty() {
        return;
    }
    produced.lock().unwrap().extend(delivered.iter().cloned());
    for (url, _oid) in delivered {
        services.reporter.report(&PipelineEvent::AssetReady {
            path: url.clone(),
            asset_type: "image".to_string(),
        });
        if let Some(ref registry) = services.assets {
            registry.set_ready(url);
        }
    }
}

/// Run image conversion over all items IN PARALLEL, memory-bounded.
///
/// ## Concurrency choice
///
/// Each image is independent (decode + resize + WebP encode). The fan-out uses
/// RAW OS THREADS, not rayon: `image::open` decodes JPEGs via jpeg-decoder,
/// which parallelizes with rayon internally, and nesting rayon-in-rayon
/// corrupts/deadlocks it. Concurrency is capped not by worker count but by a
/// [`MegapixelBudget`] — the memory driver is decoded pixels in flight, so N
/// huge images decoding at once (not N threads) is what OOMs. Each worker
/// decides an [`ItemStep`] and applies it as it finishes, so images stream into
/// the preview as they land; the accumulators are `Mutex`/atomic and reduced
/// after the scope, and the shared `AssetRegistry`, coordinator `Sender`,
/// singleflight and `end_ui_bound` counter are internally synchronized.
pub(crate) fn run_image_conversion(services: &BuildServices, ctx: &ImageRunContext) {
    use crate::build::cache::{ObjectStore, TransformCache};
    use moss_core::asset_paths;
    use crate::build::render::resolve_path_with_overrides;

    let is_headless = services.reporter.is_terminal();

    let total = ctx.items.len() as u32;
    if total == 0 {
        return;
    }

    // The per-image work below runs in PARALLEL on raw OS threads (see the
    // std::thread::scope fan-out), so these accumulators are thread-safe. Each
    // worker applies its own `ItemStep` as it finishes, so images stream into
    // the preview as they land. A Mutex is fine: the push is trivial next to the
    // encode it guards. NOT rayon — jpeg-decoder nests rayon and deadlocks.
    use std::sync::atomic::AtomicU32;
    use std::sync::Mutex;
    let advisories: Mutex<Vec<Advisory>> = Mutex::new(Vec::new());
    // Count of images this run ACTUALLY converted (produced a webp at the served
    // path). `collect_images_for_conversion` already change-filters via `should_skip`,
    // so `ctx.items` only holds images that need conversion; this counts the ones
    // that landed. With zero advisories, a zero count gates out the media Jobs
    // (FIX 1b, invariant #6) — though in practice an images-only build that reaches
    // here had a non-empty item set, so it normally does real work.
    let converted_count = AtomicU32::new(0);
    // Track .webp paths actually produced during this run (for coordinator
    // registration), paired with the CAS oid backing each one's exact bytes.
    // Only paths that were successfully produced are sent; failed items are omitted.
    let produced_webp_paths: Mutex<Vec<(String, Option<String>)>> = Mutex::new(Vec::new());
    // Monotonic count of finished items (they complete out of order in parallel)
    // for the progress bar, and a flag observed if any item hits cancellation.
    let completed = AtomicU32::new(0);
    let any_cancelled = std::sync::atomic::AtomicBool::new(false);
    // Cancellation flag: bridged from `FolderSession::cancel` to a local
    // AtomicBool so the existing per-image cancel-check (`AtomicBool::load`)
    // is preserved. In headless mode (no session) the flag stays false.
    //
    // `bridge_to_atomic` seeds the AtomicBool synchronously from
    // `token.is_cancelled()` before spawning the async bridge task, so a
    // session that's already cancelled at dispatch time is observed by
    // the synchronous runner on its first cancel check (no need to wait
    // for the bridge task to be polled — historically the source of a
    // batched-test flake).
    let local_cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(s) = services.session.as_ref() {
        s.bridge_to_atomic(local_cancel.clone());
    }
    let cancel_flag: Option<&std::sync::atomic::AtomicBool> = Some(&*local_cancel);

    if is_headless {
        eprintln!("Compressing {} image(s)...", total);
        log::info!("Starting headless image conversion for {} image(s)", total);
    }

    services.reporter.report(&PipelineEvent::BackgroundProgress {
        task: "images".to_string(),
        current: 0,
        total,
        message: format_progress_message("images", 0, total),
        completed: false,
        advisories: vec![],
    });

    let moss_paths = MossPaths::from_moss_dir(ctx.moss_dir.clone());
    let suppressed = suppressed_variants_for(&moss_paths, &ctx.staging_dir);
    let objects = ObjectStore::new(moss_paths.cache_objects());
    let transforms = TransformCache::new(
        moss_paths.cache_transforms(),
        ObjectStore::new(moss_paths.cache_objects()),
    );

    // Deferred hashing (mirrors the video worker): the blocking collect leaves
    // each item's source_oid empty (stat-match only) so first paint isn't gated
    // on hashing. We resolve the hashes here and persist the index below so the
    // next build's collect stat-matches instead of re-hashing every image.
    let hash_index_path = moss_paths.cache_hash_index();
    // Mutex: workers resolve deferred hashes concurrently. Contention is low —
    // most items stat-match (microseconds under lock); only cold misses hash.
    let bg_hash_index = Mutex::new(crate::build::cache::HashIndex::load(&hash_index_path));

    // This batch's own scratch, removed when the batch returns.
    let scratch = crate::build::io_utils::ScratchDir::new(&moss_paths.cache_tmp(), "image");
    let temp_dir = scratch.path().to_path_buf();

    // dir_overrides for page-tree mapping. Mirrors `run_video_conversion`
    // (video.rs:374): scan/render populate `item.source_path` with the
    // file-tree path; the runner remaps onto page-tree paths so the `.webp`
    // lands next to the original image reference in the built site.
    let dir_overrides = &ctx.dir_overrides;
    let project_root = Path::new(&ctx.source_path);

    // Use raw OS THREADS, not rayon. convert_single_image → image::open decodes
    // JPEGs via jpeg-decoder, which parallelizes with rayon INTERNALLY. Driving
    // this fan-out with rayon `par_iter` NESTS rayon on the same pool and
    // DEADLOCKS under load — verified: William Blake's 846 images stalled the
    // background encode at ~220 converted, 0% CPU, every worker parked. Raw OS
    // threads keep jpeg-decoder's rayon at top level. Mirrors the preview scan's
    // image fan-out (build/scan/scan.rs). The Mutex/atomic accumulators are
    // reduced after the scope exactly as before.
    {
        let n_items = ctx.items.len();
        let next = std::sync::atomic::AtomicUsize::new(0);
        // Spawn a worker per core; the MegapixelBudget (not the thread count)
        // is what actually bounds memory, so extra workers just block on the
        // budget when huge images are in flight and pack in when they're small.
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(n_items.max(1));
        // ~64 MP in flight ≈ ~256 MB of decoded RGBA + ~2-3× that in transient
        // resize/encode scratch → comfortably under a GB even with a couple of
        // huge images, and safe running alongside the editor + a browser. This
        // replaced an unbounded per-core encode that OOM-SIGKILLed the process
        // on large-photo vaults (William Blake, max 34 MP). Overridable via
        // MOSS_ENCODE_MP_BUDGET so a future OOM on lower-RAM hardware (or a
        // throughput bump on a big machine) needs no recompile.
        let budget_mp = std::env::var("MOSS_ENCODE_MP_BUDGET")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(64);
        let mp_budget = MegapixelBudget::new(budget_mp);
        std::thread::scope(|scope| {
            for _ in 0..n_threads {
                scope.spawn(|| loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= n_items {
                        break;
                    }
                    let item = &ctx.items[index];
                    // Before anything reads the source, so a write landing during the
                    // encode leaves a fingerprint the file no longer has. Recorded in
                    // the teardown below, and only for an item that delivered.
                    let fingerprint = compute_image_item_fingerprint(&ctx.source_path, &item.source_path, &ctx.config);
                    // The whole of one image's decision. Every exit is a value,
                    // never a `return` with teardown attached — see `ItemStep`.
                    // Nothing in here releases a UiBound permit, pushes to the
                    // census, ticks progress or touches the registry.
                    let step: ItemStep = (|| {
        // Cancellation: skip remaining work. Every dispatched item still runs
        // exactly one `end_ui_bound()` below (matching the old per-item count);
        // `any_cancelled` drives the partial-output registration + early bail
        // AFTER the loop, replacing the old mid-loop early return.
        if cancel_flag.map_or(false, |f| f.load(Ordering::SeqCst)) {
            return ItemStep::Cancelled;
        }

        let rel_source_str = item.source_path.to_string_lossy().to_string();
        let source_file = project_root.join(&item.source_path);
        let filename = item
            .source_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "image".to_string());

        if !source_file.exists() && !crate::build::icloud::is_still_in_the_cloud(&source_file) {
            log::warn!("Image not found: {}", rel_source_str);
            return ItemStep::nothing_shipped(vec![Advisory::for_source(
                Scope::File,
                Severity::NeedsAction,
                &rel_source_str,
                crate::infra::app_advisory::t("not_found"),
                Action::None,
            )]);
        }

        // Cloud gate. On a 1077-image Google Drive vault ~250 sources were
        // dataless: each was hashed, decoded and re-decoded per rung, failed
        // `EDEADLK` every time (591 events in one session) and ended up Failed,
        // so the work was wasted AND the site shipped full-size originals in
        // place of its `w800`/`w1600` rungs (moss#986).
        //
        // Unlike video this does NOT wait. The video gate spends up to 90s per
        // file because videos are few and a re-encode is expensive to redo; this
        // is a per-core fan-out over the whole vault, where the same deadline
        // would serialize hundreds of files behind downloads. The supervisor is
        // already draining in priority order and an arrival triggers a rebuild.
        //
        // Two lstats per item, so this is also the negative cache: nothing to
        // remember between builds when re-deciding is this cheap.
        if crate::build::icloud::is_still_in_the_cloud(&source_file) {
            return ItemStep::deferred_to_cloud(&source_file, &rel_source_str);
        }

        // Derived before the hash resolution below, not after: a hash failure
        // has to retract the promises made under these URLs.
        let mapped_source = resolve_path_with_overrides(&rel_source_str, dir_overrides);
        let relative_webp = asset_paths::to_webp(&mapped_source);

        // Rung URLs registration promised for THIS item — derived from the SAME
        // inputs blocking.rs used (SCAN dims + the ladder gate, png/jpg/jpeg AND
        // webp per Phase B + collision-map skip). A base failure retracts them;
        // the success arm sweeps any the encode did not resolve, so a registered
        // rung never sits Pending forever behind its LQIP/passthrough
        // placeholder (honest-mirror Layer 5B stuck-Pending class). The `false`
        // literal + dims-agreement rationale live on asset_paths::ladder_rungs.
        let registered_rungs = || -> Vec<(u32, String)> {
            if !moss_core::asset_paths::is_ladder_source_ext(&item.ext) {
                return Vec::new();
            }
            let Some((sw, sh)) = item.dimensions else { return Vec::new() };
            asset_paths::ladder_rungs(sw, sh, false)
                .iter()
                .map(|&rw| (rw, asset_paths::to_webp_rung(&relative_webp, rw)))
                .filter(|(_, rel)| !ctx.rung_collisions.contains_key(rel))
                .collect()
        };

        // Resolve the content hash. The blocking collect deferred it (stat-match
        // only), so first paint wasn't gated on hashing hundreds of MB of images;
        // we do it here on the background worker. `HashIndex::resolve` stat-matches
        // `bg_hash_index` and hashes only on a miss; the index is persisted after
        // the loop so the next build's collect stat-matches cheaply.
        //
        // A failure here is a source moss cannot read at all — the cloud gate
        // above already took the one transient reason for that — so it gets the
        // same answer as a failed decode, not the silent `return` it used to be.
        let source_oid = if item.source_oid.is_empty() {
            match bg_hash_index.lock().unwrap().resolve(&source_file, &rel_source_str) {
                Ok(oid) => oid,
                Err(e) => {
                    log::warn!("Failed to hash image {}: {}", rel_source_str, e);
                    return ItemStep::base_failed(&rel_source_str, &relative_webp, registered_rungs(), e);
                }
            }
        } else {
            item.source_oid.clone()
        };

        // Nothing points at this one, and the last COMPLETE reference scan is
        // what says so (moss#1085). Encoding it writes bytes the ship-time
        // prune deletes again seconds later. Rungs need no separate test — a
        // rung only ever appears in a srcset beside its base. The item is
        // dropped after blocking.rs already ran `set_pending`, so its promise
        // stays Pending behind the LQIP placeholder, for a URL that — being
        // unreferenced — no page requests.
        if suppressed.contains(&relative_webp) {
            log::trace!(
                "[image] {} is unreferenced per the last ship-time scan — not re-encoding",
                relative_webp
            );
            return ItemStep::nothing_shipped(Vec::new());
        }

        // Singleflight dedup: if another task is already converting this source_oid,
        // block until it finishes and reuse its result. Only the first caller runs
        // the encoder — generalizes the video-only dedup pattern to images (ADR-010).
        //
        // The error is carried inside ImageConversionOutcome.error so all waiters
        // (shared callers) receive the full error message, not just a silent sentinel.
        // Stat key for the placeholder-metadata cache — MUST byte-match the key
        // the blocking scan writes (scan::image_meta_stat_key), so the dominant
        // color + LQIP we compute during the encode land where a later (warm)
        // scan looks for them.
        let meta_stat_key: Option<String> = fs::metadata(&source_file).ok().map(|m| {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            crate::build::scan::scan::image_meta_stat_key(&rel_source_str, m.len(), mtime)
        });

        let (sf_result, was_shared) = {
            let source_file_ref = &source_file;
            let source_oid_ref = &source_oid;
            let relative_webp_ref: &str = &relative_webp;
            let temp_dir_ref = &temp_dir;
            let staging_dir_ref = &ctx.staging_dir;
            let objects_ref = &objects;
            let transforms_ref = &transforms;
            let config_ref = &ctx.config;
            let meta_stat_key_ref = meta_stat_key.as_deref();
            let rung_collisions_ref = &ctx.rung_collisions;

            // Singleflight dedups concurrent duplicate-content conversions so a
            // repeated image decodes once (waiters block on its condvar and reuse
            // the result). do_work is correct under concurrency — the earlier
            // wedge that looked like a do_work deadlock was actually the OS
            // OOM-killing the process; the MegapixelBudget fixes that root cause.
            //
            // The megapixel permit is acquired INSIDE the converter closure, so
            // ONLY the actual converter reserves budget. A waiting duplicate
            // holds no budget while parked, so it can't starve its own converter
            // (which would deadlock the budget). do_work wraps the closure in
            // catch_unwind (which only catches in dev/test — the release profile
            // is panic=abort, so a panicking encode takes the process down rather
            // than leaking a permit; either way the budget can't wedge).
            let acquire_mp = item
                .dimensions
                .map(|(w, h)| {
                    (w as u64).saturating_mul(h as u64).saturating_add(999_999) / 1_000_000
                })
                .unwrap_or(u64::MAX); // unknown dims → reserve the whole budget (run alone)
            services.in_flight_images.do_work(&source_oid, || {
                let _permit = mp_budget.acquire(acquire_mp);
                convert_single_image(
                    source_file_ref,
                    source_oid_ref,
                    relative_webp_ref,
                    temp_dir_ref,
                    staging_dir_ref,
                    objects_ref,
                    transforms_ref,
                    config_ref,
                    cancel_flag,
                    meta_stat_key_ref,
                    rung_collisions_ref,
                )
            })
        };

        if was_shared {
            log::debug!("Image {} shared result from concurrent conversion", filename);
        }

        // Unpack the outcome; error is self-contained in the struct.
        let convert_result: Result<ImageConversionOutcome, String> = match sf_result {
            Some(outcome) => match outcome.error.clone() {
                Some(err) => Err(err),
                None => Ok(outcome),
            },
            None => {
                // singleflight returned None — another concurrent task panicked.
                // Treat as a transient failure; log and continue.
                log::warn!("Image singleflight returned None for {}", filename);
                Err(format!("singleflight returned None for {}", filename))
            }
        };

        match convert_result {
            Ok(outcome) => {
                if is_headless && outcome.webp_oid.is_some() {
                    eprintln!(
                        "  [{}/{}] Done: {} ({} → {} bytes)",
                        index + 1, total, filename, outcome.original_size, outcome.webp_size
                    );
                }
                // A variant counts only when it was produced AND landed at the
                // served path: the cache-hit branch returns Some even when
                // link_to(canonical) failed. Unreachable in practice — every
                // `webp_oid: None` return from `convert_single_image` carries an
                // error the unpack turned into `Err` — but answered with a
                // retraction anyway, because silence here is the exact shape of
                // the "warn-and-return-Ok" pattern that caused the broken-hero
                // bug (docs/archive/2026-05-20-image-variant-honest-mirror.md,
                // Layer 5B).
                let Some(ref base_oid) = outcome.webp_oid.clone().filter(|_| outcome.error.is_none())
                else {
                    return ItemStep::base_failed(
                        &rel_source_str,
                        &relative_webp,
                        registered_rungs(),
                        "encode reported success without producing a variant".to_string(),
                    );
                };

                let mut delivered: Vec<(String, Option<String>)> = Vec::new();
                let mut failed: Vec<(String, String)> = Vec::new();
                let mut item_advisories: Vec<Advisory> = Vec::new();
                {
                    let out = ctx.staging_dir.join(&relative_webp);
                    if !ensure_staged(&objects, base_oid, &out, &relative_webp) {
                        return ItemStep::base_failed(
                            &rel_source_str,
                            &relative_webp,
                            registered_rungs(),
                            "link_to staging failed".to_string(),
                        );
                    }
                    delivered.push((relative_webp.clone(), Some(base_oid.clone())));

                    // Collision membership is re-tested per ITEM: a
                    // singleflight-shared outcome spans duplicate-content items
                    // at DIFFERENT paths, and a collided path must never be
                    // written or flipped Ready — the user's file wins there.
                    for r in &outcome.rungs {
                        let rung_rel =
                            asset_paths::to_webp_rung(&relative_webp, r.width);
                        if ctx.rung_collisions.contains_key(&rung_rel) {
                            log::debug!(
                                "[image] rung {} collides with a user file — leaving the user's file in place",
                                rung_rel
                            );
                            continue;
                        }
                        let rung_err = match (&r.oid, &r.error) {
                            (Some(oid), None) => {
                                let out = ctx.staging_dir.join(&rung_rel);
                                if ensure_staged(&objects, oid, &out, &rung_rel) {
                                    delivered.push((rung_rel.clone(), Some(oid.clone())));
                                    None
                                } else {
                                    Some("link_to staging failed".to_string())
                                }
                            }
                            _ => Some(
                                r.error
                                    .clone()
                                    .unwrap_or_else(|| "rung encode failed".to_string()),
                            ),
                        };
                        // Per-rung failure: THAT rung goes Failed in the
                        // registry (visible warning instead of a stuck
                        // placeholder) + an advisory; the base success above
                        // stands and later rungs were still attempted.
                        if let Some(err) = rung_err {
                            log::warn!(
                                "Rung {}w conversion failed for {}: {}",
                                r.width, filename, err
                            );
                            // Names the SOURCE image, not the failed rung's own
                            // (build-output) path — `item` only ever means "the
                            // source file this is about", and the rung width the
                            // filename used to carry is said in `what` instead.
                            item_advisories.push(Advisory::for_source(
                                Scope::File,
                                Severity::ShippedDegraded,
                                &rel_source_str,
                                crate::infra::app_advisory::fmt(
                                    "shipped_without_optimizing",
                                    &[("err", &format!("{}w: {}", r.width, err))],
                                ),
                                Action::None,
                            ));
                            failed.push((rung_rel.clone(), err));
                        }
                    }

                    // Unresolved-promise sweep: a REGISTERED rung the outcome
                    // does not carry at all (dims-unreadable fallback in
                    // encode_rungs, scan/decode dims divergence, a first-caller
                    // collision) goes Failed — never left Pending forever.
                    for (rw, rung_rel) in registered_rungs() {
                        if outcome.rungs.iter().any(|r| r.width == rw) {
                            continue;
                        }
                        log::warn!(
                            "Registered rung {} was not produced by the encode for {} — marking failed",
                            rung_rel, filename
                        );
                        failed.push((rung_rel, "rung not produced by encode".to_string()));
                    }
                }
                ItemStep::Handled {
                    delivered,
                    failed,
                    advisories: item_advisories,
                    // Real work: a webp landed at the served path (FIX 1b gate input).
                    encoded: true,
                }
            }
            Err(ref e) if e == "Cancelled" => {
                // Observed cancellation mid-encode. Bail this item; the post-loop
                // reduce registers the partial outputs and returns early.
                ItemStep::Cancelled
            }
            // The source went back to the cloud between the gate above and the
            // decode — a live possibility on a vault the provider is actively
            // evicting, and the one failure here that is not the image's fault.
            // Answer it the way the gate does (Pending + a soft notice), not
            // the way a corrupt file is answered (Failed + a warning SVG), or a
            // race decides whether the user sees a broken image.
            Err(_) if crate::build::icloud::is_still_in_the_cloud(&source_file) => {
                ItemStep::deferred_to_cloud(&source_file, &rel_source_str)
            }
            // Bazel's FindMissingBlobs invariant — a manifest-level "hit" must
            // not stand in for a filesystem-level "present". Failed is the
            // negative form; see docs/archive/2026-05-20-image-variant-honest-
            // mirror.md (Layer 5B).
            Err(e) => {
                if is_headless {
                    eprintln!("  [{}/{}] Failed: {}: {}", index + 1, total, filename, e);
                }
                log::warn!("Image conversion failed for {}: {}", filename, e);
                ItemStep::base_failed(&rel_source_str, &relative_webp, registered_rungs(), e)
            }
        }
                    })();

                    // The one teardown.
                    match step {
                        ItemStep::Handled { delivered, failed, advisories: item_advisories, encoded } => {
                            if let Some(fingerprint) = fingerprint.filter(|_| !delivered.is_empty()) {
                                record_image_item_fingerprint(&item.source_path.to_string_lossy(), fingerprint);
                            }
                            record_deliveries(services, &produced_webp_paths, delivered);
                            if let Some(ref registry) = services.assets {
                                for (url, err) in failed {
                                    registry.set_failed(url, err);
                                }
                            }
                            if !item_advisories.is_empty() {
                                advisories.lock().unwrap().extend(item_advisories);
                            }
                            if encoded {
                                converted_count.fetch_add(1, Ordering::SeqCst);
                            }
                            services.end_ui_bound();
                            // Progress ticks on COMPLETION: items finish out of
                            // order, so a monotonic done-count beats the index.
                            let done = completed.fetch_add(1, Ordering::SeqCst) + 1;
                            services.reporter.report(&PipelineEvent::BackgroundProgress {
                                task: "images".to_string(),
                                current: done,
                                total,
                                message: format_progress_message("images", done, total),
                                completed: false,
                                advisories: vec![],
                            });
                        }
                        ItemStep::Cancelled => {
                            any_cancelled.store(true, Ordering::SeqCst);
                            services.end_ui_bound();
                        }
                    }
                });
            }
        });
    }

    // ---- Reduce the parallel accumulators ----
    let advisories = advisories.into_inner().unwrap();
    let converted_count = converted_count.load(Ordering::SeqCst);
    let produced_webp_paths = produced_webp_paths.into_inner().unwrap();

    // Self-heal before registering: this batch's own outputs are verified
    // present (re-materializing from CAS if not) right before both
    // registration call sites below read them, closing the eviction race
    // `self_heal_before_registration` documents. Scoped to `ctx.items` (this
    // batch only, already small after per-image dispatch) so a large vault's
    // untouched images are never touched here. The lock is released before
    // `into_inner()` below moves the same `HashIndex` out for its own save.
    {
        let mut hash_index_guard = bg_hash_index.lock().unwrap();
        let params = ctx.config.to_params();
        let healed = self_heal_before_registration(
            &ctx.items,
            project_root,
            dir_overrides,
            &ctx.staging_dir,
            &objects,
            &transforms,
            &params,
            &mut hash_index_guard,
            &ctx.rung_collisions,
            &suppressed,
        );
        if healed > 0 {
            log::info!(
                "[image] self-heal: re-materialized {} staged .webp file(s) from CAS before registration",
                healed
            );
        }
    }

    // Persist the hash index so the next build's blocking `collect_images_for_
    // conversion` stat-matches instead of re-hashing every image (mirrors the
    // video worker at video.rs). save_merging (not save): the video worker writes
    // the same file concurrently and holds only video entries — a plain overwrite
    // would clobber whichever category was written first. Merge preserves both.
    if let Err(e) = bg_hash_index.into_inner().unwrap().save_merging(&hash_index_path) {
        log::warn!("Failed to save image HashIndex: {}", e);
    }

    // Cancelled mid-run: register whatever landed so the next rebuild's stale
    // cleanup preserves it, then bail before the completion emits — matching the
    // old early-return cancel behavior.
    if any_cancelled.load(Ordering::SeqCst) {
        log::info!(
            "Image conversion cancelled ({} converted before cancel)",
            converted_count
        );
        emit_image_outputs_via_channel(&ctx.tx, &produced_webp_paths, &ctx.staging_dir, &suppressed, services.assets.as_deref());
        return;
    }

    if is_headless {
        if advisories.is_empty() {
            eprintln!("Image conversion complete ({} images)", total);
        } else {
            eprintln!(
                "Image conversion complete ({} images, {} advisories)",
                total,
                advisories.len()
            );
            for a in &advisories {
                match &a.item {
                    Some(f) => eprintln!("  Advisory: {}: {}", f, a.what),
                    None => eprintln!("  Advisory: {}", a.what),
                }
            }
        }
    }

    // Register .webp output paths in the manifest coordinator. Without
    // registration, generated WebP files get deleted on the next rebuild
    // because they have no entry in `image_outputs`. Pre-#620 Item 2 this
    // had a `tx.is_none()` fallback to `update_image_hashes` (on-disk
    // hashes.json read+write); that fallback is gone — every caller goes
    // through the coordinator now.
    emit_image_outputs_via_channel(&ctx.tx, &produced_webp_paths, &ctx.staging_dir, &suppressed, services.assets.as_deref());

    // Dual-emit (Step 3 Phase 4): legacy BackgroundProgress + a media child Job
    // under the Build parent. Gated on REAL work (FIX 1b).
    crate::build::progress::spawn_media_child_job(
        services,
        "images",
        converted_count,
        advisories.clone(),
    );
    services.reporter.report(&PipelineEvent::BackgroundProgress {
        task: "images".to_string(),
        current: total,
        total,
        message: format_progress_message("images", total, total),
        completed: true,
        advisories,
    });
    log::info!("Image conversion complete ({} images)", total);

    // No terminal receipt here, and none in the video worker either: both now
    // belong to `BuildTerminalBarrier`, the single post-join point. The comment
    // this replaced claimed video dispatch "runs on every build" and left the
    // receipt to it; it does not, so image-only sites emitted no `BuildComplete`
    // at all and their preview never refreshed after the first build.
}

/// Send produced `.webp` paths to the manifest coordinator via channel.
///
/// Called at every exit point of `run_image_conversion` when a coordinator
/// channel is available. Uses `blocking_send` since this function runs inside
/// `Spawner::spawn_blocking` (a sync worker thread, not an async task).
///
/// Each path is sent as `HashBucket::ImageVariants`, which inserts into both
/// `image_outputs` (stale-cleanup preservation) AND `inner.files`
/// (content-addressed deploy upload). The xxh3_64 hash is computed from the
/// on-disk bytes at `staging_dir/<path>` — without it the wire manifest would
/// carry an empty hash and the server-side content-addressed diffing would
/// not be able to detect changes to the .webp content (the path would only
/// upload once, then never refresh on re-encode).
///
/// **Producer-side coherence invariant:** every path passed in must exist at
/// `staging_dir/<path>` before this function emits its `EmitMessage`. A
/// pre-flight existence check enforces it; paths whose staged file is missing
/// are skipped (logged at error level) and never reach the manifest. Without
/// this check, a silent failure in `convert_single_image`'s staging-link step
/// would still register the path in `image_outputs` and `inner.files`,
/// breaking deploy with `"Manifest claims '<path>' exists but it's missing on
/// disk"`. Together with the fatal staging-link guard in `convert_single_image`,
/// this closes the register-without-write class of bugs.
///
/// No-op when `paths` is empty (no images were produced).
///
/// `suppressed` (the ship-time prune's verdict, `suppressed_variants_for`)
/// excludes paths from the violation report only — an absent suppressed path
/// is an expected prune, not a bug. Registration is unaffected: it still
/// depends only on presence (moss#1085 gated the self-heal on this set but
/// not the report, hence the false violation on every settled vault).
///
/// Returns the violation lines logged, for tests (no log-capture harness here).
fn emit_image_outputs_via_channel(
    tx: &Option<mpsc::Sender<EmitMessage>>,
    paths: &[(String, Option<String>)],
    staging_dir: &Path,
    suppressed: &std::collections::HashSet<String>,
    registry: Option<&crate::types::assets::AssetRegistry>,
) -> Vec<String> {
    let Some(tx) = tx else { return Vec::new() };
    if paths.is_empty() {
        return Vec::new();
    }
    // Producer-side coherence: never register a manifest entry for a file that
    // is not on disk. If a path reaches these branches, an upstream step
    // (convert_single_image staging-link, ship phase, etc.) reported success
    // without actually landing the bytes. Skip emit; let the missing variant
    // resurface in the next build's scan rather than poisoning the deploy
    // manifest. Violations are COLLECTED and summarized once after the loop —
    // a single root cause (e.g. iCloud evicting every staged .webp) previously
    // logged once per file: 5,967 identical lines in one real bundle.
    let mut missing: Vec<String> = Vec::new();
    let mut suppressed_absent_count: usize = 0;
    let mut read_failures: Vec<(String, String)> = Vec::new();
    // Settling `Failed` — not merely skipping — is what lets `degrade` strip the
    // `<source>`; a skipped variant left `Pending` ships a live 404 that
    // `<picture>` cannot fall back from (ADR-013 amendment 2026-09-09).
    // Suppressed paths stay excluded: absent by the prune's intent, and failing
    // each would make `degrade` read every page on every build of a settled vault.
    let settle_failed = |path: &String, why: String| {
        if let Some(registry) = registry {
            registry.set_failed(path.clone(), why);
        }
    };
    // An I/O error that is not a positive `NotFound` settles nothing: `Failed`
    // strips the `<source>` from the page, and an unreadable variant is not a
    // missing one. The coordinator records it instead, which withholds the
    // generation rather than shipping a page repaired from a blind read.
    let unverified = |path: &String, err: &std::io::Error| {
        let _ = tx.blocking_send(EmitMessage::Unverified { rel_path: path.clone(), detail: err.to_string() });
    };
    for (path, oid) in paths {
        let abs = staging_dir.join(path);
        match crate::build::io_utils::probe_path(&abs) {
            crate::build::io_utils::Presence::Present => {}
            crate::build::io_utils::Presence::Unverified(e) => {
                unverified(path, &e);
                continue;
            }
            crate::build::io_utils::Presence::Absent | crate::build::io_utils::Presence::Evicted => {
                if suppressed.contains(path) {
                    suppressed_absent_count += 1;
                } else {
                    settle_failed(path, "missing at manifest registration".to_string());
                    missing.push(abs.display().to_string());
                }
                continue;
            }
        }
        let hash = match std::fs::read(&abs) {
            Ok(bytes) => crate::build::assets::paths::compute_binary_hash(&bytes),
            Err(e) if !crate::build::icloud::is_definitely_absent(&abs, &e) => {
                unverified(path, &e);
                continue;
            }
            Err(e) => {
                // Race: existed at the pre-flight check, gone now. Treat as the
                // same coherence violation as the missing case and skip —
                // never register without a hash.
                settle_failed(path, format!("unreadable at manifest registration: {}", e));
                read_failures.push((abs.display().to_string(), e.to_string()));
                continue;
            }
        };
        let msg = EmitMessage::File {
            rel_path: path.clone(),
            hash,
            bucket: HashBucket::ImageVariants,
            // `Some` only when the caller already knows the CAS oid backing
            // these exact bytes (the just-encoded main path's own
            // `produced_webp_paths`) — the carry-forward skip path below
            // passes `None` for every entry, since self-heal never surfaces
            // the oid it relinked from. `ship_phase`'s fingerprint fallback
            // covers a `None` entry just as it always has.
            oid: oid.clone(),
        };
        // blocking_send: safe because run_image_conversion runs inside spawn_blocking
        // (dispatched via Spawner::spawn_blocking, not an async spawn).
        // A send failure means the coordinator was dropped (coordinator task panicked
        // or the BackgroundHandle was abandoned). Log and continue — the conversion
        // output is still on disk; the coordinator failure is already propagated
        // through the BackgroundHandle machinery.
        if let Err(e) = tx.blocking_send(msg) {
            log::warn!("[image] Failed to register .webp in coordinator: {}", e);
        }
    }
    if suppressed_absent_count > 0 {
        log::info!(
            "[image] {} orphan-pruned .webp variant(s) absent from staging as expected; \
             not registered, not reported as a violation",
            suppressed_absent_count
        );
    }
    let lines = summarize_coherence_violations(&missing, &read_failures);
    for line in &lines {
        log::error!("{}", line);
    }
    lines
}

/// Collapse per-file coherence violations into at most one log line per class
/// (missing / unreadable), each carrying the suppressed count and a
/// representative sample — journald "suppressed N" style. Without this, one
/// root cause (e.g. iCloud evicting every staged `.webp` in a build) logs once
/// per file: a real "Send logs" bundle had 5,967 identical lines from a single
/// build (see docs/archive/2026-06-03-send-logs-redesign.md).
fn summarize_coherence_violations(
    missing: &[String],
    read_failures: &[(String, String)],
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(first) = missing.first() {
        lines.push(format!(
            "[image] coherence violation: staged .webp missing ({}× this build); \
             skipping manifest registration. First: {}. Upstream staging-link \
             reported success but the bytes are not on disk.",
            missing.len(),
            first
        ));
    }
    if let Some((path, err)) = read_failures.first() {
        lines.push(format!(
            "[image] coherence violation: staged .webp unreadable after existence \
             check ({}× this build); skipping manifest registration. First: {} ({})",
            read_failures.len(),
            path,
            err
        ));
    }
    lines
}

/// Self-heal every base + rung output `items` are expected to have
/// produced, immediately before `run_image_conversion` registers them.
///
/// **Why this exists.** A successful `link_to` at encode time is not proof
/// the bytes survive to registration: `run_image_conversion` registers the
/// whole batch's `produced_webp_paths` only ONCE, after every item in the
/// batch finishes, so an early-finished item's `.webp` sits in
/// `.moss/build/staging` for as long as the rest of the batch takes. On a
/// cloud-synced vault (iCloud / Google Drive) that staging directory's
/// exclusion marker (`moss_paths::exclude_from_cloud_sync`,
/// `com.apple.fileprovider.ignore#P`) can silently fail to stick — moss#964
/// measured it ABSENT on `.moss/build` while present on `.moss/cache` on a
/// real vault — so the provider can evict a just-staged `.webp` before this
/// batch's own registration pass reads it back:
/// `emit_image_outputs_via_channel`'s `output_present` check then finds it
/// gone and silently drops it ("coherence violation: staged .webp missing
/// … Upstream staging-link reported success but the bytes are not on
/// disk"), with no further attempt to recover it.
///
/// The CAS blob store (`.moss/cache`) keeps its own exclusion marker far
/// more reliably (moss#964's own field data), so re-linking from there
/// recovers the bytes without a full re-encode — the same self-heal
/// `dispatch_image_conversions`'s skip branch already relies on for a
/// *carried-forward* image, reused here for one that was *just dispatched*
/// in the batch that is about to register it. No-op per candidate whose
/// staging file is already present. Returns the count actually healed.
fn self_heal_before_registration(
    items: &[ImageConversionItem],
    source_root: &Path,
    dir_overrides: &HashMap<String, String>,
    staging_dir: &Path,
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    params: &serde_json::Value,
    hash_index: &mut crate::build::cache::HashIndex,
    rung_collisions: &HashMap<String, PathBuf>,
    suppressed: &std::collections::HashSet<String>,
) -> usize {
    let mut healed = 0usize;
    for item in items {
        let rel_source = item.source_path.to_string_lossy().to_string();
        let mapped = crate::build::scan::page_map::resolve_path_with_overrides(&rel_source, dir_overrides);
        let relative_webp = moss_core::asset_paths::to_webp(&mapped);
        if !suppressed.contains(&relative_webp) {
            healed += usize::from(matches!(
                rematerialize(
                    objects,
                    transforms,
                    params,
                    hash_index,
                    &source_root.join(&item.source_path),
                    &rel_source,
                    &staging_dir.join(&relative_webp),
                    "image/webp",
                    HashPolicy::HashOnMiss,
                ),
                HealOutcome::Healed
            ));
        }
        if moss_core::asset_paths::is_ladder_source_ext(&item.ext) {
            if let Some((w, h)) = item.dimensions {
                for &rung in moss_core::asset_paths::ladder_rungs(w, h, false) {
                    let rung_rel = moss_core::asset_paths::to_webp_rung(&mapped, rung);
                    if rung_collisions.contains_key(&rung_rel) {
                        continue;
                    }
                    if !suppressed.contains(&rung_rel) {
                        healed += usize::from(matches!(
                            rematerialize(
                                objects,
                                transforms,
                                params,
                                hash_index,
                                &source_root.join(&item.source_path),
                                &rel_source,
                                &staging_dir.join(&rung_rel),
                                &format!("image/webp-w{}", rung),
                                HashPolicy::HashOnMiss,
                            ),
                            HealOutcome::Healed
                        ));
                    }
                }
            }
        }
    }
    healed
}

// `update_image_hashes` removed in #620 Item 2. Pre-Track A this performed
// a read-modify-write on `hashes.json::image_outputs`; the runner now sends
// every produced `.webp` path through the coordinator channel
// (`emit_image_outputs_via_channel`) and the coordinator merges into the
// SealedManifest. There is no longer an on-disk fallback.

/// Dispatch image conversion — GUI spawns async, headless runs synchronously.
///
/// Takes `ctx` by reference because both `dispatch_video_conversions` and
/// `dispatch_image_conversions` are called back-to-back from build.rs with
/// the same `BackgroundContext`. We clone only what we need into an owned
/// `ImageRunContext` for the `'static` spawn.
///
/// Empty `image_items` → return early. Terminal receipts are not this worker's
/// concern at all — see `BuildTerminalBarrier`.
///
/// When `tx` is `Some`, each produced `.webp` path is registered via the
/// coordinator channel instead of the legacy `update_image_hashes` disk write.
///
/// In GUI mode the skip/dispatch decision is made per image, not for the set
/// as a whole: each image's own fingerprint
/// (`compute_image_item_fingerprint`) is compared against the fingerprint
/// recorded when a worker last delivered THAT image. An image whose
/// fingerprint matches is a skip candidate; self-heal (re-materializing its
/// `.webp` from the CAS blob store) is attempted before deciding, and only
/// if that leaves the output present is it carried forward without ever
/// entering `run_image_conversion` — its output keys (base + any ladder
/// rungs) are re-registered so seal() doesn't prune them. Everything else —
/// new, changed, or an unchanged image whose self-heal still leaves the
/// output missing — is dispatched. One added or changed image therefore no
/// longer forces every other, untouched image to re-dispatch and drop out
/// of this round's manifest before the async batch finishes registering
/// them (mirrors `dispatch_video_conversions`, build/media/video.rs, commit
/// 5323496908).
pub(crate) fn dispatch_image_conversions(
    services: Option<&BuildServices>,
    ctx: &BackgroundContext,
    tx: Option<mpsc::Sender<EmitMessage>>,
) {
    if ctx.image_items.is_empty() {
        return;
    }

    let Some(svc) = services else { return };

    let config = ImageCompressionConfig::default();

    if let Some(spawner) = svc.spawner.clone() {
        use std::collections::HashSet;

        let heal_paths = MossPaths::from_moss_dir(ctx.moss_dir.clone());
        let heal_objects = crate::build::cache::ObjectStore::new(heal_paths.cache_objects());
        let heal_transforms = crate::build::cache::TransformCache::new(
            heal_paths.cache_transforms(),
            crate::build::cache::ObjectStore::new(heal_paths.cache_objects()),
        );
        let heal_params = config.to_params();
        let mut heal_index =
            crate::build::cache::HashIndex::load(&heal_paths.cache_hash_index());
        let heal_root = Path::new(&ctx.source_path);

        // WHAT to heal is the ship-time prune's verdict, not the disk scan
        // this loop walks (moss#1085): `ctx.image_items` is every image
        // file under the vault, and the prune keeps only what something
        // points at. See `orphan_prune::suppressed_variants`.
        let heal_suppressed = suppressed_variants_for(&heal_paths, &ctx.staging_dir);

        let total_items = ctx.image_items.len();
        // No `Option<String>` is ever a fresh oid here — this is the
        // carry-forward skip path (`rematerialize` relinks a cached blob but
        // does not surface which one), never the just-encoded main path — so
        // every push below pairs its path with `None`, ship-by-OID's
        // fingerprint fallback covers it.
        let mut skip_paths: Vec<(String, Option<String>)> = Vec::new();
        let mut to_dispatch: Vec<ImageConversionItem> = Vec::new();
        let mut current_paths: HashSet<String> = HashSet::new();
        let mut healed_count: usize = 0;

        for item in &ctx.image_items {
            let rel_source = item.source_path.to_string_lossy().to_string();
            current_paths.insert(rel_source.clone());

            let mapped = crate::build::scan::page_map::resolve_path_with_overrides(
                &rel_source,
                &ctx.dir_overrides,
            );
            let relative_webp = moss_core::asset_paths::to_webp(&mapped);
            let staging_path = ctx.staging_dir.join(&relative_webp);

            // A fingerprint that can't be computed (source unreadable) can't
            // be proven unchanged either — dispatch it rather than risk
            // carrying forward a stale skip.
            let fingerprint_matched =
                compute_image_item_fingerprint(&ctx.source_path, &item.source_path, &config)
                    .is_some_and(|fp| image_item_fingerprint_matches(&rel_source, &fp));

            // Self-heal is only attempted for a skip CANDIDATE: resolving the
            // source hash can require a real read+hash on a cache miss (see
            // `lifecycle::cas_heal::rematerialize`), and a genuinely changed image
            // is about to be dispatched (and re-hashed) anyway — attempting
            // it here first would pay that cost twice on the hot path this
            // runs on every rebuild.
            let outputs_present = if fingerprint_matched {
                if !heal_suppressed.contains(&relative_webp) {
                    healed_count += usize::from(matches!(
                        rematerialize(
                            &heal_objects,
                            &heal_transforms,
                            &heal_params,
                            &mut heal_index,
                            &heal_root.join(&item.source_path),
                            &rel_source,
                            &staging_path,
                            "image/webp",
                            HashPolicy::HashOnMiss,
                        ),
                        HealOutcome::Healed
                    ));
                }
                std::fs::metadata(&staging_path).is_ok_and(|m| m.len() > 0)
            } else {
                false
            };

            if fingerprint_matched && outputs_present {
                // Re-register every key the encode path delivers for this
                // image so seal() keeps them. `emit_image_outputs_via_channel`'s
                // existence check drops any key whose staging file is absent
                // rather than registering a lie.
                skip_paths.push((relative_webp.clone(), None));
                if let Some(ref asset_reg) = svc.assets {
                    // Relay an AssetReady swap ONLY when set_ready actually
                    // flips the asset Pending→Ready — e.g. a just-re-dropped
                    // / re-viewed image whose .webp is already on disk
                    // (2026-07-02). Gating on the transition (not merely
                    // "present") is essential: on a text-only rebuild every
                    // on-disk .webp is already Ready, so an ungated emit
                    // would re-fetch+decode every on-page image on EVERY
                    // save, scaling with image count.
                    if asset_reg.set_ready(relative_webp.clone()) {
                        svc.reporter.report(&PipelineEvent::AssetReady {
                            path: relative_webp,
                            asset_type: "image".to_string(),
                        });
                    }
                }

                // Ladder rungs ride the same skip-path registration +
                // self-heal (Task 5), only for an image that is itself
                // skipping — a dispatched image gets fresh rungs from
                // `run_image_conversion`. Without this, a text-only
                // rebuild's stale cleanup would delete every rung file —
                // they'd be absent from the manifest. Collided paths belong
                // to the user's own file and are skipped with the same
                // membership test as everywhere else.
                if moss_core::asset_paths::is_ladder_source_ext(&item.ext) {
                    if let Some((w, h)) = item.dimensions {
                        for &rung in moss_core::asset_paths::ladder_rungs(w, h, false) {
                            let rung_rel = moss_core::asset_paths::to_webp_rung(&mapped, rung);
                            if ctx.rung_collisions.contains_key(&rung_rel) {
                                continue;
                            }
                            let rung_staging = ctx.staging_dir.join(&rung_rel);
                            if !heal_suppressed.contains(&rung_rel) {
                                healed_count += usize::from(matches!(
                                    rematerialize(
                                        &heal_objects,
                                        &heal_transforms,
                                        &heal_params,
                                        &mut heal_index,
                                        &heal_root.join(&item.source_path),
                                        &rel_source,
                                        &rung_staging,
                                        &format!("image/webp-w{}", rung),
                                        HashPolicy::HashOnMiss,
                                    ),
                                    HealOutcome::Healed
                                ));
                            }
                            skip_paths.push((rung_rel.clone(), None));
                            if rung_staging.exists() {
                                if let Some(ref asset_reg) = svc.assets {
                                    if asset_reg.set_ready(rung_rel.clone()) {
                                        svc.reporter.report(&PipelineEvent::AssetReady {
                                            path: rung_rel,
                                            asset_type: "image".to_string(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                if fingerprint_matched {
                    log::info!(
                        "Image '{}' unchanged but its .webp is missing — \
                         re-dispatching to self-heal instead of skipping",
                        rel_source
                    );
                }
                to_dispatch.push(item.clone());
            }
        }

        // Drop stored fingerprints for images no longer in the current set,
        // so a removed-then-re-added image starts fresh rather than
        // replaying a stale match against bytes that may since have
        // changed. Bounds the map to the live image set.
        retain_image_item_fingerprints(&current_paths);

        // ONE line, not one per file: the per-file form was 96% of an upload.
        if healed_count > 0 {
            log::info!(
                "[image] self-heal: re-materialized {} staged .webp file(s) from CAS",
                healed_count
            );
        }
        // Emit the manifest registrations so seal+stale-cleanup preserves
        // the carried-forward files. No registry: absent here means
        // not-yet-encoded, not vanished, so the contract keeps these
        // `Pending` and preview serves the original. `Failed` would kill
        // that passthrough — and since `output_present` is false for a
        // cloud-evicted file, it would strip the `<source>` of a healthy
        // variant on a synced vault. See
        // docs/archive/2026-05-20-image-variant-honest-mirror.md (Layer 5A).
        if !skip_paths.is_empty() {
            emit_image_outputs_via_channel(
                &tx, &skip_paths, &ctx.staging_dir, &heal_suppressed, None,
            );
        }

        if to_dispatch.is_empty() {
            log::info!(
                "Image set unchanged ({} images), skipping re-dispatch — re-registered carry-forward output keys",
                total_items
            );
            return;
        }
        if to_dispatch.len() < total_items {
            log::info!(
                "{} of {} images unchanged, carrying forward output keys; dispatching {} for conversion",
                total_items - to_dispatch.len(),
                total_items,
                to_dispatch.len()
            );
        }

        // (Pre-Track A, this called `svc.image_cancellation.start_new_conversion()`
        // to clear the legacy AtomicBool flag before spawning. The flag is
        // gone — folder-switch cancel now flows through `FolderSession::cancel`
        // and the bridge inside `run_image_conversion` reads from a fresh
        // local AtomicBool seeded `false` per call. No reset needed.)

        // Only the changed/new/missing-output subset ever enters
        // run_image_conversion's loop — unchanged images never contend for
        // an encode permit or a megapixel-budget slot behind one large
        // encode.
        let mut run_ctx = ImageRunContext::from_background(ctx, config.clone());
        run_ctx.items = to_dispatch;
        let run_ctx = if let Some(t) = tx {
            run_ctx.with_tx(t)
        } else {
            run_ctx
        };

        let total = run_ctx.items.len() as u32;
        for _ in 0..total {
            svc.begin_ui_bound();
        }

        let services_arc = std::sync::Arc::new(BuildServices {
            session: svc.session.clone(),
            cancellation: svc.cancellation.clone(),
            image_cancellation: svc.image_cancellation.clone(),
            notebook_cancellation: svc.notebook_cancellation.clone(),
            processes: svc.processes.clone(),
            reporter: svc.reporter.clone(),
            spawner: svc.spawner.clone(),
            assets: svc.assets.clone(),
            in_flight_videos: svc.in_flight_videos.clone(),
            in_flight_images: svc.in_flight_images.clone(),
            in_flight_metadata: svc.in_flight_metadata.clone(),
            skipped_symlinks: svc.skipped_symlinks.clone(),
            videos_converted: svc.videos_converted.clone(),
            task_registry: svc.task_registry.clone(),
            build_parent: svc.build_parent.clone(),
            page_count: svc.page_count.clone(),
            deploy_video_max_size_mb: svc.deploy_video_max_size_mb,
        });

        log::info!(
            "Spawning background image conversion for {} images",
            run_ctx.items.len()
        );
        // Image dispatch does not use epochs — unlike video, there is no
        // long-running external process to cancel (encoding a single image
        // takes ~100–500 ms). Cancellation is handled via `cancel_flag`
        // on the image-specific `ImageConversionState`, which is checked
        // between items.
        //
        // This task may race `dispatch_background_assets` (a parallel
        // `spawn_blocking`). Safety relies on `copy_deferred_assets`
        // re-reading `hashes.json` to merge `image_outputs` before running
        // stale cleanup — see `media/pipeline.rs::copy_deferred_assets`.
        // If that merge is ever removed, rebuilds will delete our `.webp`s.
        spawner.spawn_blocking(Box::new(move || {
            run_image_conversion(&services_arc, &run_ctx);
        }));
    } else {
        // Headless mode: run synchronously, every item, every time — a
        // headless build is called once per invocation, so there is no
        // previous dispatch to compare against.
        let run_ctx = ImageRunContext::from_background(ctx, config.clone());
        let run_ctx = if let Some(t) = tx {
            run_ctx.with_tx(t)
        } else {
            run_ctx
        };

        let total = ctx.image_items.len() as u32;
        for _ in 0..total {
            svc.begin_ui_bound();
        }
        log::info!(
            "Running headless image conversion for {} images",
            ctx.image_items.len()
        );
        run_image_conversion(svc, &run_ctx);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "image_tests.rs"]
pub(crate) mod tests;
