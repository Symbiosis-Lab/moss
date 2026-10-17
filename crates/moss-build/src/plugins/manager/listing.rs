//! Running a plugin's `listing` export, and what crosses the boundary: the
//! context moss hands it and the result it returns.
//!
//! The context is moss's resolved model of the pages, so a plugin never
//! re-parses markdown, re-derives an address or guesses a language.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{HookRun, PluginManager};

impl PluginManager {
    /// Call `listing(ctx)` on one plugin and read what it returned.
    ///
    /// Not singleflighted: the singleflight key is `plugin::hook`, so two
    /// pages' calls would share the first one's answer. The caller runs them
    /// one after another.
    pub async fn execute_listing(&self, plugin_name: &str, ctx: &ListingContext) -> Result<ListingResult, String> {
        let plugin = self
            .find_plugin_by_name(plugin_name)
            .ok_or_else(|| format!("plugin `{plugin_name}` is not loaded"))?;
        let context_json = serde_json::to_string(ctx).map_err(|e| format!("could not serialize the listing context: {e}"))?;
        match self.run_hook(&plugin, "listing", &context_json, &self.host.state.hooks).await {
            HookRun::NeverRan(e) => Err(e),
            HookRun::Finished(outcome) => {
                let (_, returned) = outcome?;
                let value = returned.ok_or("`listing` returned nothing")?;
                serde_json::from_value(value).map_err(|e| format!("`listing` returned a malformed result: {e}"))
            }
        }
    }
}


/// Version of the context's shape. Fields may be added within a version;
/// renaming or removing one bumps it.
pub const LISTING_MODEL: u32 = 1;

/// One page, as the plugin sees it. The listing page itself, every page it
/// lists and every subfolder's home page all have this shape.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PageRecord {
    pub title: String,
    /// The page's address in the built site, as moss names it (`board/index.html`).
    pub url_path: String,
    /// Where the page is, written so it resolves from the listing page.
    pub url: String,
    pub lang: String,
    pub translations: Vec<TranslationRecord>,
    /// The page's frontmatter as moss resolved it: values inherited from a
    /// parent folder are applied and `title` is filled in when the page set none.
    pub frontmatter: BTreeMap<String, serde_json::Value>,
    /// The page's declared fields (`[fields.*]` in `.moss/config.toml`) that
    /// its own frontmatter carries and that parse, each as its typed value. A
    /// date is `{value, precision, qualifier}`; a period is `{start, end, open, text}`
    /// with `start` and `end` in that date shape; a choice is `{value, label}`
    /// with the label in this page's language; a count is `{number, noun, text}`.
    /// A value that does not parse is left out here and stays in `frontmatter`.
    /// Values a folder's `cascade:` supplies are not typed here. Empty when none applies.
    pub fields: BTreeMap<String, serde_json::Value>,
    /// For each declared field whose value does not fit, the reason it does
    /// not, as the build warning states it. Empty when every value fits.
    #[serde(default)]
    pub field_errors: BTreeMap<String, String>,
    /// The rendered body. Relative links and image sources are rewritten to
    /// resolve from the listing page, where the HTML is drawn. Empty for a
    /// subfolder that has no home page.
    pub html: String,
    /// The folder the page sits in, relative to the listed folder: empty for
    /// the listing page and for its direct children.
    pub folder: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslationRecord {
    pub lang: String,
    pub url_path: String,
}

/// The argument of the plugin's `listing(ctx)` export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingContext {
    /// [`LISTING_MODEL`].
    pub model: u32,
    /// The style id the page asked for.
    pub style: String,
    pub page: PageRecord,
    /// Exactly the pages moss's own listing of the folder would show, in the
    /// order it shows them.
    pub pages: Vec<PageRecord>,
    /// The listed folder's subfolders that have a home page, in the listing's
    /// own order (the folder's sort applied to the home pages).
    pub folders: Vec<PageRecord>,
    /// `terms[namespace][name]` is where that term's page is, written like a
    /// record's `url` so it resolves from the listing page, for every name
    /// used by `pages`.
    pub terms: BTreeMap<String, BTreeMap<String, String>>,
    /// Every declared field, by name, so a plugin can draw a field's choices
    /// or place without reading `.moss/config.toml`. Choice labels are in the
    /// listing page's language.
    #[serde(default)]
    pub declarations: BTreeMap<String, FieldDeclarationRecord>,
}

/// One `[fields.<name>]` declaration as a plugin sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDeclarationRecord {
    /// The declared type, as the config names it (`choice`, `period`, ...).
    #[serde(rename = "type")]
    pub kind: String,
    /// Only for a choice: the allowed values in declared order.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub choices: Option<Vec<ChoiceRecord>>,
    /// Where the field is placed on the page, or `None` for no place.
    pub place: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceRecord {
    pub value: String,
    pub label: String,
}

/// One tag for the page's `<head>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeadEntry {
    Stylesheet(String),
    Script(String),
}

/// A file the plugin wants written beside the listing page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListingFile {
    pub path: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub base64: Option<String>,
}

/// A warning the plugin raises: plain text, or text about one page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ListingWarning {
    Plain(String),
    /// `page` is a record's `url_path`; the build warning names that page's source file.
    About { text: String, page: Option<String> },
}

/// What the plugin's `listing` export returns.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListingResult {
    pub html: String,
    #[serde(default)]
    pub head: Vec<HeadEntry>,
    #[serde(default)]
    pub files: Vec<ListingFile>,
    #[serde(default)]
    pub warnings: Vec<ListingWarning>,
    /// `wide` or `full`; overrides the style's declared width for this page.
    #[serde(default)]
    pub content_width: Option<String>,
}
