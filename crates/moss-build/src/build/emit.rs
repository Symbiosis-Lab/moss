//! Emit-shaped artifact families: a new emit family gets an `emit/`-shaped
//! module, never more code in `blocking.rs`.
//!
//! Each submodule owns one generated-artifact family end to end — content
//! addressing, rasterization/encoding, disk write, manifest registration —
//! and exposes a single build-phase entry point that `blocking.rs` merely
//! *gates* (the `fullscreen.js` conditional-emission precedent).

pub mod beacon;
pub mod commerce;
pub mod members;
pub mod inventory;
pub mod listing_style;
pub mod math_png;
pub(crate) mod hosted_png;
pub mod diagram_png;
pub mod archival;
pub mod feature_styles;
pub mod place_map_assets;
pub mod place_map_labels;
pub mod place_map_layer_tiles;
pub mod place_map_layers;
pub mod scripts;
pub mod slots;
pub mod stylesheet;
pub mod theme_css_assets;
pub mod welcome_email;
