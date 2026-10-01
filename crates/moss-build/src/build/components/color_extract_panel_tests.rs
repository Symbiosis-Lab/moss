use super::*;

/// Re-derive the WCAG contrast ratio between a `panel_background` result and
/// `text_lum`, after compositing the panel over a given 0.0/1.0 grey
/// backdrop — the same worst-case check a browser's actual paint performs,
/// done here without a browser so the guarantee has a fast, deterministic
/// regression test alongside the end-to-end Playwright one.
fn contrast_after_compositing(panel_rgba: &str, backdrop: f32, text_lum: f32) -> f32 {
    let inner = panel_rgba
        .strip_prefix("rgba(")
        .and_then(|s| s.strip_suffix(")"))
        .expect("panel_background emits rgba(...)");
    let parts: Vec<f32> = inner
        .split(',')
        .map(|p| p.trim().parse::<f32>().expect("numeric rgba component"))
        .collect();
    let alpha = parts[3];
    let composite = |c: f32| (alpha * (c / 255.0) + (1.0 - alpha) * backdrop).clamp(0.0, 1.0);
    let panel_lum = relative_luminance(composite(parts[0]), composite(parts[1]), composite(parts[2]));
    contrast_ratio(panel_lum, text_lum)
}

/// A dense pattern's average colour can land almost anywhere in the sRGB
/// cube; sweep a representative spread of hues/lightnesses rather than one
/// fixture; the whole point of `panel_lightness_for_contrast`'s proof is
/// that it holds for ANY of them, not just the one busy-image fixture the
/// Playwright test happens to use.
const SAMPLE_COLORS: &[&str] = &[
    "#F5E642", // bright yellow — the busy-image fixture's dominant colour
    "#1B2A38", // dark navy
    "#EDF6F4", // near-white pale
    "#D23C3C", // saturated red
    "#2E7D32", // mid green
    "#000000", // pure black
    "#FFFFFF", // pure white
];

#[test]
fn panel_background_white_text_clears_aa_over_black_and_white() {
    for raw in SAMPLE_COLORS {
        let panel = panel_background(raw, true).unwrap_or_else(|| panic!("{raw} should parse"));
        for backdrop in [0.0_f32, 1.0_f32] {
            let ratio = contrast_after_compositing(&panel, backdrop, 1.0);
            assert!(
                ratio >= 4.5,
                "white text over {raw}'s panel ({panel}) composited on backdrop {backdrop} \
                 only reaches {ratio:.2}:1"
            );
        }
    }
}

#[test]
fn panel_background_dark_text_clears_aa_over_black_and_white() {
    let text_lum = hero_tone_color_luminance();
    for raw in SAMPLE_COLORS {
        let panel = panel_background(raw, false).unwrap_or_else(|| panic!("{raw} should parse"));
        for backdrop in [0.0_f32, 1.0_f32] {
            let ratio = contrast_after_compositing(&panel, backdrop, text_lum);
            assert!(
                ratio >= 4.5,
                "dark text over {raw}'s panel ({panel}) composited on backdrop {backdrop} \
                 only reaches {ratio:.2}:1"
            );
        }
    }
}

#[test]
fn panel_background_passes_through_none_for_already_processed_video_color() {
    // Same passthrough boundary as `is_light_cover`: a video-scan colour
    // arrives already WCAG-darkened `hsla(...)`, which this function does
    // not (yet) parse — callers fall back to the CSS default rather than
    // get a wrong tint.
    assert_eq!(panel_background("hsla(200, 45%, 30%, 1)", true), None);
}

#[test]
fn panel_background_keeps_the_true_colour_when_it_already_passes() {
    // Pure black already clears 4.5:1 against white text with room to
    // spare at any alpha — panel_lightness_for_contrast's early return
    // should hand it back unmodified rather than searching.
    let panel = panel_background("#000000", true).expect("black parses");
    assert_eq!(panel, "rgba(0, 0, 0, 0.8)");
}
