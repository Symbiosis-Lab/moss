use crate::vault::places::Precision;

pub(super) fn precision_rank(precision: &Precision) -> u8 {
    match precision {
        Precision::Exact => 0,
        Precision::City => 1,
        Precision::Region => 2,
        Precision::Country => 3,
    }
}

pub(super) fn precision_name(precision: Precision) -> &'static str {
    match precision {
        Precision::Exact => "exact",
        Precision::City => "city",
        Precision::Region => "region",
        Precision::Country => "country",
    }
}

pub(super) fn xml_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            '\t' | '\n' | '\r' => escaped.push(character),
            character if character >= '\u{20}' && character != '\u{7f}' => escaped.push(character),
            _ => escaped.push('\u{fffd}'),
        }
    }
    escaped
}
