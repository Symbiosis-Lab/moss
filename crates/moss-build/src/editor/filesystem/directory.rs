//! Atomic editor listings with complete visible-entry metadata.
use super::{DirEntry, cmp_dir_entry, extract_file_id};

/// Inner implementation of `list_directory` for testability (no Tauri State dependency).
pub fn list_directory_inner(
    path: &str,
    project_path: &str,
    show_internal: bool,
) -> Result<Vec<DirEntry>, String> {
    list_directory_counted(path, project_path, show_internal)
        .map(|(entries, _)| entries).map_err(|failure| failure.to_string())
}

pub(super) fn list_directory_counted(
    path: &str,
    project_path: &str,
    show_internal: bool,
) -> Result<(Vec<DirEntry>, u32), crate::build::cloud_readiness::storage::Unavailable> {
    let dir = std::path::Path::new(path);

    let session = crate::system::folder_session::registry().get(project_path);
    let snapshot = crate::build::cloud_readiness::storage::await_operation(
        dir,
        crate::build::cloud_readiness::storage::OperationPolicy::EditorDirectory { project: project_path.into(), show_internal },
        std::time::Duration::from_secs(3),
        &|| session.as_ref().is_some_and(|s| s.cancel.is_cancelled()),
        &|| log::info!("Waiting for the editor directory to become available: {}", path),
        crate::build::cloud_readiness::storage::directory_operation(dir, std::path::Path::new(project_path), show_internal),
    )?;
    let crate::build::cloud_readiness::storage::StorageValue::Directory { entries: raw_entries, hidden } = &*snapshot else {
        unreachable!("editor-listing requests return a directory snapshot");
    };
    let mut entries: Vec<DirEntry> = Vec::new();
    for (entry, metadata) in raw_entries {
        let name = entry.file_name().to_string_lossy().to_string();

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64());

        let file_id = extract_file_id(&metadata);

        entries.push(DirEntry {
            name,
            path: entry.path().to_string_lossy().to_string(),
            is_dir: metadata.is_dir(),
            modified,
            file_id,
        });
    }

    entries.sort_by(cmp_dir_entry);

    Ok((entries, *hidden))
}
