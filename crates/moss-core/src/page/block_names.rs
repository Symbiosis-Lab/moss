//! Which `:::name` fences the installed plugins own.
//!
//! A block's full name is `<plugin-id>--<short>`. The short name works only
//! while exactly one plugin declares it, so installing a plugin can never hand
//! existing content to a different owner: both owners stop placing it, the body
//! renders, and a diagnostic names the full names to write.
use std::collections::{BTreeMap, BTreeSet};

const SEPARATOR: &str = "--";

/// The name an author writes when a short name is shared.
pub fn qualified_name(plugin: &str, short: &str) -> String {
    format!("{plugin}{SEPARATOR}{short}")
}

fn is_fence_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A plugin id has to fit the fence grammar for its blocks to be nameable.
pub fn validate_plugin_id(plugin: &str) -> Result<(), String> {
    if is_fence_name(plugin) {
        return Ok(());
    }
    Err(format!("plugin `{plugin}` needs a name of letters, digits, `-` or `_`, because its declarations are named and served by it"))
}

/// Refuse a short name that could never be written, or that belongs to moss.
pub fn validate_short_name(short: &str) -> Result<(), String> {
    if !is_fence_name(short) || !short.starts_with(|c: char| c.is_ascii_alphanumeric()) || short.contains(SEPARATOR) {
        return Err(format!("block name `{short}` must be letters, digits, `-` or `_`, without `--`"));
    }
    if crate::ast::shortcode_extract::is_typed_known(short) {
        return Err(format!("block name `{short}` is one of moss's own shortcodes"));
    }
    Ok(())
}

/// What a written fence name refers to.
#[derive(Debug, PartialEq, Eq)]
pub enum BlockName<'a> {
    /// The full name of the one block it means.
    Block(&'a str),
    /// A short name more than one plugin declares: these full names, sorted.
    Ambiguous(&'a [String]),
    Unknown,
}

/// Every block name the installed plugins offer, decided once per build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockNames {
    qualified: BTreeSet<String>,
    by_short: BTreeMap<String, Vec<String>>,
}

impl BlockNames {
    pub fn from_contributions<'a>(contributions: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut names = Self::default();
        for (plugin, short) in contributions {
            let full = qualified_name(plugin, short);
            names.qualified.insert(full.clone());
            names.by_short.entry(short.to_string()).or_default().push(full);
        }
        names.by_short.values_mut().for_each(|owners| owners.sort());
        names
    }

    pub fn resolve(&self, written: &str) -> BlockName<'_> {
        if let Some(full) = self.qualified.get(written) {
            return BlockName::Block(full);
        }
        match self.by_short.get(written).map(Vec::as_slice) {
            Some([only]) => BlockName::Block(only),
            Some(owners) => BlockName::Ambiguous(owners),
            None => BlockName::Unknown,
        }
    }

    /// One sentence per short name that places nothing because two plugins declare it.
    pub fn conflicts(&self) -> Vec<String> {
        self.by_short.iter().filter(|(_, owners)| owners.len() > 1).map(|(short, owners)| {
            let full: Vec<String> = owners.iter().map(|o| format!("`:::{o}`")).collect();
            format!("block `:::{short}` is declared by more than one plugin, so it places nothing; write {} instead", full.join(" or "))
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_name_resolves_only_while_one_plugin_declares_it() {
        let one = BlockNames::from_contributions([("catalog", "card")]);
        assert_eq!(one.resolve("card"), BlockName::Block("catalog--card"));
        assert_eq!(one.resolve("catalog--card"), BlockName::Block("catalog--card"));
        assert_eq!(one.resolve("other"), BlockName::Unknown);
        assert!(one.conflicts().is_empty());

        let two = BlockNames::from_contributions([("zeta", "card"), ("catalog", "card")]);
        assert_eq!(two.resolve("card"), BlockName::Ambiguous(&["catalog--card".to_string(), "zeta--card".to_string()]));
        assert_eq!(two.resolve("zeta--card"), BlockName::Block("zeta--card"));
        let conflicts = two.conflicts();
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("`:::catalog--card` or `:::zeta--card`"), "{conflicts:?}");
    }

    #[test]
    fn builtin_and_unwritable_names_are_refused() {
        assert!(validate_short_name("card").is_ok());
        for bad in ["form", "gallery", "", "a--b", "-card", "a.b", "a b"] {
            assert!(validate_short_name(bad).is_err(), "{bad:?}");
        }
        assert!(validate_plugin_id("my.plugin").is_err());
        assert!(validate_plugin_id("my-plugin_2").is_ok());
    }
}
