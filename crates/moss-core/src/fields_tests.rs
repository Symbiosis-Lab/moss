use super::*;
use crate::date::{parse_edtf, Edtf};
use serde_json::json;

fn decl(kind: FieldKind) -> FieldDecl {
    FieldDecl { name: "f".into(), kind, choices: vec![], place: None, scope: vec![] }
}

fn choice(values: &[&str]) -> FieldDecl {
    let choices = values.iter().map(|v| Choice { value: v.to_string(), label: v.to_string() }).collect();
    FieldDecl { choices, ..decl(FieldKind::Choice) }
}

fn ok(kind: FieldKind, value: Value) -> Value {
    let typed = parse_value(&decl(kind), &value).expect("expected Ok");
    serde_json::to_value(typed).expect("typed values serialize")
}

fn err(kind: FieldKind, value: Value) -> String {
    parse_value(&decl(kind), &value).expect_err("expected Err")
}

/// The JSON a date at this precision and qualifier serializes to.
fn date(value: &str, precision: &str, qualifier: Option<&str>) -> Value {
    json!({"value": value, "precision": precision, "qualifier": qualifier})
}

#[test]
fn text_takes_scalars_and_refuses_containers() {
    assert_eq!(ok(FieldKind::Text, json!("Riverside")), json!("Riverside"));
    assert_eq!(ok(FieldKind::Text, json!(1930)), json!("1930"));
    assert_eq!(ok(FieldKind::Text, json!(true)), json!("true"));
    assert_eq!(err(FieldKind::Text, json!(["a"])), "expected text");
}

#[test]
fn null_is_refused_for_every_kind() {
    assert!(err(FieldKind::Text, Value::Null).contains("has no value"));
    assert!(err(FieldKind::Period, Value::Null).contains("has no value"));
}

#[test]
fn number_accepts_numbers_and_numeric_strings() {
    assert_eq!(ok(FieldKind::Number, json!(48.1)), json!(48.1));
    assert_eq!(ok(FieldKind::Number, json!(" 48.1 ")), json!(48.1));
    assert!(err(FieldKind::Number, json!("forty")).contains("not a number"));
    assert!(err(FieldKind::Number, json!("NaN")).contains("not a number"));
}

#[test]
fn date_takes_one_date_at_each_precision() {
    assert_eq!(ok(FieldKind::Date, json!("1930-10-12")), date("1930-10-12", "day", None));
    assert_eq!(ok(FieldKind::Date, json!("1930-10")), date("1930-10", "month", None));
    assert_eq!(ok(FieldKind::Date, json!("1930")), date("1930", "year", None));
    assert_eq!(ok(FieldKind::Date, json!(1930)), date("1930", "year", None), "a bare YAML year");
}

#[test]
fn date_rejects_intervals_and_dates_the_calendar_does_not_have() {
    assert!(err(FieldKind::Date, json!("1927/1933")).contains("is an interval"));
    for bad in ["1930-13", "1930-00-10", "1930-10-32", "2021-02-30", "2020-13", "30-10-12", "1927-1933"] {
        assert!(err(FieldKind::Date, json!(bad)).contains("not a date"), "{bad} must be rejected");
    }
    assert!(err(FieldKind::Date, json!(true)).contains("expected a date"));
}

#[test]
fn edtf_qualifiers_mark_uncertain_approximate_and_both() {
    assert_eq!(ok(FieldKind::Date, json!("1923?")), date("1923", "year", Some("uncertain")));
    assert_eq!(ok(FieldKind::Date, json!("~1930")), date("1930", "year", Some("approximate")));
    assert_eq!(ok(FieldKind::Date, json!("1930~")), date("1930", "year", Some("approximate")));
    assert_eq!(ok(FieldKind::Date, json!("1930%")), date("1930", "year", Some("both")));
    assert!(parse_edtf("1930?~").is_none(), "one qualifier only");
    assert!(parse_edtf("~1930?").is_none(), "one qualifier only");
}

#[test]
fn edtf_day_check_knows_leap_years() {
    assert_eq!(ok(FieldKind::Date, json!("2000-02-29")), date("2000-02-29", "day", None));
    assert_eq!(ok(FieldKind::Date, json!("2024-02-29")), date("2024-02-29", "day", None));
    assert!(parse_edtf("1900-02-29").is_none(), "1900 is a century year, not a leap year");
    assert!(parse_edtf("2021-02-29").is_none());
}

#[test]
fn period_reads_intervals_and_one_date() {
    let period = |start: Value, end: Value, text: &str| json!({"start": start, "end": end, "text": text});
    let y = |v: &str| date(v, "year", None);
    let cases: [(&str, Value); 6] = [
        ("1927/1933", period(y("1927"), y("1933"), "1927/1933")),
        ("1927–1933", period(y("1927"), y("1933"), "1927–1933")),
        ("1927—1933", period(y("1927"), y("1933"), "1927—1933")),
        ("1927/..", period(y("1927"), Value::Null, "1927/..")),
        ("1927–", period(y("1927"), Value::Null, "1927–")),
        ("1927", period(y("1927"), Value::Null, "1927")),
    ];
    for (text, expected) in cases {
        assert_eq!(ok(FieldKind::Period, json!(text)), expected, "{text}");
    }
    assert_eq!(ok(FieldKind::Period, json!(1927)), period(y("1927"), Value::Null, "1927"), "a bare YAML year");
    assert_eq!(
        ok(FieldKind::Period, json!("1927-06–1933-09")),
        period(date("1927-06", "month", None), date("1933-09", "month", None), "1927-06–1933-09")
    );
}

#[test]
fn period_rejects_the_dash_form_and_other_shapes() {
    for bad in ["1927-1933", "1927/1933/1940", "–1933", "the twenties", "1927/1933-02-30"] {
        let message = err(FieldKind::Period, json!(bad));
        assert!(message.contains("is not a period"), "{bad}: {message}");
        assert!(message.contains("1927/1933"), "the message names the accepted forms: {message}");
    }
}

#[test]
fn choice_returns_value_and_label_and_names_the_allowed_values() {
    let d = FieldDecl {
        choices: vec![Choice { value: "Building".into(), label: "Built".into() }],
        ..decl(FieldKind::Choice)
    };
    let typed = parse_value(&d, &json!("Building")).expect("ok");
    assert_eq!(typed, TypedValue::Choice { value: "Building".into(), label: "Built".into() });
    let d = choice(&["Building", "Service", "Programme"]);
    assert_eq!(
        parse_value(&d, &json!("Buidling")).expect_err("err"),
        "\"Buidling\" is not one of: Building, Service, Programme"
    );
    assert!(parse_value(&d, &json!("building")).is_err(), "match is case-sensitive");
}

#[test]
fn choice_given_a_yaml_number_names_the_allowed_values() {
    let d = choice(&["Building", "Service"]);
    assert_eq!(parse_value(&d, &json!(1)).expect_err("err"), "1 is not one of: Building, Service");
}

#[test]
fn page_needs_a_wikilink_and_takes_an_alias() {
    assert_eq!(ok(FieldKind::Page, json!("[[A]]")), json!({"target": "A"}));
    assert_eq!(ok(FieldKind::Page, json!("[[B|alias]]")), json!({"target": "B"}));
    assert!(err(FieldKind::Page, json!("Anna Weber")).contains("[[name]]"));
    assert!(err(FieldKind::Page, json!([["A"]])).contains("in quotes"));
}

#[test]
fn pages_takes_a_list_or_one_link_and_names_the_bad_item() {
    assert_eq!(
        ok(FieldKind::Pages, json!(["[[A]]", "[[B|alias]]"])),
        json!([{"target": "A"}, {"target": "B"}])
    );
    assert_eq!(ok(FieldKind::Pages, json!("[[A]]")), json!([{"target": "A"}]));
    let message = err(FieldKind::Pages, json!(["[[A]]", "Bob"]));
    assert!(message.contains("\"Bob\" is not a page link"), "got: {message}");
}

#[test]
fn count_splits_a_number_from_its_noun() {
    assert_eq!(
        ok(FieldKind::Count, json!("1,382 flats")),
        json!({"number": 1382, "noun": "flats", "text": "1,382 flats"})
    );
    assert_eq!(ok(FieldKind::Count, json!("48.1 km")), json!({"number": 48.1, "noun": "km", "text": "48.1 km"}));
    assert_eq!(ok(FieldKind::Count, json!(12)), json!({"number": 12, "text": "12"}));
    assert_eq!(ok(FieldKind::Count, json!(48.5)), json!({"number": 48.5, "text": "48.5"}));
}

#[test]
fn count_that_is_not_a_number_with_a_noun_stays_text() {
    assert_eq!(ok(FieldKind::Count, json!("about 65 million")), json!({"text": "about 65 million"}));
    assert_eq!(ok(FieldKind::Count, json!("1927")), json!({"text": "1927"}));
    assert!(err(FieldKind::Count, json!({"a": 1})).contains("expected"));
}

#[test]
fn count_takes_integers_beyond_i64_as_floats() {
    let huge = ok(FieldKind::Count, json!("18446744073709551615 flats"));
    assert_eq!(huge["noun"], "flats");
    assert!(huge["number"].as_f64().is_some_and(|n| n > 1.8e19), "{huge}");
}

#[test]
fn applies_to_matches_folder_prefixes_at_a_boundary() {
    let housing = FieldDecl { scope: vec!["Housing".into()], ..decl(FieldKind::Pages) };
    let rooted = FieldDecl { scope: vec!["/Housing/".into()], ..decl(FieldKind::Pages) };
    let whole = decl(FieldKind::Text);
    let cases: [(&FieldDecl, &str, bool); 8] = [
        (&whole, "anything/at/all.md", true),
        (&housing, "Housing/riverside.md", true),
        (&housing, "Housing/deep/nested.md", true),
        (&housing, "Housingly/other.md", false),
        (&housing, "Services/x.md", false),
        (&housing, "../Housing/a.md", false),
        (&rooted, "./Housing/a.md", true),
        (&rooted, "Housing", true),
    ];
    for (decl, path, expected) in cases {
        assert_eq!(applies_to(decl, path), expected, "{path} with scope {:?}", decl.scope);
    }
}

#[test]
fn parse_place_takes_each_name_and_refuses_others() {
    let cases = [
        ("eyebrow", Some(Place::Eyebrow)),
        ("subtitle", Some(Place::Subtitle)),
        ("facts", Some(Place::Facts)),
        ("key-figure", Some(Place::KeyFigure)),
        ("none", None),
        ("sidebar", None),
    ];
    for (name, expected) in cases {
        assert_eq!(parse_place(name), expected, "{name}");
    }
}

#[test]
fn every_listed_type_name_parses_and_others_do_not() {
    for name in FieldKind::names() {
        assert!(FieldKind::from_name(name).is_some(), "{name}");
    }
    assert_eq!(FieldKind::from_name("Text"), None);
    assert!(matches!(parse_edtf("1930"), Some(Edtf::Date(_))));
}
