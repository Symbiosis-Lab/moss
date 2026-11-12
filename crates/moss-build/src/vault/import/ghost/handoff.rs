//! Retain only reader inputs, outside both the source bundle and the project.
use std::{fs, io::Write, path::Path};
use super::bundle::Bundle;
use crate::vault::import::source::SubscriberHandoff;

pub(super) fn retain(bundle: &mut Bundle, folder: &Path, directory: Option<&Path>) -> Result<Option<SubscriberHandoff>, String> {
    let mut inputs = Vec::new();
    for name in ["members.csv", "members.json", "email-disabled.csv"] {
        if let Some(bytes) = bundle.read(name)? { inputs.push((name, bytes)); }
    }
    if inputs.is_empty() { return Ok(None); }
    let default = crate::infra::app_data::app_data_dir_early().map(|p| p.join("imports"));
    let directory = directory.or(default.as_deref()).ok_or("No private app-data directory is available")?;
    let folder = fs::canonicalize(folder).map_err(|e| e.to_string())?;
    // Check the existing ancestor before creating anything, including when a
    // caller gives a symlink or a not-yet-created directory inside the project.
    let mut ancestor = directory;
    while !ancestor.exists() { ancestor = ancestor.parent().ok_or("Invalid private members directory")?; }
    if fs::canonicalize(ancestor).map_err(|e| e.to_string())?.starts_with(&folder) { return Err("Reader files must be retained outside the project".into()); }
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let directory = fs::canonicalize(directory).map_err(|e| e.to_string())?;
    if directory.starts_with(bundle.root()) { return Err("Reader retention must outlive the source bundle".into()); }
    if directory.starts_with(&folder) { return Err("Reader files must be retained outside the project".into()); }
    let retained = directory.join(uuid::Uuid::new_v4().to_string());
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
    builder.create(&retained).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut files = Vec::new();
        for (name, bytes) in inputs {
            let path = retained.join(name);
            let mut options = fs::OpenOptions::new(); // allow:raw_write reader files in private app storage outside any site folder, never the build tree
            options.write(true).create_new(true);
            #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
            options.open(&path).and_then(|mut f| f.write_all(&bytes)).map_err(|e| e.to_string())?;
            files.push(path);
        }
        Ok(Some(SubscriberHandoff { files }))
    })();
    if result.is_err() { let _ = fs::remove_dir_all(retained); }
    result
}
