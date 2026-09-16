//! Plugin Registry - Bundled plugins shown in the plugin installer
//!
//! This module provides a list of all bundled plugins for the plugin installer UI.
//! For Phase 1, all plugins come from bundled plugins (embedded in the binary).
//! Remote plugins may be added later.
//!
//! All bundled plugins appear in the installer (a `preview` manifest
//! declaration hides a row from users who haven't opted into preview
//! features — see `channel_is_listed` and ADR-053). No plugin is
//! auto-installed — users install them from the catalog or by dev sideload.
//! The frontend uses capabilities to decide rendering behavior (e.g., syndicators
//! get control panel icons, process plugins get toast + settings integration).

use moss_core::untrusted_text::{bounded, MAX_NAME};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::bundled::{get_bundled_plugin_names, get_bundled_plugin_info, install_bundled_plugin};
use super::install::registry_client::artifact;
use super::install::registry_client::enforce;
use super::install::registry_client::enforce::Refusal;
use super::install::registry_client::index::IndexEntry;

mod catalog;

use catalog::{
    add_rows_for_the_folder, annotate_available_updates, drop_offers_this_moss_would_refuse, identity_follows_the_bytes,
    merge_registry_rows,
};

/// Where a channel comes from — invisible to the user, used internally
/// to route install/uninstall/icon operations.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChannelSource {
    /// Built into the moss binary (e.g., email)
    Builtin,
    /// Provided by an installable plugin (e.g., Matters)
    Plugin,
}

/// Where a plugin's bytes come from — which decides how it installs and
/// updates, where `ChannelSource` decides only whether a channel needs a
/// plugin at all.
///
/// A row can be `Registry` and installed at the same time: origin is about the
/// source of the bytes, not about whether they are on disk yet.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PluginOrigin {
    /// moss ships these bytes in its own binary. Exempt from the registry's
    /// version floor and its kill list, because shipping them is the review.
    Bundled,
    /// The registry publishes it. Installing means fetching the pinned
    /// artifact; updating means the user asking for the newer pinned one.
    Registry,
    /// Only the folder knows it: a dev sideload, or whatever a shared folder
    /// brought with it. Nothing vouches for it but the user, so it has a row
    /// for one reason — the tile is where they allow it to run.
    Sideloaded,
}

fn default_plugin_origin() -> PluginOrigin {
    PluginOrigin::Bundled
}

/// Information about an available channel (publication destination).
///
/// This struct unifies built-in channels (email) and plugin-provided channels
/// (Matters, Mastodon) under a single type. The frontend renders them
/// identically — `source` is an implementation detail.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AvailablePlugin {
    /// Unique channel identifier (e.g., "matters", "email")
    pub id: String,

    /// The version this row describes — the bundled manifest's for a bundled
    /// plugin. Empty for a built-in channel, which has no manifest and no
    /// registry entry. Revocation is per version, so a row that cannot say
    /// which version it is cannot be checked against the kill list.
    #[serde(default)]
    pub version: String,

    /// Short description of what the channel does
    pub description: String,

    /// URL to the plugin icon (empty for bundled - icon loaded separately)
    pub icon_url: Option<String>,

    /// Channel capabilities (e.g., ["syndicate"])
    pub capabilities: Vec<String>,

    /// Whether this channel is currently active (installed + enabled)
    #[serde(default)]
    pub installed: bool,

    /// Where this channel comes from (builtin or plugin)
    #[serde(default = "default_plugin_source")]
    pub source: ChannelSource,

    /// The channel's runtime is a machine-wide companion stack that must be
    /// downloaded/installed on first install (e.g. OnionPress). Mirrors the
    /// plugin manifest's `requires_stack` declaration.
    #[serde(default)]
    pub requires_stack: bool,

    /// The publisher does not consider this version ready to be offered by
    /// default. Mirrors the manifest's `preview` declaration for a bundled
    /// plugin, and the index entry's `preview` for a remote one. Presentation
    /// only — it decides who is SHOWN the row, never who may install it.
    #[serde(default)]
    pub preview: bool,

    /// Why this row will not run, if it will not — withdrawn by the registry,
    /// built for a newer moss than this one, or waiting for the user to allow
    /// it. Presented, never hidden: a user with the plugin already installed
    /// needs to be told why it stopped working. Set from the same verdict that
    /// refuses the load, so the badge and the refusal cannot disagree, and
    /// carrying its own kind so that the precedence is decided once (see
    /// [`Refusal`]).
    #[serde(default)]
    pub refusal: Option<Refusal>,

    /// A newer version the registry publishes that this moss would accept.
    ///
    /// `None` covers several situations on purpose — no registry entry, nothing
    /// newer, a newer version whose `min_moss_version` outruns this host, one
    /// the kill list has withdrawn — because they mean the same thing to the
    /// person reading the row: there is no update to take. Offering a version
    /// that install and the loader would both refuse is worse than silence.
    #[serde(default)]
    pub update_available: Option<String>,

    /// Where this row's bytes come from. Defaults to `Bundled` so a built-in
    /// channel — which has no plugin at all — is never mistaken for something
    /// the registry can serve.
    #[serde(default = "default_plugin_origin")]
    pub origin: PluginOrigin,

    /// The name to show when moss has no `channels.name.<id>` string. See
    /// [`published_name`], which is the one place that decides it.
    #[serde(default)]
    pub display_name: Option<String>,
}

/// A row that does not say where it comes from is a plugin channel.
///
/// Serde's default for the field, and the value both plugin-row constructors
/// use, so "a plugin channel's source" is written once. Built-in rows are the
/// ones that have to say so, which is the right way round: they are the
/// closed set, enumerated in `BUILTIN_CHANNELS`.
fn default_plugin_source() -> ChannelSource {
    ChannelSource::Plugin
}

/// The publisher's own name for a plugin moss does not ship, or `None` for one
/// it does.
///
/// One owner for "can a `channels.name.<id>` string exist for this id?" — and
/// it cannot, exactly when moss did not know the id when its strings were
/// written. Both rails ask it: the catalog builds `AvailablePlugin`, the
/// publish rail builds `PluginInfo`, and each used to decide it its own way
/// (one from `origin`, one from the bundled-name list). Same question, so one
/// function, in Rust — a copy in TypeScript would be a third.
///
/// `None` for a plugin moss ships is the load-bearing half: there a missing
/// key is a bug, and the frontend has to keep warning and Title-Casing the id
/// rather than quietly rendering the manifest's English.
/// The name for a plugin that is on disk — [`published_name`] when something
/// vouches for the bytes, and the folder's own name when nothing does.
///
/// Both rails ask it, and they have to give the same answer: the catalog tile
/// asks the trust question and the publish rail draws the disabled button, and
/// a plugin nobody vouched for must not be called by moss's own name for the
/// id its directory borrowed on either of them.
pub fn installed_display_name(manifest: &crate::plugins::PluginManifest, dir: &std::path::Path) -> Option<String> {
    if enforce::vouched_for(manifest, dir) {
        published_name(&manifest.name, manifest.display_name.as_deref())
    } else {
        Some(plugin_human_name(manifest))
    }
}

pub fn published_name(id: &str, declared: Option<&str>) -> Option<String> {
    if get_bundled_plugin_names().contains(&id) || BUILTIN_CHANNELS.iter().any(|c| c.id == id) {
        return None;
    }
    declared.map(|declared| bounded(declared, MAX_NAME))
}

/// Get the list of all bundled plugins for the installer UI
///
/// Returns all bundled plugins for the installer UI.
/// No plugin is auto-installed — users install from the catalog (or by dev
/// sideload into `.moss/plugins/`).
/// The frontend uses capabilities to decide rendering behavior.
/// The `installed` field will be false — call `get_available_plugins_with_status`
/// to populate it.
pub fn get_available_plugins() -> Vec<AvailablePlugin> {
    let mut plugins = Vec::new();

    for plugin_name in get_bundled_plugin_names() {
        // Get metadata from bundled manifest
        if let Some(manifest) = get_bundled_plugin_info(plugin_name) {
            plugins.push(AvailablePlugin {
                id: plugin_name.to_string(),
                version: manifest.version.clone(),
                requires_stack: manifest.needs_stack(),
                description: manifest.description.unwrap_or_default(),
                icon_url: None, // Icon loaded from bundled plugin directory
                capabilities: manifest.capabilities.iter()
                    .map(|c| c.hook_name().to_string())
                    .collect(),
                installed: false,
                source: default_plugin_source(),
                preview: manifest.preview,
                refusal: None,
                update_available: None,
                origin: PluginOrigin::Bundled,
                display_name: None,
            });
        }
    }

    plugins
}

/// A kebab id as a person would read it: `dev-to` -> `Dev To`.
///
/// Private, and reached only through [`plugin_human_name`]. `channelName` in
/// the frontend Title-Cases an id too, and the two are not a duplicated rule:
/// that one is the last resort for a channel whose `channels.name.<id>` key is
/// missing — a bug, which it warns about — while this one names a SIDELOADED
/// plugin, which can have no key and is not a bug — including a catalog row
/// re-identified from the folder's own bytes, which carries one for exactly
/// that reason.
fn format_plugin_name(name: &str) -> String {
    name.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The name a person should see for a plugin: what the manifest declares, or
/// its id in Title Case. One owner — the settings list and the action panel's
/// titlebar must not disagree about what a plugin is called.
///
/// Which is also why the bound and the bidi strip belong HERE. `display_name`
/// is publisher-authored and, for a sideloaded plugin, authored by whoever
/// wrote the folder: nothing validates a manifest that was never installed
/// through the registry. It lands in six sentences moss writes — the settings
/// rail, the credential modal's "Enter your credentials for …", the setup
/// probe, the progress panel — so a direction override inside it reverses
/// moss's own words. Guarding the caller that was reported would leave the
/// other five, which is the same miss twice over.
pub fn plugin_human_name(manifest: &crate::plugins::types::PluginManifest) -> String {
    let name = manifest
        .display_name
        .clone()
        .unwrap_or_else(|| format_plugin_name(&manifest.name));
    bounded(&name, MAX_NAME)
}

/// What to call the contribution granting a capability, on screen.
///
/// The contribution's own name when it declares one ("GitHub Pages" is where
/// the site lands; "GitHub" is who runs it), the plugin's human name
/// otherwise. The id is never a label — the Host row used to show the raw
/// manifest name while the setup modal beside it said "GitHub". Which
/// contribution grants which capability is the manifest's own knowledge, so
/// the lookup lives there and this only supplies the fallback.
pub fn contribution_display_name(
    manifest: &crate::plugins::types::PluginManifest,
    capability: &crate::plugins::types::Capability,
) -> String {
    // Bounded on BOTH branches. A manifest that declares a per-contribution
    // name never reaches `plugin_human_name`, so clamping only the fallback
    // left the credential modal, the setup probe and the deploy target list
    // reading a sideloaded folder's string verbatim — the same three sentences
    // the clamp was added for.
    match manifest.display_name_for(capability) {
        Some(declared) => bounded(declared, MAX_NAME),
        None => plugin_human_name(manifest),
    }
}

/// Get available plugins with installed status populated
///
/// Checks which plugins are already installed in the given project path
/// and marks their `installed` field accordingly.
///
/// # Arguments
/// * `project_path` - Path to the project containing .moss/plugins/
///
/// # Returns
/// Vector of available plugins with correct installed status
pub fn get_available_plugins_with_status(project_path: &str) -> Vec<AvailablePlugin> {
    let mut plugins = get_available_plugins();
    let entries = enforce::cached_entries();
    // Before the installed pass, not after, so a registry row goes through the
    // same "what is actually on disk" read the bundled ones do. A plugin the
    // user installed from the registry is an installed plugin like any other.
    // Every published id gets a row here; which of them are *offers* is
    // decided below, once there is an answer about what is on disk.
    merge_registry_rows(&mut plugins, &entries);
    let plugins_dir = crate::moss_paths::MossPaths::new(std::path::Path::new(project_path)).plugins_dir();
    add_rows_for_the_folder(&mut plugins, &plugins_dir);

    for plugin in &mut plugins {
        // The INSTALLED manifest, not just its existence: revocation is per
        // version, and the version the user has is the only one a badge can
        // honestly describe. Reading it here is also what makes the badge and
        // the loader's refusal agree — `load_plugin` reads this same file, so
        // asking the kill list about the bundled version instead would badge a
        // working plugin, or leave a refused one looking fine.
        let dir = plugins_dir.join(&plugin.id);
        if let Some(installed) = crate::plugins::bundled::read_installed_manifest(&dir) {
            plugin.installed = true;
            plugin.version = installed.version.clone();
            // The same call the loader makes, on the same manifest and
            // directory, so the tile explains exactly what the refusal will
            // say — including which reason wins when more than one applies.
            plugin.refusal = enforce::verdict(&installed, &dir);
            if !enforce::vouched_for(&installed, &dir) {
                identity_follows_the_bytes(plugin, &installed);
            }
        } else if let Some(bundled) = crate::plugins::bundled::get_bundled_plugin_info(&plugin.id) {
            // Nothing on disk: nothing to approve, and installing copies the
            // binary's own bytes, so only a published withdrawal can refuse
            // this row. A revoked registry row is dropped a moment later, so
            // this badge only ever reaches the bundled row moss ships and the
            // user has not installed.
            plugin.refusal = enforce::withheld(&bundled, true);
        }
    }

    // Both of the last two steps ask about an OFFER, which is why they run
    // last: whether a row is an offer is decided by what the installed read
    // above found on disk. Their own rules live on the functions.
    drop_offers_this_moss_would_refuse(&mut plugins, &entries);

    annotate_available_updates(&mut plugins, entries);

    plugins
}

// ============================================================================
// Built-in Channels
// ============================================================================

/// Email envelope icon SVG (Lucide "mail" icon)
const EMAIL_ICON_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="16" x="2" y="4" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/></svg>"#;

/// One registration row per built-in channel.
///
/// NORTH-STAR pattern 2 (registration table with a totality test beside
/// it — see test_builtin_channel_table_totality): adding a built-in
/// channel is adding ONE row here; ids, catalog rows, and icons all
/// derive from this table. The row-home moves to host_api/registry.rs
/// at M3; until then this is the single source in this file.
struct BuiltinChannel {
    id: &'static str,
    description: &'static str,
    capabilities: &'static [&'static str],
    icon_svg: &'static str,
}

const BUILTIN_CHANNELS: &[BuiltinChannel] = &[BuiltinChannel {
    id: "email",
    description: "Send articles to email subscribers",
    capabilities: &["syndicate"],
    icon_svg: EMAIL_ICON_SVG,
}];

/// Get the list of built-in channels.
///
/// Built-in channels are publication destinations that ship with moss
/// and don't require plugin installation. They use the same AvailablePlugin
/// struct as plugin channels so the frontend renders them identically.
fn get_builtin_channels() -> Vec<AvailablePlugin> {
    BUILTIN_CHANNELS
        .iter()
        .map(|c| AvailablePlugin {
            id: c.id.to_string(),
            // A built-in channel is moss code, not a versioned artifact.
            version: String::new(),
            description: c.description.to_string(),
            icon_url: None,
            capabilities: c.capabilities.iter().map(|s| s.to_string()).collect(),
            installed: false, // Will be populated by get_available_channels_with_status
            source: ChannelSource::Builtin,
            requires_stack: false,
            // A built-in channel is moss's own surface; if one is ever
            // unfinished that belongs in BUILTIN_CHANNELS as a row field.
            preview: false,
            // Not a plugin at all, so nothing can serve it but moss. The
            // default rather than a third variant: `source` already says a
            // built-in channel is built in, and a second enum saying the same
            // thing is a second place to keep in step.
            origin: PluginOrigin::Bundled,
            refusal: None,
            // A built-in channel always has a key — it is enumerated in this
            // binary — so it never needs a published name.
            display_name: None,
            // moss's own code cannot be updated apart from moss.
            update_available: None,
        })
        .collect()
}

/// Get the list of built-in channel IDs.
///
/// Used by `get_syndicator_plugins` to check which built-in channels
/// are active without doing a full channel scan.
pub fn get_builtin_channel_ids() -> Vec<&'static str> {
    BUILTIN_CHANNELS.iter().map(|c| c.id).collect()
}

/// Get the icon SVG for a built-in channel.
///
/// Returns None if the channel_id is not a built-in channel.
pub fn get_builtin_channel_icon(channel_id: &str) -> Option<String> {
    BUILTIN_CHANNELS
        .iter()
        .find(|c| c.id == channel_id)
        .map(|c| c.icon_svg.to_string())
}

/// Get the icon SVG of a plugin installed in the project's `.moss/plugins/`.
///
/// Reached only for an id the binary does not carry — see
/// [`resolve_channel_icon`], which asks the binary first. Only SVG content is
/// served: raster icons fall through to the caller's next source.
pub fn get_installed_plugin_icon(project_path: &str, plugin_id: &str) -> Option<String> {
    // Read the manifest directly instead of going through discover_plugins:
    // discovery side-effects (ensure_bundled_plugins_installed) and rejects
    // plugins whose entry file is missing — both wrong for an icon read.
    let plugin_dir = crate::plugins::plugin_dir(project_path, plugin_id);
    let manifest = std::fs::read_to_string(plugin_dir.join("manifest.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
    let field = |key: &str| {
        manifest.as_ref().and_then(|m| m.get(key).and_then(|v| v.as_str()).map(String::from))
    };
    let manifest_icon = field("icon");

    let icon_path = super::types::icon_path_in(&plugin_dir, manifest_icon.as_deref())?;
    if icon_path.extension().and_then(|e| e.to_str()) != Some("svg") {
        return None;
    }
    let version = field("version").unwrap_or_default();
    let raw = std::fs::read_to_string(icon_path).ok()?;
    // Everything else is a stranger's. The download path cleans the PREVIEW
    // icon, but `resolve_channel_icon` reaches the installed copy first, and an
    // installed plugin's `icon.svg` is whatever its artifact contained —
    // hash-pinned to what the publisher submitted, which is the point: the hash
    // proves provenance, not intent. This string is written into the app's own
    // DOM with `innerHTML`, in the window that can call every Tauri command,
    // while the plugin's own code runs in a JS runtime with no Tauri access at
    // all. Unsanitized, the icon is the shorter path to exactly the privilege
    // the sandbox exists to withhold.
    crate::plugins::install::registry_client::icon::sanitize_icon_svg(
        &raw, plugin_id, &version,
    )
}

/// Resolve a channel's icon SVG: builtin → bundled copy → installed copy →
/// the registry's cached copy.
///
/// The ONE resolution order for every surface, and both of the first two are
/// there so that a directory on disk cannot speak for moss. Builtin first: a
/// rogue installed dir must never shadow a moss-supplied channel. Bundled
/// second, at any version — writing `.moss/plugins/matters/manifest.json` and
/// a hostile `icon.svg` was otherwise enough to put `<svg onload>` into the
/// window that can call every Tauri command, and moss's own logos do not
/// survive the stranger allowlist anyway: matters paints entirely from a
/// `<style>` block through `class`, both of which are stripped, so cleaning it
/// yields two solid black shapes. What that costs is a developer's sideloaded
/// override of a BUNDLED plugin's icon; a sideloaded plugin moss does not ship
/// is unaffected, which is the case the installed leg was written for.
///
/// The registry's copy is last because it is the only one moss did not
/// produce: every local source is preferred to a downloaded one, so the
/// downloaded icon is reached exactly when the row is for a plugin that is
/// published and not here yet — which is the only row that has no other icon.
pub fn resolve_channel_icon(project_path: Option<&str>, channel_id: &str) -> Option<String> {
    if let Some(icon) = get_builtin_channel_icon(channel_id) {
        return Some(icon);
    }
    if let Some(path) = project_path {
        let dir = crate::moss_paths::MossPaths::new(std::path::Path::new(path)).plugins_dir().join(channel_id);
        if super::bundled::read_installed_manifest(&dir)
            .is_some_and(|manifest| !enforce::vouched_for(&manifest, &dir))
        {
            // The folder holds this id and nothing vouches for what is in it,
            // so no icon moss holds for the id belongs to it: its own file, or
            // none. Falling through would draw the bundled plugin's logo
            // beside the name the catalog just stopped lending it.
            return get_installed_plugin_icon(path, channel_id);
        }
    }
    if let Some(icon) = super::bundled::get_bundled_plugin_icon(channel_id) {
        return Some(icon);
    }
    if let Some(path) = project_path {
        if let Some(icon) = get_installed_plugin_icon(path, channel_id) {
            return Some(icon);
        }
    }
    published_icon(channel_id)
}

/// The cached icon for a published plugin, sanitized when it was downloaded.
fn published_icon(channel_id: &str) -> Option<String> {
    let app_data_dir = crate::infra::app_data::app_data_dir_early()?;
    let entries = enforce::cached_entries();
    let entry = entries.iter().find(|e| e.id == channel_id)?;
    crate::plugins::install::registry_client::icon::cached_icon(&app_data_dir, entry)
}

/// Get all available channels (built-in + plugins) with installed status.
///
/// Merges built-in channels with plugin channels into a single list.
/// For built-in channels, "installed" means enabled in the syndicate hooks.
/// For plugin channels, "installed" means plugin files exist in .moss/plugins/.
pub fn get_available_channels_with_status(
    project_path: &str,
    show_preview: bool,
) -> Vec<AvailablePlugin> {
    let mut channels = get_builtin_channels();

    // Check built-in channel status from channels config
    let channels_config = super::discovery::get_channels_config(project_path).ok();
    for channel in &mut channels {
        if channel.source == ChannelSource::Builtin {
            channel.installed = channels_config
                .as_ref()
                .map(|cc| cc.is_installed(&channel.id))
                .unwrap_or(false);
        }
    }

    // Append plugin channels regardless of CAPABILITY — the catalog is the
    // one management surface; entry points narrow by capability instead of
    // this command hiding rows. channel_is_listed is the one exception
    // (readiness/preview policy, not capability), and only while uninstalled. The
    // publish rail is unaffected: it builds from get_syndicator_plugins,
    // which filters to Syndicate itself.
    channels.extend(
        get_available_plugins_with_status(project_path)
            .into_iter()
            .filter(|p| channel_is_listed(p, show_preview)),
    );

    channels
}

/// Whether the catalog draws this row at all.
///
/// One question, because it is the only one the caller has: the warning a
/// refused row wears is drawn from `AvailablePlugin::refusal`, which crosses
/// to the frontend on its own.
///
/// Two things can decide this. The row carries a `refusal` — withdrawn, or
/// built for a newer moss — which keeps it LISTED either way: the plugin has
/// stopped running and the row is where the user finds out, and the catalog
/// is the only surface that can remove it. Or the plugin's own manifest says
/// this version isn't ready to be offered yet (`preview`) and the user hasn't
/// turned on preview features, which hides. Nothing else affects a row —
/// capability narrowing happens at the entry points.
///
/// A floor reaches this function only for something already installed. An
/// *offer* whose floor this moss fails is dropped by
/// [`drop_offers_this_moss_would_refuse`] before it gets here, and a bundled
/// row's floor is met by the binary it shipped inside.
///
/// Ordering: a refusal outranks `installed`, which outranks readiness.
/// Readiness fails OPEN; revocation fails CLOSED. Never fold them together —
/// a readiness rule that failed closed would empty the catalog whenever moss
/// was offline.
///
/// Takes the merged catalog row, not an id, so a plugin that came from the
/// registry gets the same verdict from the same code as a bundled one; and it
/// is called AFTER id-dedup, so exactly one verdict exists per id.
///
/// An installed copy is real state: it stays visible and manageable whatever
/// its readiness says. Being listed is a presentation choice, not an
/// entitlement — `install_plugin` deliberately does not consult this (ADR-053,
/// ADR-032 clause 2). Being *revoked* is not a presentation choice: the
/// refusal lives at the loader (`crate::plugins::admission`), and this
/// only decides whether the row explaining it is on screen.
pub fn channel_is_listed(channel: &AvailablePlugin, show_preview: bool) -> bool {
    channel.refusal.is_some() || channel.installed || !channel.preview || show_preview
}

/// Install a plugin into a project.
///
/// Copies it out of the binary when moss ships it, and otherwise downloads the
/// version the registry publishes — see [`published_entry`].
///
/// # Arguments
/// * `plugin_id` - ID of the plugin to install
/// * `project_path` - Path to the project where plugin should be installed
///
/// # Returns
/// * `Ok(())` on success
/// * `Err(String)` with error message on failure
pub fn install_plugin(plugin_id: &str, project_path: &str) -> Result<(), String> {
    // No catalog-row check first. A row is a thing the UI drew; what decides
    // whether an install may proceed is whether the bytes can be got, and the
    // two branches below each answer that for their own origin. Checking the
    // rows made the remote branch unreachable — the row list holds only
    // plugins moss ships, so an id it did not know was refused before
    // anything looked in the catalog for it.
    //
    // The id arrives from the frontend and is joined to a path below, so it
    // meets the same check an id off the wire meets. One rule, both origins.
    if !crate::plugins::install::is_usable_id(plugin_id) {
        return Err(format!("'{plugin_id}' is not a plugin id"));
    }

    // Idempotent: if already installed, return success
    let plugin_dir = crate::plugins::plugin_dir(project_path, plugin_id);
    if plugin_dir.exists() && plugin_dir.join("manifest.json").exists() {
        return Ok(());
    }

    // Which bytes exist decides where they come from. A plugin moss ships is
    // copied out of the binary; anything else is fetched, hash-checked against
    // what review pinned, and unpacked by `registry_client::artifact`. One command
    // rather than two: the frontend has no business knowing which, and a
    // second command would be a second place for the refusals below to be
    // forgotten.
    let Some(manifest) = crate::plugins::bundled::get_bundled_plugin_info(plugin_id) else {
        let entry = published_entry(plugin_id)?;
        return artifact::install_from_registry(&entry, project_path).map(|_| ());
    };

    // Refusing at load only would let the whole install run and succeed — ring
    // animation, "ready" announcement, tile flipped to installed — for a
    // plugin the very next `load_plugin` refuses, with nothing on screen to
    // say why. The remote path above runs the same check on the manifest that
    // comes out of the archive, which is the only one it can trust. These
    // bytes are the binary's own, so the floor does not apply and no consent
    // is needed: the loader's exact-bytes test admits them on its own.
    if let Some(refusal) = enforce::withheld(&manifest, true) {
        return Err(format!("{plugin_id} cannot be installed: {}", refusal.sentence(&manifest)));
    }

    // Install from bundled plugins (no download needed)
    install_bundled_plugin(plugin_id, project_path)
}

/// The catalog's current entry for `plugin_id`.
///
/// Looked up at every call rather than passed in by whoever drew the row: the
/// cache is refreshed on launch and on catalog open, and a stale entry held by
/// a caller is exactly how a withdrawn version gets installed after the
/// refresh that withdrew it. Both the install and the update ask here.
fn published_entry(plugin_id: &str) -> Result<IndexEntry, String> {
    enforce::cached_entries()
        .into_iter()
        .find(|e| e.id == plugin_id)
        .ok_or_else(|| format!("Plugin '{plugin_id}' is not in this build or the catalog"))
}

/// Take the version the catalog offers for an already-installed plugin.
///
/// Reached only from a click on a badge, and there is no caller that takes an
/// update without one: every surveyed supply-chain incident rode an automatic
/// one, so the decision is the user's and moss only offers.
///
/// The offer is re-derived here rather than taken from the caller. The badge
/// asked [`catalog::offerable_version`] whenever the catalog was last built,
/// and a refresh since then may have withdrawn the version or raised the
/// floor — so the question is asked again against the cache as it stands at
/// the moment of the click, and a stale badge buys nothing.
pub fn update_plugin(plugin_id: &str, project_path: &str) -> Result<String, String> {
    if !crate::plugins::install::is_usable_id(plugin_id) {
        return Err(format!("'{plugin_id}' is not a plugin id"));
    }

    // A bundled id never reaches here from a click, because
    // `annotate_available_updates` refuses to badge one — that rule lives at
    // the offer, so it cannot be broken after the user has already pressed.
    let plugin_dir = crate::plugins::plugin_dir(project_path, plugin_id);
    let installed = crate::plugins::bundled::read_installed_manifest(&plugin_dir)
        .ok_or_else(|| format!("'{plugin_id}' is not installed here"))?;

    let entry = published_entry(plugin_id)?;

    // Not `>`: this is the same function the badge called, so it also carries
    // the kill list and the version floor. An update that installs a revoked
    // or too-new version is the one outcome an update button must not have.
    catalog::offerable_version(&installed.version, &entry)
        .ok_or_else(|| format!("There is nothing newer for '{plugin_id}' to take"))?;

    // The same download, hash-check and unpack the install used. The receipt
    // written there is what lets this one replace the code without taking the
    // plugin's own files with it.
    artifact::install_from_registry(&entry, project_path)
}

/// Uninstall a plugin
///
/// Removes the plugin from .moss/plugins/ and removes it from any hooks in config.toml
///
/// # Arguments
/// * `plugin_id` - ID of the plugin to uninstall
/// * `project_path` - Path to the project
///
/// # Returns
/// * `Ok(())` on success
/// * `Err(String)` with error message on failure
pub fn uninstall_plugin(plugin_id: &str, project_path: &str) -> Result<(), String> {
    // This one joins the id to a path and then removes the tree at it, so it
    // is the door that matters most — the same check the install path runs.
    if !crate::plugins::install::is_usable_id(plugin_id) {
        return Err(format!("'{plugin_id}' is not a plugin id"));
    }
    let plugin_dir = crate::plugins::plugin_dir(project_path, plugin_id);

    // Check if plugin is installed
    if !plugin_dir.exists() {
        return Err(format!("Plugin '{}' is not installed", plugin_id));
    }

    // Ensure config is migrated before raw-TOML surgery. We log but don't fail —
    // uninstall should proceed even if migration is unhappy (defensive cleanup path).
    // See docs/reference/config-migrations.md.
    if let Err(e) = super::discovery::get_channels_config(project_path) {
        log::warn!("config migration failed during uninstall_plugin: {}. Proceeding with raw TOML surgery.", e);
    }

    // Remove from hooks in config.toml. Best-effort like the migration above:
    // a config that cannot be read or written is logged, and the plugin
    // directory still goes.
    let config_path = std::path::Path::new(project_path).join(".moss").join("config.toml");
    if let Err(e) = strip_plugin_from_config(&config_path, plugin_id) {
        log::warn!("could not strip '{plugin_id}' from {}: {e}", config_path.display());
    }

    // Remove plugin directory
    std::fs::remove_dir_all(&plugin_dir)
        .map_err(|e| format!("Failed to remove plugin directory: {}", e))?;

    Ok(())
}

/// Drop `plugin_id` from `[hooks]` and `[channels]` in `config.toml`, through
/// `vault::config` — the read substitutes an empty table only for a file
/// proven absent and the write edits the original bytes, so uninstalling a
/// plugin cannot delete the comments the user wrote, and a config the plugin
/// was not in is not rewritten at all.
fn strip_plugin_from_config(config_path: &std::path::Path, plugin_id: &str) -> Result<(), String> {
    let crate::vault::config::ManagedToml { original, root: mut table } =
        crate::vault::config::load_managed_toml(config_path)?;
    if let Some(hooks_table) = table.get_mut("hooks").and_then(|h| h.as_table_mut()) {
        // Remove plugin from all hook types. Hooks come in two shapes: an
        // array (`process`) and a single string (`deploy = "<id>"`,
        // discovery::parse_single).
        for hook_name in &["process", "deploy"] {
            let matches_string = hooks_table
                .get(*hook_name)
                .and_then(|v| v.as_str())
                .is_some_and(|s| s == plugin_id);
            if matches_string {
                hooks_table.remove(*hook_name);
            } else if let Some(arr) = hooks_table
                .get_mut(*hook_name)
                .and_then(|v| v.as_array_mut())
            {
                arr.retain(|v| v.as_str().map(|s| s != plugin_id).unwrap_or(true));
            }
        }
    }

    // Strip from [channels.<id>] (new home for channel install state).
    if let Some(channels_table) = table.get_mut("channels").and_then(|c| c.as_table_mut()) {
        channels_table.remove(plugin_id);
        if channels_table.is_empty() {
            table.remove("channels");
        }
    }
    crate::vault::config::write_managed_toml(config_path, &original, &table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::catalog::{row, row_with};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_get_available_plugins_includes_all_bundled() {
        let plugins = get_available_plugins();

        // Every bundled plugin with a valid manifest should appear
        for name in get_bundled_plugin_names() {
            if get_bundled_plugin_info(name).is_some() {
                assert!(
                    plugins.iter().any(|p| p.id == *name),
                    "Bundled plugin '{}' should appear in the registry",
                    name
                );
            }
        }
    }

    #[test]
    fn test_get_available_plugins_includes_deployers() {
        let plugins = get_available_plugins();

        // The github deployer SHOULD appear in the registry
        // (available for install via plugin installer or first-publish flow)
        assert!(
            plugins.iter().any(|p| p.id == "github"),
            "Deployer plugin 'github' should appear in the installer registry"
        );
    }

    #[test]
    fn test_get_available_plugins_all_start_uninstalled() {
        let plugins = get_available_plugins();

        for plugin in plugins {
            assert!(
                !plugin.installed,
                "Plugin {} should start as not installed",
                plugin.id
            );
        }
    }

    #[test]
    fn test_get_available_plugins_with_status_empty_project() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();

        // Create .moss/plugins/ directory (but no plugins installed)
        fs::create_dir_all(temp.path().join(".moss").join("plugins")).unwrap();

        let plugins = get_available_plugins_with_status(project_path);

        for plugin in plugins {
            assert!(
                !plugin.installed,
                "Plugin {} should not be installed in empty project",
                plugin.id
            );
        }
    }

    #[test]
    fn test_get_available_plugins_with_status_detects_installed() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();

        // Create .moss/plugins/matters/ with manifest.json
        let matters_dir = temp.path().join(".moss").join("plugins").join("matters");
        fs::create_dir_all(&matters_dir).unwrap();
        fs::write(matters_dir.join("manifest.json"), r#"{"name":"matters","version":"1.0.0","entry":"main.js","capabilities":["syndicate"]}"#).unwrap();

        let plugins = get_available_plugins_with_status(project_path);

        // Find matters in the list (if bundled)
        if let Some(matters) = plugins.iter().find(|p| p.id == "matters") {
            assert!(matters.installed, "Matters should be marked as installed");
        }
    }

    #[test]
    fn test_get_available_plugins_with_status_requires_manifest() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();

        // Create .moss/plugins/matters/ directory but NO manifest.json
        let matters_dir = temp.path().join(".moss").join("plugins").join("matters");
        fs::create_dir_all(&matters_dir).unwrap();
        // Note: NOT creating manifest.json

        let plugins = get_available_plugins_with_status(project_path);

        // Find matters in the list (if bundled)
        if let Some(matters) = plugins.iter().find(|p| p.id == "matters") {
            assert!(
                !matters.installed,
                "Matters should NOT be marked as installed without manifest.json"
            );
        }
    }

    #[test]
    fn test_format_plugin_name() {
        assert_eq!(format_plugin_name("matters"), "Matters");
        assert_eq!(format_plugin_name("email"), "Email");
        assert_eq!(format_plugin_name("dev-to"), "Dev To");
    }

    /// The Host row and the credential modal both name the DESTINATION, which
    /// is not always the plugin: "GitHub Pages", not "GitHub".
    #[test]
    fn a_deploy_target_is_labelled_by_where_the_site_lands() {
        let contributed = crate::plugins::types::PluginManifest::parse(
            r#"{"name":"github","version":"1.6.0","entry":"main.js","display_name":"GitHub",
                "contributes":{"deploy_target":{"display_name":"GitHub Pages"}}}"#,
        )
        .unwrap();
        assert_eq!(
            contribution_display_name(&contributed, &crate::plugins::types::Capability::Deploy),
            "GitHub Pages"
        );

        let silent = crate::plugins::types::PluginManifest::parse(
            r#"{"name":"onionpress","version":"0.4.0","entry":"main.js","display_name":"OnionPress",
                "contributes":{"deploy_target":{}}}"#,
        )
        .unwrap();
        assert_eq!(
            contribution_display_name(&silent, &crate::plugins::types::Capability::Deploy),
            "OnionPress",
            "no contributed name falls back to the plugin's own, never the id"
        );

        // Both branches are a plugin's own string, so both are bounded. A
        // manifest that declares a contribution name never reaches
        // `plugin_human_name`, and this one lands in "Enter your credentials
        // for …" — a sentence moss wrote.
        let hostile = crate::plugins::types::PluginManifest::parse(&format!(
            r#"{{"name":"x","version":"1.0.0","entry":"main.js",
                "contributes":{{"deploy_target":{{"display_name":"Cancel\u202e{}"}}}}}}"#,
            "y".repeat(300)
        ))
        .unwrap();
        let shown = contribution_display_name(&hostile, &crate::plugins::types::Capability::Deploy);
        assert!(!shown.contains('\u{202e}'), "no direction override: {shown}");
        assert!(shown.chars().count() <= 65, "bounded: {shown}");
    }

    /// The name on the settings row and the name on the action panel's titlebar
    /// come from here, so what a plugin declares wins over the derived id.
    #[test]
    fn plugin_human_name_prefers_what_the_manifest_declares() {
        let declared = crate::plugins::types::PluginManifest::parse(
            r#"{"name":"onionpress","version":"0.4.0","entry":"main.js","display_name":"OnionPress"}"#,
        )
        .unwrap();
        assert_eq!(plugin_human_name(&declared), "OnionPress");

        let silent =
            crate::plugins::types::PluginManifest::parse(r#"{"name":"dev-to","version":"1.0.0","entry":"main.js"}"#)
                .unwrap();
        assert_eq!(plugin_human_name(&silent), "Dev To", "falls back to the id in Title Case");

        // …and whatever it declares is a name, not a licence to write the
        // sentence moss puts it in. A sideloaded folder's manifest is
        // arbitrary, and this string reaches the settings rail, the credential
        // modal and the progress panel — all of which frame it in moss's own
        // words.
        let hostile = crate::plugins::types::PluginManifest::parse(&format!(
            r#"{{"name":"x","version":"1.0.0","entry":"main.js","display_name":"Cancel\u202e{}"}}"#,
            "y".repeat(300)
        ))
        .unwrap();
        let shown = plugin_human_name(&hostile);
        assert!(!shown.contains('\u{202e}'), "no direction override: {shown}");
        assert!(shown.chars().count() <= 65, "bounded: {shown}");
    }

    #[test]
    fn test_install_plugin_not_in_registry() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        fs::create_dir_all(temp.path().join(".moss")).unwrap();

        let result = install_plugin("nonexistent-plugin", project_path);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("not in this build or the catalog"),
            "an id neither origin knows is refused by the one that looked last"
        );
    }

    /// An update is offered, never taken on moss's own initiative — so the
    /// three ways there is no offer to take are all refusals, not no-ops. The
    /// fourth way, a version the kill list or the floor rules out, is
    /// `offerable_version`'s own test: this asks it, so proving it is asked is
    /// what is left to prove here.
    #[test]
    fn an_update_is_refused_when_there_is_no_offer_to_take() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();

        let not_installed = update_plugin("some-plugin", project_path).unwrap_err();
        assert!(not_installed.contains("is not installed here"), "{not_installed}");

        // Installed, not bundled, and the cache does not list it — the state a
        // plugin is in after the registry withdraws its id entirely.
        let dir = temp.path().join(".moss").join("plugins").join("gone-plugin");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.json"), r#"{"name":"gone-plugin","version":"1.0.0","entry":"main.js"}"#)
            .unwrap();
        let withdrawn = update_plugin("gone-plugin", project_path).unwrap_err();
        assert!(withdrawn.contains("not in this build or the catalog"), "{withdrawn}");
    }

    #[test]
    fn test_uninstall_plugin_removes_directory() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();

        // Create installed plugin
        let plugin_dir = temp.path().join(".moss").join("plugins").join("test-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("manifest.json"), r#"{"name":"test-plugin"}"#).unwrap();
        fs::write(plugin_dir.join("main.js"), "// plugin code").unwrap();

        // Verify it exists
        assert!(plugin_dir.exists());

        // Uninstall
        let result = uninstall_plugin("test-plugin", project_path);
        assert!(result.is_ok(), "Uninstall should succeed: {:?}", result.err());

        // Verify it's gone
        assert!(!plugin_dir.exists(), "Plugin directory should be removed");
    }

    #[test]
    fn test_uninstall_plugin_not_installed() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();

        // Create .moss/plugins/ but don't install the plugin
        fs::create_dir_all(temp.path().join(".moss").join("plugins")).unwrap();

        let result = uninstall_plugin("not-installed", project_path);
        assert!(result.is_err(), "Should fail when plugin not installed");
        assert!(result.unwrap_err().contains("not installed"));
    }

    #[test]
    fn test_uninstall_plugin_removes_from_hooks() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();

        // Create installed plugin
        let plugin_dir = temp.path().join(".moss").join("plugins").join("test-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("manifest.json"), r#"{"name":"test-plugin"}"#).unwrap();

        // Create config.toml with plugin as an installed channel
        let config_path = temp.path().join(".moss").join("config.toml");
        fs::write(&config_path, &format!(r#"
schema_version = {}

[channels.test-plugin]
[channels.other-plugin]
# keep me
"#, crate::config::migrations::CURRENT_VERSION)).unwrap();

        // Uninstall
        let result = uninstall_plugin("test-plugin", project_path);
        assert!(result.is_ok(), "Uninstall should succeed");

        // Verify channel entry was removed, and the user's comment was not
        let config_content = fs::read_to_string(&config_path).unwrap();
        assert!(!config_content.contains("test-plugin"), "Plugin should be removed from channels");
        assert!(config_content.contains("other-plugin"), "Other channels should remain");
        assert!(config_content.contains("# keep me"), "a hand-written comment must survive: {config_content}");
    }

    #[test]
    fn test_uninstall_plugin_removes_from_all_hook_types() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();

        // Create installed plugin
        let plugin_dir = temp.path().join(".moss").join("plugins").join("test-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("manifest.json"), r#"{"name":"test-plugin"}"#).unwrap();

        // Create config.toml with plugin in multiple hook types
        let config_path = temp.path().join(".moss").join("config.toml");
        fs::write(&config_path, &format!(r#"
schema_version = {}

[hooks]
process = ["test-plugin", "other-processor"]
deploy = "test-plugin"

[channels.other-syndicator]
"#, crate::config::migrations::CURRENT_VERSION)).unwrap();

        // Uninstall
        let result = uninstall_plugin("test-plugin", project_path);
        assert!(result.is_ok(), "Uninstall should succeed");

        // Verify all hooks were cleaned
        let config_content = fs::read_to_string(&config_path).unwrap();
        assert!(!config_content.contains("test-plugin"), "Plugin should be removed from all hooks");
        assert!(config_content.contains("other-processor"), "Other process plugins should remain");
        assert!(config_content.contains("other-syndicator"), "Other syndicators should remain");
    }

    #[test]
    fn test_uninstall_plugin_removes_string_valued_hook() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();

        // Create installed plugin
        let plugin_dir = temp.path().join(".moss").join("plugins").join("test-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("manifest.json"), r#"{"name":"test-plugin"}"#).unwrap();

        // `[hooks] deploy = "<id>"` is the single-string shape get_deploy_plugin
        // resolves with priority 1 — leaving it dangling after uninstall keeps
        // routing deploys to a plugin that no longer exists.
        let config_path = temp.path().join(".moss").join("config.toml");
        fs::write(&config_path, &format!(r#"
schema_version = {}

[hooks]
deploy = "test-plugin"
process = ["other-plugin"]
"#, crate::config::migrations::CURRENT_VERSION)).unwrap();

        let result = uninstall_plugin("test-plugin", project_path);
        assert!(result.is_ok(), "Uninstall should succeed");

        let config_content = fs::read_to_string(&config_path).unwrap();
        assert!(!config_content.contains("test-plugin"), "String-valued deploy hook should be scrubbed");
        assert!(config_content.contains("other-plugin"), "Unrelated hooks should remain");
    }

    #[test]
    fn test_uninstall_plugin_keeps_string_hook_naming_other_plugin() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();

        let plugin_dir = temp.path().join(".moss").join("plugins").join("test-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("manifest.json"), r#"{"name":"test-plugin"}"#).unwrap();

        let config_path = temp.path().join(".moss").join("config.toml");
        fs::write(&config_path, &format!(r#"
schema_version = {}

[hooks]
deploy = "other-deployer"
"#, crate::config::migrations::CURRENT_VERSION)).unwrap();

        let result = uninstall_plugin("test-plugin", project_path);
        assert!(result.is_ok(), "Uninstall should succeed");

        let config_content = fs::read_to_string(&config_path).unwrap();
        assert!(config_content.contains("other-deployer"), "String hook naming a different plugin must remain");
    }

    // ================================================================
    // Installed plugin icon tests
    // ================================================================

    fn write_installed_plugin(temp: &std::path::Path, id: &str, icon: Option<(&str, &str)>) {
        let plugin_dir = temp.join(".moss").join("plugins").join(id);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("manifest.json"),
            format!(r#"{{"name":"{}","version":"1.0.0","entry":"main.bundle.js"}}"#, id),
        )
        .unwrap();
        if let Some((filename, content)) = icon {
            fs::write(plugin_dir.join(filename), content).unwrap();
        }
    }

    #[test]
    fn test_get_installed_plugin_icon_reads_svg() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        // Geometry, not a placeholder: an installed icon is now cleaned
        // unless moss shipped it, and markup with nothing to draw is refused.
        let icon_svg = r#"<svg viewBox="0 0 1 1"><path d="M0 0h1"/></svg>"#;
        write_installed_plugin(temp.path(), "test-plugin", Some(("icon.svg", icon_svg)));

        let icon = get_installed_plugin_icon(project_path, "test-plugin").unwrap();
        assert!(icon.contains(r#"d="M0 0h1""#), "the installed drawing is served: {icon}");
    }

    #[test]
    fn test_get_installed_plugin_icon_ignores_non_svg() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        write_installed_plugin(temp.path(), "test-plugin", Some(("icon.png", "png-bytes")));

        assert!(get_installed_plugin_icon(project_path, "test-plugin").is_none());
    }

    #[test]
    fn test_get_installed_plugin_icon_missing_plugin() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        fs::create_dir_all(temp.path().join(".moss")).unwrap();

        assert!(get_installed_plugin_icon(project_path, "nope").is_none());
    }

    #[test]
    fn test_get_installed_plugin_icon_manifest_declared_path() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        let plugin_dir = temp.path().join(".moss").join("plugins").join("test-plugin");
        fs::create_dir_all(plugin_dir.join("assets")).unwrap();
        fs::write(
            plugin_dir.join("manifest.json"),
            r#"{"name":"test-plugin","version":"1.0.0","entry":"main.bundle.js","icon":"assets/logo.svg"}"#,
        )
        .unwrap();
        // No convention-named icon at the plugin root — only the declared path.
        fs::write(
            plugin_dir.join("assets").join("logo.svg"),
            r#"<svg viewBox="0 0 2 2"><path d="M0 0h2"/></svg>"#,
        )
        .unwrap();

        let icon = get_installed_plugin_icon(project_path, "test-plugin").unwrap();
        assert!(icon.contains(r#"d="M0 0h2""#), "the declared path is read: {icon}");
    }

    // ================================================================
    // Channel icon resolution precedence (builtin → installed → bundled)
    // ================================================================

    #[test]
    fn test_resolve_channel_icon_builtin_wins_over_installed() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        // A rogue installed dir shadowing the builtin id must NOT win.
        write_installed_plugin(
            temp.path(),
            "email",
            Some(("icon.svg", r#"<svg viewBox="0 0 1 1"><path d="M0 0h1"/></svg>"#)),
        );

        let icon = resolve_channel_icon(Some(project_path), "email").unwrap();
        assert_eq!(icon, get_builtin_channel_icon("email").unwrap());
    }

    /// The folder wins for an id the binary does not carry, and only then.
    #[test]
    fn an_installed_icon_wins_unless_the_binary_carries_that_id() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        let drawing = r#"<svg viewBox="0 0 3 3"><path d="M0 0h3"/></svg>"#;

        // Nothing in the binary answers for a sideloaded plugin, so its own
        // icon is the only one there is.
        write_installed_plugin(temp.path(), "sideloaded", Some(("icon.svg", drawing)));
        let icon = resolve_channel_icon(Some(project_path), "sideloaded").unwrap();
        assert!(icon.contains(r#"d="M0 0h3""#), "the installed copy wins: {icon}");

        // For an id moss ships, the binary answers whatever the folder holds:
        // the folder must not speak for moss, and moss's own logos are drawn
        // with `<style>` and `class`, which the stranger allowlist strips.
        write_installed_plugin(temp.path(), "github", Some(("icon.svg", drawing)));
        assert_eq!(
            resolve_channel_icon(Some(project_path), "github"),
            crate::plugins::bundled::get_bundled_plugin_icon("github"),
            "a bundled id is answered from the binary"
        );
        // …and the branch that says so is the ORDER, not a carve-out inside
        // the installed reader: that function still answers for the folder.
        assert!(
            get_installed_plugin_icon(project_path, "github")
                .is_some_and(|icon| icon.contains(r#"d="M0 0h3""#)),
            "the installed reader reads the installed copy"
        );
    }

    #[test]
    fn test_resolve_channel_icon_falls_back_to_bundled() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        fs::create_dir_all(temp.path().join(".moss")).unwrap();

        let icon = resolve_channel_icon(Some(project_path), "github");
        assert!(icon.is_some(), "bundled github icon should resolve when nothing is installed");
        assert!(icon.unwrap().contains("<svg"));
    }

    #[test]
    fn test_resolve_channel_icon_unknown_and_no_project() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        fs::create_dir_all(temp.path().join(".moss")).unwrap();

        assert!(resolve_channel_icon(Some(project_path), "nope").is_none());
        // No project open: builtin and bundled still resolve; installed is skipped.
        assert!(resolve_channel_icon(None, "email").is_some());
        assert!(resolve_channel_icon(None, "github").is_some());
        assert!(resolve_channel_icon(None, "nope").is_none());
    }

    // ================================================================
    // Built-in channel tests
    // ================================================================

    #[test]
    fn test_get_builtin_channel_ids() {
        let ids = get_builtin_channel_ids();
        assert!(ids.contains(&"email"), "email should be a built-in channel");
    }

    /// NORTH-STAR pattern 2: the registration table carries a totality test
    /// beside it. Every surface (rows, ids, icons) derives from
    /// BUILTIN_CHANNELS, and every row fully resolves.
    #[test]
    fn manifest_stack_declaration_drives_requires_stack() {
        // requires_stack flows from the plugin's own manifest declaration,
        // not a registry id-list.
        let plugins = get_available_plugins();
        let onionpress = plugins.iter().find(|p| p.id == "onionpress").unwrap();
        assert!(onionpress.requires_stack, "onionpress declares requires_stack");
        let matters = plugins.iter().find(|p| p.id == "matters").unwrap();
        assert!(!matters.requires_stack, "matters declares no stack");
    }

    fn row_revoked(preview: bool, installed: bool) -> AvailablePlugin {
        row_with(preview, installed, Some("leaked reader email addresses"))
    }

    /// The icon of an installed plugin is markup from whoever published it,
    /// and it lands in the privileged webview via `innerHTML`.
    ///
    /// Both halves in one test because they are one rule: a stranger's icon is
    /// cleaned, and moss's own is not — and the second half is what stops the
    /// first from silently repainting the channels the user already has, since
    /// moss's icons use `<style>`, `class` and an embedded raster.
    #[test]
    fn an_installed_icon_is_cleaned_unless_moss_shipped_the_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().to_str().unwrap().to_string();
        let write = |id: &str, version: &str, icon: &str| {
            let dir = tmp.path().join(".moss").join("plugins").join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("manifest.json"),
                format!(r#"{{"name":"{id}","version":"{version}","entry":"index.js"}}"#),
            )
            .unwrap();
            std::fs::write(dir.join("icon.svg"), icon).unwrap();
        };

        write(
            "stranger",
            "1.0.0",
            r#"<svg viewBox="0 0 24 24" onload="alert(1)"><path d="M1 1h2"/>
               <image href="https://tracker.invalid/px.gif"/></svg>"#,
        );
        let cleaned = get_installed_plugin_icon(&vault, "stranger").expect("the drawing survives");
        assert!(!cleaned.contains("onload"), "{cleaned}");
        assert!(!cleaned.contains("tracker.invalid"), "{cleaned}");

        // And moss's own bytes are never taken from the folder — at the
        // bundled version or any other. That rule lives in the resolution
        // ORDER, so this asks the function every surface asks: a hostile
        // `icon.svg` beside a `manifest.json` claiming to be matters was
        // otherwise `<svg onload>` in the window that can call every Tauri
        // command.
        let matters = crate::plugins::bundled::get_bundled_plugin_info("matters")
            .expect("matters is bundled");
        let impostor = r#"<svg viewBox="0 0 1 1" onload="alert(1)"><path d="M0 0h1"/></svg>"#;
        for version in [matters.version.as_str(), "99.9.9"] {
            write("matters", version, impostor);
            let served =
                resolve_channel_icon(Some(&vault), "matters").expect("an icon is still served");
            assert!(!served.contains("onload"), "the folder does not speak for the binary");
            assert_eq!(
                Some(served),
                crate::plugins::bundled::get_bundled_plugin_icon("matters"),
                "a bundled id is answered from the binary at version {version}"
            );
        }
    }

    #[test]
    fn preview_channels_are_hidden_until_the_user_opts_in() {
        // Unready while the user hasn't asked for unfinished work.
        assert!(!channel_is_listed(&row(true, false), false));
        assert!(channel_is_listed(&row(true, false), true));

        // An installed copy (dev sideload) stays visible and manageable.
        assert!(channel_is_listed(&row(true, true), false));

        // Everything else is always listed.
        assert!(channel_is_listed(&row(false, false), false));

        // And a refusal outranks every reason to hide it: a user whose plugin
        // just stopped working has to be told why, and that is exactly the
        // user for whom the readiness rule says "hide it".
        assert!(channel_is_listed(&row_revoked(true, false), false));
    }

    #[test]
    fn manifest_preview_declaration_drives_catalog_visibility() {
        // The readiness bit flows from the plugin's own manifest, not from a
        // registry-side id list. Every bundled plugin declares it today, so the
        // counter-case is a built-in channel, which carries no manifest and must
        // not inherit a preview default.
        let plugins = get_available_plugins();
        for id in ["github", "onionpress", "matters"] {
            assert!(
                plugins.iter().find(|p| p.id == id).unwrap().preview,
                "{id} declares preview in its manifest"
            );
        }
        assert!(!get_builtin_channels().iter().any(|c| c.preview));
    }

    #[test]
    fn test_builtin_channel_table_totality() {
        let rows = get_builtin_channels();
        let ids = get_builtin_channel_ids();
        assert_eq!(BUILTIN_CHANNELS.len(), rows.len());
        assert_eq!(BUILTIN_CHANNELS.len(), ids.len());

        let mut seen = std::collections::HashSet::new();
        for row in &rows {
            assert!(seen.insert(row.id.clone()), "duplicate channel id '{}'", row.id);
            assert!(ids.contains(&row.id.as_str()), "id '{}' missing from ids", row.id);
            assert!(!row.description.is_empty(), "'{}' needs a description", row.id);
            assert!(!row.capabilities.is_empty(), "'{}' needs capabilities", row.id);
            assert_eq!(row.source, ChannelSource::Builtin);
            let icon = get_builtin_channel_icon(&row.id)
                .unwrap_or_else(|| panic!("'{}' must resolve an icon", row.id));
            assert!(icon.contains("<svg"), "'{}' icon must be SVG", row.id);
        }
    }

    #[test]
    fn test_get_builtin_channels_returns_email() {
        let channels = get_builtin_channels();
        assert!(!channels.is_empty(), "Should have at least one built-in channel");

        let email = channels.iter().find(|c| c.id == "email");
        assert!(email.is_some(), "email channel should exist");

        let email = email.unwrap();
        assert_eq!(email.source, ChannelSource::Builtin);
        assert!(email.capabilities.contains(&"syndicate".to_string()));
        assert!(!email.installed, "Should default to not installed");
    }

    #[test]
    fn test_get_builtin_channel_icon_known() {
        let icon = get_builtin_channel_icon("email");
        assert!(icon.is_some(), "email should have an icon");
        assert!(icon.unwrap().contains("<svg"), "Icon should be SVG");
    }

    #[test]
    fn test_get_builtin_channel_icon_unknown() {
        let icon = get_builtin_channel_icon("nonexistent");
        assert!(icon.is_none(), "Unknown channel should return None");
    }

    #[test]
    fn test_get_available_channels_includes_builtin() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        fs::create_dir_all(temp.path().join(".moss")).unwrap();

        let channels = get_available_channels_with_status(project_path, false);

        // Should include at least the email built-in channel
        let email = channels.iter().find(|c| c.id == "email");
        assert!(email.is_some(), "Channels should include built-in email");
        assert_eq!(email.unwrap().source, ChannelSource::Builtin);
    }

    #[test]
    fn test_get_available_channels_email_installed_when_in_hooks() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        let moss_dir = temp.path().join(".moss");
        fs::create_dir_all(&moss_dir).unwrap();

        // Write config with email as an installed channel
        let config_path = moss_dir.join("config.toml");
        fs::write(&config_path, &format!(r#"
schema_version = {}

[channels.email]
"#, crate::config::migrations::CURRENT_VERSION)).unwrap();

        let channels = get_available_channels_with_status(project_path, false);
        let email = channels.iter().find(|c| c.id == "email").unwrap();
        assert!(email.installed, "Email should be marked as installed when [channels.email] exists");
    }

    #[test]
    fn test_get_available_channels_email_not_installed_when_absent() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        let moss_dir = temp.path().join(".moss");
        fs::create_dir_all(&moss_dir).unwrap();

        // Write config with matters channel only (no email)
        let config_path = moss_dir.join("config.toml");
        fs::write(&config_path, &format!(r#"
schema_version = {}

[channels.matters]
"#, crate::config::migrations::CURRENT_VERSION)).unwrap();

        let channels = get_available_channels_with_status(project_path, false);
        let email = channels.iter().find(|c| c.id == "email").unwrap();
        assert!(!email.installed, "Email should NOT be installed when no [channels.email] exists");
    }

    #[test]
    fn test_get_available_channels_hides_preview_until_installed() {
        // Roster policy: capability never hides a row (that's the entry-point
        // filter's job), but a plugin whose own manifest declares `preview`
        // is offered to no one by default, while an actually-installed copy
        // (dev sideload) stays visible and manageable.
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        fs::create_dir_all(temp.path().join(".moss")).unwrap();

        let channels = get_available_channels_with_status(project_path, false);
        for id in ["github", "matters", "onionpress"] {
            assert!(
                !channels.iter().any(|c| c.id == id),
                "preview {id} must not be offered while uninstalled"
            );
        }
        assert!(channels.iter().any(|c| c.id == "email"), "builtin still present");

        // …and every one of them IS offered once the user opts in, so the
        // manifest declaration is the whole of the difference.
        let opted_in = get_available_channels_with_status(project_path, true);
        for id in ["github", "matters", "onionpress"] {
            assert!(
                opted_in.iter().any(|c| c.id == id),
                "{id} must be offered once preview features are on"
            );
        }
    }

    #[test]
    fn test_get_available_channels_shows_preview_when_installed() {
        let temp = tempdir().unwrap();
        let project_path = temp.path().to_str().unwrap();
        write_installed_plugin(temp.path(), "github", Some(("icon.svg", "<svg/>")));

        let channels = get_available_channels_with_status(project_path, false);
        let github = channels.iter().find(|c| c.id == "github");
        assert!(github.is_some(), "an installed copy is real state and stays manageable");
        assert!(github.unwrap().installed);
    }

    /// One test, where there were two that built the same struct to assert
    /// two of its fields. The round trip is what matters — the frontend
    /// receives this JSON — so it is asserted over the whole row.
    #[test]
    fn a_catalog_row_survives_the_round_trip_to_the_frontend() {
        let plugin = AvailablePlugin {
            id: "email".to_string(),
            version: String::new(),
            description: "Send articles to email subscribers".to_string(),
            icon_url: None,
            capabilities: vec!["syndicate".to_string()],
            installed: true,
            source: ChannelSource::Builtin,
            requires_stack: false,
            preview: false,
            refusal: None,
            update_available: Some("2.0.0".to_string()),
            origin: PluginOrigin::Bundled,
            display_name: None,
        };

        let json = serde_json::to_string(&plugin).unwrap();
        assert!(json.contains("\"source\":\"builtin\""));

        let deserialized: AvailablePlugin = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.source, ChannelSource::Builtin);
        assert_eq!(deserialized.update_available.as_deref(), Some("2.0.0"));
    }
}
