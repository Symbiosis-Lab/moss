//! Whether a hero's words can sit straight on its picture, in ink, with no
//! panel behind them.
//!
//! The scan keeps each picture once, as a linear-luminance image ([`Ground`]).
//! A hero asks for the pixels its words can land on in each of the three
//! layouts moss draws ([`Layout`]) and, on a closing picture, on each page
//! column ([`Page`]), and for its foot. Where dark or light ink clears WCAG AA
//! (4.5:1, or 3:1 for large text) against the worst window under a part of the
//! words, a panel only hides the picture; the stylesheet applies the panel or
//! the ink per layout and page.
//!
//! The worst window is read at the scale of the letters: a square about 0.08
//! em wide of the smallest type the part is set in slides over the pixels the
//! part covers (under that layout's crop, `object-fit: cover`) and the darkest
//! and lightest mean decide ([`Ground::read`]). Texture finer than a stroke is
//! averaged away and a shape a stroke wide counts in full. Under `panel=none`,
//! where neither ink clears its contrast against the worst window (which then
//! cannot judge between them: for the light ink it is a pale hairline, for the
//! dark ink the dark ground), the ink with the greater contrast against the
//! median window, the part's typical tone, is taken. On a closing
//! picture with `align=top` the pixels are where the glyphs sit: the heading's
//! lines, the text and the row of links, as their widths and the stylesheet's
//! type scale set them, from where the page's own text column starts at each
//! window width ([`Type`]). Where the theme sets the type in a way the build
//! cannot read ([`TypeTheme`]) the words keep their panel; elsewhere it is the
//! words box (`max(600px, 35%)` wide). Pages are taken as left-to-right, which
//! is what moss emits; a right-to-left page should set `panel=always`.
//!
//! A closing picture's crop is build data ([`Frame`]): its `max-height=` and
//! the position its `image=` names, so the pixels are those its box shows
//! ([`Crop`]) at each layout. A closing picture with `fade=top` shows the page
//! through its top. There each cell is composited over the page colour of the
//! theme being read ([`PageGround`]) before it is measured. The credit sits at
//! the foot, in whichever of the theme's secondary text colour and its page
//! colour reads there ([`CreditInk`]).

mod ground;
mod page;
mod words;
pub use ground::{built as grounds_built, Ground};
pub use page::PageGround;
pub use words::{Column, TypeTheme, Words};
use words::{credit_height, Type, CREDIT_FONT_PX};

use crate::build::components::color_extract::hero_tone_color_luminance;
use crate::build::scan::section_text::{luminance, Rgb, BACKGROUNDS};
use crate::build::scan::wcag::{contrast_ratio, linear_to_srgb, srgb_to_linear};

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
/// where the 600px box is most of the width) and a wide one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Narrow,
    Mid,
    Wide,
}

/// A page's own text column: its `content_width:` and whether it keeps the
/// lane for a contents list (`contents: pictures`). A closing picture's words
/// start at the text column, so each page is measured in its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page {
    pub column: Column,
    pub contents: bool,
}

impl Page {
    pub const ALL: [Page; 6] = [
        Page { column: Column::Default, contents: false },
        Page { column: Column::Wide, contents: false },
        Page { column: Column::Full, contents: false },
        Page { column: Column::Default, contents: true },
        Page { column: Column::Wide, contents: true },
        Page { column: Column::Full, contents: true },
    ];

    /// The attribute token a closing picture's reading has at `layout`: a
    /// phone reads one (the columns differ by a few px there), the others one
    /// a page.
    fn token(self, layout: Layout) -> String {
        let column = match self.column {
            Column::Default => "default",
            Column::Wide => "wide",
            Column::Full => "full",
        };
        match layout {
            Layout::Narrow => "narrow".to_string(),
            _ => format!("{}-{column}{}", layout.as_str(), if self.contents { "-lane" } else { "" }),
        }
    }

    /// Whether the lane is kept at a window: only where the pointer is fine,
    /// which the build cannot see, so a contents page is read both ways.
    fn lanes(self) -> &'static [bool] {
        if self.contents { &[false, true] } else { &[false] }
    }
}

impl Layout {
    pub const ALL: [Layout; 3] = [Layout::Narrow, Layout::Mid, Layout::Wide];

    pub fn as_str(self) -> &'static str {
        match self {
            Layout::Narrow => "narrow",
            Layout::Mid => "mid",
            Layout::Wide => "wide",
        }
    }

    /// The window widths the layout spans: from the `48rem` and `80rem` media
    /// queries of site.css at the default 16px root, the widest taken as 2560px.
    fn widths(self) -> (f32, f32) {
        match self {
            Layout::Narrow => (320.0, 767.0),
            Layout::Mid => (768.0, 1279.0),
            Layout::Wide => (1280.0, 2560.0),
        }
    }

    /// How far across the picture the words box can reach, as a share of the
    /// width, at the layout's narrowest window (768px and 1280px). A hero's
    /// box is 600px from the start; a closing picture's starts where the page's
    /// text column does, less the box's own padding, as [`Type`] sets it.
    fn box_share(self, closing: bool, theme: &TypeTheme, page: Page) -> f32 {
        let width = self.widths().0;
        match (self, closing) {
            (Layout::Narrow, _) => 1.0,
            (_, false) => 600.0 / width,
            (_, true) => page.lanes().iter().map(|lane| Type::at(width, theme, page.column, *lane)).map(|t| (t.inset - t.pad + t.box_px) / width).fold(0.0, f32::max),
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
    /// CSS px the picture's whole width spans in the box.
    css_per_width: f32,
    /// The smallest type the text is set in, in px, and the contrast it needs.
    font: f32,
    need: f32,
}

/// The type a part is set in: its smallest size in px and the contrast it
/// needs (4.5:1, or 3:1 where WCAG counts the text as large).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub font: f32,
    pub need: f32,
}

impl Face {
    /// Small print: the credit, the colophon, an edition line.
    const SMALL: Face = Face { font: CREDIT_FONT_PX, need: MIN_CONTRAST };
}

impl Spot {
    /// A spot on a picture its box shows whole, `css_per_width` px wide.
    #[cfg(test)]
    fn uncropped(rows: Span, cols: Span, css_per_width: f32, face: Face) -> Self {
        Spot(vec![Area { rows, cols, shown: (0.0, 1.0), css_per_width, font: face.font, need: face.need }])
    }
}

/// The part of a picture its box shows under `object-fit: cover`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    rows: Span,
    cols: Span,
    /// CSS px the picture's whole width spans in the box.
    css_per_width: f32,
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
        let (rows, cols) = if natural >= h { (window(h / natural, y), (0.0, 1.0)) } else { ((0.0, 1.0), window(natural / h.max(1.0), x)) };
        Crop { rows, cols, css_per_width: w / (cols.1 - cols.0).max(0.001) }
    }

    /// The picture under `rows` and `cols` of the box.
    fn area(self, rows: Span, cols: Span, face: Face) -> Area {
        let within = |(a, b): Span, (from, to): Span| (a + from * (b - a), a + to * (b - a));
        Area { rows: within(self.rows, rows), cols: within(self.cols, cols), shown: self.rows, css_per_width: self.css_per_width, font: face.font, need: face.need }
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
    /// What the words and the credit say, as widths ([`Words`]): they set how
    /// many lines each takes and so where the glyphs land.
    words: Words,
    /// The theme's changes to the type the words are set in.
    theme: TypeTheme,
}

/// A faded closing picture's least height on a phone (38rem), so its words
/// sit on the faded top.
pub const PHONE_FADE_FLOOR_PX: f32 = 608.0;
/// A phone's box is never under 18rem.
const PHONE_FLOOR_PX: f32 = 288.0;

impl Frame {
    /// The frame `position`, `max_height`, `align` and `fade=top` make. The
    /// default position is site.css's: a closing picture with `align=top`
    /// keeps its foot, every other hero its top. Only a closing picture is
    /// capped or faded.
    pub fn new(position: Option<moss_core::media::Position>, max_height: Option<moss_core::media::MaxHeight>, align: Option<&str>, faded: bool, closing: bool, words: Words) -> Self {
        let position = match position.map(|p| p.percentages()) {
            Some((x, y)) => (f32::from(x) / 100.0, f32::from(y) / 100.0),
            None if closing && align == Some("top") => (0.5, 1.0),
            None => (0.5, 0.0),
        };
        Frame { position, max_height: max_height.filter(|_| closing), faded: faded && closing, closing, words, theme: TypeTheme::default() }
    }

    /// The frame on a page whose theme is `ground`.
    fn on(self, ground: &PageGround) -> Self {
        Frame { theme: ground.type_theme, ..self }
    }

    /// The frame of a hero read straight from its fence.
    pub fn of(hero: &moss_core::ast::HeroShortcode, closing: bool) -> Self {
        let position = moss_core::media::parse_media_attrs(&hero.attrs).position;
        Frame::new(position, hero.max_height, hero.align.as_deref(), hero.fade.as_deref() == Some("top"), closing, Words::read(&hero.overlay_text, &moss_core::ast::plain_text::inlines_to_plain_text(&hero.caption)))
    }

    /// The box's height at `width` px in `layout`. A phone's box without the
    /// fade is set by its words and the room below them; it is taken as the
    /// picture's own shape, or the floor if that is taller.
    fn box_height(&self, aspect: f32, layout: Layout, width: f32) -> f32 {
        let natural = width * aspect.max(0.01);
        let cap = self.max_height.map(|c| c.px(width));
        match layout {
            Layout::Narrow if self.faded => cap.unwrap_or(0.0).max(PHONE_FADE_FLOOR_PX),
            Layout::Narrow => natural.max(cap.unwrap_or(PHONE_FLOOR_PX)),
            _ if self.closing => cap.unwrap_or(natural),
            _ => DESKTOP_BOX_RATIO * width,
        }
    }

    /// The picture under the box rows `rows(width, height)` and columns
    /// `cols`, over the layout's widths: each measured window adds its area,
    /// since a crop that is not the picture's own shape shows a different part
    /// at each width.
    fn spot(&self, aspect: f32, layout: Layout, rows: impl Fn(f32, f32) -> Span, cols: impl Fn(f32, f32) -> Span, face: impl Fn(f32) -> Face) -> Spot {
        let (from, to) = layout.widths();
        let areas = (0..=4)
            .map(|i| from + (to - from) * i as f32 / 4.0)
            .map(|width| {
                let height = self.box_height(aspect, layout, width);
                Crop::cover(aspect, (width, height), self.position).area(rows(width, height), cols(width, height), face(width))
            })
            .collect();
        Spot(areas)
    }
}

/// The most of a box the words can cover: on a phone the bottom 40% is kept
/// clear (`closing-picture` gate).
const WORDS_SHARE: f32 = 0.6;
/// How tall a closing picture's words box is at most on a desktop (22rem): a
/// heading, a row of links and the box's padding. It bounds the box where the
/// glyphs' own reach is not read (`align` other than `top`), the edition
/// band and the least cap.
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

/// Where the words can land for `align` in `layout` on a `page`, on a picture
/// `aspect` tall (height over width) filling its box as `frame` says. On a
/// closing picture with `align=top` that is where the glyphs sit, inside the
/// box's padding ([`Type`]), from where the page's own column starts them; an
/// ink with no panel is read on the glyphs, not on the padding a panel would
/// have covered.
pub fn words_spot(align: Option<&str>, aspect: f32, layout: Layout, frame: &Frame, page: Page) -> Spot {
    let share = layout.box_share(frame.closing, &frame.theme, page);
    let said = !matches!(frame.words, Words { heading: 0.0, text: 0.0, link_count: 0, .. });
    if frame.closing && align == Some("top") && said {
        let areas = type_cases(layout, page).into_iter().flat_map(|(column, lane)| {
            (0..3).flat_map(move |i| {
                let at = move |width: f32| Type::at(width, &frame.theme, column, lane);
                let block = move |width: f32| at(width).blocks(&frame.words).into_iter().nth(i).filter(|b| !b.link);
                let rows = move |width: f32, height: f32| block(width).map_or((0.0, 0.0), |b| (b.rows.0 / height, b.rows.1 / height));
                let cols = move |width: f32, _| block(width).map_or((0.0, 0.0), |b| (at(width).inset / width, (at(width).inset + b.across) / width));
                let face = move |width: f32| block(width).map_or(Face::SMALL, |b| b.face);
                frame.spot(aspect, layout, rows, cols, face).0.into_iter().filter(|a| a.rows.1 > a.rows.0)
            })
        });
        return Spot(areas.collect());
    }
    let cols = move |_, _| match align {
        _ if layout == Layout::Narrow => (0.0, 1.0),
        Some("end") => (1.0 - share, 1.0),
        Some("center") => {
            let share = layout.box_share(false, &frame.theme, page);
            ((1.0 - share) / 2.0, (1.0 + share) / 2.0)
        }
        _ => (0.0, share),
    };
    let rows = |_, height: f32| {
        let share = if frame.closing && layout != Layout::Narrow { (WORDS_PX / height).min(WORDS_SHARE) } else { WORDS_SHARE };
        words_span(align, share)
    };
    let face = |width: f32| Type::at(width, &frame.theme, page.column, false).smallest_face();
    frame.spot(aspect, layout, rows, cols, face)
}

/// The colophon's row along a closing picture's bottom edge (2.25rem).
const COLOPHON_PX: f32 = 36.0;

/// A closing picture's colophon row: the bottom edge, across the width, at
/// every layout.
pub fn foot_spot(frame: &Frame, aspect: f32) -> Spot {
    Spot(Layout::ALL.iter().flat_map(|layout| frame.spot(aspect, *layout, |_, height| (1.0 - COLOPHON_PX / height, 1.0), |_, _| (0.0, 1.0), |_| Face::SMALL).0).collect())
}

/// The text columns a closing picture's words are read in at `layout`, as
/// `(column, lane)`: the page's own where the layouts differ by page, a phone's
/// every column, since its columns differ by a few px.
fn type_cases(layout: Layout, page: Page) -> Vec<(Column, bool)> {
    if layout == Layout::Narrow {
        [Column::Default, Column::Wide, Column::Full].map(|c| (c, false)).to_vec()
    } else {
        page.lanes().iter().map(|lane| (page.column, *lane)).collect()
    }
}

/// Where a closing picture's credit can sit: its text, right-aligned to the end
/// of the page's text column, in the rows above the colophon's row, in `layout`.
/// The footer is one for every page, so it is read on every page's column.
pub fn credit_spot(frame: &Frame, aspect: f32, layout: Layout, colophon: bool) -> Spot {
    let strip = if colophon { COLOPHON_PX } else { 0.0 };
    let cases = if layout == Layout::Narrow { type_cases(layout, Page::ALL[0]) } else { Page::ALL.iter().flat_map(|page| type_cases(layout, *page)).collect() };
    let areas = cases.into_iter().flat_map(|(column, lane)| {
        let credit = move |width: f32| Type::at(width, &frame.theme, column, lane).credit(width, frame.words.credit);
        let rows = move |width: f32, height: f32| {
            let (near, far) = credit(width).rows(strip);
            (1.0 - far / height, 1.0 - near / height)
        };
        let cols = move |width: f32, _| {
            let c = credit(width);
            (1.0 - (c.end + c.across) / width, 1.0 - c.end / width)
        };
        frame.spot(aspect, layout, rows, cols, |_| Face::SMALL).0
    });
    Spot(areas.collect())
}

/// How tall the edition block is.
const EDITION_PX: f32 = 96.0;

/// Where a closing picture's edition block can sit. Align top puts it under the
/// words, which reach at most `WORDS_PX`, so the band runs from the box's top
/// to that plus the block's height. Any other alignment, and a phone's column,
/// can put it anywhere in the box.
pub fn edition_spot(align: Option<&str>, aspect: f32, layout: Layout, frame: &Frame) -> Spot {
    let rows = |_, height: f32| match (layout, align) {
        (Layout::Narrow, _) => (0.0, 1.0),
        (_, Some("top")) => (0.0, ((WORDS_PX + EDITION_PX) / height).min(1.0)),
        _ => (0.0, 1.0),
    };
    frame.spot(aspect, layout, rows, |_, _| (0.0, 1.0), |_| Face::SMALL)
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

    /// The fade a theme's page colour makes at `layout`: none where the top is
    /// not faded, and `None` where the faded page colour is unresolved.
    fn resolve(faded: bool, page: Option<Rgb>, layout: Layout) -> Option<Option<Fade>> {
        match (faded, page) {
            (false, _) => Some(None),
            (true, Some(page)) => Some(Some(Fade::new(page, layout))),
            (true, None) => None,
        }
    }

    /// An sRGB level `level` seen through the mask at `alpha`.
    fn blend(self, level: f32, alpha: f32) -> f32 {
        (1.0 - alpha) * self.ground + alpha * level
    }
}

/// The ink an edition block takes: one ink that reads in every layout's band,
/// else `None` and the block keeps its panel. `fade_at` gives a layout's fade
/// ([`Fade::resolve`]).
pub fn edition_ink(grid: &Ground, aspect: f32, align: Option<&str>, frame: &Frame, fade_at: impl Fn(Layout) -> Option<Option<Fade>>) -> Option<Ink> {
    let inks: Vec<Option<Ink>> = Layout::ALL
        .into_iter()
        .map(|layout| readable_ink(grid, &edition_spot(align, aspect, layout, frame), fade_at(layout)?, Inks::words()))
        .collect();
    let first = inks[0]?;
    inks.iter().all(|ink| *ink == Some(first)).then_some(first)
}

/// WCAG AA for text.
pub const MIN_CONTRAST: f32 = 4.5;

/// The darkest and lightest window mean (linear luminance) over a part of the
/// picture, and the contrast its text needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    darkest: f32,
    lightest: f32,
    /// The median window mean, the part's typical tone, read only under `panel=none`.
    median: Option<f32>,
    need: f32,
}

/// A ratio an ink reaches, what the text needs, and the ratio scaled to the AA 4.5:1.
#[derive(Clone, Copy, Debug)]
struct Limit {
    raw: f32,
    need: f32,
    normalized: f32,
}

/// The readings of a spot's areas: each part at each window width.
#[derive(Clone, Debug, PartialEq)]
pub struct Tones(Vec<Tone>);

impl Tones {
    /// The contrast of an ink of luminance `ink` over the darkest window, worst
    /// over the areas, in units of the AA 4.5:1 each area's text needs.
    fn over_darkest(&self, ink: f32) -> f32 {
        self.0.iter().map(|t| contrast_ratio(t.darkest, ink) * MIN_CONTRAST / t.need).fold(f32::MAX, f32::min)
    }

    /// The contrast of an ink over the median window, worst over the areas, in
    /// the same units.
    fn over_median(&self, ink: f32) -> f32 {
        self.0.iter().map(|t| contrast_ratio(t.median.unwrap_or(t.darkest), ink) * MIN_CONTRAST / t.need).fold(f32::MAX, f32::min)
    }

    /// The worst ratio an ink of luminance `ink` reaches over the darkest (or the
    /// lightest) window, with what that area's text needs.
    fn limit(&self, ink: f32, over_darkest: bool) -> Limit {
        let at = |t: &Tone| {
            let raw = contrast_ratio(if over_darkest { t.darkest } else { t.lightest }, ink);
            Limit { raw, need: t.need, normalized: raw * MIN_CONTRAST / t.need }
        };
        self.0.iter().map(at).fold(Limit { raw: f32::MAX, need: MIN_CONTRAST, normalized: f32::MAX }, |a, b| if b.normalized < a.normalized { b } else { a })
    }

    /// The worst contrast of an ink of luminance `ink` over the typical (median) window of each area, which a few stroke-sized specks cannot move.
    fn typical(&self, ink: f32) -> f32 {
        self.0.iter().map(|t| contrast_ratio(t.median.unwrap_or(t.darkest), ink)).fold(f32::MAX, f32::min)
    }

    /// As [`Tones::over_darkest`], over the lightest window.
    fn over_lightest(&self, ink: f32) -> f32 {
        self.0.iter().map(|t| contrast_ratio(t.lightest, ink) * MIN_CONTRAST / t.need).fold(f32::MAX, f32::min)
    }
}

impl Ground {
    /// Every area of `spot` read at the scale of its type ([`Ground::read`]).
    pub fn tones(&self, spot: &Spot, fade: Option<Fade>, median: bool) -> Tones {
        Tones(spot.0.iter().map(|area| self.read(area, fade, median)).collect())
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

    /// The words of a faded closing picture in one theme: they sit on the page
    /// colour, so the dark ink and the light ink are the theme's text colour
    /// and page colour, whichever the theme makes darker (the text in the
    /// light theme, the page in the dark one). Any other picture takes
    /// [`Inks::words`]; a faded one whose colours are unresolved takes none.
    fn of_words(faded: bool, text: Option<Rgb>, page: Option<Rgb>, night: bool) -> Option<Self> {
        if !faded {
            return Some(Self::words());
        }
        let (text, page) = (luminance(text?), luminance(page?));
        Some(if night { Self { dark: page, light: text } } else { Self { dark: text, light: page } })
    }
}

/// The credit's text ink: `--moss-color-text-secondary` in the light and the
/// dark theme, which site.css repeats.
pub static CREDIT_INK: std::sync::LazyLock<(Rgb, Rgb)> = std::sync::LazyLock::new(|| token_colours("moss-color-text-secondary"));

/// The page text colour in the light and the dark theme, which a faded closing
/// picture's words take as one of their inks.
pub static TEXT_INK: std::sync::LazyLock<(Rgb, Rgb)> = std::sync::LazyLock::new(|| token_colours("moss-color-text"));

fn token_colours(name: &str) -> (Rgb, Rgb) {
    let tokens = moss_core::contract::tokens::load_tokens().unwrap_or_else(|e| panic!("tokens.json must parse: {e}"));
    let token = moss_core::contract::tokens::find_token(&tokens, name);
    let parse = |hex: Option<&str>| hex.and_then(crate::build::components::color_extract::parse_css_color).unwrap_or_else(|| panic!("--{name} has a light and a dark colour"));
    (parse(token.map(|t| t.0)), parse(token.and_then(|t| t.1)))
}

/// The ink that reads better over a spot's readings, and its contrast against
/// the worst window, in units of the AA 4.5:1 the text needs.
pub fn better_ink(tones: &Tones, inks: Inks) -> (Ink, f32) {
    let dark = tones.over_darkest(inks.dark);
    let light = tones.over_lightest(inks.light);
    if dark >= light { (Ink::Dark, dark) } else { (Ink::Light, light) }
}

/// For `panel=none` where neither ink clears [`MIN_CONTRAST`] against the worst
/// window: the worst window cannot judge between them (for the light ink it is a
/// pale hairline, for the dark ink the dark ground), so the ink with the greater
/// contrast against the median window, the part's typical tone, is taken.
pub fn better_ink_by_median(tones: &Tones, inks: Inks) -> Ink {
    if tones.over_median(inks.dark) >= tones.over_median(inks.light) { Ink::Dark } else { Ink::Light }
}

/// As [`better_ink_by_median`] for the credit's text or page colour.
pub fn credit_ink_by_median(tones: &Tones, text: Option<Rgb>, page: Option<Rgb>) -> CreditInk {
    match page {
        Some(page) if text.map_or(true, |text| tones.over_median(luminance(page)) > tones.over_median(luminance(text))) => CreditInk::Page,
        _ => CreditInk::Text,
    }
}

/// The ink whose worst case over `spot` clears [`MIN_CONTRAST`], if either.
pub fn readable_ink(grid: &Ground, spot: &Spot, fade: Option<Fade>, inks: Inks) -> Option<Ink> {
    Backing::Auto.accept(better_ink(&grid.tones(spot, fade, false), inks))
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
/// spot's readings, and its worst contrast there. The page colour is left out
/// where the build cannot read it.
pub fn credit_ink(tones: &Tones, text: Option<Rgb>, page: Option<Rgb>) -> (CreditInk, f32) {
    let worst = |ink: Rgb| tones.over_darkest(luminance(ink)).min(tones.over_lightest(luminance(ink)));
    let text = (CreditInk::Text, text.map_or(0.0, worst));
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
fn measure(lookup: &crate::build::media::dimensions::MediaDimensionLookup, href: &str) -> Option<(std::sync::Arc<Ground>, f32)> {
    let key = href.strip_prefix('/').unwrap_or(href);
    let (w, h) = lookup.get(key);
    Some((lookup.ground(key)?, h as f32 / w.max(1) as f32))
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

/// One theme around a picture: its page colour, its text colour and whether it is the dark one.
#[derive(Clone, Copy)]
struct Theme {
    page: Option<Rgb>,
    text: Option<Rgb>,
    night: bool,
}

/// What a reading measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Words,
    Credit,
}

/// One part of a hero at one layout (and, for a closing picture's words, one
/// page): the part of the picture it can cover and the worst windows there,
/// as the attributes and the warning both read them.
struct Reading {
    part: Part,
    /// The attribute token: the layout, and for a closing picture's words at
    /// the wider layouts the page's column.
    token: String,
    layout: Layout,
    spot: Spot,
    tones: Tones,
    /// The inks the words are judged between; unused by the credit.
    inks: Inks,
}

/// The words at each layout (and, on a closing picture, for each page that has
/// its own column there), and on a closing picture with a `credit` its credit
/// at each layout. A words reading is left out where the faded page colour
/// behind it is unresolved.
fn readings(grid: &Ground, aspect: f32, align: Option<&str>, frame: &Frame, credit: bool, ground: &PageGround, theme: Theme, forced: bool) -> Vec<Reading> {
    let page = theme.page;
    let words_inks = Inks::of_words(frame.faded, theme.text, page, theme.night);
    let mut out = Vec::new();
    for layout in Layout::ALL {
        let Some(fade) = Fade::resolve(frame.faded, page, layout) else { continue };
        let Some(inks) = words_inks else { continue };
        // A theme that sets the words' type outside what is read keeps their panel, unless the
        // author asked for plain ink, which takes the better ink by the stylesheet's own scale.
        if frame.closing && frame.theme.resized && !forced {
            continue;
        }
        let pages: &[Page] = if !frame.closing || layout == Layout::Narrow { &Page::ALL[..1] } else { &Page::ALL };
        for on in pages {
            let spot = words_spot(align, aspect, layout, frame, *on);
            let token = if frame.closing { on.token(layout) } else { layout.as_str().to_string() };
            out.push(Reading { part: Part::Words, token, layout, tones: grid.tones(&spot, fade, forced), spot, inks });
        }
    }
    if frame.closing && credit {
        for layout in Layout::ALL {
            let spot = credit_spot(frame, aspect, layout, ground.colophon);
            out.push(Reading { part: Part::Credit, token: layout.as_str().to_string(), layout, tones: grid.tones(&spot, None, forced), spot, inks: Inks::words() });
        }
    }
    out
}

/// The attributes that carry each picture's measured ink to the stylesheet.
/// `data-words` lists where the words sit in ink with no panel
/// (`narrow:dark wide-full:light`; absent from the list, the panel stays). A
/// closing picture lists a token for each page it can be on: the layout and
/// the page's column (`mid-default`, `wide-full-lane`, ...); a phone has one,
/// `narrow`. `data-foot` is the words' ink along the foot for the colophon.
/// `data-credit` is, per layout (`narrow`, `mid`, `wide`), the credit's ink at
/// the foot ([`CreditInk`]; absent, it keeps a small panel). The `-night` set
/// says the same of the dark theme: the `dark=` picture, or the one picture
/// again, over the dark page colour where it fades. A faded theme whose page
/// colour is unresolved measures no words, so they keep their panel.
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
    let frame = frame.on(&ground);
    let closing = frame.closing;
    let reading = |href: &str, theme: Theme, text: Option<Rgb>| {
        let page = theme.page;
        if backing == Backing::Panel && !closing {
            return None;
        }
        let (grid, aspect) = measure(lookup, href)?;
        // Under a panel nothing is measured for the words or the credit.
        let readings = if backing == Backing::Panel { Vec::new() } else { readings(&grid, aspect, align, &frame, credit, &ground, theme, backing == Backing::Never) };
        let tokens = |part: Part, ink: &dyn Fn(&Reading) -> Option<&'static str>| -> Vec<String> {
            readings.iter().filter(|r| r.part == part).filter_map(|r| Some(format!("{}:{}", r.token, ink(r)?))).collect()
        };
        let words = tokens(Part::Words, &|r| {
            let (ink, contrast) = better_ink(&r.tones, r.inks);
            let ink = if backing == Backing::Never && contrast < MIN_CONTRAST { better_ink_by_median(&r.tones, r.inks) } else { ink };
            Some(backing.accept((ink, contrast))?.as_str())
        });
        let credit = tokens(Part::Credit, &|r| {
            let (ink, contrast) = credit_ink(&r.tones, text, page);
            let ink = if backing == Backing::Never && contrast < MIN_CONTRAST { credit_ink_by_median(&r.tones, text, page) } else { ink };
            Some(backing.accept((ink, contrast))?.as_str())
        });
        let foot_tones = closing.then(|| grid.tones(&foot_spot(&frame, aspect), None, false));
        let foot = foot_tones.and_then(|tones| Backing::Auto.accept(better_ink(&tones, Inks::words())));
        let edition = closing.then(|| edition_ink(&grid, aspect, align, &frame, |layout| Fade::resolve(frame.faded, page, layout))).flatten();
        Some((words.join(" "), foot, credit.join(" "), edition))
    };
    let day = reading(light, ground.theme(false), ground.credit_light);
    // Without a `dark=` picture the one picture is shown in both themes.
    let night = reading(dark.unwrap_or(light), ground.theme(true), ground.credit_dark);
    let mut out = String::new();
    for (suffix, picture) in [("", day), ("-night", night)] {
        if let Some((words, foot, credit, edition)) = picture {
            if !words.is_empty() {
                out.push_str(&format!(r#" data-words{suffix}="{words}""#));
            }
            if let Some(ink) = foot {
                out.push_str(&format!(r#" data-foot{suffix}="{}""#, ink.as_str()));
            }
            if !credit.is_empty() {
                out.push_str(&format!(r#" data-credit{suffix}="{credit}""#));
            }
            if let Some(ink) = edition {
                out.push_str(&format!(r#" data-edition{suffix}="{}""#, ink.as_str()));
            }
        }
    }
    out
}

/// The ink `panel=none` puts under the words: the better of the two by the
/// worst window where one passes, else by the median window (see
/// [`better_ink_by_median`]), with the limiting ratio it reaches and what it
/// needs.
fn plain_words_ink(tones: &Tones, inks: Inks) -> (Limit, f32) {
    let (ink, contrast) = better_ink(tones, inks);
    let ink = if contrast < MIN_CONTRAST { better_ink_by_median(tones, inks) } else { ink };
    match ink {
        Ink::Dark => (tones.limit(inks.dark, true), inks.dark),
        Ink::Light => (tones.limit(inks.light, false), inks.light),
    }
}

/// As [`plain_words_ink`] for the credit. An ink the build cannot read counts as black.
fn plain_credit_ink(tones: &Tones, text: Option<Rgb>, page: Option<Rgb>) -> (Limit, f32) {
    let (ink, contrast) = credit_ink(tones, text, page);
    let ink = if contrast < MIN_CONTRAST { credit_ink_by_median(tones, text, page) } else { ink };
    let lum = match (ink, page, text) {
        (CreditInk::Page, Some(page), _) => luminance(page),
        (_, _, Some(text)) => luminance(text),
        _ => 0.0,
    };
    let (a, b) = (tones.limit(lum, true), tones.limit(lum, false));
    (if a.normalized <= b.normalized { a } else { b }, lum)
}

/// Where `panel=none` ink falls short: the part, the layout, the theme, the
/// worst ratio the plain ink reaches there with the ratio its text needs, and
/// the reading and ink behind it, for the ratio over the typical ground.
pub struct Shortfall {
    part: Part,
    layout: Layout,
    night: bool,
    limit: Limit,
    tones: Tones,
    ink: f32,
}

/// For `panel=none`: where the plain ink cannot be shown to reach the contrast
/// it needs, for each part, layout and theme (the worst over the page columns),
/// in the light theme over `light` and the dark one over `dark` (or `light`
/// again). The author asked for plain ink, so these are what they have taken on.
pub fn forced_ink_shortfalls(
    lookup: &crate::build::media::dimensions::MediaDimensionLookup,
    (light, dark): (&str, Option<&str>),
    align: Option<&str>,
    frame: &Frame,
    credit: bool,
) -> Vec<Shortfall> {
    let ground = lookup.page_ground();
    let frame = &frame.on(&ground);
    let mut found: Vec<Shortfall> = Vec::new();
    for (href, text, theme) in [
        (light, ground.credit_light, ground.theme(false)),
        (dark.unwrap_or(light), ground.credit_dark, ground.theme(true)),
    ] {
        let (page, night) = (theme.page, theme.night);
        let Some((grid, aspect)) = measure(lookup, href) else { continue };
        for r in readings(&grid, aspect, align, frame, credit, &ground, theme, true) {
            let limit = match r.part {
                Part::Words => plain_words_ink(&r.tones, r.inks),
                Part::Credit => plain_credit_ink(&r.tones, text, page),
            };
            let (limit, ink) = limit;
            if limit.normalized >= MIN_CONTRAST {
                continue;
            }
            match found.iter_mut().find(|f| f.part == r.part && f.layout == r.layout && f.night == night) {
                Some(known) if known.limit.normalized <= limit.normalized => {}
                Some(known) => (known.limit, known.tones, known.ink) = (limit, r.tones.clone(), ink),
                None => found.push(Shortfall { part: r.part, layout: r.layout, night, limit, tones: r.tones.clone(), ink }),
            }
        }
    }
    found
}

/// A note for each `panel=none` hero (never a problem: plain ink is the
/// author's call) saying where its ink cannot be shown to reach the contrast
/// it needs, with the reading, so the author knows what they have taken on.
/// Names the page and the hero's opening words.
pub fn note_forced_ink(
    doc: &moss_core::ast::Document,
    lookup: Option<&crate::build::media::dimensions::MediaDimensionLookup>,
    file_path: &str,
    closes_page: bool,
) {
    use moss_core::ast::{Block, Shortcode, Url};
    let Some(lookup) = lookup else { return };
    let last = doc.blocks.len().saturating_sub(1);
    fn href(u: &Url) -> Option<&str> {
        match u {
            Url::Resolved(r) => Some(r.href.as_str()),
            Url::Unresolved(_) => None,
        }
    }
    for (i, block) in doc.blocks.iter().enumerate() {
        let Block::Shortcode(Shortcode::Hero(hero)) = block else { continue };
        if hero.panel.as_deref() != Some("none") || hero.overlay.is_empty() {
            continue;
        }
        let Some(light) = hero.image.as_ref().and_then(href) else { continue };
        let frame = Frame::of(hero, closes_page && i == last);
        let shortfalls = forced_ink_shortfalls(lookup, (light, hero.dark.as_ref().and_then(href)), hero.align.as_deref(), &frame, !hero.caption.is_empty());
        if shortfalls.is_empty() {
            continue;
        }
        let where_ = |layout: Layout| match layout {
            Layout::Narrow => "on a phone",
            Layout::Mid => "from 48rem to 80rem wide",
            Layout::Wide => "from 80rem wide",
        };
        let mut groups: Vec<String> = Vec::new();
        for (part, night) in [(Part::Words, false), (Part::Words, true), (Part::Credit, false), (Part::Credit, true)] {
            let list: Vec<String> = Layout::ALL
                .into_iter()
                .filter_map(|layout| shortfalls.iter().find(|f| f.part == part && f.night == night && f.layout == layout))
                .map(|f| format!("{} ({:.1}:1 at the worst spot, {:.1}:1 over the typical ground, needs {:.1}:1)", where_(f.layout), f.limit.raw, f.tones.typical(f.ink), f.limit.need))
                .collect();
            if !list.is_empty() {
                groups.push(format!("the {} in the {} theme {}", if part == Part::Words { "words" } else { "credit" }, if night { "dark" } else { "light" }, list.join(", ")));
            }
        }
        let words: String = hero.overlay_text.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
        crate::build::cli_output::log_note!(
            "[{file_path}] `:::hero {{panel=none}}` (\"{words}\"): plain ink, as asked. The ink cannot be shown to read at {}. Look at the picture at every width in both themes; `panel=auto` keeps the panel where the ink does not read.",
            groups.join("; ")
        );
    }
}

/// A closing picture's `max-height=` at 48rem, where it is least (a px or vw
/// cap never shrinks as the window widens), when it is under what the box
/// must hold at its tallest: [`WORDS_PX`], which it clips past, and with a
/// credit its height more, since the credit at the foot shares the words'
/// column there and would otherwise sit over them. Returns the cap and what
/// the box needs.
pub fn short_cap(hero: &moss_core::ast::HeroShortcode) -> Option<(f32, f32)> {
    let width = Layout::Mid.widths().0;
    let px = hero.max_height?.px(width);
    let credit = moss_core::ast::plain_text::inlines_to_plain_text(&hero.caption);
    let need = if credit.is_empty() { WORDS_PX } else { WORDS_PX + credit_height(Words::read("", &credit).credit, width) };
    (px < need).then_some((px, need))
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
        if let Some((px, need)) = short_cap(hero).filter(|_| closing && !hero.overlay.is_empty()) {
            crate::build::cli_output::log_warn_problem!(
                "[{file_path}] `:::hero {{max-height=…}}` is {px:.0}px tall at 768px wide, under the {need:.0}px its words and credit can take, so they may be cut off or covered. Raise the cap's smallest value."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::media::dimensions::MediaDimensionLookup;
    use crate::types::content::MediaMetadata;
    use image::{DynamicImage, Rgb, RgbImage};

    const PAGE: Page = Page { column: Column::Default, contents: false };

    /// A closing picture's `data-words` when every page and layout reads `ink`.
    fn closing_tokens(ink: &str) -> String {
        Layout::ALL.into_iter().flat_map(|l| if l == Layout::Narrow { vec![Page::ALL[0]] } else { Page::ALL.to_vec() }.into_iter().map(move |p| format!("{}:{ink}", p.token(l)))).collect::<Vec<_>>().join(" ")
    }

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
            for x in 96..160 {
                img.put_pixel(x, y, Rgb(INK));
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    /// An ordinary hero's frame.
    fn ordinary() -> Frame {
        Frame::new(None, None, None, false, false, Words::default())
    }

    /// A closing picture's, uncapped, with the default position for `align`.
    fn closing(align: Option<&str>) -> Frame {
        Frame::new(None, None, align, false, true, Words::default())
    }

    fn faded_frame() -> Frame {
        Frame::new(None, None, Some("top"), true, true, Words::default())
    }

    fn plain(align: Option<&str>, backing: Backing, closing: bool) -> InkRequest<'_> {
        InkRequest { align, backing, frame: Frame::new(None, None, align, false, closing, Words::default()), credit: true, ground: PageGround::default() }
    }

    fn lookup_of(pictures: &[(&str, &DynamicImage)]) -> MediaDimensionLookup {
        let images: Vec<MediaMetadata> = pictures
            .iter()
            .map(|(path, img)| MediaMetadata { path: path.to_string(), dimensions: Some((img.width(), img.height())), ..Default::default() })
            .collect();
        let grounds = pictures.iter().map(|(path, img)| (path.to_string(), Ground::from_image(img))).collect();
        MediaDimensionLookup::new(&images, &[], &Default::default(), None).with_grounds(grounds)
    }

    #[test]
    fn a_pale_sky_takes_dark_ink_and_its_dark_ground_takes_light() {
        let grid = Ground::from_image(&banded(SKY, GROUND, 0.7));
        for layout in Layout::ALL {
            assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, layout, &ordinary(), PAGE), None, Inks::words()), Some(Ink::Dark));
        }
        assert_eq!(readable_ink(&grid, &foot_spot(&closing(Some("top")), 70.0 / 160.0), None, Inks::words()), Some(Ink::Light));
    }

    #[test]
    fn a_mid_tone_area_takes_neither() {
        let grid = Ground::from_image(&banded([120; 3], [120; 3], 0.7));
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, &ordinary(), PAGE), None, Inks::words()), None);
    }

    #[test]
    fn a_far_dark_patch_spares_the_wide_box_but_not_a_phone() {
        let grid = Ground::from_image(&sky_with_far_patch());
        // Wide: the words box is the start 47% of the width; the patch is at the far end.
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, &ordinary(), PAGE), None, Inks::words()), Some(Ink::Dark));
        // `align=end` words sit at the foot of the picture, below the patch.
        assert_eq!(readable_ink(&grid, &words_spot(Some("end"), 0.4, Layout::Wide, &ordinary(), PAGE), None, Inks::words()), Some(Ink::Dark));
        // A phone runs the words across the whole width.
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Narrow, &ordinary(), PAGE), None, Inks::words()), None);
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
        let grid = Ground::from_image(&DynamicImage::ImageRgb8(img));
        assert_eq!(readable_ink(&grid, &words_spot(None, 0.8, Layout::Wide, &ordinary(), PAGE), None, Inks::words()), Some(Ink::Dark));
        assert_eq!(readable_ink(&grid, &words_spot(None, 0.8, Layout::Narrow, &ordinary(), PAGE), None, Inks::words()), None);
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
            Ground::from_image(&DynamicImage::ImageRgb8(img))
        };
        let top = words_spot(Some("top"), 0.8, Layout::Wide, &closing(Some("top")), PAGE);
        assert_eq!(readable_ink(&band(40, 80), &top, None, Inks::words()), Some(Ink::Dark));
        assert_eq!(readable_ink(&band(16, 24), &top, None, Inks::words()), None);
        assert_eq!(readable_ink(&band(0, 40), &foot_spot(&closing(Some("top")), 0.8), None, Inks::words()), Some(Ink::Dark));
        // The foot is the colophon's row, 36px of the 1024: a band above it is not under it.
        assert_eq!(readable_ink(&band(40, 70), &foot_spot(&closing(Some("top")), 0.8), None, Inks::words()), Some(Ink::Dark));
        assert_eq!(readable_ink(&band(68, 80), &foot_spot(&closing(Some("top")), 0.8), None, Inks::words()), Some(Ink::Light));
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
        let grid = Ground::from_image(&DynamicImage::ImageRgb8(img));
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, &closing(Some("top")), PAGE), None, Inks::words()), None);
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, &closing(Some("top")), Page { column: Column::Full, contents: false }), None, Inks::words()), Some(Ink::Dark));
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
            Ground::from_image(&DynamicImage::ImageRgb8(img))
        };
        let spot = |is_closing: bool| words_spot(Some("top"), 0.4, Layout::Wide, &if is_closing { closing(Some("top")) } else { ordinary() }, PAGE);
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
        let tokens = format!(r#"="{}""#, closing_tokens("dark"));
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
            ..Default::default()
        };
        let overrides = std::collections::HashMap::from([("評選".to_string(), "awards".to_string())]);
        let lookup = MediaDimensionLookup::new(&[meta], &[], &overrides, None).with_grounds(vec![("評選/art.png".to_string(), Ground::from_image(&img))]);
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
        InkRequest { align, backing: Backing::Auto, frame: Frame::new(None, None, align, true, true, Words::default()), credit: true, ground }
    }

    #[test]
    fn a_faded_top_is_measured_over_the_page_it_fades_into() {
        let grid = Ground::from_image(&strip_at_the_top());
        let (light, dark) = *BACKGROUNDS;
        for layout in Layout::ALL {
            let spot = words_spot(Some("top"), 70.0 / 160.0, layout, &faded_frame(), PAGE);
            assert_eq!(readable_ink(&grid, &spot, None, Inks::words()), None, "{layout:?}: the strip is too dark for either ink");
            assert_eq!(readable_ink(&grid, &spot, Some(Fade::new(light, layout)), Inks::words()), Some(Ink::Dark), "{layout:?}: faded into a light page");
            assert_eq!(readable_ink(&grid, &spot, Some(Fade::new(dark, layout)), Inks::words()), None, "{layout:?}: dark page above a pale sky");
        }
        // The foot is below the fade.
        assert_eq!(readable_ink(&grid, &foot_spot(&closing(Some("top")), 70.0 / 160.0), Some(Fade::new(light, Layout::Wide)), Inks::words()), Some(Ink::Light));
    }

    #[test]
    fn a_faded_tops_words_are_judged_in_the_themes_own_colours() {
        let (text, page) = ((200, 190, 170), (10, 20, 30));
        let (day, night) = (Inks::of_words(true, Some(text), Some(page), false).unwrap(), Inks::of_words(true, Some(text), Some(page), true).unwrap());
        assert_eq!((day.dark, day.light), (luminance(text), luminance(page)));
        assert_eq!((night.dark, night.light), (luminance(page), luminance(text)));
        let fixed = Inks::words();
        assert_eq!(Inks::of_words(false, Some(text), Some(page), false).map(|i| (i.dark, i.light)), Some((fixed.dark, fixed.light)));
        assert!(Inks::of_words(true, None, Some(page), false).is_none(), "an unreadable text colour leaves the panel");
    }

    #[test]
    fn a_faded_closing_picture_reads_each_theme_over_its_own_page_colour() {
        let lookup = lookup_of(&[("strip.png", &strip_at_the_top()), ("night.png", &banded([20, 28, 48], [8, 10, 14], 0.7))]);
        let one = ink_attributes(Some(&lookup), "/strip.png", None, faded(Some("top"), PageGround::default()));
        assert!(one.contains(&format!(r#"data-words="{}""#, closing_tokens("dark"))), "{one}");
        assert!(!one.contains("data-words-night"), "the light picture over a dark page reads in neither ink: {one}");
        assert!(one.contains(r#"data-foot-night="light""#), "{one}");
        let pair = ink_attributes(Some(&lookup), "/strip.png", Some("/night.png"), faded(Some("top"), PageGround::default()));
        assert!(pair.contains(&format!(r#"data-words-night="{}""#, closing_tokens("light"))), "{pair}");
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
        let lookup = lookup_of(&[("dark-foot.png", &banded(SKY, GROUND, 0.4)), ("pale-foot.png", &banded(GROUND, SKY, 0.6))]);
        let dark = ink_attributes(Some(&lookup), "/dark-foot.png", None, plain(Some("top"), Backing::Auto, true));
        assert!(dark.contains(r#"data-credit="narrow:page mid:page wide:page""#) && dark.contains(r#"data-credit-night="narrow:text mid:text wide:text""#), "{dark}");
        let pale = ink_attributes(Some(&lookup), "/pale-foot.png", None, plain(Some("top"), Backing::Auto, true));
        assert!(pale.contains(r#"data-credit="narrow:text mid:text wide:text""#) && pale.contains(r#"data-credit-night="narrow:page mid:page wide:page""#), "{pale}");
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
        assert_eq!(PageGround::of_theme(None), PageGround::default());
        assert_eq!(PageGround::of_theme(Some("h1 { color: red; }")), PageGround::default());
        let root = PageGround::of_theme(Some(":root { --moss-color-bg: #ffffff; }"));
        assert_eq!(root, PageGround { light: Some((255, 255, 255)), dark: Some((255, 255, 255)), ..PageGround::default() }, "a `:root` colour wins in dark too");
        let both = PageGround::of_theme(Some(":root { --moss-color-bg: #fff }\n:root[data-theme='dark'] { --moss-color-bg: rgb(10, 20, 30) !important; }"));
        assert_eq!(both, PageGround { light: Some((255, 255, 255)), dark: Some((10, 20, 30)), ..PageGround::default() });
        let dark_only = PageGround::of_theme(Some(":root[data-theme=\"dark\"] { --moss-color-bg: #000; }"));
        assert_eq!(dark_only, PageGround { light: Some(light), dark: Some((0, 0, 0)), ..PageGround::default() });
        for unread in [
            ":root { --moss-color-bg: var(--paper); }",
            ":root { --moss-color-bg: color-mix(in oklch, white, tan); }",
            "@media (prefers-color-scheme: dark) { :root { --moss-color-bg: #000; } }",
            "body { --moss-color-bg: #fff; }",
        ] {
            let ground = PageGround::of_theme(Some(unread));
            assert!(ground.light.is_none() || ground.dark.is_none(), "{unread}: {ground:?}");
        }
        assert_eq!(PageGround::of_theme(Some(":root { --moss-color-bg: var(--paper); }")), PageGround { light: None, dark: None, ..PageGround::default() });
    }

    #[test]
    fn site_css_paints_the_credit_in_the_theme_token_it_is_measured_in() {
        let css = crate::build::emit::stylesheet::CORE_CSS;
        assert!(css.contains("color: var(--moss-color-text-secondary);") && !css.contains("color: #5d5853;"));
        let tokens = moss_core::contract::tokens::load_tokens().unwrap();
        let token = moss_core::contract::tokens::find_token(&tokens, "moss-color-text-secondary").unwrap();
        let hex = |(r, g, b): crate::build::scan::section_text::Rgb| format!("#{r:02x}{g:02x}{b:02x}");
        assert_eq!((hex(CREDIT_INK.0), hex(CREDIT_INK.1)), (token.0.to_string(), token.1.unwrap().to_string()), "the default is the token's own value");
    }

    /// A 100x77 sky (the shape of a 1400x1073 print) with a dark band over `rows` of its height.
    fn sky_with_band(rows: (u32, u32)) -> Ground {
        let mut img = RgbImage::from_pixel(100, 77, Rgb(SKY));
        for y in rows.0..rows.1 {
            for x in 0..100 {
                img.put_pixel(x, y, Rgb(INK));
            }
        }
        Ground::from_image(&DynamicImage::ImageRgb8(img))
    }

    fn capped() -> Frame {
        Frame::new(Some(moss_core::media::Position::Focus(50, 72)), moss_core::media::MaxHeight::parse("clamp(420px, 50vw, 680px)"), Some("top"), true, true, Words::default())
    }

    #[test]
    fn a_capped_closing_picture_is_measured_over_the_rows_its_box_shows() {
        // At 1280px the cap is 640px of a 980px-tall picture, anchored at 72%:
        // the box shows rows 25% to 90%, so a band over the top 10% is cropped away.
        let aspect = 77.0 / 100.0;
        let top = sky_with_band((0, 8));
        for layout in [Layout::Mid, Layout::Wide] {
            let spot = words_spot(Some("top"), aspect, layout, &capped(), PAGE);
            assert_eq!(readable_ink(&top, &spot, None, Inks::words()), Some(Ink::Dark), "{layout:?}: the band is above the crop");
            let uncropped = words_spot(Some("top"), aspect, layout, &closing(Some("top")), PAGE);
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
        let capped_spot = Spot(vec![Crop::cover(77.0 / 100.0, (1280.0, 640.0), (0.5, 0.72)).area((0.0, 0.2), (0.0, 1.0), Face::SMALL)]);
        assert_eq!(readable_ink(&band, &capped_spot, fade, Inks::words()), Some(Ink::Dark));
        assert_ne!(readable_ink(&band, &Spot::uncropped((0.26, 0.32), (0.0, 1.0), 1280.0, Face::SMALL), fade, Inks::words()), Some(Ink::Dark), "uncropped, the band shows through the fade");
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
        let grid = Ground::from_image(&DynamicImage::ImageRgb8(img));
        let faded = faded_frame();
        let spot = words_spot(Some("top"), 70.0 / 160.0, Layout::Narrow, &faded, PAGE);
        assert_eq!(readable_ink(&grid, &spot, None, Inks::words()), Some(Ink::Dark));
        // Without the fade the phone's box is set by its words, so the whole width counts.
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 70.0 / 160.0, Layout::Narrow, &closing(Some("top")), PAGE), None, Inks::words()), None);
        // The rows: a band under the words (at 50% of the box) keeps the panel.
        let low = sky_with_band((38, 42));
        assert_eq!(readable_ink(&low, &words_spot(Some("top"), 77.0 / 100.0, Layout::Narrow, &faded, PAGE), None, Inks::words()), None);
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
        assert_eq!(foot(Frame::new(None, cap, Some("top"), false, true, Words::default())), Some(Ink::Dark));
        assert_eq!(foot(Frame::new(Some(moss_core::media::Position::Top), cap, Some("top"), false, true, Words::default())), None);
        // An ordinary hero keeps its top unless it names a position.
        assert_eq!(Frame::new(None, None, Some("top"), false, false, Words::default()).position, (0.5, 0.0));
    }

    #[test]
    fn a_cap_under_the_words_box_is_found_at_48rem() {
        let hero = |cap: &str| moss_core::ast::HeroShortcode { max_height: moss_core::media::MaxHeight::parse(cap), ..Default::default() };
        assert_eq!(short_cap(&hero("300px")).map(|c| c.0), Some(300.0));
        assert_eq!(short_cap(&hero("25vw")).map(|c| c.0), Some(192.0));
        assert_eq!(short_cap(&hero("clamp(420px, 50vw, 680px)")), None);
        // The credit at the foot needs its rows too: 370px holds the words, not both.
        let captioned = moss_core::ast::HeroShortcode { caption: vec![moss_core::ast::Inline::Text("Credit".into())], ..hero("370px") };
        assert_eq!(short_cap(&captioned).map(|c| c.0), Some(370.0));
        assert_eq!(short_cap(&hero("370px")), None);
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
        assert!(!forced_ink_shortfalls(&lookup, ("/mid.png", None), Some("top"), &ordinary(), false).is_empty());
        assert!(forced_ink_shortfalls(&lookup, ("/day.png", None), Some("top"), &ordinary(), false).is_empty());
        // A closing picture's credit counts too: quiet under the words, mid-tone at the foot.
        let lookup = lookup_of(&[("foot.png", &banded(SKY, [120; 3], 0.7))]);
        assert!(forced_ink_shortfalls(&lookup, ("/foot.png", None), Some("top"), &closing(Some("top")), false).is_empty());
        assert!(forced_ink_shortfalls(&lookup, ("/foot.png", None), Some("top"), &closing(Some("top")), true).iter().any(|s| s.part == Part::Credit));
    }

    /// A print the shape of a 480x368 etching: a pale sky over a dark
    /// landscape, trees in the right half rising into the sky, a dark foot.
    fn etching() -> DynamicImage {
        let mut img = RgbImage::from_pixel(480, 368, Rgb([225, 232, 240]));
        for (x, y, p) in img.enumerate_pixels_mut() {
            let (fx, fy) = (x as f32 / 480.0, y as f32 / 368.0);
            let ground = ((fy - 0.6) / 0.06).clamp(0.0, 1.0);
            let tree = if (0.7..0.95).contains(&fx) && fy > 0.6 { 1.0 } else { 0.0 };
            let dark = ground.max(tree);
            *p = Rgb([0, 1, 2].map(|i| (f32::from(p[i]) + (f32::from([40u8, 62, 104][i]) - f32::from(p[i])) * dark) as u8));
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn a_short_heading_reads_over_the_faded_sky_above_the_landscape() {
        // A heading and a link end well inside the fade, left of the trees.
        let lookup = lookup_of(&[("etching.png", &etching())]);
        let hero = ":::hero {image=\"etching.png|focus:50%,72%\" align=top fade=top max-height=\"clamp(440px, 50vw, 680px)\" caption=\"A credit.\"}\n## Open the shelf. What next?\n\n[Propose](#propose)\n:::";
        let hero = moss_core::ast::parse(hero);
        let moss_core::ast::Block::Shortcode(moss_core::ast::Shortcode::Hero(hero)) = &hero.blocks[0] else { panic!("a hero") };
        let request = InkRequest { align: Some("top"), backing: Backing::Auto, frame: Frame::of(hero, true), credit: true, ground: PageGround::default() };
        let attrs = ink_attributes(Some(&lookup), "/etching.png", None, request);
        assert!(attrs.contains(&format!(r#"data-words="{}""#, closing_tokens("dark"))), "{attrs}");
        assert!(attrs.contains(r#"data-credit="narrow:page mid:page wide:page""#), "a dark foot: {attrs}");
    }

    #[test]
    fn a_long_heading_reaches_the_landscape_and_keeps_its_panel() {
        let lookup = lookup_of(&[("etching.png", &etching())]);
        let long = "A long heading that wraps across several lines of the words box on a wide screen and carries on, and on, and on until it reaches down into the landscape, ".repeat(3);
        let hero = format!(":::hero {{image=\"etching.png|focus:50%,72%\" align=top fade=top max-height=\"clamp(440px, 50vw, 680px)\"}}\n## {long}\n\n[Propose](#propose)\n:::");
        let hero = moss_core::ast::parse(&hero);
        let moss_core::ast::Block::Shortcode(moss_core::ast::Shortcode::Hero(hero)) = &hero.blocks[0] else { panic!("a hero") };
        let request = InkRequest { align: Some("top"), backing: Backing::Auto, frame: Frame::of(hero, true), credit: false, ground: PageGround::default() };
        let attrs = ink_attributes(Some(&lookup), "/etching.png", None, request);
        assert!(!attrs.contains("wide-full:dark"), "{attrs}");
    }

    #[test]
    fn the_credit_is_measured_where_its_text_sits_at_the_end_of_the_column() {
        // A dark foot with a pale field over its first 30%, and a pale line in the last pixel column,
        // in the margin beyond where any page's text column ends.
        let mut img = RgbImage::from_pixel(200, 100, Rgb(GROUND));
        for y in 0..100 {
            for x in (0..60).chain(199..200) {
                img.put_pixel(x, y, Rgb(SKY));
            }
        }
        let grid = Ground::from_image(&DynamicImage::ImageRgb8(img));
        let page = Some(BACKGROUNDS.0);
        let credit = |text: &str, layout| {
            let frame = Frame { words: Words::read("## Words", text), ..closing(Some("top")) };
            credit_ink(&grid.tones(&credit_spot(&frame, 0.5, layout, true), None, false), Some(CREDIT_INK.0), page)
        };
        for layout in [Layout::Narrow, Layout::Wide] {
            let (ink, worst) = credit("A credit.", layout);
            assert_eq!((ink, worst >= MIN_CONTRAST), (CreditInk::Page, true), "{layout:?}: a short credit sits at the column's end, clear of the margin and of the start");
        }
        let long = "A credit that runs the whole width of the column and wraps, so that it reaches back over the start of the foot. ".repeat(2);
        for layout in [Layout::Narrow, Layout::Wide] {
            assert!(credit(&long, layout).1 < MIN_CONTRAST, "{layout:?}: a long credit reaches the pale field");
        }
    }

    #[test]
    fn the_shortfall_names_the_part_layout_and_theme_with_the_real_ratio_and_sees_the_fade() {
        let lookup = lookup_of(&[("strip.png", &strip_at_the_top())]);
        // Faded into a light page the strip reads; into the dark page it does not.
        let shortfalls = forced_ink_shortfalls(&lookup, ("/strip.png", None), Some("top"), &faded_frame(), false);
        assert!(!shortfalls.is_empty() && shortfalls.iter().all(|s| s.night && s.part == Part::Words), "only the dark theme falls short");
        let first = &shortfalls[0];
        assert!(first.limit.raw < first.limit.need && (first.limit.need == 3.0 || first.limit.need == 4.5), "{:?}", first.limit);
    }

    /// A closing frame with these words, uncropped.
    fn saying(overlay: &str) -> Frame {
        Frame { words: Words::read(overlay, ""), ..closing(Some("top")) }
    }

    #[test]
    fn the_words_are_measured_where_the_text_column_starts_at_a_wide_window() {
        // On a full-width page the words start at 72px of 1280px and at 712px of
        // 2560px, so their lines reach 42% of the first and 53% of the second. A
        // subject at 47-53% sits under them only at the wide window.
        let mut img = RgbImage::from_pixel(200, 80, Rgb(SKY));
        for y in 0..32 {
            for x in 94..106 {
                img.put_pixel(x, y, Rgb(INK));
            }
        }
        let grid = Ground::from_image(&DynamicImage::ImageRgb8(img));
        let frame = saying("## A heading long enough to set over two lines of the words box\n\n[Go](#go)");
        assert_eq!(readable_ink(&grid, &words_spot(Some("top"), 0.4, Layout::Wide, &frame, Page { column: Column::Full, contents: false }), None, Inks::words()), None);
    }

    #[test]
    fn a_theme_that_sets_the_words_outside_what_is_read_keeps_their_panel() {
        let lookup = lookup_of(&[("sky.png", &banded(SKY, SKY, 1.0))]);
        let read = |css: Option<&str>| {
            let ground = PageGround::of_theme(css);
            ink_attributes(Some(&lookup), "/sky.png", None, InkRequest { ground, ..plain(Some("top"), Backing::Auto, true) })
        };
        let words = |css: Option<&str>| {
            let attrs = read(css);
            attrs.contains("data-words=")
        };
        assert!(words(None));
        assert!(words(Some(".moss-hero-content h2 { font-size: 3rem; }")), "a size in the `max-height=` grammar is read");
        assert!(!words(Some(".moss-hero-content h2 { font-size: calc(2rem + 2px); }")), "outside the grammar");
        assert!(!words(Some(".moss-hero-content h2 { font-weight: 300; }")), "a light weight");
        assert!(words(Some(".moss-hero-content h2 { font-weight: 500; }")));
    }

    #[test]
    fn the_type_a_theme_sets_is_read_or_given_up() {
        let read = TypeTheme::of_theme(Some(":root { --moss-reading-size-base: 1.0625rem; --moss-container-padding: 16px; }"));
        assert!(!read.resized && read != TypeTheme::default(), "{read:?}");
        let sized = TypeTheme::of_theme(Some(".moss-hero-closing h2 { font-size: clamp(24px, 3vw, 44px); } .moss-hero-closing p { font-size: 1.1rem; }"));
        assert!(!sized.resized && sized.heading_size.is_some() && sized.text_size.is_some(), "{sized:?}");
        for moves in [
            "@media (width < 48rem) { :root { --moss-container-padding: 1rem; } }",
            ":root { --moss-container-padding: clamp(1rem, 5vw, 2rem); }",
            ":root { --moss-reading-size: 20px; }",
            ":root { --moss-content-width: 800px; }",
            ".moss-hero-closing h2 { text-transform: uppercase; }",
            ".moss-hero-closing h2 { letter-spacing: 0.1em; }",
            ".moss-hero-closing p { font-family: serif; }",
            "body > footer.container h2 { font-size: calc(2rem + 2px); }",
            ".moss-hero-closing p { font-size: calc(1rem + 2px); }",
            ".moss-hero-closing h2 { font-weight: 200; }",
            "body { --moss-reading-size-base: 1rem; }",
        ] {
            assert!(TypeTheme::of_theme(Some(moves)).resized, "{moves}");
        }
        assert!(!TypeTheme::of_theme(Some("h1 { font-size: 3rem; } .card { padding: 1rem; }")).resized);
        let tokens = TypeTheme::of_theme(Some(":root { --moss-space-md: 1.25rem; --moss-size-sm: 15px; --moss-size-lg: 1.2rem; }"));
        assert!(!tokens.resized && tokens != TypeTheme::default(), "plain spacing and size tokens are read, not given up: {tokens:?}");
        assert!(!TypeTheme::of_theme(Some("body > footer.container h2 { color: red; }")).resized, "a rule that does not move the words");
    }

    #[test]
    fn site_css_sets_the_type_the_words_are_modelled_in() {
        let css = crate::build::emit::stylesheet::CORE_CSS;
        for rule in ["font-size: 2.5rem;", "font-size: 1.75rem;", "font-size: 1.4rem;", "font-size: var(--moss-size-lg);", "--moss-words-pad: 5%;", "font-weight: 500;", "padding-block: 0 0.875rem;", "--moss-credit-end: calc(var(--moss-words-inset) - 0.5rem);", "padding-block-start: clamp(8px, 2vw, 24px);", "line-height: 1.08;", "--moss-words-pad: var(--moss-space-md);", "--moss-contents-inset: 32px;", "--moss-contents-pad: 8px;", "--moss-contents-row: 20px;", "--moss-contents-gutter: 24px;"] {
            assert!(css.contains(rule), "{rule}");
        }
        assert_eq!(32.0 + 2.0 * 8.0 + 20.0 + 24.0, words::LANE_PX);
    }

    /// What the `closing-picture` render gate asserts the real heading and link
    /// boxes sit inside: the model's rows and columns for its words, as px from
    /// the picture's top and left edges, for a default-width page.
    /// `SNAPSHOTS=overwrite` rewrites the file the gate reads.
    #[test]
    fn the_model_rows_the_render_gate_holds_the_stylesheet_to_are_current() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let site = std::fs::read_to_string(root.join("tests/e2e/helpers/closing-picture-site.ts")).unwrap();
        let quoted = |name: &str| site.lines().find_map(|l| l.strip_prefix(&format!("export const {name} = \""))?.strip_suffix("\";")).unwrap_or_else(|| panic!("{name}")).to_string();
        let words = Words::read(&format!("## {}\n\n[{}](https://example.org/new)", quoted("HEADING"), quoted("LINK_TEXT")), "");
        let theme = TypeTheme::default();
        let mut model = serde_json::Map::new();
        for width in [390.0_f32, 768.0, 1360.0, 2560.0] {
            // The gate's page is a default-width page with no contents lane.
            let mut boxes = vec![[f32::MAX, f32::MAX, f32::MIN, f32::MIN]; 2];
            let t = Type::at(width, &theme, PAGE.column, false);
            for (i, block) in t.blocks(&words).into_iter().enumerate() {
                boxes[i] = [t.inset, block.rows.0, t.inset + block.across, block.rows.1];
            }
            let round = |v: f32| (v * 10.0).round() / 10.0;
            let named = |b: [f32; 4]| serde_json::json!({ "left": round(b[0]), "top": round(b[1]), "right": round(b[2]), "bottom": round(b[3]) });
            model.insert(format!("{width}"), serde_json::json!({ "heading": named(boxes[0]), "link": named(boxes[1]) }));
        }
        let current = serde_json::to_string_pretty(&serde_json::Value::Object(model)).unwrap() + "\n";
        let path = root.join("tests/render-gates/site/closing-picture-model.json");
        if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
            std::fs::write(&path, &current).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), current, "run with SNAPSHOTS=overwrite to rewrite {}", path.display());
    }

    /// A `w` x `h` pale picture with dark lines `line` px thick every `period` px,
    /// horizontal, over all of it.
    fn hatched(w: u32, h: u32, line: u32, period: u32) -> Ground {
        let mut img = RgbImage::from_pixel(w, h, Rgb([240, 240, 240]));
        for (_, y, p) in img.enumerate_pixels_mut() {
            if y % period < line {
                *p = Rgb([0, 0, 0]);
            }
        }
        Ground::from_image(&DynamicImage::ImageRgb8(img))
    }

    /// A heading's face, 40px at the default weight: large text.
    const HEADING: Face = Face { font: 40.0, need: 3.0 };

    /// The picture's middle, `css` px wide over its width (2 source px a CSS px).
    fn middle(css: f32, face: Face) -> Spot {
        Spot::uncropped((0.2, 0.8), (0.1, 0.9), css, face)
    }

    #[test]
    fn hairline_hatching_under_a_heading_gets_ink_and_thicker_lines_keep_the_panel() {
        // 2000px over 1000 CSS px: a 40px heading's window is 0.08em = 3.2 CSS px = 6 source px.
        let em = 40.0 * 2.0;
        let hairline = hatched(2000, 600, (0.05 * em) as u32, (0.12 * em) as u32);
        let bold = hatched(2000, 600, (0.15 * em) as u32, (0.30 * em) as u32);
        assert_eq!(readable_ink(&hairline, &middle(1000.0, HEADING), None, Inks::words()), Some(Ink::Dark));
        assert_eq!(readable_ink(&bold, &middle(1000.0, HEADING), None, Inks::words()), None);
        // The same hairlines under credit-sized type are resolved by its smaller window.
        assert_eq!(readable_ink(&hairline, &middle(1000.0, Face::SMALL), None, Inks::words()), None);
    }

    #[test]
    fn a_letter_sized_blot_under_a_small_part_of_the_region_keeps_the_panel() {
        // A 0.5em square of the heading in a region of 1000 x 300 CSS px: under a twentieth of it.
        let mut img = RgbImage::from_pixel(2000, 600, Rgb([240, 240, 240]));
        for y in 300..340 {
            for x in 900..940 {
                img.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        let ground = Ground::from_image(&DynamicImage::ImageRgb8(img));
        assert_eq!(readable_ink(&ground, &middle(1000.0, HEADING), None, Inks::words()), None);
    }

    #[test]
    fn a_stroke_wide_stripe_counts_in_full_wherever_it_falls() {
        for offset in 0..9 {
            let mut img = RgbImage::from_pixel(2000, 600, Rgb([240, 240, 240]));
            for y in 0..600 {
                for x in (1000 + offset)..(1006 + offset) {
                    img.put_pixel(x, y, Rgb([0, 0, 0]));
                }
            }
            let ground = Ground::from_image(&DynamicImage::ImageRgb8(img));
            assert_eq!(readable_ink(&ground, &middle(1000.0, HEADING), None, Inks::words()), None, "offset {offset}");
        }
    }

    #[test]
    fn one_texture_reads_differently_on_a_wide_screen_and_a_phone() {
        // Lines 1px thick every 6px on a 1400x2100px picture: at 2560px wide a 40px heading's
        // window is a source pixel and resolves them; a phone shows the picture 390px wide, so a
        // 22px heading's window is 6px or more and averages them away.
        let ground = hatched(1400, 2100, 1, 6);
        let frame = saying("## A short heading");
        let read = |layout| readable_ink(&ground, &words_spot(Some("top"), 1.5, layout, &frame, PAGE), None, Inks::words());
        assert_eq!(read(Layout::Wide), None);
        assert_eq!(read(Layout::Narrow), Some(Ink::Dark));
    }

    #[test]
    fn a_heading_under_24px_needs_the_full_contrast_and_a_larger_one_only_three() {
        let ground = Ground::from_image(&banded([120, 120, 120], [120, 120, 120], 1.0));
        let reads = |face| better_ink(&ground.tones(&Spot::uncropped((0.0, 1.0), (0.0, 1.0), 1000.0, face), None, false), Inks::words()).1;
        let (large, small) = (reads(Face { font: 40.0, need: 3.0 }), reads(Face { font: 22.4, need: 4.5 }));
        assert!((large / small - 1.5).abs() < 0.01, "{large} {small}");
        let heading = |font: f32, weight: u16| TypeTheme { heading_size: moss_core::media::MaxHeight::parse(&format!("{font}px")), heading_weight: Some(weight), ..Default::default() };
        let face = |t: TypeTheme| Type::at(1400.0, &t, Column::Default, false).blocks(&Words::read("## Heading", "")).remove(0).face;
        assert_eq!(face(heading(24.0, 480)).need, 3.0);
        assert_eq!(face(heading(23.0, 480)).need, 4.5);
        assert_eq!(face(heading(19.0, 700)).need, 3.0, "18.66px at weight 700 is large");
    }

    #[test]
    fn a_theme_that_sets_the_heading_size_is_measured_at_that_size() {
        // Lines 1px thick every 8px: a 40px heading's window of 3px averages them away, an 8px one's resolves them.
        let sky = hatched(2048, 880, 1, 8);
        let frame = saying("## A short heading");
        let read = |frame: &Frame| readable_ink(&sky, &words_spot(Some("top"), 880.0 / 2048.0, Layout::Wide, frame, PAGE), None, Inks::words());
        assert_eq!(read(&frame), Some(Ink::Dark));
        let small = TypeTheme::of_theme(Some(".moss-hero-content h2 { font-size: 8px; }"));
        assert!(!small.resized);
        assert_eq!(read(&Frame { theme: small, ..frame }), None);
    }

    #[test]
    fn a_dark_shape_only_another_column_reaches_keeps_ink_on_this_page() {
        // At 2560px the full-width page starts its words at 28% and they reach 53%; a
        // default-width page's start at 36% and reach 61%. A shape at 56-60% is under the
        // words of the second only.
        let mut img = RgbImage::from_pixel(2048, 800, Rgb([240, 240, 240]));
        for y in 0..320 {
            for x in 1150..1230 {
                img.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        let ground = Ground::from_image(&DynamicImage::ImageRgb8(img));
        let frame = saying("## A heading long enough to set over two lines of the words box\n\n[Go](#go)");
        let on = |column| readable_ink(&ground, &words_spot(Some("top"), 800.0 / 2048.0, Layout::Wide, &frame, Page { column, contents: false }), None, Inks::words());
        assert_eq!(on(Column::Full), Some(Ink::Dark));
        assert_eq!(on(Column::Default), None);
    }

    #[test]
    fn plain_ink_over_a_dark_ground_with_pale_hairlines_takes_the_light_ink() {
        // A dark foot with a pale line on every fourth row, under credit-sized type: the worst
        // window for each ink is the other tone, so neither passes; the typical tone is dark.
        let mut img = RgbImage::from_pixel(2000, 600, Rgb([20, 28, 48]));
        for (_, y, p) in img.enumerate_pixels_mut() {
            if y % 4 == 0 {
                *p = Rgb([230, 235, 240]);
            }
        }
        let ground = Ground::from_image(&DynamicImage::ImageRgb8(img));
        let tones = ground.tones(&Spot::uncropped((0.2, 0.8), (0.1, 0.9), 1000.0, Face::SMALL), None, true);
        assert!(better_ink(&tones, Inks::words()).1 < MIN_CONTRAST, "neither ink clears it");
        assert_eq!(better_ink_by_median(&tones, Inks::words()), Ink::Light);
        assert_eq!(credit_ink_by_median(&tones, Some(CREDIT_INK.0), Some(BACKGROUNDS.0)), CreditInk::Page);
        // Through the attributes: `panel=none` takes it, the measured choice does not apply.
        let lookup = lookup_of(&[("foot.png", &DynamicImage::ImageRgb8({
            let mut img = RgbImage::from_pixel(400, 200, Rgb([20, 28, 48]));
            for (_, y, p) in img.enumerate_pixels_mut() {
                if y % 4 == 0 {
                    *p = Rgb([230, 235, 240]);
                }
            }
            img
        }))]);
        let attrs = ink_attributes(Some(&lookup), "/foot.png", None, plain(Some("top"), Backing::Never, true));
        assert!(attrs.contains(r#"data-credit="narrow:page mid:page wide:page""#), "{attrs}");
    }

    #[test]
    fn a_one_pixel_window_would_read_the_hairlines_the_stroke_window_averages_away() {
        let em = 40.0 * 2.0;
        let hairline = hatched(2000, 600, (0.05 * em) as u32, (0.12 * em) as u32);
        let area = middle(1000.0, HEADING).0[0];
        let stroke = hairline.read(&area, None, false);
        let pixel = hairline.read_with(&area, None, false, Some(1));
        assert!(stroke.darkest > 0.25 && pixel.darkest < 0.01, "stroke {} pixel {}", stroke.darkest, pixel.darkest);
    }

    #[test]
    fn the_words_start_clamp_8_2vw_24_below_the_top_and_the_heading_sets_at_1_08() {
        let words = Words::read("## Heading", "");
        for (width, top, font) in [(390.0_f32, 8.0, 22.4), (768.0, 15.36, 28.0), (1024.0, 20.48, 40.0), (1360.0, 24.0, 40.0), (2560.0, 24.0, 40.0)] {
            let block = Type::at(width, &TypeTheme::default(), Column::Default, false).blocks(&words).remove(0);
            assert!((block.rows.0 - top).abs() < 0.01, "{width}: the words start {} px down", block.rows.0);
            assert!((block.rows.1 - block.rows.0 - 2.0 * 1.08 * font).abs() < 0.01, "{width}: one line and the line past it at 1.08");
        }
    }

    #[test]
    fn a_cap_on_the_heading_narrows_its_measure_and_a_line_height_moves_its_rows() {
        let words = Words::read("## A heading long enough to wrap in a narrow cap, but not in the box", "");
        let blocks = |css: &str| {
            let theme = TypeTheme::of_theme(Some(css));
            assert!(!theme.resized, "{css}");
            Type::at(1360.0, &theme, Column::Default, false).blocks(&words).remove(0)
        };
        let plain = Type::at(1360.0, &TypeTheme::default(), Column::Default, false).blocks(&words).remove(0);
        let capped = blocks(".moss-hero-closing h2 { max-inline-size: 300px; }");
        assert!(capped.across <= 300.0 && capped.rows.1 > plain.rows.1, "a cap only narrows: {} px across, rows {:?} against {:?}", capped.across, capped.rows, plain.rows);
        assert_eq!(blocks(".moss-hero-closing h2 { max-width: 900px; }").across, plain.across, "a cap wider than the box changes nothing");
        let loose = blocks(".moss-hero-closing h2 { line-height: 1.4; }");
        assert!(loose.rows.1 > plain.rows.1);
        let fixed = blocks(".moss-hero-closing h2 { line-height: 60px; }");
        assert!(fixed.rows.1 - fixed.rows.0 > loose.rows.1 - loose.rows.0);
        for unread in [
            ".moss-hero-closing h2 { max-width: calc(20rem + 2px); }",
            ".moss-hero-closing h2 { max-width: 15em; }",
            ".moss-hero-closing h2 { line-height: normal; }",
            ".moss-hero-closing h2 { line-height: calc(1em + 2px); }",
            ".moss-hero-closing p { max-width: 300px; }",
            ".moss-hero-closing h2, .moss-hero-closing p { max-width: 300px; }",
        ] {
            assert!(TypeTheme::of_theme(Some(unread)).resized, "{unread} is outside what is read");
        }
    }

    #[test]
    fn the_link_row_leaves_the_words_ink_to_the_rest_of_the_words() {
        // A dark picture in the light theme: the words take the light ink, with the link, which has its own ground, left out.
        let lookup = lookup_of(&[("dark.png", &banded(GROUND, GROUND, 1.0)), ("mid.png", &banded([120; 3], [120; 3], 1.0))]);
        let words = |picture: &str, text: &str| {
            let frame = Frame::new(None, None, Some("top"), false, true, Words::read(text, ""));
            ink_attributes(Some(&lookup), picture, None, InkRequest { frame, ..plain(Some("top"), Backing::Auto, true) })
        };
        let attrs = words("/dark.png", "## Heading\n\n[Go](#go)");
        assert!(attrs.contains(&format!(r#"data-words="{}""#, closing_tokens("light"))) && attrs.contains("data-words-night="), "{attrs}");
        // The link alone has nothing to read; over a mid-tone ground text still keeps the panel.
        assert!(!words("/mid.png", "Some running text.\n\n[Go](#go)").contains("data-words"));
    }

    #[test]
    fn a_hero_rule_inside_a_condition_is_not_read() {
        for css in [
            "@media (width < 48rem) { .moss-hero-closing h2 { font-size: 20px; } }",
            "@supports (display: grid) { .moss-hero-closing h2 { max-inline-size: 300px; } }",
            "@media print { .moss-hero-closing h2 { line-height: 1.5; } }",
        ] {
            assert!(TypeTheme::of_theme(Some(css)).resized, "{css}");
        }
        assert!(!TypeTheme::of_theme(Some(".moss-hero-closing h2 { font-size: 20px; }")).resized);
    }

    #[test]
    fn a_cap_in_ch_takes_its_rows_from_the_narrow_end_and_its_width_from_the_wide_end() {
        let words = Words::read("## A heading long enough to wrap in a narrow cap, but not in the box", "");
        let theme = TypeTheme::of_theme(Some(".moss-hero-closing h2 { max-inline-size: 15ch; }"));
        assert!(!theme.resized && theme.heading_cap.is_some());
        let at = |t: &TypeTheme| Type::at(1360.0, t, Column::Default, false).blocks(&words).remove(0);
        let (plain, capped) = (at(&TypeTheme::default()), at(&theme));
        let (narrow, wide) = (15.0 * 0.5 * 40.0, 15.0 * 0.6 * 40.0);
        assert!(capped.across <= wide + 0.01 && capped.across > narrow, "the width is the wide end: {}", capped.across);
        let rows = |measure: f32| ((words.heading * 40.0 / measure).ceil() + 1.0) * 1.08 * 40.0;
        assert!((capped.rows.1 - capped.rows.0 - rows(narrow)).abs() < 0.01 && capped.rows.1 > plain.rows.1, "the rows are the narrow end's: {:?}", capped.rows);
    }

    #[test]
    fn the_credit_takes_the_themes_secondary_colour_as_it_paints_it() {
        // A pale foot: the default secondary colour (dark) reads on it; a theme that sets it pale cannot, and the page colour is pale too.
        let lookup = lookup_of(&[("pale.png", &banded(SKY, SKY, 1.0))]);
        let credit = |css: Option<&str>| ink_attributes(Some(&lookup), "/pale.png", None, InkRequest { ground: PageGround::of_theme(css), ..plain(Some("top"), Backing::Auto, true) });
        assert!(credit(None).contains(r#"data-credit="narrow:text mid:text wide:text""#));
        let pale = credit(Some(":root { --moss-color-text-secondary: #f4f4f0; }"));
        assert!(!pale.contains("data-credit=\""), "{pale}");
        assert!(!credit(Some(":root { --moss-color-text-secondary: var(--ink); }")).contains("data-credit=\""), "an unread colour keeps the panel");
    }

    #[test]
    // Pins the reading as it is, pending a design for small type: a 12px credit's window is one source
    // pixel, so any foot with a highlight fails it.
    fn a_few_pale_specks_set_the_worst_reading_but_not_the_typical_one() {
        // A dark foot with a pale speck every 37 pixels: the credit's page-coloured ink meets a speck as pale
        // as itself at the worst spot, and the dark foot nearly everywhere else.
        let mut img = RgbImage::from_pixel(400, 100, Rgb(GROUND));
        for y in (0..100).step_by(9) {
            for x in (y % 37..400).step_by(37) {
                img.put_pixel(x, y, Rgb(SKY));
            }
        }
        let lookup = lookup_of(&[("specks.png", &DynamicImage::ImageRgb8(img))]);
        let frame = Frame { words: Words::read("## Words", "A credit line."), ..closing(Some("top")) };
        let credit: Vec<_> = forced_ink_shortfalls(&lookup, ("/specks.png", None), Some("top"), &frame, true).into_iter().filter(|s| s.part == Part::Credit && !s.night).collect();
        assert_eq!(credit.len(), 3, "every layout falls short at its worst spot");
        for s in &credit {
            assert!(s.limit.raw < 1.5 && s.tones.typical(s.ink) >= MIN_CONTRAST, "{:?}: a speck as pale as the ink, a foot it reads on", s.limit);
        }
    }
}
