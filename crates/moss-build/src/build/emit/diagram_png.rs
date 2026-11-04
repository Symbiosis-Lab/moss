//! Email and feed pictures of diagrams.
//!
//! A page carries each diagram as inline SVG, but a mail client strips SVG
//! and so do most feed readers. Those surfaces get a hosted 2× PNG instead,
//! drawn from the same wide layout with the page's light colours written in
//! and an opaque page-colour background, so it reads in a dark mail client
//! too. Its URL is content-addressed and append-only, as math's is: mail
//! proxies cache a picture forever, so one URL must never serve two images.
//!
//! What a picture is (its content address, fonts, size and flattened SVG) is
//! decided in `markdown::diagram::picture`; this module draws and ships it.

use std::collections::BTreeSet;
use std::path::Path;

use crate::build::markdown::diagram::picture::{self, size_1x, Fonts, Picture, PAD_PX, RASTER_SCALE};
use crate::build::features::open_tag_attr as attr;
use crate::build::served_path::{ServedPath, DIAGRAM_PNG_PREFIX};
use crate::i18n::Language;

/// The hosted `<img>` for a diagram, or `None` when the diagram gets no
/// picture (the newsletter then keeps its source as code).
pub fn email_diagram_html(source: &str, site_url: &str, lang: Language) -> Option<String> {
    let picture = picture::picture(source, lang)?;
    let (w, h) = size_1x(picture.width, picture.height);
    Some(img_html(&picture.hash, w, h, &picture.alt, site_url))
}

fn img_html(hash: &str, width: u32, height: u32, alt: &str, site_url: &str) -> String {
    format!(
        r#"<img src="{}/{DIAGRAM_PNG_PREFIX}{hash}.png" width="{width}" height="{height}" alt="{alt}" style="max-width:100%;height:auto">"#,
        site_url.trim_end_matches('/')
    )
}

/// The content addresses of the diagrams in `markdown` that get a picture,
/// for a sender to check before mailing that each one is published.
pub fn diagram_png_hashes(markdown: &str) -> Vec<String> {
    diagram_sources(markdown).into_iter().filter_map(|s| picture::picture(&s, Language::En).map(|p| p.hash)).collect()
}

fn diagram_sources(markdown: &str) -> Vec<String> {
    let doc = moss_core::ast::parse(markdown);
    let mut out = Vec::new();
    moss_core::ast::visit_blocks(&doc, |block| {
        if let moss_core::ast::Block::CodeBlock { lang: Some(info), value } = block {
            if moss_core::ast::is_diagram_lang(info) {
                out.push(value.clone());
            }
        }
        true
    });
    out
}

fn rasterize(picture: &Picture) -> Result<Vec<u8>, String> {
    let mut opt = usvg::Options::default();
    opt.fontdb = std::sync::Arc::clone(fontdb(picture.fonts));
    opt.font_family = picture::FONT_FAMILY.to_string();
    // The renderer's own markup, never an author's: a usvg warning here names
    // no moss file. See `cli_output::suppress_renderer_warnings`.
    let _suppress = crate::build::cli_output::suppress_renderer_warnings();
    let tree = usvg::Tree::from_str(&picture.svg, &opt).map_err(|e| format!("diagram svg parse: {e}"))?;
    let (w, h) = size_1x(picture.width, picture.height);
    let mut pixmap = tiny_skia::Pixmap::new(w * RASTER_SCALE, h * RASTER_SCALE).ok_or("pixmap allocation failed")?;
    pixmap.fill(page_colour());
    let scale = RASTER_SCALE as f32;
    let pad = (PAD_PX * RASTER_SCALE) as f32;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale).post_translate(pad, pad), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| format!("png encode: {e}"))
}

/// Exactly the fonts `fonts` names, and nothing from the host.
fn fontdb(fonts: Fonts) -> &'static std::sync::Arc<usvg::fontdb::Database> {
    static INTER: std::sync::OnceLock<std::sync::Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
    static WITH_CJK: std::sync::OnceLock<std::sync::Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
    let build = |cjk: bool| {
        let mut db = usvg::fontdb::Database::new();
        for face in crate::build::page::og_card::inter_faces() {
            db.load_font_data(face.to_vec());
        }
        if let Some(pinned) = cjk.then(crate::build::markdown::math::font::pinned).flatten() {
            let (bytes, _) = pinned.face();
            db.load_font_source(usvg::fontdb::Source::Binary(bytes));
        }
        db.set_sans_serif_family(picture::FONT_FAMILY);
        db.set_monospace_family(picture::FONT_FAMILY);
        std::sync::Arc::new(db)
    };
    match fonts {
        Fonts::Inter => INTER.get_or_init(|| build(false)),
        Fonts::InterAndCjk => WITH_CJK.get_or_init(|| build(true)),
    }
}

/// The light page colour the drawing was made for, as the picture's ground.
fn page_colour() -> tiny_skia::Color {
    let bg = picture::palette().iter().find(|(name, _)| name == "--pele-bg").map(|(_, v)| v.as_str()).unwrap_or("#ffffff");
    let hex = bg.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(0xff);
    tiny_skia::Color::from_rgba8(byte(0), byte(2), byte(4), 0xff)
}

/// Draws every diagram in `documents` that has no picture yet, then keeps
/// every picture already on disk in this build (append-only). A diagram that
/// cannot be drawn has no picture; one that fails to rasterize is skipped
/// with a note and never fails the build.
pub fn emit_diagram_pngs(
    documents: &[crate::build::types::ParsedDocument],
    output_dir: &Path,
    pending: &mut crate::build::manifest::PendingManifest,
) -> Result<(), String> {
    let unique: BTreeSet<String> =
        documents.iter().filter(|doc| doc.features.diagrams).flat_map(|doc| diagram_sources(&doc.content)).collect();
    for source in &unique {
        // Drawing decides the fonts and so the address; a draw is far cheaper
        // than the raster it saves when the file exists.
        let Some(picture) = picture::picture(source, Language::En) else { continue };
        let Ok(served) = ServedPath::for_diagram_png(&picture.hash) else { continue };
        let disk = served.to_disk(output_dir);
        if crate::build::io_utils::output_present(&disk) {
            continue; // content-addressed: same hash, same bytes
        }
        match rasterize(&picture) {
            Ok(png) => super::hosted_png::write_atomic(&disk, &png)?,
            // The page's figure names this picture, so a feed would point at a missing image.
            Err(e) => log::warn!("diagram picture {} not drawn: {e}", picture.hash),
        }
    }
    super::hosted_png::retain(output_dir, DIAGRAM_PNG_PREFIX, ServedPath::for_diagram_png, pending)
}

/// Swaps each drawn diagram in rendered page HTML for its hosted picture,
/// for a feed. A figure that could not be drawn carries no `data-diagram`
/// and keeps its source and reason.
pub fn swap_diagram_figures_for_feed_imgs(html: &str, site_url: &str) -> String {
    const OPEN: &str = "<figure class=\"moss-diagram\"";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        let figure_and_rest = &rest[at..];
        // The allowlist admits no <figure> inside a drawing, so the first close is this one's.
        let Some(end) = figure_and_rest.find("</figure>").map(|e| e + "</figure>".len()) else {
            out.push_str(figure_and_rest);
            return out;
        };
        let figure = &figure_and_rest[..end];
        let open = &figure[..figure.find('>').unwrap_or(0)];
        let swapped = attr(open, "data-diagram").and_then(|hash| {
            let svg = &figure[figure.find("<svg")?..];
            let svg_open = &svg[..svg.find('>')?];
            let width: f64 = attr(svg_open, "width")?.parse().ok()?;
            let height: f64 = attr(svg_open, "height")?.parse().ok()?;
            let (w, h) = size_1x(width, height);
            Some(format!("<p>{}</p>", img_html(hash, w, h, &picture::alt_text(svg), site_url)))
        });
        match swapped {
            Some(img) => out.push_str(&img),
            None => out.push_str(&without_narrow(figure)),
        }
        rest = &figure_and_rest[end..];
    }
    out.push_str(rest);
    out
}

/// A figure with no picture, as a feed carries it: the wide layout only. A
/// feed reader has no container query to choose a layout, so it would show
/// both.
fn without_narrow(figure: &str) -> String {
    const OPEN: &str = "<div class=\"moss-diagram-narrow\"";
    match figure.find(OPEN) {
        // The allowlist admits no <div> inside a drawing, so the next close is the wrapper's.
        Some(at) => match figure[at..].find("</div>") {
            Some(close) => format!("{}{}", &figure[..at], &figure[at + close + "</div>".len()..]),
            None => figure.to_string(),
        },
        None => figure.to_string(),
    }
}

#[cfg(test)]
#[path = "diagram_png_tests.rs"]
mod tests;
