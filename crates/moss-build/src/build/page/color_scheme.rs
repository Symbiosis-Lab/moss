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
}
