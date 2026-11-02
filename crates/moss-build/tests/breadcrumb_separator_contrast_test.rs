//! The breadcrumb's `/` is a visual delimiter hidden from assistive technology,
//! so WCAG 1.4.11 (non-text contrast, 3:1) is its bar, not 1.4.3. Its colour must
//! clear 3:1 on the page background in both themes. The test reads the token the
//! stylesheet actually uses, so swapping in a hairline token fails it.

const SITE_CSS: &str = include_str!("../src/assets/css/site.css");
const TOKENS: &str = include_str!("../../moss-core/src/contract/tokens.json");

fn channel(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn luminance(hex: &str) -> f64 {
    let h = hex.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
    0.2126 * channel(p(0)) + 0.7152 * channel(p(2)) + 0.0722 * channel(p(4))
}

fn ratio(a: &str, b: &str) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn token(name: &str, mode: &str) -> String {
    let json: serde_json::Value = serde_json::from_str(TOKENS).unwrap();
    fn find<'a>(v: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
        match v {
            serde_json::Value::Object(m) => m.get(name).or_else(|| m.values().find_map(|c| find(c, name))),
            _ => None,
        }
    }
    let entry = find(&json, name).unwrap_or_else(|| panic!("token {name} not in tokens.json"));
    entry["$value"][mode].as_str().unwrap().to_lowercase()
}

/// The colour token in the stylesheet's top-level `.breadcrumb-separator` rule.
fn separator_color_token() -> String {
    let start = SITE_CSS.find("\n.breadcrumb-separator {").expect("the separator rule");
    let block = &SITE_CSS[start..start + SITE_CSS[start..].find('}').unwrap()];
    let at = block.find("color: var(--").expect("the separator sets its colour from a token") + "color: var(--".len();
    block[at..at + block[at..].find(')').unwrap()].to_string()
}

#[test]
fn the_breadcrumb_separator_meets_non_text_contrast_in_both_themes() {
    let fg = separator_color_token();
    for mode in ["light", "dark"] {
        let r = ratio(&token(&fg, mode), &token("moss-color-bg", mode));
        assert!(r >= 3.0, "--{fg} on the page background in {mode} mode is {r:.2}:1, below 3:1");
    }
}

#[test]
fn the_ratio_matches_a_known_value() {
    assert!((ratio("#ffffff", "#000000") - 21.0).abs() < 0.01);
    // The hairline token the separator used before, on the dark page: 1.63:1.
    assert!((ratio("#443d32", "#1c1914") - 1.63).abs() < 0.01);
}
