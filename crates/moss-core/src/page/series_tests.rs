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

/// Arrow and placement show the direction, so the words "Previous" / "Next"
/// are never painted: each lives only in the link's visually hidden span,
/// where a screen reader hears it before the title.
#[test]
fn direction_words_are_hidden_from_sight_but_not_from_screen_readers() {
    for (lang, prev_word, next_word) in [
        (Language::En, "Previous", "Next"),
        (Language::ZhHans, "上一个", "下一个"),
        (Language::ZhHant, "上一個", "下一個"),
    ] {
        let html = render("作品", "index.html", Some(("甲", "/p/")), Some(("乙", "/n/")), None, lang, None);
        for (rel, word) in [("prev", prev_word), ("next", next_word)] {
            assert!(html.contains(&format!(r#"rel="{rel}""#)), "{lang:?}: Got: {html}");
            assert!(
                html.contains(&format!(r#"<span class="visually-hidden">{word}</span> <span class="moss-series-nav-title">"#)),
                "{lang:?}: `{word}` should be the hidden word read before the title. Got: {html}"
            );
        }
        let painted = html
            .replace(&format!(r#"<span class="visually-hidden">{prev_word}</span>"#), "")
            .replace(&format!(r#"<span class="visually-hidden">{next_word}</span>"#), "");
        for word in [prev_word, next_word] {
            assert!(!painted.contains(word), "{lang:?}: `{word}` is painted. Got: {html}");
        }
        assert_eq!(html.matches(r#"class="moss-series-nav-arrow" aria-hidden="true""#).count(), 2, "{lang:?}: Got: {html}");
    }
}

/// A page as a host knows it: the passes run over any [`SitePage`].
#[derive(Default)]
struct Page {
    url: &'static str,
    label: &'static str,
    folder: bool,
    weight: Option<i32>,
    date: Option<&'static str>,
    series: Option<SeriesField>,
    layout: Option<&'static str>,
    draft: Option<bool>,
    sort: Option<ResolvedSort>,
}

impl crate::sort::SortableDoc for Page {
    fn url_path(&self) -> &str { self.url }
    fn date(&self) -> Option<&str> { self.date }
    fn weight(&self) -> Option<i32> { self.weight }
    fn declared_sort(&self) -> Option<&crate::sort::SortField> { None }
    fn clean_stem(&self) -> &str { self.label }
    fn is_folder_index(&self) -> bool { self.folder }
}

impl crate::sort::SortableLabel for Page {
    fn label(&self) -> &str { self.label }
}

impl SitePage for Page {
    fn layout(&self) -> Option<&str> { self.layout }
    fn series(&self) -> Option<&SeriesField> { self.series.as_ref() }
    fn draft(&self) -> Option<bool> { self.draft }
    fn listed(&self) -> Option<bool> { None }
    fn search_engines(&self) -> Option<bool> { None }
    fn slot_only(&self) -> bool { false }
    fn hidden_home(&self) -> Option<&str> { None }
    fn direct_children_sort(&self) -> Option<&ResolvedSort> { self.sort.as_ref() }
    fn source_path(&self) -> Option<&str> { None }
    fn lang(&self) -> crate::i18n::Language { crate::i18n::Language::En }
    fn edition(&self) -> &str { "" }
    fn is_root_level(&self) -> bool { false }
    fn partial(&self) -> bool { false }
    fn nav(&self) -> Option<bool> { None }
    fn nav_label(&self) -> Option<&str> { None }
    fn nav_link(&self) -> Option<&str> { None }
    fn breadcrumb(&self) -> Option<crate::frontmatter_typed::BreadcrumbSetting> { None }
    fn translates_home(&self) -> bool { false }
    fn passthrough_root(&self) -> Option<&str> { None }
    fn also_in(&self) -> &[String] { &[] }
    fn translation_urls(&self) -> impl Iterator<Item = &str> { std::iter::empty() }
    fn children_source(&self) -> Option<&str> { None }
    fn children_depth(&self) -> Option<&str> { None }
    fn children_listed(&self) -> Option<&str> { None }
    fn term_listing(&self) -> Option<&str> { None }
    fn children_style(&self) -> Option<&crate::resolved::Resolved<String>> { None }
    fn children_group(&self) -> Option<&crate::resolved::Resolved<String>> { None }
    fn children_covers(&self) -> Option<&str> { None }
    fn children_when(&self) -> Option<&str> { None }
    fn children_limit(&self) -> Option<u32> { None }
    fn event_span(&self) -> Option<crate::event::EventSpan> { None }
    fn awaits_event_date(&self) -> bool { false }
}

fn by_weight() -> Option<ResolvedSort> {
    Some(ResolvedSort { axis: SortAxis::Weight, explicit_order: None })
}

fn weighted_series() -> Vec<Page> {
    vec![
        Page { url: "guide/index.html", label: "Guide", folder: true, sort: by_weight(), series: Some(SeriesField::Flag(true)), ..Default::default() },
        Page { url: "guide/two/index.html", label: "Two", weight: Some(2), ..Default::default() },
        Page { url: "guide/one/index.html", label: "One", weight: Some(1), ..Default::default() },
        Page { url: "guide/draft/index.html", label: "Draft", weight: Some(3), draft: Some(true), ..Default::default() },
        Page { url: "guide/deep/more/index.html", label: "Deeper", weight: Some(0), ..Default::default() },
        Page { url: "about/index.html", label: "About", ..Default::default() },
    ]
}

#[test]
fn a_weighted_folder_chains_its_listable_direct_children_in_weight_order() {
    let pages = weighted_series();
    let parent = series_parent(&pages[1], &pages).expect("a step in a weighted folder");
    assert_eq!(parent.url, "guide/index.html");
    let labels: Vec<&str> = chain(&pages, parent).iter().map(|p| p.label).collect();
    assert_eq!(labels, ["One", "Two"], "a draft and a grandchild are not steps");
}

#[test]
fn ordering_a_folder_does_not_make_it_a_series() {
    let mut pages = weighted_series();
    pages[0].series = None;
    assert!(series_parent(&pages[1], &pages).is_none(), "a weight orders the listing, it does not chain the pages");
}

#[test]
fn a_dated_folder_reads_forward_in_time() {
    let mut pages = vec![
        Page { url: "log/index.html", label: "Log", folder: true, series: Some(SeriesField::Flag(true)), ..Default::default() },
        Page { url: "log/new/index.html", label: "New", date: Some("2026-02-01"), ..Default::default() },
        Page { url: "log/old/index.html", label: "Old", date: Some("2026-01-01"), ..Default::default() },
    ];
    let labels: Vec<&str> = chain(&pages, &pages[0]).iter().map(|p| p.label).collect();
    assert_eq!(labels, ["Old", "New"]);
    pages[0].series = None;
    assert!(series_parent(&pages[1], &pages).is_none(), "a dated folder that says nothing has no series chrome");
}

#[test]
fn an_opted_out_page_and_a_top_level_page_have_no_series_parent() {
    let mut pages = weighted_series();
    pages[1].series = Some(SeriesField::Flag(false));
    assert!(series_parent(&pages[1], &pages).is_none());
    assert!(chain(&pages, &pages[0]).iter().all(|p| p.label != "Two"), "the opt-out leaves the whole chain");
    assert!(series_parent(&pages[5], &pages).is_none(), "about/index.html has no folder above its own");
}

#[test]
fn a_folder_home_is_a_step_only_when_laid_out_as_an_article() {
    let mut pages = weighted_series();
    pages.push(Page { url: "guide/part/index.html", label: "Part", folder: true, weight: Some(4), ..Default::default() });
    assert!(series_parent(&pages[6], &pages).is_none());
    pages[6].layout = Some("article");
    assert_eq!(series_parent(&pages[6], &pages).map(|p| p.url), Some("guide/index.html"));
}
