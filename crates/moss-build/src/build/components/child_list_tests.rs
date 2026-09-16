use super::*;
use crate::i18n::Language;

#[test]
fn test_render_basic_article() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: "post.html".to_string(),
        title: "My Post".to_string(),
    };
    let html = render(&props, false, false, Language::En, None);

    // Link wraps both date and title with shared prefix-link classes
    assert!(html.contains(r#"<a href="post.html" class="moss-prefix-link">"#));
    assert!(html.contains(r#"<span class="moss-prefix-link-prefix date">2025 · 11</span>"#));
    assert!(html.contains(r#"<span class="moss-prefix-link-title title">My Post</span>"#));
}

#[test]
fn test_render_escapes_html_in_title() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: None,
        url: "post.html".to_string(),
        title: "<script>alert('xss')</script>".to_string(),
    };
    let html = render(&props, false, false, Language::En, None);

    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
}

#[test]
fn test_render_escapes_html_in_url() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: None,
        url: "post.html?a=1&b=2".to_string(),
        title: "Test".to_string(),
    };
    let html = render(&props, false, false, Language::En, None);

    assert!(html.contains("a=1&amp;b=2"));
}

#[test]
fn test_render_minimal_shows_month_only() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: "post.html".to_string(),
        title: "My Post".to_string(),
    };
    let html = render(&props, true, false, Language::En, None);

    // Should show only month "11", not "2025 · 11"
    assert!(html.contains(r#"<span class="moss-prefix-link-prefix date">11</span>"#));
    assert!(!html.contains("2025"));
}

#[test]
fn test_render_minimal_fallback_without_raw_date() {
    let props = ArticleListItemProps {
        date_display: "2025-11-17".to_string(),
        date_raw: None,
        url: "post.html".to_string(),
        title: "Test".to_string(),
    };
    let html = render(&props, true, false, Language::En, None);

    // Should extract month from date_display
    assert!(html.contains(r#"<span class="moss-prefix-link-prefix date">11</span>"#));
}

#[test]
fn test_render_uses_prefix_link_classes() {
    // TDD: The article item should use shared prefix-link classes
    // for consistent styling with series-nav component
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: "post.html".to_string(),
        title: "My Post".to_string(),
    };
    let html = render(&props, false, false, Language::En, None);

    // The <a> element should have prefix-link class
    assert!(
        html.contains(r#"class="moss-prefix-link""#),
        "Link should have prefix-link class for shared styling"
    );

    // Date span should have moss-prefix-link-prefix class (in addition to date)
    assert!(
        html.contains(r#"class="moss-prefix-link-prefix date""#),
        "Date span should have moss-prefix-link-prefix class"
    );

    // Title span should have moss-prefix-link-title class (in addition to title)
    assert!(
        html.contains(r#"class="moss-prefix-link-title title""#),
        "Title span should have moss-prefix-link-title class"
    );
}

#[test]
fn test_render_no_date_omits_date_span() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: "post.html".to_string(),
        title: "Series Item".to_string(),
    };
    let html = render(&props, false, true, Language::En, None);

    // Should NOT contain any date span
    assert!(
        !html.contains("date"),
        "no_date should omit date span entirely"
    );
    // Should still contain the title
    assert!(
        html.contains("Series Item"),
        "Should still render the title"
    );
    // Should still have prefix-link structure
    assert!(
        html.contains(r#"class="moss-prefix-link""#),
        "Should have prefix-link class"
    );
    assert!(
        html.contains(r#"class="moss-prefix-link-title title""#),
        "Should have title class"
    );
}

#[test]
fn test_render_child_article() {
    let props = ChildItemProps {
        title: "My Article".to_string(),
        url: "/blog/my-article/".to_string(),
        date_display: Some("2025 · 11".to_string()),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
    };
    let html = render_child(&props, crate::i18n::Language::ZhHans, None);
    assert!(html.contains(r#"<div class="moss-card">"#));
    assert!(html.contains(r#"<span class="moss-prefix-link-prefix date">11</span>"#));
    assert!(html.contains(r#"<span class="moss-prefix-link-title">My Article</span>"#));
    assert!(html.contains(r#"href="/blog/my-article/""#));
}

/// The list card shape shared the summary and grid cards' bug in a second
/// form: it had no CJK-numeral branch at all, so a vertical CJK site
/// counted in Arabic digits, laid on their sides.
#[test]
fn folder_count_uses_chinese_numerals_in_vertical_cjk() {
    let props = ChildItemProps {
        title: "書".to_string(),
        url: "/書/".to_string(),
        date_display: None,
        date_raw: None,
        child_count: Some(38),
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
    };
    let vertical = render_child(&props, crate::i18n::Language::ZhHant, Some("vertical"));
    assert!(vertical.contains("三十八篇"), "{vertical}");
    let horizontal = render_child(&props, crate::i18n::Language::ZhHant, None);
    assert!(horizontal.contains("38 篇"), "{horizontal}");
}

#[test]
fn test_render_child_folder_without_description() {
    // Folders WITHOUT description: count in prefix slot (like date for articles)
    let props = ChildItemProps {
        title: "Tutorials".to_string(),
        url: "/tutorials/".to_string(),
        date_display: None,
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        child_count: Some(4),
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
    };
    let html = render_child(&props, crate::i18n::Language::ZhHans, None);
    assert!(html.contains(r#"<div class="moss-card">"#));
    assert!(html.contains(r#"<span class="moss-prefix-link-title">Tutorials</span>"#));
    // Count should be in the prefix slot
    assert!(html.contains(r#"<span class="moss-prefix-link-prefix">4 篇</span>"#));
    assert!(!html.contains("date"));
    // Should NOT have description
    assert!(!html.contains("moss-folder-description"));
}

#[test]
fn test_render_child_folder_with_description() {
    // Folders WITH description: title + count suffix on first line, description below
    let props = ChildItemProps {
        title: "Tutorials".to_string(),
        url: "/tutorials/".to_string(),
        date_display: None,
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        child_count: Some(4),
        description: Some("Learn the basics of moss".to_string()),
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
    };
    let html = render_child(&props, crate::i18n::Language::ZhHans, None);
    assert!(html.contains(r#"<div class="moss-card moss-folder-item">"#));
    // Title on the link
    assert!(html.contains(r#"<span class="moss-prefix-link-title">Tutorials</span>"#));
    // Count as suffix on title line
    assert!(html.contains(r#"<span class="moss-prefix-link-suffix">4 篇</span>"#));
    // Description as subtitle
    assert!(html.contains(r#"<p class="moss-folder-description">Learn the basics of moss</p>"#));
    // The link should have moss-folder-link class for justify-content: space-between
    assert!(html.contains(r#"class="moss-prefix-link moss-folder-link""#));
}

#[test]
fn test_render_child_folder_escapes_html() {
    let props = ChildItemProps {
        title: "<b>Bold</b> & \"Quoted\"".to_string(),
        url: "/test/?a=1&b=2".to_string(),
        date_display: None,
        date_raw: None,
        child_count: Some(2),
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
    };
    let html = render_child(&props, crate::i18n::Language::ZhHans, None);
    assert!(html.contains("&lt;b&gt;Bold&lt;/b&gt; &amp; &quot;Quoted&quot;"));
    assert!(html.contains("a=1&amp;b=2"));
    assert!(!html.contains("<b>"));
}

#[test]
fn test_render_child_article_no_date() {
    let props = ChildItemProps {
        title: "Dateless Article".to_string(),
        url: "/blog/dateless/".to_string(),
        date_display: None,
        date_raw: None,
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
    };
    let html = render_child(&props, crate::i18n::Language::ZhHans, None);
    assert!(html.contains(r#"<div class="moss-card">"#));
    assert!(html.contains(r#"<span class="moss-prefix-link-title">Dateless Article</span>"#));
    assert!(!html.contains(r#"class="moss-prefix-link-prefix date">"#));
    assert!(!html.contains("moss-prefix-link-prefix"));
    assert!(!html.contains("moss-prefix-link-suffix"));
}

#[test]
fn test_render_no_date_ignores_minimal_flag() {
    // When no_date is true, the minimal flag is irrelevant
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: "post.html".to_string(),
        title: "Series Item".to_string(),
    };
    let html_minimal = render(&props, true, true, Language::En, None);
    let html_normal = render(&props, false, true, Language::En, None);

    // Both should be identical when no_date is true
    assert_eq!(
        html_minimal, html_normal,
        "no_date=true should make minimal flag irrelevant"
    );
}

// ── props_for_document ─────────────────────────────────────────────────
//
// These behaviours all existed before the hoist, reachable only through
// `folder_embed::generate_children`. Testing them directly is half of what the
// hoist bought.

use crate::build::types::ParsedDocument;
use moss_core::PageKind;
use std::collections::HashMap;

fn doc(url_path: &str, label: &str) -> ParsedDocument {
    ParsedDocument {
        title: label.to_string(),
        label: label.to_string(),
        url_path: url_path.to_string(),
        kind: PageKind::Article,
        lang: Language::En,
        ..Default::default()
    }
}

fn props(d: &ParsedDocument, all: &[ParsedDocument]) -> ChildItemProps {
    props_for_document(d, all, "", &HashMap::new(), None)
}

#[test]
fn a_folder_carries_its_child_count_and_its_latest_child_date() {
    let docs = vec![
        ParsedDocument { kind: PageKind::Folder, ..doc("works/index.html", "Works") },
        ParsedDocument { date: Some("2023-01-05".into()), ..doc("works/one/index.html", "One") },
        ParsedDocument { date: Some("2024-06-30".into()), ..doc("works/two/index.html", "Two") },
    ];
    let p = props(&docs[0], &docs);
    assert_eq!(p.child_count, Some(2));
    assert_eq!(p.date_raw.as_deref(), Some("2024-06-30"));
    // A folder's own row shows the count, never a date display.
    assert_eq!(p.date_display, None);
}

/// The card's count is recursive: a page in a subfolder counts toward the
/// parent folder's card even though the subfolder's own listing (`direct` by
/// default) would not show it. Only the subfolder's own index page — a
/// `Folder`, not an `Article` — is excluded.
#[test]
fn a_folder_card_counts_pages_in_a_subfolder_too() {
    let docs = vec![
        ParsedDocument { kind: PageKind::Folder, ..doc("works/index.html", "Works") },
        ParsedDocument { date: Some("2023-01-05".into()), ..doc("works/one/index.html", "One") },
        ParsedDocument { kind: PageKind::Folder, ..doc("works/letters/index.html", "Letters") },
        ParsedDocument { date: Some("2024-06-30".into()), ..doc("works/letters/first/index.html", "First") },
        ParsedDocument { date: Some("2024-07-01".into()), ..doc("works/letters/second/index.html", "Second") },
    ];
    let p = props(&docs[0], &docs);
    assert_eq!(p.child_count, Some(3), "direct page + both nested letters, subfolder index excluded");
}

#[test]
fn a_leaf_falls_back_from_frontmatter_to_a_filename_date() {
    let docs = vec![doc("posts/2021-04-09-spring.html", "Spring")];
    let p = props(&docs[0], &docs);
    assert_eq!(p.child_count, None);
    // The raw date is the filename stem as written; only the leading
    // year-month run has to parse.
    assert_eq!(p.date_raw.as_deref(), Some("2021-04-09-spring"));
    assert_eq!(p.date_display.as_deref(), Some("2021 · 04"));
}

#[test]
fn a_dateless_leaf_gets_no_date_display() {
    let docs = vec![doc("posts/undated/index.html", "Undated")];
    let p = props(&docs[0], &docs);
    // The filesystem rungs find nothing under an empty root, and a
    // non-explicit date must not be displayed as if the author wrote it.
    assert_eq!(p.date_display, None);
}

#[test]
fn an_external_url_takes_the_card_href_and_the_local_one_becomes_the_permalink() {
    let mut d = doc("links/elsewhere/index.html", "Elsewhere");
    d.raw_frontmatter.insert(
        "external_url".to_string(),
        serde_json::Value::String("https://example.com/piece".to_string()),
    );
    d.raw_frontmatter.insert(
        "publisher".to_string(),
        serde_json::Value::String("The Paper".to_string()),
    );
    let docs = vec![d];
    let p = props(&docs[0], &docs);
    assert_eq!(p.url, "https://example.com/piece");
    assert_eq!(p.permalink.as_deref(), Some("/links/elsewhere/"));
    assert_eq!(p.kicker.as_deref(), Some("The Paper"));
}

#[test]
fn an_ordinary_card_has_no_permalink() {
    let docs = vec![doc("posts/local/index.html", "Local")];
    let p = props(&docs[0], &docs);
    assert_eq!(p.url, "/posts/local/");
    assert_eq!(p.permalink, None);
}

#[test]
fn a_pipe_encoded_cover_resolves_only_its_path_and_keeps_its_attrs() {
    let docs = vec![ParsedDocument {
        cover: Some("images/plate.png|crop:top".to_string()),
        ..doc("works/plate/index.html", "Plate")
    }];
    let mut overrides = HashMap::new();
    overrides.insert("images".to_string(), "media".to_string());
    let p = props_for_document(&docs[0], &docs, "", &overrides, None);
    assert_eq!(p.cover.as_deref(), Some("/media/plate.png|crop:top"));
    assert_eq!(p.cover_type, Some(CoverType::Image));
}

#[test]
fn frontmatter_description_only_and_blank_is_none() {
    let docs = vec![
        ParsedDocument { description: Some("  ".into()), ..doc("a/index.html", "A") },
        ParsedDocument { description: Some("Real.".into()), ..doc("b/index.html", "B") },
    ];
    assert_eq!(props(&docs[0], &docs).description, None);
    assert_eq!(props(&docs[1], &docs).description.as_deref(), Some("Real."));
}
