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
fn test_format_article_date_standard_en() {
    // A full date renders in English's own conventional written form, not
    // raw numbers joined by " · " — that numeric form is
    // `format_date_string`'s own documented card/listing display, not this
    // function's.
    assert_eq!(format_article_date("2024-09-22", Language::En), "September 22, 2024");
    assert_eq!(format_article_date("2025-11-17", Language::En), "November 17, 2025");
}

#[test]
fn test_format_article_date_standard_zh_hant() {
    assert_eq!(format_article_date("2024-09-22", Language::ZhHant), "2024年9月22日");
    assert_eq!(format_article_date("1919-04-10", Language::ZhHant), "1919年4月10日");
}

#[test]
fn test_format_article_date_iso_format() {
    assert_eq!(
        format_article_date("2025-09-24T20:49:08.546Z", Language::En),
        "September 24, 2025"
    );
}

#[test]
fn test_format_article_date_removes_leading_zeros() {
    assert_eq!(format_article_date("2024-01-05", Language::En), "January 5, 2024");
    assert_eq!(format_article_date("2024-09-01", Language::En), "September 1, 2024");
}

#[test]
fn test_format_article_date_invalid_returns_original() {
    assert_eq!(format_article_date("invalid", Language::En), "invalid");
}

#[test]
fn test_format_article_date_partial_precision() {
    // Year+month: month name and year, no day. Year-only: unchanged in
    // every language — see the function's own doc comment for why.
    assert_eq!(format_article_date("2025-11", Language::En), "November 2025");
    assert_eq!(format_article_date("2025-11", Language::ZhHant), "2025年11月");
    assert_eq!(format_article_date("1797", Language::En), "1797");
    assert_eq!(format_article_date("1797", Language::ZhHant), "1797");
}

// Tests for format_date_string
#[test]
fn test_format_date_string_standard() {
    assert_eq!(format_date_string("2025-09-02"), "2025 · 09");
    assert_eq!(format_date_string("2025-01-15"), "2025 · 01");
}

#[test]
fn test_format_date_string_invalid() {
    assert_eq!(format_date_string("invalid"), "invalid");
    assert_eq!(format_date_string(""), "");
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

#[test]
fn compact_date_names_the_month_at_day_precision_as_at_month_precision() {
    assert_eq!(format_compact_date("2026-04", Language::En, None), "April 2026");
    assert_eq!(format_compact_date("2026-03-04", Language::En, None), "March 2026");
    assert_eq!(format_compact_date("2026-03-04", Language::ZhHant, None), "2026年3月");
    assert_eq!(format_compact_date("2026", Language::En, None), "2026");
}

#[test]
fn time_html_wraps_the_text_with_the_full_date_and_escapes_it() {
    assert_eq!(time_html("March 2026", "2026-03-04"), r#"<time datetime="2026-03-04">March 2026</time>"#);
    assert_eq!(time_html("<b>x</b>", "2026"), r#"<time datetime="2026">&lt;b&gt;x&lt;/b&gt;</time>"#);
    assert_eq!(time_html("<b>", "not a date"), "&lt;b&gt;");
}

#[test]
fn a_clock_time_is_read_only_when_written_without_a_zone() {
    assert_eq!(clock_time("2026-11-06T14:32"), Some("14:32"));
    assert_eq!(clock_time("2026-11-06 14:32:05"), Some("14:32"));
    assert_eq!(clock_time("2026-11-06"), None);
    assert_eq!(clock_time("2025-11-17T00:50:43.135Z"), None);
    assert_eq!(clock_time("2026-11-06T14:32+08:00"), None);
    assert_eq!(with_clock("2026年11月6日", "14:32", Language::ZhHans), "2026年11月6日 14:32");
    assert_eq!(with_clock("November 6, 2026", "14:32", Language::En), "November 6, 2026, 14:32");
}

