//! The one Lanczos3 scaler for the pictures a build encodes.
//!
//! `fast_image_resize` picks NEON, AVX2 or SSE4.1 at run time. A rung's bytes
//! must not depend on the machine that built it, so the test below forces
//! every path the host has and compares each against pinned output.
//!
//! The JPEG/PNG fallback raster, the favicon and the folder icon still use
//! `image`'s own Lanczos3: each is a single small picture, so the faster
//! filter would save little there.

use fast_image_resize as fir;
use image::DynamicImage;

/// Scale `img` to exactly `w` × `h`.
pub(crate) fn scale_exact(img: &DynamicImage, w: u32, h: u32) -> DynamicImage {
    scale_with(&mut fir::Resizer::new(), img, w, h)
}

/// The size of `(w, h)` scaled to fit a `bound` × `bound` box, rounded the
/// way `image`'s own `DynamicImage::resize` rounds it.
pub(crate) fn fit_within(w: u32, h: u32, bound: u32) -> (u32, u32) {
    let ratio = (f64::from(bound) / f64::from(w)).min(f64::from(bound) / f64::from(h));
    let edge = |e: u32| ((f64::from(e) * ratio).round() as u32).max(1);
    (edge(w), edge(h))
}

fn scale_with(resizer: &mut fir::Resizer, img: &DynamicImage, w: u32, h: u32) -> DynamicImage {
    use DynamicImage::{ImageRgb8, ImageRgba8};
    // Only 8-bit RGB and RGBA are scaled as they are, so only those two
    // filters are linked. Anything else (grey, 16-bit, float) becomes one of
    // them first: the encoders take 8-bit RGB or RGBA in any case.
    let widened;
    let src = match img {
        ImageRgb8(_) | ImageRgba8(_) => img,
        other if other.color().has_alpha() => {
            widened = ImageRgba8(other.to_rgba8());
            &widened
        }
        other => {
            widened = ImageRgb8(other.to_rgb8());
            &widened
        }
    };
    let mut dst = DynamicImage::new(w, h, src.color());
    let from = (src.width(), src.height(), src.as_bytes());
    let scaled = match &mut dst {
        ImageRgb8(out) => scale_pixels::<fir::pixels::U8x3>(resizer, from, out, (w, h)),
        ImageRgba8(out) => scale_pixels::<fir::pixels::U8x4>(resizer, from, out, (w, h)),
        _ => Err("not 8-bit RGB or RGBA".to_string()),
    };
    match scaled {
        Ok(()) => dst,
        // Unreachable while source and destination share one layout.
        Err(e) => {
            log::warn!("[image] fast scale to {w}x{h} failed ({e}); using the scalar filter");
            src.resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        }
    }
}

fn scale_pixels<P: fir::PixelTrait>(
    resizer: &mut fir::Resizer,
    (src_w, src_h, src): (u32, u32, &[u8]),
    out: &mut [u8],
    (w, h): (u32, u32),
) -> Result<(), String> {
    let src = fir::images::TypedImageRef::<P>::from_buffer(src_w, src_h, src).map_err(|e| e.to_string())?;
    let mut dst = fir::images::TypedImage::<P>::from_buffer(w, h, out).map_err(|e| e.to_string())?;
    // `use_alpha`: colour is weighted by coverage, so a transparent pixel's
    // hidden colour does not bleed into the edge beside it.
    let options = fir::ResizeOptions::new()
        .resize_alg(fir::ResizeAlg::Convolution(fir::FilterType::Lanczos3))
        .use_alpha(true);
    resizer.resize_typed(&src, &mut dst, &options).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ColorType, GenericImageView};

    /// A photograph-like test card: smooth gradients, hard edges and noise,
    /// with alpha (where the layout has it) running from clear to opaque.
    fn card(color: ColorType) -> DynamicImage {
        let (w, h) = (1031u32, 777u32);
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let rgba = image::RgbaImage::from_fn(w, h, |x, y| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let noise = (seed >> 56) as u8 / 8;
            let edge = if (x / 37 + y / 53) % 2 == 0 { 200 } else { 20 };
            image::Rgba([
                ((x * 255 / w) as u8).wrapping_add(noise),
                ((y * 255 / h) as u8) / 2 + edge / 2,
                edge.wrapping_add(noise),
                ((x + y) * 255 / (w + h)) as u8,
            ])
        });
        let img = DynamicImage::ImageRgba8(rgba);
        match color {
            ColorType::La8 => DynamicImage::ImageLumaA8(img.to_luma_alpha8()),
            ColorType::Rgb8 => DynamicImage::ImageRgb8(img.to_rgb8()),
            ColorType::Rgba8 => img,
            ColorType::Rgb16 => DynamicImage::ImageRgb16(img.to_rgb16()),
            ColorType::Rgba16 => DynamicImage::ImageRgba16(img.to_rgba16()),
            other => panic!("no test card for {other:?}"),
        }
    }

    fn host_paths() -> Vec<fir::CpuExtensions> {
        let mut paths = vec![fir::CpuExtensions::None];
        #[cfg(target_arch = "aarch64")]
        paths.push(fir::CpuExtensions::Neon);
        #[cfg(target_arch = "x86_64")]
        paths.extend([fir::CpuExtensions::Sse4_1, fir::CpuExtensions::Avx2]);
        paths.retain(fir::CpuExtensions::is_supported);
        paths
    }

    /// The pinned hashes are the plain (no SIMD) path's output. A path that
    /// gives other bytes would make one source encode differently on two
    /// machines, and a shared transform cache would then serve either.
    #[test]
    fn every_cpu_path_gives_the_pinned_bytes() {
        const PINNED: [(ColorType, u32, u32, u64); 4] = [
            (ColorType::Rgb8, 800, 602, 0xed37_f293_5b88_101b),
            (ColorType::Rgb8, 277, 208, 0xbcac_cdf2_05dd_6543),
            (ColorType::Rgba8, 800, 602, 0xf76c_1795_f1f4_690e),
            (ColorType::Rgba8, 277, 208, 0x7988_a76e_d3fe_43c7),
        ];
        let paths = host_paths();
        assert!(paths.len() > 1 || !cfg!(any(target_arch = "aarch64", target_arch = "x86_64")));
        let mut wrong = Vec::new();
        for (color, w, h, pinned) in PINNED {
            let src = card(color);
            for path in &paths {
                let mut resizer = fir::Resizer::new();
                // SAFETY: `host_paths` kept only what this CPU reports.
                unsafe { resizer.set_cpu_extensions(*path) };
                let out = scale_with(&mut resizer, &src, w, h);
                assert_eq!(out.dimensions(), (w, h));
                let hash = xxhash_rust::xxh3::xxh3_64(out.as_bytes());
                if hash != pinned {
                    wrong.push(format!("{color:?} to {w}x{h} on {path:?}: {hash:#018x}"));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn any_other_layout_is_scaled_as_eight_bit_rgb_or_rgba() {
        assert_eq!(scale_exact(&card(ColorType::Rgb16), 80, 60).color(), ColorType::Rgb8);
        assert_eq!(scale_exact(&card(ColorType::Rgba16), 80, 60).color(), ColorType::Rgba8);
        for color in [ColorType::Rgba16, ColorType::La8] {
            let widened = DynamicImage::ImageRgba8(card(color).to_rgba8());
            assert_eq!(scale_exact(&card(color), 80, 60), scale_exact(&widened, 80, 60), "{color:?}");
        }
    }

    #[test]
    fn fit_within_rounds_as_the_image_crate_does() {
        for (w, h, bound) in [
            (4001, 2999, 2400), (2999, 4001, 2400), (5161, 1356, 2400), (7, 9001, 2400),
            (2401, 2400, 2400), (3333, 3334, 1200), (16383, 3, 800), (4032, 3024, 2400),
            (6000, 4001, 2400), (2500, 1667, 277),
        ] {
            let theirs = image::DynamicImage::new_luma8(w, h)
                .resize(bound, bound, image::imageops::FilterType::Nearest)
                .dimensions();
            assert_eq!(fit_within(w, h, bound), theirs, "{w}x{h} into {bound}");
        }
    }
}
