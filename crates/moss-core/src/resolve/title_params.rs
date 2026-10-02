//! In-process typed-params struct for moss embed synthesizers.
//!
//! [`TitleParams`] is the typed-param key/value bag, parsed from a
//! wikilink embed's pothole text by
//! [`super::wikilink_dispatch::parse_pothole_params`] rather than from a
//! markdown image title. Every per-kind synthesizer (`render/iframe.rs`,
//! `audio.rs`, `pdf.rs`, `video.rs`, `model.rs`, image via
//! `render/image.rs`) takes `&TitleParams` as its first argument; the
//! dispatcher constructs an instance from the wikilink pothole and hands
//! it to the synth function.

use std::collections::BTreeMap;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TitleParams {
    pub params: BTreeMap<String, String>,
}

impl TitleParams {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub fn insert(&mut self, k: impl Into<String>, v: impl Into<String>) {
        self.params.insert(k.into(), v.into());
    }
}
