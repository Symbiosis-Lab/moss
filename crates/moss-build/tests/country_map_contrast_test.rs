//! Every colour step of a country map keeps 3:1 (WCAG 1.4.11) against the land
//! colour, in both themes, for every number of levels the map supports, and
//! the ramp reads as order: lightness moves one way, in steps wide enough to
//! tell apart (light to dark in light mode, dark to light in dark mode).

const CSS: &str = include_str!("../src/assets/css/site/country-map.css");

fn rgb(hex: &str) -> [f64; 3] {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap();
    [(n >> 16) as f64 / 255.0, ((n >> 8) & 255) as f64 / 255.0, (n & 255) as f64 / 255.0]
}

fn linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn oklab(c: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = c.map(linear);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s]
}

/// Linear-light sRGB of an OKLab colour, clamped into gamut as a browser does.
fn from_oklab([l, a, b]: [f64; 3]) -> [f64; 3] {
    let (l_, m_, s_) = ((l + 0.3963377774 * a + 0.2158037573 * b).powi(3), (l - 0.1055613458 * a - 0.0638541728 * b).powi(3), (l - 0.0894841775 * a - 1.291485548 * b).powi(3));
    [4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_, -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_, -0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_].map(|c| c.clamp(0.0, 1.0))
}

/// `color-mix(in oklch, high share%, low)`: lightness, chroma and hue by the shorter arc.
fn mix(low: [f64; 3], high: [f64; 3], t: f64) -> [f64; 3] {
    let (a, b) = (oklab(low), oklab(high));
    let (ca, cb) = (a[1].hypot(a[2]), b[1].hypot(b[2]));
    let (ha, hb) = (a[2].atan2(a[1]), b[2].atan2(b[1]));
    let mut d = hb - ha;
    while d > std::f64::consts::PI { d -= 2.0 * std::f64::consts::PI; }
    while d < -std::f64::consts::PI { d += 2.0 * std::f64::consts::PI; }
    let (l, c, h) = (a[0] + (b[0] - a[0]) * t, ca + (cb - ca) * t, ha + d * t);
    from_oklab([l, c * h.cos(), c * h.sin()])
}

fn luminance(linear_rgb: [f64; 3]) -> f64 {
    0.2126 * linear_rgb[0] + 0.7152 * linear_rgb[1] + 0.0722 * linear_rgb[2]
}

fn ratio(a: f64, b: f64) -> f64 {
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// The sRGB colour a `--moss-country-<name>` fallback gives in the light or the dark block.
fn fallback(name: &str, dark: bool) -> [f64; 3] {
    let selector = ":is(.moss-country-map, .moss-country-list) {";
    let after = if dark { CSS.split(&format!("[data-theme=\"dark\"] {selector}")).nth(1) } else { CSS.split(selector).nth(1) };
    let block = after.unwrap().split('}').next().unwrap();
    let line = block.lines().find(|l| l.contains(&format!("var(--moss-country-{name},"))).unwrap_or_else(|| panic!("no {name}"));
    rgb(line.rsplit(", ").next().unwrap().trim_end_matches(");").trim_end_matches(')').trim_end_matches(';').trim_end_matches(')'))
}

#[test]
fn every_step_is_three_to_one_against_land_in_both_themes() {
    let mut worst = f64::MAX;
    for dark in [false, true] {
        let land = luminance(fallback("land", dark).map(linear));
        let (low, high) = (fallback("fill-low", dark), fallback("fill-high", dark));
        for n in 2..=7usize {
            for i in 0..n {
                // color-mix(in oklch) gives light-linear sRGB here.
                let r = ratio(luminance(mix(low, high, i as f64 / (n - 1) as f64)), land);
                worst = worst.min(r);
                assert!(r >= 3.0, "{} theme, {n} levels, step {}: {r:.2}:1 against land", if dark { "dark" } else { "light" }, i + 1);
            }
        }
    }
    eprintln!("worst step: {worst:.2}:1");
}

#[test]
fn the_ramp_is_ordered_in_lightness_with_visible_steps() {
    for dark in [false, true] {
        let (low, high) = (fallback("fill-low", dark), fallback("fill-high", dark));
        let n = 7usize;
        let ls: Vec<f64> = (0..n).map(|i| oklab(from_linear_to_srgb(mix(low, high, i as f64 / (n - 1) as f64)))[0]).collect();
        for pair in ls.windows(2) {
            let step = if dark { pair[1] - pair[0] } else { pair[0] - pair[1] };
            assert!(step >= 0.06, "{} theme: adjacent steps differ by {step:.3} in lightness, need 0.06", if dark { "dark" } else { "light" });
        }
    }
}

fn from_linear_to_srgb(c: [f64; 3]) -> [f64; 3] {
    c.map(|v| if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 })
}
