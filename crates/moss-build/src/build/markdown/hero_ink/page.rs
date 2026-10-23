//! What the page around a closing picture is: its colours as the theme sets
//! them, and its colophon row.

use super::{TypeTheme, CREDIT_INK};
use crate::build::scan::section_text::{Rgb, BACKGROUNDS};

/// What the page around a closing picture is: the page colour behind a faded
/// picture, `--moss-color-bg` in the light and the dark theme (`None` where the
/// theme sets it to a value the build can't read), the secondary text colour the
/// credit can take, and its colophon row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGround {
    pub light: Option<Rgb>,
    pub dark: Option<Rgb>,
    /// The theme's secondary text colour in each, which the credit can take (`None` where it cannot be read).
    pub credit_light: Option<Rgb>,
    pub credit_dark: Option<Rgb>,
    /// Whether the page sets a colophon row along a closing picture's foot,
    /// which the credit then sits above.
    pub colophon: bool,
    /// What the theme changes in the type the words are set in.
    pub type_theme: TypeTheme,
}

impl Default for PageGround {
    fn default() -> Self {
        let (light, dark) = *BACKGROUNDS;
        Self { light: Some(light), dark: Some(dark), credit_light: Some(CREDIT_INK.0), credit_dark: Some(CREDIT_INK.1), colophon: true, type_theme: TypeTheme::default() }
    }
}

impl PageGround {
    /// The same page with its colophon row set.
    pub fn with_colophon(self, colophon: bool) -> Self {
        Self { colophon, ..self }
    }

    /// moss's colours as the theme's stylesheet overrides them. Read only
    /// where the authoring guide tells a theme to set a token: `:root {}`,
    /// which wins in both themes, and `:root[data-theme="dark"] {}`. A
    /// declaration anywhere else, or a value that is not a plain colour
    /// (`var()`, `color-mix()`), leaves both themes unresolved.
    pub fn of_theme(theme_css: Option<&str>) -> Self {
        let (light, dark) = theme_colours(theme_css, "--moss-color-bg", *BACKGROUNDS);
        let (credit_light, credit_dark) = theme_colours(theme_css, "--moss-color-text-secondary", *CREDIT_INK);
        Self { light, dark, credit_light, credit_dark, type_theme: TypeTheme::of_theme(theme_css), ..Self::default() }
    }
}

/// A colour token's value in the light and the dark theme: moss's own where the
/// theme does not set it, and `None` in both where it sets it somewhere the
/// build cannot follow or to a value it cannot read.
fn theme_colours(theme_css: Option<&str>, token: &str, default: (Rgb, Rgb)) -> (Option<Rgb>, Option<Rgb>) {
    let (mut root, mut dark) = (None, None);
    for rule in crate::build::theme_lint::style_rules(theme_css.unwrap_or_default()) {
        let Some((_, body)) = rule.rule.split_once('{') else { continue };
        for decl in body.trim_end_matches('}').split(';') {
            let Some((prop, value)) = decl.split_once(':') else { continue };
            if prop.trim() != token {
                continue;
            }
            let colour = crate::build::components::color_extract::parse_css_color(value.trim().trim_end_matches("!important").trim());
            let scope: String = rule.prelude.chars().filter(|c| !c.is_whitespace()).collect::<String>().replace('\'', "\"");
            match (rule.conditions.is_empty(), scope.as_str()) {
                (true, ":root") => root = Some(colour),
                (true, ":root[data-theme=\"dark\"]") => dark = Some(colour),
                _ => return (None, None),
            }
        }
    }
    let (mut light_theme, mut dark_theme) = (Some(default.0), Some(default.1));
    if let Some(colour) = root {
        (light_theme, dark_theme) = (colour, colour);
    }
    if let Some(colour) = dark {
        dark_theme = colour;
    }
    (light_theme, dark_theme)
}
