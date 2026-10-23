//! What a closing picture's words and credit say and how much room they take,
//! as the default type scale sets them: where their glyphs land in the box.
//!
//! The sizes come from tokens.json where a token holds them and from site.css's
//! hero rules where one does not (`site_css_sets_the_type_the_words_are_modelled_in`
//! and the `closing-picture` render gate hold the model to the stylesheet).

/// A text's width in ems: a letter is half an em and a CJK character a whole
/// one. The theme's face is not seen, so a narrower one sets fewer lines than
/// this counts.
fn em(text: &str) -> f32 {
    text.chars().map(|c| if c as u32 >= 0x2E80 { 1.0 } else { 0.5 }).sum()
}

/// What a closing picture's words say, as widths in ems: the first heading,
/// the running text, the links (one button each, as the links' text) and the
/// credit.
#[derive(Clone, Copy, Debug, Default)]
pub struct Words {
    pub(super) heading: f32,
    pub(super) text: f32,
    pub(super) links: f32,
    pub(super) link_count: u32,
    pub(super) credit: f32,
}

impl Words {
    /// Reads the hero's overlay source: a `#` block is the heading, a block
    /// of nothing but `[text](url)` links is the links, any other is text.
    pub fn read(overlay: &str, credit: &str) -> Self {
        let mut words = Words { credit: em(credit), ..Self::default() };
        for block in overlay.split("\n\n").map(str::trim).filter(|b| !b.is_empty()) {
            let (mut texts, mut rest, mut left) = (Vec::new(), String::new(), block);
            while let Some((before, after)) = left.split_once('[') {
                let Some((text, tail)) = after.split_once("](") else { break };
                let Some((_, tail)) = tail.split_once(')') else { break };
                texts.push(text);
                rest.push_str(before);
                left = tail;
            }
            rest.push_str(left);
            if block.starts_with('#') {
                words.heading = if words.heading > 0.0 { words.heading } else { em(block.trim_start_matches('#').trim()) };
            } else if !texts.is_empty() && rest.trim().is_empty() {
                words.links += texts.iter().map(|t| em(t)).sum::<f32>();
                words.link_count += texts.len() as u32;
            } else {
                words.text += em(block);
            }
        }
        words
    }
}

/// A theme's cap on the heading's width: a length in the grammar `max-height=`
/// takes, or a number of `ch`, which depends on the face and is taken as 0.5 to
/// 0.6 em each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Cap {
    Length(moss_core::media::MaxHeight),
    Ch(f32),
}

impl Cap {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim().trim_end_matches("!important").trim();
        match value.strip_suffix("ch").map(|n| n.trim().parse::<f32>()) {
            Some(Ok(n)) if n.is_finite() && n > 0.0 => Some(Cap::Ch(n)),
            Some(_) => None,
            None => moss_core::media::MaxHeight::parse(value).map(Cap::Length),
        }
    }

    /// The narrowest and the widest the cap can be at a window `width` wide with the heading at `font` px.
    fn px(self, font: f32, width: f32) -> (f32, f32) {
        match self {
            Cap::Length(m) => (m.px(width), m.px(width)),
            Cap::Ch(n) => (n * CH_NARROW * font, n * CH_WIDE * font),
        }
    }
}

/// What a `ch` is in ems at the least and at the most, whatever the face.
const CH_NARROW: f32 = 0.5;
const CH_WIDE: f32 = 0.6;

/// A theme's `line-height` on the heading: a unitless factor of its size, or a
/// length in the grammar `max-height=` takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Leading {
    Factor(f32),
    Length(moss_core::media::MaxHeight),
}

impl Leading {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim().trim_end_matches("!important").trim();
        match value.parse::<f32>() {
            Ok(n) if n.is_finite() && n > 0.0 => Some(Leading::Factor(n)),
            Ok(_) => None,
            Err(_) => moss_core::media::MaxHeight::parse(value).map(Leading::Length),
        }
    }

    fn px(self, font: f32, width: f32) -> f32 {
        match self {
            Leading::Factor(n) => n * font,
            Leading::Length(m) => m.px(width),
        }
    }
}

/// What a theme's stylesheet changes in the type or the column the model
/// assumes. A value the model can read is kept: a plain length for the reading
/// size or the container padding in `:root`, a size for the words' heading or
/// text in the grammar `max-height=` takes (a length, or `clamp()` of px and
/// vw), and for the heading also that grammar as a cap on its measure
/// (`max-inline-size`, `max-width`) or a `line-height` (a number or that
/// grammar). Anything else that moves the words, or a weight under 400, sets
/// `resized`, and the words keep their panel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TypeTheme {
    pub(super) reading_base: Option<f32>,
    pub(super) container_pad: Option<f32>,
    pub(super) heading_size: Option<moss_core::media::MaxHeight>,
    pub(super) text_size: Option<moss_core::media::MaxHeight>,
    pub(super) heading_weight: Option<u16>,
    /// A `max-inline-size` or `max-width` on the heading: it only narrows the
    /// lines, so it caps the measure the heading is modelled in.
    pub(super) heading_cap: Option<Cap>,
    pub(super) heading_line: Option<Leading>,
    /// `--moss-hero-link-size` and `--moss-hero-link-padding` (block, inline, in em).
    pub(super) link_size: Option<moss_core::media::MaxHeight>,
    pub(super) link_pad: Option<(f32, f32)>,
    pub(super) space_md: Option<f32>,
    pub(super) size_sm: Option<f32>,
    pub(super) size_lg: Option<f32>,
    pub resized: bool,
}

/// Tokens that move the words' size or column and that the model does not read
/// (the others a theme may set, `--moss-space-md`, `--moss-size-sm` and
/// `--moss-size-lg`, are read when plain lengths).
const UNREAD_TOKENS: [&str; 5] = ["--moss-reading-size", "--moss-reading-script-scale", "--moss-reading-step", "--moss-content-width", "--moss-site-max-width"];

/// `1.5rem` or `24px` as px at the 16px root.
fn plain_px(value: &str) -> Option<f32> {
    let value = value.trim().trim_end_matches("!important").trim();
    match value.strip_suffix("rem") {
        Some(n) => n.trim().parse::<f32>().ok().map(|n| n * 16.0),
        None => value.strip_suffix("px")?.trim().parse().ok(),
    }
}

/// `0.5em` or `0.5em 1.2em` as (block, inline) in em.
fn em_pair(value: &str) -> Option<(f32, f32)> {
    let em = |v: &str| v.strip_suffix("em")?.trim().parse::<f32>().ok();
    let value = value.trim().trim_end_matches("!important");
    let mut parts = value.split_whitespace();
    let (block, inline) = (em(parts.next()?)?, parts.next().map_or(Some(None), |v| em(v).map(Some))?);
    parts.next().is_none().then_some((block, inline.unwrap_or(block)))
}

/// The weight a `font-weight` value names, `None` for one the build cannot read.
fn weight_of(value: &str) -> Option<u16> {
    match value.trim().trim_end_matches("!important").trim() {
        "normal" => Some(400),
        "bold" => Some(700),
        "lighter" | "light" | "thin" => Some(300),
        n => n.parse().ok(),
    }
}

impl TypeTheme {
    pub fn of_theme(theme_css: Option<&str>) -> Self {
        Self::read(theme_css, |_, _| {})
    }

    /// Say, once, which declaration of the theme moves a closing picture's words
    /// beyond what the measurement reads. Called only for a site that has a
    /// closing picture, since the note means nothing otherwise.
    pub fn note_unread(theme_css: Option<&str>) {
        Self::read(theme_css, |prop, prelude| {
            crate::build::cli_output::log_note!("theme: `{prop}` on `{prelude}` is outside what the ink measurement reads, so a closing picture's words keep their panel (sizes read as `max-height=` does; weight 400 or more).");
        });
    }

    fn read(theme_css: Option<&str>, mut on_first_unread: impl FnMut(&str, &str)) -> Self {
        let mut theme = Self::default();
        for rule in crate::build::theme_lint::style_rules(theme_css.unwrap_or_default()) {
            let Some((_, body)) = rule.rule.split_once('{') else { continue };
            let root = rule.conditions.is_empty() && rule.prelude.split_whitespace().collect::<String>() == ":root";
            let words = |tags: &[&str]| rule.prelude.split(|c: char| !c.is_ascii_alphanumeric()).any(|t| tags.contains(&t));
            // A rule reaches the closing words by naming the hero, or by the footer's own
            // structure: a heading, paragraph or link under the page's footer.
            let reaches = rule.prelude.contains("moss-hero") || (rule.prelude.contains("footer") && words(&["h1", "h2", "h3", "h4", "h5", "h6", "p", "a"]));
            // Inside a media query or another condition, the value holds only at some widths: it is not read.
            let hero = reaches && rule.conditions.is_empty();
            let heading_only = hero && words(&["h1", "h2", "h3", "h4", "h5", "h6"]) && !words(&["p", "a"]);
            for decl in body.trim_end_matches('}').split(';') {
                let Some((prop, value)) = decl.split_once(':') else { continue };
                let prop = prop.trim();
                let read = match prop {
                    "--moss-reading-size-base" if root => plain_px(value).map(|px| theme.reading_base = Some(px)),
                    "--moss-container-padding" if root => plain_px(value).map(|px| theme.container_pad = Some(px)),
                    "--moss-space-md" if root => plain_px(value).map(|px| theme.space_md = Some(px)),
                    "--moss-size-sm" if root => plain_px(value).map(|px| theme.size_sm = Some(px)),
                    "--moss-size-lg" if root => plain_px(value).map(|px| theme.size_lg = Some(px)),
                    "--moss-hero-title-size" if root => moss_core::media::MaxHeight::parse(value.trim().trim_end_matches("!important")).map(|m| theme.heading_size = Some(m)),
                    "--moss-hero-link-size" if root => moss_core::media::MaxHeight::parse(value.trim().trim_end_matches("!important")).map(|m| theme.link_size = Some(m)),
                    "--moss-hero-link-padding" if root => em_pair(value).map(|p| theme.link_pad = Some(p)),
                    "font-size" if hero && words(&["h1", "h2", "h3", "h4", "h5", "h6"]) => {
                        moss_core::media::MaxHeight::parse(value.trim().trim_end_matches("!important")).map(|m| theme.heading_size = Some(m))
                    }
                    "font-size" if hero && words(&["p", "a"]) => moss_core::media::MaxHeight::parse(value.trim().trim_end_matches("!important")).map(|m| theme.text_size = Some(m)),
                    "font-weight" if hero && words(&["h1", "h2", "h3", "h4", "h5", "h6"]) => weight_of(value).filter(|w| *w >= 400).map(|w| theme.heading_weight = Some(w)),
                    "max-inline-size" | "max-width" if heading_only => Cap::parse(value).map(|c| theme.heading_cap = Some(c)),
                    "line-height" if heading_only => Leading::parse(value).map(|l| theme.heading_line = Some(l)),
                    _ => None,
                };
                let moves = read.is_none()
                    && (matches!(prop, "--moss-reading-size-base" | "--moss-container-padding" | "--moss-space-md" | "--moss-size-sm" | "--moss-size-lg" | "--moss-hero-title-size" | "--moss-hero-link-size" | "--moss-hero-link-padding")
                        || UNREAD_TOKENS.contains(&prop)
                        || (reaches && matches!(prop, "font-size" | "font" | "font-weight" | "font-family" | "line-height" | "letter-spacing" | "text-transform" | "padding" | "max-width" | "max-inline-size" | "--moss-hero-content-width")));
                if moves && !theme.resized {
                    on_first_unread(prop, rule.prelude.trim());
                }
                theme.resized |= moves;
            }
        }
        theme
    }
}

/// A token's value in px from tokens.json, at the 16px root.
fn token_px(name: &str) -> f32 {
    static TOKENS: std::sync::LazyLock<moss_core::contract::tokens::Tokens> =
        std::sync::LazyLock::new(|| moss_core::contract::tokens::load_tokens().unwrap_or_else(|e| panic!("tokens.json must parse: {e}")));
    moss_core::contract::tokens::find_token(&TOKENS, name).and_then(|t| plain_px(t.0)).unwrap_or_else(|| panic!("--{name} is a plain length in tokens.json"))
}

/// The text column a page sets (`content_width:`): the default, `wide`, or `full`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Default,
    Wide,
    Full,
}

/// The lane `contents: pictures` keeps free on each side from 64rem
/// (`--moss-contents-inset` + 2 * `--moss-contents-pad` + `--moss-contents-row`
/// + `--moss-contents-gutter`), which narrows the column of any page that has
/// it. The footer does not know its page's frontmatter, so both are measured.
pub(super) const LANE_PX: f32 = 92.0;

/// What a closing picture's words are set in at a window `width` wide in a
/// `column`, from the stylesheet's hero rules: where the words start
/// (`--moss-words-inset`), the box's padding, the heading's size and line
/// height, the running text's, and the width they set in.
pub(super) struct Type {
    /// Where the glyphs start, in px from the picture's edge.
    pub(super) inset: f32,
    pub(super) pad: f32,
    /// Where the first line starts, in px from the picture's top: the box's block-start padding.
    top: f32,
    font: f32,
    line: f32,
    /// The heading's weight; moss's default is 480.
    weight: u16,
    text_font: f32,
    text_line: f32,
    /// The links' size and their padding (block, inline) in em.
    link_font: f32,
    link_pad: (f32, f32),
    /// The width the lines set in.
    measure: f32,
    /// The width the heading sets in, at the narrowest and the widest: the measure, or a theme's cap if narrower.
    heading_measure: (f32, f32),
    /// The words box's own width, padding included.
    pub(super) box_px: f32,
}

/// The default heading weight (`--moss-hero` headings).
const HEADING_WEIGHT: u16 = 480;

/// A closing picture's lead heading's line height, as a factor of its size
/// (`line-height: 1.08` in site.css).
const HEADING_LEADING: f32 = 1.08;

/// Where a closing picture's words start below its top edge: `clamp(8px, 2vw, 24px)`.
fn words_top(width: f32) -> f32 {
    (0.02 * width).clamp(8.0, 24.0)
}

/// A block of the words: its rows in px from the picture's top, how wide its
/// longest line can be from the text's start, and the type it is set in.
pub(super) struct Block {
    pub(super) rows: (f32, f32),
    pub(super) across: f32,
    pub(super) face: super::Face,
    /// The row of links: a button on a ground of its own, so it is not part of what the words' ink is read over.
    pub(super) link: bool,
}

/// The reading size on a phone: site.css's `--moss-reading-size-base` under 48rem.
const PHONE_READING_PX: f32 = 17.0;

/// A link button's padding in em, block then inline (`padding: 0.5em 1.2em`).
const LINK_PAD_EM: (f32, f32) = (0.5, 1.2);

/// The gap under the heading (`--moss-space-sm`), and from a block to the links' row.
const HEADING_GAP_PX: f32 = 8.0;
const LINKS_GAP_PX: f32 = 24.0;

impl Type {
    pub(super) fn at(width: f32, theme: &TypeTheme, column: Column, lane: bool) -> Self {
        let mut t = Self::scale(width, theme, column, lane);
        if let Some(m) = theme.heading_size {
            t.font = m.px(width);
            t.line = HEADING_LEADING * t.font;
        }
        if let Some(l) = theme.heading_line {
            t.line = l.px(t.font, width);
        }
        if let Some(cap) = theme.heading_cap {
            let (narrow, wide) = cap.px(t.font, width);
            t.heading_measure = (t.measure.min(narrow), t.measure.min(wide));
        }
        if let Some(m) = theme.text_size {
            t.text_font = m.px(width);
            t.text_line = 1.5 * t.text_font;
        }
        t.link_font = theme.link_size.map_or(t.text_font, |m| m.px(width));
        t.link_pad = theme.link_pad.unwrap_or(LINK_PAD_EM);
        t.weight = theme.heading_weight.unwrap_or(HEADING_WEIGHT);
        t
    }

    /// The stylesheet's own scale, before a theme's sizes.
    fn scale(width: f32, theme: &TypeTheme, column: Column, lane: bool) -> Self {
        let phone = width < 768.0;
        let reading = theme.reading_base.unwrap_or_else(|| if phone { PHONE_READING_PX } else { token_px("moss-reading-size-base") });
        let space_md = theme.space_md.unwrap_or_else(|| token_px("moss-space-md"));
        let cp = theme.container_pad.unwrap_or(if phone { space_md } else { (0.05 * width).clamp(16.0, 32.0) });
        let mut column_px = match column {
            Column::Default => 42.0 * reading,
            Column::Wide => 50.0 * reading,
            Column::Full => token_px("moss-site-max-width"),
        };
        if lane && width >= 1024.0 {
            column_px = column_px.min(width - 2.0 * LANE_PX);
        }
        let inset = ((width - column_px) / 2.0).max(0.0) + cp;
        if phone {
            let text = theme.size_sm.unwrap_or_else(|| token_px("moss-size-sm"));
            let measure = width - 2.0 * inset;
            return Type { inset, pad: 0.05 * width, top: words_top(width), font: 22.4, line: HEADING_LEADING * 22.4, weight: HEADING_WEIGHT, text_font: text, text_line: 1.5 * text, link_font: text, link_pad: LINK_PAD_EM, measure, heading_measure: (measure, measure), box_px: width };
        }
        let (pad, font, text) = if width < 1024.0 { (space_md, 28.0, reading) } else { (0.05 * width, 40.0, theme.size_lg.unwrap_or_else(|| token_px("moss-size-lg"))) };
        let box_px = (0.35 * width).max(600.0).min(column_px - 2.0 * cp + 2.0 * pad);
        let measure = box_px - 2.0 * pad;
        Type { inset, pad, top: words_top(width), font, line: HEADING_LEADING * font, weight: HEADING_WEIGHT, text_font: text, text_line: 1.5 * text, link_font: text, link_pad: LINK_PAD_EM, measure, heading_measure: (measure, measure), box_px }
    }

    /// The most and the fewest lines `ems` of text at `font` px set in the
    /// measure (its narrowest and widest, for the most and the fewest): a face is narrower than half an em a letter (`NARROWEST`).
    fn lines(&self, ems: f32, font: f32, (narrow, wide): (f32, f32)) -> (f32, f32) {
        if ems <= 0.0 {
            return (0.0, 0.0);
        }
        let lines = |factor: f32, measure: f32| (ems * font * factor / measure).ceil().max(1.0);
        (lines(1.0, narrow), lines(NARROWEST, wide))
    }

    /// The heading's type: large text (3:1) from 24px, or from 18.66px at weight 700.
    fn heading_face(&self) -> super::Face {
        let large = self.font >= 24.0 || (self.font >= 18.66 && self.weight >= 700);
        super::Face { font: self.font, need: if large { 3.0 } else { 4.5 } }
    }

    /// The type of the text and the links.
    fn text_face(&self) -> super::Face {
        super::Face { font: self.text_font, need: 4.5 }
    }

    /// The type of the smallest part of the words, for where they are read over their box.
    pub(super) fn smallest_face(&self) -> super::Face {
        super::Face { font: self.text_font.min(self.font), need: 4.5 }
    }

    /// The blocks the glyphs of `words` sit in below the padding: the heading,
    /// the text, the links' row. Each starts where the blocks above it end if
    /// they set the fewest lines and ends a line past where they end if they
    /// set the most. A line that is alone is as wide as its text; a block of
    /// several can end a word short of the measure at the least, so its longest
    /// line is the measure. A button is its text and its padding.
    pub(super) fn blocks(&self, words: &Words) -> Vec<Block> {
        let (mut least, mut most) = (self.top, self.top);
        let mut blocks = Vec::new();
        let mut push = |lines: (f32, f32), line: f32, width: f32, measure: f32, gap: f32, face: super::Face| {
            if lines.0 > 0.0 {
                blocks.push(Block { rows: (least, most + (lines.0 + 1.0) * line), across: if lines.0 > 1.0 { measure } else { width.min(measure) }, face, link: false });
                least += lines.1 * line + gap;
                most += lines.0 * line + gap;
            }
        };
        let text_gap = if words.link_count > 0 { LINKS_GAP_PX } else { 0.0 };
        push(self.lines(words.heading, self.font, self.heading_measure), self.line, words.heading * self.font, self.heading_measure.1, if words.text > 0.0 { HEADING_GAP_PX } else { text_gap }, self.heading_face());
        push(self.lines(words.text, self.text_font, (self.measure, self.measure)), self.text_line, words.text * self.text_font, self.measure, text_gap, self.text_face());
        if words.link_count > 0 {
            // A line and a hair of border, and the padding twice over, in the link's own em.
            let row = (1.6 + 2.0 * self.link_pad.0) * self.link_font;
            let width = (LINK_WEIGHT_WIDTH * words.links + (2.0 * self.link_pad.1 + 0.2) * words.link_count as f32) * self.link_font + 16.0 * (words.link_count - 1) as f32;
            let lines = (width / self.measure).ceil().max(1.0);
            blocks.push(Block { rows: (least, most + lines * row + self.text_line), across: width.min(self.measure), face: self.text_face(), link: true });
        }
        blocks
    }

    pub(super) fn credit(&self, width: f32, credit_em: f32) -> CreditBox {
        let measure = (width - 2.0 * self.inset).max(CREDIT_FONT_PX);
        let text = credit_em * CREDIT_FONT_PX * CREDIT_LETTER_WIDEST;
        CreditBox { lines: (text / measure).ceil().max(1.0), across: text.min(measure), end: self.inset }
    }
}

/// How much wider a link's text sets at weight 500 than at 400, as a factor.
const LINK_WEIGHT_WIDTH: f32 = 1.04;

/// How much of half an em a letter may be in a narrow face.
const NARROWEST: f32 = 0.7;

/// The credit's type: 12px on a 1.45 line, set where the page's text column
/// ends (`--moss-credit-end` in site.css) with 14px below its last line when it
/// is in ink, and as wide as its text up to the column's width. A theme's face
/// is not seen, so a letter is taken at 0.6 em, a monospace face's width.
pub(super) const CREDIT_FONT_PX: f32 = 12.0;
const CREDIT_LINE_PX: f32 = 17.4;
const CREDIT_PAD_BELOW_PX: f32 = 14.0;
const CREDIT_LETTER_WIDEST: f32 = 1.2;

/// Where a credit's text sits on a closing picture at a window `width` wide in
/// a `column`, from the picture's inline end: how many lines it sets, how wide
/// they reach, and where its text ends.
pub(super) struct CreditBox {
    pub(super) lines: f32,
    pub(super) across: f32,
    /// From the window's inline end to the text's end: the column's own end.
    pub(super) end: f32,
}

impl CreditBox {
    /// The rows its text covers, as px above the box's bottom edge, the colophon's
    /// `strip` clear: nearest and farthest.
    pub(super) fn rows(&self, strip: f32) -> (f32, f32) {
        (strip + CREDIT_PAD_BELOW_PX, strip + CREDIT_PAD_BELOW_PX + self.lines * CREDIT_LINE_PX)
    }
}

/// How tall the credit is at a window `width` wide in the default column, in px.
pub(super) fn credit_height(credit_em: f32, width: f32) -> f32 {
    let credit = Type::at(width, &TypeTheme::default(), Column::Default, false).credit(width, credit_em);
    credit.rows(0.0).1
}
