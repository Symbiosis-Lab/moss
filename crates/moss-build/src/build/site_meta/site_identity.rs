//! Two files that describe the site to whatever reads it besides a browser
//! tab: `manifest.webmanifest` (the W3C web app manifest: its name, language,
//! colours and, when it has its own, its icon) and `_moss/tokens.json` (its
//! colour and font tokens as the page resolves them, light and dark, in the
//! format and under the names of moss's own token registry). Home-screen
//! shortcuts, link previews and any other software that shows the site's look
//! read the same facts, so moss writes them once, as data.

use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::page::color_scheme::{ColorScheme, ThemeColor};
use crate::build::served_path::ServedPath;
use crate::build::theme_lint::Layered;
use serde_json::{json, Map, Value};
use std::path::Path;

/// Raised when a reader of `_moss/tokens.json` would misread the new shape.
pub const TOKENS_VERSION: u32 = 1;

/// The site's own icon: its file under `assets/` and the PNG sizes made from it.
#[derive(Clone, Copy)]
pub struct OwnIcon<'a> {
    pub file: &'a str,
    pub raster_sizes: &'a [u32],
}

/// What the manifest says about the site.
pub struct Identity<'a> {
    pub name: &'a str,
    pub lang_tag: &'a str,
    pub theme_color: &'a ThemeColor,
    pub color_scheme: ColorScheme,
    /// `None` when the site uses moss's own mark, which is not the site's icon.
    pub own_icon: Option<OwnIcon<'a>>,
}

/// The manifest. Its icon paths are relative to it.
pub fn manifest(id: &Identity) -> Value {
    let mut icons = Vec::new();
    if let Some(icon) = id.own_icon {
        for size in icon.raster_sizes.iter().filter(|s| **s >= 144) {
            icons.push(json!({ "src": ServedPath::for_favicon_raster(*size).as_str(), "sizes": format!("{size}x{size}"), "type": "image/png" }));
        }
        if icon.file.ends_with(".svg") {
            icons.push(json!({ "src": format!("assets/{}", icon.file), "sizes": "any", "type": "image/svg+xml" }));
        }
    }
    let paper = if id.color_scheme == ColorScheme::Dark { &id.theme_color.dark } else { &id.theme_color.light };
    let mut m = json!({
        "name": id.name,
        "lang": id.lang_tag,
        "start_url": "./",
        "display": "browser",
        "background_color": paper,
        "theme_color": paper,
    });
    if !icons.is_empty() {
        m["icons"] = Value::Array(icons);
    }
    m
}

/// The colour and font tokens as a page in `lang_tag` shows them at its root.
/// A site fixed to one scheme shows only that side, so it fills both. A token
/// whose value the build cannot reduce to a plain value (`color-mix()`, a
/// translucent colour, a `var()` it cannot follow) is left out, never guessed.
pub fn tokens(registry: &moss_core::contract::tokens::Tokens, theme_css: Option<&str>, lang_tag: &str, scheme: ColorScheme) -> Value {
    let layered = Layered::for_page(theme_css, lang_tag);
    let mut out = Map::new();
    out.insert("$description".into(), "This site's colour and font tokens as its pages resolve them; names and format follow moss's token registry.".into());
    out.insert("$extensions".into(), json!({ "moss": { "version": TOKENS_VERSION } }));
    for group in &registry.groups {
        let mut entries = Map::new();
        for entry in &group.entries {
            let kind = entry.type_hint.as_deref().unwrap_or_default();
            let (light, dark) = layered.values(&format!("--{}", entry.name));
            let value = match kind {
                "color" => {
                    let (Some(l), Some(d)) = (light.as_deref().and_then(opaque_hex), dark.as_deref().and_then(opaque_hex)) else { continue };
                    let (l, d) = match scheme {
                        ColorScheme::Auto => (l, d),
                        ColorScheme::Light => (l.clone(), l),
                        ColorScheme::Dark => (d.clone(), d),
                    };
                    json!({ "light": l, "dark": d })
                }
                "fontFamily" => match light {
                    Some(stack) if !stack.contains("var(") => json!(stack),
                    _ => continue,
                },
                _ => continue,
            };
            entries.insert(entry.name.clone(), json!({ "$type": kind, "$value": value }));
        }
        if !entries.is_empty() {
            out.insert(group.name.clone(), Value::Object(entries));
        }
    }
    Value::Object(out)
}

/// A plain opaque colour as `#rrggbb`; `None` for anything translucent or unreadable.
fn opaque_hex(value: &str) -> Option<String> {
    use cssparser::{Color, Parser, ParserInput};
    let mut input = ParserInput::new(value.trim());
    let mut parser = Parser::new(&mut input);
    let colour = Color::parse(&mut parser).ok()?;
    parser.expect_exhausted().ok()?;
    let opaque = |alpha: Option<f32>| alpha.unwrap_or(1.0) >= 1.0;
    let rgb = match colour {
        Color::Rgba(c) if opaque(c.alpha) => (c.red?, c.green?, c.blue?),
        Color::Hsl(c) if opaque(c.alpha) => {
            let (r, g, b) = cssparser::hsl_to_rgb(c.hue?.rem_euclid(360.0) / 360.0, c.saturation?, c.lightness?);
            let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
            (byte(r), byte(g), byte(b))
        }
        _ => return None,
    };
    Some(crate::build::scan::section_text::to_hex(rgb))
}

/// Write both files.
pub fn emit(
    layout: &crate::build::page::layout::LayoutConfig,
    own_icon: Option<OwnIcon>,
    theme_css: Option<&str>,
    output_dir: &Path,
    pending: &mut PendingManifest,
) -> Result<(), String> {
    let id = Identity { name: &layout.site_name, lang_tag: &layout.lang_tag, theme_color: &layout.theme_color, color_scheme: layout.color_scheme, own_icon };
    let registry = moss_core::contract::tokens::load_tokens()?;
    for (served, body) in [
        (ServedPath::for_web_manifest(), manifest(&id)),
        (ServedPath::for_site_tokens(), tokens(&registry, theme_css, &layout.lang_tag, layout.color_scheme)),
    ] {
        let bytes = serde_json::to_vec_pretty(&body).map_err(|e| e.to_string())?;
        BuildContext::for_render(output_dir, pending)
            .emit_held(&served, bytes, HashBucket::Files)
            .map_err(|e| format!("Failed to emit {}: {}", served.as_str(), e))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "site_identity_tests.rs"]
mod tests;
