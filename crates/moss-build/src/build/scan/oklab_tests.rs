use super::*;

/// Expected values are from colorjs.io, the reference implementation by the
/// CSS Color 4 editors (`toGamut({ space: "srgb", method: "css" })`).
fn hex(rgb: [f32; 3]) -> String {
    rgb.map(|c| format!("{:02x}", (c.clamp(0.0, 1.0) * 255.0).round() as u8)).concat()
}

fn oklch(l: f32, c: f32, hue_degrees: f32) -> String {
    hex(srgb_from_oklch(l, c, hue_degrees.to_radians()))
}

#[test]
fn a_colour_inside_srgb_converts_exactly() {
    assert_eq!(oklch(0.145, 0.006, 60.0), "0c0a08");
    assert_eq!(oklch(0.955, 0.008, 80.0), "f3f0ea");
    assert_eq!(oklch(0.665, 0.19, 37.0), "f05e31");
    assert_eq!(oklch(0.5, 0.0, 0.0), "636363");
}

#[test]
fn a_colour_outside_srgb_loses_chroma_not_its_hue_or_lightness() {
    // Plain channel clipping would give 00d200, ff7053 and 3800ff.
    assert_eq!(oklch(0.7, 0.4, 145.0), "00c300");
    assert_eq!(oklch(0.9, 0.3, 30.0), "ffccc1");
    assert_eq!(oklch(0.5, 0.35, 270.0), "3425ff");
}

#[test]
fn lightness_past_either_end_is_white_or_black() {
    assert_eq!(oklch(1.2, 0.1, 50.0), "ffffff");
    assert_eq!(oklch(0.0, 0.3, 50.0), "000000");
}

#[test]
fn srgb_round_trips_through_oklch() {
    for rgb in [[0.2, 0.4, 0.6], [1.0, 0.0, 0.0], [0.95, 0.94, 0.92]] {
        let (l, c, h) = oklch_from_srgb(rgb);
        assert_eq!(hex(srgb_from_oklch(l, c, h)), hex(rgb));
    }
}

#[test]
fn cie_lab_is_adapted_from_its_d50_white() {
    let [l, a, b] = oklab_from_lab([52.2, 40.1, 59.9]);
    assert_eq!(hex(srgb_from_oklch(l, a.hypot(b), b.atan2(a))), "c65d07");
    let [l, a, b] = oklab_from_lab([100.0, 0.0, 0.0]);
    assert!((l - 1.0).abs() < 1e-3 && a.hypot(b) < 1e-3, "Lab white is OKLab white: {l} {a} {b}");
}
