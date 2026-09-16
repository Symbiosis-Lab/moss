//! Pure URL/path transform helpers — no filesystem access, no env lookups.
//! Used by moss-core's render functions (see crate::render::*) and by
//! upstream src-tauri call sites.
//!
//! # Design Intent
//!
//! Every component that derives an output path from a video or image source
//! path (e.g., .mov → .mp4, .mov → .thumb.jpg, .jpg → .webp) MUST use these
//! functions. This ensures consistency between:
//! - HTML attributes set by the synthesizers (src, data-placeholder-src, poster)
//! - AssetReady event paths emitted by the build pipeline
//! - AssetRegistry keys used by the preview server
//! - iframe-bridge.ts path comparisons
//!
//! If naming conventions change, they change here only.
//!
//! These are pure string functions — no I/O, no filesystem access.
//! They work on both root-relative paths ("videos/clip.mov") and
//! document-relative paths ("../clip.mov").

/// Known video extensions that get transcoded to .mp4.
const MOV_SUFFIXES: &[&str] = &[".mov", ".MOV", ".Mp4", ".MP4"];

/// All video extensions (including those that pass through unchanged for `to_mp4`
/// but still need thumbnail derivation).
const ALL_VIDEO_SUFFIXES: &[&str] = &[
    ".mov", ".MOV", ".mp4", ".MP4", ".Mp4", ".webm", ".WEBM",
    // Dispatched to video conversion by `media::pipeline` like the rest. They
    // were missing here, so `video_stem` left their extension on while the
    // build's own `Path::with_extension("")` stripped it — two derivations of
    // one stem, disagreeing. Anything the converter accepts belongs here.
    ".m4v", ".M4V", ".avi", ".AVI", ".mkv", ".MKV",
];

/// Known image extensions that get re-encoded to .webp.
/// Order matters: longer suffixes (jpeg) before shorter (jpg) is not required
/// because we strip exact suffixes, but keep the list grouped by family for readability.
const IMAGE_SUFFIXES: &[&str] = &[
    ".jpg", ".JPG", ".jpeg", ".JPEG",
    ".png", ".PNG",
    ".webp", ".WEBP",
];

/// Converts a video source path to its .mp4 output path.
///
/// - `.mov` / `.MOV` → `.mp4`
/// - `.Mp4` / `.MP4` → `.mp4` (case normalization)
/// - `.mp4` → `.mp4` (unchanged)
/// - `.webm` → `.webm` (pass-through, not transcoded)
///
/// Any directory prefix is preserved.
///
/// # Examples
/// ```
/// # use moss_core::asset_paths::to_mp4;
/// assert_eq!(to_mp4("clip.mov"), "clip.mp4");
/// assert_eq!(to_mp4("../clip.MOV"), "../clip.mp4");
/// assert_eq!(to_mp4("clip.webm"), "clip.webm");
/// ```
pub fn to_mp4(source: &str) -> String {
    for suffix in MOV_SUFFIXES {
        if let Some(stem) = source.strip_suffix(suffix) {
            return format!("{stem}.mp4");
        }
    }
    // .mp4 (lowercase) and .webm pass through unchanged
    source.to_string()
}

/// Converts a video source path to its thumbnail path (.thumb.jpg).
///
/// All recognized video extensions are stripped and replaced with `.thumb.jpg`.
///
/// # Examples
/// ```
/// # use moss_core::asset_paths::to_thumb;
/// assert_eq!(to_thumb("clip.mov"), "clip.thumb.jpg");
/// assert_eq!(to_thumb("../clip.mp4"), "../clip.thumb.jpg");
/// ```
pub fn to_thumb(source: &str) -> String {
    // A non-video path keeps its full name and gains the suffix, which is the
    // old explicit fallback — `video_stem` returns the input unchanged.
    format!("{}.thumb.jpg", video_stem(source))
}

/// Like `to_thumb`, but returns `None` for non-video paths instead of
/// blindly appending `.thumb.jpg`.
///
/// Use this when the caller needs to *decide* whether a path is a video
/// (e.g. cover-color resolution), as opposed to forcing the conversion.
///
/// # Examples
/// ```
/// # use moss_core::asset_paths::to_thumb_if_video;
/// assert_eq!(to_thumb_if_video("clip.mov").as_deref(), Some("clip.thumb.jpg"));
/// assert_eq!(to_thumb_if_video("clip.MP4").as_deref(), Some("clip.thumb.jpg"));
/// assert_eq!(to_thumb_if_video("photo.jpg"), None);
/// ```
pub fn to_thumb_if_video(source: &str) -> Option<String> {
    video_stem_opt(source).map(|stem| format!("{stem}.thumb.jpg"))
}

/// Converts an image source path to its WebP output path.
///
/// - `.jpg` / `.JPG` / `.jpeg` / `.JPEG` → `.webp`
/// - `.png` / `.PNG` → `.webp`
/// - `.webp` / `.WEBP` → `.webp` (case normalization only)
///
/// Any directory prefix is preserved. Unknown extensions fall back to
/// appending `.webp` (same defensive style as `to_thumb`).
///
/// # Examples
/// ```
/// # use moss_core::asset_paths::to_webp;
/// assert_eq!(to_webp("photo.jpg"), "photo.webp");
/// assert_eq!(to_webp("../photo.PNG"), "../photo.webp");
/// assert_eq!(to_webp("photo.webp"), "photo.webp");
/// ```
pub fn to_webp(source: &str) -> String {
    for suffix in IMAGE_SUFFIXES {
        if let Some(stem) = source.strip_suffix(suffix) {
            return format!("{stem}.webp");
        }
    }
    // Fallback: append .webp (shouldn't happen with known image files)
    format!("{source}.webp")
}

/// The raster source extensions that participate in the responsive ladder
/// (png/jpg/jpeg/webp). Single source of truth for the ladder-membership
/// **GATE** — the "does this extension take part at all?" question, distinct
/// from the "which rung WIDTHS exist?" question [`ladder_rungs`] answers (whose
/// own doc enumerates the ladder-DERIVATION sites). Phase B (Task 12) lifted
/// webp's participation by editing ONLY this predicate, so every gate site
/// picked up webp in lockstep.
///
/// GATE SITES — the six `is_ladder_source_ext(` production callers
/// (grep-verified 2026-07-23), each deciding png/jpg/jpeg/webp participation:
/// 1. **emission** — `is_raster_original` (`render/image.rs`);
/// 2. **registration** — the rung loop in `generate_blocking_content`
///    (`build/render/blocking.rs`);
/// 3. **encode dispatch** — `rungs_gated` in `convert_single_image`
///    (`build/media/image.rs`);
/// 4. **success/failure rung sweep** — `registered_rungs` in
///    `run_image_conversion` (`build/media/image.rs`);
/// 5. **fingerprint-skip heal** — `dispatch_image_conversions`
///    (`build/media/image.rs`);
/// 6. **`should_skip`'s AlreadySmall carve-out** — `raster_with_picture`
///    (`build/media/image.rs`).
///
/// These six are the "six census sites" named in the design doc and
/// MIGRATION-STATE. NOTE: `encode_rungs` is NOT a gate site — it is a
/// [`ladder_rungs`] DERIVATION consumer (it computes the rung SET, it does not
/// gate on extension). A maintainer adding a new participating format updates
/// THIS list; one adding a rung-set derivation updates [`ladder_rungs`]' census.
///
/// NOTE ON `<picture>` vs `<img srcset>`: ladder membership is NOT the same as
/// "emits a `<picture><source>` webp CONVERSION". png/jpg/jpeg are re-encoded to
/// a differently-named `.webp` and emit `<picture><source srcset=X.webp>`; a
/// webp SOURCE is already webp, so [`to_webp`]`(src) == src` and it emits the
/// ladder directly on `<img srcset>` (no `<picture>` — a `<source>` identical to
/// the `<img>` is pointless). Callers that need the CONVERSION-only subset
/// (e.g. `should_skip`'s AlreadySmall carve-out, which must keep small webp
/// eligible to skip re-encode) combine this with [`is_webp_source_ext`]:
/// `is_ladder_source_ext(ext) && !is_webp_source_ext(ext)`.
///
/// Case-insensitive; `ext` is the extension WITHOUT the leading dot
/// ("png", "JPG", "jpeg", "webp"). Emission derives the gate from a full path
/// via `render/image.rs::is_raster_original` / `is_webp_source`; the pipeline
/// sites pass the scan-derived `item.ext` / a `source_file.extension()` string.
///
/// # Examples
/// ```
/// # use moss_core::asset_paths::is_ladder_source_ext;
/// assert!(is_ladder_source_ext("png"));
/// assert!(is_ladder_source_ext("JPG"));
/// assert!(is_ladder_source_ext("webp")); // joined the ladder in Phase B (Task 12)
/// ```
pub fn is_ladder_source_ext(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")
}

/// True when `ext` is a WebP source extension (`webp`, case-insensitive).
///
/// A webp SOURCE is already webp: it does NOT get a differently-named
/// `<picture><source>` webp conversion — instead it carries the responsive
/// ladder directly on `<img srcset>` (Phase B, Task 12). Two callers need this
/// distinction that [`is_ladder_source_ext`] (which now unions webp in) can no
/// longer make alone:
/// - emission (`render/image.rs::synthesize_inner`) routes webp to the
///   `<img srcset>` branch instead of the `<picture>` branch;
/// - `should_skip`'s AlreadySmall carve-out excludes webp from the
///   "must-convert, never-skip" set so a small rung-free webp can still skip
///   the wasteful webp→webp re-encode.
///
/// `ext` is the extension WITHOUT the leading dot.
///
/// # Examples
/// ```
/// # use moss_core::asset_paths::is_webp_source_ext;
/// assert!(is_webp_source_ext("webp"));
/// assert!(is_webp_source_ext("WEBP"));
/// assert!(!is_webp_source_ext("png"));
/// assert!(!is_webp_source_ext("jpg"));
/// ```
pub fn is_webp_source_ext(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("webp")
}

/// Max edge (px) of any deployed raster. Single source of truth — the encode
/// pipeline's `ImageCompressionConfig::default()` reads this constant, and the
/// srcset base-width descriptor caps at it. 2400 covers retina displays.
pub const DEPLOY_MAX_EDGE: u32 = 2400;

/// The responsive ladder: rung widths generated below the deployed base.
/// Must be strictly ascending — the `take_while` in [`ladder_rungs`] depends on it.
/// This is the canonical ladder definition and part of the public `asset_paths`
/// vocabulary (named in the moss-core CHANGELOG); consumers should prefer
/// [`ladder_rungs`] / [`deployed_width`] over indexing `LADDER` directly, so
/// ladder policy stays in one place.
/// See docs/archive/2026-07-22-responsive-image-variants-design.md.
pub const LADDER: [u32; 2] = [800, 1600];

/// Width the deployed base variant actually has after the encoder's
/// aspect-preserving longest-EDGE resize (`img.resize(max_edge, max_edge,
/// Lanczos3)` in build/media/image.rs). When the longest edge exceeds
/// [`DEPLOY_MAX_EDGE`], BOTH dimensions shrink by the same ratio — for
/// portraits the deployed width is therefore SMALLER than `min(w, 2400)`:
/// a 3024×4032 portrait deploys at 1800×2400, so its base width is 1800.
///
/// Integer math (u64 multiply, truncating divide, floor at 1) mirrors the
/// image crate's `resize_dimensions` as closely as practical. srcset width
/// descriptors are browser HINTS: ±1px rounding drift vs the encoder's
/// float `.round()` is acceptable — a Task-5 cross-check test against real
/// encode output pins gross agreement.
pub fn deployed_width(natural_w: u32, natural_h: u32) -> u32 {
    let long_edge = natural_w.max(natural_h);
    if long_edge <= DEPLOY_MAX_EDGE {
        return natural_w;
    }
    ((natural_w as u64 * DEPLOY_MAX_EDGE as u64 / long_edge as u64) as u32).max(1)
}

/// Which ladder rungs exist for a source of `natural_w`×`natural_h` px.
///
/// DETERMINISTIC-AGREEMENT CONTRACT — the ladder-DERIVATION census: the
/// `ladder_rungs(` production callers (grep-verified 2026-07-23). Keep this
/// list current; every set-agreement site derives ladder membership from the
/// same scan-derived inputs. These answer "which rung WIDTHS exist?"; the
/// separate participation GATE ("does this extension take part at all?") is the
/// six-site [`is_ladder_source_ext`] census — a site usually gates there first,
/// then derives here, so most appear on both lists. The five set-agreement
/// sites (all five must produce the identical rung SET):
///
/// 1. **emission** — the synthesizer's `resolve_ladder`
///    (`render/image.rs::synthesize_inner`) decides which rung srcset
///    candidates appear in HTML;
/// 2. **registration** — blocking.rs's rung loop promises each rung URL via
///    `set_source_passthrough` + `set_pending`;
/// 3. **encode** — `encode_rungs` (build/media/rungs.rs, extracted from image.rs
///    at Task 10.5) derives the same ladder from oriented dims on the
///    full-encode and warm-cache paths; `convert_single_image`'s `ladder_len`
///    (build/media/image.rs) reads the same call to keep the oriented original
///    alive for the rung re-encode;
/// 4. **dispatch/sweep/Err-arm retraction** — the worker success/failure
///    arms' `registered_rungs` reconstruction (build/media/image.rs)
///    resolves or retracts every promise registration made;
/// 5. **fingerprint-skip heal** — the unchanged-fingerprint pass
///    (`dispatch_image_conversions`, build/media/image.rs) rematerializes each
///    rung it recomputes here.
///
/// One further `ladder_rungs(` caller is NOT a set-agreement member:
/// `should_skip`'s AlreadySmall carve-out (build/media/image.rs) consults only
/// `ladder_rungs(..).is_empty()` — a "carries any rung?" boolean that keeps a
/// small rung-bearing webp from skipping re-encode — so it never emits,
/// registers, encodes, or sweeps a rung SET and cannot disagree on membership.
///
/// A rung is emitted in HTML iff it is registered iff it is encoded. Never
/// add an input here that one of these sites cannot supply (e.g. encode
/// outcomes, cache state) — that is the parallel-oracle bug class deleted
/// 2026-05-20 (see build/media/image.rs:297-310).
///
/// EXIF-ORIENTATION AGREEMENT (canonical; the pipeline sites back-reference
/// here). The "same scan-derived dims on every site" premise holds
/// UNCONDITIONALLY — including an EXIF-oriented (orientation 5-8, i.e. a 90°/270°
/// rotation) png/webp/jpeg. Scan's `extract_image_dimensions` (build/scan/
/// scan.rs) swaps `w`×`h` for orientation 5-8 on EVERY ladder source, reading
/// orientation through the SAME `read_exif_orientation` the encode side's
/// `decode_oriented` uses — so the dims scan feeds emission/registration/sweep/
/// heal equal encode's oriented dims byte-for-byte and the two ladders cannot
/// diverge. (Before the 2026-07-23 scan-swap fix — design follow-up #1 — scan's
/// orientation read was JPEG-GATED, so an EXIF-rotated png/webp stored the
/// UNswapped header dims and could strand an emitted-but-never-encoded rung →
/// publish-404, preview degrading to the placeholder via the unresolved-promise
/// sweep.) Do NOT re-gate scan's swap to jpeg-only, and do NOT "fix" any future
/// divergence by narrowing the encode side: the encoder strips EXIF, so a base
/// that isn't oriented at encode time ships stored sideways (visibly rotated).
///
/// Rungs are strictly below the deployed base WIDTH (post-resize, see
/// [`deployed_width`] — portrait sources have a smaller base width than
/// `min(w, DEPLOY_MAX_EDGE)`) so the base descriptor never duplicates a
/// rung and no rung is ever wider than the base. Animated sources get no
/// ladder (animation-preserving multi-size re-encode is out of scope).
///
/// APNG: an animated PNG passes `is_raster_original` like any png, and the
/// BASE webp encode already FLATTENS it to a still today (`should_skip` in
/// build/media/image.rs sniffs animation only for gif/webp). Rungs run
/// through the same encode path and inherit the same flattening —
/// consistent by construction, no 404 risk.
///
/// ANIMATED-FLAG AGREEMENT (Phase B, Task 12). Only ONE of the five sites
/// passes a non-`false` flag: **emission** passes the scan-derived
/// `assets.is_animated(src)` for webp sources (an animated webp → empty
/// ladder → bare `<img>`, no srcset). The four pipeline sites (registration,
/// `rungs_gated`/`ladder_len`, `registered_rungs`, fingerprint-heal, plus
/// `encode_rungs`) keep the `false` literal, and that literal is PROVABLY
/// correct — not an assumption: an animated webp returns
/// `SkipReason::AnimatedWebp` in `should_skip`, so `collect_images_for_
/// conversion` drops it BEFORE it can become an `ImageConversionItem`. Every
/// webp that reaches the pipeline is therefore non-animated, and `false`
/// equals its real flag. Because scan's `sniff_is_animated` and `should_skip`
/// both call the SAME `is_animated_webp`, the emission verdict and the
/// pipeline's inclusion verdict never disagree: an animated webp is
/// simultaneously flagged in the snapshot (emission → no rungs) AND filtered
/// from the item list (pipeline → no rungs). Both sides produce zero rungs.
/// png/jpg/jpeg are never animated through this path (animated gif/webp are
/// not in the conversion set) and keep `false` everywhere too.
///
/// WARNING: any future pipeline-side skip that is NOT expressible as an
/// input to this function — e.g. copying the Y1 sized-raster APNG
/// verbatim-keep guard (build/media/image.rs ~line 830) onto rung encodes —
/// would create emitted-but-never-encoded rungs, i.e. the non-recoverable
/// chosen-`<source>` 404 (ADR-013). Task 5 must NOT copy that guard.
pub fn ladder_rungs(natural_w: u32, natural_h: u32, is_animated: bool) -> &'static [u32] {
    if is_animated {
        return &[];
    }
    let base = deployed_width(natural_w, natural_h);
    let n = LADDER.iter().take_while(|&&w| w < base).count();
    &LADDER[..n]
}

/// Rung variant URL: `photo.jpg` + 800 → `photo.w800.webp`.
/// Same derivation style as [`to_webp`]; preserves directory prefix.
///
/// # Examples
/// ```
/// # use moss_core::asset_paths::to_webp_rung;
/// assert_eq!(to_webp_rung("photo.jpg", 800), "photo.w800.webp");
/// assert_eq!(to_webp_rung("../photo.PNG", 1600), "../photo.w1600.webp");
/// ```
pub fn to_webp_rung(source: &str, width: u32) -> String {
    let webp = to_webp(source);
    let stem = webp.strip_suffix(".webp").unwrap_or(&webp);
    format!("{stem}.w{width}.webp")
}

/// One rung of the video delivery ladder: a resolution, a frame rate and the
/// video bitrate that pairs with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoRung {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub video_kbps: u32,
    /// Audio bitrate for this rung, kbps. Audio sits in the rung rather than
    /// in a separate config knob because it is not independent of the picture:
    /// pairing the 45 kbps bottom rung with the 192 kbps track the top rung
    /// uses makes it a 237 kbps rung, which the link this ladder exists for
    /// cannot carry. Only two distinct settings appear across the table, so an
    /// HLS ladder built from it needs exactly two `#EXT-X-MEDIA` groups.
    pub audio_kbps: u32,
    /// Audio channel count. Mono on the lean rungs — at 32 kbps, stereo spends
    /// bits on a second channel that the first one needs.
    pub audio_channels: u32,
}

impl VideoRung {
    /// Total kbps a viewer must sustain for this rung, video plus audio.
    /// Container overhead is a few percent on top and is not counted here.
    pub const fn total_kbps(&self) -> u32 {
        self.video_kbps + self.audio_kbps
    }
}

/// The least bits per pixel per frame that still yields a watchable picture.
///
/// A bitrate floor has to be stated per pixel, not in absolute kbps. The
/// absolute floor this replaced (250 kbps) permitted 0.0090 bits/pixel/frame at
/// 1280x720x30 while rejecting the 0.0260 of a 45 kbps 320x180x30 rung — nearly
/// three times leaner per pixel than the rung it turned away.
///
/// 0.045 is measured, not chosen. Encoding the same source at 45 kbps and
/// 320x180 (docs/archive/2026-08-27-video-delivery-on-slow-networks.md, Stage 0):
/// at 30 fps — 0.0260 — faces and wall texture dissolve; at 15 fps — 0.0521 —
/// the picture holds. Apple's own leanest published rung, 145 kbps at 416x234x30,
/// sits at 0.0497, and every other rung in [`VIDEO_LADDER`] is above 0.05.
///
/// This is what forces the bottom rung to 15 fps rather than 30: at 45 kbps
/// there is no other way to clear the floor.
pub const MIN_BITS_PER_PIXEL_PER_FRAME: f64 = 0.045;

/// The video delivery ladder, lowest rung first.
///
/// Apple's H.264 rungs, spaced the 1.5-2x apart their authoring spec asks for,
/// **plus one rung below Apple's bottom**. That extra rung is the whole reason
/// this table exists: an iPhone on a throttled link measured 118-232 kbps, and
/// Apple's leanest arm (145k video + 64k audio = 209 kbps) took 67 s to show a
/// frame and then drained its buffer. 320x180 at 45 kbps plus a 32 kbps mono
/// audio rendition totals about 77 kbps — roughly 60% of the worst rate
/// observed, which is the margin a link that swings 2x needs.
///
/// Must be strictly ascending in both width and bitrate — the `take_while` in
/// [`video_ladder_rungs`] depends on it. Every rung must clear
/// [`MIN_BITS_PER_PIXEL_PER_FRAME`]; `video_ladder_rungs_clear_the_quality_floor`
/// is what keeps that true when someone edits a number here.
///
/// The **top rung is also the delivery ceiling**. There is deliberately no
/// second `max_video_bitrate_kbps` knob beside this table: two independent
/// statements of how good the best version gets is the overlap that step 1 of
/// the archive doc deleted, and re-introducing it here would rebuild it.
pub const VIDEO_LADDER: [VideoRung; 6] = [
    VideoRung { width: 320, height: 180, fps: 15, video_kbps: 45, audio_kbps: 32, audio_channels: 1 },
    VideoRung { width: 416, height: 234, fps: 30, video_kbps: 145, audio_kbps: 32, audio_channels: 1 },
    VideoRung { width: 640, height: 360, fps: 30, video_kbps: 365, audio_kbps: 192, audio_channels: 2 },
    VideoRung { width: 768, height: 432, fps: 30, video_kbps: 730, audio_kbps: 192, audio_channels: 2 },
    VideoRung { width: 960, height: 540, fps: 30, video_kbps: 1100, audio_kbps: 192, audio_channels: 2 },
    VideoRung { width: 1280, height: 720, fps: 30, video_kbps: 2000, audio_kbps: 192, audio_channels: 2 },
];

/// The rungs worth encoding for a source of this width.
///
/// Truncated from the **top only**: `take_while` drops rungs wider than the
/// source, exactly as [`ladder_rungs`] does for images. Never truncate from the
/// bottom — the bottom rung is the entire point of the ladder, and a
/// low-bitrate source still downscales to it.
///
/// A source narrower than every rung keeps the bottom one, so the result is
/// never empty; encoding a 200 px clip at 320 px wastes a little, and having no
/// rung at all would be a video that cannot be delivered.
pub fn video_ladder_rungs(source_width: u32) -> &'static [VideoRung] {
    let n = VIDEO_LADDER
        .iter()
        .take_while(|r| r.width <= source_width)
        .count()
        .max(1);
    &VIDEO_LADDER[..n]
}

/// The rungs a ladder of `n` rungs was built from, or `None` if `n` is not a
/// ladder this table can produce.
///
/// The inverse of [`video_ladder_rungs`], which truncates from the top only —
/// so a ladder's length names its rungs. A cache holding a ladder knows how
/// many files it holds but not the width of the source that produced them, and
/// this is what lets it identify the ladder without re-reading that source.
pub fn video_ladder_rungs_by_count(n: usize) -> Option<&'static [VideoRung]> {
    (2..=VIDEO_LADDER.len()).contains(&n).then(|| &VIDEO_LADDER[..n])
}

/// The ladder flattened into a transform-cache key.
///
/// Delivery policy is a const table, not configuration, so nothing in an
/// encoder's config struct changes when a rung is edited — and every site would
/// go on serving renditions encoded under the old table, invisibly. Any edit to
/// any rung changes this string, which is what an edit to it means.
pub fn video_ladder_fingerprint() -> String {
    VIDEO_LADDER
        .iter()
        .map(|r| {
            format!(
                "{}x{}@{}/{}+{}x{}",
                r.width, r.height, r.fps, r.video_kbps, r.audio_kbps, r.audio_channels
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The single rung a progressive (non-ladder) encode targets: the best one this
/// source can fill. The progressive MP4 **is** the ladder's top rung for that
/// source, which is why no separate ceiling constant exists.
pub fn video_top_rung(source_width: u32) -> VideoRung {
    let rungs = video_ladder_rungs(source_width);
    rungs[rungs.len() - 1]
}

/// Which of the two audio renditions a rung is bound to.
///
/// Two, and exactly two, because `VIDEO_LADDER` holds two distinct audio
/// settings. Each becomes one `#EXT-X-MEDIA:TYPE=AUDIO` group in the master
/// playlist, and every rung names the one it can pay for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioGroup {
    /// 32 kbps mono, for the rungs a starved link can actually reach. One
    /// shared 192 kbps track would put the bottom rung at 237 kbps and make it
    /// unreachable however small its picture got.
    Lean,
    /// 192 kbps stereo, for rungs whose viewer can afford transparent audio.
    Clean,
}

impl AudioGroup {
    /// The short token used both as the ffmpeg variant name and as the
    /// filename infix, so a file on disk says which group it belongs to.
    pub const fn as_str(&self) -> &'static str {
        match self {
            AudioGroup::Lean => "alo",
            AudioGroup::Clean => "ahi",
        }
    }
}

impl VideoRung {
    /// The audio rendition this rung is delivered with.
    pub const fn audio_group(&self) -> AudioGroup {
        if self.audio_channels == 1 {
            AudioGroup::Lean
        } else {
            AudioGroup::Clean
        }
    }
}

/// Strips a known video extension, or `None` when `source` is not a video.
fn video_stem_opt(source: &str) -> Option<&str> {
    ALL_VIDEO_SUFFIXES
        .iter()
        .find_map(|suffix| source.strip_suffix(suffix))
}

/// Strips any known video extension, leaving the path stem. A non-video path
/// is returned unchanged.
fn video_stem(source: &str) -> &str {
    video_stem_opt(source).unwrap_or(source)
}

/// The directory one video's HLS ladder lives in: `clip.mov` → `clip.hls`.
///
/// A directory, and constant filenames inside it, so that **a ladder is
/// relocatable**. ffmpeg writes every cross-reference in these playlists by
/// name — the master names its rung playlists, each rung playlist names its own
/// segment in an `#EXT-X-MAP` — so bytes that embed the video's stem can only
/// ever be served from that one stem. The transform cache is keyed by content,
/// which means the same bytes legitimately arrive at a second path (a rename, or
/// one video copied into two folders), and linking stem-bearing playlists there
/// yields a master whose every child 404s. Constant names inside a
/// per-video directory make that impossible rather than merely unlikely.
///
/// The `.hls` suffix rather than a bare stem so the directory cannot collide
/// with a real folder sitting beside the video.
pub fn to_hls_dir(source: &str) -> String {
    format!("{}.hls", video_stem(source))
}

/// HLS master playlist URL — the ONE url the page links to.
///
/// Every other HLS file is discovered by the player through this playlist, so
/// this is the only one an emitter needs to know.
///
/// # Examples
/// ```
/// # use moss_core::asset_paths::to_hls_master;
/// assert_eq!(to_hls_master("clip.mov"), "clip.hls/master.m3u8");
/// assert_eq!(to_hls_master("videos/a.MP4"), "videos/a.hls/master.m3u8");
/// // A dot in the filename is just a filename.
/// assert_eq!(to_hls_master("2024.05.trip.mov"), "2024.05.trip.hls/master.m3u8");
/// ```
pub fn to_hls_master(source: &str) -> String {
    format!("{}/{}", to_hls_dir(source), HLS_MASTER_NAME)
}

/// The master playlist's name inside [`to_hls_dir`]. Constant, so recognising
/// one is an exact match rather than a guess about the shape of a filename.
pub const HLS_MASTER_NAME: &str = "master.m3u8";

/// Whether `url` is a ladder's master playlist, and if so the source stem it
/// belongs to: `clip.hls/master.m3u8` → `Some("clip")`.
///
/// Exact, where asking "does this stem contain a dot?" was not: a video named
/// `2024.05.trip.mov` reads as a rung playlist under that question and its
/// ladder is silently never offered, leaving seventeen encoded files on every
/// build that no page ever references.
pub fn hls_master_stem(url: &str) -> Option<&str> {
    let dir = url.strip_suffix(HLS_MASTER_NAME)?.strip_suffix('/')?;
    dir.strip_suffix(".hls")
}

/// The names inside one ladder's directory, master first.
///
/// One owner for the census. The image ladder re-derives its membership at
/// five call sites and this module documents that as a fragile contract; video
/// does not repeat it — the encoder writes this list and the registry promises
/// it, both from here.
pub fn hls_members(rungs: &[VideoRung]) -> Vec<String> {
    let mut out = vec![HLS_MASTER_NAME.to_string()];
    for i in 0..rungs.len() {
        out.push(format!("v{i}.m3u8"));
        out.push(format!("v{i}.m4s"));
    }
    for g in audio_groups(rungs) {
        out.push(format!("{}.m3u8", g.as_str()));
        out.push(format!("{}.m4s", g.as_str()));
    }
    out
}

/// The distinct audio rendition groups a set of rungs needs, in ladder order.
///
/// Groups, not rungs: several rungs share one rendition and it is encoded once.
/// The one owner — the census and the encoder's `-var_stream_map` must agree on
/// how many renditions exist, and they did each dedup it separately.
pub fn audio_groups(rungs: &[VideoRung]) -> Vec<AudioGroup> {
    let mut groups: Vec<AudioGroup> = Vec::new();
    for r in rungs {
        if !groups.contains(&r.audio_group()) {
            groups.push(r.audio_group());
        }
    }
    groups
}

/// Every URL an HLS encode of `source` writes, master first.
pub fn hls_outputs(source: &str, rungs: &[VideoRung]) -> Vec<String> {
    let dir = to_hls_dir(source);
    hls_members(rungs)
        .into_iter()
        .map(|name| format!("{dir}/{name}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── to_mp4 tests ───────────────────────────────────────────────

    #[test]
    fn test_to_mp4_mov() {
        assert_eq!(to_mp4("clip.mov"), "clip.mp4");
    }

    #[test]
    fn test_to_mp4_mov_uppercase() {
        assert_eq!(to_mp4("clip.MOV"), "clip.mp4");
    }

    #[test]
    fn test_to_mp4_mp4_mixed_case() {
        assert_eq!(to_mp4("clip.Mp4"), "clip.mp4");
    }

    #[test]
    fn test_to_mp4_mp4_uppercase() {
        assert_eq!(to_mp4("clip.MP4"), "clip.mp4");
    }

    #[test]
    fn test_to_mp4_already_mp4() {
        assert_eq!(to_mp4("clip.mp4"), "clip.mp4");
    }

    #[test]
    fn test_to_mp4_webm_passthrough() {
        assert_eq!(to_mp4("clip.webm"), "clip.webm");
    }

    #[test]
    fn test_to_mp4_with_relative_prefix() {
        assert_eq!(to_mp4("../clip.mov"), "../clip.mp4");
    }

    #[test]
    fn test_to_mp4_with_dot_prefix() {
        assert_eq!(to_mp4("./clip.MOV"), "./clip.mp4");
    }

    #[test]
    fn test_to_mp4_with_directory() {
        assert_eq!(to_mp4("videos/clip.mov"), "videos/clip.mp4");
    }

    #[test]
    fn test_to_mp4_unicode_filename() {
        assert_eq!(to_mp4("videos/冬日之歌.mov"), "videos/冬日之歌.mp4");
    }

    #[test]
    fn test_to_mp4_space_in_name() {
        assert_eq!(to_mp4("./Morning Mist.mov"), "./Morning Mist.mp4");
    }

    // ── to_thumb tests ─────────────────────────────────────────────

    #[test]
    fn test_to_thumb_mov() {
        assert_eq!(to_thumb("clip.mov"), "clip.thumb.jpg");
    }

    #[test]
    fn test_to_thumb_mp4() {
        assert_eq!(to_thumb("clip.mp4"), "clip.thumb.jpg");
    }

    #[test]
    fn test_to_thumb_mov_uppercase() {
        assert_eq!(to_thumb("clip.MOV"), "clip.thumb.jpg");
    }

    #[test]
    fn test_to_thumb_mp4_uppercase() {
        assert_eq!(to_thumb("clip.MP4"), "clip.thumb.jpg");
    }

    #[test]
    fn test_to_thumb_mp4_mixed() {
        assert_eq!(to_thumb("clip.Mp4"), "clip.thumb.jpg");
    }

    #[test]
    fn test_to_thumb_webm() {
        assert_eq!(to_thumb("clip.webm"), "clip.thumb.jpg");
    }

    #[test]
    fn test_to_thumb_with_directory() {
        assert_eq!(to_thumb("videos/clip.mov"), "videos/clip.thumb.jpg");
    }

    #[test]
    fn test_to_thumb_with_relative_prefix() {
        assert_eq!(to_thumb("../clip.mp4"), "../clip.thumb.jpg");
    }

    #[test]
    fn test_to_thumb_unicode() {
        assert_eq!(to_thumb("videos/冬日之歌.mov"), "videos/冬日之歌.thumb.jpg");
    }

    #[test]
    fn test_to_thumb_space() {
        assert_eq!(to_thumb("./Morning Mist.MOV"), "./Morning Mist.thumb.jpg");
    }

    // ── to_thumb_if_video tests ────────────────────────────────────

    #[test]
    fn test_to_thumb_if_video_recognizes_lowercase_extensions() {
        assert_eq!(to_thumb_if_video("clip.mov").as_deref(), Some("clip.thumb.jpg"));
        assert_eq!(to_thumb_if_video("clip.mp4").as_deref(), Some("clip.thumb.jpg"));
        assert_eq!(to_thumb_if_video("clip.webm").as_deref(), Some("clip.thumb.jpg"));
    }

    #[test]
    fn test_to_thumb_if_video_recognizes_uppercase_extensions() {
        // Regression for the original bug: uppercase video extensions
        // (.MOV from iPhone, .MP4 from some cameras) must be detected.
        assert_eq!(to_thumb_if_video("clip.MOV").as_deref(), Some("clip.thumb.jpg"));
        assert_eq!(to_thumb_if_video("clip.MP4").as_deref(), Some("clip.thumb.jpg"));
        assert_eq!(to_thumb_if_video("clip.Mp4").as_deref(), Some("clip.thumb.jpg"));
        assert_eq!(to_thumb_if_video("clip.WEBM").as_deref(), Some("clip.thumb.jpg"));
    }

    #[test]
    fn test_to_thumb_if_video_with_directory() {
        assert_eq!(to_thumb_if_video("音乐/cover.mp4").as_deref(), Some("音乐/cover.thumb.jpg"));
        assert_eq!(to_thumb_if_video("../clip.MOV").as_deref(), Some("../clip.thumb.jpg"));
    }

    #[test]
    fn test_to_thumb_if_video_returns_none_for_images() {
        assert_eq!(to_thumb_if_video("photo.jpg"), None);
        assert_eq!(to_thumb_if_video("photo.png"), None);
        assert_eq!(to_thumb_if_video("photo.webp"), None);
        assert_eq!(to_thumb_if_video("photo.JPEG"), None);
    }

    #[test]
    fn test_to_thumb_if_video_returns_none_for_unknown() {
        // Unlike `to_thumb`, no `.thumb.jpg` fallback is appended.
        // Callers using this helper want a definitive yes/no.
        assert_eq!(to_thumb_if_video("file.gif"), None);
        assert_eq!(to_thumb_if_video("file"), None);
        assert_eq!(to_thumb_if_video(""), None);
    }

    // ── to_webp tests ──────────────────────────────────────────────

    #[test]
    fn test_to_webp_jpg() {
        assert_eq!(to_webp("photo.jpg"), "photo.webp");
    }

    #[test]
    fn test_to_webp_jpg_uppercase() {
        assert_eq!(to_webp("photo.JPG"), "photo.webp");
    }

    #[test]
    fn test_to_webp_jpeg() {
        assert_eq!(to_webp("photo.jpeg"), "photo.webp");
    }

    #[test]
    fn test_to_webp_jpeg_uppercase() {
        assert_eq!(to_webp("photo.JPEG"), "photo.webp");
    }

    #[test]
    fn test_to_webp_png() {
        assert_eq!(to_webp("photo.png"), "photo.webp");
    }

    #[test]
    fn test_to_webp_png_uppercase() {
        assert_eq!(to_webp("photo.PNG"), "photo.webp");
    }

    #[test]
    fn test_to_webp_already_webp() {
        assert_eq!(to_webp("photo.webp"), "photo.webp");
    }

    #[test]
    fn test_to_webp_webp_uppercase() {
        assert_eq!(to_webp("photo.WEBP"), "photo.webp");
    }

    #[test]
    fn test_to_webp_with_directory() {
        assert_eq!(to_webp("images/photo.jpg"), "images/photo.webp");
    }

    #[test]
    fn test_to_webp_with_relative_prefix() {
        assert_eq!(to_webp("../photo.png"), "../photo.webp");
    }

    #[test]
    fn test_to_webp_with_dot_prefix() {
        assert_eq!(to_webp("./photo.JPG"), "./photo.webp");
    }

    #[test]
    fn test_to_webp_unicode_filename() {
        assert_eq!(to_webp("images/冬日之歌.jpg"), "images/冬日之歌.webp");
    }

    #[test]
    fn test_to_webp_space_in_name() {
        assert_eq!(to_webp("./Morning Mist.png"), "./Morning Mist.webp");
    }

    #[test]
    fn test_to_webp_unknown_extension_fallback() {
        // Unknown extensions fall back to appending .webp (matches to_thumb's defensive style).
        assert_eq!(to_webp("file.gif"), "file.gif.webp");
    }

    // ── is_ladder_source_ext tests ─────────────────────────────────

    #[test]
    fn is_ladder_source_ext_accepts_png_jpg_jpeg_webp_case_insensitively() {
        // Phase B (Task 12) added webp/WEBP: a webp SOURCE now participates in
        // the responsive ladder (emitted on `<img srcset>`, not `<picture>`).
        for ext in [
            "png", "PNG", "jpg", "JPG", "jpeg", "JPEG", "Jpg", "jPeG", "webp", "WEBP", "WebP",
        ] {
            assert!(is_ladder_source_ext(ext), "{ext} must be a ladder source");
        }
    }

    #[test]
    fn is_ladder_source_ext_rejects_non_ladder_formats() {
        // webp joined in Phase B (asserted above); these never join.
        for ext in ["gif", "svg", "avif", "heic", "bmp", "tiff", ""] {
            assert!(!is_ladder_source_ext(ext), "{ext} must NOT be a ladder source");
        }
    }

    #[test]
    fn is_webp_source_ext_matches_only_webp() {
        // The conversion-vs-webp split: webp is a ladder source that is NOT a
        // `<picture>` conversion (it emits `<img srcset>` and can skip a small
        // re-encode). png/jpg/jpeg are ladder sources that ARE conversions.
        for ext in ["webp", "WEBP", "WebP"] {
            assert!(is_webp_source_ext(ext), "{ext} must be a webp source");
        }
        for ext in ["png", "PNG", "jpg", "jpeg", "gif", "svg", ""] {
            assert!(!is_webp_source_ext(ext), "{ext} must NOT be a webp source");
        }
    }

    // ── ladder tests ───────────────────────────────────────────────

    #[test]
    fn ladder_is_strictly_ascending() {
        assert!(LADDER.windows(2).all(|w| w[0] < w[1]));
    }

    /// The floor is the table's invariant, not a separate policy: a rung that
    /// does not clear it is a rung that ships smear. This is what would fail if
    /// someone lowered a rung's bitrate or raised its resolution in isolation.
    #[test]
    fn hls_outputs_writes_one_audio_rendition_per_group_not_per_rung() {
        // The whole point of rendition groups: six rungs, but audio stored
        // twice. Muxing per rung would store it six times and degrade it in
        // lockstep with the picture.
        let outs = hls_outputs("clip.mov", &VIDEO_LADDER);
        assert_eq!(outs[0], "clip.hls/master.m3u8", "master comes first");
        assert_eq!(
            outs.len(),
            1 + VIDEO_LADDER.len() * 2 + 2 * 2,
            "master + playlist/segment per rung + playlist/segment per audio group"
        );
        assert_eq!(outs.len(), 17, "the measured file count for the full ladder");
        let audio: Vec<_> = outs.iter().filter(|o| o.contains("/a")).collect();
        assert_eq!(audio.len(), 4, "two groups, a playlist and a segment each");
    }

    #[test]
    fn hls_outputs_are_unique_so_no_rung_overwrites_another() {
        let outs = hls_outputs("clip.mov", &VIDEO_LADDER);
        let mut sorted = outs.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), outs.len(), "every emitted path is distinct");
    }

    #[test]
    fn hls_outputs_shrink_with_a_truncated_ladder() {
        // A narrow source gets fewer rungs, and if every surviving rung shares
        // one audio group only that group is written.
        let rungs = video_ladder_rungs(320);
        assert_eq!(rungs.len(), 1);
        let outs = hls_outputs("clip.mov", rungs);
        assert_eq!(outs, vec![
            "clip.hls/master.m3u8",
            "clip.hls/v0.m3u8",
            "clip.hls/v0.m4s",
            "clip.hls/alo.m3u8",
            "clip.hls/alo.m4s",
        ]);
    }

    #[test]
    fn the_two_lean_rungs_share_the_lean_audio_group() {
        // Not "the bottom rung only": Apple's 416x234 @ 145k bound to the
        // 192k track totals 370 kbps, which the measured link cannot carry.
        assert_eq!(VIDEO_LADDER[0].audio_group(), AudioGroup::Lean);
        assert_eq!(VIDEO_LADDER[1].audio_group(), AudioGroup::Lean);
        for rung in &VIDEO_LADDER[2..] {
            assert_eq!(rung.audio_group(), AudioGroup::Clean);
        }
    }

    #[test]
    fn hls_urls_keep_the_directory_and_drop_the_source_extension() {
        assert_eq!(to_hls_master("videos/a.MOV"), "videos/a.hls/master.m3u8");
        assert_eq!(to_hls_dir("videos/a.MOV"), "videos/a.hls");
    }

    /// The names inside the directory carry no trace of the video they came
    /// from, which is what lets a cached ladder be linked under a second name.
    /// A rename produces the same bytes at a new path; if these embedded the
    /// stem, the master would name children that are not there.
    #[test]
    fn nothing_inside_a_ladder_directory_names_the_video() {
        for name in hls_members(&VIDEO_LADDER) {
            assert!(!name.contains('/'), "{name} is not a bare name");
            assert!(!name.contains("clip"), "{name} embeds a source stem");
        }
        let a = hls_outputs("trips/holiday.mov", &VIDEO_LADDER);
        let b = hls_outputs("trips/iceland.mov", &VIDEO_LADDER);
        let strip = |v: Vec<String>| -> Vec<String> {
            v.iter().map(|p| p.rsplit('/').next().unwrap().to_string()).collect()
        };
        assert_eq!(strip(a), strip(b), "two videos differ only by directory");
    }

    /// A dot in a video's own filename is just a filename. Asking whether the
    /// stem contains a dot got this wrong and silently withheld the ladder.
    #[test]
    fn a_master_is_recognised_exactly_even_with_dots_in_the_name() {
        assert_eq!(hls_master_stem("2024.05.trip.hls/master.m3u8"), Some("2024.05.trip"));
        assert_eq!(hls_master_stem("a/b/clip.hls/master.m3u8"), Some("a/b/clip"));
        assert_eq!(hls_master_stem("clip.hls/v0.m3u8"), None);
        assert_eq!(hls_master_stem("clip.m3u8"), None);
        assert_eq!(hls_master_stem("clip.mp4"), None);
    }

    /// Every extension the converter accepts must strip here, or the build's
    /// own stem derivation and this one disagree and the ladder lands under a
    /// directory no page references.
    #[test]
    fn every_converted_extension_strips_to_the_same_stem() {
        for ext in ["mov", "mp4", "webm", "m4v", "avi", "mkv"] {
            assert_eq!(
                to_hls_dir(&format!("a/clip.{ext}")),
                "a/clip.hls",
                "{ext} did not strip"
            );
        }
    }

    #[test]
    fn video_ladder_rungs_clear_the_quality_floor() {
        for r in VIDEO_LADDER {
            let bpp = (r.video_kbps as f64 * 1000.0) / (r.width * r.height * r.fps) as f64;
            assert!(
                bpp >= MIN_BITS_PER_PIXEL_PER_FRAME,
                "{}x{}@{}fps {}k is {:.4} bits/pixel/frame, under the {} floor",
                r.width, r.height, r.fps, r.video_kbps, bpp, MIN_BITS_PER_PIXEL_PER_FRAME
            );
        }
    }

    /// Apple's authoring spec requires a multivariant playlist delivered over
    /// cellular to contain a variant peaking at 192 kbit/s or less. The bottom
    /// rung is that variant; if it ever stops being, the ladder is
    /// non-compliant and — more to the point — unreachable on the link it was
    /// built for.
    #[test]
    fn video_ladder_bottom_rung_fits_apples_cellular_ceiling() {
        assert!(VIDEO_LADDER[0].total_kbps() <= 192, "{}", VIDEO_LADDER[0].total_kbps());
    }

    /// An HLS ladder needs one `#EXT-X-MEDIA` audio group per distinct audio
    /// setting. Two is the design; a third would be a silent cost increase.
    #[test]
    fn video_ladder_has_exactly_two_audio_renditions() {
        let mut seen: Vec<(u32, u32)> =
            VIDEO_LADDER.iter().map(|r| (r.audio_kbps, r.audio_channels)).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 2, "{seen:?}");
    }

    #[test]
    fn video_ladder_is_strictly_ascending() {
        assert!(VIDEO_LADDER.windows(2).all(|w| w[0].width < w[1].width));
        assert!(VIDEO_LADDER.windows(2).all(|w| w[0].video_kbps < w[1].video_kbps));
    }

    /// Truncation is from the top only. A source narrower than every rung still
    /// gets the bottom one — an empty ladder is a video that cannot be played.
    #[test]
    fn video_ladder_truncates_from_the_top_and_never_to_nothing() {
        assert_eq!(video_ladder_rungs(1920).len(), VIDEO_LADDER.len());
        assert_eq!(video_ladder_rungs(1280).len(), VIDEO_LADDER.len());
        assert_eq!(video_ladder_rungs(640).len(), 3);
        assert_eq!(video_ladder_rungs(200).len(), 1);
        assert_eq!(video_ladder_rungs(200)[0], VIDEO_LADDER[0]);
    }

    #[test]
    fn video_top_rung_is_the_best_the_source_can_fill() {
        assert_eq!(video_top_rung(1920).width, 1280);
        assert_eq!(video_top_rung(640).width, 640);
        assert_eq!(video_top_rung(100).width, 320);
    }

    #[test]
    fn ladder_rungs_below_deployed_base_only() {
        assert_eq!(ladder_rungs(2000, 1200, false), &[800, 1600][..]);
        assert_eq!(ladder_rungs(1601, 900, false), &[800, 1600][..]);
        // strict: no upscale, no dup of base
        assert_eq!(ladder_rungs(1600, 900, false), &[800][..]);
        assert_eq!(ladder_rungs(801, 600, false), &[800][..]);
        assert_eq!(ladder_rungs(800, 600, false), &[] as &[u32]);
        assert_eq!(ladder_rungs(0, 0, false), &[] as &[u32]);
    }

    #[test]
    fn ladder_rungs_capped_width_never_duplicates_base() {
        // 4000px-wide landscape deploys at DEPLOY_MAX_EDGE (2400) wide;
        // rungs must stay below the cap.
        assert_eq!(ladder_rungs(4000, 3000, false), &[800, 1600][..]);
        assert_eq!(ladder_rungs(2400, 1600, false), &[800, 1600][..]);
        // Square at exactly the cap: base 2400, both rungs below it.
        assert_eq!(ladder_rungs(2400, 2400, false), &[800, 1600][..]);
    }

    #[test]
    fn ladder_rungs_portrait_uses_post_resize_width() {
        // 3024×4032 portrait: the encoder shrinks the longest EDGE to 2400,
        // so the deployed base is 1800 wide — both rungs still below it.
        assert_eq!(ladder_rungs(3024, 4032, false), &[800, 1600][..]);
        // Extreme portrait 1179×8000: base width 353 — NO rung is below it,
        // so the ladder must be empty (a w800 rung would be WIDER than the
        // base: ladder inversion).
        assert_eq!(ladder_rungs(1179, 8000, false), &[] as &[u32]);
        // 1200×3600: base width exactly 800 — strict `<` excludes the 800 rung.
        assert_eq!(ladder_rungs(1200, 3600, false), &[] as &[u32]);
    }

    #[test]
    fn ladder_rungs_animated_is_empty() {
        assert_eq!(ladder_rungs(2000, 1200, true), &[] as &[u32]);
    }

    #[test]
    fn deployed_width_caps_longest_edge() {
        // Landscape: width IS the longest edge — capped directly.
        assert_eq!(deployed_width(4000, 3000), 2400);
        assert_eq!(deployed_width(2000, 1200), 2000);
        // Portrait: HEIGHT is the longest edge; width shrinks by the same
        // aspect-preserving ratio the encoder applies.
        assert_eq!(deployed_width(3024, 4032), 1800);
        assert_eq!(deployed_width(1179, 8000), 353);
        assert_eq!(deployed_width(1200, 3600), 800);
        // Square at the cap: untouched.
        assert_eq!(deployed_width(2400, 2400), 2400);
        // Degenerate sliver never collapses to 0.
        assert_eq!(deployed_width(1, 100_000), 1);
    }

    #[test]
    fn to_webp_rung_inserts_width_suffix() {
        assert_eq!(to_webp_rung("photo.jpg", 800), "photo.w800.webp");
        assert_eq!(to_webp_rung("a/b/photo.PNG", 1600), "a/b/photo.w1600.webp");
        assert_eq!(to_webp_rung("photo.webp", 800), "photo.w800.webp");
    }

    #[test]
    fn to_webp_rung_unknown_extension_fallback() {
        // Unknown extensions inherit to_webp's defensive-append behavior.
        assert_eq!(to_webp_rung("file.gif", 800), "file.gif.w800.webp");
    }
}
