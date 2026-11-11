//! Plugin listing styles shared by site bundles and builds.
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

/// The ids moss draws itself. A plugin cannot claim them: `children_style:
/// list` must keep meaning the same thing on every install.
pub const BUILTIN_STYLE_IDS: [&str; 7] = ["list", "summary", "lead", "grid", "stream", "map", "levels"];

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
    pub fn from_contributions<'a>(contributions: impl IntoIterator<Item = (&'a str, &'a BTreeMap<String, ListingStyleContribution>)>) -> Self {
        let mut sorted: Vec<_> = contributions.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let mut plan = Self::default();
        for (name, styles) in sorted {
            for (id, style) in styles {
                match plan.owners.get(id) {
                    Some(first) => plan.conflicts.push(format!(
                        "listing style `{id}` is contributed by both `{}` and `{}`; `{}` keeps it and the claim of `{}` is ignored",
                        first.plugin, name, first.plugin, name
                    )),
                    None => {
                        plan.owners.insert(
                            id.clone(),
                            ListingStyleOwner { plugin: name.to_string(), style: style.clone() },
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
