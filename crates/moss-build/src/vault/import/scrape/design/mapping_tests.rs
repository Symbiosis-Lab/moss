use super::*;
use crate::vault::import::scrape::design::style::Family;
use crate::build::page::color_scheme::ColorScheme;
use crate::build::theme_lint::lint_theme_css;

fn registry() -> Tokens {
    moss_core::contract::tokens::load_tokens().unwrap()
}

fn some(s: &str) -> Option<String> {
    Some(s.to_string())
}

fn c(s: &str) -> Option<crate::build::components::color_extract::Rgba> {
    crate::build::components::color_extract::colour(s)
}

/// Casper's facts as the reader gives them (see the reader's tests).
fn family(name: &str, generic: &str) -> Option<Family> {
    Some(Family { name: name.to_string(), generic: some(generic) })
}

fn casper() -> StyleFacts {
    StyleFacts {
        body_family: family("-apple-system", "sans-serif"),
        heading_family: family("-apple-system", "sans-serif"),
        mono_family: family("monospace", "monospace"),
        light: Colours { text: c("#15171a"), background: c("#ffffff"), link: c("#15171a"), accent: c("#ff1a75"), muted: c("#979797") },
        dark: None,
        base_size_px: Some(16.0),
        line_height: Some(1.6),
        heading_weight: Some(800),
        content_width_px: Some(1200.0),
        font_faces: Vec::new(),
    }
}

fn decls(list: &[(String, String)]) -> Vec<(&str, &str)> {
    list.iter().map(|(t, v)| (t.as_str(), v.as_str())).collect()
}

#[test]
fn a_light_only_source_writes_only_what_differs_from_moss() {
    let tokens = registry();
    let body_stack = moss_core::contract::tokens::find_token(&tokens, "moss-font-body").unwrap().0.to_string();
    let out = map(&casper(), &tokens);
    assert_eq!(
        decls(&out.light),
        vec![
            ("--moss-font-heading", body_stack.as_str()),
            ("--moss-reading-size-base", "1rem"),
            ("--moss-read-leading", "1.6"),
            ("--moss-font-heading-weight", "800"),
            ("--moss-color-text", "#15171a"),
            ("--moss-color-bg", "#ffffff"),
            ("--moss-color-accent", "#ff1a75"),
            ("--moss-color-muted", "#979797"),
        ],
        "a system sans heading takes moss's own sans stack; the body and mono already are moss's"
    );
    assert!(out.dark.is_empty());
    assert!(out.families.is_empty());
    assert_eq!(out.residuals, vec!["page width 1200px: wider than a reading column, so moss keeps its own measure".to_string()]);
    assert!(lint_theme_css(&out.css(), ColorScheme::Light).is_empty(), "{:?}", lint_theme_css(&out.css(), ColorScheme::Light));
}

#[test]
fn a_dark_scheme_writes_a_dark_value_for_every_light_colour_written() {
    let mut facts = casper();
    // The dark muted is moss's own dark muted; it is written all the same, beside the light one.
    facts.dark = Some(Colours { text: c("rgba(255, 255, 255, 0.75)"), link: c("rgba(255, 255, 255, 0.75)"), background: c("#151719"), muted: c("#9a9080"), ..facts.light.clone() });
    let out = map(&facts, &registry());
    assert_eq!(
        decls(&out.dark),
        vec![("--moss-color-text", "rgba(255, 255, 255, 0.75)"), ("--moss-color-bg", "#151719"), ("--moss-color-accent", "#ff1a75"), ("--moss-color-muted", "#9a9080")]
    );
    assert!(out.css().contains(":root[data-theme=\"dark\"] {\n  --moss-color-text: rgba(255, 255, 255, 0.75);"), "{}", out.css());
    assert!(lint_theme_css(&out.css(), ColorScheme::Auto).is_empty(), "{:?}", lint_theme_css(&out.css(), ColorScheme::Auto));
}

#[test]
fn a_web_font_goes_in_front_of_moss_s_stack_never_alone() {
    let facts = StyleFacts { body_family: family("Inter", "sans-serif"), heading_family: family("Inter", "sans-serif"), mono_family: family("Field Mono", "monospace"), ..Default::default() };
    let out = map(&facts, &registry());
    let stack = |token: &str| out.light.iter().find(|(t, _)| t == token).unwrap().1.clone();
    assert!(stack("--moss-font-body").starts_with("'Inter', -apple-system,") && stack("--moss-font-body").contains("PingFang"), "{out:?}");
    assert_eq!(stack("--moss-font-heading"), stack("--moss-font-body"), "a sans heading falls back to sans, not to moss's serif heading stack");
    assert!(stack("--moss-font-mono").starts_with("'Field Mono', ui-monospace,"), "{out:?}");
    assert_eq!(out.families, vec!["Inter".to_string(), "Field Mono".to_string()], "each family once");
}

#[test]
fn a_family_name_that_could_break_out_of_its_quotes_is_not_written() {
    let facts = StyleFacts { body_family: family("Evil'; } body { color: red", "serif"), ..Default::default() };
    let out = map(&facts, &registry());
    assert!(out.is_empty() && out.families.is_empty(), "{out:?}");
    assert_eq!(out.residuals.len(), 1);
}

#[test]
fn moss_s_own_values_write_nothing() {
    let facts = StyleFacts {
        base_size_px: Some(18.0),
        line_height: Some(1.75),
        heading_weight: Some(480),
        light: Colours { text: c("#2c2825"), background: c("rgb(250, 248, 245)"), accent: c("#2d5a2d"), ..Default::default() },
        ..Default::default()
    };
    let out = map(&facts, &registry());
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(out.css(), "");
}

#[test]
fn a_reading_measure_becomes_the_content_width_unless_it_is_moss_s_own() {
    let facts = StyleFacts { base_size_px: Some(18.0), content_width_px: Some(720.0), ..Default::default() };
    assert_eq!(decls(&map(&facts, &registry()).light), vec![("--moss-content-width", "calc(40 * var(--moss-reading-size))")]);
    let own = StyleFacts { base_size_px: Some(18.0), content_width_px: Some(756.0), ..Default::default() };
    assert!(map(&own, &registry()).is_empty(), "42 x 18px is moss's own measure");
}

#[test]
fn links_in_the_accent_follow_it_and_other_link_colours_are_written_as_they_are() {
    let mut facts = casper();
    facts.light.link = c("#ff1a75");
    let link = |out: &Overrides| out.light.iter().find(|(t, _)| t == "--moss-link-color").map(|(_, v)| v.clone());
    assert_eq!(link(&map(&facts, &registry())), some("var(--moss-color-accent)"));
    facts.light.link = c("#2050c0");
    assert_eq!(link(&map(&facts, &registry())), some("#2050c0"));
    facts.light.link = facts.light.text.clone();
    assert_eq!(link(&map(&facts, &registry())), None, "moss's links already rest in the text colour");
}

#[test]
fn the_han_block_selects_exactly_the_pages_moss_swaps_to_its_han_stacks() {
    let swap = format!("{HAN_PAGES} {{\n  --moss-font-body: var(--moss-font-body-han);\n  --moss-font-heading: var(--moss-font-heading-han);");
    assert!(crate::build::page::shell::DEFAULT_CSS.contains(&swap), "site.css's swap selector moved; the import's must follow");
}

/// A token's value on a page in `lang`, every `var()` in it replaced, as the
/// theme sits over moss's page stylesheet and tokens.
fn resolved(css: &str, lang: &str, token: &str) -> String {
    let layered = crate::build::theme_lint::Layered::for_page(Some(css), lang);
    let mut value = layered.values(token).0.unwrap();
    while let Some((head, rest)) = value.split_once("var(") {
        let (name, tail) = rest.split_once(')').unwrap();
        value = format!("{head}{}{tail}", layered.values(name.trim()).0.unwrap());
    }
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn a_chinese_japanese_or_korean_page_keeps_moss_s_han_faces_behind_the_source_font() {
    let tokens = registry();
    let default = |name: &str| moss_core::contract::tokens::find_token(&tokens, name).unwrap().0.to_string();
    let facts = StyleFacts { body_family: family("Inter", "sans-serif"), heading_family: family("EB Garamond", "serif"), ..Default::default() };
    let css = map(&facts, &tokens).css();

    let hant = resolved(&css, "zh-Hant", "--moss-font-body");
    assert!(hant.starts_with("'Inter', ") && hant.contains("'PingFang TC'"), "{hant}");
    let hans = resolved(&css, "zh-Hans", "--moss-font-body");
    assert!(hans.starts_with("'Inter', ") && hans.contains("'PingFang SC'") && !hans.contains("TC'"), "{hans}");
    let heading = resolved(&css, "zh-Hant", "--moss-font-heading");
    assert!(heading.starts_with("'EB Garamond', 'Source Han Serif TC'"), "{heading}");
    assert_eq!(resolved(&css, "en", "--moss-font-body"), format!("'Inter', {}", default("moss-font-body")));
    assert_eq!(resolved(&css, "en", "--moss-font-heading"), format!("'EB Garamond', {}", default("moss-font-heading")));
    assert!(lint_theme_css(&css, ColorScheme::Auto).is_empty(), "{:?}", lint_theme_css(&css, ColorScheme::Auto));

    // A system sans heading (Casper) takes the Han sans stack there, not the serif one.
    let casper = map(&casper(), &tokens).css();
    let casper_heading = resolved(&casper, "zh-Hant", "--moss-font-heading");
    assert!(casper_heading.starts_with("-apple-system, BlinkMacSystemFont, 'Helvetica Neue', 'Moss PingFang TC'"), "{casper_heading}");
}
