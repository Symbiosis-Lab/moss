//! The reader against Ghost's Casper and Source stylesheets, unmodified, on
//! home pages written from each theme's own templates. Every expected value
//! here is what Chromium computes for the same page.

use super::*;

const CASPER_CSS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/design-import/casper/screen.css"));
const CASPER_HOME: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/design-import/casper/home.html"));
const CASPER_POST: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/design-import/casper/post.html"));
const SOURCE_POST: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/design-import/source/post.html"));
const SOURCE_CSS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/design-import/source/screen.css"));
const SOURCE_HOME: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/design-import/source/home.html"));

const SITE: &str = "https://notes.example/";

/// The page's `<style>` elements and its one stylesheet link, in document
/// order, as the import fetches them.
fn sheets(home: &str, linked: &str) -> Vec<Sheet> {
    let doc = Html::parse_document(home);
    let base = Url::parse(SITE).unwrap();
    doc.select(&sel("style, link[rel=stylesheet]"))
        .map(|e| match e.value().name() {
            "style" => Sheet { css: e.text().collect(), base: base.clone() },
            _ => Sheet { css: linked.to_string(), base: base.join("/assets/built/screen.css").unwrap() },
        })
        .collect()
}

fn fill(page: &str, values: &[(&str, &str)]) -> String {
    let filled = [("title", "First light on the ridge"), ("date", "2026-11-02"), ("text", "The path climbs through birch."), ("lang", "en")];
    values.iter().chain(&filled).fold(page.to_string(), |p, (k, v)| p.replace(&format!("{{{{{k}}}}}"), v))
}

fn casper_with(html_class: &str, post: bool) -> StyleFacts {
    let values = [("html_class", html_class), ("body_class", "")];
    let home = fill(CASPER_HOME, &values);
    let post = post.then(|| fill(CASPER_POST, &values));
    read(&home, post.as_deref(), &sheets(&home, CASPER_CSS))
}

fn casper(html_class: &str) -> StyleFacts {
    casper_with(html_class, false)
}

fn source(title_font: &str) -> StyleFacts {
    let home = fill(SOURCE_HOME, &[("title_font", title_font), ("serif_fonts", "")]);
    read(&home, None, &sheets(&home, SOURCE_CSS))
}

fn some(s: &str) -> Option<String> {
    Some(s.to_string())
}

fn c(s: &str) -> Option<crate::build::components::color_extract::Rgba> {
    crate::build::components::color_extract::colour(s)
}

fn name(f: &Option<Family>) -> Option<String> {
    f.as_ref().map(|f| f.name.clone())
}

#[test]
fn casper_reads_as_chromium_renders_it() {
    let facts = casper("");
    assert_eq!(name(&facts.body_family), some("-apple-system"));
    assert_eq!(facts.heading_family, Some(Family { name: "-apple-system".to_string(), generic: some("sans-serif") }));
    assert_eq!(facts.mono_family, None, "no code on either page: nothing to read, not the browser's monospace");
    assert_eq!(facts.base_size_px, Some(16.0), "1.6rem against html's 62.5%");
    assert_eq!(facts.line_height, Some(1.6), "1.6em on the body");
    assert_eq!(facts.heading_weight, Some(800), "the site title's own weight");
    assert_eq!(facts.content_width_px, Some(1200.0));
    let light = Colours { text: c("#15171a"), background: c("#ffffff"), link: c("#15171a"), accent: c("#ff1a75"), muted: c("#979797") };
    assert_eq!(facts.light, light);
    assert_eq!(facts.dark, None, "the dark rules need html.auto-color or html.dark-mode");
}

#[test]
fn casper_set_to_follow_the_system_has_a_dark_scheme() {
    let dark = casper(" class=\"auto-color\"").dark.expect("auto-color reads prefers-color-scheme");
    assert_eq!(dark.text, c("rgba(255, 255, 255, 0.75)"));
    assert_eq!(dark.background, c("#151719"));
    assert_eq!(dark.muted, c("#979797"));
}

#[test]
fn source_reads_inter_and_its_self_served_face() {
    let facts = source("sans");
    assert_eq!(facts.body_family, Some(Family { name: "Inter".to_string(), generic: some("sans-serif") }));
    assert_eq!(name(&facts.heading_family), some("Inter"));
    assert_eq!(facts.heading_weight, Some(725), "an entry title, the page having no h1");
    assert_eq!(facts.light.muted, c("rgba(0, 0, 0, 0.55)"));
    assert_eq!(facts.light.background, c("#ffffff"), "--background-color from the inline style after the stylesheet");
    let face = &facts.font_faces[0];
    assert_eq!((face.family.as_str(), face.source.as_str(), face.format.as_deref()), ("Inter", "https://notes.example/assets/fonts/inter-roman-latin.woff2", Some("woff2")));
}

#[test]
fn source_s_serif_title_setting_reaches_the_entry_titles() {
    let serif = source("serif");
    assert_eq!(serif.heading_family, Some(Family { name: "EB Garamond".to_string(), generic: some("serif") }));
    assert_eq!(serif.heading_weight, Some(550));
    assert_eq!(name(&source("serif").body_family), some("Inter"));
}

fn one(html_body: &str, css: &str) -> StyleFacts {
    let html = format!("<html><head></head><body>{html_body}</body></html>");
    read(&html, None, &[Sheet { css: css.to_string(), base: Url::parse(SITE).unwrap() }])
}

#[test]
fn document_order_decides_and_other_conditions_are_skipped() {
    let css = "body.x { color: #111111 } body { color: #222222 }\n\
               @media (min-width: 40em) { body { color: #333333 } }\n\
               @media (scripting: none) { body { color: #444444 } }";
    assert_eq!(one("", css).light.text, c("#222222"), "the later rule wins over the more specific one");
}

#[test]
fn the_accent_is_the_accent_property_the_rules_use_most() {
    let css = ":root { --accent-bg: #eeeeee; --site-accent: #b0302a } a { color: var(--site-accent) } .button { background: var(--site-accent) }";
    assert_eq!(one("", css).light.accent, c("#b0302a"), "not the alphabetically first one");
}

#[test]
fn a_size_the_reader_cannot_resolve_is_no_fact() {
    let css = "body { font-size: clamp(1rem, 2vw, 1.25rem); line-height: 1.5em } main { max-width: 40em }";
    let facts = one("<main><h2><a href=\"/x\">Entry</a></h2></main>", css);
    assert_eq!((facts.base_size_px, facts.line_height, facts.content_width_px), (None, None, None), "not the browser's 16px, and nothing relative to it");
    let written = super::super::mapping::map(&facts, &moss_core::contract::tokens::load_tokens().unwrap()).light;
    assert!(!written.iter().any(|(t, _)| ["--moss-reading-size-base", "--moss-read-leading", "--moss-content-width"].contains(&t.as_str())), "{written:?}");
}

#[test]
fn var_fallbacks_shorthands_and_relative_sizes_resolve() {
    let css = ":root { --ink: #101010 } html { font-size: 50% }\n\
               body { font: 600 2rem/1.5 'Field Sans', serif; color: var(--missing, var(--ink)); background: #fafafa }\n\
               main { font-size: 150%; max-width: 40em } main h2 { line-height: 2em }";
    let facts = one("<main><h2><a href=\"/x\">Entry</a></h2></main>", css);
    assert_eq!(name(&facts.body_family), some("Field Sans"));
    assert_eq!(facts.base_size_px, Some(16.0));
    assert_eq!(facts.line_height, Some(1.5));
    assert_eq!(facts.heading_weight, Some(700), "the browser's bold for a heading the sheet leaves alone");
    assert_eq!(facts.light.text, c("#101010"));
    assert_eq!(facts.light.background, c("#fafafa"));
    assert_eq!(facts.content_width_px, Some(960.0), "40em of main's 24px");
}

#[test]
fn a_post_gives_the_reading_styles_the_home_page_lacks() {
    let facts = casper_with("", true);
    assert_eq!(facts.light.link, c("#ff1a75"), "Casper draws links in a post's text in the accent");
    assert_eq!((facts.base_size_px, facts.line_height), (Some(20.0), Some(1.6)), "a post paragraph's size, not the body's 16px");
    assert_eq!(facts.light.muted, c("#979797"), "the post's date");
    assert_eq!(facts.content_width_px, None, "Casper sets its column with a grid, which the reader does not follow");
    assert_eq!(name(&facts.body_family), some("-apple-system"), "the body is still the home page's");
    let home = Html::parse_document(&fill(SOURCE_HOME, &[("title_font", "serif"), ("serif_fonts", "")]));
    let post = fill(SOURCE_POST, &[("title_font", "serif"), ("serif_fonts", "")]);
    let facts = read(&home.html(), Some(&post), &sheets(&home.html(), SOURCE_CSS));
    assert_eq!((name(&facts.heading_family), facts.heading_weight), (some("EB Garamond"), Some(550)));
    assert_eq!(facts.light.link, c("#ff1a75"));
    assert_eq!((facts.base_size_px, facts.line_height), (Some(17.0), Some(1.6)));
}
