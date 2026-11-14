//! Style facts as overrides of moss's own design tokens. Pure. A token is
//! written only where the source differs from moss's default, read from the
//! token contract (and `--moss-link-color`, the one escape hatch it uses); a
//! fact no token expresses becomes a residual line for the import report,
//! never a selector rule.

use moss_core::contract::tokens::{find_token, Tokens};

use crate::build::theme_lint::{plain_px, ROOT_PX};

use super::style::{Colours, Family, StyleFacts};
use crate::build::components::color_extract::{colour, Rgba};

/// The pages moss gives its Han-first font stacks, as its own stylesheet selects them.
const HAN_PAGES: &str = "html:is([lang|=\"zh\"], [lang|=\"ja\"], [lang|=\"ko\"])";

/// A content width at most this many times the reading size is a reading
/// measure; anything wider is the page's frame, which moss's column is not.
const READING_MEASURE: f32 = 60.0;

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Overrides {
    /// `:root` declarations.
    pub light: Vec<(String, String)>,
    /// `:root[data-theme="dark"]` declarations.
    pub dark: Vec<(String, String)>,
    /// Declarations for a Chinese, Japanese or Korean page ([`HAN_PAGES`]).
    pub han: Vec<(String, String)>,
    /// The source's own families a font token names, for the font copy.
    pub families: Vec<String>,
    pub residuals: Vec<String>,
}

pub(crate) fn map(facts: &StyleFacts, tokens: &Tokens) -> Overrides {
    let default = |name: &str| {
        let (light, dark) = find_token(tokens, name).unwrap_or_else(|| panic!("{name} is a moss token"));
        (light.to_string(), dark.map(str::to_string))
    };
    let mut out = Overrides::default();

    for (token, family) in [("moss-font-body", &facts.body_family), ("moss-font-heading", &facts.heading_family), ("moss-font-mono", &facts.mono_family)] {
        let Some(Family { name, generic }) = family else { continue };
        // The stack's own kind decides the fallbacks: a sans heading keeps sans ones.
        let (kind, lead) = match generic_token(name) {
            Some(class) => (class, String::new()),
            None if !is_plain_name(name) => {
                out.residuals.push(format!("font family {name:?}: a name moss does not write into a stylesheet"));
                continue;
            }
            None => (generic.as_deref().and_then(generic_token).unwrap_or(token), format!("'{name}', ")),
        };
        let stack = format!("{lead}{}", default(kind).0);
        if stack == default(token).0 {
            continue;
        }
        if !lead.is_empty() {
            if !out.families.contains(name) {
                out.families.push(name.clone());
            }
        }
        out.light.push((format!("--{token}"), stack));
        // moss swaps the body and heading stacks to Han-first ones on a Chinese,
        // Japanese or Korean page (Traditional faces on zh-Hant); a :root value
        // would override that swap, so those pages get the family in front of it.
        if token != "moss-font-mono" {
            let han = if kind == "moss-font-heading" { "var(--moss-font-heading-han)" } else { "var(--moss-font-body-han)" };
            out.han.push((format!("--{token}"), format!("{lead}{han}")));
        }
    }

    if let Some(size) = facts.base_size_px.filter(|s| Some(*s) != plain_px(&default("moss-reading-size-base").0)) {
        out.light.push(("--moss-reading-size-base".to_string(), format!("{}rem", number(size / ROOT_PX))));
    }
    if let Some(lh) = facts.line_height.filter(|l| Some(*l) != default("moss-read-leading").0.parse().ok()) {
        out.light.push(("--moss-read-leading".to_string(), number(lh)));
    }
    if let Some(w) = facts.heading_weight.filter(|w| Some(*w) != default("moss-font-heading-weight").0.parse().ok()) {
        out.light.push(("--moss-font-heading-weight".to_string(), w.to_string()));
    }
    // The measure is written as moss writes its own, a multiple of the reading
    // size, so it widens with the reader's text-size choice.
    let reading_px = facts.base_size_px.or_else(|| plain_px(&default("moss-reading-size-base").0));
    let moss_measure = default("moss-content-width").0.trim_start_matches("calc(").split('*').next().and_then(|n| n.trim().parse::<f32>().ok());
    if let (Some(width), Some(reading_px), Some(moss_measure)) = (facts.content_width_px, reading_px, moss_measure) {
        let measure = width / reading_px;
        if measure > READING_MEASURE {
            out.residuals.push(format!("page width {}px: wider than a reading column, so moss keeps its own measure", number(width)));
        } else if (measure - moss_measure).abs() >= 0.5 {
            out.light.push(("--moss-content-width".to_string(), format!("calc({} * var(--moss-reading-size))", number((measure * 10.0).round() / 10.0))));
        }
    }

    let accent = accent_of(&facts.light);
    let slots = |c: &Colours, accent: Option<Rgba>| [("moss-color-text", c.text), ("moss-color-bg", c.background), ("moss-color-accent", accent), ("moss-color-muted", c.muted)];
    let dark = facts.dark.as_ref().map(|d| slots(d, accent_of(d)));
    for (i, (token, value)) in slots(&facts.light, accent).into_iter().enumerate() {
        let (light_default, dark_default) = default(token);
        let light_default = colour(&light_default);
        let dark_default = dark_default.and_then(|d| colour(&d)).or(light_default);
        let light_differs = value.is_some_and(|v| Some(v) != light_default);
        if let Some(v) = value.filter(|_| light_differs) {
            out.light.push((format!("--{token}"), v.to_string()));
        }
        let Some(dark) = &dark else { continue };
        // A light value written needs its dark one, or dark mode would keep the light one.
        let Some(dark_value) = dark[i].1.or(dark_default) else { continue };
        if light_differs || Some(dark_value) != dark_default {
            out.dark.push((format!("--{token}"), dark_value.to_string()));
        }
    }
    // moss's links in running text rest in the text colour unless this says otherwise.
    let link_colour = |c: &Colours, accent: Option<&Rgba>| match &c.link {
        Some(link) if c.text.as_ref() != Some(link) && accent == Some(link) => Some("var(--moss-color-accent)".to_string()),
        Some(link) if c.text.as_ref() != Some(link) => Some(link.to_string()),
        _ => None,
    };
    let light_link = link_colour(&facts.light, accent.as_ref());
    let dark_link = facts.dark.as_ref().and_then(|d| link_colour(d, accent_of(d).as_ref()));
    if let Some(link) = &light_link {
        out.light.push(("--moss-link-color".to_string(), link.clone()));
    }
    if let Some(link) = dark_link.filter(|d| Some(d) != light_link.as_ref()) {
        out.dark.push(("--moss-link-color".to_string(), link));
    }
    out
}

/// The accent: the colour the site names its accent, else its links' colour
/// when that is not the text's.
fn accent_of(c: &Colours) -> Option<Rgba> {
    c.accent.or_else(|| c.link.filter(|l| c.text.as_ref() != Some(l)))
}

/// The moss token whose default stack already is this generic family's.
fn generic_token(family: &str) -> Option<&'static str> {
    match family.to_ascii_lowercase().as_str() {
        "-apple-system" | "blinkmacsystemfont" | "system-ui" | "ui-sans-serif" | "sans-serif" => Some("moss-font-body"),
        "serif" | "ui-serif" => Some("moss-font-heading"),
        "monospace" | "ui-monospace" => Some("moss-font-mono"),
        _ => None,
    }
}

/// A family name safe to write between quotes in a stylesheet.
pub(crate) fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
}

/// A number with no trailing zeros: `1.6`, `1`, `0.875`.
fn number(n: f32) -> String {
    let s = format!("{n:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl Overrides {
    pub(crate) fn is_empty(&self) -> bool {
        self.light.is_empty() && self.dark.is_empty()
    }

    /// The `:root` block, the Han pages' font block and the dark one, each when it has declarations.
    pub(crate) fn css(&self) -> String {
        let block = |selector: &str, decls: &[(String, String)]| {
            let body: String = decls.iter().map(|(t, v)| format!("  {t}: {v};\n")).collect();
            format!("{selector} {{\n{body}}}\n")
        };
        let mut css = String::new();
        if !self.light.is_empty() {
            css.push_str(&block(":root", &self.light));
        }
        if !self.han.is_empty() {
            css.push_str(&block(HAN_PAGES, &self.han));
        }
        if !self.dark.is_empty() {
            css.push_str(&block(":root[data-theme=\"dark\"]", &self.dark));
        }
        css
    }
}

#[cfg(test)]
#[path = "mapping_tests.rs"]
mod tests;
