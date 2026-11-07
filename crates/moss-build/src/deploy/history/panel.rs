//! The Versions surface's five command bodies, and the plain-data types they
//! answer with.
//!
//! They live here rather than as `#[tauri::command]` functions because the
//! command surface has two carriers, and a body that only the Tauri shell can
//! reach is a feature the HTTP carrier has to re-implement.
//! Nothing below is Tauri-shaped: every carrier reaches them through the
//! command table (`commands::versions`).
//!
//! Every backend rule — `[history] enabled`, the size ceiling, "moss never
//! writes a vault's `.git`", best-effort restore — lives entirely in
//! [`super`]'s store/record/restore/timeline modules; nothing here re-derives
//! one.
//!
//! ## No build in the way
//!
//! Saving, restoring and comparing a version read the site as the disk holds
//! it ([`SourceTree::on_disk`]), never a sealed build: a host with no build
//! running, or one whose build is busy with media, answers at once.

use std::path::Path;

use serde::Serialize;
use specta::Type;

use super::record::PublishRecord;
use super::{restore, HistoryStore, RestoreMode, SourceTree, Trigger};
use crate::build::manifest::change_set::ChangedPage;
use crate::build::manifest::published_record;
use crate::moss_paths::MossPaths;

// ── Wire types ───────────────────────────────────────────────────────────
//
// [`super::Trigger`]/[`super::Row`]/[`super::RestoreReport`] stay pure Rust
// with no specta derive; `ChangedPage`/`PageVerb` already carry one and are
// reused directly (design decision 5, "already in the bindings"). The DTOs
// here are the command boundary's own contract, independent of the store's
// internal shapes.

/// One row in the Versions list — a publish, a manual save, a pre-restore
/// save, or a kept outside edit of one page. `trigger` is
/// `"publish" | "manual" | "restore" | "displaced"`, the same flattened-
/// string convention [`restore_version`]'s `mode` argument uses, so the
/// frontend never imports a second enum shape for the same words.
#[derive(Debug, Clone, Serialize, Type)]
pub struct VersionRow {
    pub id: String,
    pub published_at: String,
    pub target: String,
    pub trigger: String,
    pub label: Option<String>,
    pub changed: u32,
    pub added: u32,
    pub removed: u32,
}

/// [`list_versions`]'s answer. `rows` is the day-grouped list the panel renders
/// for either scope ("This page" → `page_timeline`, "Whole site" with no
/// `path` → `site_timeline`); `changed_since`/`unchanged` are populated ONLY
/// for a site-scope drill-down (`scope: "site"`, `path: Some(<version id>)`)
/// — the "A site version" sub-view's Changed-since/Unchanged groups — and
/// `rows` is empty in that mode. One command rather than two: the drill-down
/// reuses [`HistoryStore::site_version_pages`] in the same body as the list.
#[derive(Debug, Clone, Serialize, Type)]
pub struct VersionList {
    pub rows: Vec<VersionRow>,
    /// The row id whose `generation_id` matches the deploy baseline live for
    /// this project's current target — never the newest row (design rule 7:
    /// "Live" is the baseline's own record, and history's write can lag it).
    pub live_id: Option<String>,
    pub has_multiple_targets: bool,
    /// "Show in Finder" footer target.
    pub store_dir: String,
    pub changed_since: Vec<ChangedPage>,
    pub unchanged: Vec<String>,
}

/// [`read_version`]'s answer. `kept: false` means the bytes at this version
/// were never stored (over the size ceiling, cloud-evicted at publish time,
/// or otherwise unreadable) — the panel shows "not kept" and disables
/// Restore, never a fabricated body. `kept: true, text: None` is a kept
/// binary entry (a media file): moss never ships raw bytes over a command
/// for a history read, only the fact that they exist. A symlink entry's
/// stored bytes are its target string, which decodes as ordinary text.
#[derive(Debug, Clone, Serialize, Type)]
pub struct VersionContent {
    pub kept: bool,
    pub text: Option<String>,
}

/// One path [`restore_version`] could not carry through — a field-named struct
/// rather than a bare `(String, String)` tuple, so the bindings read as
/// `{path, error}` instead of a positional pair.
#[derive(Debug, Clone, Serialize, Type)]
pub struct RestoreFailure {
    pub path: String,
    pub error: String,
}

/// [`restore_version`]'s answer, unified across the page and site forms (design
/// rule 4, "best-effort with an honest count"): a page restore reports itself
/// as a one-entry `restored`/`failed`, matching the shape a site restore's
/// [`HistoryStore::restore_site`] already returns. `undo_id` is the pre-restore
/// backup record's id, when one was actually written — `save_before_restore`
/// dedups against the newest record for the target, so a repeated restore
/// with nothing new to undo writes no record and `undo_id` is `None`; the
/// panel then shows no Undo action rather than one that restores nothing.
///
/// Named for the boundary, not for [`super::RestoreReport`], which is the
/// store's own tuple-shaped report this is translated from.
#[derive(Debug, Clone, Serialize, Type)]
pub struct RestoreReport {
    pub restored: Vec<String>,
    pub failed: Vec<RestoreFailure>,
    pub trashed: Vec<String>,
    pub undo_id: Option<String>,
}

// ── Shared helpers ───────────────────────────────────────────────────────

/// [`Trigger`] as the plain string the wire type carries. Pure and
/// exhaustively matched (no `_ =>` arm) so a future `Trigger` variant is a
/// compile error here, not a silently-dropped row.
pub fn trigger_str(t: Trigger) -> &'static str {
    match t {
        Trigger::Publish => "publish",
        Trigger::Manual => "manual",
        Trigger::Restore => "restore",
        Trigger::Displaced => "displaced",
    }
}

/// [`super::Row`] → [`VersionRow`]. Pure; see `panel_tests.rs`.
pub fn to_version_row(row: super::Row) -> VersionRow {
    VersionRow {
        id: row.id,
        published_at: row.published_at,
        target: row.target,
        trigger: trigger_str(row.trigger).to_string(),
        label: row.label,
        changed: row.changed as u32,
        added: row.added as u32,
        removed: row.removed as u32,
    }
}

/// A freshly-written record (`id`, not yet folded into a day-grouped
/// timeline) → [`VersionRow`], for [`save_version`]'s return — the one row it
/// just created, with no prior record to diff a changed/added/removed count
/// against (a manual save's own row never shows one; see the design's list
/// spec, "Published" rows carry the count, a named save does not).
pub fn version_row_from_record(id: String, record: &PublishRecord) -> VersionRow {
    VersionRow {
        id,
        published_at: record.published_at.clone(),
        target: record.target.clone(),
        trigger: trigger_str(record.trigger).to_string(),
        label: record.label.clone(),
        changed: 0,
        added: 0,
        removed: 0,
    }
}

/// The target a record written now belongs to: the derived deploy method with
/// moss's own hosting spelled out rather than left as `None`.
///
/// Deliberately the app's spelling (`site_config::current_deploy_plugin`, the
/// one resolver behind the Host row), not [`super::cli`]'s `current_target`,
/// which asks `DomainDeploymentConfig::publish_target` and so answers with the
/// `moss:<site_id>` *slot*. The two disagree for any site that has published,
/// which is a real divergence between `moss history --save` and the app's Save
/// — but it predates this module, it decides what a record on disk says, and
/// fixing it is not a transport change. One resolver here means the app and
/// the HTTP carrier cannot add a third disagreement.
fn current_target(vault_root: &Path) -> String {
    crate::build::site_config::current_deploy_plugin(&vault_root.to_string_lossy())
        .unwrap_or_else(|| crate::config::deployment::MOSS_TARGET_ID.to_string())
}

/// Can this site publish to more than one place — moss's own hosting plus at
/// least one installed deploy plugin? The list shows a per-row target only
/// then. The app's `get_deploy_targets` enumerates the same set as a labelled
/// list for its own command; this asks the set's size and nothing else.
fn has_multiple_targets(vault_root: &Path) -> bool {
    use crate::plugins::types::Capability;
    crate::plugins::bundled::discover_plugins(&vault_root.to_string_lossy())
        .map(|plugins| plugins.iter().any(|p| p.manifest.has_capability(&Capability::Deploy)))
        .unwrap_or(false)
}

/// The record whose `generation_id` matches the deploy baseline live for this
/// vault's current target (design rule 7) — `None` before any publish has
/// landed for it, or when the current target's baseline has drifted from every
/// kept record (history's write is best-effort).
fn resolve_live_id(vault_root: &Path, records: &[(String, PublishRecord)]) -> Option<String> {
    let moss_paths = MossPaths::new(vault_root);
    let target = current_target(vault_root);
    let live_generation_id =
        published_record::load_for(&moss_paths, Some(&target)).map(|s| s.generation_id);
    records
        .iter()
        .find(|(_, record)| super::is_live(record, live_generation_id.as_deref()))
        .map(|(id, _)| id.clone())
}

/// A caller-supplied version id, checked against the ids this store actually
/// lists.
///
/// `store::load_record` joins the id into a filename, so `"../../../secrets"`
/// — or an absolute path, which `Path::join` substitutes wholesale — would
/// read outside `publishes/`. Over Tauri IPC that cannot happen: the id is a
/// row moss itself listed. Over the HTTP carrier the id is whatever the
/// request body said, and the check belongs at the one place both carriers
/// pass through rather than in the arm, where the next command to take an id
/// would have to remember it. Every listed id is a `read_dir` stem of
/// `publishes/`, so membership IS the containment check — no second rule to
/// keep in step with the writer's naming.
fn known_id(records: &[(String, PublishRecord)], id: &str) -> Result<(), String> {
    match records.iter().any(|(rid, _)| rid == id) {
        true => Ok(()),
        false => Err(format!("no such version: {id}")),
    }
}

// ── Command bodies ───────────────────────────────────────────────────────

/// The Versions panel's list, for either scope, and the site-version
/// drill-down (see [`VersionList`]'s own doc for the three response shapes).
pub fn list_versions(vault_root: &Path, scope: &str, path: Option<String>) -> Result<VersionList, String> {
    let store = HistoryStore::in_vault(vault_root);
    let records = store.list_records();
    let live_id = resolve_live_id(vault_root, &records);
    let has_multiple_targets = has_multiple_targets(vault_root);
    let store_dir = store.store_dir().to_string_lossy().into_owned();
    let list = |rows: Vec<VersionRow>| VersionList {
        rows,
        live_id: live_id.clone(),
        has_multiple_targets,
        store_dir: store_dir.clone(),
        changed_since: Vec::new(),
        unchanged: Vec::new(),
    };

    match scope {
        "page" => {
            let page_path = path.ok_or_else(|| {
                "list_versions: a page path is required for scope \"page\"".to_string()
            })?;
            Ok(list(store.page_timeline(&page_path).into_iter().map(to_version_row).collect()))
        }
        "site" => match path {
            // The site-version drill-down: one version's pages against the
            // tree on disk now (the Changed-since/Unchanged groups).
            Some(version_id) => {
                known_id(&records, &version_id)?;
                let pages = store.site_version_pages(&version_id, &site_on_disk(vault_root)?)?;
                Ok(VersionList {
                    changed_since: pages.changed_since,
                    unchanged: pages.unchanged,
                    ..list(Vec::new())
                })
            }
            None => Ok(list(store.site_timeline().into_iter().map(to_version_row).collect())),
        },
        other => Err(format!("list_versions: unknown scope \"{other}\"")),
    }
}

/// One version's content at one path: text for a page, or (for a media entry)
/// whether the bytes were kept. See [`VersionContent`]'s own doc.
pub fn read_version(vault_root: &Path, id: &str, path: &str) -> Result<VersionContent, String> {
    let store = HistoryStore::in_vault(vault_root);

    // Validate the (id, path) pair against the record itself first, so a
    // read failure below can only mean "the bytes were not kept" — never
    // "no such version" or "not part of this version", which the store's
    // own `Result<Vec<u8>, String>` cannot distinguish from "not kept".
    let records = store.list_records();
    known_id(&records, id)?;
    let in_this_version = records
        .iter()
        .any(|(rid, record)| rid == id && record.entries.contains_key(path));
    if !in_this_version {
        return Err(format!("{path} is not part of version {id}"));
    }

    match store.read_version(id, path) {
        Ok(bytes) => Ok(VersionContent { kept: true, text: String::from_utf8(bytes).ok() }),
        Err(_) => Ok(VersionContent { kept: false, text: None }),
    }
}

/// Restore a page (when `path` is given) or the whole site (when it is not),
/// both after saving the present as a version first (design rule 3 via
/// `save_before_restore`).
///
/// A site restore's Trash closure is [`restore::real_trash`] — the shared
/// default that joins the record's vault-relative path onto the root before
/// handing it to `vault::fs::delete_entry_inner`, the one delete core
/// `delete_entry` and `batch::delete_entries` already share. The app used to
/// pass its own closure that skipped the join, so every page added since the
/// version failed the delete core's own containment check and was reported as
/// a restore failure instead of being trashed; `panel_tests.rs` pins the join.
pub fn restore_version(vault_root: &Path, id: &str, path: Option<String>, mode: &str) -> Result<RestoreReport, String> {
    restore_version_with(vault_root, id, path, mode, &restore::real_trash)
}

/// [`restore_version`] with the site restore's trash given: a test passes one
/// that records the call instead of moving a real file to the OS Trash.
pub(crate) fn restore_version_with(
    vault_root: &Path,
    id: &str,
    path: Option<String>,
    mode: &str,
    trash: &dyn Fn(&Path, &str) -> Result<(), String>,
) -> Result<RestoreReport, String> {
    let store = HistoryStore::in_vault(vault_root);
    known_id(&store.list_records(), id)?;
    if path.is_none() {
        store.site_version(id)?;
    }
    let current = site_on_disk(vault_root)?;
    let target = current_target(vault_root);

    // The backup dedups against the newest record for this target (design
    // rule 3), so it answers whether it wrote one.
    let undo_id = store.save_before_restore(vault_root, &current, &target, id)?;

    match path {
        Some(page_path) => {
            let restore_mode = match mode {
                "in_place" => RestoreMode::InPlace,
                "copy" => RestoreMode::AsCopy,
                other => return Err(format!("restore_version: unknown mode \"{other}\"")),
            };
            match store.restore_page(vault_root, id, &page_path, restore_mode) {
                Ok(()) => Ok(RestoreReport {
                    restored: vec![page_path],
                    failed: Vec::new(),
                    trashed: Vec::new(),
                    undo_id,
                }),
                Err(error) => Ok(RestoreReport {
                    restored: Vec::new(),
                    failed: vec![RestoreFailure { path: page_path, error }],
                    trashed: Vec::new(),
                    undo_id,
                }),
            }
        }
        // Whole-site restore. `mode` has no site-scoped meaning (there is no
        // "copy" form of restoring an entire tree) and is ignored here.
        None => Ok(to_restore_report(
            store.restore_site(vault_root, id, &current, trash),
            undo_id,
        )),
    }
}

/// [`super::RestoreReport`] → the boundary's [`RestoreReport`]: the same three
/// lists, with `failed`'s positional pairs given field names.
fn to_restore_report(report: super::RestoreReport, undo_id: Option<String>) -> RestoreReport {
    RestoreReport {
        restored: report.restored,
        failed: report
            .failed
            .into_iter()
            .map(|(path, error)| RestoreFailure { path, error })
            .collect(),
        trashed: report.trashed,
        undo_id,
    }
}

/// [`SourceTree::on_disk`] for the panel, which has no terminal: download
/// progress goes to the log, and a stall's diagnosis says to try again.
fn site_on_disk(vault_root: &Path) -> Result<SourceTree, String> {
    SourceTree::on_disk(vault_root, "try again", &mut |line| log::info!("history: {line}"))
}

/// Save a version now, outside the publish flow, of the site as the disk
/// holds it. The editor flushes its pending auto-save first
/// (`flushEditorSave`); no build is awaited.
pub fn save_version(vault_root: &Path, label: Option<String>) -> Result<VersionRow, String> {
    if !super::store::history_enabled(vault_root) {
        return Err("Publish history is turned off for this site.".to_string());
    }
    let store = HistoryStore::in_vault(vault_root);
    let tree = site_on_disk(vault_root)?;
    let target = current_target(vault_root);

    let new_id = store.snapshot_manual(vault_root, &tree, &target, label)?;
    let new_record = new_id.as_deref().and_then(|id| store.load_record(id));
    match (new_id, new_record) {
        (Some(id), Some(record)) => Ok(version_row_from_record(id, &record)),
        _ => Err("The version was saved but could not be read back".to_string()),
    }
}

/// "Show in Finder" on the history store's directory. `store_dir()` is a fixed
/// subpath moss itself derives from `vault_root`, never a caller-supplied
/// relative path, so there is nothing here for a containment check to validate.
pub fn reveal_history_store(vault_root: &Path) -> Result<(), String> {
    crate::system::reveal::reveal_path(HistoryStore::in_vault(vault_root).store_dir())
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;
