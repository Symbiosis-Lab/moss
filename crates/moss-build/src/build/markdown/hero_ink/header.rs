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

/// The ink a header laid over an opening picture takes in each theme; `None`
/// where no ink clears and the stylesheet's strong wash carries white.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HeaderOver {
    pub ink: Option<Ink>,
    pub ink_night: Option<Ink>,
}

impl HeaderOver {
    /// `data-header-ink` and `-night`, absent where no ink clears: the attributes
    /// the stylesheet reads on the picture's block (its wash) and on `<header>`.
    pub fn ink_attrs(&self) -> String {
        let mut out = String::new();
        for (suffix, ink) in [("", self.ink), ("-night", self.ink_night)] {
            if let Some(ink) = ink {
                out.push_str(&format!(r#" data-header-ink{suffix}="{}""#, ink.as_str()));
            }
        }
        out
    }

    /// The attributes `<header>` carries: the one hook the stylesheet selects.
    pub fn header_attrs(&self) -> String {
        format!(r#" data-header="over"{}"#, self.ink_attrs())
    }
}

/// The ink for a band of pictures the header lies across: the one every
/// picture takes, else none. The pictures stand side by side, so each is read
/// at its share of the width.
pub fn ink_over_band(lookup: Option<&MediaDimensionLookup>, hrefs: &[String]) -> HeaderOver {
    let Some(lookup) = lookup else { return HeaderOver::default() };
    let frame = Frame::new(None, None, None, false, false, super::Words::default());
    let share = hrefs.len() as f32;
    let inks: Vec<Option<Ink>> = hrefs
        .iter()
        .map(|href| {
            let (grid, aspect) = super::measure(lookup, href)?;
            header_ink(&grid, aspect / share, &frame)
        })
        .collect();
    let ink = shared_ink(&inks);
    HeaderOver { ink, ink_night: ink }
}

/// The ink every picture takes; none where one picture takes another or none.
fn shared_ink(inks: &[Option<Ink>]) -> Option<Ink> {
    let first = (*inks.first()?)?;
    inks.iter().all(|ink| *ink == Some(first)).then_some(first)
}

/// The ink for the light picture and for the dark theme's (the `dark=`
/// picture, or the one picture again).
pub fn ink_over_hero(lookup: Option<&MediaDimensionLookup>, light: &str, dark: Option<&str>, frame: Frame) -> HeaderOver {
    let Some(lookup) = lookup else { return HeaderOver::default() };
    let read = |href: &str| {
        let (grid, aspect) = super::measure(lookup, href)?;
        header_ink(&grid, aspect, &frame)
    };
    let day = read(light);
    HeaderOver { ink: day, ink_night: dark.map_or(day, |d| read(d)) }
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
    fn a_band_takes_an_ink_only_where_every_picture_does() {
        let inks = |greys: &[u8]| greys.iter().map(|g| header_ink(&picture(*g, *g), 0.4, &frame())).collect::<Vec<_>>();
        assert_eq!(shared_ink(&inks(&[245, 250])), Some(Ink::Dark));
        assert_eq!(shared_ink(&inks(&[30, 40])), Some(Ink::Light));
        assert_eq!(shared_ink(&inks(&[245, 30])), None, "a pale and a dark picture leave the wash to carry light ink");
        assert_eq!(shared_ink(&inks(&[245, 125])), None);
        assert_eq!(shared_ink(&[]), None);
    }

    #[test]
    fn a_mid_tone_picture_takes_neither() {
        assert_eq!(header_ink(&picture(125, 125), 0.4, &frame()), None);
    }
}
