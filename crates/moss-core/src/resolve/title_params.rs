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

use crate::resolve::embed_renderer::Sizing;

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

    /// The author's plain-text label as a leading-space HTML attribute named
    /// `attr` (`aria-label`, `alt`), escaped; empty when there is none.
    pub fn label_attr(&self, attr: &str) -> String {
        match self.get("label") {
            Some(l) => format!(
                " {attr}=\"{}\"",
                crate::resolve::embed_renderer::html_escape_attr(l)
            ),
            None => String::new(),
        }
    }

    pub fn insert(&mut self, k: impl Into<String>, v: impl Into<String>) {
        self.params.insert(k.into(), v.into());
    }
}

/// Reads the `|`-separated parts after a frame's URL or file name. A part
/// that parses as a size sets `width` and `height`; every other non-empty part
/// is title text, joined back with `|`. Order does not matter, and an empty
/// part is skipped, so a trailing pipe does not make an empty title.
///
/// Both the external-URL iframe and the local `.html` iframe read their alias
/// through this one function, so a frame is spelled the same way either way.
pub fn apply_frame_alias(alias: &str, params: &mut TitleParams) {
    let mut title: Vec<&str> = Vec::new();
    for part in alias.split('|').map(str::trim) {
        if part.is_empty() {
            continue;
        }
        match Sizing::parse(part) {
            Some(Sizing::Width(w)) => {
                params.insert("width", w.to_css());
            }
            Some(Sizing::Box(w, h)) => {
                params.insert("width", w.to_css());
                params.insert("height", h.to_css());
            }
            None => title.push(part),
        }
    }
    if !title.is_empty() {
        params.insert("title", title.join("|"));
    }
}
