use super::*;
use crate::i18n::Language;

#[test]
fn test_render_only_prev() {
    let html = render("S", "index.html", Some(("Prev", "/p/")), None, None, Language::En, None);
    assert!(html.contains("moss-series-nav-prev"));
    // Empty placeholder should still exist for layout
    assert!(html.contains("moss-series-nav-next"));
    // No prefetch when there's no next page
    assert!(
        !html.contains("rel=\"prefetch\""),
        "Should NOT emit prefetch link when next is None"
    );
}

#[test]
fn test_render_empty_series_name() {
    let html = render(
        "",
        "index.html",
        Some(("Prev", "/p/")),
        Some(("Next", "/n/")),
        Some((2, 3)),
        Language::En,
        None,
    );
    assert!(html.is_empty());
}

#[test]
fn test_render_two_row_layout_structure() {
    // Test that the series nav has the correct two-row layout structure
    // with separate wrapper elements for CSS targeting
    let html = render(
        "My Series",
        "/my-series/index.html",
        Some(("Previous Article", "/prev/")),
        Some(("Next Article", "/next/")),
        None,
        Language::En,
        None,
    );

    // Should have .moss-series-nav-links wrapper for the nav links
    assert!(
        html.contains(r#"class="moss-series-nav-links""#),
        "Should have moss-series-nav-links wrapper div for flexbox layout. Got: {}",
        html
    );

    // Should have .moss-series-nav-link class on links (not just moss-series-nav-prev/next)
    assert!(
        html.contains(r#"class="moss-series-nav-link moss-series-nav-prev""#),
        "Prev link should have both moss-series-nav-link and moss-series-nav-prev classes. Got: {}",
        html
    );
    assert!(
        html.contains(r#"class="moss-series-nav-link moss-series-nav-next""#),
        "Next link should have both moss-series-nav-link and moss-series-nav-next classes. Got: {}",
        html
    );

    // Should have separate spans for arrows and titles
    assert!(
        html.contains(r#"class="moss-series-nav-arrow""#),
        "Should have moss-series-nav-arrow span for fixed arrow positioning. Got: {}",
        html
    );
    assert!(
        html.contains(r#"class="moss-series-nav-title""#),
        "Should have moss-series-nav-title span for title text. Got: {}",
        html
    );

    // Should have folder row at the bottom, linking back to the folder
    assert!(
        html.contains(r#"class="moss-series-nav-collection-row""#),
        "Should have moss-series-nav-collection-row for centered folder link. Got: {}",
        html
    );
    assert!(
        html.contains(r#"href="/my-series/index.html""#)
            && html.contains(r#"class="moss-series-nav-collection""#),
        "Should link back to the folder index. Got: {}",
        html
    );

    // The series name, and a prefetch hint for the next page
    assert!(html.contains("My Series"), "Got: {}", html);
    assert!(
        html.contains(r#"<link rel="prefetch" href="/next/">"#),
        "Should emit prefetch link for the next page. Got: {}",
        html
    );

    // No position was passed, so none is claimed.
    assert!(
        !html.contains("moss-series-nav-position"),
        "Should not invent a position when the caller passed none. Got: {}",
        html
    );
}

#[test]
fn test_html_escaping() {
    let html = render(
        "Test & \"Quotes\"",
        "index.html",
        Some(("Title <with> special", "/url/")),
        None,
        None,
        Language::En,
        None,
    );
    assert!(html.contains("Test &amp; &quot;Quotes&quot;"));
    assert!(html.contains("Title &lt;with&gt; special"));
}

/// Arrows say what is next; they never say how far in you are. A reader who
/// arrives at part two from a shared link should be able to read the answer.
#[test]
fn position_is_rendered_in_the_page_language() {
    let en = render(
        "My Series",
        "index.html",
        Some(("Prev", "/p/")),
        Some(("Next", "/n/")),
        Some((2, 3)),
        Language::En,
        None,
    );
    assert!(
        en.contains(r#"<span class="moss-series-nav-position">2 of 3</span>"#),
        "Got: {}",
        en
    );

    let zh = render(
        "作品",
        "index.html",
        Some(("上篇", "/p/")),
        None,
        Some((3, 3)),
        Language::ZhHant,
        None,
    );
    assert!(
        zh.contains(r#"<span class="moss-series-nav-position">第 3 篇，共 3 篇</span>"#),
        "Got: {}",
        zh
    );
}

/// Vertical CJK typesetting converts the position numerals the same way
/// `article_count_label` converts a card's count (`use_cjk_numerals`): no
/// Arabic digits, and — because Chinese numerals are CJK glyphs themselves —
/// none of the spacing the horizontal template uses to set digits off from
/// the surrounding text (see the horizontal assertion above: "第 3 篇，共 3 篇").
#[test]
fn position_uses_chinese_numerals_under_vertical_cjk_typesetting() {
    let zh_vertical = render(
        "作品",
        "index.html",
        Some(("上篇", "/p/")),
        Some(("下篇", "/n/")),
        Some((1, 10)),
        Language::ZhHant,
        Some("vertical"),
    );
    assert!(
        zh_vertical.contains(r#"<span class="moss-series-nav-position">第一篇，共十篇</span>"#),
        "Got: {}",
        zh_vertical
    );
}
