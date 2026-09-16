//! Allowlist HTML sanitization for untrusted comment bodies.
//!
//! Comment HTML (Artalk `content_marked`, or raw `content`) is attacker-controlled
//! and is rendered both into baked SSR pages and the dehydrated store, so it MUST
//! be sanitized at ingest — never trust the upstream comment server's sanitization
//! (defense in depth). See ADR-025 §11 and `docs/reference/comment-system-design.md` §0/§11.
//!
//! The allowlist here MUST stay byte-for-byte equivalent to the TypeScript client
//! sanitizer (DOMPurify config in `frontend/site/comments/`): the cross-language
//! golden test renders the same fixture through both and asserts identical HTML,
//! so any divergence in allowed tags fails CI.

use std::collections::HashSet;

/// The comment-body tag allowlist. Shared contract with the TS DOMPurify config
/// (spec §11). Keep the two in sync; the golden test guards drift.
pub const ALLOWED_TAGS: &[&str] = &[
    "p", "a", "em", "strong", "code", "blockquote", "br", "ul", "ol", "li",
];

/// Sanitize untrusted comment HTML down to the comment allowlist.
///
/// - Only [`ALLOWED_TAGS`] survive; all other tags (and their attributes,
///   including `onerror`/`onclick` event handlers) are stripped.
/// - `<a href>` keeps only `http`/`https` URLs (so `javascript:` / `data:` are
///   dropped) and is forced to `rel="nofollow ugc noopener"`.
pub fn sanitize_comment_html(raw: &str) -> String {
    let tags: HashSet<&str> = ALLOWED_TAGS.iter().copied().collect();
    let schemes: HashSet<&str> = ["http", "https"].into_iter().collect();
    ammonia::Builder::default()
        .tags(tags)
        .url_schemes(schemes)
        .link_rel(Some("nofollow ugc noopener"))
        .clean(raw)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_and_event_handlers() {
        let out = sanitize_comment_html(
            "<p>hi</p><script>alert(1)</script><img src=x onerror=alert(1)>",
        );
        assert!(!out.contains("<script"), "script tag must be removed: {out}");
        assert!(!out.contains("onerror"), "event handler must be removed: {out}");
        assert!(!out.contains("<img"), "img not in allowlist: {out}");
        assert!(out.contains("<p>hi</p>"), "allowed content must survive: {out}");
    }

    #[test]
    fn drops_javascript_href_keeps_https_with_rel() {
        let out = sanitize_comment_html(
            r#"<a href="javascript:alert(1)">x</a><a href="https://example.com">ok</a>"#,
        );
        assert!(!out.contains("javascript:"), "js scheme must be dropped: {out}");
        assert!(out.contains("https://example.com"), "https link must survive: {out}");
        assert!(
            out.contains("rel=\"nofollow ugc noopener\""),
            "links must carry nofollow ugc noopener: {out}"
        );
    }

    #[test]
    fn keeps_allowlist_tags() {
        let out = sanitize_comment_html(
            "<blockquote><em>q</em></blockquote><ul><li>a</li></ul><code>c</code><strong>b</strong>",
        );
        for needle in ["<blockquote>", "<em>", "<ul>", "<li>", "<code>", "<strong>"] {
            assert!(out.contains(needle), "allowlist tag {needle} must survive: {out}");
        }
    }

    #[test]
    fn drops_disallowed_pre_and_h1() {
        // `pre`/`h1` are intentionally NOT in the allowlist (spec §11). Their text
        // survives, their tags do not.
        let out = sanitize_comment_html("<pre>code</pre><h1>title</h1>");
        assert!(!out.contains("<pre>"), "pre must be stripped: {out}");
        assert!(!out.contains("<h1>"), "h1 must be stripped: {out}");
        assert!(out.contains("code"), "inner text survives: {out}");
        assert!(out.contains("title"), "inner text survives: {out}");
    }
}
