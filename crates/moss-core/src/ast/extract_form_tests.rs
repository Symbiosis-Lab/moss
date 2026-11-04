use super::*;

const BOOKING: &str = "\
- Your name * {autocomplete=name}
- Email * {type=email}
- Phone {type=tel}
- Session * {type=radio}
  - Portrait
  - Family
  - Headshots
- Preferred day {type=date}
- Anything we should know {type=textarea}
  Who is coming, and any ideas for the session.
- Send me the studio's newsletter {type=checkbox}
";

fn booking() -> (FormShortcode, Vec<String>) {
    parse_form("{#booking next=/booking/received/ button=\"Ask for a date\"}", BOOKING)
}

#[test]
fn the_form_is_named_by_its_id_and_carries_next_and_button() {
    let (form, problems) = booking();
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(form.name.as_deref(), Some("booking"));
    assert_eq!(form.next.as_deref(), Some("/booking/received/"));
    assert_eq!(form.button.as_deref(), Some("Ask for a date"));
    assert_eq!(form.fields.len(), 7);
}

#[test]
fn a_label_gives_the_name_and_a_trailing_star_the_required_flag_without_showing_the_star() {
    let (form, _) = booking();
    let f = &form.fields[0];
    assert_eq!((f.label.as_str(), f.name.as_str(), f.required), ("Your name", "your_name", true));
    assert_eq!(f.kind, FieldKind::Text);
    assert_eq!(f.autocomplete.as_deref(), Some("name"));
    let phone = &form.fields[2];
    assert_eq!((phone.label.as_str(), phone.required, phone.kind), ("Phone", false, FieldKind::Tel));
}

#[test]
fn email_and_tel_fields_carry_the_matching_autocomplete_token() {
    let (form, _) = booking();
    assert_eq!(form.fields[1].autocomplete.as_deref(), Some("email"));
    assert_eq!(form.fields[2].autocomplete.as_deref(), Some("tel"));
}

#[test]
fn a_nested_list_gives_the_choices() {
    let (form, _) = booking();
    let session = &form.fields[3];
    assert_eq!(session.kind, FieldKind::Radio);
    assert_eq!(session.choices, ["Portrait", "Family", "Headshots"]);
    assert!(session.required);
    assert_eq!(session.help, None, "a nested list is the choices, never help text");
}

#[test]
fn a_paragraph_continuing_the_item_is_its_help_text() {
    let (form, _) = booking();
    assert_eq!(form.fields[5].help.as_deref(), Some("Who is coming, and any ideas for the session."));
    assert_eq!(form.fields[5].kind, FieldKind::Textarea);
    let (loose, _) = parse_form("{#a next=/a/}", "- Note {type=textarea}\n\n  First line.\n\n  Second line.\n");
    assert_eq!(loose.fields[0].help.as_deref(), Some("First line.\n\nSecond line."));
}

#[test]
fn a_checkbox_with_no_nested_list_is_one_box_and_with_one_is_a_group() {
    let (form, problems) = booking();
    assert!(problems.is_empty());
    assert_eq!(form.fields[6].kind, FieldKind::Checkbox);
    assert!(form.fields[6].choices.is_empty());
    assert_eq!(form.fields[6].label, "Send me the studio's newsletter");
    let (group, _) = parse_form("{#a next=/a/}", "- Interests {type=checkbox}\n  - Print\n  - Web\n");
    assert_eq!(group.fields[0].choices, ["Print", "Web"]);
}

#[test]
fn name_sets_the_field_name_and_a_label_without_latin_letters_must_set_it() {
    let (form, problems) = parse_form("{#a next=/a/}", "- 姓名 * {name=full_name}\n- 电话\n");
    assert_eq!(form.fields[0].name, "full_name");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("电话") && problems[0].contains("name="), "{problems:?}");
}

#[test]
fn chinese_labels_help_and_choices_read_like_latin_ones() {
    let body = "- 您的姓名 * {name=full_name}\n  请写全名。\n\n  我们只用它回信。\n- 课程 * {type=radio name=course}\n  - 肖像\n  - 家庭\n- 备注{name=note}\n";
    let (form, problems) = parse_form("{#a next=/a/}", body);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(form.fields[2].label, "备注{name=note}", "a `{{` glued to the label is part of it");
    let name = &form.fields[0];
    assert_eq!((name.label.as_str(), name.name.as_str(), name.required), ("您的姓名", "full_name", true));
    assert_eq!(name.help.as_deref(), Some("请写全名。\n\n我们只用它回信。"));
    let course = &form.fields[1];
    assert_eq!((course.kind, course.choices.as_slice()), (FieldKind::Radio, ["肖像".to_string(), "家庭".to_string()].as_slice()));
}

#[test]
fn each_build_problem_is_named() {
    let cases: &[(&str, &str, &str)] = &[
        ("{next=/a/}", "- A\n", "#"),
        ("{#Bad_Name next=/a/}", "- A\n", "Bad_Name"),
        ("{#a next=/a/}", "- A\n- A\n", "twice"),
        ("{#a next=/a/}", "- A {type=banana}\n", "banana"),
        ("{#a next=/a/}", "- A {type=select}\n  - Only\n", "at least two"),
        ("{#a next=/a/}", "- A {type=radio}\n", "at least two"),
        ("{#a next=/a/}", "- A {type=text}\n  - x\n  - y\n", "choices"),
        ("{#a next=/a/}", "- A {name=_hp}\n", "reserved"),
        ("{#a next=/a/}", "- A {name=_next}\n", "reserved"),
        ("{#a next=/a/}", "- A {name=Big}\n", "Big"),
        ("{#a next=/a/}", "- 2nd guest\n", "2nd_guest"),
        ("{#a next=https://x.test/}", "- A\n", "next"),
        ("{#a next=//x.test/}", "- A\n", "next"),
        ("{#a next=thanks}", "- A\n", "next"),
        ("{#a}", "- A\n", "next"),
    ];
    for (args, body, needle) in cases {
        let (_, problems) = parse_form(args, body);
        assert!(problems.iter().any(|p| p.contains(needle)), "{args} {body:?} wanted {needle}: {problems:?}");
    }
}

#[test]
fn a_form_name_over_forty_characters_is_a_problem() {
    let args = format!("{{#{} next=/a/}}", "a".repeat(41));
    assert!(!parse_form(&args, "- A\n").1.is_empty());
}

#[test]
fn a_clean_form_has_no_problems() {
    assert!(parse_form("{#a next=/a/}", "- Name *\n- Colour {type=select}\n  - Red\n  - Blue\n").1.is_empty());
}

#[test]
fn a_form_fence_parses_to_a_form_shortcode_and_its_id_is_not_ignored() {
    use crate::ast::{parse, Block, Shortcode};
    let doc = parse(&format!(":::form {{#booking next=/booking/received/}}\n{BOOKING}:::\n"));
    assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    match &doc.blocks[..] {
        [Block::Shortcode(Shortcode::Form(f))] => {
            assert_eq!(f.name.as_deref(), Some("booking"));
            assert_eq!(f.fields.len(), 7);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_form_problem_is_a_build_warning() {
    use crate::ast::parse;
    let doc = parse(":::form {#booking next=/x/}\n- A {type=banana}\n:::\n");
    assert!(doc.warnings.iter().any(|w| w.contains("banana")), "{:?}", doc.warnings);
}

#[test]
fn forms_inside_a_grid_cell_are_found() {
    use crate::ast::{forms_recursive, parse};
    let doc = parse("::::grid 2\n:::form {#a next=/a/}\n- A\n:::\n+++\ntext\n::::\n");
    assert_eq!(forms_recursive(&doc).len(), 1);
    assert!(forms_recursive(&parse("plain")).is_empty());
}
