//! A page renamed on disk without the watcher pairing the two events (Finder
//! does this) is still reported as a rename, because two real builds record
//! the same inode and content for the old and new paths. Unix only: elsewhere
//! no inode is recorded and the rename stays a delete plus a create.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

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

    crate::site::build_full(&folder).expect("first build");
    let before = moss_build::build::load_previous_hashes(&folder);
    fs::rename(tmp.join("note.md"), tmp.join("field-notes.md")).unwrap();
    crate::site::build_full(&folder).expect("second build");
    let after = moss_build::build::load_previous_hashes(&folder);

    let event = moss_build::build::watch::build_rebuild_event_with_renames(&after, &before, &[])
        .expect("the rename changes the site");
    assert_eq!(
        event.source_renames,
        Some(vec![("note.md".to_string(), "field-notes.md".to_string())]),
        "deletes {:?}, creates {:?}",
        event.source_deletes,
        event.source_creates,
    );
}
