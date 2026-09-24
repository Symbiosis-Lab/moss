//! The deployed raster ORIGINAL — the `<img>` inside `<picture>`.
//!
//! Sibling of `image` (which owns the WebP variant pass) and `rungs` (which
//! owns the responsive ladder), extracted out of `image.rs` to separate the
//! deployed-original path from the encoded-variant paths it used to share.
//!
//! ## The container constraint
//!
//! `render/image.rs::synthesize_inner` emits the ORIGINAL filename verbatim as
//! the `<img src>`: `foo.png` stays `foo.png`, and only the BYTES at that path
//! change. So this module can never re-encode a PNG to JPEG — that would serve
//! JPEG bytes at a `.png` URL, a Content-Type lie that also breaks consumers
//! sniffing by extension. Every byte saved here is saved INSIDE the source's
//! own container: JPEG stays JPEG (quality + resize), PNG stays PNG (resize,
//! plus an 8-bit palette when the image is opaque).
//!
//! ## Why it is worth attacking
//!
//! Measured on the 潮汐 corpus (2026-08-04): a build was 4,156 files / 592.8
//! MB, of which the raster originals were 1,086 files / 328.2 MB — 55% of the
//! bytes, and 327.7 MB of that had a `.webp` sibling that ~96% of installs
//! fetch instead. The fallback was also the build's HIGHEST-resolution asset
//! (2400px vs the ladder's 1600px top rung).

use std::fs;
use std::path::Path;

use super::image::{
    apply_exif_orientation, flatten_alpha_to_white, read_exif_orientation, ImageCompressionConfig,
};
use super::sniff::is_cmyk_jpeg;

/// JPEG quality for the deployed, sized raster original.
///
/// Slightly higher than the WebP quality (80) because JPEG at a given quality
/// number is visually a touch weaker than WebP; 82 keeps the sized fallback on
/// par while still shrinking multi-MB originals by an order of magnitude.
pub(crate) const SIZED_JPEG_QUALITY: u8 = 82;

/// Max edge (px) of the deployed raster ORIGINAL — the `<img>` inside
/// `<picture>`, i.e. the fallback only a non-WebP client ever fetches.
///
/// Deliberately the TOP LADDER RUNG, not `DEPLOY_MAX_EDGE` (2400). The webp
/// `<source>` a modern browser actually picks is generated at every ladder rung
/// plus a 2400px base, so on the web this file is fetched only by the ~4% of
/// installs without WebP — and it is never chosen by a `sizes`/`srcset`
/// negotiation (it carries no width descriptor; `render/image.rs::render_img_tag`
/// takes `width`/`height` from the SOURCE's natural dimensions, so shrinking the
/// deployed pixels changes no HTML). Sizing it at 2400 made the fallback the
/// build's HIGHEST-resolution asset: on the 潮汐 corpus the raster originals
/// were 328 MB of a 593 MB build — 55% of the bytes.
///
/// **It is not only a web fallback.** `infra/newsletter.rs`'s
/// `ImageContext::EmailBody` emits a BARE `<img src>` at this URL — no
/// `<picture>`, no webp `<source>` — and email clients are a much older
/// population than browsers. For a newsletter reader these bytes are THE image,
/// not an escape hatch. That is the floor on this constant: the top ladder rung
/// is a resolution a mail client can still render well; something aggressive
/// like 800 would not be.
///
/// A literal, not derived from `asset_paths::LADDER` — measurement showed
/// that decoupling it from the ladder's top rung (1600) down to 1200 saves
/// ~39.5% on JPEG and ~29.5% on PNG fallbacks with no HTML change (the
/// fallback carries no width descriptor) and negligible visible softening in
/// email/no-WebP contexts. `.min(config.max_edge)` at the call site keeps a
/// caller-lowered global cap authoritative.
pub(crate) const FALLBACK_MAX_EDGE: u32 = 1200;

/// Encode a `DynamicImage` as a baseline JPEG byte vector at `quality`.
///
/// Mirrors [`encode_webp`] but emits JPEG. JPEG has no alpha channel, so the
/// image is flattened to `Rgb8`; callers that started from a source with
/// transparency should [`flatten_alpha_to_white`] first (see
/// [`encode_sized_raster`]). Uses the same `JpegEncoder` the LQIP path in
/// `scan::scan` uses.
pub(crate) fn encode_jpeg(img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let rgb = img.to_rgb8();
    let mut out: Vec<u8> = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("JPEG encode failed: {}", e))?;
    if out.is_empty() {
        return Err("JPEG encoder returned 0 bytes".to_string());
    }
    Ok(out)
}

/// Encode a `DynamicImage` as a PNG byte vector, PRESERVING any alpha channel.
///
/// The PNG twin of [`encode_jpeg`]. Unlike JPEG, PNG supports transparency, so
/// the image is encoded as `Rgba8` and the alpha channel is kept verbatim —
/// never flattened to white. Used by [`encode_sized_raster`] so a transparent
/// `.png` source (logo / diagram / icon) stays a real, transparent PNG at the
/// `.png` output path instead of collapsing to a white JPEG box.
///
/// Uses `PngEncoder` with its default compression + adaptive filter (the
/// crate default), which is the fast, general-purpose setting.
pub(crate) fn encode_png(img: &image::DynamicImage) -> Result<Vec<u8>, String> {
    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;

    let rgba = img.to_rgba8();
    let mut out: Vec<u8> = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("PNG encode failed: {}", e))?;
    if out.is_empty() {
        return Err("PNG encoder returned 0 bytes".to_string());
    }
    Ok(out)
}

/// Number of palette entries the lossy PNG path quantizes to. 256 is the most
/// an 8-bit indexed PNG can address; fewer shrinks further but the banding
/// becomes visible on skin tones and skies.
const PNG_PALETTE_COLORS: usize = 256;

/// NeuQuant sampling factor: 1 = train on every pixel (slowest, best), 30 =
/// every 30th (fastest, worst). 10 is the `color_quant` docs' quality/speed
/// middle and what the `gif` encoder uses.
const PNG_QUANT_SAMPLE_FACTOR: i32 = 10;

/// Distinct colours per pixel above which an image is treated as photographic
/// and therefore dithered. See [`is_photographic`].
const PHOTOGRAPHIC_COLOR_RATIO: f32 = 0.02;

/// Whether error diffusion will help this image or hurt it.
///
/// Dithering trades spatial noise for colour accuracy. On a photograph that is
/// the right trade — 256 colours cannot express a sky, and without diffusion
/// the gradient bands. On flat, hard-edged artwork it is the wrong trade
/// twice over: the diffused error lands in large uniform regions where the eye
/// reads it as speckle, and the resulting noise destroys the run-lengths that
/// made the indices compress in the first place.
///
/// The discriminator is colour complexity — distinct RGB triples per pixel,
/// measured after the downscale, since resampling is what reintroduces colours
/// into an image that was already palettized. Measured on the 潮汐 corpus
/// (469 PNGs, 608 JPEGs, sampled 40 each, resized to `FALLBACK_MAX_EDGE`):
///
/// | class                          | p10    | p50    | p90    |
/// |--------------------------------|--------|--------|--------|
/// | posters / comics / diagrams    | 0.0000 | 0.0002 | 0.0064 |
/// | photographs                    | 0.0315 | 0.0872 | 0.2141 |
///
/// The two populations are separated by most of an order of magnitude, so the
/// threshold sits in the gap rather than on either shoulder. Misclassifying a
/// photograph as flat art costs a little gradient banding; misclassifying flat
/// art as a photograph costs visible speckle on a poster. The gap is wide
/// enough that neither happens on real input, and the failure that remains is
/// the cheaper one.
///
/// Exact, not sampled: a 2^24-bit bitset is 2 MB and one pass, which is noise
/// next to the NeuQuant training that follows.
fn is_photographic(rgba: &image::RgbaImage) -> bool {
    let pixels = rgba.as_raw().len() / 4;
    if pixels == 0 {
        return false;
    }
    // Anything one pixel wide or tall is never photographic, and saying so here
    // is also what keeps `dither` from panicking. Floyd–Steinberg pushes its
    // quantisation error to the pixel on the right and the three below, and
    // `image` 0.25's `dither` indexes (x + 1, y) unconditionally — on a 1xN or
    // Nx1 buffer that is out of bounds and aborts the encode mid-image
    // ("Image index (1, 0) out of bounds (1, 1)").
    //
    // The ratio test alone cannot catch this: `needed` is
    // `ceil(pixels * 0.02)`, which is 1 for any image under 50 pixels, so a
    // single-pixel image clears the bar with its one colour and is classified
    // photographic. A 1x1 has no gradient to band, so `false` is the honest
    // answer as well as the safe one.
    if rgba.width() < 2 || rgba.height() < 2 {
        return false;
    }
    // Early-exit budget: once this many distinct colours have been seen the
    // verdict cannot change, so stop counting.
    let needed = ((pixels as f32) * PHOTOGRAPHIC_COLOR_RATIO).ceil() as usize;

    let mut seen = vec![0u64; (1 << 24) / 64];
    let mut distinct = 0usize;
    for px in rgba.as_raw().chunks_exact(4) {
        let key = ((px[0] as usize) << 16) | ((px[1] as usize) << 8) | (px[2] as usize);
        let (word, bit) = (key / 64, 1u64 << (key % 64));
        if seen[word] & bit == 0 {
            seen[word] |= bit;
            distinct += 1;
            if distinct >= needed {
                return true;
            }
        }
    }
    false
}

/// Adapts `color_quant`'s NeuQuant palette to `image`'s `ColorMap` so
/// `imageops::colorops::dither` can Floyd–Steinberg against it. Without
/// dithering, a 256-colour palette lays visible contour bands across any
/// gradient (sky, skin, out-of-focus background).
struct NeuQuantMap {
    nq: color_quant::NeuQuant,
}

impl image::imageops::colorops::ColorMap for NeuQuantMap {
    type Color = image::Rgba<u8>;

    fn index_of(&self, color: &Self::Color) -> usize {
        self.nq.index_of(&color.0)
    }

    fn map_color(&self, color: &mut Self::Color) {
        let cm = self.nq.color_map_rgba();
        let i = self.nq.index_of(&color.0);
        color.0 = [cm[i * 4], cm[i * 4 + 1], cm[i * 4 + 2], cm[i * 4 + 3]];
    }

    fn lookup(&self, index: usize) -> Option<Self::Color> {
        let cm = self.nq.color_map_rgba();
        cm.get(index * 4 + 3)?;
        Some(image::Rgba([
            cm[index * 4],
            cm[index * 4 + 1],
            cm[index * 4 + 2],
            cm[index * 4 + 3],
        ]))
    }

    fn has_lookup(&self) -> bool {
        true
    }
}

/// True when every pixel is fully opaque, so dropping the alpha channel is
/// lossless. Gates the quantized path: an image that actually uses
/// transparency stays on the lossless `encode_png` road (see
/// [`encode_sized_png`]).
fn is_fully_opaque(rgba: &image::RgbaImage) -> bool {
    rgba.as_raw().chunks_exact(4).all(|px| px[3] == 255)
}

/// Encode an OPAQUE image as a LOSSY 8-bit indexed (palette) PNG.
///
/// This is the "pngquant in-process" path. It is what makes the `<picture>`
/// raster fallback cheap without changing its URL: the output is still a real
/// PNG at the same `.png` path (same Content-Type, same extension sniffing),
/// it is simply a palette PNG instead of a truecolour one — 1 byte per pixel
/// before DEFLATE instead of 3 or 4.
///
/// Why lossy at all: the fallback's source is usually a photograph, and a
/// truecolour re-encode of a photograph is essentially always LARGER than the
/// author's already-optimized original, so `sized_raster_oid_for_original`'s
/// keep-smaller guard used to lose every time and ship the original verbatim.
/// A lost size comparison, not a deliberate policy — measured on the 潮汐
/// corpus, PNG originals shipped at 100.0% of source bytes.
///
/// Caller must have checked [`is_fully_opaque`]: the palette is written without
/// a `tRNS` chunk, so any alpha in the input would be silently dropped.
///
/// Errors (never panics) so the caller can fall back to lossless PNG.
fn encode_png_quantized(rgba: &image::RgbaImage) -> Result<Vec<u8>, String> {
    use image::imageops::colorops::{dither, ColorMap};

    let (w, h) = (rgba.width(), rgba.height());
    if w == 0 || h == 0 {
        return Err("cannot quantize a zero-sized image".to_string());
    }

    let map = NeuQuantMap {
        nq: color_quant::NeuQuant::new(
            PNG_QUANT_SAMPLE_FACTOR,
            PNG_PALETTE_COLORS,
            rgba.as_raw(),
        ),
    };

    // Dither into a scratch copy — `dither` rewrites pixels in place, and the
    // caller's `DynamicImage` must stay intact for the lossless fallback.
    // Flat artwork skips diffusion entirely (see `is_photographic`): each
    // uniform region then maps to a single palette entry, which is both what
    // it should look like and what compresses.
    let mut dithered = rgba.clone();
    if is_photographic(rgba) {
        dither(&mut dithered, &map);
    }

    // The palette is RGB-only: the caller guaranteed opacity, so no `tRNS`.
    let cm = map.nq.color_map_rgba();
    let mut palette = Vec::with_capacity(cm.len() / 4 * 3);
    for entry in cm.chunks_exact(4) {
        palette.extend_from_slice(&entry[..3]);
    }

    // On the dithered road `dither` already snapped every pixel to a palette
    // colour, so `index_of` is an exact lookup. On the flat-art road it is the
    // quantization step itself — nearest palette entry, no diffusion — which
    // is the point: a uniform region resolves to one index.
    let indices: Vec<u8> = dithered
        .as_raw()
        .chunks_exact(4)
        .map(|px| map.index_of(&image::Rgba([px[0], px[1], px[2], px[3]])) as u8)
        .collect();

    let mut out: Vec<u8> = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(palette);
        encoder.set_compression(png::Compression::High);
        // Row filters model truecolour gradients; on palette INDICES they
        // scramble the byte distribution and make the stream bigger.
        encoder.set_filter(png::Filter::NoFilter);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("indexed PNG header failed: {}", e))?;
        writer
            .write_image_data(&indices)
            .map_err(|e| format!("indexed PNG encode failed: {}", e))?;
        writer
            .finish()
            .map_err(|e| format!("indexed PNG finish failed: {}", e))?;
    }
    if out.is_empty() {
        return Err("indexed PNG encoder returned 0 bytes".to_string());
    }
    Ok(out)
}

/// PNG output for the deployed raster fallback: lossy palette when the image
/// is opaque, lossless truecolour when it is not.
///
/// The split is the risk boundary. Opaque PNGs at this size are photographs
/// and posters — the bytes worth attacking, and the content dithering handles
/// well. PNGs that actually use alpha are logos, diagrams and icons: small
/// already, and hard-edged artwork is exactly where palette + error diffusion
/// shows. So transparency keeps the old, lossless road (which also keeps the
/// `<picture>` fallback for a transparent logo byte-honest).
///
/// A quantization failure is never fatal — it degrades to lossless PNG, and
/// the caller's keep-smaller guard degrades further to the verbatim original.
fn encode_sized_png(img: &image::DynamicImage) -> Result<Vec<u8>, String> {
    let rgba = img.to_rgba8();
    if is_fully_opaque(&rgba) {
        match encode_png_quantized(&rgba) {
            Ok(bytes) => return Ok(bytes),
            Err(e) => log::debug!("[sized-raster] palette encode failed ({e}); using lossless PNG"),
        }
    }
    encode_png(img)
}

/// Decode `source_file`, apply EXIF orientation, resize so the longest edge is
/// at most `max_edge`, and re-encode in the SOURCE's own format. This is the
/// raster twin of the decode → EXIF-orient → Lanczos3 resize chain in
/// [`convert_single_image`] (the WebP pass).
///
/// Format dispatch keeps the OUTPUT byte format matching the source extension so
/// transparency and Content-Type are preserved:
/// - `jpg` / `jpeg` → flatten any alpha to white, encode JPEG at `jpeg_quality`.
/// - `png` → **preserve alpha** (do NOT flatten), encode PNG. A transparent
///   logo / diagram / icon must stay transparent, not become a white box, and
///   the bytes at a `.png` path must actually be a PNG (correct Content-Type on
///   extension-keyed servers). Opaque PNGs additionally take the LOSSY palette
///   road ([`encode_sized_png`]) — still a PNG at the same URL, just indexed.
///
/// The output format can never diverge from the source extension: the emitted
/// `<img src>` keeps the original filename verbatim
/// (`render/image.rs::synthesize_inner`), so JPEG bytes at a `.png` path would
/// be a Content-Type lie. That constraint is why PNG shrinks via quantization
/// rather than by re-encoding to JPEG.
///
/// Returns `Err` on any decode/encode failure so the caller can fall back to a
/// verbatim copy of the original — the build must never fail on a bad image.
pub(crate) fn encode_sized_raster(
    source_file: &Path,
    max_edge: u32,
    jpeg_quality: u8,
) -> Result<Vec<u8>, String> {
    // Content-sniffed and allocation-capped by `media::decode::sniff_decode`
    // (same primitive the WebP decode path uses) rather than a second
    // hand-rolled copy of that open/guess/decode sequence.
    let orientation = read_exif_orientation(source_file);
    let img = super::decode::sniff_decode(source_file)
        .map_err(|e| format!("decode failed: {}", e))?;

    // EXIF orientation must be applied so the sized image isn't rotated wrong.
    let img = apply_exif_orientation(img, orientation);

    let (w, h) = (img.width(), img.height());
    let resized = if w.max(h) > max_edge {
        img.resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    // Dispatch on the SOURCE format so the OUTPUT stays in that format.
    let ext = source_file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "png" {
        // PNG keeps its alpha channel — transparency is the whole point.
        // Opaque PNGs take the lossy palette road; see `encode_sized_png`.
        encode_sized_png(&resized)
    } else {
        // JPEG is opaque; composite any alpha over white (mirrors the WebP pass).
        let flat = flatten_alpha_to_white(resized);
        encode_jpeg(&flat, jpeg_quality)
    }
}

/// Produce (or reuse from the transform cache) a sized/optimized raster for a
/// raster original and return the OID of that output in `objects`.
///
/// The OUTPUT stays in the SOURCE's own format (jpg/jpeg → sized JPEG, png →
/// sized PNG with alpha preserved) — see [`encode_sized_raster`]. The caller
/// links THIS oid to the output path instead of the full-resolution original,
/// so no full-res raster is ever deployed (plan
/// `2026-07-06-exclude-original-images-from-deploy.md`). Caching mirrors the
/// WebP pass: keyed by (`source_oid`, `"image/sized-raster"`,
/// params{quality, max_edge, flatten_alpha, format}). The `format` +
/// `flatten_alpha` params reflect the actual output so a JPEG entry and a PNG
/// entry never collide. A warm cache short-circuits the decode + encode
/// entirely.
///
/// Returns `None` when the source cannot be safely sized — a decode failure
/// (corrupt / mislabeled non-image), or a CMYK JPEG (libjpeg's CMYK→RGB is
/// lossy; mirrors the WebP `SkipReason::Cmyk`). On `None` the caller keeps the
/// verbatim original. Never panics; never fails the build.
pub(crate) fn sized_raster_oid_for_original(
    source_file: &Path,
    source_oid: &str,
    objects: &crate::build::cache::ObjectStore,
    transforms: &crate::build::cache::TransformCache,
    config: &ImageCompressionConfig,
    quality: u8,
) -> Option<String> {
    use crate::build::cache::{TransformEntry, TransformRecord};

    const TRANSFORM: &str = "image/sized-raster";

    let ext = source_file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // Output format mirrors the source extension. Encode the actual output
    // format + flatten decision into the cache params so a JPEG entry and a PNG
    // entry for the same source never resolve against each other.
    let is_png = ext == "png";
    // The fallback is capped at the top ladder rung, not at the global deploy
    // cap — see `FALLBACK_MAX_EDGE`. `.min` keeps a caller that lowered the
    // global cap authoritative. It goes into the cache params, so bumping
    // either constant invalidates the old entries by itself.
    let fallback_edge = config.max_edge.min(FALLBACK_MAX_EDGE);
    let params = serde_json::json!({
        "quality": quality,
        "max_edge": fallback_edge,
        "flatten_alpha": !is_png,
        "format": if is_png { "png" } else { "jpeg" },
    });

    // ---- Cache hit? (checked BEFORE reading the source at all) ----
    if !source_oid.is_empty() {
        if let Some(cached) = transforms.find_cached_output(source_oid, TRANSFORM, &params) {
            return Some(cached);
        }
    }

    // ---- Cloud guard: the bytes are not here, and that is not a verdict. ----
    //
    // Every guard below this point reads the source, and each one answers a
    // question about its CONTENT — is it CMYK, is it animated, does it
    // re-encode smaller. A source that is still in the cloud can answer none of
    // them, and before this was fixed it silently took the encode-failure arm: an
    // eviction was logged with the same words as a corrupt JPEG, 6,967 times in
    // one incident log, recording nothing anywhere the gate could see.
    //
    // The caller (`media::pipeline`) already skips this function for an evicted
    // source, so reaching here means either a caller that does not check or a
    // file evicted between that check and this one. Both are real, so the guard
    // lives here too — and here is where it can also be RECORDED, which is the
    // half that was missing.
    if crate::build::icloud::is_evicted(source_file) {
        crate::build::cloud_ledger::note_unavailable(source_file);
        log::debug!(
            "[sized-raster] {} is still in the cloud — keeping the original for now",
            source_file.display()
        );
        return None;
    }

    // ---- CMYK guard: keep verbatim rather than ship wrong colors. ----
    // JPEG-only — PNG has no CMYK variant to worry about.
    if (ext == "jpg" || ext == "jpeg") && is_cmyk_jpeg(source_file) {
        log::debug!(
            "[sized-raster] {} is CMYK — keeping verbatim original",
            source_file.display()
        );
        return None;
    }

    // ---- APNG guard: keep verbatim to preserve animation. ----
    // The still-image PNG decoder reads only the default frame, so re-encoding
    // an animated PNG would silently flatten it to a single static frame.
    if is_png && is_apng(source_file) {
        log::debug!(
            "[sized-raster] {} is an animated PNG — keeping verbatim original",
            source_file.display()
        );
        return None;
    }

    // ---- Encode (decode → orient → resize → [flatten] → JPEG/PNG). ----
    let sized_bytes = match encode_sized_raster(source_file, fallback_edge, quality) {
        Ok(b) => b,
        Err(e) => {
            // Last resort: the file was materialized when the guard above ran
            // and had been evicted again by the time the decoder opened it.
            // `encode_sized_raster` flattens every failure to a string, so
            // re-check the placeholder bit rather than trying to recover an
            // errno from the message.
            if crate::build::icloud::is_evicted(source_file) {
                crate::build::cloud_ledger::note_unavailable(source_file);
                log::debug!(
                    "[sized-raster] {} went back into the cloud mid-encode — keeping the original for now",
                    source_file.display()
                );
                return None;
            }
            log::warn!(
                "[sized-raster] encode failed for {} ({}); keeping verbatim original",
                source_file.display(),
                e
            );
            return None;
        }
    };

    // ---- Keep-smaller guard: never make an image BIGGER. ----
    // This transform REPLACES the canonical output (unlike the side-derivative
    // WebP), so if the re-encode isn't smaller than the source it is strictly
    // worse — the source is already well optimized (a small PNG whose palette
    // re-encode can't beat the author's own, or a JPEG already below q82 and
    // within max_edge). Keep the source verbatim, but CACHE the decision by
    // pointing the transform at the source oid so we don't re-encode it on
    // every build.
    let source_size = fs::metadata(source_file).map(|m| m.len()).unwrap_or(0);
    let keep_source = source_size != 0 && sized_bytes.len() as u64 >= source_size;

    let (out_oid, out_size) = if keep_source {
        log::debug!(
            "[sized-raster] {} re-encode not smaller ({} >= {} bytes) — keeping verbatim",
            source_file.display(),
            sized_bytes.len(),
            source_size
        );
        (source_oid.to_string(), source_size)
    } else {
        // Store the sized bytes in the CAS.
        match objects.store_bytes(&sized_bytes) {
            Ok(o) => (o, sized_bytes.len() as u64),
            Err(e) => {
                log::warn!(
                    "[sized-raster] CAS store failed for {} ({}); keeping verbatim original",
                    source_file.display(),
                    e
                );
                return None;
            }
        }
    };

    // ---- Write the cache record (merge-preserve, mirrors the WebP pass). ----
    // Whole-record read-modify-write like `convert_single_image`; a benign race
    // with the concurrent WebP writer for the same source_oid can at worst drop
    // one entry and force a re-encode next build (self-healing, never wrong).
    // When keep_source, out_oid == source_oid so the cached decision resolves to
    // a verbatim link on the next build without re-encoding.
    if !source_oid.is_empty() {
        let mut record = transforms.get(source_oid).unwrap_or(TransformRecord {
            source_oid: source_oid.to_string(),
            source_size: fs::metadata(source_file).map(|m| m.len()).unwrap_or(0),
            transforms: std::collections::HashMap::new(),
        });
        record.transforms.insert(
            TRANSFORM.to_string(),
            TransformEntry {
                oid: out_oid.clone(),
                size: out_size,
                params,
            },
        );
        if let Err(e) = transforms.put(&record) {
            log::warn!("[sized-raster] failed to write transform record: {}", e);
        }
    }

    Some(out_oid)
}

/// Cheap animated-PNG (APNG) detection: an `acTL` control chunk appears before
/// the first `IDAT` in an animated PNG. The still-image decoder used by the
/// sizing pass reads only the default frame, so an APNG must be kept verbatim
/// to preserve its animation. Scans a bounded header window (acTL/IDAT are near
/// the file start); a false positive would merely keep an image verbatim.
fn is_apng(path: &Path) -> bool {
    use std::io::Read;
    let mut f = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 65536];
    let n = f.read(&mut buf).unwrap_or(0);
    let data = &buf[..n];
    let actl = data.windows(4).position(|w| w == b"acTL");
    match actl {
        None => false,
        // acTL must precede the first IDAT to mark the PNG as animated.
        Some(a) => match data.windows(4).position(|w| w == b"IDAT") {
            Some(i) => a < i,
            None => true,
        },
    }
}
