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
    let v: serde_json::Value = serde_json::from_str(&json(&[booking()], None, false)).unwrap();
    let b = &v["forms"]["booking"];
    assert_eq!(b["next"], "/booking/received/");
    assert_eq!(b["turnstile"], serde_json::Value::Bool(false));
    assert_eq!(b["fields"][0], serde_json::json!({"name": "your_name", "type": "text", "required": true}));
    assert_eq!(b["fields"][1]["options"], serde_json::json!(["Portrait", "Family"]));
    assert_eq!(b["fields"][1]["type"], "radio");
    assert!(!json(&[booking()], None, false).contains("Help words") && !json(&[booking()], None, false).contains("Your name"));
    assert_eq!(json(&[], None, false), r#"{"forms":{}}"#);
}

#[test]
fn a_site_on_the_moss_host_asks_for_the_subscribe_bot_check() {
    let v: serde_json::Value = serde_json::from_str(&json(&[], None, true)).unwrap();
    assert_eq!(v["subscribe"], serde_json::json!({"turnstile": true}));
    assert!(!json(&[booking()], None, false).contains("subscribe"));
}

#[test]
fn a_repeated_form_name_and_a_missing_endpoint_are_problems() {
    assert!(problems(&[booking()], Some("https://f.example"), None).is_empty());
    let dup = problems(&[booking(), booking()], Some("https://f.example"), None);
    assert_eq!(dup.len(), 1, "{dup:?}");
    assert!(dup[0].contains("more than one") && dup[0].contains("booking"));
    let none = problems(&[booking()], None, None);
    assert!(none.len() == 1 && none[0].contains("[services.forms] endpoint"), "{none:?}");
    assert!(problems(&[], None, None).is_empty());
}

#[test]
fn a_booking_offer_sits_beside_the_forms() {
    let offer: BookingService = toml::from_str(
        "timezone = \"America/Los_Angeles\"\ncurrency = \"usd\"\nstart_every = 60\nnotice_hours = 12\nhorizon_days = 60\n\
         open = { mon = [\"09:00-17:00\"] }\ncancel = { full_before_hours = 24, late_percent = 50 }\n\
         pages = { book = \"/book/\", done = \"/book/done/\", manage = \"/book/manage/\" }\n\
         [services.intro]\nname = \"Intro call\"\nminutes = 30\nprice = 0\n",
    )
    .unwrap();
    assert!(problems(&[], None, Some(&Ok(offer.clone()))).is_empty(), "a valid offer is no problem");
    assert!(problems(&[], None, Some(&Err("[services.booking]: unknown field `pagez`".into())))[0].contains("pagez"));
    let v: serde_json::Value = serde_json::from_str(&json(&[booking()], Some(&offer), false)).unwrap();
    assert_eq!(v["booking"]["services"]["intro"]["minutes"], 30);
    assert_eq!(v["booking"]["open"]["mon"][0], "09:00-17:00");
    assert!(v["forms"]["booking"].is_object(), "the forms stay as they were");
    let none: serde_json::Value = serde_json::from_str(&json(&[], None, false)).unwrap();
    assert!(none.get("booking").is_none(), "no offer, no booking key");
}
