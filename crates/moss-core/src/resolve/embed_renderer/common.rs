//! Shared helpers for typed embed renderers.
//!
//! These live here (not in each renderer) because the pdf, audio, video and
//! 3d renderers all parse `|WxH` sizing into HTML attrs and escape attribute
//! values. One copy, many callers.

use super::Sizing;

/// Parse `|WxH` into HTML `width="..."` and `height="..."` attribute strings
/// (each with a leading space). Empty strings if alias is missing or not a
/// sizing hint.
///
/// Sizing travels as title-attribute params (`width=…` / `height=…`), so
/// this helper currently has no caller.
#[allow(dead_code)]
pub(super) fn dim_attrs(alias: Option<&str>) -> (String, String) {
    let Some(a) = alias else {
        return (String::new(), String::new());
    };
    match Sizing::parse(a) {
        Some(Sizing::Width(w)) => (format!(" width=\"{}\"", w.to_css()), String::new()),
        Some(Sizing::Box(w, h)) => (
            format!(" width=\"{}\"", w.to_css()),
            format!(" height=\"{}\"", h.to_css()),
        ),
        None => (String::new(), String::new()),
    }
}

/// Extract filename stem (no directory, no extension). Used by renderers that
/// want a human-readable label from a path.
///
/// `pub` (not `pub(super)`) so the canonical version is reachable from the
/// `crates/moss-core/src/render/*.rs` synthesizers. Before promotion,
/// `render/pdf.rs` carried its own copy with a `// Mirrors file_stem in …
/// (which is pub(super) and therefore not reachable from here)` comment;
/// see the polish-pass plan Item C1.
pub fn file_stem(path: &str) -> String {
    let filename = path.rsplit('/').next().unwrap_or(path);
    match filename.rsplit_once('.') {
        Some((stem, _ext)) if !stem.is_empty() => stem.to_string(),
        _ => filename.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_stem() {
        assert_eq!(file_stem("photo.jpg"), "photo");
        assert_eq!(file_stem("dir/photo.jpg"), "photo");
        assert_eq!(file_stem("noext"), "noext");
        assert_eq!(file_stem(".dotfile"), ".dotfile");
    }

    #[test]
    fn test_dim_attrs_none() {
        assert_eq!(dim_attrs(None), (String::new(), String::new()));
    }

    #[test]
    fn test_dim_attrs_width_only() {
        let (w, h) = dim_attrs(Some("400"));
        assert_eq!(w, " width=\"400px\"");
        assert_eq!(h, "");
    }

    #[test]
    fn test_dim_attrs_box() {
        let (w, h) = dim_attrs(Some("100%x600"));
        assert_eq!(w, " width=\"100%\"");
        assert_eq!(h, " height=\"600px\"");
    }
}
