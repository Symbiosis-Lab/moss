//! OKLab and OKLCH, and the way CSS Color 4 brings a colour outside sRGB back
//! into it: lower its OKLCH chroma until clipping the rest moves it less than
//! a just-noticeable difference. sRGB channels here are 0.0–1.0.

use crate::build::scan::wcag::{linear_to_srgb, srgb_to_linear};

/// sRGB to OKLCH `(lightness, chroma, hue in radians)`.
pub(crate) fn oklch_from_srgb(rgb: [f32; 3]) -> (f32, f32, f32) {
    let [l, a, b] = oklab_from_linear(rgb.map(srgb_to_linear));
    (l, a.hypot(b), b.atan2(a))
}

/// OKLCH (hue in radians) to sRGB, mapped into gamut. An OKLCH or OKLab
/// colour in CSS is any colour a screen could show, so this is how a page's
/// `oklch()` becomes the one sRGB value a reader without wide gamut shows.
pub(crate) fn srgb_from_oklch(lightness: f32, chroma: f32, hue: f32) -> [f32; 3] {
    const JND: f32 = 0.02;
    const EPSILON: f32 = 0.0001;
    if lightness >= 1.0 {
        return [1.0; 3];
    }
    if lightness <= 0.0 {
        return [0.0; 3];
    }
    let at = |c: f32| [lightness, c * hue.cos(), c * hue.sin()];
    let origin = at(chroma);
    let mut clipped = clip(origin);
    if in_gamut(origin) || distance(origin, oklab_from_linear(clipped.map(srgb_to_linear))) < JND {
        return clipped;
    }
    let (mut min, mut max, mut min_in_gamut) = (0.0, chroma, true);
    while max - min > EPSILON {
        let current = at((min + max) / 2.0);
        if min_in_gamut && in_gamut(current) {
            min = (min + max) / 2.0;
            continue;
        }
        clipped = clip(current);
        let e = distance(current, oklab_from_linear(clipped.map(srgb_to_linear)));
        if e < JND {
            if JND - e < EPSILON {
                return clipped;
            }
            min_in_gamut = false;
            min = (min + max) / 2.0;
        } else {
            max = (min + max) / 2.0;
        }
    }
    clipped
}

/// CIE Lab under the D50 white, as CSS `lab()` and `lch()` give it, to OKLab.
pub(crate) fn oklab_from_lab([l, a, b]: [f32; 3]) -> [f32; 3] {
    const KAPPA: f32 = 24389.0 / 27.0;
    const E: f32 = 216.0 / 24389.0;
    let f1 = (l + 16.0) / 116.0;
    let (f0, f2) = (a / 500.0 + f1, f1 - b / 200.0);
    let curve = |f: f32| if f.powi(3) > E { f.powi(3) } else { (116.0 * f - 16.0) / KAPPA };
    let y = if l > KAPPA * E { f1.powi(3) } else { l / KAPPA };
    let d50 = [curve(f0) * 0.964_295_7, y, curve(f2) * 0.825_104_6];
    // Bradford adaptation to D65, then D65 XYZ to OKLab's cone response.
    let d65 = mul([[0.955_473_4, -0.023_098_455, 0.063_259_24], [-0.028_369_71, 1.009_995_4, 0.021_041_44], [0.012_314_015, -0.020_507_65, 1.330_365_9]], d50);
    let lms = mul([[0.819_022_4, 0.361_906_26, -0.128_873_78], [0.032_983_654, 0.929_286_86, 0.036_144_666], [0.048_177_19, 0.264_239_53, 0.633_547_83]], d65);
    lms_to_oklab(lms)
}

fn oklab_from_linear(rgb: [f32; 3]) -> [f32; 3] {
    lms_to_oklab(mul([[0.412_221_47, 0.536_332_55, 0.051_445_995], [0.211_903_5, 0.680_699_5, 0.107_396_96], [0.088_302_46, 0.281_718_85, 0.629_978_7]], rgb))
}

fn lms_to_oklab(lms: [f32; 3]) -> [f32; 3] {
    mul([[0.210_454_26, 0.793_617_8, -0.004_072_047], [1.977_998_5, -2.428_592_2, 0.450_593_7], [0.025_904_037, 0.782_771_77, -0.808_675_77]], lms.map(f32::cbrt))
}

/// OKLab to linear sRGB, unclipped.
fn linear_from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let lms = mul([[1.0, 0.396_337_78, 0.215_803_76], [1.0, -0.105_561_346, -0.063_854_17], [1.0, -0.089_484_18, -1.291_485_5]], [l, a, b]).map(|x| x.powi(3));
    mul([[4.076_741_7, -3.307_711_6, 0.230_969_93], [-1.268_438, 2.609_757_4, -0.341_319_4], [-0.004_196_086, -0.703_418_6, 1.707_614_7]], lms)
}

/// Inside sRGB, with room for the float error of a colour on its edge.
fn in_gamut(oklab: [f32; 3]) -> bool {
    linear_from_oklab(oklab).iter().all(|c| (-0.000_5..=1.000_5).contains(c))
}

fn clip(oklab: [f32; 3]) -> [f32; 3] {
    linear_from_oklab(oklab).map(|c| linear_to_srgb(c.clamp(0.0, 1.0)))
}

/// Delta E OK: the distance between two OKLab colours.
fn distance(x: [f32; 3], y: [f32; 3]) -> f32 {
    ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt()
}

fn mul(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

#[cfg(test)]
#[path = "oklab_tests.rs"]
mod tests;
