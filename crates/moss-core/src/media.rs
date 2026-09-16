//! Unified media reference resolution and display attributes.
//!
//! All media reference contexts in moss (frontmatter cover, hero, gallery,
//! inline images, wikilink embeds) call into this module to parse pipe-
//! separated display attributes (`object-fit`, `object-position`).
//!
//! Pure Rust, zero I/O.

use std::collections::BTreeMap;

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
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "cover" => Some(Fit::Cover),
            "contain" => Some(Fit::Contain),
            "fill" => Some(Fit::Fill),
            "none" => Some(Fit::None),
            "scale-down" | "scaledown" => Some(Fit::ScaleDown),
            _ => Option::None,
        }
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
}

impl Position {
    /// Return the CSS `object-position` value.
    pub fn to_css_value(&self) -> &str {
        match self {
            Position::Center => "center",
            Position::Left => "left",
            Position::Right => "right",
            Position::Top => "top",
            Position::Bottom => "bottom",
            Position::TopLeft => "top left",
            Position::TopRight => "top right",
            Position::BottomLeft => "bottom left",
            Position::BottomRight => "bottom right",
        }
    }

    /// Parse from a keyword string (case-insensitive).
    ///
    /// Accepts hyphenated (`"top-left"`), concatenated (`"topleft"`), and
    /// space-separated (`"top left"`) forms.
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "center" => Some(Position::Center),
            "left" => Some(Position::Left),
            "right" => Some(Position::Right),
            "top" => Some(Position::Top),
            "bottom" => Some(Position::Bottom),
            "top-left" | "topleft" | "top left" => Some(Position::TopLeft),
            "top-right" | "topright" | "top right" => Some(Position::TopRight),
            "bottom-left" | "bottomleft" | "bottom left" => Some(Position::BottomLeft),
            "bottom-right" | "bottomright" | "bottom right" => Some(Position::BottomRight),
            _ => Option::None,
        }
    }
}

// ---------------------------------------------------------------------------
// AlignSide — editorial runaround alignment (text wraps around half-width image)
// ---------------------------------------------------------------------------

/// Image alignment for editorial runaround layout. Mirrors WordPress's
/// `alignleft` / `alignright` block-editor convention; the moss CSS class
/// is `moss-align-left` / `moss-align-right`. Float behavior plus mobile
/// collapse (≤48rem) live in `src-tauri/src/assets/css/site.css`.
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
///
/// See `docs/reference/unified-image-emission.md` Decision #10.
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
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("[[")
        .and_then(|s| s.strip_suffix("]]"))
        .unwrap_or(trimmed)
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
/// variants. Recognized `key=value` pairs: `color=<css-color>` (stored in
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
/// MIRROR: the editor's read-side `parseImageWidth` in
/// `frontend/app/editor/cm-image-extract.ts` agrees with this for every
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

/// Split pipe-delimited alt/alias text into `(remaining, width)`.
///
/// Pulls out the FIRST segment that `parse_image_width` recognizes; all
/// other segments are rejoined with `|` in order. If no segment is a
/// width, returns the input unchanged with `None`. Mirrors the segment
/// model of [`extract_width_from_alias`] but for the image width vocabulary
/// (named + percent).
pub fn split_alt_width(text: &str) -> (String, Option<String>) {
    let mut width: Option<String> = None;
    let mut remaining: Vec<&str> = Vec::new();
    for seg in text.split('|') {
        if width.is_none() {
            if let Some(w) = parse_image_width(seg) {
                width = Some(w);
                continue;
            }
        }
        remaining.push(seg);
    }
    (remaining.join("|"), width)
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
        // Strip any existing width from the pothole, keep other segments.
        let (rest, _old) = split_alt_width(pothole);
        let segments: Vec<&str> = rest.split('|').filter(|s| !s.is_empty()).collect();
        let mut parts: Vec<String> = segments.iter().map(|s| s.to_string()).collect();
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
                let (rest_alt, _old) = split_alt_width(alt_raw);
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

/// Return `true` if every token in `text` is a recognized display keyword.
///
/// Handles single-token keywords (`"left"`, `"contain"`) and two-word position
/// keywords (`"top left"`).  An empty string returns `false`.
pub fn is_all_display_keywords(text: &str) -> bool {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.is_empty() {
        return false;
    }

    let mut i = 0;
    while i < tokens.len() {
        // Try combining current token with next for two-word positions.
        if i + 1 < tokens.len() {
            let combined = format!("{} {}", tokens[i], tokens[i + 1]);
            if Position::from_keyword(&combined).is_some() {
                i += 2;
                continue;
            }
        }

        if Fit::from_keyword(tokens[i]).is_some() {
            i += 1;
            continue;
        }

        if Position::from_keyword(tokens[i]).is_some() {
            i += 1;
            continue;
        }

        if AlignSide::from_keyword(tokens[i]).is_some() {
            i += 1;
            continue;
        }

        return false;
    }

    true
}

/// True when every whitespace-separated token in `alias` is either a
/// recognized display keyword (fit / position / align) OR a canonical
/// width token (body / wide / page / screen / full).
///
/// This is the structural-vs-caption classifier for image aliases: a
/// fully-structural alias contributes only to display params; anything else
/// becomes caption / alt text. The [`is_all_display_keywords`] half is
/// unchanged (covers two-word position tokens like `top left`); the
/// width-token half lets authors write `align-left wide` without breaking
/// the pipe.
///
/// Lifted from `resolve::embed_renderer` (Phase 1 of the image-embed
/// synth-collapse) so it survives `ImageRenderer`'s deletion — it is the
/// load-bearing half of [`classify_image_alias`].
pub(crate) fn is_structural_alias(alias: &str) -> bool {
    // Fast path: any caption-like text fails `is_all_display_keywords`
    // and would also fail the per-token loop below.
    if is_all_display_keywords(alias) {
        return true;
    }
    let tokens: Vec<&str> = alias.split_whitespace().collect();
    if tokens.is_empty() {
        return false;
    }
    // Walk tokens; admit width tokens, otherwise defer to display-keyword
    // recognition (per-token, since position tokens may pair across two).
    let mut i = 0;
    while i < tokens.len() {
        // Width token: single-token, simple admit.
        if match_width_token(tokens[i]).is_some() {
            i += 1;
            continue;
        }
        // Two-word position (e.g. `top left`).
        if i + 1 < tokens.len() {
            let combined = format!("{} {}", tokens[i], tokens[i + 1]);
            if Position::from_keyword(&combined).is_some() {
                i += 2;
                continue;
            }
        }
        // Single-token display keyword.
        if Fit::from_keyword(tokens[i]).is_some()
            || Position::from_keyword(tokens[i]).is_some()
            || AlignSide::from_keyword(tokens[i]).is_some()
        {
            i += 1;
            continue;
        }
        return false;
    }
    true
}

/// Classification of an image-embed pipe alias into its display-vs-caption
/// role.
///
/// The pipe alias of `![[photo.jpg|<alias>]]` is one of three things:
/// a run of structural display keywords (`cover`, `wide cover`), human-
/// readable caption prose (`My nice photo`), or absent/empty. This struct
/// captures the disambiguation so every image-embed call site classifies
/// identically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageAliasClass {
    /// Structural display-keyword run (e.g. `"cover"`, `"wide cover"`) to be
    /// fed to `parse_media_attrs`; `None` when the alias is a caption or
    /// empty.
    pub display_keywords: Option<String>,
    /// Caption text (also used as `alt`) when the alias is human-readable
    /// prose; `None` for structural/empty aliases.
    ///
    /// **Invariant:** never `Some("")`. An empty alias yields `None` so
    /// callers never emit an empty `<figcaption>`.
    pub caption: Option<String>,
}

/// Classify an image-embed pipe alias into [`ImageAliasClass`].
///
/// Mirrors the 3-way split previously inlined in
/// `ImageRenderer::render_to_markdown` (now lifted so it survives that
/// struct's deletion in the image-embed synth-collapse):
///
/// - `None`                       → both `None`
/// - `Some("")` (empty)           → both `None` (no empty figcaption)
/// - `Some(s)` and structural     → `display_keywords = Some(s)`, `caption = None`
/// - `Some(other)`                → `display_keywords = None`, `caption = Some(other)`
pub(crate) fn classify_image_alias(alias: Option<&str>) -> ImageAliasClass {
    match alias {
        // Empty alias (`![[file|]]`) is treated as no alias. Matches the
        // historical `alias.is_empty()` guard exactly (no extra trimming).
        Some(a) if a.is_empty() => ImageAliasClass {
            display_keywords: None,
            caption: None,
        },
        Some(a) if is_structural_alias(a) => ImageAliasClass {
            display_keywords: Some(a.to_string()),
            caption: None,
        },
        Some(other) => ImageAliasClass {
            display_keywords: None,
            caption: Some(other.to_string()),
        },
        None => ImageAliasClass {
            display_keywords: None,
            caption: None,
        },
    }
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
