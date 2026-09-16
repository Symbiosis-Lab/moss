//! PDF embed synthesizer.
//!
//! Receives a [`TitleParams`] (Stage 2 dispatcher already parsed it), the
//! source URL, and an [`AssetSnapshot`]. Emits the final `<object>`-based
//! PDF embed HTML — preserving the exact byte shape moss-core's `PdfRenderer`
//! used to emit before Phase 0's Stage 1 migration.
//!
//! Pre-Phase-0 byte shape (extracted from `crates/moss-core/src/resolve/
//! embed_renderer.rs` at commit `689d975e9^`):
//!
//! ```text
//! <object class="moss-embed" data-type="pdf"{data-width?} type="application/pdf"
//!         data="{src}"{width?}{height?}>
//!   <a href="{src}">Download {stem}</a>
//! </object>
//! ```
//!
//! The `<object>` element (rather than `<iframe>`) was chosen by the original
//! `PdfRenderer` for keyboard-navigation parity and an inline download
//! fallback in browsers that can't render PDFs natively.

use crate::asset_snapshot::AssetSnapshot;
use crate::resolve::embed_renderer::{file_stem, html_escape_attr};
use crate::resolve::title_params::TitleParams;

/// Synthesize PDF embed HTML for `Tag::Link` with `moss:kind=pdf` title.
///
/// Params consumed (all optional except `kind=pdf` which the Stage 2
/// dispatcher already checked):
///
/// - `data-width` — canonical wrapper width (`body|wide|page|screen`), spec § P9
/// - `width` — HTML attribute pixel/percent/vh width (from `|WxH` alias sugar)
/// - `height` — HTML attribute pixel/percent/vh height (from `|WxH` alias sugar)
/// - `query` — URL query (appended as `?query` after the path)
/// - `fragment` — URL fragment (appended as `#fragment` — typically `page=5`)
///
/// `assets` is currently unused for PDF (no LQIP or dimension concerns) but
/// kept in the signature for symmetry with the other synthesizers and
/// future-proofing.
#[allow(unused_variables)]
pub fn synthesize_pdf_html(
    params: &TitleParams,
    src: &str,
    assets: &AssetSnapshot,
) -> String {
    // Reconstruct the embed URL from src + query + fragment.
    // Order is URL-canonical: path?query#fragment.
    let mut data_url = String::from(src);
    if let Some(q) = params.get("query") {
        data_url.push('?');
        data_url.push_str(q);
    }
    if let Some(f) = params.get("fragment") {
        data_url.push('#');
        data_url.push_str(f);
    }

    let data_width_attr = match params.get("data-width") {
        Some(w) => format!(r#" data-width="{}""#, html_escape_attr(w)),
        None => String::new(),
    };

    let html_width_attr = match params.get("width") {
        Some(w) => format!(r#" width="{}""#, html_escape_attr(w)),
        None => String::new(),
    };

    let html_height_attr = match params.get("height") {
        Some(h) => format!(r#" height="{}""#, html_escape_attr(h)),
        None => String::new(),
    };

    let name = file_stem(src);

    // <object> with inline download fallback for browsers that can't render PDFs.
    // Attribute order matches the pre-Phase-0 PdfRenderer byte shape exactly.
    format!(
        "<object class=\"moss-embed\" data-type=\"pdf\"{} type=\"application/pdf\" data=\"{}\"{}{}><a href=\"{}\">Download {}</a></object>",
        data_width_attr,
        html_escape_attr(&data_url),
        html_width_attr,
        html_height_attr,
        html_escape_attr(src),
        html_escape_attr(&name),
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
    fn pdf_basic_shape() {
        let p = params_with(&[("kind", "pdf")]);
        let out = synthesize_pdf_html(&p, "doc.pdf", &empty_snapshot());
        // Pre-Phase-0 byte shape: <object class="moss-embed" data-type="pdf" ...>
        assert!(
            out.contains(r#"<object class="moss-embed" data-type="pdf""#),
            "got: {}",
            out
        );
        assert!(
            out.contains(r#"type="application/pdf""#),
            "got: {}",
            out
        );
        // src is bound via `data="..."` on <object>, not `src=`.
        assert!(out.contains(r#"data="doc.pdf""#), "got: {}", out);
        // Inline <a> download fallback for browsers that can't render PDFs.
        assert!(
            out.contains(r#"<a href="doc.pdf">Download doc</a>"#),
            "got: {}",
            out
        );
    }

    #[test]
    fn pdf_with_height_param() {
        let p = params_with(&[("kind", "pdf"), ("width", "400px"), ("height", "800px")]);
        let out = synthesize_pdf_html(&p, "doc.pdf", &empty_snapshot());
        assert!(out.contains(r#"width="400px""#), "got: {}", out);
        assert!(out.contains(r#"height="800px""#), "got: {}", out);
    }

    #[test]
    fn pdf_escapes_url_specials() {
        let p = params_with(&[("kind", "pdf")]);
        let out = synthesize_pdf_html(
            &p,
            r#"file with "quotes".pdf"#,
            &empty_snapshot(),
        );
        // The `"` in the URL must be HTML-attribute-escaped so the emitted
        // `data="..."` attribute parses correctly. Escape lands in three
        // places: `data=` URL, `<a href=` URL, and the stem inside the
        // "Download ..." link text.
        assert!(
            out.contains(r#"data="file with &quot;quotes&quot;.pdf""#),
            "expected escaped data= attr, got: {}",
            out
        );
        assert!(
            out.contains(r#"href="file with &quot;quotes&quot;.pdf""#),
            "expected escaped href= attr, got: {}",
            out
        );
        // No raw, unescaped quote anywhere in attribute-value position.
        assert!(
            !out.contains(r#"data="file with ""#),
            "raw quote leaked into data= attr, got: {}",
            out
        );
    }

    #[test]
    fn pdf_query_and_fragment_appended_to_url() {
        // Standard PDF.js viewer convention: #page=5 jumps to page 5.
        let p = params_with(&[("kind", "pdf"), ("fragment", "page=5")]);
        let out = synthesize_pdf_html(&p, "report.pdf", &empty_snapshot());
        assert!(out.contains(r#"data="report.pdf#page=5""#), "got: {}", out);
        // The download fallback link points at the bare URL (no #page) so
        // it's a download, not a viewer-navigation request.
        assert!(out.contains(r#"<a href="report.pdf">"#), "got: {}", out);
    }

    #[test]
    fn pdf_query_before_fragment() {
        let p = params_with(&[
            ("kind", "pdf"),
            ("query", "version=2"),
            ("fragment", "page=5"),
        ]);
        let out = synthesize_pdf_html(&p, "report.pdf", &empty_snapshot());
        // URL-canonical order: path?query#fragment.
        assert!(
            out.contains(r#"data="report.pdf?version=2#page=5""#),
            "got: {}",
            out
        );
    }

    #[test]
    fn pdf_data_width_attr_when_present() {
        let p = params_with(&[("kind", "pdf"), ("data-width", "wide")]);
        let out = synthesize_pdf_html(&p, "doc.pdf", &empty_snapshot());
        assert!(out.contains(r#"data-width="wide""#), "got: {}", out);
    }

    #[test]
    fn pdf_no_data_width_when_absent() {
        let p = params_with(&[("kind", "pdf")]);
        let out = synthesize_pdf_html(&p, "doc.pdf", &empty_snapshot());
        // Themes target `:not([data-width])`; the attr must be absent by default.
        assert!(!out.contains("data-width"), "got: {}", out);
    }

    #[test]
    fn pdf_stem_strips_directory_and_extension() {
        let p = params_with(&[("kind", "pdf")]);
        let out = synthesize_pdf_html(&p, "papers/2024/big-report.pdf", &empty_snapshot());
        // Download fallback uses the file stem, not the full path.
        assert!(out.contains("Download big-report</a>"), "got: {}", out);
    }
}
