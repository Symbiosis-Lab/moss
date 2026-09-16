//! Generic bootstrap-state JSON extraction.
//!
//! Builders embed their content model as global JSON assignments inside
//! inline scripts (`$S.stores=...`, `window.ServerData = {...}`,
//! `__NEXT_DATA__ = {...}`, ...). This module parses one such assignment
//! generically: find the needle, stream-parse the first complete JSON value
//! after it. The convention is shared internet-wide; only the needle is
//! per-dialect.

/// Parse the first complete JSON value following `needle` in `html`.
///
/// serde_json stops at the end of the first complete value, so trailing
/// `;$S.next=...` (or any other script text) is simply left unconsumed.
/// Any parse failure returns `None` — callers fall back to other evidence.
pub(crate) fn extract_json_assignment(html: &str, needle: &str) -> Option<serde_json::Value> {
    let (_, after_needle) = html.split_once(needle)?;
    serde_json::Deserializer::from_str(after_needle)
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_after_needle() {
        let html = r#"<script>window.$S={};$S.conf={"a":1,"b":[2,3]};$S.next="x";</script>"#;
        let v = extract_json_assignment(html, "$S.conf=").expect("parses");
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"][1], 3);
    }

    #[test]
    fn stops_at_first_complete_value() {
        // The trailing `;$S.next=...` must not confuse the stream parser.
        let html = r#"$S.stores={"k":"v"};$S.next={"other":true};"#;
        let v = extract_json_assignment(html, "$S.stores=").expect("parses");
        assert_eq!(v["k"], "v");
        assert!(v.get("other").is_none());
    }

    #[test]
    fn truncated_value_returns_none() {
        let html = r#"$S.stores={"k":"v"#;
        assert!(extract_json_assignment(html, "$S.stores=").is_none());
    }

    #[test]
    fn missing_needle_returns_none() {
        assert!(extract_json_assignment("<html></html>", "window.ServerData =").is_none());
    }

    #[test]
    fn works_for_window_global_needles() {
        // The Readymag-shaped convention: a `window.<Name> =` assignment.
        let html = r#"<script>window.ServerData = {"mags":{"m1":{"title":"T"}}};</script>"#;
        let v = extract_json_assignment(html, "window.ServerData = ").expect("parses");
        assert_eq!(v["mags"]["m1"]["title"], "T");
    }
}
