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
//!
//! A closing picture's crop is build data ([`Frame`]): its `max-height=` and
//! the position its `image=` names, so the cells are those its box shows
//! ([`Crop`]) at each layout. A closing picture with `fade=top` shows the page
//! through its top. There each cell is composited over the page colour of the
//! theme being read ([`PageGround`]) before it is measured. The credit sits at
//! the foot, in whichever of the theme's secondary text colour and its page
//! colour reads there ([`CreditInk`]).

use image::DynamicImage;

use crate::build::components::color_extract::hero_tone_color_luminance;
use crate::build::scan::section_text::{luminance, Rgb, BACKGROUNDS};
use crate::build::scan::wcag::{contrast_ratio, linear_to_srgb, relative_luminance, srgb_to_linear};

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

    /// The window widths the layout spans: from the `48rem` and `80rem` media
    /// queries of site.css at the default 16px root, the widest taken as 2560px.
    fn widths(self) -> (f32, f32) {
        match self {
            Layout::Narrow => (320.0, 767.0),
            Layout::Mid => (768.0, 1279.0),
            Layout::Wide | Layout::WideFull => (1280.0, 2560.0),
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

/// The part of the picture a piece of text can cover: one area per window
/// size measured.
#[derive(Clone, Debug)]
pub struct Spot(Vec<Area>);

/// Rows and columns of the picture, with the rows its box shows there, which
/// place the fade (it is drawn over the box, not the picture).
#[derive(Clone, Copy, Debug)]
struct Area {
    rows: Span,
    cols: Span,
    shown: Span,
}

impl Spot {
    /// A spot on a picture its box shows whole.
    fn uncropped(rows: Span, cols: Span) -> Self {
        Spot(vec![Area { rows, cols, shown: (0.0, 1.0) }])
    }
}

/// The part of a picture its box shows under `object-fit: cover`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    rows: Span,
    cols: Span,
}

impl Crop {
    /// What a box `w` by `h` px shows of a picture `aspect` tall (height over
    /// width), anchored at `position` (`object-position`, as fractions).
    pub fn cover(aspect: f32, (w, h): (f32, f32), (x, y): (f32, f32)) -> Self {
        let window = |shown: f32, at: f32| {
            let shown = shown.clamp(0.0, 1.0);
            ((1.0 - shown) * at, (1.0 - shown) * at + shown)
        };
        let natural = w * aspect.max(0.01);
        if natural >= h {
            Crop { rows: window(h / natural, y), cols: (0.0, 1.0) }
        } else {
            Crop { rows: (0.0, 1.0), cols: window(natural / h.max(1.0), x) }
        }
    }

    /// The picture under `rows` and `cols` of the box.
    fn area(self, rows: Span, cols: Span) -> Area {
        let within = |(a, b): Span, (from, to): Span| (a + from * (b - a), a + to * (b - a));
        Area { rows: within(self.rows, rows), cols: within(self.cols, cols), shown: self.rows }
    }
}

/// How a hero's picture fills its box, as the build set it: the position its
/// `image=` names (or site.css's default) and, on a closing picture, its
/// `max-height=` and `fade=top`. A theme's own `--moss-hero-object-position`
/// or `--moss-hero-max-height` is not seen. The crop is taken as `cover`, the
/// hero's fit; a `|contain` picture is measured as if covered, which reads its
/// pixels but not the bars a contained picture leaves.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// `object-position` as fractions.
    position: (f32, f32),
    max_height: Option<moss_core::media::MaxHeight>,
    /// `fade=top`: the page shows through the top, and a phone's box keeps
    /// [`PHONE_FADE_FLOOR_PX`].
    pub faded: bool,
    /// A closing picture, whose box is its own shape or its cap. An ordinary
    /// hero's box is [`DESKTOP_BOX_RATIO`] of the width from 48rem.
    pub closing: bool,
}

/// A faded closing picture's least height on a phone (38rem), so its words
/// sit on the faded top.
pub const PHONE_FADE_FLOOR_PX: f32 = 608.0;

impl Frame {
    /// The frame `position`, `max_height`, `align` and `fade=top` make. The
    /// default position is site.css's: a closing picture with `align=top`
    /// keeps its foot, every other hero its top. Only a closing picture is
    /// capped or faded.
    pub fn new(position: Option<moss_core::media::Position>, max_height: Option<moss_core::media::MaxHeight>, align: Option<&str>, faded: bool, closing: bool) -> Self {
        let position = match position.map(|p| p.percentages()) {
            Some((x, y)) => (f32::from(x) / 100.0, f32::from(y) / 100.0),
            None if closing && align == Some("top") => (0.5, 1.0),
            None => (0.5, 0.0),
        };
        Frame { position, max_height: max_height.filter(|_| closing), faded: faded && closing, closing }
    }

    /// The frame of a hero read straight from its fence.
    pub fn of(hero: &moss_core::ast::HeroShortcode, closing: bool) -> Self {
        let position = moss_core::media::parse_media_attrs(&hero.attrs).position;
        Frame::new(position, hero.max_height, hero.align.as_deref(), hero.fade.as_deref() == Some("top"), closing)
    }

    /// The box's height at `width` px in `layout`. A phone's box without the
    /// fade is set by its words and the room below them; it is taken as the
    /// picture's own shape.
    fn box_height(&self, aspect: f32, layout: Layout, width: f32) -> f32 {
        let natural = width * aspect.max(0.01);
        let cap = self.max_height.map(|c| c.px(width));
        match layout {
            Layout::Narrow if self.faded => cap.unwrap_or(0.0).max(PHONE_FADE_FLOOR_PX),
            Layout::Narrow => natural,
            _ if self.closing => cap.unwrap_or(natural),
            _ => DESKTOP_BOX_RATIO * width,
        }
    }

    /// The picture under the box rows `rows(width, height)` and columns
    /// `cols`, over the layout's widths: each measured window adds its area,
    /// since a crop that is not the picture's own shape shows a different part
    /// at each width.
    fn spot(&self, aspect: f32, layout: Layout, rows: impl Fn(f32, f32) -> Span, cols: Span) -> Spot {
        let (from, to) = layout.widths();
        let areas = (0..=4)
            .map(|i| from + (to - from) * i as f32 / 4.0)
            .map(|width| {
                let height = self.box_height(aspect, layout, width);
                Crop::cover(aspect, (width, height), self.position).area(rows(width, height), cols)
            })
            .collect();
        Spot(areas)
    }
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
/// tall (height over width) filling its box as `frame` says.
pub fn words_spot(align: Option<&str>, aspect: f32, layout: Layout, frame: &Frame) -> Spot {
    let share = layout.box_share(frame.closing);
    let cols = match align {
        _ if layout == Layout::Narrow => (0.0, 1.0),
        Some("end") => (1.0 - share, 1.0),
        Some("center") => {
            let share = layout.box_share(false);
            ((1.0 - share) / 2.0, (1.0 + share) / 2.0)
        }
        _ => (0.0, share),
    };
    let share = |height: f32| if frame.closing && layout != Layout::Narrow { (WORDS_PX / height).min(WORDS_SHARE) } else { WORDS_SHARE };
    frame.spot(aspect, layout, |_, height| words_span(align, share(height)), cols)
}

/// A closing picture's foot (credit and colophon): the bottom 18% of the box,
/// across the width, at every layout.
pub fn foot_spot(frame: &Frame, aspect: f32) -> Spot {
    Spot(Layout::ALL.iter().flat_map(|layout| frame.spot(aspect, *layout, |_, _| (0.82, 1.0), (0.0, 1.0)).0).collect())
}

/// How tall the credit's three small lines are, with their 0.3rem padding.
const CREDIT_PX: f32 = 72.0;

/// Where a closing picture's edition block can sit. Align top puts it under the
/// words, which reach at most `WORDS_PX`, so the band runs from the box's top
/// to that plus the block's height. Any other alignment, and a phone's column,
/// can put it anywhere in the box.
const EDITION_PX: f32 = 96.0;

pub fn edition_spot(align: Option<&str>, aspect: f32, layout: Layout, frame: &Frame) -> Spot {
    let rows = |_, height: f32| match (layout, align) {
        (Layout::Narrow, _) => (0.0, 1.0),
        (_, Some("top")) => (0.0, ((WORDS_PX + EDITION_PX) / height).min(1.0)),
        _ => (0.0, 1.0),
    };
    frame.spot(aspect, layout, rows, (0.0, 1.0))
}

/// The page colour behind a faded picture: `--moss-color-bg` in the light and
/// the dark theme, `None` where the theme sets it to a value the build can't
/// read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageGround {
    pub light: Option<Rgb>,
    pub dark: Option<Rgb>,
}

impl Default for PageGround {
    fn default() -> Self {
        let (light, dark) = *BACKGROUNDS;
        Self { light: Some(light), dark: Some(dark) }
    }
}

impl PageGround {
    /// moss's colours as the theme's stylesheet overrides them. Read only
    /// where the authoring guide tells a theme to set a token: `:root {}`,
    /// which wins in both themes, and `:root[data-theme="dark"] {}`. A
    /// declaration anywhere else, or a value that is not a plain colour
    /// (`var()`, `color-mix()`), leaves both themes unresolved.
    pub fn of_theme(theme_css: Option<&str>) -> Self {
        let mut ground = Self::default();
        let (mut root, mut dark) = (None, None);
        for rule in crate::build::theme_lint::style_rules(theme_css.unwrap_or_default()) {
            let Some((_, body)) = rule.rule.split_once('{') else { continue };
            for decl in body.trim_end_matches('}').split(';') {
                let Some((prop, value)) = decl.split_once(':') else { continue };
                if prop.trim() != "--moss-color-bg" {
                    continue;
                }
                let colour = crate::build::components::color_extract::parse_css_color(value.trim().trim_end_matches("!important").trim());
                let scope: String = rule.prelude.chars().filter(|c| !c.is_whitespace()).collect::<String>().replace('\'', "\"");
                match (rule.conditions.is_empty(), scope.as_str()) {
                    (true, ":root") => root = Some(colour),
                    (true, ":root[data-theme=\"dark\"]") => dark = Some(colour),
                    _ => return Self { light: None, dark: None },
                }
            }
        }
        if let Some(colour) = root {
            ground = Self { light: colour, dark: colour };
        }
        if let Some(colour) = dark {
            ground.dark = colour;
        }
        ground
    }
}

/// How far down the box a faded top runs, as site.css draws it: on a phone,
/// and from 48rem.
pub const FADE_DEPTH: (f32, f32) = (0.5, 0.38);

/// The page showing through a faded top: its colour, and how far down the
/// box the fade runs ([`FADE_DEPTH`]).
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    /// The page colour's luminance, as an sRGB grey level: the mask blends in sRGB.
    ground: f32,
    depth: f32,
}

impl Fade {
    pub fn new(ground: Rgb, layout: Layout) -> Self {
        let depth = if layout == Layout::Narrow { FADE_DEPTH.0 } else { FADE_DEPTH.1 };
        Self { ground: linear_to_srgb(luminance(ground)), depth }
    }

    /// A luminance `lum` seen through the mask at `alpha`.
    fn over(self, lum: f32, alpha: f32) -> f32 {
        srgb_to_linear((1.0 - alpha) * self.ground + alpha * linear_to_srgb(lum))
    }
}

/// The ink an edition block takes: one ink that reads in every layout's band,
/// else `None` and the block keeps its panel. `fade_at` gives a layout's fade,
/// `None` where the page colour behind a fade is unresolved.
pub fn edition_ink(grid: &ToneGrid, aspect: f32, align: Option<&str>, frame: &Frame, fade_at: impl Fn(Layout) -> Option<Option<Fade>>) -> Option<Ink> {
    let inks: Vec<Option<Ink>> = Layout::ALL
        .into_iter()
        .map(|layout| readable_ink(grid, &edition_spot(align, aspect, layout, frame), fade_at(layout)?, Inks::words()))
        .collect();
    let first = inks[0]?;
    inks.iter().all(|ink| *ink == Some(first)).then_some(first)
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
    /// pixels and the lightest light ones, seen through `fade` when the top is
    /// faded. The mask is linear down a band, so its two edges bound the band.
    pub fn extremes(&self, spot: &Spot, fade: Option<Fade>) -> (f32, f32) {
        let range = |(from, to): Span, n: usize| {
            let first = ((from.clamp(0.0, 1.0) * n as f32).floor() as usize).min(n - 1);
            first..((to.clamp(0.0, 1.0) * n as f32).ceil() as usize).clamp(first + 1, n)
        };
        let mut worst = (f32::MAX, f32::MIN);
        for area in &spot.0 {
            for band in range(area.rows, BANDS) {
                for slice in range(area.cols, SLICES) {
                    let (mut lo, mut hi) = self.0[band * SLICES + slice];
                    if let Some(fade) = fade {
                        // The mask runs down the box, which shows `area.shown` of the rows.
                        let (from, to) = area.shown;
                        let alpha = |row: usize| ((row as f32 / BANDS as f32 - from) / (to - from).max(0.001) / fade.depth).clamp(0.0, 1.0);
                        let (top, bottom) = (alpha(band), alpha(band + 1));
                        lo = fade.over(lo, top).min(fade.over(lo, bottom));
                        hi = fade.over(hi, top).max(fade.over(hi, bottom));
                    }
                    worst = (worst.0.min(lo), worst.1.max(hi));
                }
            }
        }
        worst
    }
}

/// A dark and a light ink, by luminance.
#[derive(Clone, Copy, Debug)]
pub struct Inks {
    dark: f32,
    light: f32,
}

impl Inks {
    /// The words: `--moss-hero-tone-color` and white.
    pub fn words() -> Self {
        Self { dark: hero_tone_color_luminance(), light: 1.0 }
    }

}

/// The credit's text ink: `--moss-color-text-secondary` in the light and the
/// dark theme, which site.css repeats.
pub static CREDIT_INK: std::sync::LazyLock<(Rgb, Rgb)> = std::sync::LazyLock::new(|| {
    let tokens = moss_core::contract::tokens::load_tokens().unwrap_or_else(|e| panic!("tokens.json must parse: {e}"));
    let token = moss_core::contract::tokens::find_token(&tokens, "moss-color-text-secondary");
    let parse = |hex: Option<&str>| hex.and_then(crate::build::components::color_extract::parse_css_color).expect("--moss-color-text-secondary has a light and a dark colour");
    (parse(token.map(|t| t.0)), parse(token.and_then(|t| t.1)))
});

/// The ink that reads better over a spot reaching `(darkest, lightest)`, and
/// its contrast against the worst case.
pub fn better_ink((darkest, lightest): (f32, f32), inks: Inks) -> (Ink, f32) {
    let dark = contrast_ratio(darkest, inks.dark);
    let light = contrast_ratio(lightest, inks.light);
    if dark >= light { (Ink::Dark, dark) } else { (Ink::Light, light) }
}

/// The ink whose worst case over `spot` clears [`MIN_CONTRAST`], if either.
pub fn readable_ink(grid: &ToneGrid, spot: &Spot, fade: Option<Fade>, inks: Inks) -> Option<Ink> {
    Backing::Auto.accept(better_ink(grid.extremes(spot, fade), inks))
}

/// The ink a closing picture's credit takes at the foot, in one theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreditInk {
    /// The theme's secondary text colour ([`CREDIT_INK`]).
    Text,
    /// The theme's page colour (`--moss-color-bg`, as [`PageGround`] reads it).
    Page,
}

impl CreditInk {
    pub fn as_str(self) -> &'static str {
        match self {
            CreditInk::Text => "text",
            CreditInk::Page => "page",
        }
    }
}

/// Of the theme's `text` and `page` colours, the one that reads better over a
/// spot reaching `(darkest, lightest)`, and its worst contrast there. The page
/// colour is left out where the build cannot read it.
pub fn credit_ink((darkest, lightest): (f32, f32), text: Rgb, page: Option<Rgb>) -> (CreditInk, f32) {
    let worst = |ink: Rgb| contrast_ratio(darkest, luminance(ink)).min(contrast_ratio(lightest, luminance(ink)));
    let text = (CreditInk::Text, worst(text));
    match page.map(|p| (CreditInk::Page, worst(p))) {
        Some(page) if page.1 > text.1 => page,
        _ => text,
    }
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

    /// Whether a measured ink and its worst contrast are used: never under a
    /// panel, when it clears [`MIN_CONTRAST`] when measured, always when forced.
    pub fn accept<T>(self, (ink, contrast): (T, f32)) -> Option<T> {
        match self {
            Backing::Panel => None,
            Backing::Auto => (contrast >= MIN_CONTRAST).then_some(ink),
            Backing::Never => Some(ink),
        }
    }
}

/// One picture's measurement: its grid and its height over its width.
fn measure(lookup: &crate::build::media::dimensions::MediaDimensionLookup, href: &str) -> Option<(ToneGrid, f32)> {
    let key = href.strip_prefix('/').unwrap_or(href);
    let (w, h) = lookup.get(key);
    Some((lookup.tone_grid(key)?, h as f32 / w.max(1) as f32))
}

/// What a hero asks of the measurement besides its pictures.
#[derive(Clone, Copy, Debug)]
pub struct InkRequest<'a> {
    pub align: Option<&'a str>,
    pub backing: Backing,
    pub frame: Frame,
    /// A closing picture's credit (`caption=`).
    pub credit: bool,
    /// The page colour per theme a faded top shows.
    pub ground: PageGround,
}

/// The attributes that carry each picture's measured ink to the stylesheet.
/// `data-words` lists the layouts where the words sit in ink with no panel
/// (`narrow:dark wide:light`; a layout not listed keeps the panel). `data-foot`
/// is the words' ink along the foot for the colophon. `data-credit` is the
/// credit's ink at the foot ([`CreditInk`]; absent, it keeps a small panel).
/// The `-night` set says the same
/// of the dark theme: the `dark=` picture, or the one picture again, over the
/// dark page colour where it fades. A faded theme whose page colour is
/// unresolved measures no words, so they keep their panel.
/// `data-edition` is the ink the edition block takes, measured over its band
/// in every layout (absent: the block keeps its panel). Empty when nothing was
/// measured.
pub fn ink_attributes(
    lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    light: &str,
    dark: Option<&str>,
    request: InkRequest<'_>,
) -> String {
    let Some(lookup) = lookup else { return String::new() };
    let InkRequest { align, backing, frame, credit, ground } = request;
    let closing = frame.closing;
    let reading = |href: &str, page: Option<Rgb>, text: Rgb| {
        let (grid, aspect) = measure(lookup, href)?;
        let fade_at = |layout: Layout| match (frame.faded, page) {
            (false, _) => Some(None),
            (true, Some(page)) => Some(Some(Fade::new(page, layout))),
            (true, None) => None,
        };
        let inked = |spot: &Spot, fade: Option<Fade>, inks: Inks| backing.accept(better_ink(grid.extremes(spot, fade), inks));
        let tokens = |spot: &dyn Fn(Layout) -> Spot, inks: Inks| -> Vec<String> {
            Layout::ALL
                .into_iter()
                .filter(|layout| closing || *layout != Layout::WideFull)
                .filter_map(|layout| {
                    let ink = inked(&spot(layout), fade_at(layout)?, inks)?;
                    Some(format!("{}:{}", layout.as_str(), ink.as_str()))
                })
                .collect()
        };
        let words = tokens(&|layout| words_spot(align, aspect, layout, &frame), Inks::words());
        let foot_tones = closing.then(|| grid.extremes(&foot_spot(&frame, aspect), None));
        let foot = foot_tones.and_then(|tones| Backing::Auto.accept(better_ink(tones, Inks::words())));
        let credit = foot_tones.filter(|_| credit).and_then(|tones| backing.accept(credit_ink(tones, text, page)));
        let edition = closing.then(|| edition_ink(&grid, aspect, align, &frame, fade_at)).flatten();
        Some((words.join(" "), foot, credit, edition))
    };
    let day = reading(light, ground.light, CREDIT_INK.0);
    // Without a `dark=` picture the one picture is shown in both themes.
    let night = reading(dark.unwrap_or(light), ground.dark, CREDIT_INK.1);
    let mut out = String::new();
    for (suffix, picture) in [("", day), ("-night", night)] {
        if let Some((words, foot, credit, edition)) = picture {
            if !words.is_empty() {
                out.push_str(&format!(r#" data-words{suffix}="{words}""#));
            }
            if let Some(ink) = foot {
                out.push_str(&format!(r#" data-foot{suffix}="{}""#, ink.as_str()));
            }
            if let Some(ink) = credit {
                out.push_str(&format!(r#" data-credit{suffix}="{}""#, ink.as_str()));
            }
            if let Some(ink) = edition {
                out.push_str(&format!(r#" data-edition{suffix}="{}""#, ink.as_str()));
            }
        }
    }
    out
}

/// For `panel=none`: the lowest of the best contrasts either ink reaches
/// under the words at each layout, and with `credit` the credit's at the foot,
/// in the light theme over `light` and the dark one over `dark` (or `light`
/// again), when that is under [`MIN_CONTRAST`] (the author asked for ink that
/// may not read).
pub fn forced_ink_shortfall(
    lookup: &crate::build::media::dimensions::MediaDimensionLookup,
    (light, dark): (&str, Option<&str>),
    align: Option<&str>,
    frame: &Frame,
    credit: bool,
) -> Option<f32> {
    let ground = lookup.page_ground();
    [(light, CREDIT_INK.0, ground.light), (dark.unwrap_or(light), CREDIT_INK.1, ground.dark)]
        .into_iter()
        .filter_map(|(href, text, page)| Some((measure(lookup, href)?, text, page)))
        .flat_map(|((grid, aspect), text, page)| {
            let words = Layout::ALL.map(|layout| better_ink(grid.extremes(&words_spot(align, aspect, layout, frame), None), Inks::words()).1);
            let credit = (credit && frame.closing).then(|| credit_ink(grid.extremes(&foot_spot(frame, aspect), None), text, page).1);
            words.into_iter().chain(credit)
        })
        .filter(|contrast| *contrast < MIN_CONTRAST)
        .min_by(f32::total_cmp)
}

/// Warn for each `panel=none` hero whose picture reads under 4.5:1 where its
/// words or its credit land: the author asked for ink with no panel, and it
/// may not be legible. Names the page and the hero's opening words.
pub fn warn_unreadable_forced_ink(
    doc: &moss_core::ast::Document,
    lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    file_path: &str,
) {
    use moss_core::ast::{Block, Shortcode, Url};
    let Some(lookup) = lookup else { return };
    fn href(u: &Url) -> Option<&str> {
        match u {
            Url::Resolved(r) => Some(r.href.as_str()),
            Url::Unresolved(_) => None,
        }
    }
    for block in &doc.blocks {
        let Block::Shortcode(Shortcode::Hero(hero)) = block else { continue };
        if hero.panel.as_deref() != Some("none") || hero.overlay.is_empty() {
            continue;
        }
        let Some(light) = hero.image.as_ref().and_then(href) else { continue };
        let frame = Frame::of(hero, hero.classes.split_whitespace().any(|c| c == "moss-hero-closing"));
        if let Some(contrast) = forced_ink_shortfall(lookup, (light, hero.dark.as_ref().and_then(href)), hero.align.as_deref(), &frame, !hero.caption.is_empty()) {
            let words: String = hero.overlay_text.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
            crate::build::cli_output::log_warn_problem!(
                "[{file_path}] `:::hero {{panel=none}}` (\"{words}\"): no ink reaches {MIN_CONTRAST}:1 over the picture where the words or the credit sit (best {contrast:.1}:1), so they may be hard to read. Use `panel=auto` to keep the panel there."
            );
        }
    }
}

/// A closing picture's `max-height=` at 48rem, where it is least (a px or vw
/// cap never shrinks as the window widens), when it is under what the box
/// must hold at its tallest: [`WORDS_PX`], which it clips past, and with a
/// credit [`CREDIT_PX`] more, since the credit at the foot shares the words'
/// column there and would otherwise sit over them.
pub fn short_cap(hero: &moss_core::ast::HeroShortcode) -> Option<f32> {
    let px = hero.max_height?.px(Layout::Mid.widths().0);
    let need = if hero.caption.is_empty() { WORDS_PX } else { WORDS_PX + CREDIT_PX };
    (px < need).then_some(px)
}

/// Warn for `fade=top` on a hero that is not a closing picture (the `:::hero`
/// that ends a footer), where it does nothing, and for a closing picture
/// whose `max-height=` is shorter than its words ([`short_cap`]). Another
/// fade value already warns as unrecognized.
pub fn warn_closing_options(doc: &moss_core::ast::Document, closes_page: bool, file_path: &str) {
    use moss_core::ast::{Block, Shortcode};
    let last = doc.blocks.len().saturating_sub(1);
    for (i, block) in doc.blocks.iter().enumerate() {
        let Block::Shortcode(Shortcode::Hero(hero)) = block else { continue };
        let closing = closes_page && i == last;
        if hero.fade.as_deref() == Some("top") && !closing {
            crate::build::cli_output::log_warn_problem!(
                "[{file_path}] `:::hero {{fade=top}}` fades only a closing picture, the `:::hero` that ends footer.md, so it is ignored here."
            );
        }
        if let Some(px) = short_cap(hero).filter(|_| closing && !hero.overlay.is_empty()) {
            crate::build::cli_output::log_warn_problem!(
                "[{file_path}] `:::hero {{max-height=…}}` is {px:.0}px tall at 768px wide, under the 22rem its words can take (26.5rem with the credit at its foot), so they may be cut off or covered. Raise the cap's smallest value."
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

    /// An ordinary hero's frame.
    fn ordinary() -> Frame {
        Frame::new(None, None, None, false, false)
    }

    /// A closing picture's, uncapped, with the default position for `align`.
    fn closing(align: Option<&str>) -> Frame {
        Frame::new(None, None, align, false, true)
    }

    fn faded_frame() -> Frame {
        Frame::new(None, None, Some("top"), true, true)
    }

    fn plain(align: Option<&str>, backing: Backing, closing: bool) -> InkRequest<'_> {
        InkRequest { align, backing, frame: Frame::new(None, None, align, false, closing), credit: true, ground: PageGround::default() }
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
        let all = Spot::uncropped((0.0, 1.0), (0.0, 1.0));
        let (a, b) = (grid.extremes(&all, None), again.extremes(&all, None));
        assert!((a.0 - b.0).abs() < 0.001 && (a.1 - b.1).abs() < 0.001);
        assert!(ToneGrid::parse("1,2").is_none());
    }

    #[test]
    fn a_pale_sky_takes_dark_ink_and_its_dark_ground_takes_light() {
        let grid = ToneGrid::from_image(&banded(SKY, GROUND, 0.7));
        for layout in Layout::ALL {
            assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, layout, &ordinary()), None, Inks::words()), Some(Ink::Dark));
        }
        assert_eq!(readable_ink(&grid, &foot_spot(&closing(Some("top")), 70.0 / 160.0), None, Inks::words()), Some(Ink::Light));
    }

    #[test]
    fn a_mid_tone_area_takes_neither() {
        let grid = ToneGrid::from_image(&banded([120; 3], [120; 3], 0.7));
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, &ordinary()), None, Inks::words()), None);
    }

    #[test]
    fn a_far_dark_patch_spares_the_wide_box_but_not_a_phone() {
        let grid = ToneGrid::from_image(&sky_with_far_patch());
        // Wide: the words box is the start 47% of the width; the patch is at the far end.
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, &ordinary()), None, Inks::words()), Some(Ink::Dark));
        // `align=end` words sit at the foot of the picture, below the patch.
        assert_eq!(readable_ink(&grid, &words_spot(Some("end"), 0.4, Layout::Wide, &ordinary()), None, Inks::words()), Some(Ink::Dark));
        // A phone runs the words across the whole width.
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Narrow, &ordinary()), None, Inks::words()), None);
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
        assert_eq!(readable_ink(&grid, &words_spot(None, 0.8, Layout::Wide, &ordinary()), None, Inks::words()), Some(Ink::Dark));
        assert_eq!(readable_ink(&grid, &words_spot(None, 0.8, Layout::Narrow, &ordinary()), None, Inks::words()), None);
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
        let top = words_spot(Some("top"), 0.8, Layout::Wide, &closing(Some("top")));
        assert_eq!(readable_ink(&band(40, 80), &top, None, Inks::words()), Some(Ink::Dark));
        assert_eq!(readable_ink(&band(16, 24), &top, None, Inks::words()), None);
        assert_eq!(readable_ink(&band(0, 40), &foot_spot(&closing(Some("top")), 0.8), None, Inks::words()), Some(Ink::Dark));
        assert_eq!(readable_ink(&band(70, 80), &foot_spot(&closing(Some("top")), 0.8), None, Inks::words()), None);
    }

    #[test]
    fn an_edition_takes_the_ink_only_where_every_layout_reads_it() {
        let sky = lookup_of(&[("a.png", &banded(SKY, SKY, 1.0))]);
        let attrs = ink_attributes(Some(&sky), "/a.png", None, plain(Some("top"), Backing::Auto, true));
        assert!(attrs.contains(r#"data-edition="dark""#), "{attrs}");
        let ground = lookup_of(&[("b.png", &banded(GROUND, GROUND, 1.0))]);
        let attrs = ink_attributes(Some(&ground), "/b.png", None, plain(Some("top"), Backing::Auto, true));
        assert!(attrs.contains(r#"data-edition="light""#), "{attrs}");
        let split = lookup_of(&[("c.png", &banded(SKY, GROUND, 0.7))]);
        let attrs = ink_attributes(Some(&split), "/c.png", None, plain(Some("top"), Backing::Auto, true));
        assert!(!attrs.contains("data-edition="), "{attrs}");
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
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, &closing(Some("top"))), None, Inks::words()), None);
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::WideFull, &closing(Some("top"))), None, Inks::words()), Some(Ink::Dark));
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
        let spot = |is_closing: bool| words_spot(Some("top"), 0.4, Layout::Wide, &if is_closing { closing(Some("top")) } else { ordinary() });
        assert_eq!(readable_ink(&patch(60), &spot(false), None, Inks::words()), Some(Ink::Dark));
        assert_eq!(readable_ink(&patch(60), &spot(true), None, Inks::words()), None);
        assert_eq!(readable_ink(&patch(80), &spot(true), None, Inks::words()), Some(Ink::Dark));
    }

    #[test]
    fn each_layout_gets_its_own_token() {
        let lookup = lookup_of(&[("far.png", &sky_with_far_patch())]);
        let attrs = ink_attributes(Some(&lookup), "/far.png", None, plain(Some("top"), Backing::Auto, false));
        assert!(attrs.contains(r#"data-words="wide:dark""#), "no `wide-full` for an ordinary hero: {attrs}");
        assert!(!attrs.contains("narrow:"), "{attrs}");
        assert!(attrs.contains(r#"data-words-night="#), "{attrs}");
    }

    #[test]
    fn a_hero_without_a_dark_picture_repeats_its_reading_for_night() {
        let lookup = lookup_of(&[("day.png", &banded(SKY, GROUND, 0.7))]);
        let one = ink_attributes(Some(&lookup), "/day.png", None, plain(Some("top"), Backing::Auto, true));
        let tokens = r#"="narrow:dark mid:dark wide:dark wide-full:dark""#;
        assert!(one.contains(&format!("data-words{tokens}")) && one.contains(&format!("data-words-night{tokens}")), "{one}");
        assert!(one.contains(r#"data-foot="light""#) && one.contains(r#"data-foot-night="light""#), "{one}");
    }

    #[test]
    fn a_dark_picture_is_measured_apart_from_the_light_one() {
        let lookup = lookup_of(&[("day.png", &banded(SKY, GROUND, 0.7)), ("night.png", &banded([20, 28, 48], [230, 235, 240], 0.7))]);
        let pair = ink_attributes(Some(&lookup), "/day.png", Some("/night.png"), plain(Some("top"), Backing::Auto, true));
        assert!(pair.contains(r#"data-words="narrow:dark"#) && pair.contains(r#"data-words-night="narrow:light"#), "{pair}");
    }

    #[test]
    fn nothing_is_measured_without_a_scan_or_for_a_hero_that_keeps_its_panel() {
        let lookup = lookup_of(&[("day.png", &banded(SKY, GROUND, 0.7))]);
        assert_eq!(ink_attributes(Some(&lookup), "/other.png", None, plain(None, Backing::Auto, true)), "");
        assert_eq!(ink_attributes(None, "/day.png", None, plain(None, Backing::Auto, true)), "");
        assert!(!ink_attributes(Some(&lookup), "/day.png", None, plain(None, Backing::Panel, false)).contains("data-words"));
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
        let attrs = ink_attributes(Some(&lookup), "/awards/art.png", None, plain(Some("top"), Backing::Auto, false));
        assert!(attrs.contains("data-words="), "{attrs}");
    }

    /// A pale sky with a dark strip over its top 8%: no ink reads over the strip.
    fn strip_at_the_top() -> DynamicImage {
        let mut img = RgbImage::from_pixel(160, 70, Rgb(SKY));
        for y in 0..6 {
            for x in 0..160 {
                img.put_pixel(x, y, Rgb(INK));
            }
        }
        for y in 49..70 {
            for x in 0..160 {
                img.put_pixel(x, y, Rgb(GROUND));
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    fn faded(align: Option<&str>, ground: PageGround) -> InkRequest<'_> {
        InkRequest { align, backing: Backing::Auto, frame: Frame::new(None, None, align, true, true), credit: true, ground }
    }

    #[test]
    fn a_faded_top_is_measured_over_the_page_it_fades_into() {
        let grid = ToneGrid::from_image(&strip_at_the_top());
        let (light, dark) = *BACKGROUNDS;
        for layout in Layout::ALL {
            let spot = words_spot(Some("top"), 70.0 / 160.0, layout, &faded_frame());
            assert_eq!(readable_ink(&grid, &spot, None, Inks::words()), None, "{layout:?}: the strip is too dark for either ink");
            assert_eq!(readable_ink(&grid, &spot, Some(Fade::new(light, layout)), Inks::words()), Some(Ink::Dark), "{layout:?}: faded into a light page");
            assert_eq!(readable_ink(&grid, &spot, Some(Fade::new(dark, layout)), Inks::words()), None, "{layout:?}: dark page above a pale sky");
        }
        // The foot is below the fade.
        assert_eq!(readable_ink(&grid, &foot_spot(&closing(Some("top")), 70.0 / 160.0), Some(Fade::new(light, Layout::Wide)), Inks::words()), Some(Ink::Light));
    }

    #[test]
    fn a_faded_closing_picture_reads_each_theme_over_its_own_page_colour() {
        let lookup = lookup_of(&[("strip.png", &strip_at_the_top()), ("night.png", &banded([20, 28, 48], [8, 10, 14], 0.7))]);
        let one = ink_attributes(Some(&lookup), "/strip.png", None, faded(Some("top"), PageGround::default()));
        assert!(one.contains(r#"data-words="narrow:dark mid:dark wide:dark wide-full:dark""#), "{one}");
        assert!(!one.contains("data-words-night"), "the light picture over a dark page reads in neither ink: {one}");
        assert!(one.contains(r#"data-foot-night="light""#), "{one}");
        let pair = ink_attributes(Some(&lookup), "/strip.png", Some("/night.png"), faded(Some("top"), PageGround::default()));
        assert!(pair.contains(r#"data-words-night="narrow:light mid:light wide:light wide-full:light""#), "{pair}");
        // Without the fade the strip keeps the panel in both themes.
        let plain = ink_attributes(Some(&lookup), "/strip.png", None, plain(Some("top"), Backing::Auto, true));
        assert!(!plain.contains("data-words"), "{plain}");
    }

    #[test]
    fn an_unresolved_page_colour_keeps_the_panel_in_that_theme() {
        let lookup = lookup_of(&[("strip.png", &strip_at_the_top()), ("night.png", &banded([20, 28, 48], [8, 10, 14], 0.7))]);
        let ground = PageGround { light: None, ..PageGround::default() };
        let attrs = ink_attributes(Some(&lookup), "/strip.png", Some("/night.png"), faded(Some("top"), ground));
        assert!(!attrs.contains(r#"data-words=""#) && !attrs.contains(r#"data-credit=""#), "{attrs}");
        assert!(attrs.contains(r#"data-foot="light""#) && attrs.contains("data-words-night=") && attrs.contains("data-credit-night="), "{attrs}");
    }

    #[test]
    fn the_credit_takes_the_page_colour_over_a_dark_foot_and_the_text_colour_over_a_pale_one() {
        let lookup = lookup_of(&[("dark-foot.png", &banded(SKY, GROUND, 0.7)), ("pale-foot.png", &banded(GROUND, SKY, 0.3))]);
        let dark = ink_attributes(Some(&lookup), "/dark-foot.png", None, plain(Some("top"), Backing::Auto, true));
        assert!(dark.contains(r#"data-credit="page""#) && dark.contains(r#"data-credit-night="text""#), "{dark}");
        let pale = ink_attributes(Some(&lookup), "/pale-foot.png", None, plain(Some("top"), Backing::Auto, true));
        assert!(pale.contains(r#"data-credit="text""#) && pale.contains(r#"data-credit-night="page""#), "{pale}");
        // Without a credit, or with `panel=always`, nothing is measured for it.
        let bare = InkRequest { credit: false, ..plain(Some("top"), Backing::Auto, true) };
        assert!(!ink_attributes(Some(&lookup), "/dark-foot.png", None, bare).contains("data-credit"));
        assert!(!ink_attributes(Some(&lookup), "/dark-foot.png", None, plain(Some("top"), Backing::Panel, true)).contains("data-credit"));
    }

    #[test]
    fn a_mid_tone_foot_keeps_the_credits_panel_in_both_themes() {
        let lookup = lookup_of(&[("busy.png", &banded(SKY, [120; 3], 0.7))]);
        for request in [plain(Some("top"), Backing::Auto, true), faded(Some("top"), PageGround::default())] {
            let attrs = ink_attributes(Some(&lookup), "/busy.png", None, request);
            assert!(!attrs.contains("data-credit"), "{attrs}");
        }
    }

    #[test]
    fn the_page_colour_is_read_where_a_theme_is_told_to_set_it() {
        let (light, dark) = *BACKGROUNDS;
        assert_eq!(PageGround::of_theme(None), PageGround { light: Some(light), dark: Some(dark) });
        assert_eq!(PageGround::of_theme(Some("h1 { color: red; }")), PageGround::default());
        let root = PageGround::of_theme(Some(":root { --moss-color-bg: #ffffff; }"));
        assert_eq!(root, PageGround { light: Some((255, 255, 255)), dark: Some((255, 255, 255)) }, "a `:root` colour wins in dark too");
        let both = PageGround::of_theme(Some(":root { --moss-color-bg: #fff }\n:root[data-theme='dark'] { --moss-color-bg: rgb(10, 20, 30) !important; }"));
        assert_eq!(both, PageGround { light: Some((255, 255, 255)), dark: Some((10, 20, 30)) });
        let dark_only = PageGround::of_theme(Some(":root[data-theme=\"dark\"] { --moss-color-bg: #000; }"));
        assert_eq!(dark_only, PageGround { light: Some(light), dark: Some((0, 0, 0)) });
        for unread in [
            ":root { --moss-color-bg: var(--paper); }",
            ":root { --moss-color-bg: color-mix(in oklch, white, tan); }",
            "@media (prefers-color-scheme: dark) { :root { --moss-color-bg: #000; } }",
            "body { --moss-color-bg: #fff; }",
        ] {
            let ground = PageGround::of_theme(Some(unread));
            assert!(ground.light.is_none() || ground.dark.is_none(), "{unread}: {ground:?}");
        }
        assert_eq!(PageGround::of_theme(Some(":root { --moss-color-bg: var(--paper); }")), PageGround { light: None, dark: None });
    }

    #[test]
    fn site_css_sets_the_credit_in_the_inks_it_is_measured_in() {
        let hex = |(r, g, b): crate::build::scan::section_text::Rgb| format!("#{r:02x}{g:02x}{b:02x}");
        for ink in [CREDIT_INK.0, CREDIT_INK.1] {
            assert!(crate::build::emit::stylesheet::CORE_CSS.contains(&format!("color: {};", hex(ink))), "site.css sets the credit in {}", hex(ink));
        }
    }

    /// A 100x77 sky (the shape of a 1400x1073 print) with a dark band over `rows` of its height.
    fn sky_with_band(rows: (u32, u32)) -> ToneGrid {
        let mut img = RgbImage::from_pixel(100, 77, Rgb(SKY));
        for y in rows.0..rows.1 {
            for x in 0..100 {
                img.put_pixel(x, y, Rgb(INK));
            }
        }
        ToneGrid::from_image(&DynamicImage::ImageRgb8(img))
    }

    fn capped() -> Frame {
        Frame::new(Some(moss_core::media::Position::Focus(50, 72)), moss_core::media::MaxHeight::parse("clamp(420px, 50vw, 680px)"), Some("top"), true, true)
    }

    #[test]
    fn a_capped_closing_picture_is_measured_over_the_rows_its_box_shows() {
        // At 1280px the cap is 640px of a 980px-tall picture, anchored at 72%:
        // the box shows rows 25% to 90%, so a band over the top 10% is cropped away.
        let aspect = 77.0 / 100.0;
        let top = sky_with_band((0, 8));
        for layout in [Layout::Mid, Layout::Wide, Layout::WideFull] {
            let spot = words_spot(Some("top"), aspect, layout, &capped());
            assert_eq!(readable_ink(&top, &spot, None, Inks::words()), Some(Ink::Dark), "{layout:?}: the band is above the crop");
            let uncropped = words_spot(Some("top"), aspect, layout, &closing(Some("top")));
            assert_eq!(readable_ink(&top, &uncropped, None, Inks::words()), None, "{layout:?}: uncropped, the band is under the words");
            // Within the crop, under the words: the band keeps the panel.
            assert_eq!(readable_ink(&sky_with_band((31, 35)), &spot, None, Inks::words()), None, "{layout:?}");
        }
        let crop = Crop::cover(aspect, (1280.0, 640.0), (0.5, 0.72));
        assert!((crop.rows.0 - 0.2525).abs() < 0.001 && (crop.rows.1 - 0.9019).abs() < 0.001 && crop.cols == (0.0, 1.0), "{crop:?}");
    }

    #[test]
    fn the_fade_runs_down_the_box_not_the_picture() {
        // A band at 26-32% of the picture is the top of the capped box at 1280px,
        // so the fade hides it; uncropped it sits where the picture shows through.
        let band = sky_with_band((20, 25));
        let (light, _) = *BACKGROUNDS;
        let fade = Some(Fade::new(light, Layout::Wide));
        let capped_spot = Spot(vec![Crop::cover(77.0 / 100.0, (1280.0, 640.0), (0.5, 0.72)).area((0.0, 0.2), (0.0, 1.0))]);
        assert_eq!(readable_ink(&band, &capped_spot, fade, Inks::words()), Some(Ink::Dark));
        assert_eq!(readable_ink(&band, &Spot::uncropped((0.26, 0.32), (0.0, 1.0)), fade, Inks::words()), None);
    }

    #[test]
    fn a_faded_phone_measures_the_words_over_the_floor_its_box_keeps() {
        // A wide picture in a 38rem-tall phone box shows only its middle columns,
        // and all its rows: a dark patch at the start edge is cropped away, and the
        // words' rows run down the picture's top 60%.
        let mut img = RgbImage::from_pixel(160, 70, Rgb(SKY));
        for y in 0..70 {
            for x in 0..24 {
                img.put_pixel(x, y, Rgb(INK));
            }
        }
        let grid = ToneGrid::from_image(&DynamicImage::ImageRgb8(img));
        let faded = faded_frame();
        let spot = words_spot(Some("top"), 70.0 / 160.0, Layout::Narrow, &faded);
        assert_eq!(readable_ink(&grid, &spot, None, Inks::words()), Some(Ink::Dark));
        // Without the fade the phone's box is set by its words, so the whole width counts.
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 70.0 / 160.0, Layout::Narrow, &closing(Some("top"))), None, Inks::words()), None);
        // The rows: a band under the words (at 50% of the box) keeps the panel.
        let low = sky_with_band((38, 42));
        assert_eq!(readable_ink(&low, &words_spot(Some("top"), 77.0 / 100.0, Layout::Narrow, &faded), None, Inks::words()), None);
    }

    #[test]
    fn site_css_draws_the_fade_and_the_phone_floor_the_measurement_assumes() {
        let css = crate::build::emit::stylesheet::CORE_CSS;
        for depth in [FADE_DEPTH.0, FADE_DEPTH.1] {
            assert!(css.contains(&format!("linear-gradient(to bottom, transparent, #000 {}%)", (depth * 100.0).round())), "fade {depth}");
        }
        assert!(css.contains(&format!("min-block-size: max({}rem, var(--moss-hero-max-height, 0px))", PHONE_FADE_FLOOR_PX / 16.0)));
    }

    #[test]
    fn a_capped_align_top_picture_keeps_its_foot_unless_it_names_a_position() {
        // site.css anchors a closing `align=top` picture at its foot, so the
        // foot rows the colophon sits on are the picture's own foot, pale here;
        // a top anchor would put the dark band at 50-65% there instead.
        let grid = sky_with_band((39, 50));
        let cap = moss_core::media::MaxHeight::parse("clamp(420px, 50vw, 680px)");
        let foot = |frame: Frame| readable_ink(&grid, &foot_spot(&frame, 77.0 / 100.0), None, Inks::words());
        assert_eq!(foot(Frame::new(None, cap, Some("top"), false, true)), Some(Ink::Dark));
        assert_eq!(foot(Frame::new(Some(moss_core::media::Position::Top), cap, Some("top"), false, true)), None);
        // An ordinary hero keeps its top unless it names a position.
        assert_eq!(Frame::new(None, None, Some("top"), false, false).position, (0.5, 0.0));
    }

    #[test]
    fn a_cap_under_the_words_box_is_found_at_48rem() {
        let hero = |cap: &str| moss_core::ast::HeroShortcode { max_height: moss_core::media::MaxHeight::parse(cap), ..Default::default() };
        assert_eq!(short_cap(&hero("300px")), Some(300.0));
        assert_eq!(short_cap(&hero("25vw")), Some(192.0));
        assert_eq!(short_cap(&hero("clamp(420px, 50vw, 680px)")), None);
        // The credit at the foot needs its rows too: 400px holds the words, not both.
        let captioned = moss_core::ast::HeroShortcode { caption: vec![moss_core::ast::Inline::Text("Credit".into())], ..hero("400px") };
        assert_eq!(short_cap(&captioned), Some(400.0));
        assert_eq!(short_cap(&hero("400px")), None);
        assert_eq!(short_cap(&moss_core::ast::HeroShortcode { caption: captioned.caption.clone(), ..hero("clamp(440px, 50vw, 680px)") }), None);
        assert_eq!(short_cap(&moss_core::ast::HeroShortcode::default()), None);
    }

    #[test]
    fn a_short_cap_on_the_closing_picture_is_a_problem() {
        let problems = |md: &str, closes_page: bool| {
            let _ = crate::build::cli_output::take_cli_problems();
            warn_closing_options(&moss_core::ast::parse(md), closes_page, "footer.md");
            crate::build::cli_output::take_cli_problems()
        };
        let short = ":::hero {image=a.jpg max-height=300px}\n## Words\n:::\n";
        assert_eq!(problems(short, true), 1);
        assert_eq!(problems(short, false), 0, "an ordinary hero's cap is a floor, not a box for its words");
        assert_eq!(problems(":::hero {image=a.jpg max-height=\"clamp(420px, 50vw, 680px)\"}\n## Words\n:::\n", true), 0);
    }

    #[test]
    fn forcing_ink_over_a_mid_tone_reports_the_shortfall() {
        let lookup = lookup_of(&[("mid.png", &banded([120; 3], [120; 3], 0.7)), ("day.png", &banded(SKY, GROUND, 0.7))]);
        assert!(forced_ink_shortfall(&lookup, ("/mid.png", None), Some("top"), &ordinary(), false).is_some_and(|c| c < MIN_CONTRAST));
        assert!(forced_ink_shortfall(&lookup, ("/day.png", None), Some("top"), &ordinary(), false).is_none());
        // A closing picture's credit counts too: quiet under the words, mid-tone at the foot.
        let lookup = lookup_of(&[("foot.png", &banded(SKY, [120; 3], 0.7))]);
        assert!(forced_ink_shortfall(&lookup, ("/foot.png", None), Some("top"), &closing(Some("top")), false).is_none());
        assert!(forced_ink_shortfall(&lookup, ("/foot.png", None), Some("top"), &closing(Some("top")), true).is_some_and(|c| c < MIN_CONTRAST));
    }
}
