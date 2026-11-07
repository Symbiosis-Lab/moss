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
