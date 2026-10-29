//! Unified media reference resolution and display attributes.
//!
//! All media reference contexts in moss (frontmatter cover, hero, gallery,
//! inline images, wikilink embeds) call into this module to parse pipe-
//! separated display attributes (`object-fit`, `object-position`).
//!
//! Pure Rust, zero I/O.

use std::collections::BTreeMap;

mod max_height;
mod placement;
pub use max_height::{Length, MaxHeight};
pub use placement::{extract_placement_from_alias, Placement};
// `parse_placement_segment` has exactly one caller outside its own module:
// `segment_without_size` below, the editor's drag-resize write path — hence
// `pub(crate)`, not the wider `pub` the other two placement helpers used to
// share despite never being called from outside `placement.rs` at all.
use placement::parse_placement_segment;

// ---------------------------------------------------------------------------
// Fit — maps to CSS `object-fit`
// ---------------------------------------------------------------------------

/// CSS `object-fit` values for media display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Cover,
    Contain,
    Fill,
    None,
    ScaleDown,
}

impl Fit {
    /// Return the CSS `object-fit` value.
    pub fn to_css_value(&self) -> &str {
        match self {
            Fit::Cover => "cover",
            Fit::Contain => "contain",
            Fit::Fill => "fill",
            Fit::None => "none",
            Fit::ScaleDown => "scale-down",
        }
    }

    /// Parse from a keyword string (case-insensitive).
    ///
    /// Accepts both CSS syntax (`"scale-down"`) and space-free forms (`"scaledown"`).
    /// Every spelling [`Fit::from_keyword`] reads, with its fit.
    pub const KEYWORDS: &[(&str, Fit)] = &[
        ("cover", Fit::Cover),
        ("contain", Fit::Contain),
        ("fill", Fit::Fill),
        ("none", Fit::None),
        ("scale-down", Fit::ScaleDown),
        ("scaledown", Fit::ScaleDown),
    ];

    /// Parse from a keyword string (case-insensitive).
    pub fn from_keyword(s: &str) -> Option<Self> {
        let lower = s.to_lowercase();
        Self::KEYWORDS.iter().find(|(k, _)| *k == lower).map(|(_, f)| *f)
    }

    /// The fit an inline style written by [`MediaAttrs::to_inline_style`] sets.
    pub(crate) fn of_style(style: &str) -> Option<Self> {
        style.split(';').find_map(|d| d.trim().strip_prefix("object-fit:")).and_then(Self::from_keyword)
    }
}

// ---------------------------------------------------------------------------
// Position — maps to CSS `object-position`
// ---------------------------------------------------------------------------

/// CSS `object-position` values for media display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Center,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// A free focus point, each axis a percentage 0–100.
    Focus(u8, u8),
}

impl Position {
    /// Return the CSS `object-position` value.
    pub fn to_css_value(&self) -> std::borrow::Cow<'static, str> {
        match self {
            Position::Center => "center".into(),
            Position::Left => "left".into(),
            Position::Right => "right".into(),
            Position::Top => "top".into(),
            Position::Bottom => "bottom".into(),
            Position::TopLeft => "top left".into(),
            Position::TopRight => "top right".into(),
            Position::BottomLeft => "bottom left".into(),
            Position::BottomRight => "bottom right".into(),
            Position::Focus(x, y) => format!("{x}% {y}%").into(),
        }
    }

    /// The position as `(x, y)` percentages, as `object-position` reads it.
    pub fn percentages(&self) -> (u8, u8) {
        match self {
            Position::Center => (50, 50),
            Position::Left => (0, 50),
            Position::Right => (100, 50),
            Position::Top => (50, 0),
            Position::Bottom => (50, 100),
            Position::TopLeft => (0, 0),
            Position::TopRight => (100, 0),
            Position::BottomLeft => (0, 100),
            Position::BottomRight => (100, 100),
            Position::Focus(x, y) => (*x, *y),
        }
    }

    /// A free focus point, `focus:X%,Y%` with integer percentages 0–100.
    /// Anything else, including a bare `X%`, is `None`.
    fn from_focus(lower: &str) -> Option<Self> {
        let (x, y) = lower.strip_prefix("focus:")?.split_once(',')?;
        let x = x.strip_suffix('%')?.parse::<u8>().ok().filter(|v| *v <= 100)?;
        let y = y.strip_suffix('%')?.parse::<u8>().ok().filter(|v| *v <= 100)?;
        Some(Position::Focus(x, y))
    }

    /// Parse from a keyword string (case-insensitive).
    ///
    /// Accepts hyphenated (`"top-left"`), concatenated (`"topleft"`), and
    /// space-separated (`"top left"`) forms, and a `focus:X%,Y%` point.
    /// Every named spelling [`Position::from_keyword`] reads, with its position.
    pub const KEYWORDS: &[(&str, Position)] = &[
        ("center", Position::Center),
        ("left", Position::Left),
        ("right", Position::Right),
        ("top", Position::Top),
        ("bottom", Position::Bottom),
        ("top-left", Position::TopLeft),
        ("topleft", Position::TopLeft),
        ("top left", Position::TopLeft),
        ("top-right", Position::TopRight),
        ("topright", Position::TopRight),
        ("top right", Position::TopRight),
        ("bottom-left", Position::BottomLeft),
        ("bottomleft", Position::BottomLeft),
        ("bottom left", Position::BottomLeft),
        ("bottom-right", Position::BottomRight),
        ("bottomright", Position::BottomRight),
        ("bottom right", Position::BottomRight),
    ];

    /// Parse from a keyword string (case-insensitive).
    ///
    /// Accepts hyphenated (`"top-left"`), concatenated (`"topleft"`), and
    /// space-separated (`"top left"`) forms, and a `focus:X%,Y%` point.
    pub fn from_keyword(s: &str) -> Option<Self> {
        let lower = s.to_lowercase();
        match Self::KEYWORDS.iter().find(|(k, _)| *k == lower) {
            Some((_, p)) => Some(*p),
            None => Self::from_focus(&lower),
        }
    }
}

// ---------------------------------------------------------------------------
// AlignSide — editorial runaround alignment (text wraps around half-width image)
// ---------------------------------------------------------------------------

/// Image alignment for editorial runaround layout. Mirrors WordPress's
/// `alignleft` / `alignright` block-editor convention; the moss CSS class
/// is `moss-align-left` / `moss-align-right`. Float behavior plus mobile
/// collapse (≤48rem) live in moss-build's `site.css`.
///
/// Hyphenated `align-left` is the canonical pipe-keyword form; unhyphenated
/// `alignleft` (matching the WP class name) is a forgiveness alias.
/// Bare `left` / `right` are also accepted, because Stage 1 emits them as
/// the value of an explicit `align=` key in TitleParams (e.g. `align=left`),
/// where ambiguity with [`Position`]'s `left` / `right` does not arise.
///
/// Note: in [`parse_media_attrs`]'s space-separated keyword parser, bare
/// `left` / `right` still match [`Position::from_keyword`] FIRST and never
/// reach this function, so the disambiguation rule for the pipe-keyword
/// layer is preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignSide {
    Left,
    Right,
}

impl AlignSide {
    /// Parse from a keyword string (case-insensitive).
    ///
    /// Accepts:
    /// - hyphenated `align-left` / `align-right` (canonical pipe keyword)
    /// - concatenated `alignleft` / `alignright` (WordPress class alias)
    /// - bare `left` / `right` (Stage 1 TitleParams `align=` value)
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "align-left" | "alignleft" | "left" => Some(AlignSide::Left),
            "align-right" | "alignright" | "right" => Some(AlignSide::Right),
            _ => None,
        }
    }

    /// CSS class name emitted on the `<img>` (and escalated to the
    /// wrapping `<figure>` via `:has()` in site.css). Kept in lockstep
    /// with the entries in `crate::contract::components::COMPONENTS`.
    pub fn css_class(self) -> &'static str {
        match self {
            AlignSide::Left => "moss-align-left",
            AlignSide::Right => "moss-align-right",
        }
    }
}

// ---------------------------------------------------------------------------
// MediaAttrs
// ---------------------------------------------------------------------------

/// Parsed display attributes for a media reference.
///
/// In addition to moss's recognized vocabulary (`fit` / `position` / `align`),
/// `class_names` and `extra_attrs` carry author-provided passthroughs from
/// Pandoc attribute blocks (`{.theme-rounded key=value}`). The moss-vocabulary
/// fields map to typed enums and `moss-*` classes / inline style; the
/// passthrough fields flow through to the emitted HTML unmodified (classes
/// joined as a space-separated list, extras as additional attributes in
/// deterministic alphabetical order).
/// `color` is also moss-vocabulary — parsed into [`MediaAttrs::color`] for
/// the build's cover-color ladder, never emitted as class or inline style.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaAttrs {
    pub fit: Option<Fit>,
    pub position: Option<Position>,
    pub align: Option<AlignSide>,
    /// Cover band color override from a `color=<css-color>` pipe attr
    /// (`cover: page.html|color=black`). Consumed by the build's
    /// cover-color ladder (`resolve_card_color`); never emitted as inline
    /// style or class. The value must be space-free — pipe attrs are
    /// whitespace-tokenized — so `#0a0a0a`, `black`, and `rgb(10,10,10)`
    /// work; `rgb(10, 10, 10)` does not.
    pub color: Option<String>,
    /// Author-provided class names that aren't in moss's recognized
    /// vocabulary (`.align-left` / `.alignleft` get folded into `align`
    /// upstream; everything else lands here). Joined with spaces by
    /// [`Self::class_attr`] after any `moss-*` class from `css_class()`.
    pub class_names: Vec<String>,
    /// Author-provided `key=value` attributes from Pandoc attribute blocks
    /// that aren't recognized moss vocabulary. Emitted as title-params
    /// (`![alt](src "moss:k=v")`) by the wikilink Stage 1 translator in
    /// deterministic alphabetical order (BTreeMap iteration is sorted).
    pub extra_attrs: BTreeMap<String, String>,
}

impl MediaAttrs {
    /// True when no display attributes or passthroughs are set.
    pub fn is_empty(&self) -> bool {
        self.fit.is_none()
            && self.position.is_none()
            && self.align.is_none()
            && self.color.is_none()
            && self.class_names.is_empty()
            && self.extra_attrs.is_empty()
    }

    /// Build an inline CSS style string, or `None` if empty.
    ///
    /// Example output: `"object-fit:contain;object-position:left"`.
    /// `align` does NOT contribute — it emits as a class (see [`Self::css_class`]).
    /// `class_names` and `extra_attrs` are also out of style: classes ride on
    /// the `class` attribute, extras ride on their own attribute slots.
    pub fn to_inline_style(&self) -> Option<String> {
        if self.fit.is_none() && self.position.is_none() {
            return None;
        }

        let mut parts = Vec::new();
        if let Some(ref fit) = self.fit {
            parts.push(format!("object-fit:{}", fit.to_css_value()));
        }
        if let Some(ref pos) = self.position {
            parts.push(format!("object-position:{}", pos.to_css_value()));
        }
        Some(parts.join(";"))
    }

    /// CSS class name for the moss-recognized vocabulary, or `None` if no
    /// class-bearing attribute is set. Today only `align` produces a class;
    /// future class-bearing attributes can extend this method.
    ///
    /// This is the moss-prefixed half — see [`Self::class_attr`] for the
    /// merged value that includes author-provided `class_names`.
    pub fn css_class(&self) -> Option<&'static str> {
        self.align.map(AlignSide::css_class)
    }

    /// Build the full `class` attribute value, merging the moss-vocabulary
    /// class (from [`Self::css_class`]) with author-provided `class_names`.
    /// Returns `None` if both sources are empty.
    ///
    /// Order: moss-vocabulary class first (e.g. `moss-align-left`), then
    /// `class_names` in author-provided order. Both halves are joined with a
    /// single space.
    pub fn class_attr(&self) -> Option<String> {
        let moss_class = self.css_class();
        if moss_class.is_none() && self.class_names.is_empty() {
            return None;
        }
        let mut parts: Vec<&str> = Vec::new();
        if let Some(c) = moss_class {
            parts.push(c);
        }
        for c in &self.class_names {
            parts.push(c.as_str());
        }
        Some(parts.join(" "))
    }
}

// ---------------------------------------------------------------------------
// Parsing functions
// ---------------------------------------------------------------------------

/// Strip `[[` and `]]` brackets from a wikilink reference, if present.
///
/// Returns the inner text. If brackets are not present, returns the input
/// unchanged.
pub fn strip_wikilink(raw: &str) -> &str {
    wikilink_inner(raw).unwrap_or(raw.trim())
}

/// The inner text of a `[[...]]` wikilink, or `None` when `raw` is not one.
pub fn wikilink_inner(raw: &str) -> Option<&str> {
    raw.trim().strip_prefix("[[").and_then(|s| s.strip_suffix("]]"))
}

/// Split a media reference on the first `|`, returning `(path, attrs_str)`.
///
/// If there is no `|`, `attrs_str` is an empty string.
pub fn split_pipe(raw: &str) -> (&str, &str) {
    raw.split_once('|').unwrap_or((raw, ""))
}

/// Parse space-separated display-attribute keywords (and `key=value` pairs)
/// from the portion after `|`.
///
/// Recognized keywords map to [`Fit`], [`Position`], and [`AlignSide`]
/// variants, including a `focus:X%,Y%` [`Position::Focus`] point. Recognized
/// `key=value` pairs: `color=<css-color>` (stored in
/// [`MediaAttrs::color`]; consumed by the build's cover-color ladder).
/// Empty-value tokens (`color=`) are silently ignored. Unknown tokens are
/// silently ignored (callers may add diagnostic reporting).
///
/// Two-word position keywords like `"top left"` are handled: if a bare
/// directional keyword (`top`, `bottom`) is followed by another (`left`,
/// `right`), they are combined.
pub fn parse_media_attrs(raw: &str) -> MediaAttrs {
    let mut fit: Option<Fit> = None;
    let mut position: Option<Position> = None;
    let mut align: Option<AlignSide> = None;
    let mut color: Option<String> = None;

    let tokens: Vec<&str> = raw.split_whitespace().collect();
    let mut i = 0;

    while i < tokens.len() {
        let token = tokens[i];

        // Try combining with next token for two-word positions.
        if i + 1 < tokens.len() {
            let combined = format!("{} {}", token, tokens[i + 1]);
            if let Some(pos) = Position::from_keyword(&combined) {
                position = Some(pos);
                i += 2;
                continue;
            }
        }

        // Single-token fit.
        if let Some(f) = Fit::from_keyword(token) {
            fit = Some(f);
            i += 1;
            continue;
        }

        // Single-token position.
        if let Some(pos) = Position::from_keyword(token) {
            position = Some(pos);
            i += 1;
            continue;
        }

        // Single-token align (editorial runaround: align-left / align-right).
        if let Some(side) = AlignSide::from_keyword(token) {
            align = Some(side);
            i += 1;
            continue;
        }

        // key=value: cover color override.
        if let Some(value) = token.strip_prefix("color=") {
            if !value.is_empty() {
                color = Some(value.to_string());
            }
            i += 1;
            continue;
        }

        // Unknown token — skip.
        i += 1;
    }

    MediaAttrs {
        fit,
        position,
        align,
        color,
        ..Default::default()
    }
}

/// Recognize the spec § P9 width tokens (`body | wide | page | screen | full`).
///
/// `full` is the author-facing alias for `screen` — both at the fenced-div
/// AttrBlock layer (see [`crate::ast::attrs::match_width_token`]) and here at
/// the wikilink pipe-alias layer. The returned `&'static str` is the
/// canonical value-space term emitted as `data-width="..."`.
///
/// The check is exact-match on the full input (case-sensitive ASCII): a string
/// like `"wide screen"` returns `None` so that multi-word captions like
/// `![[img|wide angle shot]]` are not silently classified as a width hint.
/// Callers that handle multi-pipe wikilink aliases should split on `|` and
/// call this on each trimmed segment individually.
pub fn match_width_token(s: &str) -> Option<&'static str> {
    match s {
        "body" => Some("body"),
        "wide" => Some("wide"),
        "page" => Some("page"),
        "screen" | "full" => Some("screen"),
        _ => None,
    }
}

/// Recognize a single image-figure width segment: a named token
/// (`body|wide|page|screen|full`) OR a content-relative percent (`55%`).
///
/// Returns the canonical string to store in `Block::Figure.width`:
/// - named token → its canonical form (`full` → `screen`)
/// - percent → normalized `"NN%"` (clamped to `(0, 100]`)
///
/// Returns `None` for anything else (captions, `200x150` box sizing, px).
/// Box/px are intentionally rejected: image figures only support
/// content-relative widths in v1 (see design §"Out of scope").
///
/// MIRROR: the editor's read-side `parseImageWidth` (in the frontend
/// editor) agrees with this for every
/// canonical/moss-emitted width (named tokens, `NN%`, `NN.N%`) — the only
/// widths the write path (`set_image_width`) ever produces. The two may
/// diverge on malformed hand-typed input (this `f64::parse` accepts `"55 %"`,
/// `".5%"`, `"+5%"` which the TS regex rejects); harmless, read-side only.
/// `f64` (not `f32`) is used so fractional percents format identically to the
/// JS side, preserving the editor↔build string-equality the design relies on.
pub fn parse_image_width(seg: &str) -> Option<String> {
    let s = seg.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(named) = match_width_token(s) {
        return Some(named.to_string());
    }
    // Percent only — reject px / vh / box by requiring a '%' suffix here.
    if let Some(rest) = s.strip_suffix('%') {
        let v: f64 = rest.trim().parse().ok()?;
        if v <= 0.0 {
            return None;
        }
        let clamped = v.min(100.0);
        // Integer-preserving format: "55%" not "55.0%"; "50.5%" stays.
        let text = if clamped.fract() == 0.0 {
            format!("{}%", clamped as i64)
        } else {
            format!("{}%", clamped)
        };
        return Some(text);
    }
    None
}

/// Strip the width/size out of one pipe segment, keeping its align keyword
/// and any other tokens it carries.
///
/// Unlike a whole-segment width check, this reaches into a MIXED segment
/// like `"align-right 33%"` — a float and a size sharing one segment — and
/// drops only the size, so the editor's drag-resize write path (the only
/// caller) does not leave a stale size sitting next to a fresh one. A
/// segment that is not placement-shaped at all (a caption) is returned
/// unchanged; a segment that WAS width/size and nothing else disappears
/// (`None`).
fn segment_without_size(seg: &str) -> Option<String> {
    let Some((placement, rest)) = parse_placement_segment(seg) else {
        // Not placement-shaped at all — a caption, kept verbatim.
        return Some(seg.to_string());
    };
    let mut parts: Vec<String> = Vec::new();
    if let Some(side) = placement.align {
        parts.push(
            match side {
                AlignSide::Left => "align-left",
                AlignSide::Right => "align-right",
            }
            .to_string(),
        );
    }
    if !rest.is_empty() {
        parts.push(rest);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// Rewrite the width token of a single image's markdown, preserving all
/// other pipe segments (caption, alignment).
///
/// `width = Some("55%")` / `Some("wide")` sets (or replaces) the width;
/// `width = None` (or an unrecognized string) removes it. Works on both
/// standard `![alt|..](url)` and wikilink `![[file|..]]` syntaxes. Returns
/// the input unchanged if it is not recognized as a single image.
///
/// This is the SINGLE SOURCE OF TRUTH for the editor's drag-resize and
/// double-click-reset writes (via a Tauri command), so the produced text
/// round-trips through the build's image-width parse.
pub fn set_image_width(image_md: &str, width: Option<&str>) -> String {
    // Normalize the requested width through the same validator the build
    // uses. An unrecognized request becomes a removal.
    let new_width: Option<String> = width.and_then(parse_image_width);

    // ── Wikilink: ![[ inner ]] ───────────────────────────────────────
    if let Some(inner) = image_md
        .strip_prefix("![[")
        .and_then(|s| s.strip_suffix("]]"))
    {
        let (path, pothole) = inner.split_once('|').unwrap_or((inner, ""));
        // Strip any existing width/size from every segment, keeping align
        // and whatever else each segment carries.
        let mut parts: Vec<String> = pothole
            .split('|')
            .filter(|s| !s.is_empty())
            .filter_map(segment_without_size)
            .collect();
        if let Some(w) = new_width {
            parts.push(w);
        }
        return if parts.is_empty() {
            format!("![[{}]]", path)
        } else {
            format!("![[{}|{}]]", path, parts.join("|"))
        };
    }

    // ── Standard: ![alt](url) ────────────────────────────────────────
    if image_md.starts_with("![") {
        // `![` off the front and `)` off the back, then split on the last `](`:
        // the same three anchors the byte offsets used, without the arithmetic.
        let body = image_md
            .strip_prefix("![")
            .and_then(|rest| rest.strip_suffix(')'));
        if let Some((alt_raw, url)) = body.and_then(|body| body.rsplit_once("](")) {
            {
                // Strip width/size per segment (not upfront-filtered: an
                // empty leading segment is the empty-alt placeholder and
                // must survive to round-trip `![|55%]`, below).
                let rest_alt: String = alt_raw
                    .split('|')
                    .filter_map(segment_without_size)
                    .collect::<Vec<_>>()
                    .join("|");
                // Setting a width always emits `![{alt}|{w}]` — even when the
                // remaining alt is empty (`![|55%]`), so it round-trips with
                // the standard-image parser's empty-alt-with-width form
                // (`Block::Figure` carries the width, caption stays None).
                let alt_out = match new_width {
                    Some(w) => format!("{}|{}", rest_alt, w),
                    None => rest_alt,
                };
                return format!("![{}]({})", alt_out, url);
            }
        }
    }

    image_md.to_string()
}

/// Parse a wikilink alias for an embedded width token plus the remaining
/// alias content.
///
/// The wikilink parser (`parse_wikilink_inner`) splits on the first `|` only,
/// so when an author writes `![[img|caption|full]]`, the resulting `alias`
/// string is `"caption|full"`. This helper splits the alias on `|` and pulls
/// out a bare width-token segment (per [`match_width_token`]) without
/// reordering the others. The remaining segments are rejoined with `|`.
///
/// Returns `(width, remaining_alias)`:
///
/// - `width = Some("body|wide|page|screen")` if exactly one segment matched
///   a width token (per the "entire alias-segment is exactly one of the
///   tokens" rule). Width tokens never shadow longer captions.
/// - `remaining_alias` is the trimmed concatenation of non-width segments,
///   joined with `|`. Empty if the only segment was the width token.
///
/// If no width token is found, returns `(None, alias.to_string())` — the
/// caller falls through to its existing alias handling.
pub fn extract_width_from_alias(alias: &str) -> (Option<&'static str>, String) {
    let segments: Vec<&str> = alias.split('|').collect();
    let mut width: Option<&'static str> = None;
    let mut remaining: Vec<&str> = Vec::with_capacity(segments.len());

    for seg in &segments {
        let trimmed = seg.trim();
        if width.is_none() {
            if let Some(canonical) = match_width_token(trimmed) {
                width = Some(canonical);
                continue;
            }
        }
        remaining.push(seg);
    }

    (width, remaining.join("|"))
}

/// What one `|` segment of an image's pipe text says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AliasSegment {
    /// Where the picture sits (`wide`, `40%`, `align-left`), with any display
    /// words written in the same segment (`wide cover`) kept as the `String`.
    Placement(Placement, String),
    /// How the picture fills its frame: fit, position and `focus:` words only.
    Display(String),
    /// Anything else, kept as written: caption text.
    Caption(String),
}

/// Sort one `|` segment of an image's pipe text. Both image forms
/// (`![[x.jpg|…]]` and `![…](x.jpg)`) read every segment through this.
pub(crate) fn classify_alias_segment(segment: &str) -> AliasSegment {
    match placement::parse_placement_segment(segment.trim()) {
        Some((found, rest)) if !found.is_empty() => AliasSegment::Placement(found, rest),
        Some((_, rest)) => AliasSegment::Display(rest),
        None => AliasSegment::Caption(segment.to_string()),
    }
}

/// An image's pipe text, sorted segment by segment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageWords {
    pub placement: Placement,
    /// The display words of every segment, space-joined; empty when none.
    pub display: String,
    /// The caption segments, rejoined with `|` and trimmed.
    pub caption: String,
}

impl ImageWords {
    /// The `object-fit` / `object-position` declarations the display words ask for.
    pub fn img_style(&self) -> Option<String> {
        parse_media_attrs(&self.display).to_inline_style()
    }
}

/// Sort `alias` with [`classify_alias_segment`]. Earlier placement segments win.
pub fn read_image_words(alias: &str) -> ImageWords {
    let mut words = ImageWords::default();
    let mut display: Vec<String> = Vec::new();
    let mut caption: Vec<String> = Vec::new();
    for segment in alias.split('|') {
        match classify_alias_segment(segment) {
            AliasSegment::Placement(found, rest) => {
                words.placement.fill_from(found);
                display.push(rest);
            }
            AliasSegment::Display(rest) => display.push(rest),
            AliasSegment::Caption(text) => caption.push(text),
        }
    }
    display.retain(|d| !d.is_empty());
    words.display = display.join(" ");
    words.caption = caption.join("|").trim().to_string();
    words
}

/// Every single-token display word, in the spellings the parsers read, aliases included.
pub fn display_words() -> Vec<&'static str> {
    Fit::KEYWORDS
        .iter()
        .map(|(k, _)| *k)
        .chain(Position::KEYWORDS.iter().map(|(k, _)| *k))
        .filter(|k| !k.contains(' '))
        .collect()
}

/// The warning for a caption that looks like a mistyped display word, if it does.
///
/// The text stays the caption; this only names it. It covers a `focus:` point that
/// does not parse, and a single word with two adjacent letters swapped from a display
/// word (see [`swapped_from`]). Multi-word captions are never flagged.
pub fn display_word_warning(caption: &str) -> Option<String> {
    let word = caption.trim();
    if word.is_empty() || word.contains(char::is_whitespace) {
        return None;
    }
    let lower = word.to_lowercase();
    let words = display_words();
    let known = words.join(", ");
    if lower.starts_with("focus:") && Position::from_keyword(&lower).is_none() {
        return Some(format!(
            "'{word}' is not a focus point; write focus:X%,Y% with whole percentages from 0 to 100 (display words: {known})"
        ));
    }
    let near = words.iter().find(|known_word| swapped_from(&lower, known_word))?;
    Some(format!(
        "'{word}' is not an image display word; did you mean '{near}'? Display words: {known}"
    ))
}

/// Whether `word` is `known` with two adjacent letters swapped.
///
/// Only a known word of five letters or more counts, so a short caption one swap
/// away from a display word is left alone.
fn swapped_from(word: &str, known: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    let k: Vec<char> = known.chars().collect();
    k.len() >= 5
        && w.len() == k.len()
        && w != k
        && (0..k.len() - 1).any(|i| {
            let mut s = w.clone();
            s.swap(i, i + 1);
            s == k
        })
}

/// Escape a string for safe use in HTML text or attribute values.
///
/// Replaces `&`, `"`, `'`, `<`, and `>` with their HTML entities.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;
