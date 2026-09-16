//! The card's geometry: title fitting, the four SVG layouts (title-only and
//! plate, horizontal and vertical), and the compositor that draws a plate's
//! pixels onto a rasterized card.
//!
//! Everything here is a pure function of [`CardInputs`] plus, for a plate, the
//! decoded picture. Caching, hashing, writing and manifest registration live
//! in the parent module.

use super::{CardInputs, CardError, xml_escape};
use std::path::Path;

/// Estimated advance in ems: full-width CJK glyphs advance one em,
/// everything else is estimated at Inter's ~half-em average. An estimate is
/// enough — the goal is "never overflow the frame badly", not typesetting.
/// The old 40-CHAR cap was Latin-tuned and let CJK titles (one em per
/// glyph) overflow the card by hundreds of pixels.
fn char_em(ch: char) -> f32 {
    let c = ch as u32;
    let full_width = matches!(c,
        0x1100..=0x115F        // Hangul Jamo
        | 0x2E80..=0xA4CF      // CJK radicals, kana, Han, Yi
        | 0xAC00..=0xD7A3      // Hangul syllables
        | 0xF900..=0xFAFF      // CJK compatibility ideographs
        | 0xFE30..=0xFE4F      // CJK compatibility forms
        | 0xFF00..=0xFF60      // Fullwidth forms
        | 0x20000..=0x3FFFD    // CJK extensions B+
    );
    if full_width { 1.0 } else { 0.5 }
}

fn est_em(s: &str) -> f32 {
    s.chars().map(char_em).sum()
}

/// Title line budget: (1200 − 2×80 margin) / 72px ≈ 14.4 em per line.
const TITLE_LINE_EM: f32 = 14.4;
/// Site-name budget: 1040px at 32px = 32.5 em, single line.
const SITE_NAME_EM: f32 = 32.5;
/// Vertical title column budget: (630 − 84 top − 78 bottom) / 72px = 6.5 em.
const TITLE_COLUMN_EM: f32 = 6.5;
/// Vertical site-name column budget: it ends at y=560 and may rise to the
/// title's top margin, (560 − 84) / 32px ≈ 14.9 em; 14 keeps a glyph clear.
const SITE_NAME_COLUMN_EM: f32 = 14.0;

/// Wrap the title into at most two lines of `budget` ems by estimated width
/// — SVG <text> does not wrap on its own. Latin lines prefer breaking at the
/// last space; the second line ellipsizes on overflow.
pub(super) fn layout_title(title: &str, budget: f32) -> Vec<String> {
    // A leading space would otherwise become rfind(' ')==Some(0) → an empty
    // first tspan line pushing the title down a row.
    let title = title.trim();
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_em = 0.0f32;
    for ch in title.chars() {
        let w = char_em(ch);
        if cur_em + w > budget && !cur.is_empty() {
            if lines.len() == 1 {
                lines.push(fit_with_ellipsis(cur, cur_em, budget));
                return lines;
            }
            if let Some(bi) = cur.rfind(' ') {
                let tail = cur.get(bi + 1..).unwrap_or_default().to_string();
                cur.truncate(bi);
                lines.push(std::mem::take(&mut cur));
                cur_em = est_em(&tail);
                cur = tail;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur_em = 0.0;
            }
            if ch != ' ' {
                cur.push(ch);
                cur_em += w;
            }
        } else {
            cur.push(ch);
            cur_em += w;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn fit_with_ellipsis(mut s: String, mut em: f32, budget: f32) -> String {
    while em + 0.5 > budget {
        match s.pop() {
            Some(c) => em -= char_em(c),
            None => break,
        }
    }
    while s.ends_with(' ') {
        s.pop();
    }
    s.push('\u{2026}');
    s
}

fn truncate_site_name(name: &str, budget: f32) -> String {
    if est_em(name) <= budget {
        return name.to_string();
    }
    let mut s = String::new();
    let mut em = 0.0f32;
    for ch in name.chars() {
        let w = char_em(ch);
        if em + w + 0.5 > budget {
            break;
        }
        s.push(ch);
        em += w;
    }
    while s.ends_with(' ') {
        s.pop();
    }
    s.push('\u{2026}');
    s
}

/// The 1200x630 frame the picture is fitted into, in card pixels.
///
/// Computed once from the DECODED image's dimensions and used twice — the SVG
/// strokes its edge, the compositor draws into it — so the hairline and the
/// picture cannot land in different places.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PlateRect {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) w: f32,
    pub(super) h: f32,
}

/// The largest upscale a plate is allowed. A cover smaller than its box is
/// left small rather than interpolated to fill: past about 2x the picture
/// stops being the work and starts being a blur of it.
const PLATE_MAX_UPSCALE: f32 = 2.0;

/// Fit `iw x ih` entirely inside the box, centred, nothing cropped.
fn fit_centered(iw: u32, ih: u32, x0: f32, x1: f32, y0: f32, y1: f32) -> PlateRect {
    let (bw, bh) = (x1 - x0, y1 - y0);
    let scale = (bw / iw.max(1) as f32)
        .min(bh / ih.max(1) as f32)
        .min(PLATE_MAX_UPSCALE);
    let (w, h) = ((iw as f32 * scale).max(1.0), (ih as f32 * scale).max(1.0));
    PlateRect { x: x0 + (bw - w) / 2.0, y: y0 + (bh - h) / 2.0, w, h }
}

/// Vertical plate card: title columns at 1110, 1018, …; site name one pitch
/// further left. The picture takes everything to the left of that band.
const PLATE_V_TITLE_X: f32 = 1110.0;
const PLATE_V_PITCH: f32 = 92.0;
/// Column-height budget for a plate card's title: (630 − 64 top − 42 bottom) / 60px.
const PLATE_TITLE_COLUMN_EM: f32 = 8.5;
/// Line budget for a horizontal plate card's title: (1120 − 80) / 52px.
const PLATE_TITLE_LINE_EM: f32 = 20.0;

/// Where the picture goes, given the title's own column/line count.
///
/// The vertical box stops at x=900 even when the title is one short column,
/// so the picture's centre stays inside the centre 630x630 square WeChat
/// crops to. It is the picture that has to survive that crop, not the title:
/// WeChat draws the link's title as text in the bubble beside the thumbnail,
/// so the one thing the square must hold is the work.
pub(super) fn layout_plate(inputs: &CardInputs, iw: u32, ih: u32) -> PlateRect {
    if inputs.vertical {
        let columns = layout_title(inputs.title, PLATE_TITLE_COLUMN_EM).len() as f32;
        // One pitch past the site-name column, then half a pitch of air.
        let text_left = PLATE_V_TITLE_X - PLATE_V_PITCH * (columns + 0.5);
        fit_centered(iw, ih, 80.0, text_left.min(900.0), 50.0, 580.0)
    } else {
        fit_centered(iw, ih, 80.0, 1120.0, 56.0, 424.0)
    }
}

/// A hairline on the picture's edge, in the foreground colour at low opacity.
///
/// A scan of paper on a paper-coloured ground has no edge of its own; without
/// this the plate dissolves into the card. Stroked from the SVG (i.e. under
/// the composited pixels) and centred on the boundary, so the half outside
/// the picture is what shows.
fn plate_frame(rect: PlateRect, fg: &str) -> String {
    format!(
        r##"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" fill="none" stroke="{fg}" stroke-opacity="0.25" stroke-width="2"/>"##,
        x = rect.x, y = rect.y, w = rect.w, h = rect.h, fg = fg
    )
}

/// The site name to print beneath the title, or nothing.
///
/// A home page's card title IS the site name, so printing both set the same
/// four characters twice on one card — once large, once small, saying nothing
/// the first had not. Empty means the column/line is not drawn at all, which
/// the vertical layouts also fold into their centring.
fn site_name_line(inputs: &CardInputs, budget: f32) -> Option<String> {
    (inputs.site_name != inputs.title)
        .then(|| xml_escape(&truncate_site_name(inputs.site_name, budget)))
}

/// The card's ground and spine, with `body` laid on it.
///
/// The accent runs along the edge the text begins at — the left of a
/// horizontal card, the top of a vertical one — so it reads as the card's
/// spine rather than as a stripe. Stated once here instead of implied by four
/// copies of the same two rects.
fn card_svg(inputs: &CardInputs, body: &str) -> String {
    let spine = if inputs.vertical {
        r#"width="1200" height="12""#
    } else {
        r#"width="12" height="630""#
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="630" viewBox="0 0 1200 630">
  <rect width="1200" height="630" fill="{bg}"/>
  <rect x="0" y="0" {spine} fill="{accent}"/>
  {body}
</svg>"##,
        bg = inputs.bg_color,
        accent = inputs.accent_color,
        spine = spine,
        body = body,
    )
}

pub(super) fn build_svg(inputs: &CardInputs, plate: Option<PlateRect>) -> String {
    match (inputs.vertical, plate) {
        (true, Some(rect)) => build_vertical_plate_svg(inputs, rect),
        (true, None) => build_vertical_svg(inputs),
        (false, Some(rect)) => build_horizontal_plate_svg(inputs, rect),
        (false, None) => build_horizontal_svg(inputs),
    }
}

fn build_horizontal_svg(inputs: &CardInputs) -> String {
    let title_tspans: String = layout_title(inputs.title, TITLE_LINE_EM)
        .iter()
        .enumerate()
        .map(|(i, line)| {
            format!(
                r##"<tspan x="80" dy="{}">{}</tspan>"##,
                if i == 0 { 0 } else { 88 },
                xml_escape(line)
            )
        })
        .collect();
    let site_name = site_name_line(inputs, SITE_NAME_EM).map_or_else(String::new, |n| {
        format!(r##"<text x="80" y="540" font-family="Inter" font-size="32" fill="{fg}" opacity="0.65"><tspan>{n}</tspan></text>"##, fg = inputs.fg_color, n = n)
    });
    card_svg(
        inputs,
        &format!(
            r##"<text x="80" y="280" font-family="Inter" font-size="72" font-weight="700" fill="{fg}">{title_tspans}</text>
  {site_name}"##,
            fg = inputs.fg_color,
            title_tspans = title_tspans,
            site_name = site_name,
        ),
    )
}

/// The card for a vertically typeset page with no picture: the title runs
/// top-to-bottom like a book's title strip (題籤) with the site name in a
/// shorter column immediately to its left, and the whole block is CENTRED.
///
/// Centred, not hung at the right, since 2026-09-11. The right-hung form put
/// the title against one edge of the centre 630px square and the site name
/// against the other, with the card's whole middle empty between them: at the
/// ~500px a timeline actually renders, two lonely corners. Reading a 題籤 as
/// one object is what the centring restores; the 留白 is still most of the
/// card, which is the point of the form.
///
/// Both columns stay inside the centre 630x630 square (x 285..915) — WeChat
/// crops `og:image` to that square, and here, unlike the plate card, the
/// title IS the whole content.
///
/// `writing-mode="tb"` is usvg's vertical mode: it lays the run out and
/// rotates it 90° about `(x, y)`, standing Han glyphs upright and turning
/// Latin sideways — the same orientation CSS `vertical-rl` gives a mixed run
/// on the page. No `vert` feature substitution, so 「」 keep their horizontal
/// forms; a title is short enough for that to pass.
fn build_vertical_svg(inputs: &CardInputs) -> String {
    let lines = layout_title(inputs.title, TITLE_COLUMN_EM);
    let name = site_name_line(inputs, SITE_NAME_COLUMN_EM);
    // One column per title line plus the site-name column when there is one,
    // at a 100px pitch, centred on x=600. `col_x(0)` is the rightmost —
    // vertical Chinese reads right to left, so the title's first column is the
    // block's right edge.
    let total = lines.len() + usize::from(name.is_some());
    let col_x = |k: usize| 600.0 + (total as f32 - 1.0) * 50.0 - 100.0 * k as f32;
    let title_columns: String = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            format!(
                r##"<text x="{:.0}" y="84" writing-mode="tb" font-family="Inter" font-size="72" font-weight="700" fill="{}">{}</text>
  "##,
                col_x(i),
                inputs.fg_color,
                xml_escape(line)
            )
        })
        .collect();
    let site_name = name.map_or_else(String::new, |n| {
        format!(
            r##"<text x="{x:.0}" y="560" writing-mode="tb" text-anchor="end" font-family="Inter" font-size="32" fill="{fg}" opacity="0.65">{n}</text>"##,
            x = col_x(lines.len()), fg = inputs.fg_color, n = n
        )
    });
    card_svg(inputs, &format!("{title_columns}{site_name}"))
}

/// The plate card for a vertically typeset page: the whole picture on the
/// site's ground, the title standing top-to-bottom at the right of it, the
/// site name in a shorter column beside the title. A museum label.
fn build_vertical_plate_svg(inputs: &CardInputs, rect: PlateRect) -> String {
    let lines = layout_title(inputs.title, PLATE_TITLE_COLUMN_EM);
    let col_x = |k: usize| PLATE_V_TITLE_X - PLATE_V_PITCH * k as f32;
    let title_columns: String = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            format!(
                r##"<text x="{:.0}" y="64" writing-mode="tb" font-family="Inter" font-size="60" font-weight="700" fill="{}">{}</text>
  "##,
                col_x(i),
                inputs.fg_color,
                xml_escape(line)
            )
        })
        .collect();
    let site_name = site_name_line(inputs, SITE_NAME_COLUMN_EM).map_or_else(String::new, |n| {
        format!(
            r##"<text x="{x:.0}" y="566" writing-mode="tb" text-anchor="end" font-family="Inter" font-size="28" fill="{fg}" opacity="0.65">{n}</text>"##,
            x = col_x(lines.len()), fg = inputs.fg_color, n = n
        )
    });
    card_svg(
        inputs,
        &format!(
            "{frame}\n  {title_columns}{site_name}",
            frame = plate_frame(rect, inputs.fg_color),
        ),
    )
}

/// The plate card for a horizontally set page: picture above, title and site
/// name along the foot.
fn build_horizontal_plate_svg(inputs: &CardInputs, rect: PlateRect) -> String {
    let lines = layout_title(inputs.title, PLATE_TITLE_LINE_EM);
    // Last baseline fixed at 522 so a one-line and a two-line title share a
    // foot; the second line grows upward into the picture's air, not down
    // into the site name.
    let first_y = 522.0 - 62.0 * (lines.len().saturating_sub(1)) as f32;
    let title_tspans: String = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            format!(
                r##"<tspan x="80" dy="{}">{}</tspan>"##,
                if i == 0 { 0 } else { 62 },
                xml_escape(line)
            )
        })
        .collect();
    let site_name = site_name_line(inputs, SITE_NAME_EM).map_or_else(String::new, |n| {
        format!(r##"<text x="80" y="578" font-family="Inter" font-size="28" fill="{fg}" opacity="0.65"><tspan>{n}</tspan></text>"##, fg = inputs.fg_color, n = n)
    });
    card_svg(
        inputs,
        &format!(
            r##"{frame}
  <text x="80" y="{first_y:.0}" font-family="Inter" font-size="52" font-weight="700" fill="{fg}">{title_tspans}</text>
  {site_name}"##,
            fg = inputs.fg_color,
            frame = plate_frame(rect, inputs.fg_color),
            first_y = first_y,
            title_tspans = title_tspans,
            site_name = site_name,
        ),
    )
}

/// Decode the plate, apply its EXIF orientation, and scale it to `rect`.
///
/// `decode_oriented` is the build's own decoder — the one the responsive
/// ladder and the fallback raster use — so a rotated phone photo lands on the
/// card the way it lands on the page, and the 1 GiB allocation ceiling that
/// guards those paths guards this one.
pub(super) fn decode_plate(source: &Path) -> Result<image::DynamicImage, CardError> {
    crate::build::media::rungs::decode_oriented(source).map_err(CardError::Plate)
}

/// Draw the decoded picture into `rect` on an already-rasterized card.
///
/// Compositing rather than an SVG `<image>`: usvg is built here without its
/// `image` feature (see Cargo.toml), so an `<image href>` element would parse
/// and render nothing at all.
pub(super) fn draw_plate(
    pixmap: &mut tiny_skia::Pixmap,
    img: &image::DynamicImage,
    rect: PlateRect,
) -> Result<(), CardError> {
    let (w, h) = (rect.w.round().max(1.0) as u32, rect.h.round().max(1.0) as u32);
    let scaled = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3).to_rgba8();
    let mut plate = tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| CardError::Encode("plate pixmap allocation failed".into()))?;
    // tiny-skia stores PREMULTIPLIED RGBA; `image` stores straight alpha.
    // Covers are opaque in practice, but a PNG cover with transparency would
    // otherwise composite too bright.
    for (dst, src) in plate.pixels_mut().iter_mut().zip(scaled.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    pixmap.draw_pixmap(
        rect.x.round() as i32,
        rect.y.round() as i32,
        plate.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    Ok(())
}

