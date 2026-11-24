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
    let v: serde_json::Value = serde_json::from_str(&json(&[booking()], serde_json::json!({}), None, None, false)).unwrap();
    let b = &v["forms"]["booking"];
    assert_eq!(b["next"], "/booking/received/");
    assert_eq!(b["turnstile"], serde_json::Value::Bool(false));
    assert_eq!(b["fields"][0], serde_json::json!({"name": "your_name", "type": "text", "required": true}));
    assert_eq!(b["fields"][1]["options"], serde_json::json!(["Portrait", "Family"]));
    assert_eq!(b["fields"][1]["type"], "radio");
    assert!(!json(&[booking()], serde_json::json!({}), None, None, false).contains("Help words") && !json(&[booking()], serde_json::json!({}), None, None, false).contains("Your name"));
    assert_eq!(json(&[], serde_json::json!({}), None, None, false), r#"{"forms":{}}"#);
}

#[test]
fn a_site_on_the_moss_host_asks_for_the_subscribe_bot_check() {
    let v: serde_json::Value = serde_json::from_str(&json(&[], serde_json::json!({}), None, None, true)).unwrap();
    assert_eq!(v["subscribe"], serde_json::json!({"turnstile": true}));
    assert!(!json(&[booking()], serde_json::json!({}), None, None, false).contains("subscribe"));
}

#[test]
fn a_repeated_form_name_and_a_missing_endpoint_are_problems() {
    assert!(problems(&[booking()], Some("https://f.example"), None, None).is_empty());
    let dup = problems(&[booking(), booking()], Some("https://f.example"), None, None);
    assert_eq!(dup.len(), 1, "{dup:?}");
    assert!(dup[0].contains("more than one") && dup[0].contains("booking"));
    let none = problems(&[booking()], None, None, None);
    assert!(none.len() == 1 && none[0].contains("[services.forms] endpoint"), "{none:?}");
    assert!(problems(&[], None, None, None).is_empty());
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
    assert!(problems(&[], None, Some(&Ok(offer.clone())), None).is_empty(), "a valid offer is no problem");
    assert!(problems(&[], None, Some(&Err("[services.booking]: unknown field `pagez`".into())), None)[0].contains("pagez"));
    let v: serde_json::Value = serde_json::from_str(&json(&[booking()], serde_json::json!({}), None, Some(&offer), false)).unwrap();
    assert_eq!(v["booking"]["services"]["intro"]["minutes"], 30);
    assert_eq!(v["booking"]["open"]["mon"][0], "09:00-17:00");
    assert!(v["forms"]["booking"].is_object(), "the forms stay as they were");
    let none: serde_json::Value = serde_json::from_str(&json(&[], serde_json::json!({}), None, None, false)).unwrap();
    assert!(none.get("booking").is_none(), "no offer, no booking key");
}

fn subscribe(scope: &str, page: &str, body: &str) -> (String, String, SubscribeShortcode) {
    let doc = moss_core::ast::parse(&format!(":::subscribe\n{body}:::\n"));
    let sub = moss_core::ast::question_subscribes_recursive(&doc).remove(0);
    (scope.to_string(), page.to_string(), sub)
}

#[test]
fn subscribe_fields_are_keyed_by_scope_without_the_address_and_carry_other_boxes() {
    let zh = subscribe("", "subscribe", "- 電郵 * {type=email}\n- 怎麼稱呼你 *\n- 從哪裡知道 *\n  - [ ] Facebook\n  - [ ] 其他\n    - 其他管道\n");
    let en = subscribe("en", "en/subscribe", "- Name *\n");
    let (fields, problems) = subscribe_fields(&[zh, en]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(fields["en"], serde_json::json!([{"name": "name", "type": "text", "required": true}]));
    let names: Vec<_> = fields[""].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["怎麼稱呼你", "從哪裡知道", "從哪裡知道_other"]);
    assert_eq!(fields[""][1]["type"], "checkbox");
    assert_eq!(fields[""][2], serde_json::json!({"name": "從哪裡知道_other", "label": "其他管道", "type": "text", "required": false, "other_of": "從哪裡知道"}));
    let v: serde_json::Value = serde_json::from_str(&json(&[], fields.clone(), None, None, true)).unwrap();
    assert_eq!(v["subscribe"]["fields"], fields);
    assert_eq!(v["subscribe"]["turnstile"], true);
}

#[test]
fn two_subscribe_forms_in_one_scope_union_choices_require_only_what_both_require_and_report_a_disagreement() {
    let a = subscribe("", "a", "- Name *\n- City * {type=radio}\n  - Paris\n  - Rome\n");
    let b = subscribe("", "b", "- Name\n- City * {type=radio}\n  - Paris\n  - Oslo\n");
    let (fields, problems) = subscribe_fields(&[a, b]);
    assert_eq!(fields[""][0]["required"], false);
    assert_eq!(fields[""][1]["required"], true);
    assert_eq!(fields[""][1]["options"], serde_json::json!(["Paris", "Rome", "Oslo"]));
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("`a`") && problems[0].contains("`b`") && problems[0].contains("city"), "{problems:?}");
}

#[test]
fn a_hosted_site_publishes_its_newsletter_service_and_a_refused_one_is_a_problem() {
    let dest = destination(&Ok("sendgrid".into()), true).unwrap();
    assert_eq!(dest, serde_json::json!({"provider": "sendgrid"}));
    let v: serde_json::Value = serde_json::from_str(&json(&[], serde_json::json!({}), Some(dest), None, true)).unwrap();
    assert_eq!(v["subscribe"]["newsletter"], serde_json::json!({"provider": "sendgrid"}));
    assert!(destination(&Ok("sendgrid".into()), false).unwrap_err().contains("only by the moss host"));
    let refused = problems(&[], None, None, Some(&Err("[channels.email] provider \"x\" is not known".into())));
    assert!(refused[0].contains("newsletter service is left out"), "{refused:?}");
}
