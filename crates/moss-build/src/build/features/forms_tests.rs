use super::*;
use moss_core::ast::extract_form::parse_form;

const BOOKING: &str = "\
- Your name * {autocomplete=name}
- Email * {type=email}
- Phone {type=tel}
- Session * {type=radio}
  - Portrait
  - Family
- Preferred day {type=date}
- Anything we should know {type=textarea}
  Who is coming.
- Send me the newsletter {type=checkbox}
- Interests {type=checkbox}
  - Print
  - Web
- Colour {type=select}
  - Red
  - Blue
";

fn booking() -> FormShortcode {
    parse_form("{#booking next=/booking/received/ button=\"Ask for a date\"}", BOOKING).0
}

fn html(endpoint: Option<&str>) -> String {
    render_form(&booking(), Language::En, endpoint)
}

#[test]
fn the_form_posts_to_the_endpoint_and_the_form_name() {
    let h = html(Some("https://forms.example/f"));
    assert!(h.contains(r#"method="post" action="https://forms.example/f/booking""#), "{h}");
    assert!(!h.contains("data-moss-pending"));
}

#[test]
fn a_form_with_no_endpoint_is_marked_pending() {
    let h = html(None);
    assert!(h.contains(r##"action="#""##) && h.contains(r#"data-moss-pending="true""#), "{h}");
}

#[test]
fn every_field_has_a_label_for_its_control() {
    let h = html(Some("https://f.example"));
    for id in ["booking-your_name", "booking-email", "booking-phone", "booking-preferred_day", "booking-anything_we_should_know", "booking-send_me_the_newsletter", "booking-colour"] {
        assert!(h.contains(&format!(r#"<label class="moss-form__label" for="{id}">"#)) || h.contains(&format!(r#"<label for="{id}">"#)), "{id} in {h}");
        assert!(h.contains(&format!(r#"id="{id}""#)), "{id}");
    }
    assert!(h.contains(r#"<label for="booking-session-0">Portrait</label>"#));
}

#[test]
fn a_required_field_has_required_and_an_optional_one_says_optional() {
    let h = html(Some("https://f.example"));
    assert!(h.contains(r#"id="booking-your_name" name="your_name" required"#), "{h}");
    assert!(h.contains(r#"id="booking-phone" name="phone" autocomplete="tel""#), "{h}");
    assert!(!h.contains(r#"id="booking-phone" name="phone" required"#));
    assert_eq!(h.matches(r#"moss-form__optional">Optional<"#).count(), 6, "{h}");
    assert!(!h.contains("Your name *") && !h.contains(">*<"));
}

#[test]
fn choice_groups_are_fieldsets_with_a_legend() {
    let h = html(Some("https://f.example"));
    assert!(h.contains(r#"<fieldset class="moss-form__field moss-form__group" data-field="session">"#), "{h}");
    assert!(h.contains("<legend>Session</legend>"), "{h}");
    assert!(h.contains(r#"<legend>Interests <span class="moss-form__optional">Optional</span></legend>"#), "{h}");
    assert!(h.contains(r#"type="radio" id="booking-session-0" name="session" value="Portrait" required"#), "{h}");
    assert!(h.contains(r#"type="checkbox" id="booking-interests-1" name="interests" value="Web">"#), "{h}");
}

#[test]
fn a_single_checkbox_sends_on() {
    let h = html(Some("https://f.example"));
    assert!(h.contains(r#"<input type="checkbox" value="on" id="booking-send_me_the_newsletter" name="send_me_the_newsletter">"#), "{h}");
}

#[test]
fn autocomplete_tokens_help_and_describedby() {
    let h = html(Some("https://f.example"));
    assert!(h.contains(r#"name="your_name" required autocomplete="name""#), "{h}");
    assert!(h.contains(r#"name="email" required autocomplete="email""#), "{h}");
    assert!(h.contains(r#"<p class="moss-form__help" id="booking-anything_we_should_know-help">Who is coming.</p>"#), "{h}");
    assert!(h.contains(r#"aria-describedby="booking-anything_we_should_know-help""#), "{h}");
}

#[test]
fn the_honeypot_and_next_are_present_and_hidden_from_readers() {
    let h = html(Some("https://f.example"));
    assert!(h.contains(r#"<input type="hidden" name="_next" value="/booking/received/">"#), "{h}");
    assert!(h.contains(r#"<div class="moss-form__hp" aria-hidden="true">"#), "{h}");
    assert!(h.contains(r#"name="_hp" value="" tabindex="-1" autocomplete="off""#), "{h}");
}

#[test]
fn the_button_label_defaults_to_the_languages_send_and_can_be_overridden() {
    assert!(html(None).contains(">Ask for a date<"));
    let plain = parse_form("{#a next=/a/}", "- A\n").0;
    assert!(render_form(&plain, Language::En, None).contains(">Send<"));
    assert!(render_form(&plain, Language::De, None).contains(">Senden<"));
    assert!(render_form(&plain, Language::ZhHans, None).contains(">发送<"));
    assert!(render_form(&plain, Language::ZhHant, None).contains(">送出<"));
}

#[test]
fn author_text_is_escaped() {
    let f = parse_form("{#a next=/a/}", "- Say <b>hi</b> \"now\"\n").0;
    let h = render_form(&f, Language::En, None);
    assert!(!h.contains("<b>") && h.contains("&lt;b&gt;"), "{h}");
}
