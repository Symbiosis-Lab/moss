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
use crate::build::scan::oklab::{oklch_from_srgb, srgb_from_oklch};
use crate::build::scan::wcag::{contrast_ratio, relative_luminance};
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

pub(crate) fn to_hex((r, g, b): Rgb) -> String {
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
    oklch_from_srgb([r, g, b].map(|c| c as f32 / 255.0))
}

/// The OKLCH colour as sRGB bytes, brought into sRGB as CSS maps it.
fn in_gamut(lightness: f32, chroma: f32, hue: f32) -> Rgb {
    let [r, g, b] = srgb_from_oklch(lightness, chroma, hue).map(|c| (c * 255.0).round() as u8);
    (r, g, b)
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

#[cfg(test)]
#[path = "section_text_tests.rs"]
mod tests;
