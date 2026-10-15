//! WCAG 2 relative luminance and contrast ratio, the one copy the build's
//! colour code shares (image-derived card colours and section text colours).

/// sRGB channel (0.0–1.0) to linear light.
pub(crate) fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// WCAG 2.x relative luminance of an sRGB colour, channels in 0.0–1.0.
pub(crate) fn relative_luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
}

/// WCAG 2.x contrast ratio between two relative luminances.
pub(crate) fn contrast_ratio(l1: f32, l2: f32) -> f32 {
    let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}
