//! Where a page was also published: the "Also on" line at the end of an
//! article, and the one host-to-platform-name table the comment "Reply on"
//! link shares.

use crate::i18n::Language;

/// The platform a copy's address belongs to, when moss knows it. A Mastodon
/// server can live on any host, so a `/@user/<id>` path is the signal there.
pub(crate) fn platform_name(url: &str) -> Option<&'static str> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let on = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    let name = match () {
        _ if on("bsky.app") => "Bluesky",
        _ if on("x.com") || on("twitter.com") => "X",
        _ if on("matters.town") || on("matters.news") || on("matters.icu") => "Matters",
        _ if on("linkedin.com") => "LinkedIn",
        _ if on("substack.com") => "Substack",
        _ if on("threads.net") => "Threads",
        _ if on("medium.com") => "Medium",
        _ if is_mastodon_path(parsed.path()) => "Mastodon",
        _ => return None,
    };
    Some(name)
}

/// `/@user/<digits>`: the shape of a Mastodon post address.
fn is_mastodon_path(path: &str) -> bool {
    let mut parts = path.trim_matches('/').split('/');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(user), Some(id), None)
            if user.len() > 1 && user.starts_with('@') && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
    )
}

/// The label a link to `url` carries: its platform, else the bare host.
fn link_label(url: &str) -> Option<String> {
    if let Some(name) = platform_name(url) {
        return Some(name.to_string());
    }
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    Some(host.strip_prefix("www.").unwrap_or(host).to_string())
}

/// The "Also on" line for an article's `syndicated:` list, or `None` when no
/// entry is an http(s) address. `rel="syndication"` and `u-syndication` are
/// IndieWeb's markup, which tools such as Bridgy read to pair a copy with its
/// original.
pub fn render_syndication_line(urls: &[String], lang: Language) -> Option<String> {
    let links: Vec<String> = urls
        .iter()
        .filter(|u| matches!(url::Url::parse(u).map(|p| p.scheme().to_string()).as_deref(), Ok("http" | "https")))
        .filter_map(|u| {
            let label = link_label(u)?;
            Some(format!(
                "<a href=\"{}\" rel=\"syndication\" class=\"u-syndication\">{}</a>",
                crate::build::features::html_escape(u),
                crate::build::features::html_escape(&label),
            ))
        })
        .collect();
    if links.is_empty() {
        return None;
    }
    Some(format!(
        "<p class=\"moss-syndication\"><span class=\"moss-syndication-label\">{}</span> {}</p>",
        crate::i18n::t(lang, "also_on"),
        links.join(" · "),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn names_known_platforms_and_mastodon_by_path() {
        assert_eq!(platform_name("https://bsky.app/profile/a.example/post/3k"), Some("Bluesky"));
        assert_eq!(platform_name("https://matters.town/a/abc"), Some("Matters"));
        assert_eq!(platform_name("https://news.example/@mia/112233"), Some("Mastodon"));
        assert_eq!(platform_name("https://news.example/about"), None);
        assert_eq!(platform_name("https://notbsky.app.example/x"), None);
    }

    #[test]
    fn three_copies_become_three_links_in_order() {
        let html = render_syndication_line(
            &urls(&["https://bsky.app/profile/a.example/post/3k", "https://social.example/@mia/112233", "https://matters.town/a/abc"]),
            Language::En,
        )
        .unwrap();
        let bsky = html.find(">Bluesky</a>").unwrap();
        let masto = html.find(">Mastodon</a>").unwrap();
        let matters = html.find(">Matters</a>").unwrap();
        assert!(bsky < masto && masto < matters, "{html}");
        assert_eq!(html.matches("rel=\"syndication\" class=\"u-syndication\"").count(), 3, "{html}");
        assert!(html.contains("Also on"), "{html}");
    }

    #[test]
    fn unknown_host_shows_the_bare_host_and_non_http_is_skipped() {
        let html = render_syndication_line(&urls(&["javascript:alert(1)", "ftp://x.example/a", "https://www.blog.example/p"]), Language::En).unwrap();
        assert!(html.contains(">blog.example</a>"), "{html}");
        assert!(!html.contains("javascript") && !html.contains("ftp"), "{html}");
        assert_eq!(render_syndication_line(&urls(&["mailto:a@b.example"]), Language::En), None);
        assert_eq!(render_syndication_line(&[], Language::En), None);
    }

    #[test]
    fn label_follows_the_page_language() {
        let one = urls(&["https://matters.town/a/abc"]);
        assert!(render_syndication_line(&one, Language::ZhHans).unwrap().contains("也发在"));
        assert!(render_syndication_line(&one, Language::ZhHant).unwrap().contains("也發在"));
        assert!(render_syndication_line(&one, Language::De).unwrap().contains("Auch auf"));
    }

    #[test]
    fn comment_reply_link_names_its_platform_through_the_shared_table() {
        use crate::build::features::comment::{render_comment_section, CommentAuthor, NormalizedComment};
        let comment = NormalizedComment {
            id: "c1".into(),
            source: "bsky".into(),
            content: "hi".into(),
            created_at: "2026-01-02T03:04:05Z".into(),
            author: CommentAuthor { display_name: Some("Ana".into()), name: None, url: None },
            reply_to_id: None,
            state: None,
        };
        let html = render_comment_section(
            &[comment], "https://c.example", "s", "k", "t", Language::En,
            &urls(&["https://bsky.app/profile/a.example/post/3k"]), 0,
        );
        assert!(html.contains("Reply on Bluesky"), "{html}");
    }
}
