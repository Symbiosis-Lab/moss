//! A page renamed on disk without the watcher pairing the two events (Finder
//! does this) is still reported as a rename, because two real builds record
//! the same inode and content for the old and new paths. Unix only: elsewhere
//! no inode is recorded and the rename stays a delete plus a create.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

fn build_sync(folder_path: &str) -> Result<String, String> {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let source = PathBuf::from(folder_path);
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&source),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(folder_path),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_page_renamed_on_disk_between_two_builds_is_reported_as_a_rename() {
    let tmp = std::env::temp_dir().join(format!("moss_unpaired_rename_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let _cleanup = Cleanup(tmp.clone());
    let folder = tmp.to_string_lossy().to_string();
    write(&tmp, ".moss/config.toml", "schema_version = 6\n\n[site]\nlang = \"en\"\n");
    write(&tmp, "index.md", "---\ntitle: Home\n---\n\nSee [[note]].\n");
    write(&tmp, "note.md", "---\ntitle: Note\n---\n\nNotes from the field.\n");

    build_sync(&folder).expect("first build");
    let before = moss_build::build::load_previous_hashes(&folder);
    fs::rename(tmp.join("note.md"), tmp.join("field-notes.md")).unwrap();
    build_sync(&folder).expect("second build");
    let after = moss_build::build::load_previous_hashes(&folder);

    let event = moss_build::build::watch::decide_rebuild_event(Some(after), &folder, &before, &[])
        .expect("the rename changes the site");
    assert_eq!(
        event.source_renames,
        Some(vec![("note.md".to_string(), "field-notes.md".to_string())]),
        "deletes {:?}, creates {:?}",
        event.source_deletes,
        event.source_creates,
    );
}
