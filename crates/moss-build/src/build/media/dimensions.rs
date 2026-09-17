//! Media dimension / LQIP / dominant-color lookup table.
//!
//! `MediaDimensionLookup` is the in-memory index built once per build from
//! scanned `MediaMetadata` (images + videos). It serves two production
//! consumers:
//!
//! 1. `build_asset_snapshot` in `build/markdown/pipeline.rs` — iterates
//!    the lookup's dimensions / lqip / colors to populate the typed
//!    `AssetSnapshot` consumed by the moss-core synthesizers.
//! 2. Folder card background colors via `get_cover_color` —
//!    called by `render::grid_cells::apply_collection_cards` and
//!    `page::generate_children` for `children_style: grid`.
//!
//! The Stage 3 regex post-pass that lived alongside this lookup until
//! Phase 2E v5 PR5 (2026-05-26) is gone — the moss-core synthesizers
//! now own all attribute emission at parse time.
//!
//! # References
//!
//! - [`docs/reference/structural-html-emission.md`](../../../../docs/reference/structural-html-emission.md): the seam this module lives within
//! - ADR-002: Dynamic SVG placeholders with dominant color
//! - ADR-006: Thumbnail-based extraction for performance

use crate::types::content::MediaMetadata;
use moss_core::asset_snapshot::{FALLBACK_HEIGHT, FALLBACK_WIDTH};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// Lookup table for media dimensions and colors, keyed by path.
///
/// This struct provides efficient lookup of dimensions and dominant colors
/// during HTML post-processing for placeholder generation.
/// The four path-keyed maps a lookup is built from.
///
/// Grouped because they are exactly what `build_asset_snapshot` reads, and
/// naming that dependency is what lets the snapshot be built BEFORE the lookup
/// exists. Passing `&MediaDimensionLookup` instead meant constructing the
/// struct with an empty placeholder snapshot and overwriting the field — which
/// left the door open for a later reader of `&self` to see the placeholder and
/// be silently wrong, the same class of defect the snapshot field itself fixed.
pub(crate) struct MediaMaps {
    /// Map from source path to (width, height) dimensions
    pub(crate) dimensions: HashMap<String, (u32, u32)>,
    /// Map from source path to dominant color hex string (e.g., "#ff5733")
    pub(crate) colors: HashMap<String, String>,
    /// Map from source path to LQIP data URI
    pub(crate) lqips: HashMap<String, String>,
    /// Map from source path to the scanned `is_animated` flag (multi-frame GIF
    /// / ANIM-chunk WebP). Populated from the SAME `MediaMetadata` iteration as
    /// `dimensions`, so `build_asset_snapshot` can index it under the identical
    /// source key (and additive output-URL slug key).
    pub(crate) animated: HashMap<String, bool>,
}

/// Lookup table for media dimensions and colors, keyed by path.
///
/// This struct provides efficient lookup of dimensions and dominant colors
/// during HTML post-processing for placeholder generation.
pub struct MediaDimensionLookup {
    maps: MediaMaps,
    /// The `AssetSnapshot` for this lookup, built once in [`Self::new`].
    ///
    /// A field rather than a lazy `OnceLock`, and the `AssetRegistry` it folds
    /// in is a constructor argument, because the alternative shipped a bug:
    /// `generate_blocking_content` built its own registry-ful snapshot and kept
    /// it, while every other consumer — wikilink embeds, media collections,
    /// review cards, covers — read a registry-LESS one from here. `variants`
    /// was empty for all of them, so `has_hls_for_source` and
    /// `has_avif_for_source` answered false however many variants were
    /// registered, and a `![[clip.mp4]]` could never emit its HLS ladder.
    ///
    /// Two snapshots of one vault is now unrepresentable: a lookup cannot exist
    /// without its snapshot, and the snapshot cannot be built without answering
    /// what registry it folds in. Building eagerly costs nothing that the
    /// `OnceLock` saved — it was memoizing one build per lookup either way,
    /// which is what fixed the O(pages x assets) rebuild measured 2026-08-24.
    snapshot: moss_core::asset_snapshot::AssetSnapshot,
}

impl MediaDimensionLookup {
    /// Creates a new lookup table from image and video metadata.
    ///
    /// # Arguments
    /// * `images` - Slice of image metadata from project structure
    /// * `videos` - Slice of video metadata from project structure
    /// * `dir_overrides` - The build's directory slug-overrides. Stored so
    ///   [`build_asset_snapshot`] can index dims/LQIP/color under the resolved
    ///   OUTPUT-URL key (including override slugs) that cover/folder-card
    ///   synthesizers probe with — otherwise the 800x600 fallback fires for
    ///   covers under an overridden dir (BUG 6). Pass `&HashMap::new()` where
    ///   no overrides apply (headless/feature/test paths).
    ///
    ///   (Restored 2026-07 for BUG 6. It had been removed 2026-05-20 when it
    ///   only fed the now-gone `webp_variants` mapping; the new consumer is the
    ///   snapshot's additive output-URL indexing, not variant mapping.)
    ///
    /// [`build_asset_snapshot`]: crate::build::markdown::pipeline::build_asset_snapshot
    pub fn new(
        images: &[MediaMetadata],
        videos: &[MediaMetadata],
        dir_overrides: &HashMap<String, String>,
        // The registry folded into this lookup's snapshot. `None` is the honest
        // answer for a lookup built where no build services exist (tests, and
        // the fragment renderers in `html.rs`); it is NOT a default, which is
        // why it is a required argument rather than an `Option` with one.
        registry: Option<&crate::types::assets::AssetRegistry>,
    ) -> Self {
        let mut dimensions = HashMap::new();
        let mut colors = HashMap::new();
        let mut lqips = HashMap::new();
        let mut animated = HashMap::new();

        for meta in images.iter().chain(videos.iter()) {
            if let Some((w, h)) = meta.dimensions {
                dimensions.insert(meta.path.clone(), (w, h));
            }
            if let Some(ref color) = meta.dominant_color {
                colors.insert(meta.path.clone(), color.clone());
            }
            if let Some(ref lqip) = meta.lqip_data_uri {
                lqips.insert(meta.path.clone(), lqip.clone());
            }
            // Recorded for EVERY scanned media (unconditionally, like the flag
            // itself), so any source with a `dimensions` entry also has an
            // `animated` entry under the identical key — the synthesizer probes
            // both side by side. Missing key still reads false downstream.
            animated.insert(meta.path.clone(), meta.is_animated);
        }

        // Index every entry a SECOND time under its slugified OUTPUT-URL key.
        //
        // These maps are keyed by raw source path, but `get_cover_color*`
        // probes with `cover_key(cover_url)` — a resolved output URL. On a site
        // whose folders carry `slug:` overrides the two never coincide
        // (`獎項/漫畫獎/第一季/…/assets/editor-memo-cover.jpg` against
        // `awards/comics/s1/kayla/assets/editor-memo-cover.jpg`), so the exact
        // and normalized lookups missed on EVERY cover and the whole site fell
        // through to the stem fallback — which is what let 22 articles sharing
        // an `editor-memo-cover` stem hand each other's colours around.
        // `build_asset_snapshot` already carries this fix (BUG 6) for the
        // synthesizer's copy of the same three maps; the lookup's own accessors
        // never got it.
        //
        // Strictly additive and sorted, exactly as `insert_slug` there: never
        // overwrite a real source key, and when two distinct source keys
        // slugify to one output key let the lexicographically smallest win, so
        // the build stays deterministic across `HashMap` iteration orders.
        fn index_by_output_url<V: Clone>(
            map: &mut HashMap<String, V>,
            dir_overrides: &HashMap<String, String>,
        ) {
            let mut entries: Vec<(String, V)> =
                map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (source_key, value) in entries {
                let slug = moss_core::resolve::output_url::resolve_path_with_overrides(
                    &source_key,
                    dir_overrides,
                );
                if slug != source_key {
                    map.entry(slug).or_insert(value);
                }
            }
        }
        index_by_output_url(&mut dimensions, dir_overrides);
        index_by_output_url(&mut colors, dir_overrides);
        index_by_output_url(&mut lqips, dir_overrides);
        index_by_output_url(&mut animated, dir_overrides);

        let maps = MediaMaps {
            dimensions,
            colors,
            lqips,
            animated,
        };
        let snapshot =
            crate::build::markdown::pipeline::build_asset_snapshot(&maps, registry, dir_overrides);
        Self { maps, snapshot }
    }

    /// This lookup's `AssetSnapshot`. See the `snapshot` field.
    pub fn asset_snapshot(&self) -> &moss_core::asset_snapshot::AssetSnapshot {
        &self.snapshot
    }

    /// Gets the raw scan-cached dominant color for a given path.
    ///
    /// Returns whatever scan stored — `#RRGGBB` for images,
    /// `hsla(...)` for videos. CSS accepts both, so consumers that
    /// inject the value into a CSS color slot (SVG `fill`, inline
    /// `background:`) can use this as-is.
    ///
    /// **Folder-card renderers should call
    /// `get_cover_color` instead** — it normalizes the
    /// format AND ensures WCAG-AA contrast against white text.
    pub fn get_dominant_color(&self, path: &str) -> Option<String> {
        resolve_entry(&self.maps.colors, path).cloned()
    }

    /// Look up the WCAG-AA-safe cover color for a card content band.
    ///
    /// Reads the raw `dominant_color` from scan cache, then normalizes via
    /// `color_extract::prepare_cover_color` (full saturation, darkened).
    /// Returns `None` when the URL is not in the cache or the color cannot
    /// be parsed.
    pub fn get_cover_color(&self, cover_url: &str) -> Option<String> {
        let key = cover_key(cover_url);
        let raw = self.get_dominant_color(&key)?;
        crate::build::components::color_extract::prepare_cover_color(&raw)
    }

    /// Look up the WCAG-AA-safe muted cover color for hero mobile backgrounds.
    ///
    /// Same lookup as `get_cover_color` but calls `prepare_cover_color_muted`
    /// (saturation × 0.8) for a softer atmospheric quality over full-width areas.
    pub fn get_cover_color_muted(&self, cover_url: &str) -> Option<String> {
        let key = cover_key(cover_url);
        let raw = self.get_dominant_color(&key)?;
        crate::build::components::color_extract::prepare_cover_color_muted(&raw)
    }

    /// Gets LQIP data URI for a given path.
    pub fn get_lqip(&self, path: &str) -> Option<String> {
        if let Some(lqip) = resolve_entry(&self.maps.lqips, path) {
            return Some(lqip.clone());
        }

        None
    }

    /// Gets dimensions for a given path, returning fallback if not found.
    ///
    /// # Arguments
    /// * `path` - The source path of the media file
    ///
    /// # Returns
    /// * `(width, height)` - Either from metadata or fallback (800x600)
    pub fn get(&self, path: &str) -> (u32, u32) {
        // Try exact match first
        if let Some(&dims) = self.maps.dimensions.get(path) {
            return dims;
        }

        // Try normalized path (strip leading ./ or ../)
        let normalized = normalize_path(path);
        if let Some(&dims) = self.maps.dimensions.get(&normalized) {
            return dims;
        }

        // Suffix match, for a path written relative to the page. Required to
        // land on a path BOUNDARY and to be UNIQUE: bare `cover.jpg` is a
        // suffix of every article's `.../assets/cover.jpg`, and answering with
        // whichever one the `HashMap` yielded first is how the same defect
        // fixed in `resolve_entry` reached dimensions.
        if let Some(&dims) = unique_suffix_match(&self.maps.dimensions, path, &normalized) {
            return dims;
        }

        // Extension mismatch (`<video src="clip.mp4">` against a `clip.mov` on
        // disk), scoped to one directory and required unique — see
        // `resolve_entry`.
        if let Some(&dims) = resolve_entry(&self.maps.dimensions, path) {
            return dims;
        }

        (FALLBACK_WIDTH, FALLBACK_HEIGHT)
    }
}

/// This lookup's snapshot, or a shared empty one when there is no lookup.
///
/// Three render paths — `generate_grid_item`, `render_colophon` and wikilink
/// dispatch — hold an `Option<&MediaDimensionLookup>` but need a
/// `&AssetSnapshot` either way. The empty snapshot is a process-wide singleton
/// so none of them has to keep a local alive purely to borrow from it.
pub fn snapshot_or_empty(
    lookup: Option<&MediaDimensionLookup>,
) -> &moss_core::asset_snapshot::AssetSnapshot {
    static EMPTY: OnceLock<moss_core::asset_snapshot::AssetSnapshot> = OnceLock::new();
    match lookup {
        Some(l) => l.asset_snapshot(),
        None => EMPTY.get_or_init(moss_core::asset_snapshot::AssetSnapshot::new),
    }
}

/// Turn a resolved cover **URL** back into the root-relative **path** the
/// dominant-color cache is keyed by.
///
/// `PathResolver::resolve_url` percent-encodes every segment, so a cover named
/// `封面.jpg` arrives as `/%E5%B0%81%E9%9D%A2.jpg` while the cache key is the
/// literal on-disk name. Without the decode the lookup misses and the card
/// silently loses its dominant color. Decoding is a no-op on a key that carries
/// no `%`, so this is safe for every caller.
fn cover_key(cover_url: &str) -> String {
    let decoded = moss_core::resolve::fuzzy_path::percent_decode_path(cover_url);
    decoded.strip_prefix('/').unwrap_or(&decoded).to_string()
}


/// A suffix match that lands on a path separator and is unique across the map.
///
/// `"a/b/cover.jpg".ends_with("cover.jpg")` is true, and so is the same test
/// against every other article's cover — so an un-scoped first-hit answer is
/// whatever the `HashMap` iterator produced this process. Requiring the match
/// to begin at a `/` keeps the intended case (a page-relative `assets/x.jpg`
/// naming the stored `posts/assets/x.jpg`) and requiring uniqueness makes the
/// answer independent of iteration order; ambiguity falls through to the
/// caller's own fallback rather than guessing.
fn unique_suffix_match<'a, V>(
    map: &'a HashMap<String, V>,
    path: &str,
    normalized: &str,
) -> Option<&'a V> {
    fn boundary_suffix(haystack: &str, needle: &str) -> bool {
        if needle.is_empty() || !haystack.ends_with(needle) {
            return false;
        }
        haystack.len() == needle.len()
            || haystack.as_bytes()[haystack.len() - needle.len() - 1] == b'/'
    }
    let mut found = None;
    for (stored, value) in map {
        let hit = boundary_suffix(path, stored)
            || boundary_suffix(stored, path)
            || boundary_suffix(normalized, stored)
            || boundary_suffix(stored, normalized);
        if !hit {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(value);
    }
    found
}

/// The stem fallback the three accessors share: exact key, then normalized key,
/// then a same-directory match on the file STEM, required to be UNIQUE.
///
/// The stem step is there for a real case — a cover URL naming the derived
/// `.webp` while this map holds the source `.jpg`, or `<video src="clip.mp4">`
/// against a `clip.mov` on disk. What it used to do was scan the WHOLE map for
/// any entry sharing a stem and return the first one the `HashMap` happened to
/// yield.
///
/// On a site where 22 articles each keep an `assets/editor-memo-cover.jpg`,
/// that returns an arbitrary OTHER article's value — and reshuffles it every
/// build, because `HashMap` iteration order is not stable across processes.
/// Measured on the harbor vault (2026-08-19): 46 built pages differed
/// between two consecutive generations in `--moss-cover-color` and nothing
/// else, with two unrelated articles landing on the identical wrong colour.
/// Every one of those covers is also re-uploaded on every publish.
///
/// Scoping the match to the entry's own directory makes a cross-article match
/// impossible, and demanding uniqueness makes the result independent of
/// iteration order. Ambiguity (two extensions of one stem in one directory)
/// yields `None`: no value is honest, a neighbour's value is not.
fn resolve_entry<'a, V>(map: &'a HashMap<String, V>, path: &str) -> Option<&'a V> {
    if let Some(v) = map.get(path) {
        return Some(v);
    }
    let normalized = normalize_path(path);
    if let Some(v) = map.get(&normalized) {
        return Some(v);
    }
    let key = Path::new(&normalized);
    let stem = key.file_stem()?;
    let dir = key.parent();
    let mut found = None;
    for (stored, value) in map {
        let stored_path = Path::new(stored);
        if stored_path.file_stem() != Some(stem) || stored_path.parent() != dir {
            continue;
        }
        if found.is_some() {
            // Two entries in one directory share this stem — which one the
            // caller meant is unknowable, so answer nothing rather than guess.
            return None;
        }
        found = Some(value);
    }
    found
}

/// Normalizes a path by stripping leading ./ and ../
fn normalize_path(path: &str) -> String {
    let mut result = path;

    // Strip leading ./
    while let Some(rest) = result.strip_prefix("./") {
        result = rest;
    }

    // Strip leading ../
    while let Some(rest) = result.strip_prefix("../") {
        result = rest;
    }

    result.to_string()
}

/// Extract dominant color from video first frame using FFmpeg.
///
/// Fast operation (~0.1s) - extracts frame at 1 second to skip black intros.
/// Uses existing color_extract utility for consistency.
///
/// # Arguments
/// * `ffmpeg` - FFmpegManager with resolved binary path
/// * `video_path` - Path to video file
///
/// # Returns
/// * `Some(String)` - Hex color string (e.g., "#ff5733")
/// * `None` - If extraction fails
pub fn extract_video_dominant_color(
    ffmpeg: &super::ffmpeg::FFmpegManager,
    video_path: &Path,
) -> Option<String> {
    use std::fs;

    if !video_path.exists() {
        return None;
    }

    // Create temporary file path for frame extraction
    let temp_dir = std::env::temp_dir();
    let frame_filename = format!(
        "moss_frame_{}.jpg",
        video_path.file_stem()?.to_string_lossy()
    );
    let frame_path = temp_dir.join(frame_filename);

    // Extract frame at 1 second using the resolved FFmpeg binary
    let output = super::ffmpeg::background_command(Path::new(ffmpeg.bin_path()))
        .args([
            "-y",                  // Overwrite output
            "-ss", "1.0",         // Seek to 1 second (skip black intro)
            "-i", video_path.to_str()?,
            "-vframes", "1",      // Extract 1 frame
            "-q:v", "2",          // High quality
            "-f", "image2",       // Image format
            frame_path.to_str()?,
        ])
        .output()
        .ok()?;

    if !output.status.success() || !frame_path.exists() {
        return None;
    }

    // Use existing color extraction utility
    let color = crate::build::components::color_extract::extract_dominant_color(&frame_path);

    // Clean up temp file
    // allow:unlink a frame temp this call extracted
    let _ = fs::remove_file(&frame_path);

    color
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_cover_color_muted_returns_desaturated_color() {
        let mut lookup = MediaDimensionLookup::new(&[], &[], &HashMap::new(), None);
        lookup.maps.colors.insert("assets/hero.jpg".to_string(), "#0088CC".to_string());
        let color = lookup.get_cover_color_muted("assets/hero.jpg")
            .expect("should produce a color");
        assert!(color.starts_with("hsla("), "expected hsla, got {color}");
    }

    #[test]
    fn get_cover_color_muted_returns_none_for_unknown_path() {
        let lookup = MediaDimensionLookup::new(&[], &[], &HashMap::new(), None);
        assert!(lookup.get_cover_color_muted("missing.jpg").is_none());
    }

    /// The cache is keyed by the on-disk path, but callers hand these methods a
    /// URL that `PathResolver::resolve_url` has percent-encoded. Without the
    /// decode the lookup misses and the card silently loses its color — the
    /// regression the encoding fix would otherwise have introduced.
    #[test]
    fn cover_color_lookup_decodes_a_percent_encoded_url() {
        let mut lookup = MediaDimensionLookup::new(&[], &[], &HashMap::new(), None);
        lookup.maps.colors.insert("獎項/封面.jpg".to_string(), "#0088CC".to_string());
        let encoded = "/%E7%8D%8E%E9%A0%85/%E5%B0%81%E9%9D%A2.jpg";
        assert!(lookup.get_cover_color(encoded).is_some(), "encoded URL must hit the cache");
        assert!(lookup.get_cover_color_muted(encoded).is_some());
    }

    /// Decoding must not break the plain-ASCII callers that were already working.
    #[test]
    fn cover_color_lookup_still_accepts_an_unencoded_key() {
        let mut lookup = MediaDimensionLookup::new(&[], &[], &HashMap::new(), None);
        lookup.maps.colors.insert("assets/hero.jpg".to_string(), "#0088CC".to_string());
        assert!(lookup.get_cover_color("/assets/hero.jpg").is_some());
        assert!(lookup.get_cover_color("assets/hero.jpg").is_some());
    }

    /// One article must never be served another article's cover colour.
    ///
    /// A convention-named cover (`editor-memo-cover.jpg`) repeats once per
    /// article — 22 times on the harbor vault. The old stem fallback scanned
    /// the whole map for ANY entry sharing a stem and returned the first the
    /// `HashMap` yielded, so covers borrowed each other's colours and the
    /// borrowing reshuffled every build: 46 pages differed between two
    /// consecutive generations in `--moss-cover-color` and nothing else.
    #[test]
    fn covers_sharing_a_stem_never_borrow_each_others_color() {
        let mut lookup = MediaDimensionLookup::new(&[], &[], &HashMap::new(), None);
        for (dir, hex) in [("kayla", "#0088CC"), ("liweixi", "#CC4400"), ("gold", "#22AA22")] {
            lookup
                .maps
                .colors
                .insert(format!("awards/{dir}/assets/editor-memo-cover.jpg"), hex.to_string());
        }

        // Each article resolves to its own colour, not a neighbour's.
        for (dir, hex) in [("kayla", "#0088CC"), ("liweixi", "#CC4400"), ("gold", "#22AA22")] {
            assert_eq!(
                lookup.get_dominant_color(&format!("awards/{dir}/assets/editor-memo-cover.jpg")),
                Some(hex.to_string()),
                "{dir} must keep its own cover colour"
            );
        }

        // An article with no entry gets NOTHING rather than a stranger's
        // colour — the stem matches three times over, and guessing among them
        // is what made the output non-deterministic.
        assert_eq!(
            lookup.get_dominant_color("awards/unknown/assets/editor-memo-cover.jpg"),
            None,
            "an ambiguous stem must not resolve to an arbitrary entry"
        );
    }

    /// The extension-mismatch case the stem fallback exists for still works —
    /// scoping it to one directory is what was missing, not the fallback itself.
    #[test]
    fn a_stem_match_still_resolves_within_one_directory() {
        let mut lookup = MediaDimensionLookup::new(&[], &[], &HashMap::new(), None);
        lookup.maps.colors.insert("posts/assets/clip.mov".to_string(), "#0088CC".to_string());
        assert_eq!(
            lookup.get_dominant_color("posts/assets/clip.mp4"),
            Some("#0088CC".to_string()),
            "a sibling extension in the SAME directory is the case this fallback is for"
        );
        // Same stem, different directory: not a match.
        assert_eq!(lookup.get_dominant_color("other/assets/clip.mp4"), None);
    }

    /// The snapshot indexes a cover under its OVERRIDE-slug output-URL key.
    ///
    /// Was `the_memoized_snapshot_equals_a_freshly_built_one`, which compared
    /// `asset_snapshot()` against a fresh `build_asset_snapshot` call to license
    /// the `OnceLock`. There is no memo now — the snapshot is a field built once
    /// in `new` — so that comparison could only have failed if `new` passed
    /// itself different arguments. This is the half that tested something: BUG 6,
    /// the additive output-URL key, without which a cover under an overridden
    /// directory misses every probe and falls back to 800x600.
    #[test]
    fn the_snapshot_indexes_a_cover_under_its_override_slug() {
        let mut overrides = HashMap::new();
        overrides.insert("獎項".to_string(), "awards".to_string());
        let meta = MediaMetadata {
            path: "獎項/assets/cover.png".to_string(),
            dimensions: Some((1200, 800)),
            dominant_color: Some("#336699".to_string()),
            lqip_data_uri: Some("data:image/webp;base64,AAAA".to_string()),
            ..Default::default()
        };
        let lookup = MediaDimensionLookup::new(&[meta], &[], &overrides, None);

        assert!(lookup
            .asset_snapshot()
            .dimensions
            .contains_key(std::path::Path::new("awards/assets/cover.png")));
    }

    /// A lookup built WITH a registry carries that registry's variants; one
    /// built without carries none.
    ///
    /// The regression guard for the defect that made this a constructor
    /// argument. `generate_blocking_content` folded a registry into its own
    /// snapshot and kept it, while every other consumer read a registry-less one
    /// from the lookup — so `![[clip.mp4]]` asked `has_hls_for_source`, got
    /// false, and emitted a progressive `<video src>` no matter how complete the
    /// ladder on disk was. Nothing caught it: every emitter test hands
    /// `synthesize_video_html` a snapshot it built by hand.
    #[test]
    fn a_lookup_carries_the_variants_of_the_registry_it_was_built_with() {
        use std::path::Path;
        let meta = MediaMetadata {
            path: "clip.mp4".to_string(),
            dimensions: Some((1920, 1080)),
            ..Default::default()
        };
        let registry = crate::types::assets::AssetRegistry::new();
        for member in
            moss_core::asset_paths::hls_members(&moss_core::asset_paths::VIDEO_LADDER)
        {
            // Registered BARE and probed root-relative below, deliberately.
            // The two sides of this map are written by different layers in
            // different forms; a test that used one form on both sides would
            // assert an agreement it had just manufactured, which is how the
            // original defect survived.
            registry.set_pending(format!("clip.hls/{member}"), None, None);
        }

        let with = MediaDimensionLookup::new(&[meta.clone()], &[], &HashMap::new(), Some(&registry));
        assert!(
            with.asset_snapshot().has_hls_for_source(&Path::new("/clip.mp4").to_path_buf()),
            "a registry holding a master playlist must reach the snapshot"
        );

        let without = MediaDimensionLookup::new(&[meta], &[], &HashMap::new(), None);
        assert!(
            !without.asset_snapshot().has_hls_for_source(&Path::new("/clip.mp4").to_path_buf()),
            "no registry, no variants — the absence must be honest, not a stale carry-over"
        );
    }
}
