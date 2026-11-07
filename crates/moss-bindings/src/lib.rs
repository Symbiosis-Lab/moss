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
}

/// moss's page stylesheet, kept equal to the build's by a moss-build test.
const PAGE_CSS: &str = include_str!("../assets/page.css");

impl Site {
    pub fn from_json(site_json: &str) -> Result<Self, String> {
        let bundle: SiteBundle = serde_json::from_str(site_json).map_err(|e| e.to_string())?;
        let css = format!("{PAGE_CSS}\n{}", bundle.theme_css);
        Ok(Self { inputs: bundle.inputs(), css })
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

    /// A full document, its title, and the renderer's diagnostics. Use `check` for validation.
    pub fn preview(&self, path: &str, text: &str, base_href: &str) -> String {
        let page = page::render_page(path, text, &self.inputs);
        let html = page::preview_document(&page, &self.inputs.lang, &self.css, base_href);
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

#[cfg(test)]
mod source_edit_tests;
