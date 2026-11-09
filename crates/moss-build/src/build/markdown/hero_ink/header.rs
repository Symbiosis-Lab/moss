//! The ink of a site header laid over a hero's picture (`header=over`).
//!
//! The header sits on the top band of the picture, full width, so the band is
//! read in every layout as small text (nav links) and the ink chosen that
//! clears WCAG AA against the worst window. Light ink is preferred (the
//! header's own look); dark ink is taken where only it clears; where neither
//! does, or the picture cannot be read, the stylesheet's strong wash carries
//! the light ink instead.

use super::{Face, Frame, Ground, Ink, Inks, Layout, Spot, MIN_CONTRAST};
use crate::build::media::dimensions::MediaDimensionLookup;

/// How tall the header's text band is at most: the nav row and its padding.
/// The stylesheet's solid wash covers the same 5rem.
const BAND_PX: f32 = 80.0;
/// The smallest type in the header: a nav link.
const FONT_PX: f32 = 14.0;

fn band(frame: &Frame, aspect: f32) -> Spot {
    let face = Face { font: FONT_PX, need: MIN_CONTRAST };
    Spot(Layout::ALL.into_iter().flat_map(|layout| frame.spot(aspect, layout, |_, height| (0.0, (BAND_PX / height).min(1.0)), |_, _| (0.0, 1.0), |_| face).0).collect())
}

/// The ink that clears AA over the band in every layout: light if it does,
/// else dark if it does, else `None`.
pub fn header_ink(grid: &Ground, aspect: f32, frame: &Frame) -> Option<Ink> {
    let tones = grid.tones(&band(frame, aspect), None, false);
    let inks = Inks::words();
    if tones.over_lightest(inks.light) >= MIN_CONTRAST {
        Some(Ink::Light)
    } else if tones.over_darkest(inks.dark) >= MIN_CONTRAST {
        Some(Ink::Dark)
    } else {
        None
    }
}

/// `data-header-ink` for the light picture and `data-header-ink-night` for the
/// dark theme's (the `dark=` picture, or the one picture again). An attribute
/// is absent where no ink clears; the stylesheet then washes the band.
pub fn header_ink_attributes(lookup: Option<&MediaDimensionLookup>, light: &str, dark: Option<&str>, frame: Frame) -> String {
    let Some(lookup) = lookup else { return String::new() };
    let read = |href: &str| {
        let (grid, aspect) = super::measure(lookup, href)?;
        header_ink(&grid, aspect, &frame)
    };
    let day = read(light);
    let night = dark.map_or(day, |d| read(d));
    let mut out = String::new();
    for (suffix, ink) in [("", day), ("-night", night)] {
        if let Some(ink) = ink {
            out.push_str(&format!(r#" data-header-ink{suffix}="{}""#, ink.as_str()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};

    /// A 100 by 40 picture of one grey with a darker hairline across the top band.
    fn picture(grey: u8, hairline: u8) -> Ground {
        let mut img = RgbImage::from_pixel(100, 40, Rgb([grey; 3]));
        for x in 0..100 {
            img.put_pixel(x, 4, Rgb([hairline; 3]));
        }
        Ground::from_image(&DynamicImage::ImageRgb8(img))
    }

    fn frame() -> Frame {
        Frame::new(None, None, None, false, false, super::super::Words::default())
    }

    #[test]
    fn a_pale_picture_takes_dark_ink_and_a_dark_one_keeps_light() {
        assert_eq!(header_ink(&picture(245, 232), 0.4, &frame()), Some(Ink::Dark));
        assert_eq!(header_ink(&picture(30, 40), 0.4, &frame()), Some(Ink::Light));
    }

    #[test]
    fn a_mid_tone_picture_takes_neither() {
        assert_eq!(header_ink(&picture(125, 125), 0.4, &frame()), None);
    }
}
