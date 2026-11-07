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
        serde_json::to_string(&Preview {
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

#[derive(Serialize)]
struct Preview {
    /// [`page::content_hash`] of the text this render came from.
    revision: String,
    html: String,
    title: String,
    diagnostics: Vec<EditorDiagnostic>,
}

fn diagnostics(items: Vec<Diagnostic>) -> Vec<EditorDiagnostic> {
    items.into_iter().map(EditorDiagnostic::from).collect()
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
