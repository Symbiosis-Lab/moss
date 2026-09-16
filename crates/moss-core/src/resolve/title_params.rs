//! In-process typed-params struct for moss embed synthesizers.
//!
//! Phase 3 PR4 (2026-05-27): the `moss:` title-attribute channel
//! retired. Stage 1 no longer encodes typed params into markdown image
//! titles; Stage 2 wikilink dispatch reads typed params directly from
//! pulldown-cmark's `LinkType::WikiLink` pothole text (parsed by
//! [`super::wikilink_dispatch::parse_pothole_params`]).
//!
//! What survives in this module:
//!
//! - [`TitleParams`] — the typed-param key/value bag. Every per-kind
//!   synthesizer (`render/iframe.rs`, `audio.rs`, `pdf.rs`, `video.rs`,
//!   `model.rs`, image via `render/image.rs`) takes `&TitleParams` as
//!   its first argument. The dispatcher constructs an instance from the
//!   wikilink pothole and hands it to the synth function.
//!
//! What retired:
//!
//! - `parse_title` (deserialized `moss:K=V K=V` from a markdown title) —
//!   no consumer remains. Wikilink potholes are parsed by
//!   [`super::wikilink_dispatch::parse_pothole_params`] instead.
//! - `emit_title` — Phase 3 PR4.5 (2026-05-27): retired with no
//!   surviving caller. PR3 removed `format_img_tag` (the last producer);
//!   PR4 dropped `parse_title` (the last consumer); PR4.5 routes
//!   non-image wikilink embeds directly to synth, removing the final
//!   transitional hold on the `moss:` markdown round-trip.

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
