//! The site's design written where moss reads it: the token overrides in
//! `.moss/theme/style.css`, the font files they name in `.moss/theme/fonts/`,
//! and `color_scheme = "light"` for a source with no dark scheme. The
//! stylesheet is written only when absent or free of `--moss-*` overrides, the
//! setting only when absent: a re-run never overwrites what the author shaped.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use scraper::Html;
use url::Url;

use super::super::crawl_state::HostPacer;
use super::super::crawler::host_of;
use super::super::fetch::{fetch_raw, fetch_raw_bytes, hash_url, linked_filename, observe_pace};
use crate::build::theme_lint::{strip_comments, PROP_DECL};
use super::facts::sel;
use super::mapping::{self, Overrides};
use super::style::{self, FontFace, Sheet};
use super::{Capture, ChromePart, ChromeSummary};

pub(super) async fn apply(capture: &Capture<'_>, pacer: &mut HostPacer, summary: &mut ChromeSummary) -> Result<(), String> {
    let mut notes = Vec::new();
    let sheets = fetch_sheets(&capture.home.html, &capture.home.url, capture.user_agent, pacer, &mut notes).await;
    let facts = style::read(&capture.home.html, capture.post, &sheets);
    let overrides = mapping::map(&facts, &moss_core::contract::tokens::load_tokens()?);
    summary.residuals = notes;
    summary.residuals.extend(overrides.residuals.iter().cloned());
    if overrides.is_empty() {
        return Ok(());
    }
    let theme_dir = capture.out_dir.join(".moss").join("theme");
    let style_path = theme_dir.join("style.css");
    // allow:raw_read the folder the import writes into, read once to keep the author's own stylesheet
    let existing = fs::read_to_string(&style_path).ok();
    if existing.as_deref().is_some_and(overrides_a_token) {
        summary.theme = ChromePart::LeftUnchanged;
        return Ok(());
    }
    let (faces, fonts) = copy_fonts(&facts.font_faces, &overrides.families, &theme_dir, capture.user_agent, pacer, MAX_FONT_BYTES, &mut summary.residuals).await;
    let css = match existing.as_deref().map(str::trim_end).filter(|c| !c.is_empty()) {
        Some(own) => format!("{own}\n\n{faces}{}", overrides.css()),
        None => format!("{faces}{}", overrides.css()),
    };
    fs::create_dir_all(&theme_dir).map_err(|e| format!("Failed to create {}: {e}", theme_dir.display()))?;
    // allow:raw_write the author's own source folder, not build output: the theme stylesheet
    fs::write(&style_path, css).map_err(|e| format!("Failed to write {}: {e}", style_path.display()))?;
    if facts.dark.is_none() {
        light_scheme_unless_set(capture.out_dir)?;
    }
    summary.theme = ChromePart::Written;
    summary.tokens = token_count(&overrides);
    summary.fonts = fonts;
    Ok(())
}

fn token_count(overrides: &Overrides) -> usize {
    overrides.light.iter().chain(&overrides.dark).map(|(t, _)| t).collect::<HashSet<_>>().len()
}

/// Whether a stylesheet declares a `--moss-*` property of its own, outside comments.
fn overrides_a_token(css: &str) -> bool {
    PROP_DECL.is_match(&strip_comments(css))
}

/// Every stylesheet of the home page in document order: each `<style>` as
/// written, each linked one fetched. A sheet that cannot be fetched, and one
/// a sheet `@import`s (not followed), leave the read incomplete; `notes` names
/// each.
async fn fetch_sheets(html: &str, page_url: &str, user_agent: &str, pacer: &mut HostPacer, notes: &mut Vec<String>) -> Vec<Sheet> {
    let Ok(base) = Url::parse(page_url) else { return Vec::new() };
    let found: Vec<Result<String, Url>> = {
        let doc = Html::parse_document(html);
        doc.select(&sel("style, link[rel~=stylesheet][href]"))
            .filter(|e| e.value().attr("media").is_none_or(|m| !m.trim().eq_ignore_ascii_case("print")))
            .filter_map(|e| match e.value().name() {
                "style" => Some(Ok(e.text().collect())),
                _ => e.value().attr("href").and_then(|h| base.join(h.trim()).ok()).filter(|u| matches!(u.scheme(), "http" | "https")).map(Err),
            })
            .collect()
    };
    let mut sheets = Vec::new();
    for item in found {
        match item {
            Ok(css) => sheets.push(Sheet { css, base: base.clone() }),
            Err(url) => {
                let host = host_of(url.as_str());
                pacer.wait(&host).await;
                let fetched = fetch_raw(url.as_str(), user_agent).await;
                observe_pace(pacer, &host, &fetched);
                match fetched {
                    Ok(css) => sheets.push(Sheet { css, base: url }),
                    Err(e) if e.is_refused() => notes.push(format!("stylesheet {url}: address not allowed")),
                    Err(e) => notes.push(format!("stylesheet {url} could not be read ({e}), so what it styles is missing")),
                }
            }
        }
    }
    for sheet in &sheets {
        for imported in IMPORT.captures_iter(&strip_comments(&sheet.css)) {
            notes.push(format!("stylesheet {} imports {}, which is not read", sheet.base, &imported[1]));
        }
    }
    sheets
}

/// An `@import` statement, capturing what it names.
static IMPORT: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r#"@import\s+(?:url\()?\s*["']?([^"')\s;]+)"#).unwrap());

/// The most font bytes one import copies. A family that would pass it is not
/// copied at all: a variable face and its subsets run to a few hundred KB, so
/// this holds the faces of several families, not a font service's catalogue.
const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;

/// Fetch each face of the families a font token names into `fonts/`, and the
/// `@font-face` rules that serve them from there, within `max_bytes` in all.
/// A file of the same name already in `fonts/` is used as it is, never
/// overwritten. Returns the rules and the families copied.
async fn copy_fonts(
    faces: &[FontFace],
    families: &[String],
    theme_dir: &Path,
    user_agent: &str,
    pacer: &mut HostPacer,
    max_bytes: usize,
    notes: &mut Vec<String>,
) -> (String, Vec<String>) {
    let dir = theme_dir.join("fonts");
    let (mut css, mut copied, mut spent) = (String::new(), Vec::<String>::new(), 0usize);
    // Source address -> the file serving it.
    let mut files: HashMap<String, String> = HashMap::new();
    for family in families {
        if copied.iter().any(|c| c.eq_ignore_ascii_case(family)) {
            continue;
        }
        // Each face with its file, and the bytes to write (none when the file is already there).
        let mut found: Vec<(&FontFace, String, Option<Vec<u8>>)> = Vec::new();
        for face in faces.iter().filter(|f| f.family.eq_ignore_ascii_case(family)) {
            if let Some(file) = files.get(&face.source) {
                found.push((face, file.clone(), None));
                continue;
            }
            let scheme = Url::parse(&face.source).map(|u| u.scheme().to_string()).unwrap_or_default();
            if !matches!(scheme.as_str(), "http" | "https") {
                notes.push(format!("font {} is served as a {scheme}: address, which is not copied", face.family));
                continue;
            }
            let name = linked_filename(&face.source);
            let taken = files.values().any(|f| *f == name) || found.iter().any(|(_, f, _)| *f == name);
            let file = if taken { format!("{}.{}", hash_url(&face.source), name.rsplit('.').next().unwrap_or("woff2")) } else { name };
            if dir.join(&file).exists() {
                found.push((face, file, None));
                continue;
            }
            let host = host_of(&face.source);
            pacer.wait(&host).await;
            let fetched = fetch_raw_bytes(&face.source, user_agent).await;
            observe_pace(pacer, &host, &fetched);
            match fetched {
                Ok(bytes) if is_font(&bytes) => found.push((face, file, Some(bytes))),
                Ok(_) => notes.push(format!("font {} at {} is not a font file (WOFF2, WOFF, TrueType or OpenType), so it is not copied", face.family, face.source)),
                Err(e) if e.is_refused() => notes.push(format!("font {} at {}: address not allowed", face.family, face.source)),
                Err(e) => notes.push(format!("font {} at {} could not be fetched ({e}), so it is not copied", face.family, face.source)),
            }
        }
        let size: usize = found.iter().filter_map(|(_, _, bytes)| bytes.as_ref().map(Vec::len)).sum();
        if spent + size > max_bytes {
            notes.push(format!("font {family} ({} KB) would pass the {} MB of fonts one import copies; not copied", size / 1024, max_bytes / (1024 * 1024)));
            continue;
        }
        spent += size;
        for (face, file, bytes) in found {
            if let Some(bytes) = bytes {
                // allow:raw_write the author's own source folder, not build output: a font the theme names
                if let Err(e) = fs::create_dir_all(&dir).and_then(|()| fs::write(dir.join(&file), bytes)) {
                    log::warn!("import: font {} not written: {e}", face.source);
                    continue;
                }
            }
            files.insert(face.source.clone(), file.clone());
            css.push_str(&face_rule(face, &file));
            if !copied.contains(&face.family) {
                copied.push(face.family.clone());
            }
        }
    }
    (css, copied)
}

/// The `@font-face` rule serving `face` from `fonts/<file>`, with only plain
/// values of the descriptors and formats moss carries.
fn face_rule(face: &FontFace, file: &str) -> String {
    let format = face.format.as_deref().filter(|f| FORMATS.contains(f)).map(|f| format!(" format(\"{f}\")")).unwrap_or_default();
    let descriptors: String = face
        .descriptors
        .iter()
        .filter(|(p, v)| DESCRIPTORS.contains(&p.as_str()) && v.chars().all(|c| c.is_ascii_alphanumeric() || " ,+%.?-".contains(c)))
        .map(|(p, v)| format!("  {p}: {v};\n"))
        .collect();
    format!("@font-face {{\n  font-family: \"{}\";\n  src: url(fonts/{file}){format};\n{descriptors}}}\n", face.family)
}

/// The `@font-face` descriptors carried over; values are checked to be plain.
const DESCRIPTORS: &[&str] = &["font-style", "font-weight", "font-stretch", "font-display", "unicode-range"];
const FORMATS: &[&str] = &["woff2", "woff", "truetype", "opentype"];

/// Whether the bytes open as a font file: an error page served with a 200 is not one.
fn is_font(bytes: &[u8]) -> bool {
    [b"wOF2".as_slice(), b"wOFF", b"OTTO", b"true", &[0, 1, 0, 0]].iter().any(|magic| bytes.starts_with(magic))
}

/// `[site] color_scheme = "light"`, unless the folder already says.
fn light_scheme_unless_set(out_dir: &Path) -> Result<(), String> {
    let config = crate::vault::config::load_managed_toml(&out_dir.join(".moss").join("config.toml"))?;
    if config.root.get("site").and_then(|s| s.get("color_scheme")).is_some() {
        return Ok(());
    }
    crate::vault::config::save_site_str(&out_dir.to_string_lossy(), "color_scheme", "light")
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
