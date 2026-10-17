//! `contributes.listing_styles` — a way of listing a folder's pages that a
//! plugin draws.
//!
//! An author picks one with `children_style: <id>` on a folder's home page,
//! beside moss's own `list`, `summary` and `grid`. The plugin's `listing`
//! export receives the pages moss's own listing of that folder would show and
//! returns the HTML to put in their place (see [`super::super::listing`]).
//!
//! A style id belongs to one plugin. When two installed plugins claim the
//! same id, the one whose name sorts first keeps it and the other's claim is
//! ignored with one warning naming both, so which plugin draws a page never
//! depends on discovery order.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::plugins::types::Plugin;

/// The ids moss draws itself. A plugin cannot claim them: `children_style:
/// list` must keep meaning the same thing on every install.
pub const BUILTIN_STYLE_IDS: [&str; 4] = ["list", "summary", "grid", "map"];

/// Whether `children_style` names one of moss's own styles.
pub fn is_builtin_style(id: &str) -> bool {
    BUILTIN_STYLE_IDS.contains(&id)
}

/// One style a plugin offers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListingStyleContribution {
    /// Name shown in the editor's select.
    #[serde(default)]
    pub label: Option<String>,
    /// One sentence for the select's hint.
    #[serde(default)]
    pub description: Option<String>,
    /// The page width this style wants (`wide` or `full`, as the frontmatter
    /// field of that name). Applies to a page that sets none of its own.
    #[serde(default)]
    pub content_width: Option<String>,
}

/// Refuse a declaration that could never work, at manifest parse time.
pub fn validate(id: &str, style: &ListingStyleContribution) -> Result<(), String> {
    let mut chars = id.chars();
    let well_formed = chars.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !well_formed {
        return Err(format!("listing style id `{id}` must be lowercase letters, digits, `-` or `_`"));
    }
    if is_builtin_style(id) {
        return Err(format!("listing style id `{id}` is one of moss's own styles"));
    }
    if let Some(width) = style.content_width.as_deref() {
        if !matches!(width, "wide" | "full") {
            return Err(format!("listing style `{id}`: content_width must be `wide` or `full`, not `{width}`"));
        }
    }
    Ok(())
}

/// Who draws a style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingStyleOwner {
    pub plugin: String,
    pub style: ListingStyleContribution,
}

/// Every listing style the installed plugins offer, decided once per build.
#[derive(Debug, Clone, Default)]
pub struct ListingStylePlan {
    owners: BTreeMap<String, ListingStyleOwner>,
    /// One sentence per refused duplicate claim.
    pub conflicts: Vec<String>,
}

impl ListingStylePlan {
    pub fn from_plugins(plugins: &[Plugin]) -> Self {
        let mut sorted: Vec<&Plugin> = plugins.iter().collect();
        sorted.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        let mut plan = Self::default();
        for plugin in sorted {
            let Some(styles) = plugin.manifest.contributes.as_ref().and_then(|c| c.listing_styles.as_ref()) else {
                continue;
            };
            for (id, style) in styles {
                match plan.owners.get(id) {
                    Some(first) => plan.conflicts.push(format!(
                        "listing style `{id}` is contributed by both `{}` and `{}`; `{}` keeps it and the claim of `{}` is ignored",
                        first.plugin, plugin.manifest.name, first.plugin, plugin.manifest.name
                    )),
                    None => {
                        plan.owners.insert(
                            id.clone(),
                            ListingStyleOwner { plugin: plugin.manifest.name.clone(), style: style.clone() },
                        );
                    }
                }
            }
        }
        plan
    }

    pub fn owner(&self, id: &str) -> Option<&ListingStyleOwner> {
        self.owners.get(id)
    }

    /// Every style id on offer, sorted.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.owners.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::types::PluginManifest;

    fn plugin(name: &str, ids: &[&str]) -> Plugin {
        let styles: BTreeMap<String, ListingStyleContribution> =
            ids.iter().map(|id| (id.to_string(), ListingStyleContribution::default())).collect();
        let mut manifest = PluginManifest { name: name.into(), ..Default::default() };
        manifest.contributes = Some(super::super::PluginContributes { listing_styles: Some(styles), ..Default::default() });
        Plugin { manifest, path: name.into() }
    }

    #[test]
    fn the_first_name_keeps_a_style_two_plugins_claim() {
        let plan = ListingStylePlan::from_plugins(&[plugin("zeta", &["board"]), plugin("alpha", &["board", "rail"])]);
        assert_eq!(plan.owner("board").unwrap().plugin, "alpha");
        assert_eq!(plan.owner("rail").unwrap().plugin, "alpha");
        assert_eq!(plan.conflicts.len(), 1);
        assert!(plan.conflicts[0].contains("alpha") && plan.conflicts[0].contains("zeta"));
    }

    #[test]
    fn builtin_and_malformed_ids_are_refused() {
        let ok = ListingStyleContribution::default();
        assert!(validate("board", &ok).is_ok());
        assert!(validate("list", &ok).is_err());
        assert!(validate("Board", &ok).is_err());
        assert!(validate("", &ok).is_err());
        let bad_width = ListingStyleContribution { content_width: Some("huge".into()), ..Default::default() };
        assert!(validate("board", &bad_width).is_err());
    }
}
