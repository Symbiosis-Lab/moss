//! What a plugin's manifest says it can do, in terms a person can be asked
//! about.
//!
//! Read only from fields the manifest declares and moss acts on, never from
//! prose. moss does not gate network or file access, so no ability here says
//! a plugin lacks either.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::plugins::types::{Capability, PluginManifest, EXECUTE_BINARY_GRANT};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ability {
    /// Draws a folder's pages in a style it contributes.
    DrawsPages { style: String, label: String, description: Option<String> },
    /// Sends the site somewhere: `hook` is `deploy` or `syndicate`.
    Publishes { hook: String },
    /// Opens a sign-in as soon as it is allowed.
    SignsIn { domain: Option<String> },
    /// Runs a program on this computer; `*` is the deprecated blanket grant
    /// for every program.
    RunsProgram { program: String },
}

impl Ability {
    /// Whether two abilities are the same thing being allowed, whatever
    /// wording the plugin gives it: a reworded label is not a new ability.
    pub fn same_as(&self, other: &Ability) -> bool {
        self.identity() == other.identity()
    }

    fn identity(&self) -> (&'static str, &str) {
        match self {
            Ability::DrawsPages { style, .. } => ("draws_pages", style),
            Ability::Publishes { hook } => ("publishes", hook),
            Ability::SignsIn { domain } => ("signs_in", domain.as_deref().unwrap_or_default()),
            Ability::RunsProgram { program } => ("runs_program", program),
        }
    }
}

/// The abilities `manifest` declares, in a fixed order: listing styles by id,
/// then deploy, syndicate, sign-in, then programs in `requires` order.
pub fn abilities(manifest: &PluginManifest) -> Vec<Ability> {
    let mut found = Vec::new();
    let styles = manifest.contributes.as_ref().and_then(|c| c.listing_styles.as_ref());
    for (id, style) in styles.into_iter().flatten() {
        found.push(Ability::DrawsPages {
            style: id.clone(),
            label: style.label.clone().unwrap_or_else(|| id.clone()),
            description: style.description.clone(),
        });
    }
    for (capability, hook) in [(Capability::Deploy, "deploy"), (Capability::Syndicate, "syndicate")] {
        if manifest.has_capability(&capability) {
            found.push(Ability::Publishes { hook: hook.to_string() });
        }
    }
    if manifest.has_capability(&Capability::Login) {
        found.push(Ability::SignsIn { domain: manifest.domain.clone() });
    }
    let named_prefix = format!("{EXECUTE_BINARY_GRANT}:");
    for entry in manifest.requires.iter().flatten() {
        let program = match entry.strip_prefix(&named_prefix) {
            Some(name) if !name.is_empty() => name,
            None if entry == EXECUTE_BINARY_GRANT => "*",
            // Grants nothing, so it is not an ability.
            _ => continue,
        };
        let ability = Ability::RunsProgram { program: program.to_string() };
        if !found.contains(&ability) {
            found.push(ability);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> PluginManifest {
        PluginManifest::parse(json).unwrap()
    }

    #[test]
    fn a_listing_style_alone_draws_pages_and_nothing_else() {
        let manifest = parse(
            r#"{"name":"board","version":"0.2.0","entry":"main.js","contributes":{"listing_styles":{
                "board":{"label":"Project board","description":"One card per page","content_width":"full"}}}}"#,
        );
        assert_eq!(
            abilities(&manifest),
            vec![Ability::DrawsPages {
                style: "board".into(),
                label: "Project board".into(),
                description: Some("One card per page".into()),
            }]
        );
    }

    #[test]
    fn each_declared_hook_and_grant_is_one_ability_in_a_fixed_order() {
        let manifest = parse(
            r#"{"name":"all","version":"1.0.0","entry":"main.js","domain":"example.test",
                "capabilities":["login","syndicate","deploy","process"],
                "requires":["execute_binary:ffmpeg","bogus","execute_binary","execute_binary:ffmpeg"]}"#,
        );
        assert_eq!(
            abilities(&manifest),
            vec![
                Ability::Publishes { hook: "deploy".into() },
                Ability::Publishes { hook: "syndicate".into() },
                Ability::SignsIn { domain: Some("example.test".into()) },
                Ability::RunsProgram { program: "ffmpeg".into() },
                Ability::RunsProgram { program: "*".into() },
            ]
        );
    }

    #[test]
    fn an_ability_serializes_with_a_snake_case_tag() {
        let json = serde_json::to_value(Ability::SignsIn { domain: None }).unwrap();
        assert_eq!(json, serde_json::json!({"kind": "signs_in", "domain": null}));
    }
}
