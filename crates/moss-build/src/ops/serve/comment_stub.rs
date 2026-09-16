//! Preview-only Artalk stub: receives the comment form's POST during preview
//! and returns an Artalk-shaped success so the real client JS plays its full
//! success path — without any traffic to the production server (design §7).
//! Ephemeral by design: nothing is persisted; reload clears it.
//!
//! ## How the form is rerouted
//!
//! The primary mechanism is a serve-time rewrite in
//! `preview::iframe_bridge::rewrite_comment_form_server_url`: every HTML
//! response served by the preview server has its `data-server-url` attribute
//! replaced with `/__moss/comments` before reaching the browser. This means
//! even a morph-refreshed page always carries the stub URL.
//!
//! `preview::iframe_bridge` also injects a preview-only CSS tooltip
//! (`::after` on hover/focus) on the send button to indicate that submissions
//! stay local.
//!
//! The comment-preview-shim.ts script (injected by iframe_bridge before
//! `</body>`) is defense-in-depth: it re-applies the same rewrite client-side
//! for any paths that bypass the serve-time transform.
//!
//! Response shape: fields are at TOP LEVEL (no "data" wrapper).
//!
//! artalk.ts reads the parsed response body's top-level `content` field:
//!   `.then((data) => ... (data.content as string) || finalContent)`
//! The real Artalk v2 create endpoint (`POST /api/v2/comments`) also returns
//! the comment object at the top level of the JSON response body. A "data"
//! wrapper would make `data.content` undefined, causing the client to always
//! fall back to `finalContent` (raw unescaped user input) for `innerHTML` —
//! an unnecessary self-XSS vector in the preview iframe.
//!
//! Consumed response shape (verified against artalk.ts lines ~113–130):
//!   { id: number, content: string, date: string, nick: string,
//!     link: string, rid: number, is_pending: bool }

use axum::{response::IntoResponse, Json};

/// Pure core, unit-testable: Artalk-shaped success JSON, or None for an
/// invalid payload (empty content/nick → client shows its error state).
pub(crate) fn stub_response(payload: &serde_json::Value) -> Option<serde_json::Value> {
    let content = payload.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let nick = payload.get("nick").and_then(|v| v.as_str()).unwrap_or("");
    if content.trim().is_empty() || nick.trim().is_empty() {
        return None;
    }
    let escaped = content
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    Some(serde_json::json!({
        "id": 0,
        "content": format!("<p>{}</p>", escaped),
        "date": chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "nick": nick,
        "link": payload.get("link").and_then(|v| v.as_str()).unwrap_or(""),
        "rid": payload.get("rid").and_then(|v| v.as_i64()).unwrap_or(0),
        "is_pending": false
    }))
}

pub async fn handle_comment_stub(Json(payload): Json<serde_json::Value>) -> impl IntoResponse {
    match stub_response(&payload) {
        Some(body) => (axum::http::StatusCode::OK, Json(body)).into_response(),
        None => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "msg": "content and nick are required" })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_returns_top_level_fields() {
        // artalk.ts reads the parsed response body's TOP-LEVEL `content` field:
        //   `.then((data) => ... (data.content as string) || finalContent)`
        // A "data" wrapper would make data.content undefined, causing the client
        // to fall back to raw unescaped input for innerHTML (self-XSS in preview).
        let payload = serde_json::json!({
            "content": "hello from preview",
            "nick": "Tester",
            "email": "",
            "link": "",
            "page_key": "abc12345",
            "page_title": "Test",
            "site_name": "example.com"
        });
        let resp = stub_response(&payload).unwrap();
        // Fields are at top level — no "data" wrapper.
        assert_eq!(resp["nick"], "Tester");
        assert!(resp["content"].as_str().unwrap().contains("hello from preview"));
        assert!(resp["id"].is_number());
        // Verify there is no "data" wrapper (would cause the client fallback bug).
        assert!(resp.get("data").is_none(), "response must NOT have a 'data' wrapper");
    }

    #[test]
    fn stub_rejects_empty_content() {
        let payload = serde_json::json!({ "content": "", "nick": "T" });
        assert!(stub_response(&payload).is_none());
    }

    #[test]
    fn stub_escapes_html_in_content() {
        // Escaped content reaches innerHTML via the top-level `content` field.
        // With the data wrapper bug, the client would fall back to raw unescaped
        // input — this test verifies escaped content is in the right place.
        let payload = serde_json::json!({ "content": "<script>x</script>", "nick": "T" });
        let resp = stub_response(&payload).unwrap();
        let html = resp["content"].as_str().unwrap();
        assert!(!html.contains("<script>"), "content must be HTML-escaped; got: {html}");
        assert!(html.contains("&lt;script&gt;"), "angle brackets must be escaped; got: {html}");
    }

    #[test]
    fn stub_rejects_empty_nick() {
        let payload = serde_json::json!({ "content": "hello", "nick": "" });
        assert!(stub_response(&payload).is_none());
    }

    #[test]
    fn stub_propagates_rid() {
        let payload = serde_json::json!({ "content": "reply", "nick": "R", "rid": 42 });
        let resp = stub_response(&payload).unwrap();
        assert_eq!(resp["rid"], 42);
    }

    #[test]
    fn stub_defaults_rid_when_absent() {
        let payload = serde_json::json!({ "content": "hello", "nick": "T" });
        let resp = stub_response(&payload).unwrap();
        assert_eq!(resp["rid"], 0);
    }
}
