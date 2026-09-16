//! Pre-fetched asset metadata available to Stage 1 + Stage 2.
//!
//! src-tauri's asset pipeline populates this before any markdown processing
//! runs. moss-core consumes it as input — pure Rust, zero I/O, full data.
//!
//! Mirrors the architectural shape of [`crate::content_graph::ContentGraph`]:
//! src-tauri does the I/O, moss-core takes typed data IN.
//!
//! ## Variant keying
//!
//! `variants` is keyed by the **stem path** (path with file extension stripped),
//! not the source path. Rationale: a single source asset (e.g. `assets/photo.jpg`)
//! may have multiple registered variants (`assets/photo.webp`, `assets/photo.avif`)
//! and the synthesizer asks "does this source have a webp variant?" without
//! caring about the source's own extension. src-tauri's
//! `AssetRegistry::iter_registered_variants` derives variant kinds from URL
//! extension and folds them under the shared stem. Consumers should look up
//! variants via [`AssetSnapshot::has_webp_for_source`] /
//! [`has_avif_for_source`], which normalize the source path to its stem
//! before the lookup.

use std::collections::HashMap;
use std::path::PathBuf;

/// Width fallback when AssetSnapshot.dimensions has no entry for a source path.
/// Returned by `MediaDimensionLookup::get` (in src-tauri) and consumed by
/// image_render's render_img_tag. The 800x600 pair is the legacy aspect ratio
/// from pre-Phase-2E days; semantically a layout-shift hint, not a real
/// dimension. Synthesizer callers may choose to omit width/height entirely
/// when the dimension lookup returns the fallback (honest-degradation path),
/// but production keeps them today for byte-shape parity with the
/// (since-retired) Stage 3 regex post-pass.
pub const FALLBACK_WIDTH: u32 = 800;
pub const FALLBACK_HEIGHT: u32 = 600;

/// Pre-fetched asset metadata available to moss-core's synthesizer.
/// Populated by src-tauri's asset pipeline before any markdown processing runs.
#[derive(Debug, Default, Clone)]
pub struct AssetSnapshot {
    /// Original-image dimensions. Path is the source path as it appears in markdown.
    pub dimensions: HashMap<PathBuf, (u32, u32)>,

    /// Base64-encoded LQIP data URI. Empty string if no LQIP computed (e.g.
    /// SVG, decorative images that don't participate in placeholder rendering).
    pub lqip: HashMap<PathBuf, String>,

    /// Registered variant URLs per source-stem. A variant is "registered" if
    /// it's in `AssetRegistry::set_pending` (Pending or Ready per ADR-013) —
    /// moss may emit a `<source srcset=…>` for it. Keyed by stem path
    /// (extension stripped); see module docs.
    pub variants: HashMap<PathBuf, VariantKindSet>,

    /// Dominant color hex (e.g. "#a0a0a0") for color-block fallback when
    /// LQIP isn't viable.
    pub dominant_color: HashMap<PathBuf, String>,

    /// Whether the source is an animated image (multi-frame GIF or WebP with an
    /// ANIM chunk). Keyed EXACTLY like `dimensions` — the source path as it
    /// appears in markdown (with its own extension), NOT the stem-keyed
    /// `variants` map. A missing key reads as `false` via [`is_animated`], so
    /// only src-tauri's scanned media populate it. The synthesizer consults it
    /// to suppress the responsive ladder for animated sources (which must never
    /// be resized/re-encoded).
    ///
    /// [`is_animated`]: AssetSnapshot::is_animated
    pub animated: HashMap<PathBuf, bool>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct VariantKindSet {
    pub webp: bool,
    pub avif: bool,
    /// An HLS ladder was built for this video — the master playlist and every
    /// rung it names are registered. Set by the build only: preview keeps the
    /// progressive MP4 so the author's transcode-pending hydration still works
    /// (the iframe-bridge swaps `src`, and the multi-source form has no `src`
    /// to swap). See `render::video`.
    pub hls: bool,
}

impl AssetSnapshot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn dims(&self, path: &PathBuf) -> Option<(u32, u32)> {
        self.dimensions.get(path).copied()
    }

    pub fn lqip(&self, path: &PathBuf) -> Option<&str> {
        self.lqip.get(path).map(String::as_str)
    }

    /// True if the source asset is an animated image (multi-frame GIF or an
    /// ANIM-chunk WebP). Missing key → `false`.
    ///
    /// `path` is the source path as it appears in markdown (with its own
    /// extension) — keyed identically to [`dims`], so a caller can probe both
    /// with the same key/normalization side by side.
    ///
    /// [`dims`]: AssetSnapshot::dims
    pub fn is_animated(&self, path: &PathBuf) -> bool {
        self.animated.get(path).copied().unwrap_or(false)
    }

    /// True if the source asset has a registered WebP variant.
    ///
    /// `src` is the source path as it appears in markdown (with its own
    /// extension, e.g. `assets/photo.jpg`). The stem is computed and used
    /// as the lookup key — see module docs for the keying rationale.
    pub fn has_webp_for_source(&self, src: &PathBuf) -> bool {
        let stem = path_strip_extension(src);
        self.variants.get(&stem).map_or(false, |v| v.webp)
    }

    /// True if the source video has a registered HLS ladder.
    pub fn has_hls_for_source(&self, src: &PathBuf) -> bool {
        let stem = path_strip_extension(src);
        self.variants.get(&stem).map_or(false, |v| v.hls)
    }

    /// True if the source asset has a registered AVIF variant.
    pub fn has_avif_for_source(&self, src: &PathBuf) -> bool {
        let stem = path_strip_extension(src);
        self.variants.get(&stem).map_or(false, |v| v.avif)
    }
}

/// Strip the final file extension from a path:
/// `assets/photo.webp` → `assets/photo`.
///
/// Files with no extension pass through unchanged. The parent directory is
/// preserved. Used as the canonical keying transform for
/// [`AssetSnapshot::variants`].
///
/// Takes `&PathBuf` (not `&Path`) so callers can write
/// `path_strip_extension(&"a/b.jpg".into())` and let inference pick `PathBuf`
/// — `&str` does not coerce to `&Path` through `.into()`.
pub fn path_strip_extension(p: &PathBuf) -> PathBuf {
    use std::path::Path;
    let stem = p.file_stem().unwrap_or_default();
    variant_key(p.parent().unwrap_or_else(|| Path::new("")).join(stem))
}

/// Put a variant key in its one canonical form: no leading `/`, no leading `./`.
///
/// Producers and the consumer reach this map by different routes and wrote the
/// same asset two ways. The registry keys off URLs a build minted
/// (`clip.hls/master.m3u8`); the only live consumer, the video emitter, keys
/// off the `src` a markdown embed carried, which is root-relative
/// (`/clip.mp4`). Both are correct in their own layer and neither is worth
/// rewriting to match the other, so the key drops the distinction here — the
/// one place both sides pass through — rather than obliging every future
/// writer to remember which form the map is in.
pub fn variant_key(p: impl AsRef<std::path::Path>) -> PathBuf {
    let p = p.as_ref();
    p.strip_prefix("/")
        .or_else(|_| p.strip_prefix("./"))
        .unwrap_or(p)
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn snapshot_default_is_empty() {
        let s = AssetSnapshot::new();
        assert_eq!(s.dims(&PathBuf::from("x.jpg")), None);
        assert_eq!(s.lqip(&PathBuf::from("x.jpg")), None);
        assert!(!s.has_webp_for_source(&PathBuf::from("x.jpg")));
    }

    #[test]
    fn snapshot_lookups() {
        let mut s = AssetSnapshot::new();
        s.dimensions.insert("photo.jpg".into(), (1024, 768));
        s.lqip
            .insert("photo.jpg".into(), "data:image/jpeg;base64,xx".into());
        // Variants are keyed by stem, not source path — see module docs.
        s.variants.insert(
            "photo".into(),
            VariantKindSet { webp: true, avif: false, hls: false },
        );

        assert_eq!(s.dims(&"photo.jpg".into()), Some((1024, 768)));
        assert_eq!(
            s.lqip(&"photo.jpg".into()),
            Some("data:image/jpeg;base64,xx")
        );
        assert!(s.has_webp_for_source(&"photo.jpg".into()));
        assert!(!s.has_avif_for_source(&"photo.jpg".into()));
    }

    #[test]
    fn snapshot_is_animated_lookup() {
        let mut s = AssetSnapshot::new();
        // Animated is keyed like `dimensions`: the source path as it appears
        // in markdown (with its own extension), NOT the stem-keyed variant key.
        s.animated.insert("assets/loop.gif".into(), true);
        s.animated.insert("assets/still.jpg".into(), false);

        // Present-true.
        assert!(s.is_animated(&"assets/loop.gif".into()));
        // Present-false.
        assert!(!s.is_animated(&"assets/still.jpg".into()));
        // Missing key → false.
        assert!(!s.is_animated(&"assets/unknown.png".into()));
    }

    #[test]
    fn snapshot_has_webp_for_source_strips_extension() {
        let mut s = AssetSnapshot::new();
        // Variants are keyed by stem (no extension), per the spec.
        let stem = path_strip_extension(&"assets/photo.jpg".into());
        s.variants.insert(
            stem,
            VariantKindSet { webp: true, avif: false, hls: false },
        );
        // Looking up by the source path (with .jpg) should find the variant.
        assert!(s.has_webp_for_source(&"assets/photo.jpg".into()));
        assert!(!s.has_avif_for_source(&"assets/photo.jpg".into()));
        assert!(!s.has_webp_for_source(&"assets/other.jpg".into()));
    }

    #[test]
    fn path_strip_extension_basic() {
        assert_eq!(
            path_strip_extension(&PathBuf::from("assets/photo.jpg")),
            PathBuf::from("assets/photo")
        );
        assert_eq!(
            path_strip_extension(&PathBuf::from("photo.webp")),
            PathBuf::from("photo")
        );
        // No extension — passes through.
        assert_eq!(
            path_strip_extension(&PathBuf::from("assets/photo")),
            PathBuf::from("assets/photo")
        );
        // Nested path.
        assert_eq!(
            path_strip_extension(&PathBuf::from("a/b/c/photo.png")),
            PathBuf::from("a/b/c/photo")
        );
    }
}
