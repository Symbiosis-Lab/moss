//! Emit-shaped artifact families (NORTH-STAR interception rule: "new emit
//! family → `emit/`-shaped module, never more code in `blocking.rs`").
//!
//! Each submodule owns one generated-artifact family end to end — content
//! addressing, rasterization/encoding, disk write, manifest registration —
//! and exposes a single build-phase entry point that `blocking.rs` merely
//! *gates* (the `fullscreen.js` conditional-emission precedent).

pub mod inventory;
pub mod math_png;
pub mod feature_styles;
pub mod scripts;
pub mod slots;
pub mod stylesheet;
