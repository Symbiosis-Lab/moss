//! Audio embed synthesizer.
//!
//! Receives a [`TitleParams`] (Stage 2 dispatcher already parsed it), the
//! source URL, and an [`AssetSnapshot`]. Emits final `<audio>` HTML —
//! preserving the shape moss-core's `AudioRenderer` used to emit before
//! Phase 0's Stage 1 migration.
//!
//! Pre-Phase-0 reference shape (from `crates/moss-core/src/resolve/embed_renderer.rs`
//! at commit `8f9b6b55a`):
//!
//! ```html
//! <audio class="moss-embed moss-embed-audio" controls preload="metadata">
//!   <source src="{url}" type="{mime}">
//!   Your browser does not support the audio tag.
//! </audio>
//! ```
//!
//! The `AudioRenderer` always emitted `controls` and `preload="metadata"`,
//! with a `<source type="{mime}">` derived from the asset extension. Phase 1
//! preserves that exact byte shape; `TitleParams` (`controls`, `loop`,
//! `autoplay`, `muted`, `preload`) currently surface no behavioral overrides
//! because the markdown emitter does not author them — they are reserved for
//! future param plumbing without changing the dispatcher contract.

use crate::asset_snapshot::AssetSnapshot;
use crate::path_ext::path_extension_lower;
use crate::resolve::embed_renderer::html_escape_attr;
use crate::resolve::title_params::TitleParams;

/// Synthesize audio embed HTML for `Tag::Link` with `moss:kind=audio` title.
///
/// Always emits a `<audio>` element with `controls preload="metadata"` and
/// a `<source>` child carrying the URL and a MIME type derived from the
/// extension. Matches the pre-Phase-0 `AudioRenderer` byte shape one-for-one
/// so existing snapshot tests / fixtures remain valid after the Stage 2
/// dispatcher routes here.
///
/// Uses the canonical 4-char attribute escaper (`& < > "`); apostrophe is
/// safe inside `"..."` attributes per HTML5. The pre-Phase-0 path called
/// `moss_core::media::html_escape` (5 chars, also escapes `'` → `&#39;`),
/// which over-escaped attribute values relative to pdf / iframe / model.
/// This byte-shape change aligns audio/video with their siblings.
#[allow(unused_variables)]
pub fn synthesize_audio_html(
    params: &TitleParams,
    src: &str,
    assets: &AssetSnapshot,
) -> String {
    let ext = path_extension_lower(src);
    let mime = audio_mime_for_ext(&ext);
    let escaped = html_escape_attr(src);

    format!(
        "<audio class=\"moss-embed moss-embed-audio\" controls preload=\"metadata\"><source src=\"{}\" type=\"{}\">Your browser does not support the audio tag.</audio>",
        escaped, mime,
    )
}


/// Map a lowercased audio extension to the MIME emitted on `<source type="…">`.
///
/// Mirrors the table in moss-core's pre-Phase-0 `AudioRenderer` so the byte
/// shape of `type="…"` stays identical. Unknown extensions fall back to
/// `application/octet-stream` (browsers ignore the unknown type and probe
/// the response Content-Type — graceful degradation).
fn audio_mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "opus" => "audio/opus",
        _ => "application/octet-stream",
    }
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

    #[test]
    fn audio_basic_shape() {
        let p = params_with(&[("kind", "audio")]);
        let out = synthesize_audio_html(&p, "track.mp3", &empty_snapshot());
        assert!(out.contains("<audio"), "got: {}", out);
        assert!(out.contains(r#"src="track.mp3""#));
    }

    #[test]
    fn audio_with_controls() {
        let p = params_with(&[("kind", "audio"), ("controls", "true")]);
        let out = synthesize_audio_html(&p, "track.mp3", &empty_snapshot());
        assert!(out.contains("controls"));
    }

    #[test]
    fn audio_preserves_extension() {
        // Audio kinds: .mp3, .ogg, .wav, .m4a, .flac, ...
        let p = params_with(&[("kind", "audio")]);
        let mp3 = synthesize_audio_html(&p, "track.mp3", &empty_snapshot());
        let ogg = synthesize_audio_html(&p, "track.ogg", &empty_snapshot());
        assert!(mp3.contains(r#"src="track.mp3""#));
        assert!(ogg.contains(r#"src="track.ogg""#));
    }

    #[test]
    fn audio_escapes_url() {
        let p = params_with(&[("kind", "audio")]);
        let out = synthesize_audio_html(&p, r#"file with "quotes".mp3"#, &empty_snapshot());
        assert!(out.contains(r#"&quot;quotes&quot;"#), "got: {}", out);
    }

    #[test]
    fn audio_emits_pre_phase_0_class_and_attrs() {
        // Lock in the byte shape: class + controls + preload="metadata" + fallback text.
        let p = params_with(&[("kind", "audio")]);
        let out = synthesize_audio_html(&p, "track.mp3", &empty_snapshot());
        assert!(
            out.starts_with(r#"<audio class="moss-embed moss-embed-audio" controls preload="metadata">"#),
            "got: {}",
            out,
        );
        assert!(out.contains("Your browser does not support the audio tag."));
        assert!(out.ends_with("</audio>"));
    }

    #[test]
    fn audio_mime_per_extension() {
        let p = params_with(&[("kind", "audio")]);
        let snap = empty_snapshot();
        let cases = [
            ("track.mp3", "audio/mpeg"),
            ("track.wav", "audio/wav"),
            ("track.ogg", "audio/ogg"),
            ("track.flac", "audio/flac"),
            ("track.m4a", "audio/mp4"),
            ("track.opus", "audio/opus"),
        ];
        for (src, mime) in cases {
            let out = synthesize_audio_html(&p, src, &snap);
            let expected = format!(r#"type="{}""#, mime);
            assert!(out.contains(&expected), "src={}, want {}, got: {}", src, expected, out);
        }
    }

    #[test]
    fn audio_unknown_extension_falls_back_to_octet_stream() {
        let p = params_with(&[("kind", "audio")]);
        let out = synthesize_audio_html(&p, "mystery.xyz", &empty_snapshot());
        assert!(out.contains(r#"type="application/octet-stream""#), "got: {}", out);
    }

    #[test]
    fn audio_extension_lookup_ignores_query_and_fragment() {
        let p = params_with(&[("kind", "audio")]);
        let out = synthesize_audio_html(&p, "track.mp3?v=2#t=10", &empty_snapshot());
        // Extension still parsed as mp3 → audio/mpeg, full URL preserved as src.
        assert!(out.contains(r#"type="audio/mpeg""#), "got: {}", out);
        assert!(out.contains(r#"src="track.mp3?v=2#t=10""#), "got: {}", out);
    }
}
