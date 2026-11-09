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

pub async fn list_versions(host: &dyn CommandHost, a: ListVersionsArgs) -> Result<panel::VersionList, String> {
    let root = host.root().to_path_buf();
    tokio::task::spawn_blocking(move || panel::list_versions(&root, &a.scope, a.path))
        .await.map_err(|e| e.to_string())?
}

#[derive(serde::Deserialize, specta::Type)]
pub struct ReadVersionArgs {
    pub id: String,
    pub path: String,
}

pub async fn read_version(host: &dyn CommandHost, a: ReadVersionArgs) -> Result<panel::VersionContent, String> {
    let root = host.root().to_path_buf();
    tokio::task::spawn_blocking(move || panel::read_version(&root, &a.id, &a.path))
        .await.map_err(|e| e.to_string())?
}

/// Opens the history store in the file manager of the machine the engine runs on.
pub async fn reveal_history_store(host: &dyn CommandHost, _: NoArgs) -> Result<(), String> {
    let root = host.root().to_path_buf();
    tokio::task::spawn_blocking(move || panel::reveal_history_store(&root))
        .await.map_err(|e| e.to_string())?
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
    let root = host.root().to_path_buf();
    tokio::task::spawn_blocking(move || panel::restore_version(&root, &a.id, a.path, &a.mode))
        .await.map_err(|e| e.to_string())?
}

#[derive(serde::Deserialize, specta::Type)]
pub struct SaveVersionArgs {
    pub label: Option<String>,
}

pub async fn save_version(host: &dyn CommandHost, a: SaveVersionArgs) -> Result<panel::VersionRow, String> {
    let root = host.root().to_path_buf();
    tokio::task::spawn_blocking(move || panel::save_version(&root, a.label))
        .await.map_err(|e| e.to_string())?
}
