use super::*;

#[test]
fn parse_partial_date_reads_every_precision_and_notation() {
    let full = PartialDate { year: 2025, month: Some(9), day: Some(2) };
    assert_eq!(parse_partial_date("2025-09-02"), Some(full));
    assert_eq!(parse_partial_date("2025/09/02"), Some(full), "slash separator");

    assert_eq!(
        parse_partial_date("2025-09-24T20:49:08.546Z"),
        Some(PartialDate { year: 2025, month: Some(9), day: Some(24) }),
        "ISO datetime truncates at T"
    );
    assert_eq!(
        parse_partial_date("1797-06"),
        Some(PartialDate { year: 1797, month: Some(6), day: None }),
        "month precision"
    );
    assert_eq!(
        parse_partial_date("1797"),
        Some(PartialDate { year: 1797, month: None, day: None }),
        "year precision"
    );

    assert_eq!(parse_partial_date("invalid"), None);
    assert_eq!(parse_partial_date(""), None);
    assert_eq!(parse_partial_date("2025-13-01"), None, "month out of range");
}

#[test]
fn test_extract_year_standard_date() {
    assert_eq!(extract_year("2025-09-02"), Some(2025));
}

#[test]
fn test_extract_year_just_year() {
    assert_eq!(extract_year("2025"), Some(2025));
}

#[test]
fn test_extract_year_invalid() {
    assert_eq!(extract_year("invalid"), None);
    assert_eq!(extract_year(""), None);
    assert_eq!(extract_year("abc"), None);
}

#[test]
fn month_prefix_reads_every_date_shape_a_listing_hands_it() {
    // ISO, slashed, single-digit, and the "YYYY · MM" display form the caller
    // falls back to when a row has no raw date. Five one-shape tests before.
    assert_eq!(format_month_prefix("2025-11-17", Language::En, None), "11");
    assert_eq!(format_month_prefix("2025-1-15", Language::En, None), "01");
    assert_eq!(format_month_prefix("2025/11/17", Language::En, None), "11");
    assert_eq!(format_month_prefix("2026 · 03", Language::En, None), "03");
    // No month, so no prefix: the year heading above the row already says it.
    assert_eq!(format_month_prefix("1700", Language::En, None), "");
    assert_eq!(format_month_prefix("invalid", Language::En, None), "");
}

#[test]
fn month_prefix_and_year_heading_are_chinese_under_vertical_cjk() {
    // An Arabic digit in a vertical run lies on its side; 十二月 and 一七〇三
    // stand up. Horizontal CJK and vertical non-CJK both keep the digits.
    let vertical = Some("vertical");
    assert_eq!(format_month_prefix("1703-12-01", Language::ZhHant, vertical), "十二月");
    assert_eq!(format_month_prefix("1703-03-01", Language::ZhHant, vertical), "三月");
    assert_eq!(format_month_prefix("1703-12-01", Language::ZhHant, None), "12");
    assert_eq!(format_month_prefix("1703-12-01", Language::En, vertical), "12");

    assert_eq!(format_year_heading(1703, Language::ZhHant, vertical), "一七〇三");
    assert_eq!(format_year_heading(1703, Language::ZhHans, vertical), "一七〇三");
    assert_eq!(format_year_heading(1703, Language::ZhHant, None), "1703");
    assert_eq!(format_year_heading(1703, Language::En, vertical), "1703");
}

#[test]
fn test_format_article_date_standard() {
    assert_eq!(format_article_date("2024-09-22"), "2024 · 9 · 22");
    assert_eq!(format_article_date("2025-11-17"), "2025 · 11 · 17");
}

#[test]
fn test_format_article_date_iso_format() {
    assert_eq!(
        format_article_date("2025-09-24T20:49:08.546Z"),
        "2025 · 9 · 24"
    );
}

#[test]
fn test_format_article_date_removes_leading_zeros() {
    assert_eq!(format_article_date("2024-01-05"), "2024 · 1 · 5");
    assert_eq!(format_article_date("2024-09-01"), "2024 · 9 · 1");
}

#[test]
fn test_format_article_date_invalid_returns_original() {
    assert_eq!(format_article_date("invalid"), "invalid");
}

#[test]
fn test_format_article_date_partial_precision() {
    assert_eq!(format_article_date("2025-11"), "2025 · 11");
    assert_eq!(format_article_date("1797"), "1797");
}

// Tests for format_date_string
#[test]
fn test_format_date_string_standard() {
    assert_eq!(format_date_string("2025-09-02"), "2025 · 09");
    assert_eq!(format_date_string("2025-01-15"), "2025 · 01");
}

#[test]
fn test_format_date_string_with_slashes() {
    assert_eq!(format_date_string("2025/09/02"), "2025 · 09");
}

#[test]
fn test_format_date_string_single_digit_month() {
    assert_eq!(format_date_string("2025-9-02"), "2025 · 09");
}

#[test]
fn test_format_date_string_invalid() {
    assert_eq!(format_date_string("invalid"), "invalid");
    assert_eq!(format_date_string(""), "");
}

// Tests for extract_raw_date_from_filename
#[test]
fn extract_raw_date_from_filename_rejects_out_of_range_month_and_day() {
    // The hand-rolled version only checked year/month length and digit-ness,
    // never the numeric range — "99" as a month or "40" as a day slipped
    // through. Routed through parse_partial_date, both are rejected.
    assert_eq!(
        extract_raw_date_from_filename("posts/2021-99-09-spring.html"),
        None
    );
    assert_eq!(
        extract_raw_date_from_filename("posts/2021-04-40-spring.html"),
        None
    );
}

#[test]
fn extract_raw_date_from_filename_keeps_full_stem_for_a_slugged_filename() {
    // The function's contract is the whole pre-".html" stem, not just the
    // date — a leaf page's filename carries its slug after the date, and
    // downstream (format_date_string -> parse_partial_date) already ignores
    // the trailing segment. Routing through the shared parser must not
    // truncate this.
    assert_eq!(
        extract_raw_date_from_filename("posts/2021-04-09-spring.html"),
        Some("2021-04-09-spring".to_string())
    );
}

// Tests for extract_date_from_path
#[test]
fn test_extract_date_from_path_with_date_filename() {
    assert_eq!(
        extract_date_from_path("posts/2025-01-15.html", None),
        "2025 · 01"
    );
}

#[test]
fn test_extract_date_from_path_any_folder() {
    assert_eq!(
        extract_date_from_path("blog/2025-01-15.html", None),
        "2025 · 01"
    );
}

#[test]
fn test_extract_date_from_path_no_date_pattern() {
    assert_eq!(
        extract_date_from_path("posts/hello-world.html", None),
        "Unknown"
    );
}

#[test]
fn test_extract_date_from_doc_uses_source_path_for_creation_date() {
    // Create a temp file so we have a real file with a creation date
    let dir = std::env::temp_dir().join("moss_date_test");
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("友链.md");
    std::fs::write(&file_path, "# Friends").unwrap();

    let root_path = dir.to_str().unwrap().to_string();

    let doc = ParsedDocument {
        title: "友链".to_string(),
        label: "友链".to_string(),
        url_path: "you-lian/index.html".to_string(),
        // slugified — won't match file
        date: None,
        // no frontmatter date
        reading_time: 1,
        slug: "you-lian".to_string(),
        permalink: "/you-lian/".to_string(),
        is_root_level: true,
        clean_stem: "友链".to_string(),
        kind: moss_core::PageKind::Article,
        source_path: Some("友链.md".to_string()),
        ..Default::default()
    };

    let (display, _raw, _explicit) = extract_date_from_doc(&doc, &root_path);
    assert_ne!(
        display, "Unknown",
        "Should use source_path to get file creation date"
    );
    // Should be in "YYYY · MM" format
    assert!(
        display.contains(" · "),
        "Date should be in 'YYYY · MM' format, got: {}",
        display
    );

    // Cleanup
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_extract_date_from_doc_frontmatter_takes_priority() {
    let root_path = "/tmp".to_string();

    let doc = ParsedDocument {
        title: "Test".to_string(),
        label: "Test".to_string(),
        url_path: "test/index.html".to_string(),
        date: Some("2025-03-11".to_string()),
        reading_time: 1,
        slug: "test".to_string(),
        permalink: "/test/".to_string(),
        clean_stem: "test".to_string(),
        kind: moss_core::PageKind::Article,
        source_path: Some("test.md".to_string()),
        ..Default::default()
    };

    let (display, raw, _explicit) = extract_date_from_doc(&doc, &root_path);
    assert_eq!(
        display, "2025 · 03",
        "Frontmatter date should take priority"
    );
    assert_eq!(
        raw,
        Some("2025-03-11".to_string()),
        "date_raw should contain the original frontmatter date"
    );
}

#[test]
fn test_extract_date_from_doc_file_creation_returns_raw() {
    // When falling back to file creation date, date_raw should still be Some
    let dir = std::env::temp_dir().join("moss_date_raw_test");
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("友链.md");
    std::fs::write(&file_path, "# Friends").unwrap();

    let root_path = dir.to_str().unwrap().to_string();

    let doc = ParsedDocument {
        title: "友链".to_string(),
        label: "友链".to_string(),
        url_path: "you-lian/index.html".to_string(),
        reading_time: 1,
        slug: "you-lian".to_string(),
        permalink: "/you-lian/".to_string(),
        is_root_level: true,
        clean_stem: "友链".to_string(),
        kind: moss_core::PageKind::Article,
        source_path: Some("友链.md".to_string()),
        ..Default::default()
    };

    let (display, raw, _explicit) = extract_date_from_doc(&doc, &root_path);
    assert!(
        display.contains(" · "),
        "Display should be 'YYYY · MM' format, got: {}",
        display
    );
    assert!(
        raw.is_some(),
        "date_raw must be Some even for file-creation-date fallback"
    );
    // raw should be parseable as a date (contains hyphen-separated parts)
    let raw_str = raw.unwrap();
    assert!(
        raw_str.contains('-'),
        "date_raw should be in ISO-ish format, got: {}",
        raw_str
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_extract_date_from_doc_frontmatter_is_explicit() {
    let root_path = "/tmp".to_string();
    let doc = ParsedDocument {
        title: "Test".to_string(),
        label: "Test".to_string(),
        url_path: "test/index.html".to_string(),
        date: Some("2025-03-11".to_string()),
        reading_time: 1,
        slug: "test".to_string(),
        permalink: "/test/".to_string(),
        clean_stem: "test".to_string(),
        kind: moss_core::PageKind::Article,
        source_path: Some("test.md".to_string()),
        ..Default::default()
    };
    let (_display, _raw, is_explicit) = extract_date_from_doc(&doc, &root_path);
    assert!(is_explicit, "Frontmatter date should be explicit");
}

#[test]
fn test_extract_date_from_doc_filesystem_not_explicit() {
    let dir = std::env::temp_dir().join("moss_explicit_test");
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("no-date.md");
    std::fs::write(&file_path, "# No Date").unwrap();
    let root_path = dir.to_str().unwrap().to_string();
    let doc = ParsedDocument {
        title: "No Date".to_string(),
        label: "No Date".to_string(),
        url_path: "no-date/index.html".to_string(),
        reading_time: 1,
        slug: "no-date".to_string(),
        permalink: "/no-date/".to_string(),
        clean_stem: "no-date".to_string(),
        kind: moss_core::PageKind::Article,
        source_path: Some("no-date.md".to_string()),
        ..Default::default()
    };
    let (_display, _raw, is_explicit) = extract_date_from_doc(&doc, &root_path);
    assert!(
        !is_explicit,
        "Filesystem fallback date should NOT be explicit"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// Tests for vertical CJK date formatting
#[test]
fn test_format_vertical_cjk_date_full() {
    assert_eq!(
        format_vertical_cjk_date("2025-09-24"),
        Some("二〇二五年·九月二十四日".to_string())
    );
    assert_eq!(
        format_vertical_cjk_date("1694-01-01"),
        Some("一六九四年·一月一日".to_string())
    );
}

#[test]
fn test_format_vertical_cjk_date_iso_format() {
    assert_eq!(
        format_vertical_cjk_date("2025-09-24T20:49:08.546Z"),
        Some("二〇二五年·九月二十四日".to_string())
    );
}

#[test]
fn test_format_vertical_cjk_date_double_digit_month_day() {
    assert_eq!(
        format_vertical_cjk_date("2024-12-31"),
        Some("二〇二四年·十二月三十一日".to_string())
    );
}

#[test]
fn test_format_vertical_cjk_date_invalid() {
    assert_eq!(format_vertical_cjk_date("invalid"), None);
}

#[test]
fn test_format_vertical_cjk_date_year_only() {
    // A year-only partial date ("date: 1699" with no month/day) is now
    // valid input, not "invalid" — see format_vertical_cjk_date_compact's
    // sibling test for the card-meta path this feeds.
    assert_eq!(format_vertical_cjk_date("2025"), Some("二〇二五年".to_string()));
}

#[test]
fn test_format_vertical_cjk_date_compact() {
    assert_eq!(
        format_vertical_cjk_date_compact("2025-09"),
        Some("二〇二五年·九月".to_string())
    );
    assert_eq!(
        format_vertical_cjk_date_compact("2025-09-24"),
        Some("二〇二五年·九月".to_string())
    );
}

#[test]
fn test_format_vertical_cjk_date_compact_invalid() {
    assert_eq!(format_vertical_cjk_date_compact("invalid"), None);
}

#[test]
fn test_format_vertical_cjk_date_compact_year_only() {
    assert_eq!(format_vertical_cjk_date_compact("2025"), Some("二〇二五年".to_string()));
}
