//! Media pipeline. Consolidates image, video, placeholder, cover, QR, ffmpeg orchestration.
//!
//! # Module layout
//!
//! - `ffmpeg`      — FFmpeg detection, video conversion helpers, the encode plan
//! - `hls`         — HLS ladder muxing: one invocation writes every rung
//! - `image`       — WebP image conversion, ImageConversionItem, run_image_conversion
//! - `fallback_raster` — the deployed raster original (the `<img>` inside `<picture>`)
//! - `video`       — Video transcoding dispatch, run_video_conversion
//! - `decode`      — the one place `image::ImageReader` is opened with content
//!                   sniffing instead of trusting the extension; shared by every
//!                   reader of image bytes in and outside this module
//! - `dimensions`  — MediaDimensionLookup (dimension / LQIP / color lookup table) +
//!                   extract_video_dominant_color (FFmpeg-based color extraction)
//! - `cover`       — CoverType detection and cover HTML rendering
//! - `qr`          — QR code SVG generation
//! - `pipeline`    — Asset copy/cleanup/sync
//! - `raw_img_warning` — Phase 2F build-time warning for author-typed raw <img>/<video>
//!
//! # Singleflight
//!
//! Both image and video conversion use `Singleflight` dedup. When
//! concurrent builds try to convert the same source_oid, only the first runs
//! the encoder; waiters receive a clone of the result.
//!
//! # Back-compat shims
//!
//! External callers reach these modules via shims in `build.rs` and
//! back-compat shim (now deleted; modules moved to canonical paths).
//! Symbol-level flat re-exports here are only present when there is a confirmed
//! external caller that uses the `build::media::Symbol` path directly. Dead
//! re-exports are omitted to avoid compiler warnings.

pub mod cover;
pub(crate) mod decode;
pub mod dimensions;
pub mod fallback_raster;
pub mod hls;
pub mod ffmpeg;
pub mod image;
pub(crate) mod manifest_hash_memo;
pub mod orphan_prune;
pub mod pipeline;
pub(crate) mod promise;
pub mod qr;
pub mod raw_img_warning;
pub(crate) mod remote_cover;
pub mod rungs;
pub(crate) mod sniff;
pub mod symlink;
pub mod video;

// ---------------------------------------------------------------------------
// Enumerated symbol re-exports — only symbols with external callers at the
// build::media:: path. Symbols reached via shim modules (build::image::*,
// build::ffmpeg::*, etc.) are already accessible without these.
// ---------------------------------------------------------------------------

// image — ImageConversionOutcome is referenced by BuildServices in types.rs
// via crate::build::media::image::ImageConversionOutcome.
pub use image::{ImageCompressionConfig, ImageConversionItem, ImageConversionOutcome};
