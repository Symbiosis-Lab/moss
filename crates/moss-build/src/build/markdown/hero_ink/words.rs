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

/// What a theme's stylesheet changes in the type or the column the model
/// assumes. A value the model can read is kept: a plain length for the reading
/// size or the container padding in `:root`, and a size for the words' heading
/// or text in the grammar `max-height=` takes (a length, or `clamp()` of px and
/// vw). Anything else that moves the words, or a weight under 400, sets
/// `resized`, and the words keep their panel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TypeTheme {
    pub(super) reading_base: Option<f32>,
    pub(super) container_pad: Option<f32>,
    pub(super) heading_size: Option<moss_core::media::MaxHeight>,
    pub(super) text_size: Option<moss_core::media::MaxHeight>,
    pub(super) heading_weight: Option<u16>,
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
            let hero = rule.prelude.contains("moss-hero") || (rule.prelude.contains("footer") && words(&["h1", "h2", "h3", "h4", "h5", "h6", "p", "a"]));
            for decl in body.trim_end_matches('}').split(';') {
                let Some((prop, value)) = decl.split_once(':') else { continue };
                let prop = prop.trim();
                let read = match prop {
                    "--moss-reading-size-base" if root => plain_px(value).map(|px| theme.reading_base = Some(px)),
                    "--moss-container-padding" if root => plain_px(value).map(|px| theme.container_pad = Some(px)),
                    "--moss-space-md" if root => plain_px(value).map(|px| theme.space_md = Some(px)),
                    "--moss-size-sm" if root => plain_px(value).map(|px| theme.size_sm = Some(px)),
                    "--moss-size-lg" if root => plain_px(value).map(|px| theme.size_lg = Some(px)),
                    "font-size" if hero && words(&["h1", "h2", "h3", "h4", "h5", "h6"]) => {
                        moss_core::media::MaxHeight::parse(value.trim().trim_end_matches("!important")).map(|m| theme.heading_size = Some(m))
                    }
                    "font-size" if hero && words(&["p", "a"]) => moss_core::media::MaxHeight::parse(value.trim().trim_end_matches("!important")).map(|m| theme.text_size = Some(m)),
                    "font-weight" if hero && words(&["h1", "h2", "h3", "h4", "h5", "h6"]) => weight_of(value).filter(|w| *w >= 400).map(|w| theme.heading_weight = Some(w)),
                    _ => None,
                };
                let moves = read.is_none()
                    && (matches!(prop, "--moss-reading-size-base" | "--moss-container-padding" | "--moss-space-md" | "--moss-size-sm" | "--moss-size-lg")
                        || UNREAD_TOKENS.contains(&prop)
                        || (hero && matches!(prop, "font-size" | "font" | "font-weight" | "font-family" | "line-height" | "letter-spacing" | "text-transform" | "padding" | "max-width" | "max-inline-size" | "--moss-hero-content-width")));
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
    font: f32,
    line: f32,
    /// The heading's weight; moss's default is 480.
    weight: u16,
    text_font: f32,
    text_line: f32,
    /// The width the lines set in.
    measure: f32,
    /// The words box's own width, padding included.
    pub(super) box_px: f32,
}

/// The default heading weight (`--moss-hero` headings).
const HEADING_WEIGHT: u16 = 480;

/// A block of the words: its rows in px from the picture's top, how wide its
/// longest line can be from the text's start, and the type it is set in.
pub(super) struct Block {
    pub(super) rows: (f32, f32),
    pub(super) across: f32,
    pub(super) face: super::Face,
}

/// The reading size on a phone: site.css's `--moss-reading-size-base` under 48rem.
const PHONE_READING_PX: f32 = 17.0;

/// The gap under the heading (`--moss-space-sm`), and from a block to the links' row.
const HEADING_GAP_PX: f32 = 8.0;
const LINKS_GAP_PX: f32 = 24.0;

impl Type {
    pub(super) fn at(width: f32, theme: &TypeTheme, column: Column, lane: bool) -> Self {
        let mut t = Self::scale(width, theme, column, lane);
        if let Some(m) = theme.heading_size {
            t.font = m.px(width);
            t.line = 1.2 * t.font;
        }
        if let Some(m) = theme.text_size {
            t.text_font = m.px(width);
            t.text_line = 1.5 * t.text_font;
        }
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
            return Type { inset, pad: 0.05 * width, font: 22.4, line: 26.88, weight: HEADING_WEIGHT, text_font: text, text_line: 1.5 * text, measure: width - 2.0 * inset, box_px: width };
        }
        let (pad, font, line, text) = if width < 1024.0 { (space_md, 28.0, 33.6, reading) } else { (0.05 * width, 40.0, 48.0, theme.size_lg.unwrap_or_else(|| token_px("moss-size-lg"))) };
        let box_px = (0.35 * width).max(600.0).min(column_px - 2.0 * cp + 2.0 * pad);
        Type { inset, pad, font, line, weight: HEADING_WEIGHT, text_font: text, text_line: 1.5 * text, measure: box_px - 2.0 * pad, box_px }
    }

    /// The most and the fewest lines `ems` of text at `font` px set in the
    /// measure: a face is narrower than half an em a letter (`NARROWEST`).
    fn lines(&self, ems: f32, font: f32) -> (f32, f32) {
        if ems <= 0.0 {
            return (0.0, 0.0);
        }
        let lines = |factor: f32| (ems * font * factor / self.measure).ceil().max(1.0);
        (lines(1.0), lines(NARROWEST))
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
        let (mut least, mut most) = (self.pad, self.pad);
        let mut blocks = Vec::new();
        let mut push = |lines: (f32, f32), line: f32, width: f32, gap: f32, face: super::Face| {
            if lines.0 > 0.0 {
                blocks.push(Block { rows: (least, most + (lines.0 + 1.0) * line), across: if lines.0 > 1.0 { self.measure } else { width.min(self.measure) }, face });
                least += lines.1 * line + gap;
                most += lines.0 * line + gap;
            }
        };
        let text_gap = if words.link_count > 0 { LINKS_GAP_PX } else { 0.0 };
        push(self.lines(words.heading, self.font), self.line, words.heading * self.font, if words.text > 0.0 { HEADING_GAP_PX } else { text_gap }, self.heading_face());
        push(self.lines(words.text, self.text_font), self.text_line, words.text * self.text_font, text_gap, self.text_face());
        if words.link_count > 0 {
            let row = self.text_font + 2.0 * 0.8 * self.text_font;
            let width = (words.links + 2.6 * words.link_count as f32) * self.text_font + 16.0 * (words.link_count - 1) as f32;
            let lines = (width / self.measure).ceil().max(1.0);
            blocks.push(Block { rows: (least, most + lines * row + self.text_line), across: width.min(self.measure), face: self.text_face() });
        }
        blocks
    }
}

/// How much of half an em a letter may be in a narrow face.
const NARROWEST: f32 = 0.7;

/// The credit's type: 12px on a 1.45 line, in a box with 0.3rem above and
/// below and 5% of the window beside, at most 36rem wide.
pub(super) const CREDIT_FONT_PX: f32 = 12.0;
const CREDIT_LINE_PX: f32 = 17.4;
const CREDIT_PAD_PX: f32 = 9.6;
pub(super) const CREDIT_WIDTH_PX: f32 = 576.0;

/// How tall the credit is at a window `width` wide, in px.
pub(super) fn credit_height(credit_em: f32, width: f32) -> f32 {
    let measure = CREDIT_WIDTH_PX.min(width) - 0.1 * width;
    CREDIT_PAD_PX + (credit_em * CREDIT_FONT_PX / measure).ceil().max(1.0) * CREDIT_LINE_PX
}
