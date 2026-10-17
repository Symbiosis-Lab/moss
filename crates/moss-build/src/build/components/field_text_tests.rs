use super::*;
use crate::i18n::Language::{De, En, ZhHans};
use moss_core::date::{parse_edtf, Edtf};

fn date(s: &str) -> EdtfDate {
    match parse_edtf(s) {
        Some(Edtf::Date(d)) => d,
        other => panic!("{s} is not a single date: {other:?}"),
    }
}

#[test]
fn date_text_reads_each_precision_and_qualifier() {
    let cases = [
        ("1930", En, "1930"),
        ("1930-10", En, "October 1930"),
        ("1930-10", De, "Oktober 1930"),
        ("1930-10", ZhHans, "1930年10月"),
        ("1930-10-12", En, "October 12, 1930"),
        ("1930-10-12", De, "12. Oktober 1930"),
    ];
    for (input, lang, expected) in cases {
        assert_eq!(date_text(&date(input), lang, None), expected, "{input} in {lang:?}");
    }
    assert_eq!(date_text(&date("1930-10-12"), ZhHans, Some("vertical")), "一九三〇年·十月十二日");
    assert_eq!(date_text(&date("~1930"), En, None), "c. 1930", "approximate");
    assert_eq!(date_text(&date("~1930"), De, None), "um 1930", "approximate");
    assert_eq!(date_text(&date("~1930"), ZhHans, None), "约1930", "approximate");
    assert_eq!(date_text(&date("1930?"), En, None), "1930?", "uncertain");
    assert_eq!(date_text(&date("1930-10-12%"), En, None), "c. October 12, 1930?", "both");
}

#[test]
fn period_text_collapses_ranges_and_reads_open_ends() {
    let cases: [(&str, Option<&str>, Language, &str); 10] = [
        ("1927", Some("1933"), En, "1927–1933"),
        ("1930-10-12", Some("1930-10-14"), En, "October 12–14, 1930"),
        ("1930-10-12", Some("1930-10-14"), De, "12.–14. Oktober 1930"),
        ("1930-10-12", Some("1930-10-14"), ZhHans, "1930年10月12日–1930年10月14日"),
        ("1929-12-31", Some("1930-01-02"), En, "December 31, 1929 – January 2, 1930"),
        ("1930", Some("1931-10-12"), En, "1930 – October 12, 1931"),
        ("1923", None, En, "from 1923"),
        ("1923", None, De, "ab 1923"),
        ("1923", None, ZhHans, "1923起"),
        ("~1923", None, En, "from c. 1923"),
    ];
    for (start, end, lang, expected) in cases {
        let end = end.map(date);
        let open = end.is_none();
        assert_eq!(period_text(&date(start), end.as_ref(), open, lang, None), expected, "{start} to {end:?} in {lang:?}");
    }
}

#[test]
fn count_text_groups_digits_and_keeps_decimals() {
    assert_eq!(count_text(1382.0, None, En), ("1,382".to_string(), None));
    assert_eq!(count_text(1382.0, Some("flats"), De), ("1.382".to_string(), Some("flats".to_string())));
    assert_eq!(count_text(48.1, None, En), ("48.1".to_string(), None));
    assert_eq!(count_text(48.1, None, De), ("48,1".to_string(), None));
    assert_eq!(count_text(-1234567.0, None, ZhHans), ("-1,234,567".to_string(), None));
}

#[test]
fn value_text_dispatches_each_kind() {
    let choice = TypedValue::Choice { value: "yes".to_string(), label: "Yes".to_string() };
    assert_eq!(value_text(&choice, En, None), "Yes");

    let one_point = TypedValue::Period { start: date("1923"), end: None, open: false, text: "1923".to_string() };
    assert_eq!(value_text(&one_point, En, None), "1923", "a one-point period reads as the date");

    let open = TypedValue::Period { start: date("1923"), end: None, open: true, text: "1923/".to_string() };
    assert_eq!(value_text(&open, En, None), "from 1923", "an open interval reads as from");
}
