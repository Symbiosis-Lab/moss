//! A readable text colour for a section's accent.
//!
//! A folder's `color:` is an accent, and an accent is often a poor text
//! colour: a pale yellow vanishes on a white page and a mid blue sinks into a
//! dark one. For each theme this derives a colour of the same hue that reaches
//! WCAG 2 contrast 4.5:1 against moss's default page background. CSS
//! `color-mix` cannot promise a ratio, so the search runs here, at build time.
//!
//! The backgrounds are read from the same tokens the default stylesheet is
//! generated from (`--moss-color-bg`), so the two cannot drift. A theme that
//! repaints the page background must check its own contrast.

use crate::build::types::SectionAccent;
use crate::build::scan::wcag::{contrast_ratio, linear_to_srgb, relative_luminance, srgb_to_linear};
use moss_core::contract::tokens::{bg_colors, load_tokens};
use std::sync::LazyLock;

/// Minimum WCAG 2 contrast for body-size text.
const MIN_CONTRAST: f32 = 4.5;
/// Lightness step of the search, in OKLCH lightness (0..1).
const STEP: f32 = 0.005;

pub(crate) type Rgb = (u8, u8, u8);

/// moss's default page background, `(light, dark)`.
pub(crate) static BACKGROUNDS: LazyLock<(Rgb, Rgb)> = LazyLock::new(|| {
    let tokens = load_tokens().unwrap_or_else(|e| panic!("tokens.json must parse: {e}"));
    let (light, dark) = bg_colors(&tokens);
    let parse = |hex: &str| parse_hex(hex).unwrap_or_else(|| panic!("--moss-color-bg `{hex}` is not a hex colour"));
    (parse(light), parse(dark))
});

/// `#rgb` or `#rrggbb`.
fn parse_hex(hex: &str) -> Option<Rgb> {
    let digits = hex.trim().strip_prefix('#')?;
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = |s: &str| u8::from_str_radix(s, 16).ok();
    match digits.len() {
        3 => Some((v(&digits[0..1])? * 17, v(&digits[1..2])? * 17, v(&digits[2..3])? * 17)),
        6 => Some((v(&digits[0..2])?, v(&digits[2..4])?, v(&digits[4..6])?)),
        _ => None,
    }
}

fn to_hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub(crate) fn luminance((r, g, b): Rgb) -> f32 {
    relative_luminance(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

fn contrast(a: Rgb, b: Rgb) -> f32 {
    contrast_ratio(luminance(a), luminance(b))
}

/// sRGB bytes to OKLCH `(lightness, chroma, hue in radians)`.
fn to_oklch((r, g, b): Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (srgb_to_linear(r as f32 / 255.0), srgb_to_linear(g as f32 / 255.0), srgb_to_linear(b as f32 / 255.0));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let lightness = 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s;
    let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
    let b = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
    (lightness, a.hypot(b), b.atan2(a))
}

/// OKLCH to sRGB bytes, or `None` when the colour lies outside sRGB.
fn from_oklch(lightness: f32, chroma: f32, hue: f32) -> Option<Rgb> {
    let (a, b) = (chroma * hue.cos(), chroma * hue.sin());
    let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m = (lightness - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let linear = [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_93 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s,
        -0.004_196_086 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ];
    let mut out = [0u8; 3];
    for (slot, c) in out.iter_mut().zip(linear) {
        if !(-0.000_5..=1.000_5).contains(&c) {
            return None;
        }
        *slot = (linear_to_srgb(c.clamp(0.0, 1.0)) * 255.0).round() as u8;
    }
    Some((out[0], out[1], out[2]))
}

/// The colour of lightness `lightness` and hue `hue`, keeping as much of
/// `chroma` as sRGB can hold there.
fn in_gamut(lightness: f32, chroma: f32, hue: f32) -> Rgb {
    if let Some(rgb) = from_oklch(lightness, chroma, hue) {
        return rgb;
    }
    let (mut lo, mut hi) = (0.0, chroma);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if from_oklch(lightness, mid, hue).is_some() { lo = mid } else { hi = mid }
    }
    from_oklch(lightness, lo, hue).unwrap_or(if lightness > 0.5 { (255, 255, 255) } else { (0, 0, 0) })
}

/// `base` unchanged when it reaches 4.5:1 on `background`; otherwise the
/// nearest colour of the same hue that does, found by stepping lightness away
/// from the background (darker on a light page, lighter on a dark one).
fn readable_on(base: Rgb, background: Rgb) -> Rgb {
    if contrast(base, background) >= MIN_CONTRAST {
        return base;
    }
    let (lightness, chroma, hue) = to_oklch(base);
    let direction = if luminance(background) > 0.5 { -1.0 } else { 1.0 };
    let mut l = lightness;
    loop {
        l = (l + direction * STEP).clamp(0.0, 1.0);
        let candidate = in_gamut(l, chroma, hue);
        if contrast(candidate, background) >= MIN_CONTRAST || l <= 0.0 || l >= 1.0 {
            return candidate;
        }
    }
}

/// The accent for `color` (`#rgb` or `#rrggbb`) with its readable text colour
/// in each theme, derived once where the section is resolved. `None` for a
/// value that is not a hex colour, which is how a bad `color:` is rejected.
pub(crate) fn accent(color: &str, segment: String) -> Option<SectionAccent> {
    let base = parse_hex(color)?;
    let (light_bg, dark_bg) = *BACKGROUNDS;
    Some(SectionAccent {
        color: color.to_string(),
        text_light: to_hex(readable_on(base, light_bg)),
        text_dark: to_hex(readable_on(base, dark_bg)),
        segment,
    })
}

/// The inline declarations that publish a section's colour: the accent itself
/// and its two readable text variants. The default stylesheet picks the one for
/// the current theme as `--moss-section-text`.
pub(crate) fn section_style(accent: &SectionAccent) -> String {
    format!(
        "--moss-section-color: {}; --moss-section-text-light: {}; --moss-section-text-dark: {}",
        accent.color, accent.text_light, accent.text_dark
    )
}

#[cfg(test)]
#[path = "section_text_tests.rs"]
mod tests;
