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

use crate::plugins::types::Plugin;

pub use moss_core::page::listing_style::{is_builtin_style, ListingStyleContribution, ListingStylePlan};

/// Every listing style the installed plugins offer, decided once per build.
pub fn listing_style_plan(plugins: &[Plugin]) -> ListingStylePlan {
    ListingStylePlan::from_contributions(plugins.iter().filter_map(|p| {
        Some((p.manifest.name.as_str(), p.manifest.contributes.as_ref()?.listing_styles.as_ref()?))
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
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
        let plan = listing_style_plan(&[plugin("zeta", &["board"]), plugin("alpha", &["board", "rail"])]);
        assert_eq!(plan.owner("board").unwrap().plugin, "alpha");
        assert_eq!(plan.owner("rail").unwrap().plugin, "alpha");
        assert_eq!(plan.conflicts.len(), 1);
        assert!(plan.conflicts[0].contains("alpha") && plan.conflicts[0].contains("zeta"));
    }

    #[test]
    fn a_manifest_with_an_unusable_style_is_refused() {
        let manifest = |styles: &str, entry: &str| PluginManifest::parse(&format!(
            r#"{{"name":"board","version":"0.1.0",{entry}"contributes":{{"listing_styles":{styles}}}}}"#
        ));
        assert!(manifest(r#"{"board":{}}"#, r#""entry":"main.js","#).is_ok());
        let builtin = manifest(r#"{"list":{}}"#, r#""entry":"main.js","#).unwrap_err().to_string();
        assert!(builtin.contains("one of moss's own styles"), "{builtin}");
        let entryless = manifest(r#"{"board":{}}"#, r#""entry":" ","#).unwrap_err().to_string();
        assert!(entryless.contains("exports `listing`"), "{entryless}");
    }
}
