//! A shared JSON boundary for hosts that check and preview pages without a build.

use moss_core::page::{self, SiteBundle, SiteInputs};
use moss_core::schema;
use moss_core::validation::{Diagnostic, EditorDiagnostic};
use serde::Serialize;

#[cfg(feature = "wasm")]
pub mod wasm;
#[cfg(feature = "ffi")]
pub mod ffi;
#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// Host-supplied site values, with the effective schema composed once on creation.
pub struct Site {
    inputs: SiteInputs,
    css: String,
    bundle: SiteBundle,
    pages: page::site_page::SitePages,
    /// Every page's source by path, as `set_texts` received it; a listing's
    /// cards read their excerpts and pictures from it.
    texts: std::collections::HashMap<String, String>,
}

/// moss's page stylesheet, kept equal to the build's by a moss-build test.
const PAGE_CSS: &str = include_str!("../assets/page.css");

impl Site {
    pub fn from_json(site_json: &str) -> Result<Self, String> {
        let bundle: SiteBundle = serde_json::from_str(site_json).map_err(|e| e.to_string())?;
        let css = format!("{PAGE_CSS}\n{}", bundle.theme_css);
        Ok(Self { inputs: bundle.clone().inputs(), css, bundle, pages: Default::default(), texts: Default::default() })
    }

    /// Replace every page's text, by site-relative path, and derive the site's
    /// pages from them once. `root_name` is the site folder's own name.
    pub fn set_texts(&mut self, texts_json: &str, root_name: &str) -> Result<(), String> {
        self.texts = serde_json::from_str(texts_json).map_err(|e| e.to_string())?;
        self.pages = page::site_page::site_pages(&self.texts, &self.bundle, root_name);
        Ok(())
    }

    /// The effective schema used by this site's checks.
    pub fn schema_json(&self) -> String {
        serde_json::to_string(&self.inputs.schema).expect("schema is JSON-compatible")
    }

    /// Replace all notes; invalid input keeps the old map.
    pub fn set_notes(&mut self, notes_json: &str) -> Result<(), String> {
        self.inputs.notes = serde_json::from_str(notes_json).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Diagnostics use the editor's LSP severity numbers and one-based source positions.
    pub fn check(&self, path: &str, text: &str) -> String {
        serde_json::to_string(&diagnostics(page::check_page(path, text, &self.inputs)))
            .expect("diagnostics contain only JSON-compatible values")
    }

    pub fn embedded_notes(&self, path: &str, text: &str) -> String {
        serde_json::to_string(&page::embedded_notes(path, text, &self.inputs))
            .expect("note paths are strings")
    }

    /// moss's page stylesheet, then the site's theme so the theme wins. It is the same for every
    /// page, so `preview` leaves it out and the host adds it once.
    pub fn stylesheet(&self) -> &str {
        &self.css
    }

    /// A full document, its title, and the renderer's diagnostics. Use `check` for validation.
    pub fn preview(&self, path: &str, text: &str, base_href: &str) -> String {
        let mut page = page::render_page(path, text, &self.inputs);
        if let Some(heading) = self.pages.folder_heading(path, moss_core::frontmatter::body(text)).filter(|h| !h.is_empty()) {
            page.body_html = format!("{heading}\n{}", page.body_html);
        }
        let typesetting = moss_core::frontmatter::read_page(text).0.typesetting;
        if let Some(listing) = self.pages.own_listing(path, &self.texts, &self.inputs, typesetting.as_deref()) {
            page.body_html.push_str(&listing);
        }
        let url = |url_path: &str| moss_core::resolve::output_url::to_pretty_url(url_path);
        if let Some(nav) = self.pages.series_nav(path, &url, typesetting.as_deref()) {
            page.body_html.push_str(&nav);
        }
        let masthead = self.pages.masthead(path).unwrap_or_default();
        let html = page::preview_document(&page, &masthead, &self.inputs.lang, base_href);
        serde_json::to_string(&page::PagePreview {
            revision: page::content_hash(text.as_bytes()),
            html,
            title: page.title,
            diagnostics: diagnostics(page.diagnostics),
        }).expect("preview contains only JSON-compatible values")
    }
}

#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn builtin_schema_json() -> String {
    serde_json::to_string(&schema::builtin_schema()).expect("built-in schema is JSON-compatible")
}

/// Canonical frontmatter key spans in UTF-8 byte coordinates.
#[cfg_attr(feature = "ffi", uniffi::export)]
pub fn field_spans(text: &str) -> String {
    serde_json::to_string(&page::field_spans(text)).expect("field spans are JSON-compatible")
}

/// Assemble a site bundle and its warnings from host-read source bytes.
pub fn site_bundle(sources_json: &str) -> Result<String, String> {
    let sources = serde_json::from_str(sources_json).map_err(|e| e.to_string())?;
    let (bundle, warnings) = page::site_bundle(sources);
    serde_json::to_string(&serde_json::json!({ "bundle": bundle, "warnings": warnings }))
        .map_err(|e| e.to_string())
}

/// A JSON string or null, elected from a JSON array of site-relative paths.
pub fn home_file(files_json: &str) -> Result<String, String> {
    let files: Vec<String> = serde_json::from_str(files_json).map_err(|e| e.to_string())?;
    serde_json::to_string(&page::home_file(&files)).map_err(|e| e.to_string())
}

/// The paths whose texts `site_bundle` reads, from a JSON array of site-relative paths.
pub fn site_text_paths(files_json: &str) -> Result<String, String> {
    let files: Vec<String> = serde_json::from_str(files_json).map_err(|e| e.to_string())?;
    serde_json::to_string(&page::site_text_paths(&files)).map_err(|e| e.to_string())
}

fn diagnostics(items: Vec<Diagnostic>) -> Vec<EditorDiagnostic> {
    items.into_iter().map(EditorDiagnostic::from).collect()
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

/// The engine's tree root as JSON, from a `ListingEntry[]` and `TreeOptions`.
/// Leave `path_prefix` empty to keep paths relative to the folder.
pub fn build_tree(entries_json: &str, options_json: &str) -> Result<String, String> {
    let entries: Vec<moss_core::source_edit::ListingEntry> = decode(entries_json)?;
    let options: moss_core::source_edit::TreeOptions = decode(options_json)?;
    encode(&moss_core::source_edit::build_tree_root(&entries, &options))
}

/// Plan a text save. Disk is an optional UTF-8 string, not base64. `Write.bytes`
/// is a JSON array of u8 values, preserving the exact bytes to write. Conflict
/// documents have the engine's `ParsedFrontmatter` shape; after a successful
/// host write (or `already-saved`), the host reports `SaveResult::Saved`.
pub fn decide_save(disk_text: Option<&str>, expected_hash: Option<&str>, frontmatter_json: &str, body: &str) -> Result<String, String> {
    let frontmatter = decode(frontmatter_json)?;
    encode(&moss_core::source_edit::decide_save(disk_text.map(str::as_bytes), expected_hash, &frontmatter, body)?)
}

/// The same parsed document and raw-byte hash as the editor's read command.
pub fn parse_frontmatter(text: &str) -> Result<String, String> {
    encode(&moss_core::source_edit::parse_frontmatter_of(text.to_string())?)
}

pub fn describe_source_role(rel_path: &str) -> Result<String, String> {
    encode(&moss_core::source_edit::describe_source_role_inner(rel_path))
}

pub fn compute_heading_state(file_path: &str, frontmatter_json: &str, body: &str) -> Result<String, String> {
    let frontmatter: serde_json::Value = decode(frontmatter_json)?;
    if !frontmatter.is_object() { return Err("frontmatter must be a JSON object".to_string()); }
    encode(&moss_core::source_edit::compute_heading_state_inner(file_path, &frontmatter, body))
}

pub fn bootstrap_source_path(project_path: Option<&str>, source: Option<&str>) -> Result<String, String> {
    encode(&moss_core::source_edit::bootstrap_source_path(project_path, source))
}

fn decode<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

fn encode(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

/// Return a file name (not JSON), avoiding the full paths in `taken_json`.
pub fn free_name(dir: &str, name: &str, taken_json: &str) -> Result<String, String> {
    let taken: std::collections::HashSet<std::path::PathBuf> = decode(taken_json)?;
    Ok(moss_core::source_edit::free_name(std::path::Path::new(dir), name.as_ref(), |p| taken.contains(p))
        .to_string_lossy().into_owned())
}

/// Lowercase extensions, without dots, supported by image embeds.
pub fn image_extensions() -> String {
    encode(&moss_core::resolve::embed_renderer::IMAGE_EXTENSIONS).expect("image extensions are strings")
}

#[cfg(test)]
mod source_edit_tests;

/// Resolve editor references from a root-relative file snapshot and optional build map.
pub fn resolve_references(input_json: &str) -> Result<String, String> {
    let input: moss_core::resolve::editor::ResolveInput = decode(input_json)?;
    encode(&moss_core::resolve::resolve_references(&input))
}

/// Resolve one asset reference the way the desktop's single-asset command
/// does: an ambiguous name answers the file the build links, where the batch
/// keeps the ambiguity. Returns a `ResolvedAsset`, or `null` when nothing matches.
pub fn resolve_asset(input_json: &str, target: &str) -> Result<String, String> {
    let input: moss_core::resolve::editor::ResolveInput = decode(input_json)?;
    encode(&moss_core::resolve::editor::resolve_asset(&input, target))
}

/// Scan shortcode ranges with the editor's shared parser.
pub fn editor_scan(text: &str) -> Result<String, String> {
    encode(&moss_core::ast::editor_scan::editor_scan(text))
}
