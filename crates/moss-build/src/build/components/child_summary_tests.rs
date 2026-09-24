use super::*;

fn article_props(title: &str, url: &str) -> ChildItemProps {
    ChildItemProps {
        title: title.to_string(),
        url: url.to_string(),
        date_display: Some("2025 · 03 · 01".to_string()),
        date_raw: Some("2025-03-01".to_string()),
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
        url_path: String::new(),
    }
}

fn folder_props(title: &str, url: &str, count: usize) -> ChildItemProps {
    ChildItemProps {
        title: title.to_string(),
        url: url.to_string(),
        date_display: None,
        date_raw: None,
        child_count: Some(count),
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
        url_path: String::new(),
    }
}

#[test]
fn test_render_article_card_basic() {
    let props = article_props("My Article", "/blog/my-article/");
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(html.contains(r#"class="moss-card""#));
    assert!(html.contains(r#"href="/blog/my-article/""#));
    assert!(html.contains(r#"class="moss-card-meta">"#));
    assert!(html.contains("2025 · 03 · 01"));
    assert!(html.contains(r#"class="moss-card-title">My Article</h3>"#));
    assert!(!html.contains("moss-card-cover"));
    assert!(!html.contains("moss-card-description"));
    // Meta then title, directly inside moss-card-body: no head wrapper.
    assert!(html.contains(r#"moss-card-body"><div class="moss-card-meta">"#));
    assert!(!html.contains("moss-card-head"));
}

#[test]
fn test_render_folder_card_basic() {
    let props = folder_props("Tutorials", "/tutorials/", 4);
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(
        html.contains(r#"<div class="moss-card-meta">4 篇</div>"#),
        "the count is the folder's meta on every axis, got: {}",
        html
    );
}

#[test]
fn test_render_card_with_cover() {
    let mut props = article_props("Photo Post", "/blog/photo/");
    props.cover = Some("/images/cover.jpg".to_string());
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(html.contains(r#"class="moss-card-cover">"#));
    assert!(html.contains(r#"<img src="/images/cover.jpg" alt="Photo Post" />"#));
    // Cover should be inside moss-card-row alongside body
    assert!(html.contains(r#"moss-card-row"><div class="moss-card-body">"#));
    assert!(html.contains(r#"</div><div class="moss-card-cover">"#));
}

#[test]
fn test_render_card_with_description() {
    let mut props = article_props("Deep Dive", "/blog/deep-dive/");
    props.description = Some("An in-depth exploration.".to_string());
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(html.contains(r#"<p class="moss-card-description">An in-depth exploration.</p>"#));
}

#[test]
fn test_render_card_full() {
    let mut props = article_props("Full Article", "/blog/full/");
    props.description = Some("Everything included.".to_string());
    props.cover = Some("/cover.jpg".to_string());
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(html.contains("moss-card-meta"));
    assert!(html.contains("moss-card-title"));
    assert!(html.contains("moss-card-description"));
    assert!(html.contains("moss-card-cover"));
    assert!(!html.contains("child-summary-count"));
}

#[test]
fn test_render_folder_card_full() {
    let mut props = folder_props("Recipes", "/recipes/", 12);
    props.description = Some("Delicious recipes.".to_string());
    props.cover = Some("/food.jpg".to_string());
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(html.contains(r#"<div class="moss-card-meta">12 篇</div>"#), "{html}");
    assert!(html.contains("moss-card-title"));
    assert!(html.contains("moss-card-description"));
    assert!(!html.contains("child-summary-count"));
    assert!(html.contains("moss-card-cover"));
}

#[test]
fn test_render_card_escapes_html() {
    let props = ChildItemProps {
        title: "<script>alert('xss')</script>".to_string(),
        url: "/test/?a=1&b=2".to_string(),
        date_display: Some("<b>bold</b>".to_string()),
        date_raw: None,
        child_count: None,
        description: Some("A & B < C".to_string()),
        cover: Some("/img?x=1&y=2".to_string()),
        cover_type: None,
        kicker: None,
        permalink: None,
        url_path: String::new(),
    };
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("a=1&amp;b=2"));
    assert!(html.contains("&lt;b&gt;bold&lt;/b&gt;"));
    assert!(html.contains("A &amp; B &lt; C"));
    assert!(html.contains("x=1&amp;y=2"));
}

// ── Video cover tests ────────────────────────────────────────

#[test]
fn test_render_card_with_video_cover() {
    let mut props = article_props("Video Post", "/blog/video/");
    props.cover = Some("/media/clip.mp4".to_string());
    props.cover_type = Some(CoverType::Video);
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(
        html.contains("<video"),
        "Video should render as <video> tag"
    );
    assert!(html.contains(r#"src="/media/clip.mp4""#));
    assert!(html.contains("moss-card-cover"));
    assert!(html.starts_with("<a "), "Video card should use <a> wrapper");
}

#[test]
fn test_render_card_with_iframe_cover() {
    let mut props = article_props("Interactive Post", "/blog/interactive/");
    props.cover = Some("/widgets/demo.html".to_string());
    props.cover_type = Some(CoverType::Iframe);
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(html.contains("<iframe"), "Should render an <iframe> tag");
    assert!(html.contains(r#"src="/widgets/demo.html""#));
    assert!(html.contains("moss-card-cover"));
    // Should use <a> wrapper (iframe has pointer-events overlay)
    assert!(
        html.starts_with("<a "),
        "Iframe card should use <a> wrapper, got: {}",
        html
    );
    // Title should be plain (no link inside)
    assert!(html.contains(r#"<h3 class="moss-card-title">Interactive Post</h3>"#));
}

#[test]
fn test_render_card_cover_type_defaults_to_image() {
    // cover is Some but cover_type is None => defaults to Image
    let mut props = article_props("Default Cover", "/blog/default/");
    props.cover = Some("/images/photo.jpg".to_string());
    // cover_type is None (default)
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(html.contains("<img"), "Should default to <img> tag");
    assert!(html.starts_with("<a "), "Image card should use <a> wrapper");
}

#[test]
fn test_render_card_no_cover_ignores_cover_type() {
    // cover is None, cover_type is Some => no cover rendered
    let mut props = article_props("No Cover", "/blog/no-cover/");
    props.cover_type = Some(CoverType::Video);
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(!html.contains("moss-card-cover"));
    assert!(!html.contains("<video"));
}

// ── Folder-card meta on non-Date axis ──────────────────────
//
// The previous CJK-numeral tests covered the "N 篇" count rendered
// inside the meta slot. After Task 9 the count moves to a separate
// subtitle slot (Task 10), and the meta slot is fully omitted on
// non-Date axes regardless of locale/typesetting. The tests below
// pin that contract; the CJK numeral helper is still exercised by
// `moss_core::numerals::to_chinese_numeral` unit tests directly.

#[test]
fn test_render_card_with_pipe_encoded_cover_strips_attrs_from_src() {
    let mut props = article_props("Photo Post", "/blog/photo/");
    props.cover = Some("/images/cover.jpg|left".to_string());
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    // src should contain only the path, not the pipe attrs
    assert!(
        html.contains(r#"src="/images/cover.jpg""#),
        "src should not contain pipe attrs. Got: {}",
        html
    );
    assert!(
        !html.contains(r#"src="/images/cover.jpg|left""#),
        "Pipe should be stripped from src. Got: {}",
        html
    );
}

#[test]
fn test_render_card_with_pipe_encoded_cover_adds_style() {
    let mut props = article_props("Photo Post", "/blog/photo/");
    props.cover = Some("/images/cover.jpg|left".to_string());
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(
        html.contains(r#"style="object-position:left""#),
        "Should add position style. Got: {}",
        html
    );
}

#[test]
fn test_render_card_with_pipe_encoded_cover_fit_and_position() {
    let mut props = article_props("Photo Post", "/blog/photo/");
    props.cover = Some("/images/cover.jpg|contain top-right".to_string());
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(html.contains(r#"src="/images/cover.jpg""#), "Got: {}", html);
    assert!(
        html.contains(r#"style="object-fit:contain;object-position:top right""#),
        "Got: {}",
        html
    );
}

#[test]
fn test_render_card_without_pipe_no_style() {
    let mut props = article_props("Photo Post", "/blog/photo/");
    props.cover = Some("/images/cover.jpg".to_string());
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHans,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(html.contains(r#"src="/images/cover.jpg""#));
    assert!(
        !html.contains("style="),
        "No pipe means no style attr. Got: {}",
        html
    );
}

#[test]
fn renders_permalink_star_inside_kicker_when_external_url_overrides_href() {
    // Linkblog card: outer is `<div class="moss-card">` (not <a>),
    // title + cover wrap individual anchors, kicker contains the
    // `★` inline at its end. Visual layout matches ordinary cards
    // (kicker stays inside the card body next to the title) and
    // the ★ reads as part of the kicker line: "Publisher · 2024 ★".
    let props = ChildItemProps {
        title: "Article Title".to_string(),
        url: "https://outlet.example/article".to_string(),
        date_display: Some("2024 · 12".to_string()),
        date_raw: Some("2024-12-23".to_string()),
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: Some("The Wire China".to_string()),
        permalink: Some("/works/article/".to_string()),
        url_path: String::new(),
    };
    let html = render_with_sort(
        &props,
        crate::i18n::Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(
        html.starts_with(r#"<div class="moss-card" data-linkblog>"#),
        "linkblog card outer is a <div>, not <a>; got: {}",
        html
    );
    // Kicker contains the combined publisher · year AND the ★ anchor.
    assert!(
            html.contains(r#"<div class="moss-card-kicker">The Wire China · 2024 <a class="moss-card-permalink" href="/works/article/""#),
            "kicker contains text + ★ anchor inline; got: {}", html
        );
    // The kicker is the only place the ★ appears (no overhead row,
    // no separate kicker block elsewhere).
    let kicker_count = html.matches("moss-card-kicker").count();
    assert_eq!(
        kicker_count, 1,
        "kicker rendered exactly once; got {} occurrences in: {}",
        kicker_count, html
    );
    assert!(html.contains(">★</a>"), "permalink mark is the literal ★");
    // Title wrapped in its own anchor going to canonical.
    assert!(
        html.contains(r#"<a class="moss-card-title-link" href="https://outlet.example/article">"#),
        "title is wrapped in a canonical anchor; got: {}",
        html
    );
    assert!(
        html.contains(r#"aria-label="Permalink to Article Title""#),
        "aria-label carries the title for screen readers"
    );
}

#[test]
fn omits_permalink_when_kicker_is_none_even_with_external_url() {
    // Permalink without a kicker text would have no kicker DIV to
    // attach the ★ to. Falls back to the ordinary card-anchor path
    // (whole card → external URL); no ★. The archive URL stays
    // reachable via direct URL.
    let props = ChildItemProps {
        title: "Article Title".to_string(),
        url: "https://outlet.example/article".to_string(),
        date_display: None,
        date_raw: None,
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: Some("/works/article/".to_string()),
        url_path: String::new(),
    };
    let html = render_with_sort(
        &props,
        crate::i18n::Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(
        !html.contains("moss-card-permalink"),
        "no ★ when there's no kicker to host it; got: {}",
        html
    );
    assert!(
        !html.contains("data-linkblog"),
        "no data-linkblog marker; falls back to ordinary card; got: {}",
        html
    );
    assert!(html.starts_with(r#"<a href="https://outlet.example/article""#));
}

#[test]
fn cjk_date_keeps_separate_meta_inside_card() {
    // CJK-numeral dates (e.g. "二〇二四年") fail year extraction; the
    // kicker can't absorb the year. With permalink Some, the meta
    // hoists into the overhead row beside the kicker so publisher
    // and date stay on the same line instead of splitting across
    // the card boundary.
    let props = ChildItemProps {
        title: "Article".to_string(),
        url: "https://outlet.example/".to_string(),
        date_display: Some("二〇二四年十二月".to_string()),
        date_raw: None,
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: Some("遠聲媒體".to_string()),
        permalink: Some("/archive/foo/".to_string()),
        url_path: String::new(),
    };
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHant,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    // Linkblog card with CJK date: kicker (publisher only) inside
    // card-head with ★; meta renders separately also inside card-head.
    // Both are inside the card-body, no split across the card boundary.
    assert!(
        html.contains(r#"<div class="moss-card-kicker">遠聲媒體 <a class="moss-card-permalink""#),
        "kicker contains publisher + ★; got: {}",
        html
    );
    assert!(
        html.contains(r#"<div class="moss-card-meta">二〇二四年十二月</div>"#),
        "CJK date renders as separate meta div inside card-head; got: {}",
        html
    );
    assert!(
        html.starts_with(r#"<div class="moss-card" data-linkblog>"#),
        "linkblog outer div; got: {}",
        html
    );
}

#[test]
fn omits_permalink_when_no_external_url() {
    // Ordinary (non-linkblog) cards have a single URL — no second
    // anchor, no wrapper.
    let props = ChildItemProps {
        title: "Local Post".to_string(),
        url: "/posts/local/".to_string(),
        date_display: None,
        date_raw: None,
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
        url_path: String::new(),
    };
    let html = render_with_sort(
        &props,
        crate::i18n::Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(
        !html.contains("moss-card-permalink"),
        "no permalink mark when permalink is None; got: {}",
        html
    );
    assert!(
        !html.contains("moss-card-wrapper"),
        "no wrapper when permalink is None; got: {}",
        html
    );
    assert!(html.starts_with(r#"<a href="/posts/local/""#));
}

#[test]
fn renders_kicker_above_title_horizontal() {
    // When a Date-axis card has both kicker and date, the date's year
    // is absorbed into the kicker as "{kicker} · {YYYY}" and the
    // separate meta slot is suppressed (no duplication).
    let props = ChildItemProps {
        title: "Notes on writing".to_string(),
        url: "/posts/notes/".to_string(),
        date_display: Some("2025 · 04".to_string()),
        date_raw: Some("2025-04-01".to_string()),
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: Some("ESSAYS".to_string()),
        permalink: None,
        url_path: String::new(),
    };
    let html = render_with_sort(
        &props,
        crate::i18n::Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    let kicker_pos = html
        .find(r#"<div class="moss-card-kicker""#)
        .expect("kicker present");
    let title_pos = html
        .find(r#"<h3 class="moss-card-title""#)
        .expect("title present");
    assert!(
        kicker_pos < title_pos,
        "kicker before title in horizontal mode"
    );
    assert!(
        html.contains(">ESSAYS · 2025</div>"),
        "kicker combines publisher + year, got: {}",
        html
    );
    assert!(
        !html.contains(r#"class="moss-card-meta""#),
        "meta slot suppressed when kicker carries the date"
    );
}

#[test]
fn vertical_head_order_matches_horizontal() {
    // CJK-numeral dates can't be year-extracted, so the kicker stays
    // as-is and the meta slot keeps the formatted CJK date. Vertical
    // typesetting keeps the horizontal reading order — kicker, meta,
    // title — so the brow leads the card in both directions; the
    // page's writing mode does the rotation, not the markup.
    let props = ChildItemProps {
        title: "在公開場合寫作".to_string(),
        url: "/posts/notes/".to_string(),
        date_display: Some("二〇二五·四".to_string()),
        date_raw: None,
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: Some("FOLDER".to_string()),
        permalink: None,
        url_path: String::new(),
    };
    let html = render_with_sort(
        &props,
        crate::i18n::Language::ZhHant,
        Some("vertical"),
        None,
        moss_core::sort::SortAxis::Date,
    );
    let title_pos = html
        .find(r#"<h3 class="moss-card-title""#)
        .expect("title present");
    let kicker_pos = html
        .find(r#"<div class="moss-card-kicker""#)
        .expect("kicker present");
    let meta_pos = html
        .find(r#"<div class="moss-card-meta""#)
        .expect("meta present");
    assert!(kicker_pos < meta_pos, "kicker before meta in vertical mode");
    assert!(meta_pos < title_pos, "meta before title in vertical mode");
    assert!(!html.contains("moss-card-head"), "no head wrapper in vertical mode");
    assert!(
        html.contains(">FOLDER</div>"),
        "kicker stays as-is when date can't be year-extracted"
    );
}

// ── render_with_sort: meta slot is sort-driven (Task 9) ─────

#[test]
fn meta_renders_date_when_axis_is_date() {
    let props = ChildItemProps {
        title: "Hello".into(),
        url: "/h/".into(),
        date_display: Some("2025 · 03".into()),
        date_raw: Some("2025-03-01".into()),
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
        url_path: String::new(),
    };
    let html = render_with_sort(
        &props,
        crate::i18n::Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Date,
    );
    assert!(html.contains(r#"class="moss-card-meta""#));
    assert!(html.contains("2025 · 03"));
}

#[test]
fn meta_collapses_when_axis_is_title() {
    let props = ChildItemProps {
        title: "Project".into(),
        url: "/p/".into(),
        date_display: Some("2025 · 03".into()),
        date_raw: None,
        child_count: None,
        description: Some("A project".into()),
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
        url_path: String::new(),
    };
    let html = render_with_sort(
        &props,
        crate::i18n::Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Title,
    );
    assert!(
        !html.contains(r#"class="moss-card-meta""#),
        "meta slot must be fully omitted (no empty div) when axis != Date"
    );
}

#[test]
fn meta_collapses_when_axis_is_weight() {
    let props = ChildItemProps {
        title: "Intro".into(),
        url: "/i/".into(),
        date_display: None,
        date_raw: None,
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
        url_path: String::new(),
    };
    let html = render_with_sort(
        &props,
        crate::i18n::Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Weight,
    );
    assert!(!html.contains(r#"class="moss-card-meta""#));
}

fn folder(description: Option<&str>, date_display: Option<&str>) -> ChildItemProps {
    let mut props = folder_props("Tutorials", "/t/", 4);
    props.description = description.map(str::to_string);
    props.date_display = date_display.map(str::to_string);
    props
}

/// A folder's count is its meta, whatever else the card carries: a
/// description does not displace it, and on a date listing it wins over the
/// folder's own date.
#[test]
fn folder_count_fills_the_meta_slot() {
    for (desc, date, axis) in [
        (None, None, moss_core::sort::SortAxis::Title),
        (Some("Get started"), None, moss_core::sort::SortAxis::Title),
        (Some("Get started"), Some("2025 · 04"), moss_core::sort::SortAxis::Date),
    ] {
        let html = render_with_sort(&folder(desc, date), crate::i18n::Language::En, None, None, axis);
        assert!(
            html.contains(r#"<div class="moss-card-meta">4 articles</div>"#),
            "axis {axis:?}, description {desc:?}: {html}"
        );
        assert!(!html.contains("2025 · 04"), "a folder's own date is not what it is picked by");
    }
}

/// The two card shapes agree. Asserted alone, each emitter looked right
/// while the summary card silently dropped the count the grid card showed.
#[test]
fn grid_and_summary_folder_cards_carry_the_same_count() {
    let props = folder(Some("Get started"), None);
    let summary = render_with_sort(&props, crate::i18n::Language::En, None, None, moss_core::sort::SortAxis::Title);
    let grid = crate::build::components::grid_card::render_item_with_typesetting(
        &props,
        None,
        crate::i18n::Language::En,
        None,
        None,
        false,
    );
    assert!(summary.contains("4 articles"), "{summary}");
    assert!(grid.contains("4 articles"), "{grid}");
}
