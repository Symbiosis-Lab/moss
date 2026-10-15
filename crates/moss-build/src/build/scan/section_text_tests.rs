use super::*;

fn section_text_colors(color: &str) -> Option<(String, String)> {
    accent(color, String::new()).map(|a| (a.text_light, a.text_dark))
}

/// WCAG 2 contrast computed independently of the module under test.
fn ratio(a: &str, b: &str) -> f64 {
    let lum = |hex: &str| {
        let ch = |i: usize| {
            let v = u8::from_str_radix(&hex[i..i + 2], 16).unwrap() as f64 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(1) + 0.7152 * ch(3) + 0.0722 * ch(5)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn backgrounds() -> (String, String) {
    let (l, d) = *BACKGROUNDS;
    (to_hex(l), to_hex(d))
}

#[test]
fn the_backgrounds_are_the_ones_the_default_stylesheet_is_built_from() {
    let tokens = load_tokens().unwrap();
    let root = moss_core::contract::tokens::format_root_block(&tokens);
    let dark = moss_core::contract::tokens::format_dark_root_block(&tokens);
    let (light_bg, dark_bg) = backgrounds();
    assert!(root.contains(&format!("--moss-color-bg: {light_bg};")), "{root}");
    assert!(dark.contains(&format!("--moss-color-bg: {dark_bg};")), "{dark}");
}

#[test]
fn a_pale_yellow_is_darkened_on_the_light_page_and_keeps_its_hue() {
    let (light, dark) = section_text_colors("#f2d64b").unwrap();
    let (light_bg, dark_bg) = backgrounds();
    assert_ne!(light, "#f2d64b");
    assert!(ratio(&light, &light_bg) >= 4.5, "{light}");
    // Already bright enough for the dark page, so it is returned as written.
    assert_eq!(dark, "#f2d64b");
    assert!(ratio(&dark, &dark_bg) >= 4.5);
    let (_, _, base_hue) = to_oklch(parse_hex("#f2d64b").unwrap());
    let (_, _, hue) = to_oklch(parse_hex(&light).unwrap());
    assert!((hue - base_hue).abs() < 0.12, "hue moved from {base_hue} to {hue}");
}

#[test]
fn a_dark_blue_is_lightened_on_the_dark_page() {
    let (light, dark) = section_text_colors("#1f3a6b").unwrap();
    let (light_bg, dark_bg) = backgrounds();
    assert_eq!(light, "#1f3a6b");
    assert!(ratio(&light, &light_bg) >= 4.5);
    assert_ne!(dark, "#1f3a6b");
    assert!(ratio(&dark, &dark_bg) >= 4.5, "{dark}");
}

#[test]
fn a_colour_that_already_passes_is_returned_unchanged_and_shorthand_expands() {
    // Near-black passes on the light page, near-white on the dark one.
    assert_eq!(section_text_colors("#222222").unwrap().0, "#222222");
    assert_eq!(section_text_colors("#eee").unwrap().1, "#eeeeee");
}

#[test]
fn every_mode_meets_the_ratio_across_the_hue_wheel() {
    let (light_bg, dark_bg) = backgrounds();
    for step in 0..48 {
        let hue = step as f32 / 48.0 * std::f32::consts::TAU;
        for (l, c) in [(0.3, 0.12), (0.55, 0.2), (0.7, 0.15), (0.9, 0.1), (0.97, 0.05)] {
            let base = in_gamut(l, c, hue);
            let (light, dark) = section_text_colors(&to_hex(base)).unwrap();
            assert!(ratio(&light, &light_bg) >= 4.5, "{} -> light {light}", to_hex(base));
            assert!(ratio(&dark, &dark_bg) >= 4.5, "{} -> dark {dark}", to_hex(base));
        }
    }
}

#[test]
fn a_value_that_is_not_hex_has_no_text_colour() {
    for bad in ["crimson", "#12", "#12345g", "#+1+1+1", ""] {
        assert_eq!(accent(bad, String::new()), None, "{bad}");
    }
}

#[test]
fn the_style_carries_the_accent_and_both_text_colours() {
    let a = accent("#f2d64b", "work".into()).unwrap();
    assert_eq!(
        section_style(&a),
        format!(
            "--moss-section-color: #f2d64b; --moss-section-text-light: {}; --moss-section-text-dark: #f2d64b",
            a.text_light
        )
    );
}
