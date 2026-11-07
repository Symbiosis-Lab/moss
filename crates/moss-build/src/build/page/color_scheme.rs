//! The site's colour policy, shared by page chrome and SPA metadata.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorScheme {
    #[default]
    Auto,
    Light,
    Dark,
}

impl ColorScheme {
    /// Like the header mode, an unknown value warns and keeps the default.
    pub fn from_config(value: Option<&str>) -> Self {
        match value {
            None | Some("") | Some("auto") => Self::Auto,
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            Some(other) => {
                crate::build::cli_output::log_warn_problem!(
                    "[site].color_scheme must be 'auto', 'light' or 'dark', not '{other}'; using 'auto'"
                );
                Self::Auto
            }
        }
    }

    pub fn fixed(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Light => Some("light"),
            Self::Dark => Some("dark"),
        }
    }

    pub fn html_attrs(self) -> String {
        self.fixed().map(|scheme| format!(
            r#" data-moss-color-scheme="{scheme}" data-theme="{scheme}" style="color-scheme: {scheme}""#
        )).unwrap_or_default()
    }

    pub fn theme_color_meta(self, light: Option<&str>, dark: Option<&str>) -> String {
        [("light", light), ("dark", dark)].into_iter()
            .filter(|(scheme, _)| self.fixed().is_none_or(|fixed| fixed == *scheme))
            .filter_map(|(scheme, color)| color.map(|color| {
                let media = if self == Self::Auto {
                    format!(r#" media="(prefers-color-scheme: {scheme})""#)
                } else { String::new() };
                format!(r#"<meta name="theme-color" content="{}"{media}>"#, moss_core::media::html_escape_attr(color))
            }))
            .collect::<Vec<_>>().join("\n")
    }
}

/// The colour a browser paints its own chrome in beside the page
/// (`<meta name="theme-color">`): the theme's `--moss-color-bg` at the page
/// root, moss's own where the theme sets none or the build can't read it. One
/// value for the whole site, so a section that re-colours its pages keeps it.
/// A dark paper set only inside `@media (prefers-color-scheme: dark)` is not
/// read: the dark tag keeps the root paper (the light-only lint warns there).
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeColor {
    pub light: String,
    pub dark: String,
}

impl ThemeColor {
    pub fn of_theme(theme_css: Option<&str>) -> Self {
        use crate::build::scan::section_text::{to_hex, BACKGROUNDS};
        let (light_bg, dark_bg) = *BACKGROUNDS;
        let (light, dark) = crate::build::theme_lint::root_colours(theme_css, "--moss-color-bg", (light_bg, dark_bg));
        Self { light: to_hex(light.root.unwrap_or(light_bg)), dark: to_hex(dark.root.unwrap_or(dark_bg)) }
    }

    pub fn meta(&self, scheme: ColorScheme) -> String {
        scheme.theme_color_meta(Some(&self.light), Some(&self.dark))
    }
}

impl Default for ThemeColor {
    fn default() -> Self {
        Self::of_theme(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_resolves_to_a_known_policy() {
        for value in [None, Some(""), Some("auto"), Some("sepia")] {
            assert_eq!(ColorScheme::from_config(value), ColorScheme::Auto);
        }
        assert_eq!(ColorScheme::from_config(Some("light")), ColorScheme::Light);
        assert_eq!(ColorScheme::from_config(Some("dark")), ColorScheme::Dark);
    }

    /// A theme that names its paper once and re-colours it in sections.
    const SECTIONED: &str = ":root { --day: #FBFBFB; --paper: var(--day); }\n\
        :root, body[data-section] { --moss-color-bg: var(--paper); }\n\
        body[data-section] { --paper: var(--moss-section-color); }";

    #[test]
    fn theme_color_takes_the_root_paper_through_var_and_a_selector_list() {
        let color = ThemeColor::of_theme(Some(SECTIONED));
        assert_eq!(color.light, "#fbfbfb");
        assert_eq!(color.dark, "#fbfbfb", "no dark rule: the root paper holds in both");
        assert_eq!(color.meta(ColorScheme::Light), r##"<meta name="theme-color" content="#fbfbfb">"##);
    }

    #[test]
    fn a_plain_root_colour_is_read_on_each_side() {
        let css = ":root { --moss-color-bg: #ffffff; }\n:root[data-theme=\"dark\"] { --moss-color-bg: #000000; }";
        let color = ThemeColor::of_theme(Some(css));
        assert_eq!((color.light.as_str(), color.dark.as_str()), ("#ffffff", "#000000"));
    }

    #[test]
    fn a_var_cycle_or_a_missing_property_falls_back_to_moss_paper() {
        let fallback = ThemeColor::default();
        for css in [
            ":root { --moss-color-bg: var(--a); --a: var(--b); --b: var(--a); }",
            ":root { --moss-color-bg: var(--nowhere); }",
        ] {
            assert_eq!(ThemeColor::of_theme(Some(css)), fallback, "{css}");
        }
        assert_eq!((fallback.light.as_str(), fallback.dark.as_str()), ("#faf8f5", "#1c1914"));
    }
}
