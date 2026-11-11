//! The manifest contributions that a page can use without a plugin engine.
use std::collections::{BTreeMap, HashMap};
use serde::{Deserialize, Serialize};
use super::listing_style::{self, ListingStyleContribution};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributedFrontmatter {
    pub fields: HashMap<String, crate::schema::FieldDefinition>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PageContributions {
    pub frontmatter: Option<ContributedFrontmatter>,
    pub listing_styles: Option<BTreeMap<String, ListingStyleContribution>>,
    /// Only the names: what a block does is the build's concern.
    pub blocks: Option<BTreeMap<String, serde::de::IgnoredAny>>,
}

#[derive(Debug, Deserialize)]
pub struct PageManifest {
    pub name: Option<String>,
    pub entry: Option<String>,
    #[serde(default, deserialize_with = "contributions")]
    pub contributes: PageContributions,
}

/// Read and validate the page-facing portion of a manifest. Other contributions
/// remain the build's concern; an absent name uses the installed folder's name.
pub fn read_manifest(json: &str) -> serde_json::Result<PageManifest> {
    use serde::de::Error as _;
    let mut manifest: PageManifest = serde_json::from_str(json)?;
    manifest.entry = validate_page_contributions(manifest.entry.take(), manifest.contributes.listing_styles.as_ref())
        .map_err(serde_json::Error::custom)?;
    for short in manifest.contributes.blocks.iter().flatten().map(|(short, _)| short) {
        super::block_names::validate_short_name(short).map_err(serde_json::Error::custom)?;
    }
    Ok(manifest)
}

/// The checks every manifest reader applies to what pages use: a blank `entry`
/// is none, and each listing style needs a well-formed id of its own and an
/// entry that draws it. Returns the entry.
pub fn validate_page_contributions(entry: Option<String>,
    listing_styles: Option<&BTreeMap<String, ListingStyleContribution>>) -> Result<Option<String>, String> {
    let entry = entry.filter(|e| !e.trim().is_empty());
    if let Some(styles) = listing_styles {
        for (id, style) in styles {
            listing_style::validate(id, style)?;
        }
        if entry.is_none() {
            return Err("a plugin that contributes listing styles needs an `entry` that exports `listing`".into());
        }
    }
    Ok(entry)
}

fn contributions<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<PageContributions, D::Error> {
    Ok(Option::<PageContributions>::deserialize(deserializer)?.unwrap_or_default())
}
