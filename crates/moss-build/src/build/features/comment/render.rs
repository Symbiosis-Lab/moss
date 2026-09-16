//! Comment rendering: HTML generation for threaded comment sections.

use std::collections::HashMap;

use serde::Serialize;

use super::{CommentI18n, NormalizedComment};
use crate::build::features::{html_escape, inactive_label, CHECK_SVG};
use crate::i18n::{t, Language};

/// Format a comment count string using i18n.
pub fn format_comment_count(count: usize, i18n: &CommentI18n) -> String {
    match count {
        0 => i18n.comment_count_zero.to_string(),
        1 => i18n.comment_count_one.to_string(),
        n => i18n.comment_count_many.replace("{}", &n.to_string()),
    }
}

/// Build a threaded comment tree from a flat list.
/// Returns (top-level comments, replies map: (source, parent_id) -> [child comments])
/// The composite key prevents id collisions across sources.
pub fn build_thread_tree(
    comments: &[NormalizedComment],
) -> (
    Vec<&NormalizedComment>,
    HashMap<(&str, &str), Vec<&NormalizedComment>>,
) {
    let mut top_level = Vec::new();
    let mut replies: HashMap<(&str, &str), Vec<&NormalizedComment>> = HashMap::new();

    for comment in comments {
        if let Some(ref parent_id) = comment.reply_to_id {
            replies
                .entry((comment.source.as_str(), parent_id.as_str()))
                .or_default()
                .push(comment);
        } else {
            top_level.push(comment);
        }
    }

    (top_level, replies)
}

/// Format a date string for display.
/// Input: ISO 8601 (e.g., "2026-03-24 11:36:57" or "2026-03-24T11:36:57Z")
/// Output: localized date (e.g., "2026年3月24日" for zh, "Mar 24, 2026" for en)
pub fn format_date(date_str: &str, lang: Language) -> String {
    // Parse year, month, day from the date string
    let clean = date_str.replace('T', " ");
    let parts: Vec<&str> = clean.split(|c: char| c == '-' || c == ' ').collect();
    if parts.len() < 3 {
        return date_str.to_string();
    }

    let year = parts[0];
    let month: u32 = parts[1].parse().unwrap_or(1);
    let day: u32 = parts[2].parse().unwrap_or(1);

    if lang.is_cjk() {
        format!("{}年{}月{}日", year, month, day)
    } else {
        let month_name = match month {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            12 => "Dec",
            _ => "???",
        };
        format!("{} {}, {}", month_name, day, year)
    }
}

/// Pick the syndicated URL whose host contains the comment source name.
/// "matters" matches matters.town / matters.icu; "douban" matches douban.com.
/// Host-contains-source is the contract: source names are file stems and
/// syndication targets are first-party platform domains.
/// `url::Url::parse` lowercases hosts, so matching is case-insensitive.
pub(super) fn syndicated_link_for_source(syndicated: &[String], source: &str) -> Option<String> {
    syndicated
        .iter()
        .find(|u| {
            url::Url::parse(u)
                .ok()
                .and_then(|p| p.host_str().map(|h| h.contains(source)))
                .unwrap_or(false)
        })
        .cloned()
}

/// Human-readable "reply on X" label per source, localized.
fn source_reply_label(source: &str, lang: Language) -> String {
    let display = match source {
        "matters" => "Matters",
        // zh: 豆瓣 (no script mixing, both scripts); en: "Douban" (not "豆瓣 ↗").
        "douban" => {
            if lang.is_cjk() {
                "豆瓣"
            } else {
                "Douban"
            }
        }
        other => other,
    };
    t(lang, "comment_reply_on").replace("{}", display)
}

/// One comment in the dehydrated store embedded in the baked HTML. This is the
/// canonical wire shape shared with the client (camelCase for JS): the client
/// hydrates its store from these and normalizes live-fetched comments into the
/// same shape, so reconcile-by-id works across baked + fresh. ADR-025.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DehydratedComment<'a> {
    id: &'a str,
    source: &'a str,
    author: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    author_url: Option<&'a str>,
    content: &'a str,
    date: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DehydratedStore<'a> {
    /// Content watermark: the newest baked comment's `created_at` (deterministic;
    /// empty when there are no comments). NOT build time — a wall-clock value here
    /// makes every build's HTML differ and re-uploads the whole site each deploy.
    /// Advisory only — the client's primary comment dedup is `artalkHighWaterId`;
    /// retained for the stale-while-revalidate watermark design.
    watermark: &'a str,
    site_name: &'a str,
    page_key: &'a str,
    /// Max Artalk comment id known to the build for this page, computed from
    /// the FULL synced set BEFORE moderation removes hidden comments. The
    /// client appends a fetched Artalk comment only when its id > this value,
    /// preventing hidden comments from being re-appended on revalidate
    /// (resurrection bug). Zero means "no Artalk comments at bake time" and
    /// the client falls back to the id-not-in-DOM dedup only.
    artalk_high_water_id: i64,
    comments: Vec<DehydratedComment<'a>>,
}

/// Build the dehydrated comment store embedded as a JSON island so the client
/// can seed its in-memory store instantly (no fetch wait) and then
/// stale-while-revalidate against the live server. The data mirrors exactly the
/// comments the SSR list was rendered from.
///
/// Comment `content` is HTML, so the serialized JSON is embedded with `<`, `>`,
/// `&` escaped to `\uXXXX`: inside `<script type="application/json">` a literal
/// `</script>` in a comment would otherwise terminate the element early (a
/// script-tag JSON-embedding XSS). `JSON.parse` decodes the escapes transparently.
fn render_dehydrated_store(
    comments: &[NormalizedComment],
    site_name: &str,
    page_key: &str,
    lang: Language,
    artalk_high_water_id: i64,
) -> String {
    // Content-derived watermark: the newest baked comment's timestamp. Deterministic
    // for a fixed comment set, so the rendered bytes (and the deploy manifest) are
    // stable build-over-build. Empty when there are no comments. Replaces the former
    // `chrono::Utc::now()` build-time value, which changed every build and re-uploaded
    // the whole site every deploy (2026-06-16 deploy-churn root cause).
    let watermark = comments
        .iter()
        .map(|c| c.created_at.as_str())
        .max()
        .unwrap_or("");
    let store = DehydratedStore {
        watermark,
        site_name,
        page_key,
        artalk_high_water_id,
        comments: comments
            .iter()
            .map(|c| DehydratedComment {
                id: &c.id,
                source: &c.source,
                author: c.author.label(lang),
                // Same http(s)-only gate as the SSR <a href> — never ship a
                // javascript:/data: URL the client would render as a link.
                author_url: c.author.url.as_deref().filter(|u| is_safe_http_url(u)),
                content: &c.content,
                date: &c.created_at,
                reply_to: c.reply_to_id.as_deref(),
            })
            .collect(),
    };
    let json = serde_json::to_string(&store).unwrap_or_else(|_| "{}".to_string());
    let safe = json
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    format!(
        "    <script type=\"application/json\" id=\"moss-comments-data\">{}</script>\n",
        safe
    )
}

/// Render a complete comment section HTML for an article.
///
/// `page_key` is the canonical article uid for the current contract.
/// `syndicated` — URLs where this article was also published (e.g. matters.town,
/// douban.com). Used to render "Reply on X ↗" link-outs for non-Artalk comments
/// instead of in-page reply buttons.
pub fn render_comment_section(
    comments: &[NormalizedComment],
    server_url: &str,
    site_name: &str,
    page_key: &str,
    page_title: &str,
    lang: Language,
    syndicated: &[String],
    artalk_high_water_id: i64,
) -> String {
    let i18n = super::get_i18n(lang);
    let count_text = format_comment_count(comments.len(), &i18n);
    let (top_level, replies) = build_thread_tree(comments);
    let active = !server_url.is_empty();

    let mut html = String::new();
    if active {
        html.push_str(
            "<section class=\"moss-comments\" id=\"moss-comments\">\n",
        );
    } else {
        // aria-label, not title=: the inactive section is pointer-events:none
        // in preview and display:none once published, so a native tooltip can
        // never show — the state only needs to reach the accessibility tree.
        html.push_str(&format!(
            "<section class=\"moss-comments moss-service-inactive\" id=\"moss-comments\" aria-label=\"{}\">\n",
            inactive_label(lang),
        ));
    }
    html.push_str("  <details>\n");

    // Summary toggle
    html.push_str(&format!(
        "    <summary class=\"comments-toggle\"><svg class=\"comments-icon\" xmlns=\"http://www.w3.org/2000/svg\" width=\"18\" height=\"18\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092 10 10 0 1 0-4.777-4.719\"/></svg><span>{}</span><svg class=\"comments-chevron\" xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\"><path d=\"m9 18 6-6-6-6\"/></svg></summary>\n",
        html_escape(&count_text)
    ));

    // Comment form
    html.push_str("    <div class=\"comment-form-slot\" id=\"default-form-slot\">");
    html.push_str(&format!(
        "<form class=\"comment-form\" data-state=\"idle\" id=\"moss-comment-form\" data-server-url=\"{}\" data-site-name=\"{}\" data-page-key=\"{}\" data-page-title=\"{}\">\n",
        html_escape(server_url), html_escape(site_name), html_escape(page_key), html_escape(page_title)
    ));
    html.push_str(&format!(
        "  <textarea id=\"moss-comment-text\" name=\"content\" required rows=\"2\" placeholder=\"{}\"></textarea>\n",
        html_escape(i18n.placeholder)
    ));
    html.push_str("  <div class=\"comment-form-meta\">\n");
    html.push_str(&format!(
        "    <input type=\"text\" name=\"name\" class=\"comment-field\" placeholder=\"{}\" autocomplete=\"name\" required>\n",
        html_escape(i18n.name_label)
    ));
    html.push_str(&format!(
        "    <input type=\"email\" name=\"email\" class=\"comment-field\" placeholder=\"{}\" autocomplete=\"email\" required>\n",
        html_escape(i18n.email_label)
    ));
    html.push_str(&format!(
        "    <input type=\"text\" name=\"link\" class=\"comment-field\" placeholder=\"{}\" autocomplete=\"url\">\n",
        html_escape(i18n.website_label)
    ));
    html.push_str(&format!(
        "    <div class=\"moss-btn-slot\"><button type=\"submit\" class=\"moss-btn comment-form-submit\" aria-busy=\"false\"><span class=\"moss-btn__label\">{reply}</span><span class=\"moss-btn__spinner\" aria-hidden=\"true\"></span><span class=\"moss-btn__check\" aria-hidden=\"true\">{check_svg}</span></button></div>\n",
        reply = html_escape(i18n.reply_btn),
        check_svg = CHECK_SVG,
    ));
    html.push_str("  </div>\n");
    html.push_str("  <div class=\"comment-form-status\" id=\"moss-comment-status\" aria-live=\"assertive\" aria-atomic=\"true\"></div>\n");
    html.push_str("</form>\n</div>\n");

    // Comment list
    html.push_str("    <ol class=\"comment-list\">\n");
    for comment in &top_level {
        render_comment_item(&mut html, comment, &replies, lang, &i18n, 6, syndicated);
    }
    html.push_str("    </ol>\n");

    // Dehydrated store: the exact comments the SSR list was rendered from, so the
    // client hydrates its in-memory store instantly (no fetch wait) and then
    // stale-while-revalidates against the live server. ADR-025. Live only.
    if active {
        html.push_str(&render_dehydrated_store(
            comments, site_name, page_key, lang, artalk_high_water_id,
        ));
    }

    // Inline client-side JS for comment submission (Artalk is the only provider).
    html.push_str(&format!(
        "    <script>{}</script>\n",
        include_str!("../../../assets/js/comments-artalk.js")
    ));

    html.push_str("  </details>\n");
    html.push_str("</section>");
    html
}

/// True only for `http://` / `https://` URLs. Gates `<a href>` emission —
/// `html_escape` does NOT strip dangerous URI schemes (`javascript:`, `data:`),
/// so a user-supplied website field must be scheme-checked before becoming a link.
fn is_safe_http_url(url: &str) -> bool {
    let u = url.trim_start();
    u.starts_with("https://") || u.starts_with("http://")
}

/// Render a single comment item (recursive for replies).
fn render_comment_item(
    html: &mut String,
    comment: &NormalizedComment,
    replies: &HashMap<(&str, &str), Vec<&NormalizedComment>>,
    lang: Language,
    i18n: &CommentI18n,
    indent: usize,
    syndicated: &[String],
) {
    let pad = " ".repeat(indent);
    let author_label = comment.author.label(lang);
    let date_display = format_date(&comment.created_at, lang);

    html.push_str(&format!(
        "{}<li class=\"comment-item\" id=\"comment-{}-{}\" data-comment-source=\"{}\" data-comment-id=\"{}\">\n",
        pad,
        html_escape(&comment.source),
        html_escape(&comment.id),
        html_escape(&comment.source),
        html_escape(&comment.id)
    ));
    html.push_str(&format!("{}  <div class=\"comment-header\">\n", pad));

    // Author (with link only for http(s) URLs — see is_safe_http_url).
    if let Some(url) = comment.author.url.as_deref().filter(|u| is_safe_http_url(u)) {
        html.push_str(&format!(
            "{}    <a href=\"{}\" class=\"comment-author\" rel=\"nofollow ugc noopener\" target=\"_blank\">{}</a>\n",
            pad, html_escape(url), html_escape(author_label)
        ));
    } else {
        html.push_str(&format!(
            "{}    <span class=\"comment-author\">{}</span>\n",
            pad,
            html_escape(author_label)
        ));
    }

    html.push_str(&format!(
        "{}    <time class=\"comment-date\" datetime=\"{}\">{}</time>\n",
        pad,
        html_escape(&comment.created_at),
        html_escape(&date_display)
    ));

    // Reply affordance: Artalk comments get an in-page reply button;
    // syndicated-source comments get a link-out to the original platform
    // (if a matching syndicated URL is available); otherwise no affordance.
    if comment.source == "artalk" {
        // "↩\u{FE0E}": U+21A9 + VARIATION SELECTOR-15 forces text presentation,
        // so iOS/Android render a plain monochrome arrow, not a colored emoji.
        html.push_str(&format!(
            "{}    <button type=\"button\" class=\"comment-reply-btn\" data-reply-id=\"{}\" data-reply-name=\"{}\">{}</button>\n",
            pad, html_escape(&comment.id), html_escape(author_label), format_args!("↩\u{FE0E} {}", i18n.reply_btn)
        ));
    } else if let Some(url) = syndicated_link_for_source(syndicated, &comment.source) {
        let label = source_reply_label(&comment.source, lang);
        html.push_str(&format!(
            "{}    <a class=\"comment-source-link\" href=\"{}\" target=\"_blank\" rel=\"noopener nofollow\">{}</a>\n",
            pad, html_escape(&url), html_escape(&label)
        ));
    }

    html.push_str(&format!("{}  </div>\n", pad));

    // Comment body — `content` is sanitized at ingest (sync_remote) AND at
    // render-load (normalize_social_comment) via the allowlist sanitizer, so it is
    // safe HTML here. Do NOT html_escape() — that double-escapes sanitized HTML.
    // (ADR-025 §11.)
    html.push_str(&format!(
        "{}  <div class=\"comment-body\">{}</div>\n",
        pad, comment.content
    ));

    // Nested replies — keyed by (source, id) to prevent cross-source nesting.
    if let Some(children) = replies.get(&(comment.source.as_str(), comment.id.as_str())) {
        html.push_str(&format!("{}  <ol class=\"comment-replies\">\n", pad));
        for child in children {
            render_comment_item(html, child, replies, lang, i18n, indent + 4, syndicated);
        }
        html.push_str(&format!("{}  </ol>\n", pad));
    }

    html.push_str(&format!("{}</li>\n", pad));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::features::comment::{get_i18n, CommentAuthor};

    fn sample_comments() -> Vec<NormalizedComment> {
        vec![
            NormalizedComment {
                id: "8".into(),
                source: "artalk".into(),
                content: "orz你们全家都手太巧了。".into(),
                created_at: "2026-03-24 11:36:57".into(),
                author: CommentAuthor {
                    display_name: Some("Marcus".into()),
                    name: Some("Marcus".into()),
                    url: None,
                },
                reply_to_id: None,
                state: None,
            },
            NormalizedComment {
                id: "9".into(),
                source: "artalk".into(),
                content: "自己设计装修确实会很增加幸福感。".into(),
                created_at: "2026-03-25 08:00:00".into(),
                author: CommentAuthor {
                    display_name: Some("刘果".into()),
                    name: Some("刘果".into()),
                    url: None,
                },
                reply_to_id: Some("8".into()),
                state: None,
            },
        ]
    }

    // --- I18n tests ---

    #[test]
    fn test_i18n_zh_hans() {
        let i18n = get_i18n(Language::ZhHans);
        assert_eq!(i18n.placeholder, "留下你的想法");
        assert_eq!(i18n.reply_btn, "回复");
        assert_eq!(i18n.email_label, "邮箱");
    }

    #[test]
    fn test_i18n_zh_hant_uses_traditional_script() {
        // Regression: zh-hant sites used to get the Simplified strings
        // because get_i18n bucketed on a bare `starts_with("zh")`.
        let i18n = get_i18n(Language::ZhHant);
        assert_eq!(i18n.reply_btn, "回覆");
        assert_eq!(i18n.email_label, "電子郵件");
        assert_eq!(i18n.website_label, "網站（選填）");
        assert_eq!(i18n.comment_count_many, "{}則留言");
    }

    #[test]
    fn test_i18n_en() {
        let i18n = get_i18n(Language::En);
        assert_eq!(i18n.placeholder, "Leave your thoughts");
        assert_eq!(i18n.reply_btn, "Reply");
    }

    // --- Comment count formatting ---

    #[test]
    fn test_format_count_zh_zero() {
        let i18n = get_i18n(Language::ZhHans);
        assert_eq!(format_comment_count(0, &i18n), "评论");
    }

    #[test]
    fn test_format_count_en_zero() {
        let i18n = get_i18n(Language::En);
        assert_eq!(format_comment_count(0, &i18n), "Comments");
    }

    #[test]
    fn test_format_count_zh_many() {
        let i18n = get_i18n(Language::ZhHans);
        assert_eq!(format_comment_count(5, &i18n), "5条评论");
    }

    #[test]
    fn test_format_count_zh_hant_many() {
        let i18n = get_i18n(Language::ZhHant);
        assert_eq!(format_comment_count(5, &i18n), "5則留言");
    }

    #[test]
    fn test_format_count_en_one() {
        let i18n = get_i18n(Language::En);
        assert_eq!(format_comment_count(1, &i18n), "1 comment");
    }

    #[test]
    fn test_format_count_en_many() {
        let i18n = get_i18n(Language::En);
        assert_eq!(format_comment_count(3, &i18n), "3 comments");
    }

    // --- Thread tree building ---

    #[test]
    fn test_build_thread_tree_separates_top_and_replies() {
        let comments = sample_comments();
        let (top_level, replies) = build_thread_tree(&comments);
        assert_eq!(top_level.len(), 1);
        assert_eq!(top_level[0].id, "8");
        assert_eq!(replies.get(&("artalk", "8")).unwrap().len(), 1);
        assert_eq!(replies.get(&("artalk", "8")).unwrap()[0].id, "9");
    }

    #[test]
    fn test_build_thread_tree_empty() {
        let (top_level, replies) = build_thread_tree(&[]);
        assert!(top_level.is_empty());
        assert!(replies.is_empty());
    }

    // --- Date formatting ---

    #[test]
    fn test_format_date_zh() {
        let result = format_date("2026-03-24 11:36:57", Language::ZhHans);
        assert_eq!(result, "2026年3月24日");
    }

    #[test]
    fn test_format_date_en() {
        let result = format_date("2026-03-24 11:36:57", Language::En);
        assert_eq!(result, "Mar 24, 2026");
    }

    #[test]
    fn test_format_date_iso() {
        let result = format_date("2026-03-24T11:36:57Z", Language::ZhHans);
        assert_eq!(result, "2026年3月24日");
    }

    // --- Section rendering ---

    #[test]
    fn test_render_section_contains_comment_toggle() {
        let comments = sample_comments();
        let html = render_comment_section(
            &comments,
            "https://comment.example.com",
            "example.com",
            "/test/",
            "Test",
            Language::ZhHans,
            &[],
            0,
        );
        assert!(html.contains("moss-comments"));
        assert!(html.contains("comments-toggle"));
        assert!(html.contains("2条评论"));
    }

    #[test]
    fn test_render_section_contains_form() {
        let comments = sample_comments();
        let html = render_comment_section(
            &comments,
            "https://comment.example.com",
            "example.com",
            "/test/",
            "Test",
            Language::ZhHans,
            &[],
            0,
        );
        assert!(html.contains("moss-comment-form"));
        assert!(html.contains("留下你的想法"));
    }

    #[test]
    fn test_render_section_contains_comments() {
        let comments = sample_comments();
        let html = render_comment_section(
            &comments,
            "https://comment.example.com",
            "example.com",
            "/test/",
            "Test",
            Language::ZhHans,
            &[],
            0,
        );
        assert!(html.contains("Marcus"));
        assert!(html.contains("orz你们全家都手太巧了。"));
    }

    #[test]
    fn test_render_section_contains_reply_nested() {
        let comments = sample_comments();
        let html = render_comment_section(
            &comments,
            "https://comment.example.com",
            "example.com",
            "/test/",
            "Test",
            Language::ZhHans,
            &[],
            0,
        );
        assert!(html.contains("comment-replies"));
        assert!(html.contains("刘果"));
    }

    #[test]
    fn test_render_section_empty_comments_shows_form() {
        let html = render_comment_section(
            &[],
            "https://comment.example.com",
            "example.com",
            "/test/",
            "Test",
            Language::ZhHans,
            &[],
            0,
        );
        assert!(html.contains("moss-comment-form"));
        // Zero comments renders the invitation copy, not a count.
        assert!(html.contains("留下你的想法"));
    }

    #[test]
    fn test_render_section_includes_script() {
        let html = render_comment_section(
            &[],
            "https://example.com",
            "example",
            "/test/",
            "Test",
            Language::En,
            &[],
            0,
        );
        assert!(html.contains("<script>"));
    }

    fn sample_comment(id: &str, created_at: &str) -> NormalizedComment {
        NormalizedComment {
            id: id.to_string(),
            source: "artalk".to_string(),
            content: "<p>hi</p>".to_string(),
            created_at: created_at.to_string(),
            author: CommentAuthor {
                display_name: Some("tester".to_string()),
                name: None,
                url: None,
            },
            reply_to_id: None,
            state: None,
        }
    }

    /// Regression guard for the deploy-churn root cause (2026-06-16): the comment
    /// section must NOT embed wall-clock build time. `data-built-at` was a dead
    /// attribute (no reader) whose `chrono::Utc::now()` value changed every build,
    /// poisoning the deploy generation_id and re-uploading the whole site each deploy.
    #[test]
    fn render_section_has_no_build_timestamp_attribute() {
        let comments = vec![sample_comment("1", "2026-01-02T00:00:00Z")];
        let html = render_comment_section(
            &comments, "https://example.com/comments", "guo", "abc12345", "Title",
            Language::En, &[], 0,
        );
        assert!(!html.contains("data-built-at"),
            "comment section must not carry a wall-clock build timestamp; got: {html}");
    }

    /// The dehydrated-store `watermark` is content-derived: the newest baked
    /// comment's `created_at`. Deterministic for a fixed comment set.
    #[test]
    fn watermark_is_newest_comment_created_at() {
        let comments = vec![
            sample_comment("1", "2026-01-01T00:00:00Z"),
            sample_comment("2", "2026-03-15T12:30:00Z"),
            sample_comment("3", "2026-02-09T08:00:00Z"),
        ];
        let html = render_comment_section(
            &comments, "https://example.com/comments", "guo", "abc12345", "Title",
            Language::En, &[], 0,
        );
        assert!(html.contains("\"watermark\":\"2026-03-15T12:30:00Z\""),
            "watermark must equal the newest comment created_at; got: {html}");
    }

    /// Identical input -> byte-identical output, build over build.
    #[test]
    fn render_comment_section_is_deterministic() {
        let comments = vec![
            sample_comment("1", "2026-01-01T00:00:00Z"),
            sample_comment("2", "2026-02-02T00:00:00Z"),
        ];
        let render = || render_comment_section(
            &comments, "https://example.com/comments", "guo", "abc12345", "Title",
            Language::En, &[], 0,
        );
        assert_eq!(render(), render(), "comment section render must be deterministic");
    }

    #[test]
    fn test_thread_tree_namespaces_ids_per_source() {
        // Same id "1" from two sources must not cross-nest.
        let mk = |id: &str, source: &str, reply: Option<&str>| NormalizedComment {
            id: id.into(),
            source: source.into(),
            content: "x".into(),
            created_at: "2026-01-01".into(),
            author: CommentAuthor {
                display_name: Some("A".into()),
                name: None,
                url: None,
            },
            reply_to_id: reply.map(|r| r.to_string()),
            state: None,
        };
        let comments = vec![
            mk("1", "artalk", None),
            mk("1", "matters", None),
            mk("2", "matters", Some("1")), // replies to matters:1, NOT artalk:1
        ];
        let (top, replies) = build_thread_tree(&comments);
        assert_eq!(top.len(), 2);
        assert!(replies.get(&("matters", "1")).is_some());
        assert!(replies.get(&("artalk", "1")).is_none());
    }

    // --- syndicated_link_for_source tests ---

    #[test]
    fn test_syndicated_link_for_source_matches_by_host() {
        let urls = vec![
            "https://matters.town/@guo/3-river".to_string(),
            "https://www.douban.com/note/123/".to_string(),
        ];
        assert_eq!(
            syndicated_link_for_source(&urls, "matters"),
            Some("https://matters.town/@guo/3-river".to_string())
        );
        assert_eq!(
            syndicated_link_for_source(&urls, "douban"),
            Some("https://www.douban.com/note/123/".to_string())
        );
        assert_eq!(syndicated_link_for_source(&urls, "substack"), None);
    }

    #[test]
    fn test_syndicated_link_for_source_case_insensitive_host() {
        // url::Url::parse lowercases hosts — "Matters.town" must still match "matters".
        let urls = vec!["https://Matters.town/x".to_string()];
        assert_eq!(
            syndicated_link_for_source(&urls, "matters"),
            Some("https://Matters.town/x".to_string()),
            "host match must be case-insensitive via url::Url host normalization"
        );
    }

    #[test]
    fn test_form_uses_supplied_page_key() {
        let html = render_comment_section(
            &[],
            "https://api.mosspub.com/comments",
            "example.com",
            "abc12345", // page_key is whatever the caller supplies
            "Test",
            Language::ZhHans,
            &[],
            0,
        );
        assert!(html.contains("data-page-key=\"abc12345\""));
    }

    /// Canonical-key contract (ADR-025): the baked `data-page-key` is the article
    /// uid verbatim — never a pathname, never a transformed value. This pins the
    /// bake side of the bake == submit == sync three-way agreement that the
    /// key-drift thrash kept violating. If this fails, the bake path regressed to
    /// emitting a non-uid key; fix the caller in `features.rs` rather than this test.
    #[test]
    fn test_baked_page_key_is_exactly_the_uid_passed() {
        let uid = "cc4f602b";
        let html = render_comment_section(
            &[],
            "https://api.mosspub.com/comments",
            "guo",
            uid,
            "Test",
            Language::En,
            &[],
            0,
        );
        assert!(
            html.contains(&format!("data-page-key=\"{uid}\"")),
            "baked data-page-key must equal the uid argument verbatim; got:\n{html}"
        );
        assert!(
            !html.contains("data-page-key=\"/"),
            "baked data-page-key must never be a leading-slash pathname"
        );
    }

    /// The baked HTML embeds a dehydrated store the client hydrates from (ADR-025).
    #[test]
    fn test_render_embeds_dehydrated_store() {
        let comments = sample_comments();
        let html = render_comment_section(
            &comments,
            "https://api.mosspub.com/comments",
            "guo",
            "cc4f602b",
            "Test",
            Language::En,
            &[],
            9, // artalk_high_water_id: sample_comments() has ids 8 and 9
        );
        assert!(
            html.contains(r#"<script type="application/json" id="moss-comments-data">"#),
            "must embed a dehydrated store for client hydration"
        );
        assert!(
            html.contains("\"pageKey\":\"cc4f602b\""),
            "store must carry the canonical uid page key"
        );
        assert!(
            html.contains("\"watermark\":"),
            "store must carry a watermark for stale-while-revalidate"
        );
        assert!(
            html.contains("\"artalkHighWaterId\":9"),
            "store must carry the high-water id to prevent resurrection"
        );
    }

    /// The high-water id is emitted verbatim and zero when not supplied.
    #[test]
    fn test_dehydrated_store_emits_high_water_id() {
        let comments = sample_comments(); // ids "8" and "9"
        let html = render_comment_section(
            &comments,
            "https://api.mosspub.com/comments",
            "guo",
            "uid1",
            "T",
            Language::En,
            &[],
            9,
        );
        assert!(
            html.contains("\"artalkHighWaterId\":9"),
            "high-water id 9 must appear in the dehydrated store; got:\n{html}"
        );
    }

    /// When there are no Artalk comments at bake time, high-water is zero.
    #[test]
    fn test_dehydrated_store_high_water_zero_when_no_artalk() {
        let html = render_comment_section(
            &[],
            "https://api.mosspub.com/comments",
            "guo",
            "uid1",
            "T",
            Language::En,
            &[],
            0,
        );
        assert!(
            html.contains("\"artalkHighWaterId\":0"),
            "high-water id must be 0 when no Artalk comments exist at bake time; got:\n{html}"
        );
    }

    /// XSS guard: comment HTML containing `</script>` must be <-escaped inside
    /// the JSON island, or it would terminate the `<script type="application/json">`
    /// element early (script-tag JSON-embedding break-out).
    #[test]
    fn test_dehydrated_store_escapes_script_close_tag() {
        let c = NormalizedComment {
            id: "1".into(),
            source: "artalk".into(),
            content: "<p>x</script><script>alert(1)</script></p>".into(),
            created_at: "2026-01-01".into(),
            author: CommentAuthor {
                display_name: Some("X".into()),
                name: None,
                url: None,
            },
            reply_to_id: None,
            state: None,
        };
        let html = render_comment_section(
            std::slice::from_ref(&c),
            "https://api.mosspub.com/comments",
            "guo",
            "uid1",
            "T",
            Language::En,
            &[],
            0,
        );
        // Slice from the island open to the FIRST literal `</script>` — if escaping
        // works, that first literal close tag is the island's own terminator, and
        // the JSON in between contains no raw `</script>`.
        let open = html.find(r#"id="moss-comments-data">"#).unwrap();
        let island = &html[open..];
        let close = island.find("</script>").unwrap();
        let json = &island[..close];
        assert!(
            !json.contains("</script>"),
            "comment HTML </script> must be escaped inside the JSON island; got:\n{json}"
        );
        assert!(json.contains("\\u003c"), "< must be escaped to \\u003c");
    }

    #[test]
    fn test_author_javascript_url_not_rendered_as_link() {
        let c = NormalizedComment {
            id: "1".into(),
            source: "artalk".into(),
            content: "x".into(),
            created_at: "2026-01-01".into(),
            author: CommentAuthor {
                display_name: Some("Bad".into()),
                name: None,
                url: Some("javascript:alert(1)".into()),
            },
            reply_to_id: None,
            state: None,
        };
        let html = render_comment_section(
            std::slice::from_ref(&c),
            "https://api.mosspub.com/comments",
            "guo",
            "uid1",
            "T",
            Language::En,
            &[],
            0,
        );
        assert!(!html.contains("javascript:"), "js: author url must not become a link:\n{html}");
        assert!(
            html.contains("<span class=\"comment-author\">Bad</span>"),
            "non-http(s) author url falls back to a span:\n{html}"
        );
    }

    #[test]
    fn test_author_https_url_rendered_as_link() {
        let c = NormalizedComment {
            id: "1".into(),
            source: "artalk".into(),
            content: "x".into(),
            created_at: "2026-01-01".into(),
            author: CommentAuthor {
                display_name: Some("Good".into()),
                name: None,
                url: Some("https://example.com/".into()),
            },
            reply_to_id: None,
            state: None,
        };
        let html = render_comment_section(
            std::slice::from_ref(&c),
            "https://api.mosspub.com/comments",
            "guo",
            "uid1",
            "T",
            Language::En,
            &[],
            0,
        );
        assert!(
            html.contains("href=\"https://example.com/\""),
            "https author url becomes a link:\n{html}"
        );
        assert!(
            html.contains("rel=\"nofollow ugc noopener\""),
            "author link must carry rel=\"nofollow ugc noopener\" (ugc marks the \
             commenter-supplied link as user-generated); got:\n{html}"
        );
    }

    /// The reply button's ↩ must carry VARIATION SELECTOR-15 (U+FE0E) so mobile
    /// browsers render a plain monochrome arrow instead of a colored emoji.
    #[test]
    fn test_reply_button_arrow_uses_text_presentation() {
        let c = NormalizedComment {
            id: "1".into(),
            source: "artalk".into(),
            content: "x".into(),
            created_at: "2026-01-01".into(),
            author: CommentAuthor {
                display_name: Some("A".into()),
                name: None,
                url: None,
            },
            reply_to_id: None,
            state: None,
        };
        let html = render_comment_section(
            std::slice::from_ref(&c),
            "https://api.mosspub.com/comments",
            "guo",
            "uid1",
            "T",
            Language::En,
            &[],
            0,
        );
        assert!(
            html.contains("\u{21A9}\u{FE0E}"),
            "reply button arrow must carry the U+FE0E text-presentation selector; got:\n{html}"
        );
    }

    #[test]
    fn test_artalk_comment_gets_reply_button_matters_gets_link_out() {
        let mk = |id: &str, source: &str| NormalizedComment {
            id: id.into(),
            source: source.into(),
            content: "x".into(),
            created_at: "2026-01-01".into(),
            author: CommentAuthor {
                display_name: Some("A".into()),
                name: None,
                url: None,
            },
            reply_to_id: None,
            state: None,
        };
        let comments = vec![mk("1", "artalk"), mk("m1", "matters")];
        let syndicated = vec!["https://matters.town/@guo/3-river-ezdqp77gmegp".to_string()];
        let html = render_comment_section(
            &comments,
            "https://api.mosspub.com/comments",
            "example.com",
            "6a559bc6",
            "Test",
            Language::ZhHans,
            &syndicated,
            0,
        );
        assert!(html.contains("data-reply-id=\"1\""));
        assert!(html.contains("class=\"comment-source-link\""));
        assert!(html.contains("https://matters.town/@guo/3-river-ezdqp77gmegp"));
        assert!(!html.contains("data-reply-id=\"m1\""));
    }

    #[test]
    fn test_no_syndicated_url_means_no_affordance() {
        let c = NormalizedComment {
            id: "m1".into(),
            source: "matters".into(),
            content: "x".into(),
            created_at: "2026-01-01".into(),
            author: CommentAuthor {
                display_name: Some("A".into()),
                name: None,
                url: None,
            },
            reply_to_id: None,
            state: None,
        };
        let html = render_comment_section(
            &[c],
            "https://api.mosspub.com/comments",
            "example.com",
            "6a559bc6",
            "Test",
            Language::En,
            &[],
            0,
        );
        assert!(!html.contains("comment-source-link"));
        assert!(!html.contains("data-reply-id=\"m1\""));
    }

    /// Real Artalk timestamps are space-separated, no timezone (e.g. "2026-03-24 11:36:57").
    /// Lexicographic max == chronological max for that fixed format, so the watermark is
    /// the newest comment's timestamp and stays deterministic on production-shaped data.
    #[test]
    fn watermark_uses_newest_for_real_artalk_timestamp_format() {
        let comments = vec![
            sample_comment("1", "2026-03-24 11:36:57"),
            sample_comment("2", "2026-03-26 09:00:00"),
            sample_comment("3", "2026-03-25 23:59:59"),
        ];
        let html = render_comment_section(
            &comments, "https://example.com/comments", "guo", "abc12345", "Title",
            Language::En, &[], 0,
        );
        assert!(html.contains("\"watermark\":\"2026-03-26 09:00:00\""),
            "watermark must be the newest real-format timestamp; got: {html}");
    }

    /// Active section with zero comments emits an empty (but present) watermark — deterministic.
    #[test]
    fn watermark_is_empty_when_no_comments() {
        let comments: Vec<NormalizedComment> = vec![];
        let html = render_comment_section(
            &comments, "https://example.com/comments", "guo", "abc12345", "Title",
            Language::En, &[], 0,
        );
        assert!(html.contains("\"watermark\":\"\""),
            "empty comment set must yield watermark=\"\"; got: {html}");
    }

    #[test]
    fn comment_li_has_source_and_id_data_attributes() {
        // Build the smallest comment set the existing render helper accepts;
        // mirror the setup used by the neighbouring render tests in this file.
        let comments = vec![NormalizedComment {
            id: "42".into(),
            source: "matters".into(),
            content: "hello".into(),
            created_at: "2026-01-01".into(),
            author: CommentAuthor { display_name: Some("A".into()), name: None, url: None },
            reply_to_id: None,
            state: None,
        }];
        let html = render_comment_section(
            &comments, "", "site", "page-uid", "Title", Language::En, &[], -1,
        );
        assert!(html.contains(r#"data-comment-source="matters""#), "missing source attr: {html}");
        assert!(html.contains(r#"data-comment-id="42""#), "missing id attr: {html}");
    }
}
