use super::*;

#[test]
fn test_render_empty_list() {
    // Empty input renders nothing — moss does not fabricate a placeholder.
    let html = render(&[], false, crate::i18n::Language::En, None);
    assert_eq!(html, "");
}

#[test]
fn test_render_empty_list_chinese() {
    // Empty regardless of language — no placeholder string to localize.
    let html = render(&[], false, crate::i18n::Language::ZhHans, None);
    assert_eq!(html, "");
}

#[test]
fn test_render_single_article() {
    let articles = vec![ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: "post.html".to_string(),
        title: "Test Article".to_string(),
        url_path: String::new(),
    }];
    let html = render(&articles, false, crate::i18n::Language::En, None);

    assert!(html.contains(r#"<section class="moss-cards-minimal-year-group">"#));
    assert!(html.contains("<h2>2025</h2>"));
    assert!(html.contains("Test Article"));
}

#[test]
fn test_render_minimal_mode() {
    let articles = vec![ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: "post.html".to_string(),
        title: "Test Article".to_string(),
        url_path: String::new(),
    }];

    // Standard mode - shows "2025 · 11"
    let html_standard = render(&articles, false, crate::i18n::Language::En, None);
    assert!(html_standard.contains(r#"<section class="moss-cards-minimal-year-group">"#));
    assert!(!html_standard.contains(r#"class="moss-cards-minimal-year-group minimal">"#));
    assert!(html_standard.contains("2025 · 11")); // Full date format

    // Minimal mode - shows only "11" (month only)
    let html_minimal = render(&articles, true, crate::i18n::Language::En, None);
    assert!(html_minimal.contains(r#"<section class="moss-cards-minimal-year-group minimal">"#));
    assert!(html_minimal.contains(r#"<span class="moss-prefix-link-prefix date">11</span>"#)); // Month only with shared class
    assert!(!html_minimal.contains("2025 · 11")); // No full date format
}

#[test]
fn test_render_groups_by_year() {
    let articles = vec![
        ArticleListItemProps {
            date_display: "2025 · 11".to_string(),
            date_raw: Some("2025-11-17T00:00:00Z".to_string()),
            url: "post1.html".to_string(),
            title: "Post 2025 Nov".to_string(),
            url_path: String::new(),
        },
        ArticleListItemProps {
            date_display: "2024 · 12".to_string(),
            date_raw: Some("2024-12-01T00:00:00Z".to_string()),
            url: "post2.html".to_string(),
            title: "Post 2024 Dec".to_string(),
            url_path: String::new(),
        },
        ArticleListItemProps {
            date_display: "2025 · 01".to_string(),
            date_raw: Some("2025-01-15T00:00:00Z".to_string()),
            url: "post3.html".to_string(),
            title: "Post 2025 Jan".to_string(),
            url_path: String::new(),
        },
    ];
    let html = render(&articles, false, crate::i18n::Language::En, None);

    // Should have two year sections
    assert!(html.contains("<h2>2025</h2>"));
    assert!(html.contains("<h2>2024</h2>"));

    // 2025 section should come first (newer)
    let pos_2025 = html.find("<h2>2025</h2>").unwrap();
    let pos_2024 = html.find("<h2>2024</h2>").unwrap();
    assert!(pos_2025 < pos_2024);
}

#[test]
fn test_render_sorts_within_year() {
    let articles = vec![
        ArticleListItemProps {
            date_display: "2025 · 01".to_string(),
            date_raw: Some("2025-01-15T00:00:00Z".to_string()),
            url: "jan.html".to_string(),
            title: "January".to_string(),
            url_path: String::new(),
        },
        ArticleListItemProps {
            date_display: "2025 · 11".to_string(),
            date_raw: Some("2025-11-17T00:00:00Z".to_string()),
            url: "nov.html".to_string(),
            title: "November".to_string(),
            url_path: String::new(),
        },
    ];
    let html = render(&articles, false, crate::i18n::Language::En, None);

    // November should come before January (newer first)
    let pos_nov = html.find("November").unwrap();
    let pos_jan = html.find("January").unwrap();
    assert!(pos_nov < pos_jan);
}

#[test]
fn test_render_sorts_by_precise_datetime() {
    // Test that articles in the same month are sorted by exact datetime
    let articles = vec![
        ArticleListItemProps {
            date_display: "2025 · 11".to_string(),
            date_raw: Some("2025-11-04T15:28:10.481Z".to_string()),
            url: "first.html".to_string(),
            title: "First (Nov 4)".to_string(),
            url_path: String::new(),
        },
        ArticleListItemProps {
            date_display: "2025 · 11".to_string(),
            date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
            url: "fourth.html".to_string(),
            title: "Fourth (Nov 17)".to_string(),
            url_path: String::new(),
        },
        ArticleListItemProps {
            date_display: "2025 · 11".to_string(),
            date_raw: Some("2025-11-11T00:34:33.978Z".to_string()),
            url: "second.html".to_string(),
            title: "Second (Nov 11)".to_string(),
            url_path: String::new(),
        },
        ArticleListItemProps {
            date_display: "2025 · 11".to_string(),
            date_raw: Some("2025-11-13T01:06:40.170Z".to_string()),
            url: "third.html".to_string(),
            title: "Third (Nov 13)".to_string(),
            url_path: String::new(),
        },
    ];
    let html = render(&articles, false, crate::i18n::Language::En, None);

    // Should be sorted newest first: Fourth, Third, Second, First
    let pos_fourth = html.find("Fourth").unwrap();
    let pos_third = html.find("Third").unwrap();
    let pos_second = html.find("Second").unwrap();
    let pos_first = html.find("First").unwrap();

    assert!(pos_fourth < pos_third, "Fourth should come before Third");
    assert!(pos_third < pos_second, "Third should come before Second");
    assert!(pos_second < pos_first, "Second should come before First");
}

#[test]
fn test_render_handles_mixed_parseable_and_unparseable_dates() {
    let articles = vec![
        ArticleListItemProps {
            date_display: "2025 · 11".to_string(),
            date_raw: Some("2025-11-17T00:00:00Z".to_string()),
            url: "post1.html".to_string(),
            title: "Dated Article".to_string(),
            url_path: String::new(),
        },
        ArticleListItemProps {
            date_display: "Unknown".to_string(),
            date_raw: None,
            url: "post2.html".to_string(),
            title: "Undated Article".to_string(),
            url_path: String::new(),
        },
    ];
    let html = render(&articles, false, crate::i18n::Language::En, None);

    // Both articles should be rendered
    assert!(html.contains("Dated Article"));
    assert!(html.contains("Undated Article"));
    // The dated article should be in a 2025 section
    assert!(html.contains("<h2>2025</h2>"));
}

/// A vertical CJK listing groups by year at all.
///
/// The year came from `date_display`, and on a vertical CJK page that string
/// is already `一七〇三年十二月` — four leading ASCII digits are the only shape
/// `extract_year` reads, so every row returned `None` and 59 works collapsed
/// into one headingless section (zhu-da home, 2026-09-11). The raw ISO date is
/// the same string the sort already trusts.
#[test]
fn vertical_cjk_rows_still_group_by_year() {
    let article = |display: &str, raw: &str, title: &str| ArticleListItemProps {
        date_display: display.to_string(),
        date_raw: Some(raw.to_string()),
        url: format!("/{title}/"),
        title: title.to_string(),
        url_path: String::new(),
    };
    let articles = vec![
        article("一七〇三年十二月", "1703-12-01", "楊柳浴禽圖"),
        article("一七〇三年", "1703", "蘆雁圖"),
        article("一七〇〇年", "1700", "雙雁圖"),
    ];
    let html = render(&articles, true, crate::i18n::Language::ZhHant, Some("vertical"));

    // Two year sections, headed in Chinese numerals — an Arabic 1703 would lie
    // on its side in a vertical column.
    assert_eq!(html.matches("<section").count(), 2, "{html}");
    assert!(html.contains("<h2>一七〇三</h2>"), "{html}");
    assert!(html.contains("<h2>一七〇〇</h2>"), "{html}");
    assert!(!html.contains("<h2>1703</h2>"), "{html}");

    // The dated row carries its month in Chinese; the year-only rows carry no
    // prefix at all, because the heading immediately above already says it.
    assert!(html.contains(r#"<span class="moss-prefix-link-prefix date">十二月</span>"#), "{html}");
    assert!(!html.contains("1703</span>"), "{html}");
    assert_eq!(html.matches("moss-prefix-link-prefix").count(), 1, "{html}");
}

/// The same listing horizontally: Arabic headings, Arabic months, and still no
/// year repeated as a prefix under its own heading.
#[test]
fn horizontal_rows_keep_arabic_headings_and_drop_the_bare_year_prefix() {
    let articles = vec![
        ArticleListItemProps {
            date_display: "2025 · 11".to_string(),
            date_raw: Some("2025-11-17".to_string()),
            url: "/a/".to_string(),
            title: "A".to_string(),
            url_path: String::new(),
        },
        ArticleListItemProps {
            date_display: "2025".to_string(),
            date_raw: Some("2025".to_string()),
            url: "/b/".to_string(),
            title: "B".to_string(),
            url_path: String::new(),
        },
    ];
    let html = render(&articles, true, crate::i18n::Language::En, None);
    assert!(html.contains("<h2>2025</h2>"), "{html}");
    assert_eq!(html.matches("<section").count(), 1, "{html}");
    assert!(html.contains(r#"<span class="moss-prefix-link-prefix date">11</span>"#), "{html}");
    assert_eq!(html.matches("moss-prefix-link-prefix").count(), 1, "{html}");
}
