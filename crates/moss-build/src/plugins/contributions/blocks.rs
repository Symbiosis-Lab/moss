//! `contributes.blocks`: a custom element a plugin declares and an author
//! places with a `:::<short>` fence around ordinary markdown.
//!
//! Declarative on purpose. moss emits the element and renders the fence's body
//! as its children, so no plugin code runs at build and pages stay incremental.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use moss_core::ast::Document;
use moss_core::page::block_names::{qualified_name, BlockNames};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::build::context::BuildContext;
use crate::build::manifest::{HashBucket, PendingManifest};
use crate::build::served_path::{served_address, ServedPath};
use crate::build::types::ParsedDocument;
use crate::plugins::types::{Plugin, PluginManifest};

/// One block a plugin offers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockContribution {
    /// The custom element's tag.
    pub element: String,
    /// Loaded once, only on pages that use the block. A path in the plugin's folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Loaded the same way as `script`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stylesheet: Option<String>,
    /// The attributes a fence may set. Anything else is dropped with a warning.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<BlockAttribute>,
}

/// One attribute a fence may set, and what placement checks its value
/// against. There is no default: only what the fence wrote is emitted, since
/// a manifest default would outrank the site's public data, which the
/// element reads next.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockAttribute {
    pub key: String,
    #[serde(rename = "type", default)]
    pub kind: AttributeKind,
    /// A fence that leaves it out is reported.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    /// The only values it takes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// A regular expression the value must contain a match of; anchor it
    /// with `^…$` to hold the whole value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// What the warning says when `pattern` does not match; optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_message: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttributeKind {
    #[default]
    String,
    Number,
    Boolean,
}

/// The HTML spec reserves these hyphenated names for SVG and MathML.
const RESERVED_ELEMENTS: [&str; 8] = [
    "annotation-xml", "color-profile", "font-face", "font-face-src", "font-face-uri", "font-face-format",
    "font-face-name", "missing-glyph",
];

fn valid_element(tag: &str) -> bool {
    tag.starts_with(|c: char| c.is_ascii_lowercase())
        && tag.contains('-')
        && tag.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | '_'))
        && !RESERVED_ELEMENTS.contains(&tag)
}

/// Escaping protects a value, not a name, so the name is held to a plain
/// grammar and event handlers are refused outright.
fn valid_attribute(key: &str) -> bool {
    key.starts_with(|c: char| c.is_ascii_lowercase())
        && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
        && !key.starts_with("on")
}

fn inside_plugin(path: &str) -> bool {
    !path.is_empty()
        && path.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
        && Path::new(path).components().all(|c| matches!(c, Component::Normal(_)))
}

/// Refuse a declaration that could never be placed safely, when the manifest is read.
pub fn validate(plugin: &str, short: &str, block: &BlockContribution) -> Result<(), String> {
    moss_core::page::block_names::validate_plugin_id(plugin)?;
    moss_core::page::block_names::validate_short_name(short)?;
    if !valid_element(&block.element) {
        return Err(format!("block `{short}`: `{}` is not a valid custom element name", block.element));
    }
    let assets = [(&block.script, &[".js", ".mjs"][..]), (&block.stylesheet, &[".css"][..])];
    for (path, extensions) in assets.into_iter().filter_map(|(path, ext)| Some((path.as_ref()?, ext))) {
        if !inside_plugin(path) {
            return Err(format!("block `{short}`: `{path}` must be a path inside the plugin's folder"));
        }
        if !extensions.iter().any(|e| path.ends_with(e)) {
            return Err(format!("block `{short}`: `{path}` must end in {}", extensions.join(" or ")));
        }
    }
    for (index, attribute) in block.attributes.iter().enumerate() {
        let key = &attribute.key;
        if !valid_attribute(key) {
            return Err(format!("block `{short}`: `{key}` is not an attribute a block may set"));
        }
        if block.attributes[..index].iter().any(|earlier| earlier.key == *key) {
            return Err(format!("block `{short}`: attribute `{key}` is declared twice"));
        }
        if let Some(Err(e)) = attribute.pattern.as_deref().map(regex::Regex::new) {
            return Err(format!("block `{short}`: the pattern of `{key}` does not compile: {e}"));
        }
        if !attribute.options.is_empty() && attribute.kind != AttributeKind::String {
            return Err(format!("block `{short}`: `options` belong to a `string` attribute, not `{key}`"));
        }
    }
    Ok(())
}

/// A file a page loads for a block, kept by kind so nothing guesses it from an
/// extension. Stylesheets sort first, so an element is styled before its
/// script upgrades it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlockAsset {
    Stylesheet(String),
    Script(String),
}

/// Every script and stylesheet the manifest's blocks declare, each once.
pub fn asset_paths(manifest: &PluginManifest) -> Vec<&str> {
    let declared = manifest.contributes.iter().flat_map(|c| c.blocks.iter().flatten()).map(|(_, b)| b);
    let paths: BTreeSet<&str> = declared.flat_map(|b| [&b.script, &b.stylesheet]).flatten().map(String::as_str).collect();
    paths.into_iter().collect()
}

/// One block a build can place.
#[derive(Debug)]
pub struct PlacedBlock {
    pub plugin: String,
    pub dir: PathBuf,
    pub contribution: BlockContribution,
    /// The assets that exist inside the plugin, as each page loads them.
    pub assets: Vec<(String, BlockAsset)>,
}

/// Every block the enabled plugins declare, decided once per build.
#[derive(Debug, Default)]
pub struct BlockPlan {
    pub names: BlockNames,
    blocks: BTreeMap<String, PlacedBlock>,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// An asset's digest, or why it cannot be served. Symlinks are resolved first:
/// the manifest check is lexical and cannot see one pointing out of the plugin.
fn asset_digest(dir: &Path, rel: &str) -> Result<String, String> {
    let root = dir.canonicalize().map_err(|e| e.to_string())?;
    let file = dir.join(rel).canonicalize().map_err(|e| e.to_string())?;
    if !file.starts_with(&root) {
        return Err("it leads outside the plugin's folder".to_string());
    }
    crate::build::cloud_readiness::read_with_materialize_wait(&file, crate::build::cloud_readiness::INTERACTIVE_DEADLINE).map(|bytes| sha256(&bytes)).map_err(|e| e.to_string())
}

impl BlockPlan {
    pub fn from_plugins(plugins: &[Plugin]) -> Self {
        let declared: Vec<(&Plugin, &String, &BlockContribution)> = plugins.iter().flat_map(|p| {
            p.manifest.contributes.iter().flat_map(|c| c.blocks.iter().flatten()).map(move |(short, block)| (p, short, block))
        }).collect();
        let names = BlockNames::from_contributions(declared.iter().map(|(p, short, _)| (p.manifest.name.as_str(), short.as_str())));
        for conflict in names.conflicts() {
            crate::build::cli_output::log_warn_problem!(target: "plugin", "{conflict}");
        }
        let mut blocks = BTreeMap::new();
        for (plugin, short, block) in declared {
            let id = plugin.manifest.name.clone();
            let served = |rel: &str, digest: &str| format!("{}?v={}", served_address(ServedPath::for_plugin_asset(&id, rel).as_str()), digest.get(..8).unwrap_or(digest));
            let mut assets = Vec::new();
            let declared = [(&block.stylesheet, BlockAsset::Stylesheet as fn(String) -> BlockAsset), (&block.script, BlockAsset::Script)];
            for (rel, kind) in declared.into_iter().filter_map(|(rel, kind)| Some((rel.as_ref()?, kind))) {
                match asset_digest(&plugin.path, rel) {
                    Ok(digest) => assets.push((rel.clone(), kind(served(rel, &digest)))),
                    Err(why) => crate::build::cli_output::log_warn_problem!(target: "plugin", "block `{short}` of `{id}` loads `{rel}`, which cannot be served: {why}"),
                }
            }
            blocks.insert(qualified_name(&id, short), PlacedBlock { plugin: id, dir: plugin.path.clone(), contribution: block.clone(), assets });
        }
        Self { names, blocks }
    }

    /// Fill in each block's element and keep the attributes it declares,
    /// reporting the rest by the fence's line. Returns what the page loads and
    /// those problems, so the caller can report them from the page as authored.
    pub fn place(&self, doc: &mut Document) -> (BTreeSet<BlockAsset>, Vec<String>) {
        let mut used = BTreeSet::new();
        let mut all_problems = Vec::new();
        moss_core::ast::visit_plugin_blocks_mut(doc, |shortcode| {
            let Some(block) = self.blocks.get(&shortcode.name) else { return };
            let (kept, problems) = checked_attributes(&shortcode.name, shortcode.line, &block.contribution.attributes, &shortcode.attrs);
            all_problems.extend(problems);
            shortcode.element = Some(block.contribution.element.clone());
            shortcode.attrs = kept;
            used.extend(block.assets.iter().map(|(_, asset)| asset.clone()));
        });
        (used, all_problems)
    }

    /// Publish the scripts and stylesheets of the blocks pages place, so a
    /// block nobody uses publishes nothing.
    pub fn emit_assets(&self, documents: &[ParsedDocument], output_dir: &Path, pending: &mut PendingManifest) -> Result<(), String> {
        let used: BTreeSet<&BlockAsset> = documents.iter().flat_map(|d| &d.features.block_assets).collect();
        let mut ctx = BuildContext::for_render(output_dir, pending);
        for block in self.blocks.values() {
            for (path, asset) in &block.assets {
                if used.contains(asset) {
                    let bytes = crate::build::cloud_readiness::read_with_materialize_wait(&block.dir.join(path), crate::build::cloud_readiness::INTERACTIVE_DEADLINE).map_err(|e| format!("Failed to read `{path}` of `{}`: {e}", block.plugin))?;
                    ctx.emit_held(&ServedPath::for_plugin_asset(&block.plugin, path), bytes, HashBucket::Files)
                        .map_err(|e| format!("Failed to emit `{path}` of `{}`: {e}", block.plugin))?;
                }
            }
        }
        Ok(())
    }
}

/// The attributes a fence set that its block declares, and a sentence for each
/// problem.
fn checked_attributes(name: &str, line: usize, declared: &[BlockAttribute], written: &[(String, String)]) -> (Vec<(String, String)>, Vec<String>) {
    let mut problems = Vec::new();
    let mut kept = Vec::new();
    for (key, value) in written {
        let Some(field) = declared.iter().find(|f| f.key == *key) else {
            problems.push(format!("`:::{name}` on line {line}: `{key}` is not an attribute it declares, so it is dropped"));
            continue;
        };
        match invalid_because(field, value) {
            Some(why) => problems.push(format!("`:::{name}` on line {line}: `{key}={value}` is dropped because {why}")),
            None => kept.push((key.clone(), value.clone())),
        }
    }
    for field in declared.iter().filter(|f| f.required && !written.iter().any(|(k, _)| *k == f.key)) {
        problems.push(format!("`:::{name}` on line {line} needs `{}`", field.key));
    }
    (kept, problems)
}

fn invalid_because(field: &BlockAttribute, value: &str) -> Option<String> {
    match field.kind {
        AttributeKind::Number if value.parse::<f64>().is_err() => return Some("it is not a number".to_string()),
        AttributeKind::Boolean if !matches!(value, "true" | "false") => return Some("it is not `true` or `false`".to_string()),
        _ => {}
    }
    if !field.options.is_empty() && !field.options.iter().any(|o| o == value) {
        return Some(format!("it is not one of the options of `{}`", field.key));
    }
    let pattern = field.pattern.as_deref().and_then(|p| regex::Regex::new(p).ok())?;
    (!pattern.is_match(value)).then(|| field.pattern_message.clone().unwrap_or_else(|| "it does not match the declared pattern".to_string()))
}

/// The head tags for a set of block assets. Addresses carry a content version,
/// because the path never changes and a browser would keep an old script past
/// a plugin update.
pub fn asset_tags(assets: &BTreeSet<BlockAsset>) -> String {
    assets.iter().map(|asset| match asset {
        BlockAsset::Stylesheet(href) => format!(r#"<link rel="stylesheet" href="{href}">"#),
        BlockAsset::Script(src) => format!(r#"<script type="module" src="{src}"></script>"#),
    }).collect()
}

#[cfg(test)]
mod tests {
    use crate::plugins::types::PluginManifest;
    use serde_json::json;

    fn parse(name: &str, entry: Option<&str>, blocks: serde_json::Value) -> Result<PluginManifest, String> {
        let mut manifest = json!({"name":name, "version":"1.0.0", "contributes":{"blocks":blocks}});
        if let Some(entry) = entry {
            manifest["entry"] = entry.into();
        }
        PluginManifest::parse(&manifest.to_string()).map_err(|e| e.to_string())
    }

    fn card(block: serde_json::Value) -> Result<PluginManifest, String> {
        parse("catalog", Some("main.js"), json!({"card":block}))
    }

    #[test]
    fn a_declared_block_parses_and_needs_no_entry() {
        let m = parse("catalog", None, json!({"card":{
            "element":"demo-card","script":"dist/view.js","stylesheet":"view.css",
            "attributes":[{"key":"label"},{"key":"data-size","type":"number"},{"key":"page","options":["book","desk"]}]
        }})).unwrap();
        let block = &m.contributes.as_ref().unwrap().blocks.as_ref().unwrap()["card"];
        assert_eq!(block.script.as_deref(), Some("dist/view.js"));
        assert_eq!(block.attributes[0].kind, super::AttributeKind::String, "an attribute is a string unless it says");
        assert_eq!(block.attributes[2].options, ["book", "desk"]);
        assert!(m.capabilities.is_empty(), "a block grants no capability");
    }

    #[test]
    fn an_unsafe_or_unplaceable_declaration_is_refused_at_parse() {
        for (block, needle) in [
            (json!({"element":"div"}), "custom element"),
            (json!({"element":"Demo-Card"}), "custom element"),
            (json!({"element":"font-face"}), "custom element"),
            (json!({"element":"demo-card","script":"../view.js"}), "inside the plugin"),
            (json!({"element":"demo-card","stylesheet":"/etc/view.css"}), "inside the plugin"),
            (json!({"element":"demo-card","script":"a\"b.js"}), "inside the plugin"),
            (json!({"element":"demo-card","script":"https://example.com/view.js"}), "inside the plugin"),
            (json!({"element":"demo-card","script":"view.html"}), "must end in .js or .mjs"),
            (json!({"element":"demo-card","stylesheet":"view.js"}), "must end in .css"),
            (json!({"element":"demo-card","attributes":[{"key":"a","type":"string","pattern":"("}]}), "does not compile"),
            (json!({"element":"demo-card","attributes":[{"key":"onclick","type":"string"}]}), "attribute a block may set"),
            (json!({"element":"demo-card","attributes":[{"key":"a b","type":"string"}]}), "attribute a block may set"),
            (json!({"element":"demo-card","attributes":[{"key":"token","type":"secret"}]}), "unknown variant"),
            (json!({"element":"demo-card","attributes":[{"key":"items","type":"object"}]}), "unknown variant"),
            (json!({"element":"demo-card","attributes":[{"key":"size","type":"number","options":["1"]}]}), "belong to a `string`"),
            (json!({"element":"demo-card","attributes":[{"key":"a","type":"string"},{"key":"a","type":"string"}]}), "declared twice"),
        ] {
            let err = card(block.clone()).expect_err(&block.to_string());
            assert!(err.contains(needle), "{block}: {err}");
        }
    }

    #[test]
    fn a_changed_asset_is_a_different_plan() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("view.js"), "one").unwrap();
        let manifest = card(json!({"element":"demo-card","script":"view.js"})).unwrap();
        let plugin = crate::plugins::types::Plugin { manifest, path: dir.path().into() };
        let before = super::BlockPlan::from_plugins(std::slice::from_ref(&plugin));
        std::fs::write(dir.path().join("view.js"), "two").unwrap();
        let after = super::BlockPlan::from_plugins(std::slice::from_ref(&plugin));
        assert_ne!(format!("{before:?}"), format!("{after:?}"), "the parse cache keys on a plan's contents");
    }

    #[test]
    fn a_builtin_short_name_and_an_unnameable_plugin_are_refused() {
        let builtin = parse("catalog", Some("main.js"), json!({"form":{"element":"demo-form"}})).unwrap_err();
        assert!(builtin.contains("one of moss's own"), "{builtin}");
        let id = parse("my.plugin", Some("main.js"), json!({"card":{"element":"demo-card"}})).unwrap_err();
        assert!(id.contains("letters, digits"), "{id}");
    }
}
