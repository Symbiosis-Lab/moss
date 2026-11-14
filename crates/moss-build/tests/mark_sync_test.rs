//! The marks moss ships are the brand's mark: the favicon carries the brand
//! logo's own paths, and the colophon's small cut keeps the logo's two inks.
//! The small cut is a separate simplified drawing, so its paths are not
//! compared.

use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Each `<path>` tag's `fill` and `d`, in paint order.
fn paths(svg: &str) -> Vec<(String, String)> {
    let attr = |tag: &str, name: &str| tag.split(&format!(" {name}=\"")).nth(1).and_then(|v| v.split('"').next()).unwrap_or_default().to_string();
    svg.split("<path").skip(1).map(|tag| tag.split('>').next().unwrap()).map(|tag| (attr(tag, "fill"), attr(tag, "d"))).collect()
}

const BRAND: &str = "../../site/assets/brand/logo.svg";
const BRAND_DARK: &str = "../../site/assets/brand/logo-dark.svg";
const DROP: &str = "#4f7031";
const DROP_DARK: &str = "#a3d483";

#[test]
fn the_favicon_draws_the_brand_logo() {
    let (brand, favicon) = (paths(&read(BRAND)), paths(&read("icons/icon.svg")));
    assert_eq!(brand.len(), 2, "the brand mark is a black body and green tips");
    assert_eq!(favicon, brand, "icons/icon.svg and {BRAND} draw different marks");
}

#[test]
fn the_small_mark_keeps_the_brand_inks() {
    let brand = paths(&read(BRAND));
    let small = paths(&read("icons/mark-small.svg"));
    let fills: Vec<&str> = small.iter().map(|(fill, _)| fill.as_str()).collect();
    assert_eq!(fills, ["currentColor", DROP], "body tinted by CSS, tips in the drop green");
    assert_eq!(brand[1].0, DROP, "the brand's tips moved off {DROP}");
    let (dark, mark_css) = (paths(&read(BRAND_DARK)), read("src/assets/css/mark.css"));
    assert_eq!(dark[1].0, DROP_DARK, "the dark page's tips moved off {DROP_DARK}");
    assert!(mark_css.contains(DROP_DARK), "the dark-ground drop is {DROP_DARK} in mark.css too");
    assert!(mark_css.contains(&format!("[fill=\"{DROP}\"]")), "mark.css retints the drop by its literal");
}

/// A page's theme can be the reader's stored choice against their OS setting,
/// so a mark drawn in the page must not repaint itself from the OS: the theme
/// stylesheet swaps two fixed files. (The favicon does follow the OS: it sits
/// on the browser's tab strip.)
#[test]
fn the_header_logo_follows_the_page_theme_not_the_os() {
    let (light, dark) = (read(BRAND), read(BRAND_DARK));
    for (name, svg) in [(BRAND, &light), (BRAND_DARK, &dark)] {
        assert!(!svg.contains("prefers-color-scheme"), "{name} repaints itself from the OS colour setting");
    }
    let shapes = |svg: &str| paths(svg).into_iter().map(|(_, d)| d).collect::<Vec<_>>();
    assert_eq!(shapes(&dark), shapes(&light), "{BRAND_DARK} and {BRAND} draw different marks");
    let theme = read("../../site/.moss/theme/style.css");
    assert!(theme.contains(r#"[data-theme="dark"] .site-logo { content: url("/assets/brand/logo-dark.svg"); }"#), "the site theme no longer swaps in {BRAND_DARK} on a dark page");
}
