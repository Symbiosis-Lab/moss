//! Bold that stays bold in a font the theme hosts.
//!
//! moss asks for bold text at a medium weight (480). A browser matching a
//! weight from 400 to 500 looks for a face up to 500 first, then below, and
//! only then above. A family hosted as Regular and Bold therefore draws 480 in
//! Regular, and bold text looks like the text around it.
//!
//! When the family leading `--moss-font-body` is one the theme hosts with
//! `@font-face` and none of its faces lies from 401 to 500, the build sets
//! `--moss-font-weight-strong` to the family's lightest face above 500. The
//! rule is appended to the theme's stylesheet, so it takes part in the cascade
//! exactly as if the theme had written it last. A weight the theme declares
//! anywhere is left to the theme.
//!
//! Headings keep the weight their faces draw. A heading stands apart by its
//! size, and a theme drawn with a Regular heading was designed in that Regular.

use crate::build::theme_lint::{strip_comments, style_rules, Layered};
use crate::i18n::Language;

/// A weight token and the font token whose leading family draws it.
const WEIGHTS: [(&str, &str); 1] = [("--moss-font-weight-strong", "--moss-font-body")];

/// The range a medium weight is matched in before any heavier face is tried.
const MEDIUM: std::ops::RangeInclusive<u16> = 401..=500;

/// The rules to append to `theme_css`, or nothing. `imported` are the
/// stylesheets it brings in, where a theme may keep its `@font-face` rules.
pub fn rules(theme_css: &str, imported: &[String]) -> String {
    // An import stands before every rule of the sheet that brings it in, so
    // the theme reads as its imported sheets followed by its own rules.
    let theme_css = &imported.iter().map(String::as_str).chain([theme_css]).collect::<Vec<_>>().join("\n");
    let stripped = strip_comments(theme_css);
    let faces = hosted_faces(&stripped);
    if faces.is_empty() {
        return String::new();
    }
    // A page's language can change its font tokens, so each is worked out
    // alone and they share one `:root` rule when they agree.
    let per_language: Vec<(Language, String)> = Language::ALL
        .into_iter()
        .map(|lang| {
            let page = Layered::for_page(Some(theme_css), lang.as_bcp47_attr());
            let declarations: String = WEIGHTS
                .iter()
                .filter(|(weight, _)| !declares(&stripped, weight))
                .filter_map(|(weight, font)| Some(format!("{weight}:{};", derived(&page, weight, font, &faces)?)))
                .collect();
            (lang, declarations)
        })
        .collect();
    let Some((_, first)) = per_language.first() else { return String::new() };
    if per_language.iter().all(|(_, d)| d == first) {
        return if first.is_empty() { String::new() } else { format!("\n:root{{{first}}}\n") };
    }
    per_language
        .iter()
        .filter(|(_, d)| !d.is_empty())
        .map(|(lang, d)| format!("\nhtml:lang({}){{{d}}}\n", lang.as_bcp47_attr()))
        .collect()
}

/// The weight to give `weight_token` on `page`: the lightest hosted face above
/// 500 of the family leading `font_token`, when moss's weight is a medium and
/// that family has no medium face.
fn derived(page: &Layered, weight_token: &str, font_token: &str, faces: &[Face]) -> Option<u16> {
    let asked: u16 = page.values(weight_token).0?.trim().parse().ok()?;
    if !MEDIUM.contains(&asked) {
        return None;
    }
    let stack = page.values(font_token).0?;
    let lead = unquoted(stack.split(',').next()?);
    let family: Vec<&Face> = faces.iter().filter(|f| f.family.eq_ignore_ascii_case(&lead)).collect();
    if family.is_empty() || family.iter().any(|f| f.lightest <= *MEDIUM.end() && f.heaviest >= *MEDIUM.start()) {
        return None;
    }
    family.iter().map(|f| f.lightest).filter(|w| w > MEDIUM.end()).min()
}

/// One `@font-face` rule that loads a file: its family and the weights it covers.
struct Face {
    family: String,
    lightest: u16,
    heaviest: u16,
}

fn hosted_faces(stripped: &str) -> Vec<Face> {
    style_rules(stripped)
        .into_iter()
        .filter(|rule| rule.prelude.trim().eq_ignore_ascii_case("@font-face"))
        .filter_map(|rule| {
            let declarations = rule.declarations();
            let value = |name: &str| declarations.iter().rev().find(|(p, _)| p == name).map(|(_, v)| v.as_str());
            if !value("src")?.to_ascii_lowercase().contains("url(") {
                return None;
            }
            let (lightest, heaviest) = weight_range(value("font-weight").unwrap_or("400"))?;
            Some(Face { family: unquoted(value("font-family")?), lightest, heaviest })
        })
        .collect()
}

/// `400`, `bold` or a variable face's `300 700`.
fn weight_range(value: &str) -> Option<(u16, u16)> {
    let one = |w: &str| match w.to_ascii_lowercase().as_str() {
        "normal" => Some(400),
        "bold" => Some(700),
        number => number.parse::<u16>().ok(),
    };
    let mut parts = value.split_whitespace();
    let lightest = one(parts.next()?)?;
    let heaviest = parts.next().map_or(Some(lightest), one)?;
    Some((lightest.min(heaviest), lightest.max(heaviest)))
}

fn unquoted(name: &str) -> String {
    name.trim().trim_matches(['"', '\'']).trim().to_string()
}

fn declares(stripped: &str, token: &str) -> bool {
    crate::build::theme_lint::PROP_DECL.captures_iter(stripped).any(|c| &c[1] == token)
}

#[cfg(test)]
mod tests {
    fn rules(theme: &str) -> String {
        super::rules(theme, &[])
    }

    const TWO_WEIGHTS: &str = "@font-face { font-family: \"Hosted\"; font-weight: 400; src: url(\"a.woff2\"); }\
        @font-face { font-family: 'Hosted'; font-weight: 700; src: url(b.woff2) format(\"woff2\"); }";

    #[test]
    fn a_hosted_family_with_regular_and_bold_draws_bold_text_in_its_bold_and_headings_as_they_were() {
        let theme = format!("{TWO_WEIGHTS} :root {{ --moss-font-heading: Hosted, serif; --moss-font-body: var(--moss-font-heading); }}");
        assert_eq!(rules(&theme), "\n:root{--moss-font-weight-strong:700;}\n");
        let heading_only = format!("{TWO_WEIGHTS} :root {{ --moss-font-heading: Hosted, serif; }}");
        assert_eq!(rules(&heading_only), "", "a hosted heading family keeps the weight it draws");
    }

    #[test]
    fn only_the_token_whose_family_is_hosted_is_set_and_the_lightest_heavier_face_is_used() {
        let theme = "@font-face { font-family: Hosted; src: url(a.woff2); }\
            @font-face { font-family: Hosted; font-weight: 600; src: url(b.woff2); }\
            @font-face { font-family: Hosted; font-weight: bold; src: url(c.woff2); }\
            :root { --moss-font-body: \"Hosted\", sans-serif; }";
        assert_eq!(rules(theme), "\n:root{--moss-font-weight-strong:600;}\n");
    }

    #[test]
    fn a_weight_the_theme_sets_a_medium_face_and_a_family_that_is_not_hosted_are_left() {
        let own = format!("{TWO_WEIGHTS} :root {{ --moss-font-body: Hosted; --moss-font-weight-strong: 400; }}");
        assert_eq!(rules(&own), "");
        for medium in ["font-weight: 500", "font-weight: 300 700"] {
            let theme = format!("{TWO_WEIGHTS} @font-face {{ font-family: Hosted; {medium}; src: url(m.woff2); }} :root {{ --moss-font-body: Hosted; }}");
            assert_eq!(rules(&theme), "", "{medium}");
        }
        assert_eq!(rules(&format!("{TWO_WEIGHTS} :root {{ --moss-font-body: Georgia, Hosted; }}")), "", "not the leading family");
        assert_eq!(rules(":root { --moss-font-body: Georgia; }"), "", "no hosted face");
        let local = "@font-face { font-family: Hosted; src: local(\"Arial\"); } @font-face { font-family: Hosted; font-weight: 700; src: local(\"Arial Bold\"); } :root { --moss-font-body: Hosted; }";
        assert_eq!(rules(local), "", "an alias of installed faces loads no file");
    }

    #[test]
    fn faces_kept_in_an_imported_stylesheet_count() {
        let theme = "@import \"fonts.css\"; :root { --moss-font-body: Hosted; }";
        assert_eq!(super::rules(theme, &[TWO_WEIGHTS.to_string()]), "\n:root{--moss-font-weight-strong:700;}\n");
    }

    #[test]
    fn tokens_set_in_an_imported_stylesheet_count() {
        let fonts = format!("{TWO_WEIGHTS} :root {{ --moss-font-body: Hosted; }}");
        assert_eq!(super::rules("@import \"fonts.css\";", &[fonts.clone()]), "\n:root{--moss-font-weight-strong:700;}\n");
        let own = format!("{fonts} :root {{ --moss-font-weight-strong: 400; }}");
        assert_eq!(super::rules("@import \"fonts.css\";", &[own]), "", "a weight the imported sheet sets is the theme's");
    }

    #[test]
    fn a_family_hosted_for_one_language_sets_the_weight_on_that_languages_pages() {
        let theme = format!("{TWO_WEIGHTS} html:lang(de) {{ --moss-font-body: Hosted, sans-serif; }}");
        assert_eq!(rules(&theme), "\nhtml:lang(de){--moss-font-weight-strong:700;}\n");
    }
}
