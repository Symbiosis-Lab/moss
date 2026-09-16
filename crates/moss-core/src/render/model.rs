//! 3D model-viewer embed synthesizer.
//!
//! Receives a [`TitleParams`] (Stage 2 dispatcher already parsed it), the
//! source URL, and an [`AssetSnapshot`]. Emits final `<model-viewer>` HTML —
//! preserving the shape moss-core's `ModelViewerRenderer` used to emit
//! before Phase 0's Stage 1 migration.
//!
//! Source byte shape (pre-Phase-0, see commit `689d975e9^`,
//! `crates/moss-core/src/resolve/embed_renderer.rs:723`):
//!
//! ```text
//! <model-viewer class="moss-embed" data-type="3d"{data-width} src="{src}"
//!   camera-controls auto-rotate touch-action="pan-y" loading="lazy"{style}>
//! </model-viewer>
//! ```
//!
//! `{data-width}` is ` data-width="VALUE"` (with leading space) when the
//! wrapper width is set (`body | wide | page | screen`), empty otherwise.
//! `{style}` is ` style="width:W"` or ` style="width:W;height:H"` when
//! sizing params are present, empty otherwise.
//!
//! Stage 2 reads the boolean flags (`camera-controls`, `auto-rotate`, `ar`)
//! from [`TitleParams`] — when the wikilink grammar surfaces them as
//! `param=true` they appear as bare attributes on the element.

use crate::asset_snapshot::AssetSnapshot;
use crate::resolve::embed_renderer::html_escape_attr;
use crate::resolve::title_params::TitleParams;

/// CSS class the pre-Phase-0 renderer placed on the `<model-viewer>` element.
/// Mirrors `CLASS_EMBED` in `crates/moss-core/src/resolve/embed_renderer.rs`.
const CLASS_EMBED: &str = "moss-embed";

/// Synthesize 3D model-viewer embed HTML for `Tag::Link` with `moss:kind=3d` title.
///
/// Byte shape matches the pre-Phase-0 `ModelViewerRenderer::render` emission
/// in moss-core: `camera-controls` and `auto-rotate` are emitted by default
/// (suppress with `param=false`), and `ar` is opt-in (`ar=true`). This
/// preserves the implicit defaults of existing `![[scene.glb]]` wikilinks
/// while still allowing explicit override via title params.
#[allow(unused_variables)]
pub fn synthesize_model_html(
    params: &TitleParams,
    src: &str,
    assets: &AssetSnapshot,
) -> String {
    let data_width = match params.get("data-width") {
        Some(w) => format!(r#" data-width="{}""#, html_escape_attr(w)),
        None => String::new(),
    };

    let flags = collect_flag_attrs(params);
    let style = collect_style_attr(params);

    format!(
        r#"<model-viewer class="{class}" data-type="3d"{data_width} src="{src}"{flags} touch-action="pan-y" loading="lazy"{style}></model-viewer>"#,
        class = CLASS_EMBED,
        data_width = data_width,
        src = html_escape_attr(src),
        flags = flags,
        style = style,
    )
}

/// Concatenate the boolean-flag attribute fragment.
///
/// `camera-controls` and `auto-rotate` default ON to match the pre-Phase-0
/// `ModelViewerRenderer` byte shape — they emit unless `param=false` opts
/// out. `ar` defaults OFF (opt-in only). Each flag becomes a bare HTML
/// attribute (`camera-controls`, not `camera-controls="true"`).
fn collect_flag_attrs(params: &TitleParams) -> String {
    // (name, default_on)
    const FLAGS: &[(&str, bool)] = &[
        ("camera-controls", true),
        ("auto-rotate", true),
        ("ar", false),
    ];
    let mut out = String::new();
    for (flag, default_on) in FLAGS {
        let emit = match params.get(flag) {
            Some("true") => true,
            Some("false") => false,
            _ => *default_on,
        };
        if emit {
            out.push(' ');
            out.push_str(flag);
        }
    }
    out
}

/// Build the inline `style="width:...;height:..."` fragment.
///
/// Unlike iframe/video, `<model-viewer>` consumes CSS length values via
/// inline style (the element ignores HTML `width=`/`height=` attributes).
/// Mirrors `model_viewer_style` in the pre-Phase-0 renderer.
fn collect_style_attr(params: &TitleParams) -> String {
    let w = params.get("width");
    let h = params.get("height");
    match (w, h) {
        (Some(w), Some(h)) => format!(
            r#" style="width:{};height:{}""#,
            html_escape_attr(w),
            html_escape_attr(h),
        ),
        (Some(w), None) => format!(r#" style="width:{}""#, html_escape_attr(w)),
        (None, Some(h)) => format!(r#" style="height:{}""#, html_escape_attr(h)),
        (None, None) => String::new(),
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
    fn model_basic_shape() {
        let p = params_with(&[("kind", "3d")]);
        let out = synthesize_model_html(&p, "scene.glb", &empty_snapshot());
        assert!(out.contains("<model-viewer"), "got: {}", out);
        assert!(out.contains(r#"src="scene.glb""#));
    }

    #[test]
    fn model_with_camera_controls() {
        let p = params_with(&[("kind", "3d"), ("camera-controls", "true")]);
        let out = synthesize_model_html(&p, "scene.glb", &empty_snapshot());
        assert!(out.contains("camera-controls"));
    }

    #[test]
    fn model_with_auto_rotate() {
        let p = params_with(&[("kind", "3d"), ("auto-rotate", "true")]);
        let out = synthesize_model_html(&p, "scene.glb", &empty_snapshot());
        assert!(out.contains("auto-rotate"));
    }

    #[test]
    fn model_with_ar() {
        let p = params_with(&[("kind", "3d"), ("ar", "true")]);
        let out = synthesize_model_html(&p, "scene.glb", &empty_snapshot());
        assert!(out.contains(" ar"));
    }

    #[test]
    fn model_escapes_url() {
        let p = params_with(&[("kind", "3d")]);
        let out = synthesize_model_html(&p, r#"scene with "spaces".glb"#, &empty_snapshot());
        // The src attribute must HTML-escape quotes.
        assert!(
            !out.contains(r#"src="scene with """#),
            "raw quote not escaped, got: {}",
            out
        );
    }

    // --- Additional byte-shape pins (preserve pre-Phase-0 ModelViewerRenderer shape) ---

    #[test]
    fn model_emits_moss_embed_class_and_data_type() {
        let p = params_with(&[("kind", "3d")]);
        let out = synthesize_model_html(&p, "x.glb", &empty_snapshot());
        assert!(
            out.contains(r#"class="moss-embed" data-type="3d""#),
            "got: {}",
            out
        );
    }

    #[test]
    fn model_emits_touch_action_and_loading() {
        // `touch-action="pan-y"` and `loading="lazy"` are always emitted —
        // they preserve the pre-Phase-0 byte shape.
        let p = params_with(&[("kind", "3d")]);
        let out = synthesize_model_html(&p, "x.glb", &empty_snapshot());
        assert!(out.contains(r#"touch-action="pan-y""#), "got: {}", out);
        assert!(out.contains(r#"loading="lazy""#), "got: {}", out);
    }

    #[test]
    fn model_emits_width_style() {
        // Stage 1's `model_viewer_extra_params` folds `|400` aliases into
        // `width=400px`; Stage 2 re-projects to inline CSS.
        let p = params_with(&[("kind", "3d"), ("width", "400px")]);
        let out = synthesize_model_html(&p, "x.glb", &empty_snapshot());
        assert!(out.contains(r#"style="width:400px""#), "got: {}", out);
    }

    #[test]
    fn model_emits_width_and_height_style() {
        let p = params_with(&[("kind", "3d"), ("width", "400px"), ("height", "400px")]);
        let out = synthesize_model_html(&p, "x.glb", &empty_snapshot());
        assert!(
            out.contains(r#"style="width:400px;height:400px""#),
            "got: {}",
            out
        );
    }

    #[test]
    fn model_emits_data_width_wrapper_attr() {
        // Wrapper-width tokens (`body | wide | page | screen`) ride the
        // `data-width=` attribute, matching `width_attr` in moss-core common.
        let p = params_with(&[("kind", "3d"), ("data-width", "wide")]);
        let out = synthesize_model_html(&p, "x.glb", &empty_snapshot());
        assert!(out.contains(r#"data-width="wide""#), "got: {}", out);
    }

    #[test]
    fn model_suppresses_default_flag_when_param_false() {
        // `camera-controls` is on by default; `param=false` suppresses it.
        let p = params_with(&[("kind", "3d"), ("camera-controls", "false")]);
        let out = synthesize_model_html(&p, "x.glb", &empty_snapshot());
        assert!(!out.contains("camera-controls"), "got: {}", out);
        // auto-rotate still emits (its default also true, but not suppressed).
        assert!(out.contains("auto-rotate"), "got: {}", out);
    }

    #[test]
    fn model_defaults_emit_camera_controls_and_auto_rotate() {
        // Pre-Phase-0 parity: bare `![[scene.glb]]` (no flag params) must
        // still emit camera-controls and auto-rotate. `ar` stays opt-in.
        let p = params_with(&[("kind", "3d")]);
        let out = synthesize_model_html(&p, "x.glb", &empty_snapshot());
        assert!(out.contains("camera-controls"), "got: {}", out);
        assert!(out.contains("auto-rotate"), "got: {}", out);
        assert!(!out.contains(" ar"), "ar must stay opt-in, got: {}", out);
    }

    #[test]
    fn model_closes_tag() {
        let p = params_with(&[("kind", "3d")]);
        let out = synthesize_model_html(&p, "x.glb", &empty_snapshot());
        assert!(out.ends_with("></model-viewer>"), "got: {}", out);
    }
}
