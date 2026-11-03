//! A diagram as a self-contained picture, for places that run no stylesheet:
//! a feed reader, a mail client, the rasterizer that makes their PNG.
//!
//! On a page the drawing takes its colours and fonts from `--pele-*` custom
//! properties, which the diagrams stylesheet maps to moss's tokens. Here every
//! `var()` is written out with the light value of that same mapping, so the
//! picture looks like the page in its light theme. The mapping is read from
//! the stylesheet itself, which stays its one owner.
//!
//! A picture is set only in fonts moss carries: the embedded Inter, and the
//! pinned CJK font when Inter lacks a character and that font has it. Nothing
//! from the host is consulted, so a picture's bytes are the same on every build
//! machine. A diagram with a character neither font has gets no picture, rather
//! than a hosted image with a hole in it.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::engine;
use crate::build::features::open_tag_attr as attr;
use crate::build::markdown::math::font;
use crate::i18n::Language;

const DIAGRAMS_CSS: &str = include_str!("../../../assets/css/site/diagrams.css");

/// The family every label is set in.
pub(crate) const FONT_FAMILY: &str = "Inter";
/// Changes when the picture's drawing changes (padding, flattening, raster),
/// so every address re-keys instead of serving new bytes at an old URL.
const PROJECTION: &str = "1";
pub(crate) const RASTER_SCALE: u32 = 2;
/// Page-colour margin around the drawing, in CSS px at 1×.
pub(crate) const PAD_PX: u32 = 16;

/// The fonts a picture is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fonts {
    Inter,
    InterAndCjk,
}

/// Which fonts set every character a drawing shows, or `None` when no font
/// moss carries has one of them. Read from the drawing, not the source: the
/// renderer turns character references (`&#128512;`, Mermaid's `#9829;`)
/// into the characters themselves.
pub(crate) fn fonts_for(drawn_svg: &str) -> Option<Fonts> {
    let missing: Vec<char> = shown_text(drawn_svg).chars().filter(|c| !c.is_whitespace() && !c.is_control() && !inter_has(*c)).collect();
    if missing.is_empty() {
        return Some(Fonts::Inter);
    }
    let pinned = font::pinned()?;
    let (bytes, index) = pinned.face();
    let face = ttf_parser::Face::parse(&bytes, index).ok()?;
    missing.iter().all(|c| face.glyph_index(*c).is_some()).then_some(Fonts::InterAndCjk)
}

/// The text between a drawing's tags, its character references decoded.
fn shown_text(svg: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut rest = svg;
    while let Some(c) = rest.chars().next() {
        if !in_tag && c == '&' {
            if let Some((decoded, used)) = rest.find(';').filter(|&end| end <= 12).and_then(|end| decode_reference(&rest[..=end]).map(|d| (d, end + 1))) {
                out.push(decoded);
                rest = &rest[used..];
                continue;
            }
        }
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn decode_reference(reference: &str) -> Option<char> {
    let body = reference.strip_prefix('&')?.strip_suffix(';')?;
    match body {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let number = body.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

fn inter_has(c: char) -> bool {
    static FACE: std::sync::OnceLock<Option<ttf_parser::Face<'static>>> = std::sync::OnceLock::new();
    FACE.get_or_init(|| ttf_parser::Face::parse(crate::build::page::og_card::inter_faces()[0], 0).ok())
        .as_ref()
        .is_some_and(|face| face.glyph_index(c).is_some())
}

/// The picture's content address: 16 lowercase hex over everything that
/// decides its bytes. The page's figure, the build's sweep and the newsletter
/// all reach it through [`fonts_for`] and this, so they agree on the URL and
/// on whether there is one.
pub(crate) fn address(source: &str, fonts: Fonts) -> String {
    let cjk = match fonts {
        Fonts::Inter => "none",
        Fonts::InterAndCjk => font::pinned().map_or("none", |f| f.sha256()),
    };
    let mut h = Sha256::new();
    for part in [source, engine::pin(), PROJECTION, &RASTER_SCALE.to_string(), &PAD_PX.to_string(), inter_fingerprint(), cjk] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    for (name, value) in palette() {
        h.update(name.as_bytes());
        h.update(value.as_bytes());
        h.update([0]);
    }
    format!("{:x}", h.finalize())[..16].to_string()
}

/// The fonts and address of the picture of `source`, given its drawing, or
/// `None` when it gets no picture. The page's figure and [`picture`] both
/// decide through this.
pub(crate) fn address_for(source: &str, drawn_svg: &str) -> Option<(Fonts, String)> {
    let fonts = fonts_for(drawn_svg)?;
    Some((fonts, address(source, fonts)))
}

fn inter_fingerprint() -> &'static str {
    static SHA: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SHA.get_or_init(|| {
        let mut h = Sha256::new();
        for face in crate::build::page::og_card::inter_faces() {
            h.update(face);
        }
        format!("{:x}", h.finalize())
    })
}

/// The size a picture is shown at, in CSS px: the drawing plus its margin.
pub(crate) fn size_1x(width: f64, height: f64) -> (u32, u32) {
    (width.ceil() as u32 + 2 * PAD_PX, height.ceil() as u32 + 2 * PAD_PX)
}

/// One diagram's wide layout with every colour and font written in, the
/// fonts it needs, its address, and the name it carries for a reader who
/// cannot see it.
pub(crate) struct Picture {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    pub alt: String,
    pub fonts: Fonts,
    pub hash: String,
}

/// Draws `source` as a picture, named in `lang`, or `None` when the diagram
/// does not draw or shows a character no carried font sets. Its ids are fixed
/// (`p`): a picture stands alone, so nothing else shares its id space.
pub(crate) fn picture(source: &str, lang: Language) -> Option<Picture> {
    let drawn: engine::Drawn = super::draw(source, "p", 0, lang, None, &mut Vec::new()).ok()?;
    let (fonts, hash) = address_for(source, &drawn.svg)?;
    Some(Picture { alt: alt_text(&drawn.svg), svg: flatten(&drawn.svg), width: drawn.width, height: drawn.height, fonts, hash })
}

/// A drawing's accessible name: its `aria-label`, or the text of the `<title>`
/// its `aria-labelledby` points at. Already escaped for an attribute.
pub(crate) fn alt_text(svg: &str) -> String {
    let open = &svg[..svg.find('>').unwrap_or(0)];
    if let Some(label) = attr(open, "aria-label") {
        return label.to_string();
    }
    svg.find("<title")
        .and_then(|at| {
            let body = &svg[at..];
            let body = &body[body.find('>')? + 1..];
            Some(body[..body.find("</title>")?].replace('"', "&quot;"))
        })
        .unwrap_or_default()
}

/// Every `--pele-*` property the stylesheet sets, with its light value.
pub(crate) fn palette() -> &'static [(String, String)] {
    static PALETTE: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    PALETTE.get_or_init(|| {
        let tokens = moss_core::contract::tokens::load_tokens().unwrap_or(moss_core::contract::tokens::Tokens { groups: Vec::new() });
        let block = DIAGRAMS_CSS.split(".moss-diagram {").nth(1).and_then(|b| b.split('}').next()).unwrap_or("");
        let mut out: Vec<(String, String)> = block
            .split(';')
            .filter_map(|decl| decl.trim().split_once(':'))
            .filter(|(name, _)| name.trim().starts_with("--pele-"))
            .map(|(name, value)| {
                let (name, value) = (name.trim().to_string(), value.trim());
                let resolved = if name.starts_with("--pele-font") {
                    FONT_FAMILY.to_string()
                } else {
                    value
                        .strip_prefix("var(")
                        .and_then(|v| v.strip_suffix(')'))
                        .and_then(|token| moss_core::contract::tokens::find_token(&tokens, token.trim().trim_start_matches("--")))
                        .map_or_else(|| value.to_string(), |(light, _)| light.to_string())
                };
                (name, resolved)
            })
            .collect();
        out.sort();
        out
    })
}

/// Writes out every `var(--…)` inside the markup's tags, and drops the root's
/// custom property declarations, which nothing reads once they are written in.
/// Text between tags is never touched, and anything that is not a complete
/// `var(--…)` is copied as it stands.
pub(crate) fn flatten(svg: &str) -> String {
    let open_end = svg.find('>').unwrap_or(0);
    let mut defs: HashMap<String, String> = palette().iter().cloned().collect();
    let root_style = attr(&svg[..open_end], "style");
    for decl in root_style.unwrap_or("").split(';') {
        if let Some((name, value)) = decl.split_once(':') {
            if name.trim().starts_with("--") {
                defs.insert(name.trim().to_string(), value.trim().to_string());
            }
        }
    }
    let mut out = String::with_capacity(svg.len());
    let mut in_tag = false;
    let mut rest = svg;
    while let Some(c) = rest.chars().next() {
        if in_tag {
            if let Some((value, used)) = resolve_var(rest, &defs, 0) {
                out.push_str(&value);
                rest = &rest[used..];
                continue;
            }
        }
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ => {}
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    let Some(style) = root_style else { return out };
    let kept: Vec<&str> = style.split(';').filter(|d| !d.trim().starts_with("--") && !d.trim().is_empty()).collect();
    let open = &out[..out.find('>').unwrap_or(0)];
    let Some(start) = open.find(" style=\"").map(|at| at + 8) else { return out };
    let end = start + open[start..].find('"').unwrap_or(0);
    format!("{}{}{}", &out[..start], flatten_value(&kept.join(";"), &defs, 0), &out[end..])
}

fn flatten_value(value: &str, defs: &HashMap<String, String>, depth: u8) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(c) = rest.chars().next() {
        if let Some((resolved, used)) = resolve_var(rest, defs, depth) {
            out.push_str(&resolved);
            rest = &rest[used..];
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// The value of the `var(--name[, fallback])` at the start of `text`, and how
/// many bytes it spans; `None` when `text` does not start with a complete one.
fn resolve_var(text: &str, defs: &HashMap<String, String>, depth: u8) -> Option<(String, usize)> {
    if !text.starts_with("var(--") {
        return None;
    }
    let mut level = 0;
    let mut end = None;
    for (i, c) in text.char_indices() {
        match c {
            // A reference never spans the end of the value it sits in.
            '"' | '<' | '>' => return None,
            '(' => level += 1,
            ')' => {
                level -= 1;
                if level == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let inner = &text[4..end - 1];
    let (name, fallback) = match inner.split_once(',') {
        Some((name, fallback)) => (name.trim(), Some(fallback.trim())),
        None => (inner.trim(), None),
    };
    let value = defs.get(name).map(String::as_str).or(fallback).unwrap_or("");
    let value = if depth < 8 && value.contains("var(--") { flatten_value(value, defs, depth + 1) } else { value.to_string() };
    Some((value, end))
}
