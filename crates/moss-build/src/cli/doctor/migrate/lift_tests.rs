use super::*;
use moss_core::fields::{DecimalMark, FieldKind};

const FORMAT: PageFormat<'static> = PageFormat { lang: "en", decimal: DecimalMark::Point };

fn page(rel: &str, text: &str) -> (String, String) {
    (rel.to_string(), text.to_string())
}

fn lift(pages: &[(String, String)], config: &str, decls: &[FieldDecl], place: Option<Place>) -> LiftPlan {
    plan(&LiftRequest { class: "eyebrow", field: "stage", place }, pages, config, decls, FORMAT).unwrap()
}

const PIER: &str = "---\ntitle: \"Pier 4\"\nweight: 4\n---\n\n::: {.eyebrow}\nPier 4 of 9\n:::\n\nThe text.\n";

#[test]
fn a_plain_div_becomes_a_frontmatter_line_and_the_rest_of_the_page_stays() {
    let plan = lift(&[page("Piers/four.md", PIER)], "", &[], Some(Place::Eyebrow));
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    assert_eq!(plan.pages[0].after, "---\ntitle: \"Pier 4\"\nweight: 4\nstage: Pier 4 of 9\n---\n\nThe text.\n");
}

#[test]
fn the_field_is_declared_once_with_its_place() {
    let config = "# site settings\n[site]\nlang = \"en\"\n";
    let plan = lift(&[page("a.md", PIER)], config, &[], Some(Place::Eyebrow));
    assert_eq!(plan.config.as_deref(), Some("# site settings\n[site]\nlang = \"en\"\n\n[fields.stage]\nplace = \"eyebrow\"\ntype = \"text\"\n"));
}

#[test]
fn an_already_declared_field_leaves_the_config_alone() {
    let decl = FieldDecl { name: "stage".into(), kind: FieldKind::Text, choices: vec![], place: Some(Place::Eyebrow), label: vec![], scope: vec![] };
    assert_eq!(lift(&[page("a.md", PIER)], "[fields.stage]\ntype = \"text\"\nplace = \"eyebrow\"\n", &[decl], Some(Place::Eyebrow)).config, None);
}

#[test]
fn a_div_with_a_link_or_two_paragraphs_or_extra_attributes_is_left_and_named() {
    let pages = [
        page("link.md", "::: {.eyebrow}\nSee [[Pier 3]]\n:::\n"),
        page("two.md", "::: {.eyebrow}\none\n\ntwo\n:::\n"),
        page("attrs.md", "::: {.eyebrow #top}\nPier 1\n:::\n"),
        page("twice.md", "::: {.eyebrow}\na\n:::\n\n::: {.eyebrow}\nb\n:::\n"),
        page("emph.md", "::: {.eyebrow}\nPier *one*\n:::\n"),
        page("half.md", "::: {.eyebrow}\n*Pier* one\n:::\n"),
    ];
    let plan = lift(&pages, "", &[], None);
    assert!(plan.pages.is_empty(), "{:?}", plan.pages);
    let reasons: Vec<(&str, Option<usize>)> = plan.skipped.iter().map(|s| (s.path.as_str(), s.line)).collect();
    let at_one = |p| (p, Some(1));
    assert_eq!(reasons, vec![at_one("link.md"), at_one("two.md"), at_one("attrs.md"), at_one("twice.md"), at_one("emph.md"), at_one("half.md")]);
}

#[test]
fn a_different_value_already_in_the_frontmatter_is_a_conflict_and_the_same_value_is_not() {
    let differs = "---\nstage: Pier 5\n---\n::: {.eyebrow}\nPier 4\n:::\n";
    let same = "---\nstage: Pier 4\n---\n::: {.eyebrow}\nPier 4\n:::\nx\n";
    let plan = lift(&[page("differs.md", differs), page("same.md", same)], "", &[], None);
    assert_eq!(plan.skipped.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), vec!["differs.md"]);
    assert_eq!(plan.pages[0].after, "---\nstage: Pier 4\n---\nx\n");
}

#[test]
fn a_div_inside_a_code_fence_is_not_touched() {
    let fenced = "```md\n::: {.eyebrow}\nx\n:::\n```\n";
    let plan = lift(&[page("doc.md", fenced)], "", &[], None);
    assert!(plan.pages.is_empty() && plan.skipped.is_empty());
}

#[test]
fn a_value_that_does_not_fit_the_declared_type_is_left() {
    let decl = FieldDecl { name: "stage".into(), kind: FieldKind::Number, choices: vec![], place: None, label: vec![], scope: vec![] };
    let plan = lift(&[page("a.md", PIER)], "[fields.stage]\ntype = \"number\"\n", &[decl], None);
    assert!(plan.pages.is_empty());
    assert!(plan.skipped[0].reason.contains("does not fit"), "{:?}", plan.skipped);
}

#[test]
fn a_moss_field_other_than_the_placeable_ones_is_refused() {
    let err = plan(&LiftRequest { class: "eyebrow", field: "title", place: None }, &[], "", &[], FORMAT).unwrap_err();
    assert!(err.contains("moss field"), "{err}");
}

#[test]
fn removing_a_div_at_the_top_of_the_body_does_not_leave_two_blank_lines() {
    assert_eq!(remove_lines("a\n\nx\ny\n\nb\n", 2, 3), "a\n\nb\n");
    assert_eq!(remove_lines("x\ny\nb\n", 0, 1), "b\n");
}

#[test]
fn a_div_holding_math_is_left() {
    let plan = lift(&[page("m.md", "::: {.eyebrow}\nCost $x$\n:::\n")], "", &[], None);
    assert!(plan.pages.is_empty() && plan.skipped.len() == 1);
}

#[test]
fn a_div_wholly_in_italics_lifts_its_text_and_says_so() {
    let plan = lift(&[page("i.md", "---\nt: 1\n---\n::: {.eyebrow}\n*Pier four*\n:::\n")], "", &[], None);
    assert_eq!(plan.pages[0].after, "---\nt: 1\nstage: Pier four\n---\n");
    assert_eq!(plan.unstyled, 1);
}

#[test]
fn nothing_lifted_declares_nothing() {
    assert_eq!(lift(&[page("link.md", "::: {.eyebrow}\nSee [[Pier 3]]\n:::\n")], "", &[], None).config, None);
}

#[test]
fn a_list_or_empty_value_already_in_the_frontmatter_is_not_overwritten() {
    let list = "---\nstage: [a, b]\n---\n::: {.eyebrow}\nPier 4\n:::\n";
    let empty = "---\nstage:\n---\n::: {.eyebrow}\nPier 4\n:::\n";
    let plan = lift(&[page("list.md", list), page("empty.md", empty)], "", &[], None);
    assert!(plan.pages.is_empty(), "{:?}", plan.pages);
    assert_eq!(plan.skipped.len(), 2);
}

#[test]
fn place_none_clears_a_declared_place() {
    let decl = FieldDecl { name: "stage".into(), kind: FieldKind::Text, choices: vec![], place: Some(Place::Eyebrow), label: vec![], scope: vec![] };
    let config = "[fields.stage]\ntype = \"text\"\nplace = \"eyebrow\"\n";
    let request = LiftRequest { class: "eyebrow", field: "stage", place: None };
    let plan = plan(&request, &[page("a.md", PIER)], config, &[decl], FORMAT).unwrap();
    assert_eq!(plan.config.as_deref(), Some("[fields.stage]\ntype = \"text\"\n"));
}

#[test]
fn a_different_place_replaces_the_declared_one() {
    let decl = FieldDecl { name: "stage".into(), kind: FieldKind::Text, choices: vec![], place: Some(Place::Eyebrow), label: vec![], scope: vec![] };
    let config = "[fields.stage]\ntype = \"text\"\nplace = \"eyebrow\"\n";
    let plan = lift(&[page("a.md", PIER)], config, &[decl], Some(Place::Subtitle));
    assert_eq!(plan.config.as_deref(), Some("[fields.stage]\ntype = \"text\"\nplace = \"subtitle\"\n"));
}
