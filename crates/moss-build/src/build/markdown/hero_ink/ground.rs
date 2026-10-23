//! A picture's luminance, read at the scale of the letters' strokes.
//!
//! The scan keeps the picture as a linear-luminance image ([`Ground`]). A
//! reading slides a window about as wide as a stroke over the part of the
//! picture a text covers and keeps the darkest and the lightest window mean:
//! texture finer than a stroke is averaged away, and a shape a stroke wide
//! counts in full wherever it falls.

use image::DynamicImage;

use super::{Area, Fade, Tone};
use crate::build::scan::wcag::{linear_to_srgb, srgb_to_linear};

/// The longest side a picture is kept at: the widest window shows a picture
/// no wider than this at its own resolution or coarser.
const CACHE_SIDE: u32 = 2048;
/// The window's side in ems of the smallest type of the part it reads: regular
/// Latin and CJK strokes are at or above it.
pub(super) const STROKE_EM: f32 = 0.08;

/// A picture's linear luminance, one byte a pixel as the square root, so the
/// darks keep their steps.
#[derive(Clone, Debug, PartialEq)]
pub struct Ground {
    width: usize,
    height: usize,
    bytes: Vec<u8>,
}

/// How many luminance images this process has built: a site with no hero builds none.
static BUILT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn built() -> usize {
    BUILT.load(std::sync::atomic::Ordering::Relaxed)
}

impl Ground {
    pub fn from_image(img: &DynamicImage) -> Self {
        BUILT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let small;
        let img = if img.width().max(img.height()) > CACHE_SIDE {
            let scale = CACHE_SIDE as f32 / img.width().max(img.height()) as f32;
            small = img.resize_exact(((img.width() as f32 * scale) as u32).max(1), ((img.height() as f32 * scale) as u32).max(1), image::imageops::FilterType::Triangle);
            &small
        } else {
            img
        };
        let rgb = img.to_rgb8();
        let lin: Vec<f32> = (0..=255).map(|i| srgb_to_linear(i as f32 / 255.0)).collect();
        let bytes = rgb
            .pixels()
            .map(|p| {
                let lum = 0.2126 * lin[p[0] as usize] + 0.7152 * lin[p[1] as usize] + 0.0722 * lin[p[2] as usize];
                (lum.sqrt() * 255.0).round() as u8
            })
            .collect();
        Self { width: rgb.width() as usize, height: rgb.height() as usize, bytes }
    }

    /// The darkest and the lightest stroke-wide window over `area`, with the
    /// fade composited per pixel in sRGB and the window averaged in linear light.
    pub(super) fn read(&self, area: &Area, fade: Option<Fade>, median: bool) -> Tone {
        self.read_with(area, fade, median, None)
    }

    /// [`Ground::read`] with the window's side given, in source pixels.
    pub(super) fn read_with(&self, area: &Area, fade: Option<Fade>, median: bool, window: Option<usize>) -> Tone {
        let span = |(from, to): (f32, f32), n: usize| {
            let first = ((from.clamp(0.0, 1.0) * n as f32).floor() as usize).min(n - 1);
            (first, ((to.clamp(0.0, 1.0) * n as f32).ceil() as usize).clamp(first + 1, n))
        };
        let (r0, r1) = span(area.rows, self.height);
        let (c0, c1) = span(area.cols, self.width);
        let (rows, cols) = (r1 - r0, c1 - c0);
        let window = window.unwrap_or(((STROKE_EM * area.font * self.width as f32 / area.css_per_width).floor() as usize).max(1)).clamp(1, rows.min(cols));
        let stride = cols + 1;
        let mut sums = vec![0f64; (rows + 1) * stride];
        for r in 0..rows {
            let y = (r0 + r) as f32 + 0.5;
            let alpha = fade.map(|f| ((y / self.height as f32 - area.shown.0) / (area.shown.1 - area.shown.0).max(0.001) / f.depth).clamp(0.0, 1.0));
            let mut run = 0f64;
            for c in 0..cols {
                let level = f32::from(self.bytes[(r0 + r) * self.width + c0 + c]) / 255.0;
                let lum = level * level;
                let seen = match (fade, alpha) {
                    (Some(f), Some(a)) if a < 1.0 => srgb_to_linear(f.blend(linear_to_srgb(lum), a)),
                    _ => lum,
                };
                run += f64::from(seen);
                sums[(r + 1) * stride + c + 1] = sums[r * stride + c + 1] + run;
            }
        }
        let (mut darkest, mut lightest) = (f64::MAX, f64::MIN);
        let area_px = (window * window) as f64;
        let mut means = Vec::new();
        for r in 0..=rows - window {
            for c in 0..=cols - window {
                let sum = sums[(r + window) * stride + c + window] - sums[r * stride + c + window] - sums[(r + window) * stride + c] + sums[r * stride + c];
                let mean = (sum / area_px) as f32;
                darkest = darkest.min(f64::from(mean));
                lightest = lightest.max(f64::from(mean));
                if median {
                    means.push(mean);
                }
            }
        }
        let median = (!means.is_empty()).then(|| {
            let middle = means.len() / 2;
            *means.select_nth_unstable_by(middle, f32::total_cmp).1
        });
        Tone { darkest: darkest as f32, lightest: lightest as f32, median, need: area.need }
    }
}
