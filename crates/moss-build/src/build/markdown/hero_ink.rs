//! Whether a hero's words can sit straight on its picture, in ink, with no
//! panel behind them.
//!
//! The scan measures each picture once, on a grid of horizontal bands by
//! vertical slices ([`ToneGrid`]). A hero asks for the cells its words can land
//! on in each of the three layouts moss draws ([`Layout`]) and for its foot.
//! Where dark or light ink clears WCAG AA (4.5:1) against the worst case of
//! those cells, a panel only hides the picture; the stylesheet applies the
//! panel or the ink per layout.
//!
//! A cell's worst case is its 5th-percentile luminance for dark ink and its
//! 95th for light ink, so a few stray lines do not decide it but a hatched or
//! dark patch does. The cells are those the words box covers under that
//! layout's crop (`object-fit: cover`, top-anchored), with the box's width the
//! content's own (`max(600px, 35%)`). Pages are taken as left-to-right, which
//! is what moss emits; a right-to-left page should set `panel=always`.

use image::DynamicImage;

use crate::build::components::color_extract::hero_tone_color_luminance;
use crate::build::scan::wcag::{contrast_ratio, relative_luminance};

/// The colour the words are set in, with no panel behind them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Dark,
    Light,
}

impl Ink {
    pub fn as_str(self) -> &'static str {
        match self {
            Ink::Dark => "dark",
            Ink::Light => "light",
        }
    }
}

/// The layouts the words are drawn in, each with its own crop and words box: a
/// phone (under 48rem, words edge to edge), a narrow desktop (48 to 80rem,
/// where the 600px box is most of the width) and a wide one. A closing
/// picture's words start at the text column, which is wider on a
/// `content_width: full` page, so the wide layout is measured twice: for the
/// other pages (`wide`) and for a full-width page (`wide-full`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Narrow,
    Mid,
    Wide,
    WideFull,
}

impl Layout {
    pub const ALL: [Layout; 4] = [Layout::Narrow, Layout::Mid, Layout::Wide, Layout::WideFull];

    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Narrow => "narrow",
            Layout::Mid => "mid",
            Layout::Wide => "wide",
            Layout::WideFull => "wide-full",
        }
    }

    /// The narrowest viewport the layout is drawn at, in px: the `48rem` and
    /// `80rem` media queries of site.css at the default 16px root.
    fn min_width(self) -> f32 {
        match self {
            Layout::Narrow => 320.0,
            Layout::Mid => 768.0,
            Layout::Wide | Layout::WideFull => 1280.0,
        }
    }

    /// How far across the picture the words box can reach, as a share of the
    /// width, at the layout's narrowest viewport (768px and 1280px). A hero's
    /// box is 600px from the start; a closing picture's box starts where the
    /// text column does, less the box's own padding: 27px in at 768px, 229px in
    /// at 1280px on a default-width page and 8px in on a full-width one.
    fn box_share(self, closing: bool) -> f32 {
        match (self, closing) {
            (Layout::Narrow, _) => 1.0,
            (Layout::Mid, false) => 600.0 / 768.0,
            (Layout::Wide | Layout::WideFull, false) => 600.0 / 1280.0,
            (Layout::Mid, true) => 627.0 / 768.0,
            (Layout::Wide, true) => 829.0 / 1280.0,
            (Layout::WideFull, true) => 608.0 / 1280.0,
        }
    }
}

/// A span as fractions of the picture's height or width.
pub type Span = (f32, f32);

/// The part of the picture a piece of text can cover.
#[derive(Clone, Debug)]
pub struct Spot {
    rows: Span,
    cols: Vec<Span>,
}

/// The most of a box the words can cover: on a phone the bottom 40% is kept
/// clear (`closing-picture` gate).
const WORDS_SHARE: f32 = 0.6;
/// How tall a closing picture's words box is at most on a desktop (22rem): a
/// heading, a row of links and the box's padding.
const WORDS_PX: f32 = 352.0;
/// How tall an ordinary hero's box is on a desktop, as a share of the
/// viewport width. The picture is cropped from the foot up, so a picture
/// taller than that shows only the top of it.
const DESKTOP_BOX_RATIO: f32 = 0.42;
/// The rows of a box a block `share` of its height tall covers, for `align`.
fn words_span(align: Option<&str>, share: f32) -> Span {
    match align {
        Some("top") => (0.0, share),
        Some("center") => ((1.0 - share) / 2.0, (1.0 + share) / 2.0),
        _ => (1.0 - share, 1.0),
    }
}

/// Where the words can land for `align` in `layout`, on a picture `aspect`
/// tall (height over width).
pub fn words_spot(align: Option<&str>, aspect: f32, layout: Layout, closing: bool) -> Spot {
    let aspect = aspect.max(0.01);
    let rows = match (layout, closing) {
        (Layout::Narrow, _) => words_span(align, WORDS_SHARE),
        (_, false) => {
            let shown = (DESKTOP_BOX_RATIO / aspect).min(1.0);
            let span = words_span(align, WORDS_SHARE);
            (span.0 * shown, span.1 * shown)
        }
        // The box is the picture's own shape, so nothing is cropped.
        (_, true) => words_span(align, (WORDS_PX / (layout.min_width() * aspect)).min(WORDS_SHARE)),
    };
    let share = layout.box_share(closing);
    let cols = match align {
        _ if layout == Layout::Narrow => vec![(0.0, 1.0)],
        Some("end") => vec![(1.0 - share, 1.0)],
        Some("center") => {
            let share = layout.box_share(false);
            vec![((1.0 - share) / 2.0, (1.0 + share) / 2.0)]
        }
        _ => vec![(0.0, share)],
    };
    Spot { rows, cols }
}

/// A closing picture's foot (credit and colophon): the bottom 18% of the box,
/// across the width.
pub fn foot_spot() -> Spot {
    Spot { rows: (0.82, 1.0), cols: vec![(0.0, 1.0)] }
}

/// WCAG AA for text.
pub const MIN_CONTRAST: f32 = 4.5;

const BANDS: usize = 20;
const SLICES: usize = 10;

/// A picture's luminance extremes (5th and 95th percentile, linear) in each
/// cell of twenty horizontal bands by ten vertical slices.
#[derive(Clone, Debug, PartialEq)]
pub struct ToneGrid(Vec<(f32, f32)>);

impl ToneGrid {
    pub fn from_image(img: &DynamicImage) -> Self {
        let rgb = img.thumbnail(96, 96).to_rgb8();
        let (w, h) = (rgb.width() as usize, rgb.height() as usize);
        let lum: Vec<f32> = rgb
            .pixels()
            .map(|p| relative_luminance(p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0))
            .collect();
        let span = |i: usize, n: usize, len: usize| {
            let from = (i * len / n).min(len.saturating_sub(1));
            (from, ((i + 1) * len).div_ceil(n).clamp(from + 1, len.max(1)))
        };
        let cells = (0..BANDS)
            .flat_map(|band| (0..SLICES).map(move |slice| (band, slice)))
            .map(|(band, slice)| {
                let (r0, r1) = span(band, BANDS, h);
                let (c0, c1) = span(slice, SLICES, w);
                let mut seen: Vec<f32> = (r0..r1).flat_map(|r| lum[r * w + c0..r * w + c1].to_vec()).collect();
                seen.sort_by(f32::total_cmp);
                let at = |q: f32| seen[((seen.len() - 1) as f32 * q).round() as usize];
                (at(0.05), at(0.95))
            })
            .collect();
        Self(cells)
    }

    /// `lo,hi;lo,hi;…` per cell, in thousandths: what the scan caches.
    pub fn encode(&self) -> String {
        let q = |v: f32| (v * 1000.0).round() as u32;
        self.0.iter().map(|(lo, hi)| format!("{},{}", q(*lo), q(*hi))).collect::<Vec<_>>().join(";")
    }

    pub fn parse(encoded: &str) -> Option<Self> {
        let cells: Vec<(f32, f32)> = encoded
            .split(';')
            .map(|b| {
                let (lo, hi) = b.split_once(',')?;
                Some((lo.parse::<f32>().ok()? / 1000.0, hi.parse::<f32>().ok()? / 1000.0))
            })
            .collect::<Option<_>>()?;
        (cells.len() == BANDS * SLICES).then_some(Self(cells))
    }

    /// The worst luminances over every cell a spot touches: the darkest dark
    /// pixels and the lightest light ones.
    pub fn extremes(&self, spot: &Spot) -> (f32, f32) {
        let range = |(from, to): Span, n: usize| {
            let first = ((from.clamp(0.0, 1.0) * n as f32).floor() as usize).min(n - 1);
            first..((to.clamp(0.0, 1.0) * n as f32).ceil() as usize).clamp(first + 1, n)
        };
        let mut worst = (f32::MAX, f32::MIN);
        for band in range(spot.rows, BANDS) {
            for cols in &spot.cols {
                for slice in range(*cols, SLICES) {
                    let (lo, hi) = self.0[band * SLICES + slice];
                    worst = (worst.0.min(lo), worst.1.max(hi));
                }
            }
        }
        worst
    }
}

/// The ink that reads better over a spot reaching `(darkest, lightest)`, and
/// its contrast against the worst case.
pub fn better_ink((darkest, lightest): (f32, f32)) -> (Ink, f32) {
    let dark = contrast_ratio(darkest, hero_tone_color_luminance());
    let light = contrast_ratio(lightest, 1.0);
    if dark >= light { (Ink::Dark, dark) } else { (Ink::Light, light) }
}

/// The ink whose worst case over `spot` clears [`MIN_CONTRAST`], if either.
pub fn readable_ink(grid: &ToneGrid, spot: &Spot) -> Option<Ink> {
    let (ink, contrast) = better_ink(grid.extremes(spot));
    (contrast >= MIN_CONTRAST).then_some(ink)
}

/// How an overlay hero's words are backed (`panel=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backing {
    /// The tinted panel, as every overlay hero has had.
    Panel,
    /// Ink straight on the picture where it reads there, the panel elsewhere.
    Auto,
    /// Ink straight on the picture, whatever it measures.
    Never,
}

impl Backing {
    /// What `panel=` asks for. Unset means the panel, except on a closing
    /// picture, which measures; an unrecognised value is the unset case (the
    /// parser warns).
    pub fn of(panel: Option<&str>, closing: bool) -> Self {
        match panel {
            Some("always") => Backing::Panel,
            Some("auto") => Backing::Auto,
            Some("none") => Backing::Never,
            _ if closing => Backing::Auto,
            _ => Backing::Panel,
        }
    }
}

/// One picture's measurement: its grid and its height over its width.
fn measure(lookup: &crate::build::media::dimensions::MediaDimensionLookup, href: &str) -> Option<(ToneGrid, f32)> {
    let key = href.strip_prefix('/').unwrap_or(href);
    let (w, h) = lookup.get(key);
    Some((lookup.tone_grid(key)?, h as f32 / w.max(1) as f32))
}

/// The attributes that carry each picture's measured ink to the stylesheet.
/// `data-words` lists the layouts where the words sit in ink with no panel
/// (`narrow:dark wide:light`; a layout not listed keeps the panel). `data-foot`
/// is the ink along the foot for the credit and colophon (absent: the credit
/// gets a small panel). The `-night` pair says the same of the `dark=` picture,
/// or repeats the light one when there is none. Empty when nothing was
/// measured.
pub fn ink_attributes(
    lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    light: &str,
    dark: Option<&str>,
    align: Option<&str>,
    backing: Backing,
    closing: bool,
) -> String {
    let Some(lookup) = lookup else { return String::new() };
    let reading = |href: &str| {
        let (grid, aspect) = measure(lookup, href)?;
        let tokens: Vec<String> = Layout::ALL
            .into_iter()
            .filter(|layout| closing || *layout != Layout::WideFull)
            .filter_map(|layout| {
                let spot = words_spot(align, aspect, layout, closing);
                let ink = match backing {
                    Backing::Panel => None,
                    Backing::Auto => readable_ink(&grid, &spot),
                    Backing::Never => Some(better_ink(grid.extremes(&spot)).0),
                }?;
                Some(format!("{}:{}", layout.as_str(), ink.as_str()))
            })
            .collect();
        Some((tokens.join(" "), closing.then(|| readable_ink(&grid, &foot_spot())).flatten()))
    };
    let day = reading(light);
    // Without a `dark=` picture the one picture is shown in both themes.
    let night = dark.map_or(day.clone(), reading);
    let mut out = String::new();
    for (suffix, picture) in [("", day), ("-night", night)] {
        if let Some((words, foot)) = picture {
            if !words.is_empty() {
                out.push_str(&format!(r#" data-words{suffix}="{words}""#));
            }
            if let Some(ink) = foot {
                out.push_str(&format!(r#" data-foot{suffix}="{}""#, ink.as_str()));
            }
        }
    }
    out
}

/// For `panel=none`: the best contrast either ink reaches under the words, in
/// the layout where it is lowest, when that is under [`MIN_CONTRAST`] (the
/// author asked for ink that may not read).
pub fn forced_ink_shortfall(
    lookup: &crate::build::media::dimensions::MediaDimensionLookup,
    hrefs: &[&str],
    align: Option<&str>,
    closing: bool,
) -> Option<f32> {
    hrefs
        .iter()
        .filter_map(|href| measure(lookup, href))
        .flat_map(|(grid, aspect)| Layout::ALL.map(|layout| better_ink(grid.extremes(&words_spot(align, aspect, layout, closing))).1))
        .filter(|contrast| *contrast < MIN_CONTRAST)
        .min_by(f32::total_cmp)
}

/// Warn for each `panel=none` hero whose picture reads under 4.5:1 in either
/// ink where its words land: the author asked for ink with no panel, and it
/// may not be legible. Names the page and the hero's opening words.
pub fn warn_unreadable_forced_ink(
    doc: &moss_core::ast::Document,
    lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    file_path: &str,
) {
    use moss_core::ast::{Block, Shortcode, Url};
    let Some(lookup) = lookup else { return };
    let href = |u: &Url| match u {
        Url::Resolved(r) => Some(r.href.clone()),
        Url::Unresolved(_) => None,
    };
    for block in &doc.blocks {
        let Block::Shortcode(Shortcode::Hero(hero)) = block else { continue };
        if hero.panel.as_deref() != Some("none") || hero.overlay.is_empty() {
            continue;
        }
        let hrefs: Vec<String> = hero.image.iter().chain(hero.dark.iter()).filter_map(href).collect();
        let hrefs: Vec<&str> = hrefs.iter().map(String::as_str).collect();
        if let Some(contrast) = forced_ink_shortfall(lookup, &hrefs, hero.align.as_deref(), hero.classes.split_whitespace().any(|c| c == "moss-hero-closing")) {
            let words: String = hero.overlay_text.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
            crate::build::cli_output::log_warn_problem!(
                "[{file_path}] `:::hero {{panel=none}}` (\"{words}\"): neither dark nor light ink reaches {MIN_CONTRAST}:1 over the picture where the words sit (best {contrast:.1}:1), so they may be hard to read. Use `panel=auto` to keep the panel there."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::media::dimensions::MediaDimensionLookup;
    use crate::types::content::MediaMetadata;
    use image::{Rgb, RgbImage};

    /// A 160x70 picture: `top` over the first `split` of its rows, `bottom` below.
    fn banded(top: [u8; 3], bottom: [u8; 3], split: f32) -> DynamicImage {
        let mut img = RgbImage::new(160, 70);
        for (_, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb(if (y as f32) < 70.0 * split { top } else { bottom });
        }
        DynamicImage::ImageRgb8(img)
    }

    const SKY: [u8; 3] = [205, 218, 232];
    const GROUND: [u8; 3] = [30, 40, 60];
    const INK: [u8; 3] = [10, 14, 24];

    /// A sky with a dark patch over the right fifth of its top rows.
    fn sky_with_far_patch() -> DynamicImage {
        let mut img = RgbImage::from_pixel(160, 70, Rgb(SKY));
        for y in 0..21 {
            for x in 128..160 {
                img.put_pixel(x, y, Rgb(INK));
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    fn lookup_of(pictures: &[(&str, &DynamicImage)]) -> MediaDimensionLookup {
        let images: Vec<MediaMetadata> = pictures
            .iter()
            .map(|(path, img)| MediaMetadata {
                path: path.to_string(),
                dimensions: Some((img.width(), img.height())),
                tone_bands: Some(ToneGrid::from_image(img).encode()),
                ..Default::default()
            })
            .collect();
        MediaDimensionLookup::new(&images, &[], &Default::default(), None)
    }

    #[test]
    fn the_grid_survives_the_cache_encoding() {
        let grid = ToneGrid::from_image(&banded(SKY, GROUND, 0.7));
        let again = ToneGrid::parse(&grid.encode()).unwrap();
        let all = Spot { rows: (0.0, 1.0), cols: vec![(0.0, 1.0)] };
        let (a, b) = (grid.extremes(&all), again.extremes(&all));
        assert!((a.0 - b.0).abs() < 0.001 && (a.1 - b.1).abs() < 0.001);
        assert!(ToneGrid::parse("1,2").is_none());
    }

    #[test]
    fn a_pale_sky_takes_dark_ink_and_its_dark_ground_takes_light() {
        let grid = ToneGrid::from_image(&banded(SKY, GROUND, 0.7));
        for layout in Layout::ALL {
            assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, layout, false)), Some(Ink::Dark));
        }
        assert_eq!(readable_ink(&grid, &foot_spot()), Some(Ink::Light));
    }

    #[test]
    fn a_mid_tone_area_takes_neither() {
        let grid = ToneGrid::from_image(&banded([120; 3], [120; 3], 0.7));
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, false)), None);
    }

    #[test]
    fn a_far_dark_patch_spares_the_wide_box_but_not_a_phone() {
        let grid = ToneGrid::from_image(&sky_with_far_patch());
        // Wide: the words box is the start 47% of the width; the patch is at the far end.
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, false)), Some(Ink::Dark));
        // `align=end` words sit at the foot of the picture, below the patch.
        assert_eq!(readable_ink(&grid, &words_spot(Some("end"), 0.4, Layout::Wide, false)), Some(Ink::Dark));
        // A phone runs the words across the whole width.
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Narrow, false)), None);
    }

    #[test]
    fn a_picture_taller_than_the_desktop_box_is_measured_only_where_it_shows() {
        // Wide crop shows the top 42/80 of a tall picture: a dark band lower down does not count there.
        let mut img = RgbImage::from_pixel(100, 80, Rgb(SKY));
        for y in 56..80 {
            for x in 0..100 {
                img.put_pixel(x, y, Rgb(INK));
            }
        }
        let grid = ToneGrid::from_image(&DynamicImage::ImageRgb8(img));
        assert_eq!(readable_ink(&grid, &words_spot(None, 0.8, Layout::Wide, false)), Some(Ink::Dark));
        assert_eq!(readable_ink(&grid, &words_spot(None, 0.8, Layout::Narrow, false)), None);
    }

    #[test]
    fn a_closing_picture_is_measured_uncropped_over_the_words_real_height() {
        // 0.8 tall: the box is the whole picture (1024px at 1280), and the words take 352px of it, 34%.
        let band = |from: u32, to: u32| {
            let mut img = RgbImage::from_pixel(100, 80, Rgb(SKY));
            for y in from..to {
                for x in 0..100 {
                    img.put_pixel(x, y, Rgb(INK));
                }
            }
            ToneGrid::from_image(&DynamicImage::ImageRgb8(img))
        };
        let top = words_spot(Some("top"), 0.8, Layout::Wide, true);
        assert_eq!(readable_ink(&band(40, 80), &top), Some(Ink::Dark));
        assert_eq!(readable_ink(&band(16, 24), &top), None);
        assert_eq!(readable_ink(&band(0, 40), &foot_spot()), Some(Ink::Dark));
        assert_eq!(readable_ink(&band(70, 80), &foot_spot()), None);
    }

    #[test]
    fn a_full_width_page_reaches_less_far_than_a_default_one() {
        // The wide box on a default-width page runs to 65% of the width, on a full-width page to 48%.
        let mut img = RgbImage::from_pixel(100, 40, Rgb(SKY));
        for y in 0..12 {
            for x in 55..100 {
                img.put_pixel(x, y, Rgb(INK));
            }
        }
        let grid = ToneGrid::from_image(&DynamicImage::ImageRgb8(img));
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, true)), None);
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::WideFull, true)), Some(Ink::Dark));
    }

    #[test]
    fn a_closing_picture_is_measured_to_where_its_words_can_reach() {
        // A dark patch beyond the Wide reach (65% of the width) spares the words; one within it does not.
        let patch = |from: u32| {
            let mut img = RgbImage::from_pixel(100, 40, Rgb(SKY));
            for y in 0..12 {
                for x in from..100 {
                    img.put_pixel(x, y, Rgb(INK));
                }
            }
            ToneGrid::from_image(&DynamicImage::ImageRgb8(img))
        };
        let spot = |closing| words_spot(Some("top"), 0.4, Layout::Wide, closing);
        assert_eq!(readable_ink(&patch(60), &spot(false)), Some(Ink::Dark));
        assert_eq!(readable_ink(&patch(60), &spot(true)), None);
        assert_eq!(readable_ink(&patch(80), &spot(true)), Some(Ink::Dark));
    }

    #[test]
    fn each_layout_gets_its_own_token() {
        let lookup = lookup_of(&[("far.png", &sky_with_far_patch())]);
        let attrs = ink_attributes(Some(&lookup), "/far.png", None, Some("top"), Backing::Auto, false);
        assert!(attrs.contains(r#"data-words="wide:dark""#), "no `wide-full` for an ordinary hero: {attrs}");
        assert!(!attrs.contains("narrow:"), "{attrs}");
        assert!(attrs.contains(r#"data-words-night="#), "{attrs}");
    }

    #[test]
    fn a_hero_without_a_dark_picture_repeats_its_reading_for_night() {
        let lookup = lookup_of(&[("day.png", &banded(SKY, GROUND, 0.7))]);
        let one = ink_attributes(Some(&lookup), "/day.png", None, Some("top"), Backing::Auto, true);
        let tokens = r#"="narrow:dark mid:dark wide:dark wide-full:dark""#;
        assert!(one.contains(&format!("data-words{tokens}")) && one.contains(&format!("data-words-night{tokens}")), "{one}");
        assert!(one.contains(r#"data-foot="light""#) && one.contains(r#"data-foot-night="light""#), "{one}");
    }

    #[test]
    fn a_dark_picture_is_measured_apart_from_the_light_one() {
        let lookup = lookup_of(&[("day.png", &banded(SKY, GROUND, 0.7)), ("night.png", &banded([20, 28, 48], [230, 235, 240], 0.7))]);
        let pair = ink_attributes(Some(&lookup), "/day.png", Some("/night.png"), Some("top"), Backing::Auto, true);
        assert!(pair.contains(r#"data-words="narrow:dark"#) && pair.contains(r#"data-words-night="narrow:light"#), "{pair}");
    }

    #[test]
    fn nothing_is_measured_without_a_scan_or_for_a_hero_that_keeps_its_panel() {
        let lookup = lookup_of(&[("day.png", &banded(SKY, GROUND, 0.7))]);
        assert_eq!(ink_attributes(Some(&lookup), "/other.png", None, None, Backing::Auto, true), "");
        assert_eq!(ink_attributes(None, "/day.png", None, None, Backing::Auto, true), "");
        assert!(!ink_attributes(Some(&lookup), "/day.png", None, None, Backing::Panel, false).contains("data-words"));
    }

    #[test]
    fn a_slugged_folder_is_found_under_its_output_url() {
        let img = banded(SKY, GROUND, 0.7);
        let meta = MediaMetadata {
            path: "評選/art.png".into(),
            dimensions: Some((160, 70)),
            tone_bands: Some(ToneGrid::from_image(&img).encode()),
            ..Default::default()
        };
        let overrides = std::collections::HashMap::from([("評選".to_string(), "awards".to_string())]);
        let lookup = MediaDimensionLookup::new(&[meta], &[], &overrides, None);
        let attrs = ink_attributes(Some(&lookup), "/awards/art.png", None, Some("top"), Backing::Auto, false);
        assert!(attrs.contains("data-words="), "{attrs}");
    }

    #[test]
    fn forcing_ink_over_a_mid_tone_reports_the_shortfall() {
        let lookup = lookup_of(&[("mid.png", &banded([120; 3], [120; 3], 0.7)), ("day.png", &banded(SKY, GROUND, 0.7))]);
        assert!(forced_ink_shortfall(&lookup, &["/mid.png"], Some("top"), false).is_some_and(|c| c < MIN_CONTRAST));
        assert!(forced_ink_shortfall(&lookup, &["/day.png"], Some("top"), false).is_none());
    }
}
