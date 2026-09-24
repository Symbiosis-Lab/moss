use super::*;

/// A card with nothing filled in — every test names only what it is about.
fn blank() -> ChildItemProps {
    ChildItemProps {
        title: String::new(),
        url: String::new(),
        date_display: None,
        date_raw: None,
        child_count: None,
        description: None,
        cover: None,
        cover_type: None,
        kicker: None,
        permalink: None,
        url_path: String::new(),
    }
}

#[test]
fn test_render_item_english() {
    let props = ChildItemProps {
        title: "Posts".to_string(),
        url: "posts/".to_string(),
        child_count: Some(5),
        ..blank()
    };

    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(html.contains("5 articles"));
}

#[test]
fn test_render_item_chinese() {
    let props = ChildItemProps {
        title: "Posts".to_string(),
        url: "posts/".to_string(),
        child_count: Some(5),
        ..blank()
    };

    let html = render_item_with_typesetting(&props, None, Language::ZhHans, None, None, false);
    assert!(html.contains("5 篇"));
}

#[test]
fn test_render_item_with_cover() {
    let props = ChildItemProps {
        title: "Travel".to_string(),
        url: "travel/".to_string(),
        child_count: Some(3),
        cover: Some("https://example.com/cover.jpg".to_string()),
        ..blank()
    };

    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(html.contains(r#"<img src="https://example.com/cover.jpg""#));
}

#[test]
fn test_render_item_escapes_html() {
    let props = ChildItemProps {
        title: "Code & Tips".to_string(),
        url: "code/".to_string(),
        child_count: Some(2),
        ..blank()
    };

    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(html.contains("Code &amp; Tips"));
}

#[test]
fn test_render_list_empty() {
    let cards: Vec<ChildItemProps> = vec![];
    let html = render_list_with_typesetting(
        &cards,
        None,
        Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Title,
        false,
        &Default::default(),
    );
    assert!(html.is_empty());
}

#[test]
fn test_render_list_multiple() {
    let cards = vec![
        ChildItemProps {
            title: "Posts".to_string(),
            url: "posts/".to_string(),
            child_count: Some(5),
            ..blank()
        },
        ChildItemProps {
            title: "Projects".to_string(),
            url: "projects/".to_string(),
            child_count: Some(3),
            cover: Some("cover.jpg".to_string()),
            ..blank()
        },
    ];

    let html = render_list_with_typesetting(
        &cards,
        None,
        Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Title,
        false,
        &Default::default(),
    );
    assert!(html.contains("5 articles"));
    assert!(html.contains("3 articles"));
}

#[test]
fn test_render_item_with_subtitle_override() {
    let props = ChildItemProps {
        title: "My Article".to_string(),
        url: "posts/my-article/".to_string(),
        date_display: Some("2025 · 06".to_string()),
        cover: Some("cover.jpg".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains("2025 · 06"),
        "Should use subtitle override. Got: {}",
        html
    );
    assert!(
        !html.contains("0 articles"),
        "Should not show article count. Got: {}",
        html
    );
}

#[test]
fn test_render_item_cover_uses_img_tag() {
    let props = ChildItemProps {
        title: "Travel".to_string(),
        url: "travel/".to_string(),
        child_count: Some(3),
        cover: Some("assets/photo.jpg".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains(r#"<img src="assets/photo.jpg""#),
        "Cover should use <img> tag. Got: {}",
        html
    );
    assert!(
        html.contains("moss-card-cover"),
        "Cover should be wrapped in .moss-card-cover div. Got: {}",
        html
    );
}

#[test]
fn test_render_item_no_cover_no_img() {
    let props = ChildItemProps {
        title: "Drafts".to_string(),
        url: "drafts/".to_string(),
        child_count: Some(2),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        !html.contains("<img"),
        "No-cover card should not have <img>. Got: {}",
        html
    );
    assert!(
        html.contains("moss-card-no-cover"),
        "No-cover card should have no-cover class. Got: {}",
        html
    );
}

// ── Quote card: coverless card in a covered list ─────────────

/// A card
/// with no cover, inside a list where other cards DO have one, fills the
/// cover slot with its description text (`data-cover="quote"`) instead of
/// the empty `.moss-card-no-cover` placeholder — and does not also print
/// the description below the title, or it would show twice.
#[test]
fn coverless_card_in_covered_list_gets_quote_slot() {
    let cards = vec![
        ChildItemProps {
            title: "First".to_string(),
            url: "first/".to_string(),
            cover: Some("cover1.jpg".to_string()),
            ..blank()
        },
        ChildItemProps {
            title: "Second".to_string(),
            url: "second/".to_string(),
            cover: Some("cover2.jpg".to_string()),
            ..blank()
        },
        ChildItemProps {
            title: "A Letter".to_string(),
            url: "letter/".to_string(),
            description: Some("Dear friend, the rain has stopped.".to_string()),
            ..blank()
        },
    ];

    let html = render_list_with_typesetting(
        &cards,
        None,
        Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Title,
        false,
        &Default::default(),
    );

    let letter_start = html
        .find(r#"href="letter/""#)
        .expect("letter card present");
    let letter_html = &html[letter_start..];

    assert!(
        letter_html.contains(r#"data-cover="quote""#),
        "coverless card in a covered list must get the quote slot. Got: {}",
        letter_html
    );
    assert!(
        letter_html.contains("Dear friend, the rain has stopped."),
        "quote slot must carry the description text. Got: {}",
        letter_html
    );
    assert!(
        !letter_html.contains("moss-card-description"),
        "description must not ALSO render below the title. Got: {}",
        letter_html
    );

    // Siblings with real covers are unaffected.
    let first_start = html.find(r#"href="first/""#).expect("first card present");
    let first_html = &html[first_start..letter_start];
    assert!(
        !first_html.contains(r#"data-cover="quote""#),
        "a card with its own cover must not get the quote slot. Got: {}",
        first_html
    );
}

/// With no description either, the quote slot falls back to the title.
#[test]
fn coverless_card_without_description_shows_title_in_quote_slot() {
    let cards = vec![
        ChildItemProps {
            title: "Has Cover".to_string(),
            url: "has-cover/".to_string(),
            cover: Some("cover.jpg".to_string()),
            ..blank()
        },
        ChildItemProps {
            title: "Untitled Note".to_string(),
            url: "note/".to_string(),
            ..blank()
        },
    ];

    let html = render_list_with_typesetting(
        &cards,
        None,
        Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Title,
        false,
        &Default::default(),
    );

    let note_start = html.find(r#"href="note/""#).expect("note card present");
    let note_html = &html[note_start..];
    assert!(
        note_html.contains(r#"data-cover="quote""#),
        "Got: {}",
        note_html
    );
    let cover_slot_end = note_html.find("moss-card-content").unwrap_or(note_html.len());
    assert!(
        note_html[..cover_slot_end].contains("Untitled Note"),
        "quote slot must fall back to the title when there is no description. Got: {}",
        note_html
    );
}

/// A coverless card in a list with NO covers at all keeps the plain
/// `.moss-card-no-cover` placeholder — the quote slot is only for the
/// mixed case.
#[test]
fn coverless_card_in_coverless_list_keeps_no_cover_placeholder() {
    let cards = vec![ChildItemProps {
        title: "Plain".to_string(),
        url: "plain/".to_string(),
        description: Some("A description.".to_string()),
        ..blank()
    }];

    let html = render_list_with_typesetting(
        &cards,
        None,
        Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Title,
        false,
        &Default::default(),
    );
    assert!(
        html.contains("moss-card-no-cover"),
        "Got: {}",
        html
    );
    assert!(
        !html.contains(r#"data-cover="quote""#),
        "a uniformly coverless list must not get the quote slot. Got: {}",
        html
    );
}

#[test]
fn test_render_item_cover_img_has_alt() {
    let props = ChildItemProps {
        title: "Travel".to_string(),
        url: "travel/".to_string(),
        child_count: Some(3),
        cover: Some("assets/photo.jpg".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains(r#"alt=""#),
        "Cover <img> should have alt attribute. Got: {}",
        html
    );
}

#[test]
fn file_card_renders_description_under_title() {
    let props = ChildItemProps {
        title: "My Article".to_string(),
        url: "posts/my-article/".to_string(),
        // A file/article card.
        date_display: Some("2025 · 06".to_string()),
        description: Some("A short description.".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains(r#"<p class="moss-card-description">A short description.</p>"#),
        "file card should render a description paragraph. Got: {}",
        html
    );
    // Coexistence: the date survives in the meta slot ABOVE the title while
    // the description renders BELOW it — they are not mutually exclusive.
    assert!(
        html.contains(r#"<span class="moss-card-meta">2025 · 06</span>"#),
        "date must remain in the meta slot when a description is also present. Got: {}",
        html
    );
    let meta_pos = html.find(r#"class="moss-card-meta"#).expect("meta present");
    let title_pos = html
        .find(r#"class="moss-card-title"#)
        .expect("title present");
    let desc_pos = html
        .find(r#"class="moss-card-description"#)
        .expect("description present");
    assert!(
        meta_pos < title_pos,
        "date (meta) must render above the title. Got: {}",
        html
    );
    assert!(
        title_pos < desc_pos,
        "description must render AFTER (below) the title. Got: {}",
        html
    );
}

#[test]
fn folder_card_does_not_render_description() {
    let props = ChildItemProps {
        title: "Travel".to_string(),
        url: "travel/".to_string(),
        child_count: Some(3), // folder card
        description: Some("A collection of travel stories.".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        !html.contains("moss-card-description"),
        "folder card must NOT render a description. Got: {}",
        html
    );
    assert!(
        html.contains("3 articles"),
        "folder card should still show its count. Got: {}",
        html
    );
}

#[test]
fn file_card_without_description_has_no_description_element() {
    let props = ChildItemProps {
        title: "Posts".to_string(),
        url: "posts/".to_string(),
        date_display: Some(String::new()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        !html.contains("moss-card-description"),
        "no description field => no description element. Got: {}",
        html
    );
}

#[test]
fn file_card_blank_description_is_not_rendered() {
    let props = ChildItemProps {
        title: "Posts".to_string(),
        url: "posts/".to_string(),
        description: Some("   ".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        !html.contains("moss-card-description"),
        "whitespace-only description must not render. Got: {}",
        html
    );
}

#[test]
fn file_card_escapes_description() {
    let props = ChildItemProps {
        title: "Posts".to_string(),
        url: "posts/".to_string(),
        description: Some("Tips & tricks <b>".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains("Tips &amp; tricks &lt;b&gt;"),
        "description must be HTML-escaped. Got: {}",
        html
    );
}

// ── Video cover tests ────────────────────────────────────────

#[test]
fn test_render_item_with_video_cover() {
    let props = ChildItemProps {
        title: "Demos".to_string(),
        url: "demos/".to_string(),
        child_count: Some(2),
        cover: Some("clip.mp4".to_string()),
        cover_type: Some(CoverType::Video),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains("<video"),
        "Video should render as <video> tag. Got: {}",
        html
    );
    assert!(html.contains(r#"src="clip.mp4""#));
    assert!(html.contains("moss-card-cover"));
    assert!(
        html.starts_with("<a "),
        "Video card should use <a> wrapper. Got: {}",
        html
    );
}

#[test]
fn test_render_item_with_iframe_cover() {
    let props = ChildItemProps {
        title: "Widgets".to_string(),
        url: "widgets/".to_string(),
        child_count: Some(1),
        cover: Some("demo.html".to_string()),
        cover_type: Some(CoverType::Iframe),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains("<iframe"),
        "Should render an <iframe> tag. Got: {}",
        html
    );
    // Should use <a> wrapper (iframe has pointer-events overlay)
    assert!(
        html.starts_with("<a "),
        "Iframe card should use <a> wrapper. Got: {}",
        html
    );
}

#[test]
fn test_render_item_cover_type_defaults_to_image() {
    let props = ChildItemProps {
        title: "Photos".to_string(),
        url: "photos/".to_string(),
        child_count: Some(5),
        cover: Some("photo.jpg".to_string()),
        cover_type: None, // should default to Image,
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains("<img"),
        "Should default to <img> tag. Got: {}",
        html
    );
    assert!(html.starts_with("<a "), "Image card should use <a> wrapper");
}

// ── Above-fold / eager-loading tests ──────────────────────

#[test]
fn first_card_in_list_gets_eager_loading() {
    let cards = vec![
        ChildItemProps {
            title: "First".to_string(),
            url: "first/".to_string(),
            child_count: Some(5),
            cover: Some("cover1.jpg".to_string()),
            ..blank()
        },
        ChildItemProps {
            title: "Second".to_string(),
            url: "second/".to_string(),
            child_count: Some(3),
            cover: Some("cover2.jpg".to_string()),
            ..blank()
        },
    ];

    let html = render_list_with_typesetting(
        &cards,
        None,
        Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Title,
        false,
        &Default::default(),
    );
    // First card's img should have loading="eager" (fetchpriority added later by placeholder.rs)
    let first_card_pos = html.find("cover1.jpg").unwrap();
    let second_card_pos = html.find("cover2.jpg").unwrap();
    let first_card_html = &html[..second_card_pos];
    let second_card_html = &html[second_card_pos..];

    assert!(
        first_card_html.contains(r#"loading="eager""#),
        "First card should have loading=eager. Got: {}",
        first_card_html
    );
    assert!(
        !second_card_html.contains(r#"loading="eager""#),
        "Second card should NOT have loading=eager. Got: {}",
        second_card_html
    );
}

#[test]
fn single_card_list_gets_eager_loading() {
    let cards = vec![ChildItemProps {
        title: "Only".to_string(),
        url: "only/".to_string(),
        child_count: Some(1),
        cover: Some("cover.jpg".to_string()),
        ..blank()
    }];

    let html = render_list_with_typesetting(
        &cards,
        None,
        Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Title,
        false,
        &Default::default(),
    );
    assert!(
        html.contains(r#"loading="eager""#),
        "Single card should have loading=eager. Got: {}",
        html
    );
}

#[test]
fn first_card_with_cover_gets_eager_even_if_not_first_overall() {
    let cards = vec![
        ChildItemProps {
            title: "NoCover".to_string(),
            url: "no-cover/".to_string(),
            child_count: Some(5),
            ..blank()
        },
        ChildItemProps {
            title: "WithCover".to_string(),
            url: "with-cover/".to_string(),
            child_count: Some(3),
            cover: Some("cover.jpg".to_string()),
            ..blank()
        },
    ];

    let html = render_list_with_typesetting(
        &cards,
        None,
        Language::En,
        None,
        None,
        moss_core::sort::SortAxis::Title,
        false,
        &Default::default(),
    );
    // The first card with a cover should get eager loading
    assert!(
        html.contains(r#"loading="eager""#),
        "First card with cover should get eager loading. Got: {}",
        html
    );
}

/// The band colour is resolved here, where it is consumed, from the cover the
/// props carry — rung 1 of the ladder, the `|color=` override, needs no root
/// and no cache. #0a2a3f is already past 4.5:1 against white, so its lightness
/// survives the contrast step (14%) rather than being darkened further.
#[test]
fn a_card_takes_its_band_color_from_the_covers_color_override() {
    let props = ChildItemProps {
        title: "Research".to_string(),
        url: "/research/".to_string(),
        child_count: Some(3),
        cover: Some("research.jpg|color=#0a2a3f".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    // The colour rides on the outer <a>, not on .moss-card-content.
    assert!(
        html.contains(r#"<a href="/research/" class="moss-card" data-cover-color style="--moss-cover-color: hsla(203, 73%, 14%, 1)">"#),
        "outer <a> must carry the band colour, got: {html}"
    );
    // A cover with no override, no root and no cache yields no colour at all.
    let plain = ChildItemProps { cover: Some("research.jpg".to_string()), ..props };
    let html = render_item_with_typesetting(&plain, None, Language::En, None, None, false);
    assert!(!html.contains("data-cover-color"), "got: {html}");
}

// ── Pipe-encoded cover with display attrs ───────────────────

#[test]
fn test_render_item_pipe_encoded_cover_strips_attrs_from_src() {
    let props = ChildItemProps {
        title: "Travel".to_string(),
        url: "travel/".to_string(),
        child_count: Some(3),
        cover: Some("assets/photo.jpg|left".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains(r#"src="assets/photo.jpg""#),
        "src should not contain pipe attrs. Got: {}",
        html
    );
    assert!(
        !html.contains(r#"src="assets/photo.jpg|left""#),
        "Pipe should be stripped from src. Got: {}",
        html
    );
}

#[test]
fn test_render_item_pipe_encoded_cover_adds_style() {
    let props = ChildItemProps {
        title: "Travel".to_string(),
        url: "travel/".to_string(),
        child_count: Some(3),
        cover: Some("assets/photo.jpg|contain top".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains(r#"style="object-fit:contain;object-position:top""#),
        "Got: {}",
        html
    );
}

#[test]
fn test_render_item_cover_without_pipe_no_style() {
    let props = ChildItemProps {
        title: "Travel".to_string(),
        url: "travel/".to_string(),
        child_count: Some(3),
        cover: Some("assets/photo.jpg".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        !html.contains("style="),
        "No pipe means no style attr. Got: {}",
        html
    );
}

#[test]
fn renders_kicker_above_title() {
    let props = ChildItemProps {
        title: "Essays".to_string(),
        url: "/essays/".to_string(),
        child_count: Some(12),
        kicker: Some("FOLDER".to_string()),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    let kicker_pos = html
        .find(r#"<span class="moss-card-kicker""#)
        .expect("kicker present");
    let title_pos = html
        .find(r#"class="moss-card-title"#)
        .expect("title present");
    assert!(kicker_pos < title_pos, "kicker must render before title");
    assert!(html.contains(">FOLDER</span>"), "kicker text rendered");
}

// ── CJK numeral formatting tests ─────────────────────────────

#[test]
fn formats_count_as_chinese_numeral_in_vertical_cjk() {
    let props = ChildItemProps {
        title: "文集".to_string(),
        url: "/essays/".to_string(),
        child_count: Some(12),
        ..blank()
    };
    let html =
        render_item_with_typesetting(&props, None, Language::ZhHant, Some("vertical"), None, false);
    assert!(
        html.contains("十二篇"),
        "count rendered as Chinese numeral; got: {}",
        html
    );
    assert!(
        !html.contains("12 篇"),
        "Arabic count should not appear in vertical CJK"
    );
}

#[test]
fn keeps_arabic_count_in_horizontal_western() {
    let props = ChildItemProps {
        title: "Essays".to_string(),
        url: "/essays/".to_string(),
        child_count: Some(12),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::En, None, None, false);
    assert!(
        html.contains("12 articles") || html.contains("12 article"),
        "Arabic count in Western horizontal: {}",
        html
    );
}

#[test]
fn keeps_arabic_count_in_horizontal_cjk() {
    let props = ChildItemProps {
        title: "文集".to_string(),
        url: "/essays/".to_string(),
        child_count: Some(12),
        ..blank()
    };
    let html = render_item_with_typesetting(&props, None, Language::ZhHant, None, None, false);
    assert!(
        html.contains("12 篇") || html.contains("12篇"),
        "Arabic count in horizontal CJK: {}",
        html
    );
    assert!(
        !html.contains("十二篇"),
        "CJK numeral should only appear in vertical CJK"
    );
}
