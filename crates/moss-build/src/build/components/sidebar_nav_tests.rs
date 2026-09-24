use super::*;

fn make_article(title: &str, date_raw: Option<&str>, url: &str) -> ArticleListItemProps {
    ArticleListItemProps {
        date_display: date_raw
            .map(|d| super::super::date::format_date_string(d))
            .unwrap_or_default(),
        date_raw: date_raw.map(|s| s.to_string()),
        url: url.to_string(),
        title: title.to_string(),
        url_path: String::new(),
    }
}

// ========== render tests ==========

#[test]
fn test_render_includes_latest_header() {
    let articles = vec![make_article(
        "My Article",
        Some("2025-01-15T10:00:00Z"),
        "article.html",
    )];

    let html = render(&articles, None, crate::i18n::Language::En);

    assert!(
        html.contains("<h3>Latest</h3>"),
        "Should have Latest header"
    );
}

#[test]
fn test_render_empty_articles_returns_empty() {
    let articles: Vec<ArticleListItemProps> = vec![];
    let html = render(&articles, None, crate::i18n::Language::En);
    assert!(
        html.is_empty(),
        "Empty articles should produce empty output"
    );
}

#[test]
fn test_render_article_uses_prefix_link_classes() {
    let articles = vec![make_article(
        "Test Article",
        Some("2025-01-15T10:00:00Z"),
        "test.html",
    )];

    let html = render(&articles, None, crate::i18n::Language::En);

    // Reuse existing moss-prefix-link pattern for DRY
    assert!(
        html.contains(r#"class="moss-prefix-link""#),
        "Should use moss-prefix-link class for consistent styling"
    );
    assert!(
        html.contains(r#"class="moss-prefix-link-prefix"#),
        "Should use moss-prefix-link-prefix for date"
    );
    assert!(
        html.contains(r#"class="moss-prefix-link-title"#),
        "Should use moss-prefix-link-title for article title"
    );
}

#[test]
fn test_render_article_format_year_dot_month() {
    let articles = vec![make_article(
        "Test Article",
        Some("2025-01-15T10:00:00Z"),
        "test.html",
    )];

    let html = render(&articles, None, crate::i18n::Language::En);

    // Format should be "2025 · 01" (year · month)
    assert!(
        html.contains("2025 · 01"),
        "Date should be formatted as year · month"
    );
}

#[test]
fn test_render_article_includes_link() {
    let articles = vec![make_article(
        "Test Article",
        Some("2025-01-15T10:00:00Z"),
        "test.html",
    )];

    let html = render(&articles, None, crate::i18n::Language::En);

    assert!(
        html.contains(r#"href="test.html""#),
        "Should include article link"
    );
    assert!(
        html.contains("Test Article"),
        "Should include article title"
    );
}

#[test]
fn test_render_multiple_articles() {
    let articles = vec![
        make_article("First", Some("2025-01-15T10:00:00Z"), "first.html"),
        make_article("Second", Some("2025-01-14T10:00:00Z"), "second.html"),
    ];

    let html = render(&articles, None, crate::i18n::Language::En);

    assert!(html.contains("First"));
    assert!(html.contains("Second"));
    assert!(html.contains(r#"href="first.html""#));
    assert!(html.contains(r#"href="second.html""#));
}

#[test]
fn test_render_escapes_html_in_title() {
    let articles = vec![make_article(
        "<script>alert('xss')</script>",
        Some("2025-01-15T10:00:00Z"),
        "test.html",
    )];

    let html = render(&articles, None, crate::i18n::Language::En);

    assert!(!html.contains("<script>"), "Should escape script tags");
    assert!(html.contains("&lt;script&gt;"), "Should have escaped HTML");
}

#[test]
fn test_render_escapes_html_in_url() {
    let articles = vec![make_article(
        "Test",
        Some("2025-01-15T10:00:00Z"),
        "test.html?a=1&b=2",
    )];

    let html = render(&articles, None, crate::i18n::Language::En);

    assert!(
        html.contains("a=1&amp;b=2"),
        "Should escape ampersands in URL"
    );
}

#[test]
fn test_render_has_sidebar_wrapper_class() {
    let articles = vec![make_article(
        "Test",
        Some("2025-01-15T10:00:00Z"),
        "test.html",
    )];

    let html = render(&articles, None, crate::i18n::Language::En);

    assert!(
        html.contains(r#"class="latest-sidebar""#),
        "Should have latest-sidebar wrapper class for CSS styling"
    );
}

// ========== more_url tests ==========

#[test]
fn test_render_with_more_link() {
    let articles = vec![make_article(
        "Article",
        Some("2025-01-15T10:00:00Z"),
        "article.html",
    )];

    let html = render(&articles, Some("news/"), crate::i18n::Language::En);

    assert!(
        html.contains(r#"<a href="news/" class="sidebar-more">More →</a>"#),
        "Should render More link when more_url is Some. Got:\n{}",
        html
    );
}

#[test]
fn test_render_without_more_link() {
    let articles = vec![make_article(
        "Article",
        Some("2025-01-15T10:00:00Z"),
        "article.html",
    )];

    let html = render(&articles, None, crate::i18n::Language::En);

    assert!(
        !html.contains("sidebar-more"),
        "Should NOT render More link when more_url is None. Got:\n{}",
        html
    );
    assert!(
        !html.contains("More →"),
        "Should NOT contain 'More →' text when more_url is None"
    );
}

#[test]
fn test_render_more_link_escapes_url() {
    let articles = vec![make_article(
        "Article",
        Some("2025-01-15T10:00:00Z"),
        "article.html",
    )];

    let html = render(&articles, Some("news/?a=1&b=2"), crate::i18n::Language::En);

    assert!(
        html.contains("a=1&amp;b=2"),
        "Should escape ampersands in more_url"
    );
}

#[test]
fn test_render_more_link_after_list() {
    let articles = vec![make_article(
        "Article",
        Some("2025-01-15T10:00:00Z"),
        "article.html",
    )];

    let html = render(&articles, Some("news/"), crate::i18n::Language::En);

    // The more link should appear after </ul> and before </nav>
    let ul_end = html.find("</ul>").expect("should have </ul>");
    let more_start = html.find("sidebar-more").expect("should have sidebar-more");
    assert!(
        more_start > ul_end,
        "More link should appear after the <ul> list"
    );
}
