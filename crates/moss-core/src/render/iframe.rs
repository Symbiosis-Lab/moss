//! Iframe embed synthesizer.
//!
//! Receives a [`TitleParams`] (Stage 2 dispatcher already parsed it), the
//! source URL, and an [`AssetSnapshot`]. Emits final `<iframe>` HTML.
//!
//! **Sole iframe HTML emitter.** Two call sites converge here:
//!
//! 1. The Stage 2 dispatcher in `pipeline.rs`, after pulldown-cmark parses a
//!    `Tag::Link` carrying `moss:kind=iframe`.
//! 2. `src-tauri/src/build/folder_embed.rs`'s folder-as-iframe feature, which
//!    has no pulldown-cmark involvement and calls this synthesizer directly
//!    with an empty `TitleParams`.
//!
//! The pre-Phase-1 moss-core `IframeRenderer::render_to_html` escape valve
//! retired once folder_embed migrated (P2E prereq #2, 2026-05-25).

use crate::asset_snapshot::AssetSnapshot;
use crate::resolve::embed_renderer::html_escape_attr;
use crate::resolve::title_params::TitleParams;

/// Base wrapper class — kept in sync with moss-core's `CLASS_EMBED`
/// (`crates/moss-core/src/resolve/embed_renderer.rs`).
const CLASS_EMBED: &str = "moss-embed";

/// Synthesize iframe embed HTML for `Tag::Link` with `moss:kind=iframe` title.
///
/// Reads from `params`:
/// - `query` — appended to `src` after `?`
/// - `fragment` — appended to `src` after `#`
/// - `data-width` — canonical width token (`body | wide | page | screen`),
///   emitted as `data-width="…"` on the iframe
/// - `width` — pixel/percent dimension (e.g. `400px`, `100%`), emitted as
///   the HTML `width="…"` attribute
/// - `height` — pixel/percent dimension, emitted as `height="…"` attribute
/// - `title` — accessible name, emitted as `title="…"` attribute
///
/// Byte shape:
///
/// ```text
/// <iframe class="moss-embed" data-type="iframe"{data-width} src="{src}"{title}{width}{height} loading="lazy"></iframe>
/// ```
///
/// `assets` is currently unused — iframes target HTML files which moss does
/// not transform, so there is no variant URL to resolve. Kept in the
/// signature for parity with the other synthesizers (image, video, audio)
/// where it carries the variant manifest.
#[allow(unused_variables)]
pub fn synthesize_iframe_html(
    params: &TitleParams,
    src: &str,
    assets: &AssetSnapshot,
) -> String {
    // Reconstruct the iframe `src` from the URL slot + Stage-1-folded
    // `?query#fragment` params. pulldown-cmark would percent-encode `?`
    // and `#` if they stayed in the URL slot, so Stage 1 lifts them out;
    // Stage 2 puts them back here.
    let mut full_src = String::from(src);
    if let Some(q) = params.get("query") {
        full_src.push('?');
        full_src.push_str(q);
    }
    if let Some(f) = params.get("fragment") {
        full_src.push('#');
        full_src.push_str(f);
    }

    let data_provider_attr = match params.get("data-provider") {
        Some(p) if !p.is_empty() => format!(r#" data-provider="{}""#, html_escape_attr(p)),
        _ => String::new(),
    };

    let data_width_attr = match params.get("data-width") {
        Some(w) => format!(r#" data-width="{}""#, html_escape_attr(w)),
        None => String::new(),
    };

    let title_attr = match params.get("title") {
        Some(t) => format!(" title=\"{}\"", html_escape_attr(t)),
        None => String::new(),
    };

    let width_attr = match params.get("width") {
        Some(w) => format!(" width=\"{}\"", html_escape_attr(w)),
        None => String::new(),
    };

    let height_attr = match params.get("height") {
        Some(h) => format!(" height=\"{}\"", html_escape_attr(h)),
        None => String::new(),
    };

    let allow_attr = match params.get("allow") {
        Some(a) if !a.is_empty() => format!(" allow=\"{}\"", html_escape_attr(a)),
        _ => String::new(),
    };

    let sandbox_attr = match params.get("sandbox") {
        Some(s) if !s.is_empty() => format!(" sandbox=\"{}\"", html_escape_attr(s)),
        _ => String::new(),
    };

    let allowfullscreen_attr = if params.get("allowfullscreen") == Some("true") {
        " allowfullscreen"
    } else {
        ""
    };

    format!(
        "<iframe class=\"{}\" data-type=\"iframe\"{}{} src=\"{}\"{}{}{}{}{}{} loading=\"lazy\"></iframe>",
        CLASS_EMBED,
        data_provider_attr,
        data_width_attr,
        html_escape_attr(&full_src),
        title_attr,
        width_attr,
        height_attr,
        allow_attr,
        sandbox_attr,
        allowfullscreen_attr,
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

    #[test]
    fn iframe_basic_shape() {
        let p = params_with(&[("kind", "iframe")]);
        let out = synthesize_iframe_html(&p, "widget.html", &empty_snapshot());
        assert!(out.contains("<iframe"));
        assert!(out.contains(r#"src="widget.html""#));
        assert!(out.contains(r#"loading="lazy""#));
        assert!(out.contains(r#"class="moss-embed""#));
        assert!(out.contains(r#"data-type="iframe""#));
    }

    #[test]
    fn iframe_with_data_width() {
        let p = params_with(&[("kind", "iframe"), ("data-width", "wide")]);
        let out = synthesize_iframe_html(&p, "widget.html", &empty_snapshot());
        assert!(out.contains(r#"data-width="wide""#), "got: {}", out);
    }

    #[test]
    fn iframe_with_query_param() {
        let p = params_with(&[("kind", "iframe"), ("query", "k=v&x=y")]);
        let out = synthesize_iframe_html(&p, "widget.html", &empty_snapshot());
        assert!(
            out.contains(r#"src="widget.html?k=v"#)
                || out.contains(r#"src="widget.html?k=v&amp;x=y""#),
            "expected query in src, got: {}",
            out
        );
    }

    #[test]
    fn iframe_with_title() {
        let p = params_with(&[("kind", "iframe"), ("title", "My Widget")]);
        let out = synthesize_iframe_html(&p, "widget.html", &empty_snapshot());
        assert!(out.contains(r#"title="My Widget""#));
    }

    #[test]
    fn iframe_with_allow_attr() {
        let p = params_with(&[("allow", "autoplay; encrypted-media")]);
        let out = synthesize_iframe_html(&p, "x.html", &empty_snapshot());
        assert!(out.contains(r#"allow="autoplay; encrypted-media""#), "got: {out}");
    }

    #[test]
    fn iframe_with_sandbox_attr() {
        let p = params_with(&[("sandbox", "allow-scripts allow-same-origin")]);
        let out = synthesize_iframe_html(&p, "x.html", &empty_snapshot());
        assert!(out.contains(r#"sandbox="allow-scripts allow-same-origin""#), "got: {out}");
    }

    #[test]
    fn iframe_with_allowfullscreen() {
        let p = params_with(&[("allowfullscreen", "true")]);
        let out = synthesize_iframe_html(&p, "x.html", &empty_snapshot());
        assert!(out.contains("allowfullscreen"), "got: {out}");
    }

    #[test]
    fn iframe_without_allow_omits_attr() {
        let p = params_with(&[]);
        let out = synthesize_iframe_html(&p, "x.html", &empty_snapshot());
        assert!(!out.contains("allow="), "got: {out}");
        assert!(!out.contains("sandbox="), "got: {out}");
        assert!(!out.contains("allowfullscreen"), "got: {out}");
    }

    #[test]
    fn iframe_allow_empty_string_is_omitted() {
        // Empty string should be treated same as absent — no allow= attr emitted
        let p = params_with(&[("allow", "")]);
        let out = synthesize_iframe_html(&p, "x.html", &empty_snapshot());
        assert!(!out.contains("allow="), "got: {out}");
    }

    #[test]
    fn iframe_allowfullscreen_false_is_omitted() {
        // Only "true" emits the boolean attr; any other value is treated as absent
        let p = params_with(&[("allowfullscreen", "false")]);
        let out = synthesize_iframe_html(&p, "x.html", &empty_snapshot());
        assert!(!out.contains("allowfullscreen"), "got: {out}");
    }

}
