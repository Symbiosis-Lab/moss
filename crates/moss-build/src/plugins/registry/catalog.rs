//! Where the published catalog meets the local one.
//!
//! One row per plugin the user could have, assembled from two sources that
//! disagree about almost everything: the bundled list is in the binary and
//! always installable, the registry's is fetched, versioned, and can withdraw
//! or outrun what it publishes. Three functions, run in this order, and the
//! order is the design:
//!
//! 1. [`merge_registry_rows`] — every published id that moss does not ship
//!    gets a row. Nothing is judged here.
//! 2. [`add_rows_for_the_folder`] — so does every plugin only the folder
//!    knows. Nothing is judged here either.
//! 3. *(the caller reads the folder)* — which rows are actually installed.
//! 4. [`drop_offers_this_moss_would_refuse`] — a row that is only an offer,
//!    and an offer this moss would refuse, stops being a row.
//! 5. [`annotate_available_updates`] — what is left learns whether something
//!    newer is published.
//!
//! Step 4 cannot move earlier, which is the whole reason this is four
//! functions instead of one: whether a row is an *offer* depends on whether
//! the user already has the plugin, and nothing knows that until step 3.

use std::path::Path;

use super::{default_plugin_source, AvailablePlugin, PluginOrigin};
use crate::plugins::install::registry_client::enforce;
use crate::plugins::install::registry_client::index::IndexEntry;
use moss_core::untrusted_text::{bounded, MAX_SENTENCE};

/// A row for every plugin in the folder that neither moss nor the registry
/// knows: a dev sideload, or whatever a shared folder brought with it.
///
/// Before the consent gate these had no row — the catalog was bundled ∪
/// published, and a folder-only plugin simply ran. Now it does not run until
/// the user allows it, and the tile is where they do that, so a plugin with
/// no tile would be refused with no remedy on any screen. The id is the
/// directory's name, because that is what the loader is handed; `installed`
/// is left for the read that follows, so one place decides it for every row.
pub(super) fn add_rows_for_the_folder(plugins: &mut Vec<AvailablePlugin>, plugins_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(plugins_dir) else { return };
    let mut found: Vec<(String, crate::plugins::PluginManifest)> = entries
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_str()?.to_string();
            if plugins.iter().any(|p| p.id == id) {
                return None;
            }
            let manifest = crate::plugins::bundled::read_installed_manifest(&entry.path())?;
            Some((id, manifest))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, manifest) in found {
        plugins.push(AvailablePlugin {
            id,
            version: manifest.version.clone(),
            description: String::new(),
            icon_url: None,
            capabilities: manifest.capabilities.iter().map(|c| c.hook_name().to_string()).collect(),
            installed: false,
            source: default_plugin_source(),
            requires_stack: manifest.needs_stack(),
            preview: manifest.preview,
            refusal: None,
            update_available: None,
            origin: PluginOrigin::Sideloaded,
            display_name: None,
        });
        // Name, description and origin are the folder's own, decided by the
        // same rule as for a folder directory that shadows a row moss or the
        // registry already built.
        identity_follows_the_bytes(plugins.last_mut().expect("just pushed"), &manifest);
    }
}

/// Rewrite a row to describe the bytes in the folder rather than the plugin
/// whose id they borrowed.
///
/// Rows merge by id and the registry's are merged first, so `.moss/plugins/x/`
/// never gets a row of its own when moss ships an `x` or the registry
/// publishes one: it inherits that row's display name, description and origin,
/// and only `installed`, `version` and `refusal` are re-read from disk. The
/// tile then names someone else's plugin over code nobody checked — and the
/// consent question it asks ("this plugin is installed in this folder, do you
/// trust it?") is answered against that borrowed name.
///
/// So the identity follows whatever vouches for the bytes
/// ([`enforce::vouched_for`]): nothing does, and the row says so — the
/// manifest's own name, its own description, and a sideloaded origin. Bounded
/// and stripped like every other name a folder wrote.
pub(super) fn identity_follows_the_bytes(row: &mut AvailablePlugin, installed: &crate::plugins::PluginManifest) {
    row.origin = PluginOrigin::Sideloaded;
    row.display_name = Some(super::plugin_human_name(installed));
    row.description = bounded(installed.description.as_deref().unwrap_or_default(), MAX_SENTENCE);
}

/// Tell each row whether the registry publishes a newer version it could take.
///
/// Only annotates ids that are already rows, which since the catalog merge is
/// every published id this moss could take. It stays an annotation rather than
/// a second place rows are created: an id that has no row by now is one
/// [`drop_offers_this_moss_would_refuse`] deliberately removed, and an update
/// badge is not a reason to put it back.
///
/// Only an installed row is annotated, and this is the only place that rule is
/// applied. `installed` is true exactly when `.moss/plugins/<id>` holds a
/// manifest, which is the same file `update_plugin` reads to learn what to
/// measure the published version against — so annotating anything else would
/// badge a row whose update cannot run. A row with nothing on disk is not
/// missing an update; its own click is the install.
///
/// Not an origin test. `origin` says where the *row* was built from, not where
/// the bytes in the folder came from: a hand-installed copy of an id moss also
/// bundles still sits on a bundled row, and it is exactly the copy an update
/// can replace. Refusing at the take instead of here would be a promise moss
/// breaks after the click.
pub(super) fn annotate_available_updates(plugins: &mut [AvailablePlugin], entries: Vec<IndexEntry>) {
    for entry in entries {
        if let Some(row) = plugins.iter_mut().find(|p| p.id == entry.id && p.installed) {
            row.update_available = offerable_version(&row.version, &entry);
        }
    }
}

/// The entry's version when it is newer than what is here and this moss would
/// accept it.
///
/// Asks the same two questions install and the loader ask, because an update
/// offer is a promise that installing it will work. The floor test is not
/// hypothetical: github's live entry is 1.6.0 needing moss 0.11.7, and a
/// 0.11.6 app that offered it would install a plugin its own loader refuses.
/// Neither is the kill list — the index and `revoked.json` carry independent
/// serials, so the latest entry for an id can be a version that was withdrawn
/// after it was published.
///
/// Versions that do not parse are not compared. A registry entry that cannot
/// say which of two versions is newer has no business claiming one is.
pub(super) fn offerable_version(have: &str, entry: &IndexEntry) -> Option<String> {
    let newer = crate::plugins::install::version_update_decision(have, &entry.version, true);
    (newer == crate::plugins::install::VersionDecision::Update && this_moss_would_take(entry))
        .then(|| entry.version.clone())
}

/// Would installing this exact published version work?
///
/// The floor and the kill list, in the order they are published: an entry may
/// be withdrawn after it was published, and the two documents carry
/// independent serials, so neither answers for the other. Asked before every
/// offer moss makes — the update badge and the catalog tile alike — because
/// an offer is a promise that taking it will work, and there is no honest way
/// for the two to disagree.
fn this_moss_would_take(entry: &IndexEntry) -> bool {
    enforce::host_meets_floor(entry.min_moss_version.as_deref())
        && enforce::revoked_reason(&entry.id, &entry.version).is_none()
}

/// Remove the rows that offer something this moss would then refuse to
/// install.
///
/// Separate from [`merge_registry_rows`] and run after the installed read,
/// because the two ask different questions. Merge asks "does the registry
/// publish this"; this asks "is this row an offer, and would taking it work" —
/// and nothing can answer the first half of that until the folder has been
/// looked at.
///
/// Anything installed is kept whatever the registry now says about it. The
/// catalog is the only surface that can uninstall, so a plugin that outran its
/// host or was withdrawn is precisely the one a user needs to reach: dropping
/// its row would leave it loading, or refused, with nothing on screen to act
/// on. Bundled rows are kept for the reason the floor and the kill list
/// already exempt them — moss ships those bytes.
///
/// What makes a row an offer worth drawing is [`this_moss_would_take`] — the
/// same question [`offerable_version`] asks before offering an update, asked
/// of the index entry rather than of the row, so this no longer depends on an
/// earlier pass having written the row's refusal first.
pub(super) fn drop_offers_this_moss_would_refuse(plugins: &mut Vec<AvailablePlugin>, entries: &[IndexEntry]) {
    plugins.retain(|row| {
        row.installed
            || row.origin != PluginOrigin::Registry
            || entries.iter().find(|e| e.id == row.id).is_some_and(this_moss_would_take)
    });
}

/// Append a row for every published plugin moss does not ship.
///
/// Bundled wins on a shared id, and not as a tie-break: the bundled bytes are
/// in the binary, so they install offline and are exempt from the floor and
/// the kill list. A registry row for the same id would offer a second, worse
/// way to get the same plugin. The registry entry still reaches that row —
/// [`annotate_available_updates`] reads it for the update offer — so nothing
/// is lost by not drawing it twice.
///
/// Nothing is judged here beyond that. Whether a row survives is
/// [`drop_offers_this_moss_would_refuse`]'s question, asked after the
/// installed read — a plugin this moss could not install is still one the user
/// may have, and its tile is the only place to uninstall it from.
pub(super) fn merge_registry_rows(plugins: &mut Vec<AvailablePlugin>, entries: &[IndexEntry]) {
    for entry in entries {
        if plugins.iter().any(|p| p.id == entry.id) {
            continue;
        }
        plugins.push(AvailablePlugin {
            id: entry.id.clone(),
            // The version the user would get. Overwritten by the installed
            // read below when this folder already has a copy, so the row keeps
            // describing what is here rather than what is published.
            version: entry.version.clone(),
            description: bounded(&entry.description, MAX_SENTENCE),
            // Deliberately not `entry.icon_url`: the frontend never fetches an
            // icon (the one-icon-path rule), and a populated field here is an
            // invitation for it to start. Icons reach the row through
            // `resolve_channel_icon`, which serves the cached copy the backend
            // downloaded and sanitized.
            icon_url: None,
            capabilities: entry.capabilities.clone(),
            installed: false,
            source: default_plugin_source(),
            requires_stack: entry.requires_stack,
            preview: entry.preview,
            refusal: None,
            update_available: None,
            origin: PluginOrigin::Registry,
            // A plugin moss does not ship can have no `channels.name.<id>`
            // string, so the registry's own name is the only name there is.
            display_name: super::published_name(&entry.id, Some(&entry.display_name)),
        });
    }
}

#[cfg(test)]
/// The catalog row every test in this module and in `registry.rs` starts from.
///
/// Deserialized rather than a struct literal so the row leans on the same
/// serde defaults a real catalog row does (source defaults to plugin; the
/// verdict must not depend on where the row came from).
pub(super) fn row_with(preview: bool, installed: bool, revoked: Option<&str>) -> AvailablePlugin {
    serde_json::from_value(serde_json::json!({
        "id": "x",
        "version": "1.0.0",
        "name": "x",
        "description": "",
        "icon_url": null,
        "capabilities": [],
        "installed": installed,
        "preview": preview,
        "refusal": revoked.map(|reason| serde_json::json!({"kind": "revoked", "reason": reason})),
    }))
    .unwrap()
}

/// [`row_with`] with no refusal — the shape most tests want.
#[cfg(test)]
pub(super) fn row(preview: bool, installed: bool) -> AvailablePlugin {
    row_with(preview, installed, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use moss_core::untrusted_text::MAX_NAME;

    fn entry(id: &str, version: &str, floor: Option<&str>) -> IndexEntry {
        serde_json::from_value(serde_json::json!({
            "type": "plugin",
            "id": id,
            "display_name": id,
            "version": version,
            "download_url": "https://example.invalid/x.zip",
            "sha256": "0".repeat(64),
            "min_moss_version": floor,
        }))
        .unwrap()
    }



    /// The catalog is bundled ∪ published, and the union has one rule.
    ///
    /// Three things at once because they are one behaviour: an id moss ships
    /// is drawn from the bytes in the binary, an id it does not ship is drawn
    /// from the registry, and neither produces two tiles for one plugin.
    #[test]
    fn a_published_plugin_moss_does_not_ship_gets_a_row_and_one_it_ships_does_not_get_a_second() {
        let mut rows = vec![row(false, false)];

        merge_registry_rows(
            &mut rows,
            &[entry("x", "2.0.0", None), entry("stranger", "1.4.0", None)],
        );

        assert_eq!(rows.len(), 2, "the shared id is one row, the new id adds one");
        assert!(rows[0].display_name.is_none(), "bundled wins: the registry does not get to name a row moss ships");
        assert_eq!(rows[0].origin, PluginOrigin::Bundled);

        let published = &rows[1];
        assert_eq!(published.id, "stranger");
        assert_eq!(published.origin, PluginOrigin::Registry, "and it says where it comes from");
        assert_eq!(published.version, "1.4.0", "the version the user would get");
        assert!(!published.installed, "which is not the same as having it");
        assert!(
            published.icon_url.is_none(),
            "the frontend never fetches an icon; the cached copy reaches it through \
             resolve_channel_icon"
        );
    }


    /// The registry's row, wearing the folder's bytes.
    ///
    /// `.moss/plugins/matters/` gets no row of its own — [`merge_registry_rows`]
    /// ran first and [`add_rows_for_the_folder`] skips an id that already has
    /// one — so before this rule the tile offered the registry's name and
    /// description over code nobody checked, and the consent question asked
    /// whether the user trusts *that* plugin.
    #[test]
    fn a_row_worn_by_unvouched_bytes_is_named_by_them() {
        let mut rows = Vec::new();
        merge_registry_rows(&mut rows, &[entry("stranger", "1.4.0", None)]);

        let squatter = crate::plugins::PluginManifest::parse(
            r#"{"name":"stranger","version":"0.0.1","entry":"index.js","description":"whatever came with the folder"}"#,
        )
        .unwrap();
        identity_follows_the_bytes(&mut rows[0], &squatter);

        assert_eq!(rows[0].origin, PluginOrigin::Sideloaded, "nothing vouches for these bytes");
        assert_eq!(rows[0].display_name.as_deref(), Some("Stranger"), "the manifest's name, not the registry's");
        assert_eq!(rows[0].description, "whatever came with the folder");
        assert_eq!(rows[0].id, "stranger", "the id is the directory, and install/uninstall still need it");
    }

    /// A tile whose install button cannot work is worse than no tile.
    ///
    /// The floor is asked here for the same reason `offerable_version` asks it
    /// for updates: an offer is a promise that taking it will work, and this
    /// one would end at the refusal `load_plugin` raises.
    /// What a row offers, and what a row *is*, are different questions.
    ///
    /// One table because the mistake is always the same shape — deciding a
    /// row's fate from the registry alone, without asking whether the user
    /// already has the thing. The kill list's half of the same question needs
    /// a published `revoked.json` and lives in
    /// `tests/plugin_catalog_composition_test.rs`.
    #[test]
    fn a_row_this_moss_would_refuse_to_install_is_dropped_unless_it_is_already_here() {
        let unrunnable = [entry("stranger", "2.0.0", Some("99.0.0"))];
        let fine = [entry("stranger", "1.0.0", Some("0.0.1"))];

        let mut rows = Vec::new();
        merge_registry_rows(&mut rows, &unrunnable);
        assert_eq!(rows.len(), 1, "merge builds every published row");
        drop_offers_this_moss_would_refuse(&mut rows, &unrunnable);
        assert!(rows.is_empty(), "an install button that would refuse draws no tile");

        let mut rows = Vec::new();
        merge_registry_rows(&mut rows, &fine);
        drop_offers_this_moss_would_refuse(&mut rows, &fine);
        assert_eq!(rows.len(), 1, "a floor this moss meets is not a reason to hide it");

        // The case the first version of this got wrong: the user HAS it, and
        // the catalog is the only place they can remove it from.
        let mut rows = Vec::new();
        merge_registry_rows(&mut rows, &unrunnable);
        rows[0].installed = true;
        drop_offers_this_moss_would_refuse(&mut rows, &unrunnable);
        assert_eq!(rows.len(), 1, "an installed plugin keeps its tile whatever the index says");

        // A bundled row is never dropped, whatever the index says: moss ships
        // those bytes, and the floor and the kill list already exempt them.
        let mut rows = vec![row(false, false)];
        drop_offers_this_moss_would_refuse(&mut rows, &unrunnable);
        assert_eq!(rows.len(), 1);
    }


    #[test]
    fn a_newer_published_version_becomes_the_rows_update_and_nothing_else_does() {
        let mut rows = vec![row_with(false, true, None)];

        annotate_available_updates(&mut rows, vec![entry("x", "2.0.0", None)]);

        assert_eq!(rows.len(), 1, "an id moss ships is one row, not two");
        assert!(rows[0].display_name.is_none(), "identity comes from the bytes that are here");
        assert_eq!(rows[0].version, "1.0.0", "the row still describes what is installed");
        assert_eq!(rows[0].update_available.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn a_row_with_nothing_on_disk_is_never_offered_an_update() {
        // The bundled tile a user has not installed. Its click installs, and
        // an update badge on it would point at a version `update_plugin`
        // cannot take: there is no manifest in the folder to measure against.
        let mut rows = vec![row_with(false, false, None)];

        annotate_available_updates(&mut rows, vec![entry("x", "2.0.0", None)]);

        assert_eq!(
            rows[0].update_available, None,
            "an offer here would badge a row whose update cannot run"
        );
    }


    #[test]
    fn an_update_this_moss_would_refuse_is_not_offered() {
        // github's live entry is exactly this shape: a version newer than the
        // installed one, declaring a floor above the app asking for it.
        let mut rows = vec![row_with(false, true, None)];
        annotate_available_updates(&mut rows, vec![entry("x", "2.0.0", Some("99.0.0"))]);
        assert_eq!(
            rows[0].update_available, None,
            "offering it would install a plugin the loader then refuses"
        );

        // Nothing newer is likewise nothing to offer.
        let mut rows = vec![row_with(false, true, None)];
        annotate_available_updates(&mut rows, vec![entry("x", "0.9.0", None)]);
        assert_eq!(rows[0].update_available, None);
    }

    /// The wiring, not the rule: `moss_core::untrusted_text` owns what
    /// bounding a string means and tests it, so what is left to prove here is
    /// that a row's two published fields go through it, at their own lengths.
    #[test]
    fn a_published_name_and_description_are_bounded_where_they_are_built() {
        let mut rows = Vec::new();
        let mut e = entry("stranger", "1.0.0", None);
        e.display_name = "x".repeat(MAX_NAME + 10);
        e.description = "。".repeat(MAX_SENTENCE + 5);
        merge_registry_rows(&mut rows, &[e]);

        assert_eq!(
            rows[0].display_name.as_deref().unwrap().chars().count(),
            MAX_NAME + 1,
            "the published name, at a name's length"
        );
        assert_eq!(
            rows[0].description.chars().count(),
            MAX_SENTENCE + 1,
            "the description, at a sentence's"
        );
    }
}

