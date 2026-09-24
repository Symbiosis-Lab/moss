//! Feature stylesheets: `comments.css`, `email.css`, `review.css`.
//!
//! These are the sheets whose gate is not a parse-time page fact — comments
//! need synced social JSON, review needs `article-map.json`, email needs the
//! installed-channels config — so they cannot join `emit/stylesheet.rs`'s
//! `CSS_PARTIALS`, which is folded from `PageFeatures` inside
//! `generate_blocking_content`. They resolve one phase later, in
//! `features::generate_native_slots`.
//!
//! ## Why they became files
//!
//! Until 2026-08-04 each was `include_str!`-ed and injected as an inline
//! `<style>` block into `Slot::HeadEnd` — which is *site-wide*, meaning the
//! bytes were replicated into every page of the site:
//!
//! | sheet | bytes | on a 215-page site |
//! |---|---|---|
//! | `comments.css` | 18,314 | 3.9 MB |
//! | `email.css` | 6,808 | 1.5 MB |
//! | `review.css` | 3,964 | 0.9 MB |
//!
//! Inline bytes are also uncacheable: a reader who visits ten pages downloads
//! all ten copies. Emitting one content-hashed file and linking it makes the
//! second page load free, and the win scales with the site rather than being
//! a fixed saving.
//!
//! The cost is one request on the first page. That is the right trade here
//! because these sheets are large and site-wide — the same reasoning does not
//! transfer to a small per-page block, and this module deliberately holds only
//! the three that clear the bar.
//!
//! ## Cascade
//!
//! Each sheet is emitted wrapped in `@layer plugins`, the layer the inline
//! blocks were given by `enhance::wrap_style_blocks_in_layer`. Layer order is
//! declared once, at the top of the main stylesheet
//! (`@layer reset, tokens, base, layout, shortcodes, plugins, themes;`), which
//! is linked earlier in `<head>` — so a layer's precedence does not depend on
//! where its rules load. That property is exactly what makes moving these from
//! inline `<style>` at head-end to a `<link>` a safe move rather than a
//! cascade change.

use crate::build::assets::paths::compute_content_hash;
use crate::build::render::html::load_js_asset;
use crate::build::served_path::ServedPath;

/// One feature stylesheet and the disk path it reloads from in dev.
pub struct FeatureStyle {
    /// Basename, without `.css`. Becomes `_moss/css/<name>.<hash>.css`.
    pub name: &'static str,
    /// Path under the desktop app's source tree for dev hot-reload.
    pub dev_path: &'static str,
    /// The file's bytes, `include_str!`-ed at compile time.
    pub source: &'static str,
}

/// Every feature stylesheet, keyed by the name callers pass to [`link_tag`].
///
/// A registration table in the NORTH-STAR pattern-glossary #2 sense, and the
/// sibling of `emit::stylesheet::CSS_PARTIALS` — same shape, different phase.
/// The totality test is `feature_styles_tests::every_feature_css_file_has_a_row`.
pub const FEATURE_STYLES: &[FeatureStyle] = &[
    FeatureStyle {
        name: "comments",
        dev_path: "src/assets/css/comments.css",
        source: include_str!("../../assets/css/comments.css"),
    },
    FeatureStyle {
        name: "email",
        dev_path: "src/assets/css/email.css",
        source: include_str!("../../assets/css/email.css"),
    },
    FeatureStyle {
        name: "review",
        dev_path: "src/assets/css/review.css",
        source: include_str!("../../assets/css/review.css"),
    },
];

/// Look up a registered sheet by name.
fn find(name: &str) -> &'static FeatureStyle {
    FEATURE_STYLES
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no feature stylesheet named '{name}' in FEATURE_STYLES"))
}

/// The exact bytes emitted for a sheet: its source inside `@layer plugins`.
///
/// One function so the hash in [`link_tag`] and the bytes in [`emit`] cannot
/// drift — a content-addressed filename computed over different bytes than the
/// file receives is a 404 on every page.
fn wrapped(style: &FeatureStyle) -> String {
    let source = load_js_asset(style.dev_path, style.source);
    format!("@layer plugins {{\n{source}\n}}\n")
}

/// The `<link>` tag for a sheet, with its content-addressed URL.
///
/// Callable without a build context: the hash is a function of embedded bytes
/// alone, so slot resolution can name the file before anything writes it.
pub fn link_tag(name: &str) -> String {
    let style = find(name);
    let hash = compute_content_hash(&wrapped(style));
    let url = ServedPath::for_feature_stylesheet_hashed(style.name, &hash)
        .expect("FEATURE_STYLES names are valid")
        .to_relative_url();
    format!("<link rel=\"stylesheet\" href=\"{url}\">")
}

/// Every registered sheet's content hash, in table order.
///
/// Folded into `FacadeCache::asset_versions` so a sheet whose bytes move
/// invalidates the Stage 5b render skip. Without it, a `ContentOnly` watch
/// rebuild in dev — where [`load_js_asset`] re-reads `comments.css` from disk —
/// deletes the old file via `remove_stale_files` while every skipped page keeps
/// the old `<link href>`, and the whole comment UI loses its styling.
///
/// Ungated on purpose, like `ScriptAssets::all_hashes`: the point is to notice
/// a filename moving, and a feature turning ON must not be the first build that
/// learns the hash.
pub fn all_hashes() -> Vec<String> {
    FEATURE_STYLES
        .iter()
        .map(|s| compute_content_hash(&wrapped(s)))
        .collect()
}

/// Write the named sheets into the build output.
///
/// Only the sheets whose feature actually resolved are passed in — that is the
/// whole point of the module. Emitting one the site does not link would leave
/// an orphan file; linking one this never emits would 404 every page, which is
/// why both sides go through [`wrapped`].
pub fn emit(
    names: &[&'static str],
    output_dir: &std::path::Path,
    pending: &mut crate::build::manifest::PendingManifest,
) -> Result<(), String> {
    use crate::build::context::BuildContext;
    use crate::build::manifest::HashBucket;
    for name in names {
        let style = find(name);
        let bytes = wrapped(style);
        let hash = compute_content_hash(&bytes);
        BuildContext::for_render(output_dir, pending)
            .emit(
                &ServedPath::for_feature_stylesheet_hashed(style.name, &hash)
                    .expect("FEATURE_STYLES names are valid"),
                bytes.as_bytes(),
                HashBucket::Files,
            )
            .map_err(|e| format!("Failed to emit _moss/css/{}.<hash>.css: {e}", style.name))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "feature_styles_tests.rs"]
mod tests;
