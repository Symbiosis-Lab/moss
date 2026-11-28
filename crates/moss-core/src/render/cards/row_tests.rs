use crate::render::cards::*;
use crate::render::cards::row::*;
use crate::page::card::*;
use crate::i18n::Language;

#[test]
fn test_render_basic_article() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: Some("post.html".to_string()),
        title: "My Post".to_string(),
        ..Default::default()
    };
    let html = render(&props, false, false, Language::En, None);

    // Link wraps both date and title with shared prefix-link classes
    assert!(html.contains(r#"<a href="post.html" class="moss-prefix-link">"#));
    assert!(html.contains(r#"<span class="moss-prefix-link-prefix date">2025 · 11</span>"#));
    assert!(html.contains(r#"<span class="moss-prefix-link-title title">My Post</span>"#));
}

/// A chronology of located, dated pages (the motivating case: "Cambridge
/// 1924", "Hayes Court 1926") must show the place next to the compact date,
/// not just the date.
#[test]
fn render_shows_the_resolved_place_next_to_the_date() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: Some("post.html".to_string()),
        title: "My Post".to_string(),
        place: Some("Cambridge".to_string()),
        ..Default::default()
    };
    let html = render(&props, false, false, Language::En, None);

    assert!(
        html.contains(r#"<span class="moss-prefix-link-prefix date">2025 · 11 · Cambridge</span>"#),
        "place must appear next to the date: {html}"
    );
}

/// No `no_date` run gets a place either — there is no date for it to sit
/// next to.
#[test]
fn render_omits_the_place_when_no_date_is_shown() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: Some("post.html".to_string()),
        title: "Series Item".to_string(),
        place: Some("Cambridge".to_string()),
        ..Default::default()
    };
    let html = render(&props, false, true, Language::En, None);
    assert!(!html.contains("Cambridge"));
}

#[test]
fn test_render_escapes_html_in_title() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        url: Some("post.html".to_string()),
        title: "<script>alert('xss')</script>".to_string(),
        ..Default::default()
    };
    let html = render(&props, false, false, Language::En, None);

    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
}

#[test]
fn test_render_escapes_html_in_url() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        url: Some("post.html?a=1&b=2".to_string()),
        title: "Test".to_string(),
        ..Default::default()
    };
    let html = render(&props, false, false, Language::En, None);

    assert!(html.contains("a=1&amp;b=2"));
}

#[test]
fn test_render_minimal_shows_month_only() {
    let props = ArticleListItemProps {
        date_display: "2025 · 11".to_string(),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        url: Some("post.html".to_string()),
        title: "My Post".to_string(),
        ..Default::default()
    };
    let html = render(&props, true, false, Language::En, None);

    // Should show only month "11", not "2025 · 11"
    assert!(html.contains(r#"<span class="moss-prefix-link-prefix date"><time datetime="2025-11-17">11</time></span>"#));
    assert!(!html.contains("2025 "));
}

#[test]
fn test_render_minimal_fallback_without_raw_date() {
    let props = ArticleListItemProps {
        date_display: "2025-11-17".to_string(),
        url: Some("post.html".to_string()),
        title: "Test".to_string(),
        ..Default::default()
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
        url: Some("post.html".to_string()),
        title: "My Post".to_string(),
        ..Default::default()
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
        url: Some("post.html".to_string()),
        title: "Series Item".to_string(),
        ..Default::default()
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

/// A dateless, placeless leaf's meta is the credits alone: no separator
/// before them. Deleting the empty-base arm of `leaf_meta` makes this fail.
#[test]
fn leaf_meta_with_no_date_or_place_is_the_credits_alone() {
    let credits = r#"<span class="moss-card-credits">X</span>"#;
    assert_eq!(leaf_meta("", None, None, Some(credits)), credits);
}

/// The separator before an event's credits is a span, as the one before its
/// place is, so a theme can drop it.
#[test]
fn leaf_meta_separates_credits_from_an_event_time_with_a_span() {
    let credits = r#"<span class="moss-card-credits">c</span>"#;
    assert_eq!(
        leaf_meta("", Some("<time>x</time>"), None, Some(credits)),
        format!(r#"<time>x</time><span class="moss-event-sep"> · </span>{credits}"#)
    );
}

/// An event's listing row puts its place after the `<time>` as its own span,
/// behind a separator span; a non-event row keeps the bare text.
#[test]
fn render_child_wraps_an_event_place_in_its_own_span() {
    let when = crate::event::when::render_when(
        crate::event::EventTime::parse("2026-11-01 14:00").unwrap(),
        None,
        crate::i18n::Language::En,
    );
    let props = ChildItemProps {
        title: "Opening".to_string(),
        url: Some("/events/opening.html".to_string()),
        date_raw: Some("2026-11-01".to_string()),
        place: Some("Example Hall".to_string()),
        when_html: Some(when),
        ..Default::default()
    };
    let html = render_child(&props, crate::i18n::Language::En, None);
    assert!(
        html.contains(r#"</time><span class="moss-event-sep"> · </span><span class="moss-event-place">Example Hall</span>"#),
        "{html}"
    );
    assert!(!html.contains("</time> · "), "bare separator text must not remain: {html}");
}

/// An event row whose page has only `online:` keeps its title as the link and
/// carries the Online link after it, with no anchor around the whole row.
#[test]
fn render_child_event_row_with_only_online_links_title_and_carries_its_online_link() {
    let props = ChildItemProps {
        online: Some("https://example.test/live".to_string()),
        title: "Spring Gala".to_string(),
        url: Some("spring-gala.html".to_string()),
        date_raw: Some("2026-11-01".to_string()),
        ..Default::default()
    };
    let html = render_child(&props, crate::i18n::Language::En, None);
    assert!(html.contains(r#"<a class="moss-card-link" href="spring-gala.html">Spring Gala</a>"#), "{html}");
    assert!(
        html.contains(r#"<a class="moss-event-link moss-event-online" href="https://example.test/live">Online<span class="visually-hidden"> for Spring Gala</span></a>"#),
        "{html}"
    );
    assert_eq!(max_anchor_depth(&html), 1, "{html}");
}

/// A folder's count is not a date — its own `location:` (e.g. a self-named
/// work folder) must never leak into the count slot.
#[test]
fn render_child_never_shows_a_place_beside_a_folders_count() {
    let props = ChildItemProps {
        title: "Kyoto Walk".to_string(),
        url: Some("/works/kyoto-walk/".to_string()),
        child_count: Some(2),
        place: Some("Kyoto".to_string()),
        ..Default::default()
    };
    let html = render_child(&props, crate::i18n::Language::En, None);
    assert!(
        html.contains(r#"<span class="moss-prefix-link-prefix">2 articles</span>"#),
        "the count slot must be the count alone, not the count plus the place: {html}"
    );
}

/// The list card shape shared the summary and grid cards' bug in a second
/// form: it had no CJK-numeral branch at all, so a vertical CJK site
/// counted in Arabic digits, laid on their sides.
#[test]
fn folder_count_uses_chinese_numerals_in_vertical_cjk() {
    let props = ChildItemProps {
        title: "書".to_string(),
        url: Some("/書/".to_string()),
        child_count: Some(38),
        ..Default::default()
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
        url: Some("/tutorials/".to_string()),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        child_count: Some(4),
        ..Default::default()
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
        url: Some("/tutorials/".to_string()),
        date_raw: Some("2025-11-17T00:50:43.135Z".to_string()),
        child_count: Some(4),
        description: Some("Learn the basics of moss".to_string()),
        ..Default::default()
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
        url: Some("/test/?a=1&b=2".to_string()),
        child_count: Some(2),
        ..Default::default()
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
        url: Some("/blog/dateless/".to_string()),
        ..Default::default()
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
        url: Some("post.html".to_string()),
        title: "Series Item".to_string(),
        ..Default::default()
    };
    let html_minimal = render(&props, true, true, Language::En, None);
    let html_normal = render(&props, false, true, Language::En, None);

    // Both should be identical when no_date is true
    assert_eq!(
        html_minimal, html_normal,
        "no_date=true should make minimal flag irrelevant"
    );
}



// ── `meta_text`: the one owner `grid_card`/`child_summary`/`render_child` all call ─

#[test]
fn meta_text_is_a_leafs_date_and_place() {
    assert_eq!(meta_text(Some("1924 · 05"), None, Some("Cambridge"), None, None), "1924 · 05 · Cambridge");
}

#[test]
fn meta_text_is_a_bare_folders_count_alone_even_with_a_place() {
    // A folder with no date of its own: its `location:` (if any) is not a
    // date to sit beside — only the count shows.
    assert_eq!(meta_text(None, None, Some("Kyoto"), Some("2 articles"), None), "2 articles");
}

#[test]
fn meta_text_is_a_dated_folders_date_and_place_then_its_count() {
    assert_eq!(
        meta_text(Some("1928 · 10"), None, Some("Cambridge"), Some("6 articles"), None),
        "1928 · 10 · Cambridge · 6 articles"
    );
}

#[test]
fn meta_text_is_a_dated_folders_date_then_count_with_no_place() {
    assert_eq!(meta_text(Some("1928 · 10"), None, None, Some("6 articles"), None), "1928 · 10 · 6 articles");
}

// ── `with_place`: the one owner of "date text plus resolved place" ────────

#[test]
fn with_place_appends_the_place_after_a_middot() {
    assert_eq!(with_place("2025 · 11", Some("Kyoto")), "2025 · 11 · Kyoto");
}

#[test]
fn with_place_returns_the_text_unchanged_when_there_is_no_place() {
    assert_eq!(with_place("2025 · 11", None), "2025 · 11");
}

#[test]
fn with_place_returns_the_place_alone_when_the_text_is_empty() {
    assert_eq!(with_place("", Some("Hayes Court")), "Hayes Court");
}

#[test]
fn with_place_returns_empty_when_both_are_empty() {
    assert_eq!(with_place("", None), "");
}

/// A place name is author-supplied text, not machine-generated — it must be
/// escaped like any other frontmatter value reaching the page.
#[test]
fn with_place_escapes_an_unsafe_place_name() {
    let out = with_place("2025", Some("<script>"));
    assert!(!out.contains("<script>"), "place name must be escaped: {out}");
    assert!(out.contains("&lt;script&gt;"));
}

/// A blank (whitespace-only) place is treated the same as no place at all.
#[test]
fn with_place_ignores_a_blank_place() {
    assert_eq!(with_place("2025", Some("   ")), "2025");
}

fn event_row_props(tickets: Option<&str>) -> ChildItemProps {
    ChildItemProps {
        tickets: tickets.map(str::to_string),
        title: "Spring Gala".to_string(),
        url: Some("spring-gala.html".to_string()),
        date_display: Some("2026 · 11".to_string()),
        ..Default::default()
    }
}

#[test]
fn event_row_with_tickets_links_title_and_carries_its_own_tickets_link() {
    let props = event_row_props(Some("https://example.test/tickets"));
    let html = render_child(&props, Language::En, None);
    assert_eq!(
        html,
        r#"<div class="moss-card"><div class="moss-prefix-link"><span class="moss-prefix-link-prefix date">2026 · 11</span><span class="moss-prefix-link-title"><a class="moss-card-link" href="spring-gala.html">Spring Gala</a></span><a class="moss-event-link moss-event-tickets" href="https://example.test/tickets">Tickets<span class="visually-hidden"> for Spring Gala</span></a></div></div>"#
    );
    let title_at = html.find(r#"<a class="moss-card-link""#).unwrap();
    let tickets_at = html.find(r#"<a class="moss-event-link moss-event-tickets""#).unwrap();
    assert!(title_at < tickets_at, "got: {html}");
    assert_eq!(max_anchor_depth(&html), 1, "got: {html}");
}

#[test]
fn event_row_without_tickets_keeps_its_whole_row_anchor() {
    let props = event_row_props(None);
    let html = render_child(&props, Language::En, None);
    assert_eq!(
        html,
        r#"<div class="moss-card"><a href="spring-gala.html" class="moss-prefix-link"><span class="moss-prefix-link-prefix date">2026 · 11</span><span class="moss-prefix-link-title">Spring Gala</span></a></div>"#
    );
}
