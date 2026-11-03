use super::*;
use moss_core::ast::extract_form::parse_form;

fn booking() -> FormShortcode {
    parse_form(
        "{#booking next=/booking/received/}",
        "- Your name *\n- Session * {type=radio}\n  - Portrait\n  - Family\n- Notes {type=textarea}\n  Help words.\n",
    )
    .0
}

#[test]
fn commerce_lists_fields_options_next_and_a_false_turnstile_and_no_labels() {
    let v: serde_json::Value = serde_json::from_str(&json(&[booking()])).unwrap();
    let b = &v["forms"]["booking"];
    assert_eq!(b["next"], "/booking/received/");
    assert_eq!(b["turnstile"], serde_json::Value::Bool(false));
    assert_eq!(b["fields"][0], serde_json::json!({"name": "your_name", "type": "text", "required": true}));
    assert_eq!(b["fields"][1]["options"], serde_json::json!(["Portrait", "Family"]));
    assert_eq!(b["fields"][1]["type"], "radio");
    assert!(!json(&[booking()]).contains("Help words") && !json(&[booking()]).contains("Your name"));
    assert_eq!(json(&[]), r#"{"forms":{}}"#);
}

#[test]
fn a_repeated_form_name_and_a_missing_endpoint_are_problems() {
    assert!(problems(&[booking()], Some("https://f.example")).is_empty());
    let dup = problems(&[booking(), booking()], Some("https://f.example"));
    assert_eq!(dup.len(), 1, "{dup:?}");
    assert!(dup[0].contains("more than one") && dup[0].contains("booking"));
    let none = problems(&[booking()], None);
    assert!(none.len() == 1 && none[0].contains("[services.forms] endpoint"), "{none:?}");
    assert!(problems(&[], None).is_empty());
}
