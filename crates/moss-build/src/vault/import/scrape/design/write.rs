//! Put a home page's chrome facts where moss looks for them, each only where
//! the folder does not already have its own.
//!
//! - the site name becomes the home note's `title:` (moss takes the site's
//!   title and brand from there); the title it replaces moves to `description:`
//!   when the note has none
//! - the navigation is `nav: true` plus a `weight:` on the pages it links to,
//!   which is how moss builds its nav bar and orders it
//! - the logo is the home note's `logo:`, the footer is `footer.md` at the
//!   site root, the icon is `assets/favicon.<ext>`

use std::fs;
use std::path::Path;

use super::super::crawl_state::HostPacer;
use crate::build::components::nav::is_navigational_page;
use super::super::crawler::host_of;
use super::super::fetch::{download_asset, fetch_raw_bytes, observe_pace};
use super::super::finalize::{with_fields_absent, with_title};
use super::super::run::{write_note, ASSETS_SUBDIR};
use super::super::service::{escape_yaml_string, rewrite_links};
use super::facts::{is_generic_title, norm_url, Facts, Icon, IconKind};
use super::{Capture, ChromePart, ChromeSummary};

/// Image extensions a downloaded logo may carry; anything else (`bin`, from a
/// response that named no image type) is not a logo moss can show.
const LOGO_EXTENSIONS: &[&str] = &["svg", "png", "jpg", "gif", "webp"];

pub(super) async fn apply(
    capture: &Capture<'_>,
    facts: &Facts,
    pacer: &mut HostPacer,
) -> Result<ChromeSummary, String> {
    let mut summary = ChromeSummary { ran: true, ..Default::default() };
    let home_key = norm_url(&capture.home.url);
    let home_note = home_key.as_ref().and_then(|k| capture.written.get(k));

    write_nav(capture, facts, home_note.map(String::as_str), &mut summary)?;
    if let (true, Some(rel)) = (capture.home.is_start, home_note) {
        write_home_note(capture, facts, rel, pacer, &mut summary).await?;
    }
    write_footer(capture, facts, &mut summary)?;
    write_favicon(capture, facts, pacer, &mut summary).await;
    Ok(summary)
}

fn read_note(out_dir: &Path, rel: &str) -> Result<String, String> {
    // allow:raw_read the importer's own output, written moments ago in this run — not a cloud-evictable vault input
    fs::read_to_string(out_dir.join(rel)).map_err(|e| format!("Failed to read {rel}: {e}"))
}

fn rewrite_note(out_dir: &Path, rel: &str, content: &str) -> Result<(), String> {
    // allow:raw_write the importer rewrites a note it wrote moments ago in this run, adding the site chrome only the finished crawl can place
    fs::write(out_dir.join(rel), content).map_err(|e| format!("Failed to write {rel}: {e}"))
}

fn has_key(content: &str, key: &str) -> bool {
    moss_core::frontmatter::frontmatter_map(content).contains_key(key)
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", escape_yaml_string(value))
}

/// `nav: true` and an ascending `weight:` on every written page the home
/// page's primary nav links to, in the nav's own order. Fewer than two such
/// pages is not a nav bar.
fn write_nav(
    capture: &Capture<'_>,
    facts: &Facts,
    home_note: Option<&str>,
    summary: &mut ChromeSummary,
) -> Result<(), String> {
    let mut targets: Vec<&str> = Vec::new();
    for entry in &facts.nav {
        let Some(rel) = norm_url(&entry.url).and_then(|k| capture.written.get(&k)) else {
            continue;
        };
        if Some(rel.as_str()) != home_note && !targets.contains(&rel.as_str()) {
            targets.push(rel);
        }
    }
    if targets.len() < 2 {
        return Ok(());
    }
    for rel in &targets {
        let content = read_note(capture.out_dir, rel)?;
        // An author's `nav` or `weight` stands: writing the other half over it
        // would break the order they chose.
        if has_key(&content, "nav") || has_key(&content, "weight") {
            continue;
        }
        let fields = [("nav", "true".to_string()), ("weight", (summary.nav_items + 1).to_string())];
        if let Some(next) = with_fields_absent(&content, &fields) {
            rewrite_note(capture.out_dir, rel, &next)?;
            summary.nav_items += 1;
        }
    }
    summary.nav = if summary.nav_items > 0 { ChromePart::Written } else { ChromePart::LeftUnchanged };
    if summary.nav_items > 0 {
        opt_out_of_auto_nav(capture, &targets, home_note)?;
    }
    Ok(())
}

/// moss puts every root-level page of a site with content folders in its nav
/// bar unless the page says otherwise, so the pages the home page's menu does
/// not show are written `nav: false`, or the rendered menu would be longer than
/// the original. The candidate test is moss's own.
fn opt_out_of_auto_nav(
    capture: &Capture<'_>,
    targets: &[&str],
    home_note: Option<&str>,
) -> Result<(), String> {
    let has_content_folders = capture.written.values().any(|rel| rel.contains('/'));
    for rel in capture.written.values() {
        let stem = Path::new(rel).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let is_home = Some(rel.as_str()) == home_note;
        let auto = is_navigational_page(!rel.contains('/'), is_home, stem, has_content_folders);
        if !auto || targets.contains(&rel.as_str()) {
            continue;
        }
        let content = read_note(capture.out_dir, rel)?;
        if let Some(next) = with_fields_absent(&content, &[("nav", "false".to_string())]) {
            rewrite_note(capture.out_dir, rel, &next)?;
        }
    }
    Ok(())
}

/// Site name as `title:`, the title it replaces as `description:` (only when
/// absent), and the logo as `logo:` (only when absent), in one rewrite.
async fn write_home_note(
    capture: &Capture<'_>,
    facts: &Facts,
    rel: &str,
    pacer: &mut HostPacer,
    summary: &mut ChromeSummary,
) -> Result<(), String> {
    let original = read_note(capture.out_dir, rel)?;
    let mut content = original.clone();
    let mut fields: Vec<(&str, String)> = Vec::new();

    let former = moss_core::frontmatter::frontmatter_map(&original)
        .get("title")
        .and_then(moss_core::frontmatter::value_as_string)
        .map(|t| t.trim().to_string())
        .unwrap_or_default();
    if let Some(name) = facts.site_name.as_deref().filter(|n| !n.is_empty() && *n != former) {
        if let Some(next) = with_title(&content, name) {
            content = next;
            if !is_generic_title(&former, name) {
                fields.push(("description", quoted(&former)));
            }
        }
    }

    if has_key(&original, "logo") {
        if facts.logo.is_some() {
            summary.logo = ChromePart::LeftUnchanged;
        }
    } else if let Some(url) = &facts.logo {
        if let Some(file) = download_logo(url, capture, pacer).await {
            fields.push(("logo", quoted(&format!("./{ASSETS_SUBDIR}/{file}"))));
            summary.logo = ChromePart::Written;
        }
    }

    if let Some(next) = with_fields_absent(&content, &fields) {
        content = next;
    }
    if content != original {
        rewrite_note(capture.out_dir, rel, &content)?;
    }
    Ok(())
}

async fn download_logo(url: &str, capture: &Capture<'_>, pacer: &mut HostPacer) -> Option<String> {
    let host = host_of(url);
    pacer.wait(&host).await;
    let result = download_asset(url, &capture.out_dir.join(ASSETS_SUBDIR), capture.user_agent).await;
    observe_pace(pacer, &host, &result);
    match result {
        Ok(file) if LOGO_EXTENSIONS.contains(&file.rsplit('.').next().unwrap_or("")) => Some(file),
        Ok(file) => {
            log::warn!("import: logo {url} came back as {file}, not an image type; skipped");
            None
        }
        Err(e) => {
            log::warn!("import: logo download failed: {url}: {e}");
            None
        }
    }
}

fn write_footer(capture: &Capture<'_>, facts: &Facts, summary: &mut ChromeSummary) -> Result<(), String> {
    let Some(markdown) = &facts.footer else { return Ok(()) };
    if capture.out_dir.join("footer.md").exists() {
        summary.footer = ChromePart::LeftUnchanged;
        return Ok(());
    }
    let markdown = rewrite_links(markdown, capture.scope);
    if markdown.trim().is_empty() {
        return Ok(());
    }
    write_note(capture.out_dir, "footer.md", &markdown)?;
    summary.footer = ChromePart::Written;
    Ok(())
}

fn has_favicon(out_dir: &Path) -> bool {
    fs::read_dir(out_dir.join("assets")).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.path().file_stem().is_some_and(|s| s == "favicon"))
    })
}

/// What the bytes are, whatever the link declared: a 404 page served with a
/// 200, or a stray HTML shell, is none of the three and not an icon.
fn sniff(bytes: &[u8]) -> Option<IconKind> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some(IconKind::Png);
    }
    if bytes.starts_with(&[0, 0, 1, 0]) {
        return Some(IconKind::Ico);
    }
    // `<svg` must open the document: only a BOM, whitespace, comments, an XML
    // prolog and a doctype may come first, all within the first KB.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    let mut rest = head.trim_start_matches('\u{FEFF}').trim_start();
    while rest.starts_with("<?") || rest.starts_with("<!") {
        rest = rest.split_once('>')?.1.trim_start();
    }
    rest.starts_with("<svg").then_some(IconKind::Svg)
}

async fn write_favicon(
    capture: &Capture<'_>,
    facts: &Facts,
    pacer: &mut HostPacer,
    summary: &mut ChromeSummary,
) {
    let Some(Icon { url, .. }) = &facts.favicon else { return };
    if has_favicon(capture.out_dir) {
        summary.favicon = ChromePart::LeftUnchanged;
        return;
    }
    let host = host_of(url);
    pacer.wait(&host).await;
    let (bytes, kind) = match fetch_raw_bytes(url, capture.user_agent).await {
        Ok(b) => match sniff(&b) {
            Some(kind) => (b, kind),
            None => {
                log::warn!("import: favicon {url} is not an SVG, PNG or ICO file; skipped");
                return;
            }
        },
        Err(e) => {
            log::warn!("import: favicon download failed: {url}: {e}");
            return;
        }
    };
    let dir = capture.out_dir.join("assets");
    // allow:raw_write the author's own source content, not `.moss/build.nosync/` output — the site's icon file
    let written = fs::create_dir_all(&dir)
        .and_then(|()| fs::write(dir.join(format!("favicon.{}", kind.extension())), bytes));
    match written {
        Ok(()) => summary.favicon = ChromePart::Written,
        Err(e) => log::warn!("import: favicon write failed: {e}"),
    }
}

#[cfg(test)]
#[path = "write_tests.rs"]
mod tests;
