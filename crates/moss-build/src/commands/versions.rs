//! The Versions surface: the folder's publish history.
//!
//! A caller's `projectPath` is not read: the host's folder is the one acted on.
//! A version `id` is checked against the records the store lists before it is
//! joined into a filename, and a version `path` is a key into a record, never
//! joined onto the folder.

use super::{CommandHost, NoArgs};
use crate::deploy::history::panel;

#[derive(serde::Deserialize, specta::Type)]
pub struct ListVersionsArgs {
    pub scope: String,
    pub path: Option<String>,
}

/// Only the site-version drill-down needs the sealed site, so opening the
/// panel costs no build.
pub async fn list_versions(host: &dyn CommandHost, a: ListVersionsArgs) -> Result<panel::VersionList, String> {
    panel::list_versions(host.root(), &a.scope, a.path, host.sealed_now()).await
}

#[derive(serde::Deserialize, specta::Type)]
pub struct ReadVersionArgs {
    pub id: String,
    pub path: String,
}

pub async fn read_version(host: &dyn CommandHost, a: ReadVersionArgs) -> Result<panel::VersionContent, String> {
    panel::read_version(host.root(), &a.id, &a.path)
}

/// Opens the history store in the file manager of the machine the engine runs on.
pub async fn reveal_history_store(host: &dyn CommandHost, _: NoArgs) -> Result<(), String> {
    panel::reveal_history_store(host.root())
}

#[derive(serde::Deserialize, specta::Type)]
pub struct RestoreVersionArgs {
    pub id: String,
    pub path: Option<String>,
    pub mode: String,
}

/// Saves the present as a version first; restoring the whole site moves pages
/// added since to the OS trash.
pub async fn restore_version(host: &dyn CommandHost, a: RestoreVersionArgs) -> Result<panel::RestoreReport, String> {
    panel::restore_version(host.root(), &a.id, a.path, &a.mode, host.sealed_now()).await
}

#[derive(serde::Deserialize, specta::Type)]
pub struct SaveVersionArgs {
    pub label: Option<String>,
}

pub async fn save_version(host: &dyn CommandHost, a: SaveVersionArgs) -> Result<panel::VersionRow, String> {
    panel::save_version(host.root(), a.label, host.sealed_now()).await
}
