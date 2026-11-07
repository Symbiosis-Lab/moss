//! JavaScript bindings for the shared JSON boundary.

use crate::Site;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct MossSite(Site);

#[wasm_bindgen]
impl MossSite {
    #[wasm_bindgen(constructor)]
    pub fn new(site_json: &str) -> Result<MossSite, JsValue> {
        Site::from_json(site_json).map(Self).map_err(|e| JsValue::from_str(&e))
    }

    #[wasm_bindgen(js_name = schemaJson)]
    pub fn schema_json(&self) -> String {
        self.0.schema_json()
    }

    pub fn check(&self, path: &str, text: &str) -> String {
        self.0.check(path, text)
    }

    #[wasm_bindgen(js_name = embeddedNotes)]
    pub fn embedded_notes(&self, path: &str, text: &str) -> String {
        self.0.embedded_notes(path, text)
    }

    #[wasm_bindgen(js_name = setNotes)]
    pub fn set_notes(&mut self, notes_json: &str) -> Result<(), JsValue> {
        self.0.set_notes(notes_json).map_err(|e| JsValue::from_str(&e))
    }

    pub fn preview(&self, path: &str, text: &str, base_href: &str) -> String {
        self.0.preview(path, text, base_href)
    }
}

#[wasm_bindgen(js_name = builtinSchemaJson)]
pub fn builtin_schema_json() -> String {
    crate::builtin_schema_json()
}

#[wasm_bindgen(js_name = buildTree)]
pub fn build_tree(entries_json: &str, options_json: &str) -> Result<String, JsValue> {
    crate::build_tree(entries_json, options_json).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen(js_name = decideSave)]
pub fn decide_save(disk_text: Option<String>, expected_hash: Option<String>, frontmatter_json: &str, body: &str) -> Result<String, JsValue> {
    crate::decide_save(disk_text.as_deref(), expected_hash.as_deref(), frontmatter_json, body).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen(js_name = parseFrontmatter)]
pub fn parse_frontmatter(text: &str) -> Result<String, JsValue> {
    crate::parse_frontmatter(text).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen(js_name = describeSourceRole)]
pub fn describe_source_role(rel_path: &str) -> Result<String, JsValue> {
    crate::describe_source_role(rel_path).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen(js_name = computeHeadingState)]
pub fn compute_heading_state(file_path: &str, frontmatter_json: &str, body: &str) -> Result<String, JsValue> {
    crate::compute_heading_state(file_path, frontmatter_json, body).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen(js_name = bootstrapSourcePath)]
pub fn bootstrap_source_path(project_path: Option<String>, source: Option<String>) -> Result<String, JsValue> {
    crate::bootstrap_source_path(project_path.as_deref(), source.as_deref()).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen(js_name = fieldSpans)]
pub fn field_spans(text: &str) -> String {
    crate::field_spans(text)
}

#[wasm_bindgen(js_name = siteBundle)]
pub fn site_bundle(sources_json: &str) -> Result<String, JsValue> {
    crate::site_bundle(sources_json).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen(js_name = homeFile)]
pub fn home_file(files_json: &str) -> Result<String, JsValue> {
    crate::home_file(files_json).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen(js_name = siteTextPaths)]
pub fn site_text_paths(files_json: &str) -> Result<String, JsValue> {
    crate::site_text_paths(files_json).map_err(|e| JsValue::from_str(&e))
}
