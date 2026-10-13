//! Pure URL/path transform helpers — no filesystem access, no env lookups.
//! Used by moss-core's render functions (see crate::render::*) and by
//! upstream call sites in moss-build.
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

/// Suffix of a video's hero poster: the thumbnail's frame at the video's own size.
pub const POSTER_SUFFIX: &str = ".poster.jpg";

/// A video source's hero poster path (`.poster.jpg`). A hero fills the window,
/// so it shows this instead of the 800-pixel [`to_thumb`] while the video starts.
///
/// # Examples
/// ```
/// # use moss_core::asset_paths::to_poster;
/// assert_eq!(to_poster("../clip.mp4"), "../clip.poster.jpg");
/// ```
pub fn to_poster(source: &str) -> String {
    format!("{}{POSTER_SUFFIX}", video_stem(source))
}

/// Whether a served path is a video's still image: its `.thumb.jpg` thumbnail
/// or its `.poster.jpg` hero poster.
pub fn is_video_still(path: &str) -> bool {
    path.ends_with(".thumb.jpg") || path.ends_with(POSTER_SUFFIX)
}

/// The video stem a hero poster is named after: `a/clip.poster.jpg` is
/// `Some("a/clip")`. The registry strips one extension and would otherwise key
/// the file as `a/clip.poster`.
pub fn poster_stem(url: &str) -> Option<&str> {
    url.strip_suffix(POSTER_SUFFIX).filter(|stem| !stem.is_empty())
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
/// own doc enumerates the ladder-DERIVATION sites). Changing the predicate
/// changes every gate site in lockstep.
///
/// GATE SITES — the six `is_ladder_source_ext(` production callers, each deciding png/jpg/jpeg/webp participation:
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
/// NOTE: `encode_rungs` is NOT a gate site — it is a
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
/// assert!(is_ladder_source_ext("webp")); // webp participates in the ladder
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

/// Long-edge cap (px) of a deployed raster up to 2:1. Single source of truth —
/// the encode pipeline's `ImageCompressionConfig::default()` reads this
/// constant, and [`deployed_long_edge`] applies it. 2400 covers retina
/// displays. A more elongated image may deploy longer: see
/// [`deployed_long_edge`].
pub const DEPLOY_MAX_EDGE: u32 = 2400;

/// WebP's hard per-dimension limit: libwebp refuses to encode anything
/// larger, so no deployed webp may exceed it on either edge.
pub const WEBP_MAX_DIMENSION: u32 = 16383;

/// The long edge a `natural_w`×`natural_h` source deploys at, under a
/// long-edge cap of `max_edge` — the one owner of the deploy resize rule.
///
/// The long edge is capped at `max_edge`, but the short edge is never
/// reduced below `max_edge / 2`, the source is never upscaled, and neither
/// edge exceeds [`WEBP_MAX_DIMENSION`]. As a scale:
/// `min(1, max(max_edge / long, (max_edge / 2) / short))`, then the WebP clamp.
///
/// A long-edge cap alone turned elongated images into strips: a 21969×950
/// handscroll deployed at 2400×104, useless at any column height (found on a
/// real vertical-writing site, 2026-09-23); a 1356×5161 hanging scroll at
/// 631×2400, soft in a horizontal column. `max_edge / 2` is the largest floor
/// that changes nothing up to 2:1 — at exactly 2:1 the two terms are equal —
/// so every such image deploys byte-identically to the long-edge-only rule.
/// `ceil` keeps the encoder's rounded short edge at or above the floor.
pub fn deployed_long_edge(natural_w: u32, natural_h: u32, max_edge: u32) -> u32 {
    let long = natural_w.max(natural_h);
    let short = natural_w.min(natural_h).max(1);
    let floor = max_edge / 2;
    let target = if long <= max_edge || short <= floor {
        long
    } else {
        let floored = (u64::from(long) * u64::from(floor)).div_ceil(u64::from(short));
        max_edge.max(floored as u32)
    };
    target.min(long).min(WEBP_MAX_DIMENSION)
}

/// The responsive ladder: rung widths generated below the deployed base.
/// Must be strictly ascending — the `take_while` in [`ladder_rungs`] depends on it.
/// This is the canonical ladder definition and part of the public `asset_paths`
/// vocabulary (named in the moss-core CHANGELOG); consumers should prefer
/// [`ladder_rungs`] / [`deployed_width`] over indexing `LADDER` directly, so
/// ladder policy stays in one place.
pub const LADDER: [u32; 2] = [800, 1600];

/// The most elongated (long edge ÷ short edge) a deployed base may be
/// before [`ladder_rungs`] gives up on offering any rung at all. A rung is
/// a uniform scale of the base, so it is exactly as elongated — past this
/// ratio an 800-wide rung is thin enough (under 3 digits tall) that it
/// stops conveying anything a browser would pick it for. The boundary is
/// INCLUSIVE: a base at exactly this ratio still keeps its ladder
/// (2400×240, exactly 10:1), and the ladder only empties once the ratio
/// exceeds it (2400×239, ~10.04:1) — [`ladder_rungs`]'s check is a strict
/// `<`, not `<=`. 10 sits well clear of every source this crate's own
/// tests still expect a ladder for — the widest is 2400×316 (7.6:1) — and
/// well under the narrowest known pathological source, a 2200×100 fixture
/// (22:1) and real handscrolls (23–27:1); see [`ladder_rungs`]'s doc for
/// the worked cases.
pub const MAX_LADDER_ASPECT: u32 = 10;

/// The most elongated (long edge ÷ short edge) a source may be before it
/// still reads as an ordinary wide or tall photo. Past this,
/// `render::image`'s default in-column presentation — fit width to the
/// text measure horizontally, fit height to the plate vertically — has
/// already squashed it to an unreadable sliver, well before
/// [`MAX_LADDER_ASPECT`] even makes its responsive ladder pointless: a
/// presentation concern, checked first, not a downstream consequence of
/// the rung-generation cutoff. Deliberately far below `MAX_LADDER_ASPECT`
/// (10) for that reason — by the time a source's ladder disappears, its
/// in-column presentation has been broken for a while.
///
/// A handscroll (手卷) runs 4.6:1 to 30:1 or more, while a wide logo, banner
/// or panorama crop (2:1 to 3.8:1) reads fine at the column width, so a WIDE
/// source is a scroll only past 4:1. A hanging scroll (立軸) is tall, and a tall
/// source past 3:1 is already taller than a screen at the column width, so a
/// TALL source keeps the 3:1 threshold.
pub const SCROLL_SHAPE_ASPECT: u32 = 4;
/// The threshold for a tall source; see [`SCROLL_SHAPE_ASPECT`].
pub const SCROLL_SHAPE_ASPECT_TALL: u32 = 3;

/// Whether a source's aspect ratio is extreme enough to need the "scroll"
/// default presentation (`render::image`'s `data-aspect="scroll"`, CSS in
/// site.css / vertical.css) instead of ordinary in-column sizing — the
/// width past [`SCROLL_SHAPE_ASPECT`] times the height for a wide source (a
/// handscroll), or the height past [`SCROLL_SHAPE_ASPECT_TALL`] times the
/// width for a tall one (a hanging scroll). The boundary is INCLUSIVE on the
/// ordinary side, matching [`MAX_LADDER_ASPECT`]'s own convention: exactly
/// 4:1 wide or 3:1 tall is still ordinary.
pub fn is_scroll_shape(natural_w: u32, natural_h: u32) -> bool {
    if natural_w >= natural_h {
        natural_w > natural_h.max(1).saturating_mul(SCROLL_SHAPE_ASPECT)
    } else {
        natural_h > natural_w.max(1).saturating_mul(SCROLL_SHAPE_ASPECT_TALL)
    }
}

/// Width the deployed base variant actually has after the encoder's
/// aspect-preserving resize to [`deployed_long_edge`] (`img.resize(bound,
/// bound, Lanczos3)` in build/media/image.rs). BOTH dimensions shrink by the
/// same ratio — for portraits the deployed width is therefore SMALLER than
/// the long edge: a 3024×4032 portrait deploys at 1800×2400, so its base
/// width is 1800.
///
/// Integer math (u64 multiply, truncating divide, floor at 1) mirrors the
/// image crate's `resize_dimensions` as closely as practical. srcset width
/// descriptors are browser HINTS: ±1px rounding drift vs the encoder's
/// float `.round()` is acceptable — a Task-5 cross-check test against real
/// encode output pins gross agreement.
pub fn deployed_width(natural_w: u32, natural_h: u32) -> u32 {
    let long_edge = natural_w.max(natural_h);
    let bound = deployed_long_edge(natural_w, natural_h, DEPLOY_MAX_EDGE);
    deployed_scaled_dim(natural_w, bound, long_edge)
}

/// Scale `dim` — an edge of a source whose long edge is `long_edge` — by
/// the same `bound / long_edge` ratio the deploy resize applies to the
/// long edge itself (`bound` is [`deployed_long_edge`]'s return value).
/// Shared by [`deployed_width`] (scaling `natural_w` — a no-op read of
/// `bound` for a landscape source, where `natural_w` already IS
/// `long_edge`) and [`ladder_rungs`] (scaling the SHORT edge by that same
/// ratio, to get the deployed base's actual short edge and check it
/// against [`MAX_LADDER_ASPECT`]) — one ratio-scale primitive instead of
/// two copies with the dimensions swapped.
fn deployed_scaled_dim(dim: u32, bound: u32, long_edge: u32) -> u32 {
    if bound >= long_edge {
        return dim;
    }
    ((u64::from(dim) * u64::from(bound) / u64::from(long_edge)) as u32).max(1)
}

/// Which ladder rungs exist for a source of `natural_w`×`natural_h` px.
///
/// DETERMINISTIC-AGREEMENT CONTRACT — the ladder-DERIVATION census: the
/// `ladder_rungs(` production callers. Keep this
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
/// 3. **encode** — `encode_rungs` (build/media/rungs.rs) derives the same ladder from oriented dims on the
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
/// outcomes, cache state) — that is the parallel-oracle bug class.
///
/// EXIF-ORIENTATION AGREEMENT (canonical; the pipeline sites back-reference
/// here). The "same scan-derived dims on every site" premise holds
/// UNCONDITIONALLY — including an EXIF-oriented (orientation 5-8, i.e. a 90°/270°
/// rotation) png/webp/jpeg. Scan's `extract_image_dimensions` (build/scan/
/// scan.rs) swaps `w`×`h` for orientation 5-8 on EVERY ladder source, reading
/// orientation through the SAME `read_exif_orientation` the encode side's
/// `decode_oriented` uses — so the dims scan feeds emission/registration/sweep/
/// heal equal encode's oriented dims byte-for-byte and the two ladders cannot
/// diverge. Gating that read to jpeg would let an EXIF-rotated png/webp store
/// the UNswapped header dims and strand an emitted-but-never-encoded rung →
/// publish-404. Do NOT re-gate scan's swap to jpeg-only, and do NOT "fix" any future
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
/// ANIMATED-FLAG AGREEMENT. Only ONE of the five sites
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
/// chosen-`<source>` 404. Do NOT copy that guard.
///
/// A rung is a WIDTH contract (`asset_paths::to_webp_rung`'s `wN`, checked
/// exactly by `validate_webp_output`), so its OTHER edge is always
/// `short_edge * rung / long_edge` — the source's own aspect ratio,
/// unchanged at every rung width because every rung is a uniform scale of
/// the same source, on WHICHEVER side (width for landscape, height for
/// portrait) that ratio makes thin. That is also true of the deployed
/// BASE: it is the same uniform scale (`deployed_scaled_dim`, driven by
/// [`deployed_long_edge`]'s `bound`), so a rung's thin edge is thin in
/// exactly the same proportion the base's own is — never worse, never
/// better. Most of the time that's fine: an ordinary photo or hero far
/// past 2:1 (2400×316, or the 6000×1500 fixture
/// `vertical_sizes_render_gate_golden_matches_the_synthesizer` pins) keeps
/// its full, ordinary ladder, landscape or portrait alike.
///
/// The ladder empties when that shared ratio is extreme enough that the
/// base itself is more than [`MAX_LADDER_ASPECT`] times as long as it is
/// short — checked as the deployed short edge against the deployed long
/// edge (`bound`), the same "whichever side is short" comparison for
/// either orientation, not a landscape-only special case. A 38415×1400
/// handscroll's long edge clamps at [`WEBP_MAX_DIMENSION`], short edge
/// landing at ~597 against a ~16383 long edge (27:1) — its `w800` rung
/// would be 800×29. A 1300×23660 hanging scroll clamps the same way,
/// short edge ~900 against ~16383 (18:1) — its only rung, `w800`, is
/// itself narrower than that already-compromised 900px base, making a bad
/// ratio worse rather than offering a genuinely smaller variant. Neither
/// needs `WEBP_MAX_DIMENSION` to clamp, though: a 2200×100 source (22:1)
/// is never resized at all — deploying at its own native size — and is
/// still too elongated for an 800-wide rung to mean anything (800×36).
/// Once the ratio is this extreme, no rung width helps: widening one would
/// break the `wN` contract (breaking `validate_webp_output`) and cropping
/// would lose content, so the ladder is empty and only the base is
/// offered — no upscale.
pub fn ladder_rungs(natural_w: u32, natural_h: u32, is_animated: bool) -> &'static [u32] {
    if is_animated {
        return &[];
    }
    let long_edge = natural_w.max(natural_h);
    let short_edge = natural_w.min(natural_h);
    let bound = deployed_long_edge(natural_w, natural_h, DEPLOY_MAX_EDGE);
    let deployed_short = deployed_scaled_dim(short_edge, bound, long_edge);
    if u64::from(deployed_short) * u64::from(MAX_LADDER_ASPECT) < u64::from(bound) {
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

/// One rung of the video delivery ladder: a bounding box, a frame rate and the
/// most video bitrate a picture that size may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoRung {
    /// The box the picture must fit inside, not the frame it is encoded at: a
    /// source of another shape is scaled down to fit it, aspect preserved
    /// ([`VideoRung::fitted_frame`]).
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// A ceiling, not a target: an encode aims at a picture quality and this
    /// bounds what it may spend getting there. A file can weigh less than this
    /// rate times its duration, never more than a short buffer's worth over.
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
    /// The most a viewer must sustain for this rung, video plus audio, in
    /// kbps. Container overhead is a few percent on top and is not counted.
    pub const fn total_kbps(&self) -> u32 {
        self.video_kbps + self.audio_kbps
    }

    /// The frame (width, height) a source of `display` size is encoded at for
    /// this rung: the display size scaled down to fit the box, aspect kept,
    /// both sides even, never enlarged (the one exception is the 2-pixel floor:
    /// a display under 2 px on a side still yields a 2-pixel side, since an
    /// encoded frame cannot be smaller). Square pixels are the caller's job
    /// (`setsar=1`), since the numbers here are already display pixels.
    ///
    /// A 1600x678 source in the 1280x720 box is 1280x542; a 1080x1920 portrait
    /// source is 406x720. Computed here, once, so the ffmpeg filter graph, the
    /// progressive encode and the tests all read the same numbers instead of
    /// each leaving the arithmetic to a filter expression.
    pub fn fitted_frame(&self, display: (u32, u32)) -> (u32, u32) {
        let (dw, dh) = (display.0.max(1) as f64, display.1.max(1) as f64);
        let scale = (self.width as f64 / dw).min(self.height as f64 / dh).min(1.0);
        // Round to the nearest even number when shrinking; when the source
        // already fits, round down so an odd side is trimmed, not enlarged.
        let even = |v: f64, cap: u32| {
            let half = if scale < 1.0 { (v / 2.0).round() } else { (v / 2.0).floor() };
            ((half as u32) * 2).clamp(2, (cap & !1).max(2))
        };
        (even(dw * scale, self.width), even(dh * scale, self.height))
    }
}

/// A source's display size: its coded width stretched by its sample aspect
/// ratio, over its height. An unknown or non-positive ratio counts as square
/// pixels. A source that declares 64:45 pixels at 1350x1080 is a 1920x1080
/// picture, and that — not the coded 1350 — is what a rung's box is compared
/// against.
pub fn display_size(width: u32, height: u32, sar: f64) -> (u32, u32) {
    let sar = if sar.is_finite() && sar > 0.0 { sar } else { 1.0 };
    ((width as f64 * sar).round() as u32, height)
}

/// The least bits per pixel per frame that still yields a watchable picture.
///
/// A bitrate floor has to be stated per pixel, not in absolute kbps. The
/// absolute floor this replaced (250 kbps) permitted 0.0090 bits/pixel/frame at
/// 1280x720x30 while rejecting the 0.0260 of a 45 kbps 320x180x30 rung — nearly
/// three times leaner per pixel than the rung it turned away.
///
/// 0.045 is measured, not chosen. Encoding the same source at 45 kbps and
/// 320x180: at 30 fps — 0.0260 — faces and wall texture dissolve; at 15 fps — 0.0521 —
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
/// The 1920x1080 rung is the ceiling: a full-screen background video on a
/// high-density display needs about 1920 px across, and a ladder that stopped
/// at 720p served it soft. At 4000 kbps it clears the quality floor at 0.064
/// bits per pixel per frame, twice the rung below it.
///
/// The **top rung is also the delivery ceiling**. There is deliberately no
/// second `max_video_bitrate_kbps` knob beside this table: two independent
/// statements of how much the best version may spend would overlap. (How good
/// a picture each encode aims for under these rates is the encoder's setting,
/// in moss-build.) The hosting-budget knobs that narrow a source's ladder
/// (`max_size_mb`, `hls_max_file_mb`) pick a LOWER rung off this table, and a
/// source's own size between two rows gets a ceiling between theirs
/// ([`video_top_rung`]). Nothing this crate builds ever asks libx264 for more
/// than this table's top rung.
pub const VIDEO_LADDER: [VideoRung; 7] = [
    VideoRung { width: 320, height: 180, fps: 15, video_kbps: 45, audio_kbps: 32, audio_channels: 1 },
    VideoRung { width: 416, height: 234, fps: 30, video_kbps: 145, audio_kbps: 32, audio_channels: 1 },
    VideoRung { width: 640, height: 360, fps: 30, video_kbps: 365, audio_kbps: 192, audio_channels: 2 },
    VideoRung { width: 768, height: 432, fps: 30, video_kbps: 730, audio_kbps: 192, audio_channels: 2 },
    VideoRung { width: 960, height: 540, fps: 30, video_kbps: 1100, audio_kbps: 192, audio_channels: 2 },
    VideoRung { width: 1280, height: 720, fps: 30, video_kbps: 2000, audio_kbps: 192, audio_channels: 2 },
    VideoRung { width: 1920, height: 1080, fps: 30, video_kbps: 4000, audio_kbps: 192, audio_channels: 2 },
];

/// The sizes a source of this display size ([`display_size`]) is delivered
/// at, smallest first: the table's boxes it is meaningfully larger than, then
/// its own picture ([`video_top_rung`]).
///
/// A box is offered only when the frame the source gets in it has at most
/// three quarters of the top frame's pixels. A box nearer the source's own
/// size than that buys a viewer almost nothing: a 1280x810 recording would get
/// a 1138x720 rung under its own, the same picture for nine tenths of the
/// rate. The rows are taken in order and the first one too close stops the
/// take, so the rungs below the top are always the table's first rows: rung
/// `i` below the top is row `i`, which is what lets a ladder's files be named
/// by position. (The table's widths and heights both ascend, so the frames
/// do too.)
///
/// Never empty: the source's own picture is always there. One rung means
/// there is nothing to switch between, which callers read as no ladder.
pub fn video_ladder_rungs(display: (u32, u32)) -> Vec<VideoRung> {
    let top = video_top_rung(display);
    let pixels = |r: &VideoRung| {
        let (w, h) = r.fitted_frame(display);
        u64::from(w) * u64::from(h)
    };
    let top_pixels = pixels(&top);
    let mut rungs: Vec<VideoRung> = VIDEO_LADDER
        .iter()
        .take_while(|r| 4 * pixels(r) <= 3 * top_pixels)
        .copied()
        .collect();
    rungs.push(top);
    rungs
}

/// The most the single file an HLS rung writes can weigh, in bytes, for a
/// track capped at `kbps` over `duration_secs` — `-hls_flags single_file`
/// makes a rung's video, and each audio group, exactly one file apiece, so
/// that file is bounded by its rate ceiling times the whole video's duration.
///
/// An upper bound, not a prediction: a rung aims at a picture quality and a
/// simple picture comes out far under its ceiling. The budget still has to
/// assume the ceiling, because how far under is not known before the encode.
/// The 5% margin is measured on footage that does run at the ceiling: the two
/// production files this budget exists to keep under a cap were 221,843,463
/// bytes (1280x720, 2000 kbps) and 122,579,638 bytes (960x540, 1100 kbps) for
/// a 903.47 s source, 98–99% of the ceiling in both cases.
fn hls_file_bytes(kbps: u32, duration_secs: f64) -> f64 {
    (kbps as f64) * 1000.0 / 8.0 * duration_secs * 1.05
}

/// The rungs worth encoding for a source of `display` size and
/// `duration_secs` seconds, or a single rung (which callers read as no
/// ladder).
///
/// [`video_ladder_rungs`] is narrowed from the top by the per-file byte
/// budget: a rung whose file could reach `max_file_bytes` at its rate ceiling
/// ([`hls_file_bytes`]) is dropped, with every rung above it. The check
/// stops at the first rung it fails, sound because a rung higher up the table
/// never has a lower ceiling, in either `video_kbps` or `audio_kbps`
/// ([`video_ladder_is_strictly_ascending`],
/// [`video_ladder_audio_kbps_is_non_decreasing`]).
///
/// Then the ladder must hold `progressive`, the rung of the single web file it
/// would be played instead of (one of [`video_ladder_rungs`], or the source's
/// own top when the original is kept). A player that can use the ladder never
/// fetches that file, so a ladder without it would show every viewer a
/// smaller picture than the file. A ten-minute 1280x810
/// screen recording kept as its own file loses its full-size rung to the
/// budget; its ladder would stop at 854x540, so it gets none. A long film
/// whose web file the size budget already pulled down to 640x360 keeps a
/// ladder that reaches past it.
///
/// Never empty: an over-budget bottom rung still ships, because a file too
/// big beats a video nobody can watch.
pub fn video_ladder_rungs_within(
    display: (u32, u32),
    duration_secs: f64,
    max_file_bytes: u64,
    progressive: VideoRung,
) -> Vec<VideoRung> {
    let rungs = video_ladder_rungs(display);
    let fits = |kbps: u32| hls_file_bytes(kbps, duration_secs) <= max_file_bytes as f64;
    let n = rungs
        .iter()
        .take_while(|r| fits(r.video_kbps) && fits(r.audio_kbps))
        .count()
        .max(1);
    if n > 1 && !rungs[..n].contains(&progressive) {
        return rungs[..1].to_vec();
    }
    rungs[..n].to_vec()
}

/// Any rung list flattened into a stable string — one rung per comma-joined
/// segment, each carrying everything about it that shapes bytes on the wire
/// (resolution, frame rate, video and audio bitrate, audio channels).
///
/// Generalizes what [`video_ladder_fingerprint`] used to compute only for the
/// static table, so a per-video EFFECTIVE ladder (topped by the source's own
/// size, budget-narrowed — not a slice of [`VIDEO_LADDER`]) can be
/// fingerprinted the exact same way for a transform-cache key.
pub fn rung_list_fingerprint(rungs: &[VideoRung]) -> String {
    rungs
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

/// The static ladder flattened into a transform-cache key, for the
/// progressive MP4 path — the only cache key this table's own edits still
/// need to invalidate directly; the HLS ladder's key names its EFFECTIVE
/// rungs instead (see `hls::ladder_params` in moss-build), so a table edit
/// invalidates it only when it actually changes what a video encodes at.
///
/// Delivery policy is a const table, not configuration, so nothing in an
/// encoder's config struct changes when a rung is edited — and every site would
/// go on serving renditions encoded under the old table, invisibly. Any edit to
/// any rung changes this string, which is what an edit to it means.
pub fn video_ladder_fingerprint() -> String {
    rung_list_fingerprint(&VIDEO_LADDER)
}

/// The best size a source is delivered at: its own picture, scaled down only
/// if it is larger than the table's top box, and never enlarged. The
/// progressive MP4 is encoded to it and the ladder ends on it.
///
/// The rung is the smallest table box that holds that picture, so fitting the
/// source into it ([`VideoRung::fitted_frame`]) leaves the picture its own
/// size. A picture that fills the box on a side takes the box's rate ceiling.
/// One strictly between two boxes takes the larger box's rate scaled to its
/// own pixel count, and never less than the smaller box's. A 1280x810 screen
/// recording is delivered at 1280x810 under a 2000 kbps ceiling, where the
/// 1280x720 box below made it 1138x720 and resampled every line of text.
pub fn video_top_rung(display: (u32, u32)) -> VideoRung {
    let ceiling = VIDEO_LADDER[VIDEO_LADDER.len() - 1];
    let (width, height) = ceiling.fitted_frame(display);
    // The ceiling row holds every frame fitted into it, so this always finds one.
    let i = VIDEO_LADDER
        .iter()
        .position(|r| width <= r.width && height <= r.height)
        .unwrap_or(VIDEO_LADDER.len() - 1);
    let holder = VIDEO_LADDER[i];
    if i == 0 || width == holder.width || height == holder.height {
        return holder;
    }
    let pixels = u64::from(width) * u64::from(height);
    let box_pixels = u64::from(holder.width) * u64::from(holder.height);
    let scaled = (u64::from(holder.video_kbps) * pixels / box_pixels) as u32;
    VideoRung { video_kbps: scaled.max(VIDEO_LADDER[i - 1].video_kbps), ..holder }
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
    /// Every group, so a reader of the naming scheme need not retype them.
    pub const ALL: [AudioGroup; 2] = [AudioGroup::Lean, AudioGroup::Clean];

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
/// ladder is silently never offered, leaving a ladder's encoded files on every
/// build that no page ever references.
pub fn hls_master_stem(url: &str) -> Option<&str> {
    hls_dir_stem(url.strip_suffix(HLS_MASTER_NAME)?.strip_suffix('/')?)
}

/// The source stem a ladder directory is named after: `a/clip.hls` →
/// `Some("a/clip")`. The directory is a bundle named after a file, so a path
/// normaliser must leave its spelling alone; see [`to_hls_dir`].
pub fn hls_dir_stem(dir: &str) -> Option<&str> {
    dir.strip_suffix(".hls").filter(|stem| !stem.is_empty())
}

/// The names inside one ladder's directory, master first — what the ENCODER
/// writes for `rungs`. The image ladder re-derives its membership at five
/// call sites and this module documents that as a fragile contract; video's
/// encoder does not repeat it — `produce_ladder`/`link_members` (moss-build)
/// write exactly this list, from here.
///
/// This is not how a reader answers "what's in this ladder right now,
/// today, on disk" — a ladder already sitting in a site's build output may
/// have been produced under an older or newer table than the one running
/// today, and this function only ever knows the CURRENT table. moss-build's
/// `hls::ladder_members_from_master` answers that question instead, by
/// reading the ladder's own `master.m3u8` back: the master playlist is
/// authoritative for whatever it actually references, independent of which
/// table wrote it.
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

/// The video stem a ladder file belongs to: `clips/demo.hls/v0.m4s` is
/// `Some("clips/demo")`. `None` unless the path is exactly one of the file names
/// [`hls_members`] can produce (master playlist, `v<N>` rung playlist or
/// segment file, an audio group's playlist or segment file) directly inside a
/// `<stem>.hls` directory — the last `.hls/` in the path, so a folder an author
/// happens to name `x.hls` or a stray file beside the ladder is not one.
pub fn hls_ladder_file_stem(path: &str) -> Option<&str> {
    let (stem, name) = path.rsplit_once(".hls/")?;
    let is_member = name == HLS_MASTER_NAME
        || AudioGroup::ALL.iter().any(|g| {
            name.strip_prefix(g.as_str())
                .is_some_and(|ext| ext == ".m3u8" || ext == ".m4s")
        })
        || name
            .strip_prefix('v')
            .and_then(|rest| rest.strip_suffix(".m3u8").or_else(|| rest.strip_suffix(".m4s")))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    is_member.then_some(stem)
}

/// Every path a video with this stem can be served under (`<stem>.mp4`,
/// `<stem>.webm`, ...): the stem plus each extension [`video_stem`] strips.
pub fn video_served_paths(stem: &str) -> impl Iterator<Item = String> + '_ {
    ALL_VIDEO_SUFFIXES.iter().map(move |suffix| format!("{stem}{suffix}"))
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

    // ── to_poster tests ────────────────────────────────────────────

    #[test]
    fn to_poster_swaps_any_video_extension_and_poster_stem_inverts_it() {
        assert_eq!(to_poster("clip.mov"), "clip.poster.jpg");
        assert_eq!(to_poster("videos/冬日之歌.MP4"), "videos/冬日之歌.poster.jpg");
        assert_eq!(poster_stem("videos/clip.poster.jpg"), Some("videos/clip"));
        assert_eq!(poster_stem("videos/clip.thumb.jpg"), None);
        assert_eq!(poster_stem(".poster.jpg"), None);
    }

    #[test]
    fn a_video_still_is_a_thumbnail_or_a_hero_poster() {
        assert!(is_video_still("v/clip.thumb.jpg") && is_video_still("v/clip.poster.jpg"));
        assert!(!is_video_still("v/clip.jpg") && !is_video_still("v/clip.mp4"));
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
        // The whole point of rendition groups: seven rungs, but audio stored
        // twice. Muxing per rung would store it seven times and degrade it in
        // lockstep with the picture.
        let outs = hls_outputs("clip.mov", &VIDEO_LADDER);
        assert_eq!(outs[0], "clip.hls/master.m3u8", "master comes first");
        assert_eq!(
            outs.len(),
            1 + VIDEO_LADDER.len() * 2 + 2 * 2,
            "master + playlist/segment per rung + playlist/segment per audio group"
        );
        assert_eq!(outs.len(), 19, "7 rungs x2 + 2 renditions x2 + master");
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
        let rungs = video_ladder_rungs((320, 180));
        assert_eq!(rungs.len(), 1);
        let outs = hls_outputs("clip.mov", &rungs);
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

    /// Apple's authoring spec asks for rungs 1.5-2x apart: closer than that, a
    /// player has nothing worth switching between.
    #[test]
    fn video_ladder_consecutive_rungs_are_at_least_one_and_a_half_times_apart() {
        assert!(
            VIDEO_LADDER.windows(2).all(|w| w[1].video_kbps as f64 >= 1.5 * w[0].video_kbps as f64),
            "{VIDEO_LADDER:?}"
        );
    }

    /// `video_ladder_rungs_within`'s `take_while` stops at the first rung that
    /// fails the budget and never looks past it — sound only because a rung
    /// higher up the table never needs FEWER bytes than the one below it.
    #[test]
    fn video_ladder_audio_kbps_is_non_decreasing() {
        assert!(VIDEO_LADDER.windows(2).all(|w| w[0].audio_kbps <= w[1].audio_kbps));
    }

    /// `video_ladder_rungs_within` checks a rung's audio-group file size even
    /// for a silent source, which writes no audio files at all. That is only
    /// ever a harmless extra check, never a wrong answer, because this holds:
    /// the video file is always the pricier of the two at the same duration,
    /// so a rung that clears the video check always clears the audio one too.
    /// If a future rung ever violated this, a silent source could get turned
    /// away by a budget its own ladder would never actually spend.
    #[test]
    fn video_ladder_video_kbps_exceeds_audio_kbps() {
        assert!(VIDEO_LADDER.iter().all(|r| r.video_kbps > r.audio_kbps), "{VIDEO_LADDER:?}");
    }

    /// A source gets the table's rows below its own size and then its own
    /// picture; one smaller than every row still gets its own picture, alone.
    #[test]
    fn video_ladder_is_the_rows_below_the_source_then_the_source_itself() {
        assert_eq!(video_ladder_rungs((1920, 1080)), VIDEO_LADDER);
        assert_eq!(video_ladder_rungs((1280, 720)), VIDEO_LADDER[..VIDEO_LADDER.len() - 1]);
        assert_eq!(video_ladder_rungs((640, 360)), VIDEO_LADDER[..3]);
        assert_eq!(video_ladder_rungs((200, 112)), VIDEO_LADDER[..1]);
        assert_eq!(VIDEO_LADDER[0].fitted_frame((200, 112)), (200, 112), "never enlarged");
    }

    /// The frames (and ceilings) a source of each size is delivered at.
    fn frames(display: (u32, u32)) -> String {
        video_ladder_rungs(display)
            .iter()
            .map(|r| {
                let (w, h) = r.fitted_frame(display);
                format!("{w}x{h}@{}", r.video_kbps)
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_source_between_two_sizes_keeps_its_own_size_and_skips_the_row_too_close_to_it() {
        // 1280x810: the 1280x720 row would be 1138x720, four fifths of the
        // top's pixels, so it is not offered; the top is the recording itself.
        assert_eq!(
            frames((1280, 810)),
            "284x180@45 370x234@145 568x360@365 682x432@730 854x540@1100 1280x810@2000"
        );
        // 1600x678 is far enough above the 1280 row to keep it.
        assert_eq!(
            frames((1600, 678)),
            "320x136@45 416x176@145 640x272@365 768x326@730 960x406@1100 1280x542@2000 1600x678@2092"
        );
        // A 4:3 clip keeps its 640x480 instead of shrinking to 576x432.
        assert_eq!(frames((640, 480)), "240x180@45 312x234@145 480x360@365 640x480@730");
    }

    /// Whatever the source's size, frames grow up the ladder, no rung is
    /// allowed more than the one above it, and the top is the picture the
    /// ceiling allows.
    #[test]
    fn every_ladder_grows_to_the_picture_the_ceiling_allows() {
        for w in (120..=4000).step_by(77) {
            for h in (90..=2400).step_by(53) {
                let rungs = video_ladder_rungs((w, h));
                let top = &rungs[rungs.len() - 1..];
                let pixels = |r: &VideoRung| {
                    let (fw, fh) = r.fitted_frame((w, h));
                    u64::from(fw) * u64::from(fh)
                };
                for pair in rungs.windows(2) {
                    assert!(pixels(&pair[0]) < pixels(&pair[1]), "{w}x{h}: {rungs:?}");
                    assert!(pair[0].video_kbps <= pair[1].video_kbps, "{w}x{h}: {rungs:?}");
                }
                let ceiling = VIDEO_LADDER[VIDEO_LADDER.len() - 1];
                assert_eq!(
                    top[0].fitted_frame((w, h)),
                    ceiling.fitted_frame((w, h)),
                    "{w}x{h}: the top rung is the picture the ceiling allows, no smaller"
                );
            }
        }
    }

    // ── video_ladder_rungs_within ──────────────────────────────────

    const MIB: u64 = 1024 * 1024;

    /// The per-file budget is priced at each rung's ceiling. The real case it
    /// exists for: a 903.47 s, 1280x720 source's top two rungs' single files
    /// measured 221,843,463 and 122,579,638 bytes, 515 MB for the whole ladder
    /// against a hosting cap meant to bound one upload. A 150 MiB cap drops
    /// the top rung, a 100 MiB cap the next one too. The web file this ladder
    /// stands in for (here a 640x360 one, as a long video's is) is smaller
    /// than either, so both still are ladders.
    #[test]
    fn video_ladder_rungs_within_drops_the_rungs_whose_files_the_budget_cannot_hold() {
        let duration = 903.47;
        let at_150_mib = video_ladder_rungs_within((1280, 720), duration, 150 * MIB, VIDEO_LADDER[2]);
        assert_eq!(at_150_mib, VIDEO_LADDER[..5]);
        let at_100_mib = video_ladder_rungs_within((1280, 720), duration, 100 * MIB, VIDEO_LADDER[2]);
        assert_eq!(at_100_mib, VIDEO_LADDER[..4]);
    }

    #[test]
    fn video_ladder_rungs_within_keeps_the_full_ladder_for_a_short_clip() {
        let got = video_ladder_rungs_within((1920, 1080), 10.0, 150 * MIB, VIDEO_LADDER[6]);
        assert_eq!(got, video_ladder_rungs((1920, 1080)), "10s at any real bitrate is nowhere near 150 MiB");
        assert_eq!(got.len(), VIDEO_LADDER.len());
    }

    /// A player that can use the ladder never fetches the web file, so a
    /// ladder whose best frame is smaller than that file is no ladder. A
    /// ten-minute 1280x810 screen recording kept as its own file loses its
    /// full-size rung to the budget and would stop at 854x540.
    #[test]
    fn a_ladder_that_cannot_reach_the_web_files_size_is_no_ladder() {
        let own = video_top_rung((1280, 810));
        let rungs = video_ladder_rungs_within((1280, 810), 600.0, 150 * MIB, own);
        assert_eq!(rungs.len(), 1, "{rungs:?}");
        // The same recording at a minute keeps its full-size top.
        let short = video_ladder_rungs_within((1280, 810), 60.0, 150 * MIB, own);
        assert_eq!(short.last().unwrap().fitted_frame((1280, 810)), (1280, 810));
        // An odd side is trimmed to even in every rung, the top included, and
        // the ladder still holds the web file.
        let odd = video_ladder_rungs_within((1281, 721), 60.0, 150 * MIB, video_top_rung((1281, 721)));
        assert_eq!(odd, VIDEO_LADDER[..6]);
    }

    /// However tight the budget, the bottom rung always ships: an over-budget
    /// file that plays beats a video nobody can watch.
    #[test]
    fn video_ladder_rungs_within_never_returns_empty_for_a_very_long_video() {
        let got = video_ladder_rungs_within((1280, 720), 20_000.0, 1, VIDEO_LADDER[0]);
        assert_eq!(got, VIDEO_LADDER[..1]);
    }

    /// The per-rung check is two conditions, not one: a rung's audio group is
    /// its own single file (`alo.m4s`/`ahi.m4s`) and can exceed the budget on
    /// its own. Today's `VIDEO_LADDER` never exercises this side in practice —
    /// every real rung's `video_kbps` exceeds its own `audio_kbps`, so the
    /// video half is always at least as tight as the audio half for the
    /// current table — so this pins the estimator's own "and" directly rather
    /// than through a `VIDEO_LADDER` row, guarding the mechanism for a future
    /// table edit that narrows that margin.
    #[test]
    fn hls_file_bytes_audio_can_be_pricier_than_video_at_the_same_duration() {
        let duration = 1000.0;
        let cheap_video_bytes = hls_file_bytes(100, duration);
        let costly_audio_bytes = hls_file_bytes(192, duration);
        assert!(cheap_video_bytes < costly_audio_bytes);
        let budget = ((cheap_video_bytes + costly_audio_bytes) / 2.0) as u64;
        assert!(cheap_video_bytes <= budget as f64, "the cheaper file fits");
        assert!(costly_audio_bytes > budget as f64, "the pricier one alone exceeds the same budget");
    }

    #[test]
    fn hls_ladder_file_stem_accepts_only_the_ladders_own_file_names() {
        let rungs = &VIDEO_LADDER[..];
        for name in hls_members(rungs) {
            assert_eq!(hls_ladder_file_stem(&format!("clips/demo.hls/{name}")), Some("clips/demo"), "{name}");
        }
        for path in [
            "clips/demo.hls/notes.html",
            "clips/demo.hls/sub/v0.m4s",
            "clips/demo.hls/v.m4s",
            "clips/demo.hls/vx.m3u8",
            "clips/demo.hls/ahi.txt",
            "clips/demo/master.m3u8",
        ] {
            assert_eq!(hls_ladder_file_stem(path), None, "{path}");
        }
        assert_eq!(hls_ladder_file_stem("a.hls/b.hls/master.m3u8"), Some("a.hls/b"));
    }

    #[test]
    fn the_top_rung_is_the_sources_own_picture_under_the_ceiling_for_its_size() {
        let top = |d| {
            let r = video_top_rung(d);
            let (w, h) = r.fitted_frame(d);
            (w, h, r.fps, r.video_kbps)
        };
        // Larger than the table: scaled down to its top box.
        assert_eq!(top((3840, 2160)), (1920, 1080, 30, 4000));
        // Exactly a row, or one odd pixel over it: that row.
        assert_eq!(video_top_rung((1280, 720)), VIDEO_LADDER[5]);
        assert_eq!(video_top_rung((1281, 721)), VIDEO_LADDER[5]);
        // Filling a box on one side takes the box's ceiling.
        assert_eq!(top((1708, 1080)), (1708, 1080, 30, 4000));
        assert_eq!(top((1080, 1920)), (608, 1080, 30, 4000));
        // Strictly between two boxes: its own size, at the larger box's rate
        // for its pixel count and never under the smaller box's.
        assert_eq!(top((1280, 810)), (1280, 810, 30, 2000));
        assert_eq!(top((1600, 900)), (1600, 900, 30, 2777));
        assert_eq!(top((640, 480)), (640, 480, 30, 730));
        // Smaller than every box: itself, on the bottom row's terms.
        assert_eq!(top((100, 56)), (100, 56, 15, 45));
    }

    const BOX_720: VideoRung = VIDEO_LADDER[5];
    const BOX_360: VideoRung = VIDEO_LADDER[2];

    #[test]
    fn a_rung_is_a_box_the_sources_own_shape_is_fitted_into() {
        // Wide: the width binds, the height follows the source, not the box.
        assert_eq!(BOX_720.fitted_frame((1600, 678)), (1280, 542));
        // Portrait: the height binds.
        let (w, h) = BOX_720.fitted_frame((1080, 1920));
        assert_eq!(h, 720);
        assert!(w == 404 || w == 406, "{w}");
        // A declared pixel aspect ratio is part of the shape.
        assert_eq!(BOX_720.fitted_frame(display_size(1350, 1080, 64.0 / 45.0)), (1280, 720));
        // Unknown ratio (0 or negative) means square pixels.
        assert_eq!(display_size(640, 480, 0.0), (640, 480));
        assert_eq!(BOX_360.fitted_frame(display_size(640, 480, 0.0)), (480, 360));
    }

    #[test]
    fn a_fitted_frame_is_even_and_never_larger_than_the_source_or_the_box() {
        assert_eq!(BOX_720.fitted_frame((640, 360)), (640, 360), "never upscaled");
        assert_eq!(BOX_720.fitted_frame((641, 361)), (640, 360), "an odd side is trimmed, not enlarged");
        for src in [(1601, 677), (1919, 1081), (333, 1999), (4000, 100)] {
            let (w, h) = BOX_720.fitted_frame(src);
            assert!(w % 2 == 0 && h % 2 == 0 && w >= 2 && h >= 2, "{src:?} -> {w}x{h}");
            assert!(w <= BOX_720.width && h <= BOX_720.height, "{src:?} -> {w}x{h}");
        }
    }

    #[test]
    fn a_long_1080p_source_loses_the_1080_rung_to_the_per_file_budget() {
        // 4000 kbps for 40 minutes is ~1.2 GB; under the default 150 MiB
        // per-file cap that rung (and only the rungs the budget cannot hold)
        // goes, while a 2-minute clip keeps all seven.
        let cap = 150 * MIB;
        let long = video_ladder_rungs_within((1920, 1080), 2400.0, cap, VIDEO_LADDER[2]);
        assert!(long.len() < VIDEO_LADDER.len(), "{long:?}");
        assert!(long.iter().all(|r| r.width < 1920));
        let short = video_ladder_rungs_within((1920, 1080), 120.0, cap, VIDEO_LADDER[6]);
        assert_eq!(short.len(), VIDEO_LADDER.len());
    }

    /// The ladder (frame @ ceiling) a source of each shape gets: a 16:9
    /// source, a 3:2 one filling the 1080 box's height, and a wide cinema
    /// crop filling its width.
    #[test]
    fn the_ladder_a_source_gets_follows_its_own_shape() {
        let ladder = |disp: (u32, u32)| -> String {
            video_ladder_rungs_within(disp, 60.0, 150 * MIB, video_top_rung(disp))
                .iter()
                .map(|r| {
                    let (w, h) = r.fitted_frame(disp);
                    format!("{w}x{h}@{}", r.video_kbps)
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(
            ladder((1920, 1080)),
            "320x180@45 416x234@145 640x360@365 768x432@730 960x540@1100 1280x720@2000 1920x1080@4000"
        );
        assert_eq!(
            ladder((1620, 1080)),
            "270x180@45 352x234@145 540x360@365 648x432@730 810x540@1100 1080x720@2000 1620x1080@4000"
        );
        assert_eq!(
            ladder((1920, 812)),
            "320x136@45 416x176@145 640x270@365 768x324@730 960x406@1100 1280x542@2000 1920x812@4000"
        );
    }

    #[test]
    fn a_frame_is_floored_at_two_pixels_the_one_exception_to_never_enlarged() {
        assert_eq!(BOX_720.fitted_frame((1, 1)), (2, 2));
        // A zero side (an old cache record with no height) stays total.
        assert_eq!(BOX_720.fitted_frame((1920, 0)).0, 1280);
    }

    #[test]
    fn a_portrait_source_is_eligible_by_its_height() {
        // 1080 px across is under the 1280 box, but 1920 px tall fills it.
        assert_eq!(video_ladder_rungs((1080, 1920)).len(), VIDEO_LADDER.len());
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
    fn ladder_rungs_extreme_aspect_is_empty() {
        // A real handscroll: WEBP_MAX_DIMENSION clamps the deployed long
        // edge to 16383, short edge landing at ~597 — a ~27:1 deployed
        // ratio, past MAX_LADDER_ASPECT. Its w800 rung would be 800×29.
        assert_eq!(ladder_rungs(38415, 1400, false), &[] as &[u32]);
        // The 21969×950 handscroll `deployed_long_edge`'s own docs cite —
        // WEBP_MAX_DIMENSION clamps it the same way, short edge ~708
        // against a 16383 long edge (~23:1).
        assert_eq!(ladder_rungs(21969, 950, false), &[] as &[u32]);
        // No clamp needed to be this extreme: 2200×100 (22:1) deploys at
        // its own native size — nothing capped, nothing widened — and is
        // still past MAX_LADDER_ASPECT. Its w800 rung would be 800×36.
        assert_eq!(ladder_rungs(2200, 100, false), &[] as &[u32]);
        // The portrait twin: 1300×23660 clamps the same way as the
        // handscrolls above (short edge ~900 against a 16383 long edge,
        // ~18:1) — its only candidate rung, w800, would be narrower than
        // the already-compromised 900px base, the same "worse, not
        // smaller" problem, just on the width axis instead of height.
        assert_eq!(ladder_rungs(1300, 23660, false), &[] as &[u32]);
        // Far past 2:1 is still "ordinary" under MAX_LADDER_ASPECT: 2400×316
        // (7.6:1) and 6000×1500 (4:1, this crate's own hero and
        // vertical-sizes render-gate fixtures) both keep their full ladder.
        assert_eq!(ladder_rungs(2400, 316, false), &[800, 1600][..]);
        assert_eq!(ladder_rungs(6000, 1500, false), &[800, 1600][..]);
        // And an ordinary portrait, however far from square, is unaffected
        // the same way — 1500×3000 (2:1) keeps deriving its ladder from
        // deployed_width alone, same as any other source.
        assert_eq!(ladder_rungs(1500, 3000, false), &[800][..]);
    }

    #[test]
    fn ladder_rungs_max_aspect_boundary_is_inclusive() {
        // Pinned so a future change to MAX_LADDER_ASPECT or the `<`/`<=`
        // comparison can't shift the cutoff silently. Neither is resized
        // (long edge 2400 <= DEPLOY_MAX_EDGE), so the deployed short edge
        // is the natural one, and the check is exactly `ratio > 10`.
        // Exactly 10:1 (2400×240) is still "ordinary" — the boundary is
        // inclusive on the kept side.
        assert_eq!(ladder_rungs(2400, 240, false), &[800, 1600][..]);
        // One px thinner tips the ratio to ~10.04:1 and empties the ladder.
        assert_eq!(ladder_rungs(2400, 239, false), &[] as &[u32]);
    }

    #[test]
    fn is_scroll_shape_boundary_is_inclusive() {
        // Ordinary photos, any orientation, well under 4:1: not a scroll.
        assert!(!is_scroll_shape(1600, 900));
        assert!(!is_scroll_shape(900, 1600));
        // Wide logos and banners between 3:1 and 4:1 fit the column.
        assert!(!is_scroll_shape(800, 214));
        assert!(!is_scroll_shape(2400, 720));
        assert!(!is_scroll_shape(2500, 750));
        // Exactly 4:1 wide is still "ordinary" — inclusive on the kept side;
        // one px past it tips into scroll territory.
        assert!(!is_scroll_shape(3200, 800));
        assert!(is_scroll_shape(3201, 800));
        // A tall source keeps 3:1: a hanging scroll at 800x2579 (3.2:1) is
        // still a scroll, and exactly 3:1 tall is ordinary.
        assert!(is_scroll_shape(800, 2579));
        assert!(!is_scroll_shape(800, 2400));
        assert!(is_scroll_shape(800, 2401));
        // The real fixtures this feature targets: a handscroll (河上花圖,
        // 1600×58, ~27.6:1) and a hanging scroll (portrait, well past 4:1).
        assert!(is_scroll_shape(1600, 58));
        assert!(is_scroll_shape(1300, 23660));
        // Degenerate zero height/width never panics or divides by zero.
        assert!(is_scroll_shape(100, 0));
        assert!(!is_scroll_shape(0, 0));
    }

    #[test]
    fn ladder_rungs_portrait_uses_post_resize_width() {
        // 3024×4032 portrait: the encoder shrinks the longest EDGE to 2400,
        // so the deployed base is 1800 wide — both rungs still below it.
        assert_eq!(ladder_rungs(3024, 4032, false), &[800, 1600][..]);
        // Extreme portrait 500×20000: the WebP limit binds (16383 tall), base
        // width 409 — NO rung is below it, so the ladder must be empty (a
        // w800 rung would be WIDER than the base: ladder inversion).
        assert_eq!(ladder_rungs(500, 20000, false), &[] as &[u32]);
        // 800×3000: short edge under the floor, deployed whole — base width
        // exactly 800, and strict `<` excludes the 800 rung.
        assert_eq!(ladder_rungs(800, 3000, false), &[] as &[u32]);
        // 1179×8000 keeps its 1179 short edge (under the 1200 floor), so the
        // hanging scroll now carries a w800 rung instead of a 353-wide base.
        assert_eq!(ladder_rungs(1179, 8000, false), &[800][..]);
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
        // Past 2:1 the short edge keeps at least max_edge / 2 (below).
        assert_eq!(deployed_width(1179, 8000), 1179);
        assert_eq!(deployed_width(1200, 3600), 1200);
        // Square at the cap: untouched.
        assert_eq!(deployed_width(2400, 2400), 2400);
        // Degenerate sliver never collapses to 0.
        assert_eq!(deployed_width(1, 100_000), 1);
    }

    #[test]
    fn deployed_long_edge_keeps_the_short_edge_of_an_elongated_image() {
        let dims = |w: u32, h: u32| {
            let l = deployed_long_edge(w, h, DEPLOY_MAX_EDGE);
            let long = w.max(h);
            (w as u64 * l as u64 / long as u64, h as u64 * l as u64 / long as u64)
        };
        // A handscroll: short edge under the floor, so no downscale — until
        // WebP's 16383 limit binds.
        assert_eq!(deployed_long_edge(21969, 950, DEPLOY_MAX_EDGE), 16383);
        assert_eq!(dims(21969, 950), (16383, 708));
        // 4:1 with a big short edge: shrunk until the short edge is 1200.
        assert_eq!(dims(6000, 1500), (4800, 1200));
        // A hanging scroll: `ceil` keeps the short edge at the floor.
        assert_eq!(deployed_long_edge(1356, 5161, DEPLOY_MAX_EDGE), 4568);
        assert_eq!(dims(1356, 5161), (1200, 4568));
        // 3:1, short edge under the floor: deployed whole.
        assert_eq!(deployed_long_edge(3000, 1000, DEPLOY_MAX_EDGE), 3000);
        // Never upscaled, and the floor tracks the cap it is given.
        assert_eq!(deployed_long_edge(500, 100, DEPLOY_MAX_EDGE), 500);
        assert_eq!(deployed_long_edge(4000, 1000, 1600), 3200);
    }

    #[test]
    fn deployed_long_edge_is_the_long_edge_cap_up_to_two_to_one() {
        // Byte-identical deploys for every image up to 2:1: the long edge is
        // exactly what the long-edge-only rule gave.
        for w in (1..=8000u32).step_by(53) {
            for h in (1..=8000u32).step_by(59) {
                if w.max(h) > 2 * w.min(h) {
                    continue;
                }
                let old = w.max(h).min(DEPLOY_MAX_EDGE);
                assert_eq!(deployed_long_edge(w, h, DEPLOY_MAX_EDGE), old, "{w}x{h}");
            }
        }
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
