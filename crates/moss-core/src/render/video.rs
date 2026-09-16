//! Video embed synthesizer.
//!
//! Receives a [`TitleParams`] (Stage 2 dispatcher already parsed it), the
//! source URL, and an [`AssetSnapshot`]. Emits final `<video>` HTML at the
//! typed-data boundary — including every attribute the legacy
//! `add_video_placeholder_attributes` regex used to inject.
//!
//! # Authoritative byte shape (Phase 2E parity)
//!
//! ```html
//! <video class="moss-embed moss-embed-video" src="URL.mp4"
//!        data-placeholder-src="URL.original" poster="URL.thumb.jpg"
//!        data-thumb-src="URL.thumb.jpg" controls playsinline preload="metadata"
//!        { width="W"}?{ height="H"}?></video>
//! ```
//!
//! - Single `src=` attribute on `<video>` (no nested `<source>` child) —
//!   load-bearing for the surviving `add_video_placeholder_attributes`
//!   regex pass (preserved until PR3 of Phase 2E). The regex's skip guard
//!   at `placeholder.rs:435` triggers on `data-placeholder-src`, making
//!   the post-pass a no-op for synthesizer-emitted videos.
//!   Source: moss-core pre-Phase-0 `VideoRenderer` at
//!   `crates/moss-core/src/resolve/embed_renderer.rs:509-571` (commit `efb834a3e`).
//! - `controls playsinline preload="metadata"` emitted on the **default**
//!   branch. `playsinline` keeps playback in the page on iOS instead of
//!   handing the video to the fullscreen AVKit player; both branches carry
//!   it, so the attribute is not what distinguishes them. The ambient loop
//!   branch (`![[clip.mp4|loop]]`) instead emits
//!   `autoplay muted loop playsinline preload="metadata"` with no `controls`
//!   and adds `data-loop` for JS/CSS targeting. See `AMBIENT_PLAYBACK_ATTRS`.
//! - Width/height priority: (1) `TitleParams` `|WxH` sizing alias wins
//!   when present (with units like `640px`). (2) `AssetSnapshot.dimensions`
//!   lookup as fallback, emitted unitless (e.g. `width="1920"`). (3) Both
//!   attributes omitted when the snapshot dims are `(0, 0)` — that is the
//!   sentinel for "unknown," NOT a real dimension. Both omitted when no
//!   entry is in the snapshot.
//! - `data-placeholder-src` is the ORIGINAL src (e.g. `clip.mov`). The
//!   iframe-bridge swaps `src` to the `.mp4` payload once the transcode
//!   completes (`frontend/bridge/iframe-bridge.ts:666`).
//! - `poster` and `data-thumb-src` both reference `to_thumb(original_src)`.
//!   The iframe-bridge listens for `moss-asset-ready` and swaps `poster`
//!   in when the thumbnail lands.
//!
//! # `.mov` → `.mp4` source-extension swap (moved from placeholder.rs)
//!
//! moss converts `.mov` source files to `.mp4` during build, so a raw
//! `.mov` reference in the rendered HTML would 404. Pre-Phase-0 this swap
//! lived in `placeholder.rs::add_video_placeholder_attributes` as a regex
//! post-pass. Phase 1's typed-data-boundary architecture moves it here:
//! the synthesizer is the single source of truth for the URL that ends up
//! in `<video src=>`.
//!
//! # Multi-source HLS form
//!
//! This synthesizer emits `<video><source>…</video>` when — and only when —
//! the snapshot says an HLS ladder is registered for the source. It was
//! previously documented as never emitting that shape, with the note that a
//! callsite wanting it "must NOT route through this synthesizer". Extending
//! the contract here rather than adding a second emitter is the deliberate
//! choice: `synthesize_image_html` already owns both the bare `<img>` and the
//! multi-source `<picture>` form and picks between them on the same snapshot,
//! and a second video emitter would mean two owners of `poster`, the sizing
//! rules, the ambient-loop branch and the `.mov` swap — four things that have
//! to agree and that nothing would force to.
//!
//! The two forms are exclusive by HTML's own rule: a `<video>` carrying `src`
//! ignores its `<source>` children, so the ladder form drops `src` entirely.
//! That is what makes the snapshot flag load-bearing rather than cosmetic.
//!
//! Consequences, both intended:
//!
//! - The surviving `add_video_placeholder_attributes` regex requires `src="…"`
//!   on `<video>` and so no-ops on the ladder form. It was already a no-op for
//!   synthesizer-emitted videos — its skip guard triggers on
//!   `data-placeholder-src`, which this emits on both branches — so nothing is
//!   lost that was being used.
//! - The iframe-bridge's transcode-pending hydration swaps `src`, and the
//!   ladder form has no `src` to swap. So the ladder form is emitted only once
//!   the ladder is already **built and registered** — never merely planned.
//!   moss renders HTML before the video worker runs, so a video's first build
//!   ships the single-src form with live hydration and the ladder appears on
//!   the next build, off the transform cache. That ordering is also what keeps
//!   ADR-013's never-404 rule: `<video>` recovers from a failed `<source>` no
//!   better than `<picture>` does, so a `.m3u8` URL is only offered when its
//!   bytes exist.
//!
//! The progressive MP4 is always the last `<source>`, so a browser that
//! understands neither HLS natively nor hls.js still plays the video. A plain
//! download link follows the player for the case where it plays nothing at all.

use crate::asset_paths::{to_hls_master, to_mp4, to_thumb};
use crate::asset_snapshot::AssetSnapshot;
use crate::resolve::embed_renderer::html_escape_attr;
use crate::resolve::title_params::TitleParams;
use std::path::PathBuf;

/// Core playback attributes shared by the ambient loop branch and cover.rs.
///
/// The loop branch prepends `autoplay` to this. cover.rs intentionally omits
/// `autoplay` (covers are hover-played, not auto) — if cover.rs can't import
/// this const directly, keep its literal with:
/// `// keep in sync with render::video::AMBIENT_PLAYBACK_ATTRS (covers omit autoplay)`
pub const AMBIENT_PLAYBACK_ATTRS: &str = r#"muted loop playsinline preload="metadata""#;

/// The master playlist's type. Apple's `application/vnd.apple.mpegurl` rather
/// than the `application/x-mpegURL` seen in older examples: Safari accepts both
/// and the former is the registered one.
const HLS_MIME: &str = "application/vnd.apple.mpegurl";

/// Shown beside a ladder-form player. The player can fail for reasons the page
/// cannot detect — a codec the device lacks, a corporate proxy that strips
/// streaming — and on the slow links this ladder exists for, a file the viewer
/// can start and leave running beats one that will not start at all.
const DOWNLOAD_LABEL: &str = "Download video";

/// Synthesize video embed HTML for `Tag::Link` with `moss:kind=video` title.
///
/// Owns the `.mov` → `.mp4` source-extension swap at the typed-data
/// boundary (moved from `placeholder.rs` per the unified-emission
/// migration). Emits the full attribute set the surviving regex pass
/// would have injected — `data-placeholder-src`, `poster`,
/// `data-thumb-src`, and snapshot-derived `width`/`height` — so the
/// post-pass becomes a no-op for synthesizer-emitted videos. See module
/// docs for the authoritative byte-shape contract.
pub fn synthesize_video_html(
    params: &TitleParams,
    src: &str,
    assets: &AssetSnapshot,
) -> String {
    // .mov → .mp4 source-extension swap (idempotent for .mp4 / .webm).
    // Pre-Phase-0 lived in placeholder.rs; moved here because the typed-
    // data layer is the single source of truth for the served URL.
    let converted_src = to_mp4(src);

    // Thumbnail path — used for both `poster` (initial frame before
    // transcode lands) and `data-thumb-src` (iframe-bridge swap target
    // when `moss-asset-ready` fires). Mirrors the regex behaviour at
    // `placeholder.rs:444-446`.
    let thumb = to_thumb(src);

    // Width/height priority:
    //   (1) TitleParams alias (e.g. `|640x360` → `width="640px" height="360px"`)
    //   (2) AssetSnapshot.dimensions lookup (unitless ints)
    //   (3) omit both
    // `(0, 0)` from the snapshot is a sentinel for unknown — omit both
    // when we see it, per the Phase 2E design (the regex emits `width="0"`
    // in that case; the synthesizer corrects that long-standing bug).
    let (width_attr, height_attr) = if let (Some(w), Some(h)) = (params.get("width"), params.get("height")) {
        (
            format!(r#" width="{}""#, html_escape_attr(w)),
            format!(r#" height="{}""#, html_escape_attr(h)),
        )
    } else if let Some(w) = params.get("width") {
        // Width-only alias is unusual but historically supported.
        (
            format!(r#" width="{}""#, html_escape_attr(w)),
            String::new(),
        )
    } else {
        match assets.dims(&PathBuf::from(src)) {
            Some((w, h)) if w > 0 && h > 0 => (
                format!(r#" width="{}""#, w),
                format!(r#" height="{}""#, h),
            ),
            _ => (String::new(), String::new()),
        }
    };

    // Ambient loop branch: `![[clip.mp4|loop]]` → autoplay + ambient set,
    // no controls, data-loop JS/CSS hook. Default branch keeps controls.
    let is_loop = params.get("loop").is_some();
    let (playback, data_loop) = if is_loop {
        (
            format!("autoplay {}", AMBIENT_PLAYBACK_ATTRS),
            r#" data-loop"#,
        )
    } else {
        (r#"controls playsinline preload="metadata""#.to_string(), "")
    };

    if assets.has_hls_for_source(&PathBuf::from(src)) {
        // Ladder form. `src` is deliberately absent — a <video> carrying it
        // ignores its <source> children — and the progressive MP4 is last so a
        // browser with neither native HLS nor hls.js still plays something.
        return format!(
            r#"<video class="moss-embed moss-embed-video" data-type="video"{data_loop} data-placeholder-src="{orig}" poster="{thumb}" data-thumb-src="{thumb}" {playback}{w}{h}><source src="{hls}" type="{HLS_MIME}"><source src="{src}" type="video/mp4"></video><p class="moss-embed-video-download"><a href="{src}" download>{download}</a></p>"#,
            data_loop = data_loop,
            hls = html_escape_attr(&to_hls_master(src)),
            src = html_escape_attr(&converted_src),
            orig = html_escape_attr(src),
            thumb = html_escape_attr(&thumb),
            playback = playback,
            w = width_attr,
            h = height_attr,
            download = DOWNLOAD_LABEL,
        );
    }

    format!(
        r#"<video class="moss-embed moss-embed-video" data-type="video"{data_loop} src="{src}" data-placeholder-src="{orig}" poster="{thumb}" data-thumb-src="{thumb}" {playback}{w}{h}></video>"#,
        data_loop = data_loop,
        src = html_escape_attr(&converted_src),
        orig = html_escape_attr(src),
        thumb = html_escape_attr(&thumb),
        playback = playback,
        w = width_attr,
        h = height_attr,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_snapshot() -> AssetSnapshot {
        AssetSnapshot::new()
    }

    fn params_with(kvs: &[(&str, &str)]) -> TitleParams {
        let mut p = TitleParams::default();
        for (k, v) in kvs {
            p.insert(*k, *v);
        }
        p
    }

    // --- byte-shape parity with pre-Phase-0 VideoRenderer ---

    #[test]
    fn video_basic_shape() {
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(out.contains("<video"), "got: {}", out);
        assert!(out.contains(r#"src="clip.mp4""#), "got: {}", out);
    }

    #[test]
    fn video_emits_moss_embed_classes() {
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(
            out.contains(r#"class="moss-embed moss-embed-video""#),
            "got: {}",
            out
        );
    }

    #[test]
    fn video_emits_closing_tag() {
        // Pre-Phase-0 VideoRenderer ended with `</video>` (not self-closing).
        // The downstream rewriter regex matches `<video … src="…">` and
        // expects a separate closing tag.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(out.ends_with("</video>"), "got: {}", out);
    }

    #[test]
    fn video_emits_controls_and_playsinline_on_default_path() {
        // The default wikilink `![[clip.mp4]]` (no `loop` param) emits
        // `controls playsinline preload="metadata"`. The loop branch is the one
        // exception on `controls` (it emits the ambient set instead), but it
        // carries `playsinline` too — without it iOS hands playback to the
        // fullscreen AVKit player instead of playing in the page.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(out.contains(" controls"), "default path must emit controls, got: {}", out);
        assert!(out.contains(" playsinline"), "default path must emit playsinline, got: {}", out);
    }

    #[test]
    fn video_emits_preload_metadata() {
        // `preload="metadata"` is the historical default: browser fetches
        // duration/dimensions but defers the full payload until play.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(out.contains(r#"preload="metadata""#), "got: {}", out);
    }

    #[test]
    fn video_single_src_no_nested_source() {
        // Single src= on <video>, NOT a nested <source> child, whenever no HLS
        // ladder is registered — which is every preview render and every video
        // too narrow for a ladder. Load-bearing: the surviving
        // add_video_placeholder_attributes regex matches `<video\s+[^>]*?src="…">`,
        // and the iframe-bridge's transcode-pending hydration swaps that `src`.
        // With a nested <source> and no `src`, both silently drop. See
        // pre-Phase-0 VideoRenderer doc comment at embed_renderer.rs:519-545.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(!out.contains("<source"), "must not emit <source>: {}", out);
    }

    fn hls_snapshot(stem: &str) -> AssetSnapshot {
        let mut s = AssetSnapshot::new();
        s.variants.insert(
            PathBuf::from(stem),
            crate::asset_snapshot::VariantKindSet {
                webp: false,
                avif: false,
                hls: true,
            },
        );
        s
    }

    #[test]
    fn video_with_a_registered_ladder_emits_sources_and_drops_src() {
        // The two forms are exclusive by HTML's own rule: a <video> carrying
        // `src` ignores its <source> children, so emitting both would silently
        // serve the progressive MP4 to everyone and make the ladder dead bytes.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mov", &hls_snapshot("clip"));
        assert!(
            !out.contains(r#"<video class="moss-embed moss-embed-video" data-type="video" src="#),
            "the ladder form must not carry src on <video>: {out}"
        );
        assert!(
            out.contains(
                r#"<source src="clip.hls/master.m3u8" type="application/vnd.apple.mpegurl">"#
            ),
            "got: {out}"
        );
    }

    #[test]
    fn the_progressive_mp4_is_the_last_source() {
        // A browser with neither native HLS nor hls.js picks the last source it
        // understands. If the MP4 were first, every Safari would take it and
        // the ladder would never be used.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mov", &hls_snapshot("clip"));
        let hls_at = out.find("clip.hls/master.m3u8").expect("hls source");
        let mp4_at = out.find(r#"<source src="clip.mp4""#).expect("mp4 source");
        assert!(hls_at < mp4_at, "MP4 must come last: {out}");
    }

    #[test]
    fn the_ladder_form_keeps_the_poster_and_offers_a_download() {
        // The poster is what a viewer on the link this exists for sees during
        // the seconds before the first segment lands; losing it would make the
        // slow case look broken rather than slow.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mov", &hls_snapshot("clip"));
        assert!(out.contains(r#"poster="clip.thumb.jpg""#), "got: {out}");
        assert!(
            out.contains(r#"<a href="clip.mp4" download>"#),
            "a player that fails entirely still leaves a way to watch: {out}"
        );
    }

    #[test]
    fn a_ladder_registered_for_another_video_does_not_leak() {
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "other.mp4", &hls_snapshot("clip"));
        assert!(!out.contains("<source"), "got: {out}");
    }

    // --- .mov → .mp4 source-extension swap ---

    #[test]
    fn video_mov_extension_swaps_to_mp4() {
        // moss transcodes .mov source files to .mp4 during build; the
        // emitted URL must reference the .mp4 output. Pre-Phase-0 the
        // regex pass in placeholder.rs owned this swap. Phase 1 moves it
        // to the synthesizer at the typed-data boundary (Decision #6:
        // zero carve-outs).
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mov", &empty_snapshot());
        assert!(
            out.contains(r#"src="clip.mp4""#),
            "expected .mov swapped to .mp4, got: {}",
            out
        );
        // The `src=` attribute (preceded by space, not `-src=` from
        // data-placeholder-src) must point at the .mp4. Using a leading
        // space disambiguates `src=` from `data-placeholder-src=`, where
        // `.mov` legitimately survives as the original-src attribute that
        // the iframe-bridge listens to (`iframe-bridge.ts:666`).
        assert!(
            !out.contains(r#" src="clip.mov""#),
            "raw .mov must not appear in src= after swap, got: {}",
            out
        );
    }

    #[test]
    fn video_uppercase_mov_extension_swaps_to_mp4() {
        // .MOV (uppercase) is the macOS QuickTime export default and must
        // also swap. Mirrors the to_mp4 helper's case-insensitive contract.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.MOV", &empty_snapshot());
        assert!(
            out.contains(r#"src="clip.mp4""#),
            "expected .MOV swapped to .mp4, got: {}",
            out
        );
    }

    #[test]
    fn video_mp4_extension_pass_through() {
        // .mp4 is the served form; pass through unchanged.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "../assets/clip.mp4", &empty_snapshot());
        assert!(
            out.contains(r#"src="../assets/clip.mp4""#),
            "got: {}",
            out
        );
    }

    #[test]
    fn video_webm_extension_pass_through() {
        // .webm is not transcoded; pass through.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.webm", &empty_snapshot());
        assert!(out.contains(r#"src="clip.webm""#), "got: {}", out);
    }

    // --- width/height from TitleParams (Stage 1 lifted |WxH alias) ---

    #[test]
    fn video_emits_width_param_when_present() {
        let p = params_with(&[("kind", "video"), ("width", "640px")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(out.contains(r#"width="640px""#), "got: {}", out);
    }

    #[test]
    fn video_emits_height_param_when_present() {
        let p = params_with(&[("kind", "video"), ("width", "640px"), ("height", "360px")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(out.contains(r#"width="640px""#), "got: {}", out);
        assert!(out.contains(r#"height="360px""#), "got: {}", out);
    }

    #[test]
    fn video_no_width_or_height_attrs_when_snapshot_empty() {
        // Phase 2E: width/height priority is (1) TitleParams alias, then
        // (2) AssetSnapshot.dims lookup, then (3) omit. With no alias and
        // no snapshot entry, the synthesizer omits both — matching the
        // "no fallback dims" decision in the Phase 2E design (the regex's
        // 800x600 fallback was a long-standing band-aid; Phase 2E drops it).
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(!out.contains("width="), "got: {}", out);
        assert!(!out.contains("height="), "got: {}", out);
    }

    // --- HTML-escape contract ---

    #[test]
    fn video_src_is_html_escaped() {
        // & must escape to &amp; in attribute values. The URL Q&A is
        // contrived but exercises the html_escape pass.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "q&a.mp4", &empty_snapshot());
        assert!(out.contains(r#"src="q&amp;a.mp4""#), "got: {}", out);
    }

    // --- Phase 2E: regex-parity emission ---

    #[test]
    fn synth_video_mov_to_mp4() {
        // `![[clip.mov]]` → `src="clip.mp4"` (transcoded payload) plus
        // `data-placeholder-src="clip.mov"` (original src — what the
        // iframe-bridge listens to when transcode-ready fires). Mirrors
        // the regex contract at `placeholder.rs:440-490` so the post-pass
        // becomes a no-op for synthesizer output.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mov", &empty_snapshot());
        assert!(
            out.contains(r#"src="clip.mp4""#),
            "expected src= to point at transcoded mp4, got: {}",
            out
        );
        assert!(
            out.contains(r#"data-placeholder-src="clip.mov""#),
            "expected data-placeholder-src= to point at original mov, got: {}",
            out
        );
    }

    #[test]
    fn synth_video_poster_and_thumb() {
        // `poster` carries the thumbnail URL so the first frame paints
        // immediately; `data-thumb-src` is the iframe-bridge's swap-target
        // when `moss-asset-ready` fires (covers the case where the thumb
        // arrives after the page renders). Both reference `to_thumb(src)`,
        // mirroring `placeholder.rs:444-446`.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mov", &empty_snapshot());
        assert!(
            out.contains(r#"poster="clip.thumb.jpg""#),
            "expected poster= from to_thumb(src), got: {}",
            out
        );
        assert!(
            out.contains(r#"data-thumb-src="clip.thumb.jpg""#),
            "expected data-thumb-src= from to_thumb(src), got: {}",
            out
        );
    }

    #[test]
    fn synth_video_dims_from_snapshot() {
        // When TitleParams has no |WxH alias but the AssetSnapshot has
        // (w, h) for the source path, the synthesizer emits unitless
        // width/height. Matches the regex's `lookup.get(original_src)`
        // behaviour (placeholder.rs:449).
        let p = params_with(&[("kind", "video")]);
        let mut snap = AssetSnapshot::new();
        snap.dimensions.insert(PathBuf::from("clip.mov"), (1920, 1080));
        let out = synthesize_video_html(&p, "clip.mov", &snap);
        assert!(out.contains(r#"width="1920""#), "got: {}", out);
        assert!(out.contains(r#"height="1080""#), "got: {}", out);
    }

    #[test]
    fn synth_video_omits_zero_dims() {
        // `(0, 0)` in AssetSnapshot.dimensions is a sentinel for "unknown"
        // (e.g. probe failed). The regex emits `width="0" height="0"`
        // which is invalid; the synthesizer corrects that by omitting both.
        let p = params_with(&[("kind", "video")]);
        let mut snap = AssetSnapshot::new();
        snap.dimensions.insert(PathBuf::from("clip.mov"), (0, 0));
        let out = synthesize_video_html(&p, "clip.mov", &snap);
        assert!(
            !out.contains("width="),
            "(0, 0) sentinel must NOT produce width=, got: {}",
            out
        );
        assert!(
            !out.contains("height="),
            "(0, 0) sentinel must NOT produce height=, got: {}",
            out
        );
    }

    // Phase 2E v5 PR5 (2026-05-26) retired the Stage 3 regex post-pass; the
    // video synthesizer in this module is now the sole emitter of width /
    // height / poster / data-thumb-src / .mov→.mp4 src rewriting for
    // moss-emitted <video> tags. The two regex-parity tests at
    // `src-tauri/tests/video_synth_regex_parity.rs` were deleted alongside
    // the regex.

    // --- |loop ambient-video attribute (spec §3.6) -----------------------

    #[test]
    fn loop_keyword_emits_autoplay_muted_loop_playsinline_no_controls() {
        // `![[clip.mp4|loop]]` must emit the ambient playback set and must
        // NOT emit `controls` (the chrome-free ambient branch has no control bar).
        let p = params_with(&[("kind", "video"), ("loop", "1")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(out.contains(" autoplay"), "missing autoplay, got: {}", out);
        assert!(out.contains(" muted"), "missing muted, got: {}", out);
        assert!(out.contains(" loop"), "missing loop, got: {}", out);
        assert!(out.contains(" playsinline"), "missing playsinline, got: {}", out);
        assert!(!out.contains(" controls"), "controls must be absent on loop branch, got: {}", out);
    }

    #[test]
    fn loop_keyword_emits_data_type_and_data_loop() {
        // data-type="video" (drift fix) and data-loop (JS/CSS hook) must both
        // be present on the loop branch.
        let p = params_with(&[("kind", "video"), ("loop", "1")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(out.contains(r#"data-type="video""#), "missing data-type=video, got: {}", out);
        assert!(out.contains(" data-loop"), "missing data-loop attribute, got: {}", out);
    }

    #[test]
    fn loop_with_size_emits_width_height_and_loop_set() {
        // `![[clip.mp4|640x360 loop]]` must set width AND height AND the loop
        // ambient attribute set. The parser arm is tested end-to-end here via
        // the synthesizer (width/height come from TitleParams the parser sets).
        let p = params_with(&[("kind", "video"), ("loop", "1"), ("width", "640px"), ("height", "360px")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(out.contains(r#"width="640px""#), "missing width, got: {}", out);
        assert!(out.contains(r#"height="360px""#), "missing height, got: {}", out);
        assert!(out.contains(" autoplay"), "missing autoplay, got: {}", out);
        assert!(out.contains(" loop"), "missing loop, got: {}", out);
        assert!(!out.contains(" controls"), "controls must be absent on loop branch, got: {}", out);
    }

    #[test]
    fn data_type_video_emitted_on_default_branch() {
        // data-type="video" must be on the default (non-loop) branch too — this
        // is the drift fix bundled with the loop feature.
        let p = params_with(&[("kind", "video")]);
        let out = synthesize_video_html(&p, "clip.mp4", &empty_snapshot());
        assert!(out.contains(r#"data-type="video""#), "missing data-type=video on default branch, got: {}", out);
    }
}
