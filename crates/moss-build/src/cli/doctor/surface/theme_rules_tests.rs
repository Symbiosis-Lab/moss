use super::*;

fn site(pages: &[(&str, &str)], script: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, html) in pages {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, html).unwrap();
    }
    if let Some(js) = script {
        std::fs::write(dir.path().join("theme.js"), js).unwrap();
    }
    dir
}

fn unmatched(css: &str, pages: &[(&str, &str)], script: Option<&str>) -> Vec<UnmatchedRule> {
    let dir = site(pages, script);
    match check(&[("style.css".to_string(), css.to_string())], dir.path()) {
        ThemeRules::Checked { unmatched, .. } => unmatched,
        ThemeRules::NoBuild => panic!("pages were built"),
    }
}

const PAGE: &str = "<html><body><p class=\"moss-eyebrow\"><span data-field=\"stage\">Station 1</span></p><a href=\"/\">x</a></body></html>";

#[test]
fn a_rule_on_a_class_no_page_carries_is_reported_with_its_line() {
    let css = "[data-field=\"stage\"] { color: red; }\n\n.eyebrow,\n.slip { color: blue; }\n";
    let found = unmatched(css, &[("a/index.html", PAGE)], None);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].line, found[0].selector.as_str()), (3, ".eyebrow, .slip"));
    assert!(found[0].scripted.is_empty());
}

#[test]
fn one_matching_selector_in_the_list_keeps_the_rule() {
    assert!(unmatched(".gone, .moss-eyebrow { color: red; }", &[("index.html", PAGE)], None).is_empty());
}

#[test]
fn state_pseudo_classes_and_pseudo_elements_do_not_hide_a_live_rule() {
    let css = "a:hover { x: 1 }\n.moss-eyebrow::before { x: 1 }\n.moss-eyebrow span:first-child:after { x: 1 }\n:focus-visible { x: 1 }\nbody > ::selection { x: 1 }\n";
    assert!(unmatched(css, &[("index.html", PAGE)], None).is_empty());
}

#[test]
fn a_dead_class_behind_a_pseudo_class_is_still_reported() {
    let found = unmatched(".gone:hover::after { x: 1 }", &[("index.html", PAGE)], None);
    assert_eq!(found.len(), 1);
}

#[test]
fn keyframe_steps_and_font_faces_are_not_selectors() {
    let css = "@keyframes fade { from { opacity: 0 } to { opacity: 1 } }\n@font-face { font-family: X; src: url(x.woff2); }\n";
    assert!(unmatched(css, &[("index.html", PAGE)], None).is_empty());
}

#[test]
fn a_rule_inside_a_media_query_is_matched_against_the_pages_too() {
    let found = unmatched("@media (min-width: 40rem) { .moss-eyebrow { x: 1 } .gone { x: 1 } }", &[("index.html", PAGE)], None);
    assert_eq!(found.iter().map(|r| r.selector.as_str()).collect::<Vec<_>>(), vec![".gone"]);
}

#[test]
fn a_class_a_script_adds_is_reported_as_possibly_scripted() {
    let found = unmatched(".menu.is-open { x: 1 }", &[("index.html", PAGE)], Some("el.classList.add(\"is-open\")"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].scripted, vec!["is-open".to_string()]);
}

#[test]
fn a_selector_the_matcher_cannot_parse_is_never_reported() {
    assert!(unmatched(".x:unknown-thing(1) { x: 1 }", &[("index.html", PAGE)], None).is_empty());
}

#[test]
fn no_built_pages_is_no_build() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(check(&[("s.css".into(), ".a{}".into())], dir.path()), ThemeRules::NoBuild);
}

#[test]
fn static_form_strips_what_a_static_page_cannot_show() {
    let form = |s: &str| static_form(s);
    assert_eq!(form("a:hover").as_deref(), Some("a"));
    assert_eq!(form(".x::before").as_deref(), Some(".x"));
    assert_eq!(form(".x :focus").as_deref(), Some(".x *"));
    assert_eq!(form("li:not(.a):hover").as_deref(), Some("li:not(.a)"));
    assert_eq!(form("[title=\"a:hover\"]").as_deref(), Some("[title=\"a:hover\"]"));
    assert_eq!(form("p:first-child").as_deref(), Some("p:first-child"));
    assert_eq!(form(".peer\\:checked").as_deref(), Some(".peer\\:checked"));
}

#[test]
fn a_state_inside_not_makes_the_selector_unknown_not_dead() {
    assert_eq!(static_form("button:not(:disabled)"), None);
    let css = "button:not(:disabled) { x: 1 }\na:not(:hover) { x: 1 }\n";
    let page = "<html><body><button>b</button><a href=\"/\">a</a></body></html>";
    assert!(unmatched(css, &[("index.html", page)], None).is_empty());
}

#[test]
fn a_comment_inside_a_selector_list_keeps_every_selector() {
    let css = ".moss-eyebrow, /* legacy */ .gone { x: 1 }\n.gone /* c */ .also-gone { x: 1 }\n";
    let found = unmatched(css, &[("index.html", PAGE)], None);
    assert_eq!(found.iter().map(|r| r.selector.as_str()).collect::<Vec<_>>(), vec![".gone .also-gone"]);
}

#[test]
fn a_data_attribute_set_through_dataset_counts_as_scripted() {
    let found = unmatched("html[data-color-scheme=\"dark\"] { x: 1 }", &[("index.html", PAGE)], Some("document.documentElement.dataset.colorScheme = \"dark\""));
    assert_eq!(found[0].scripted, vec!["data-color-scheme".to_string()]);
}

#[test]
fn json_in_a_script_tag_is_not_a_script() {
    let page = "<html><body><script type=\"application/ld+json\">{\"name\": \"menu-open\"}</script></body></html>";
    let found = unmatched(".menu-open { x: 1 }", &[("index.html", page)], None);
    assert!(found[0].scripted.is_empty());
}

#[test]
fn redirect_stubs_and_moss_fragments_are_not_pages() {
    let stub = "<html><head><meta http-equiv=\"refresh\" content=\"0; url=/b/\"></head><body><p class=\"only-here\">x</p></body></html>";
    let fragment = "<div class=\"only-there\"></div>";
    let dir = site(&[("index.html", PAGE), ("old/index.html", stub), ("_moss/part.html", fragment)], None);
    let ThemeRules::Checked { pages, unmatched } = check(&[("s.css".into(), ".only-here {}\n.only-there {}\n".into())], dir.path()) else { panic!() };
    assert_eq!((pages, unmatched.len()), (1, 2));
}

#[test]
fn vendor_keyframes_are_not_selectors() {
    assert!(unmatched("@-moz-keyframes spin { from { x: 1 } to { x: 2 } }", &[("index.html", PAGE)], None).is_empty());
}

#[test]
fn split_selector_list_keeps_commas_inside_functions() {
    assert_eq!(split_selector_list(":is(.a, .b) p, .c"), vec![":is(.a, .b) p", ".c"]);
}
