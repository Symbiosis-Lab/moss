//! Real-process proof that the preview stops serving a page once its source
//! is deleted, in the two cases where the build may not sweep staging: a
//! folder that has never promoted a generation, and a rebuild that starts
//! before the previous build has promoted.
#![cfg(unix)]

#[path = "served_child.rs"]
mod served_child;

use served_child::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PAGE: &str = "/notes/quiet-harbour-notes/";

struct Folder {
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    home: PathBuf,
    site: PathBuf,
}

impl Folder {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let site = std::fs::canonicalize(root.path()).unwrap().join("site");
        // The page is in a folder of its own: a deleted top-level file changes
        // the watch set, and its event can be lost to that change.
        std::fs::create_dir_all(site.join("notes")).unwrap();
        std::fs::write(site.join("index.md"), "# Home\n").unwrap();
        std::fs::write(site.join("about.md"), "# About\n").unwrap();
        std::fs::write(site.join("notes/Quiet Harbour Notes.md"), "# Quiet Harbour Notes\n").unwrap();
        Folder { home: home.path().to_path_buf(), site, _dirs: (home, root) }
    }

    fn page_source(&self) -> PathBuf {
        self.site.join("notes/Quiet Harbour Notes.md")
    }

    /// Served, with its first build sealed and its manifest on disk. A watched
    /// folder is walked several times a second, so a change is found on time
    /// however late the system reports it.
    fn serve(&self, watch: bool) -> (ServedChild, u16) {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_moss-cli"));
        command.arg("build").arg(&self.site).args(["--serve", "--no-plugins"]);
        if watch {
            command.arg("--watch").env("MOSS_TEST_SWEEP_TICK_MS", "300");
        }
        let child = spawn_piped(command, &self.home);
        let port = port_from_url_line(&child.wait_for_access_line(Duration::from_secs(60)));
        wait_for_line(&child.stderr, "Background tasks complete", Duration::from_secs(60))
            .unwrap_or_else(|| panic!("the first build never sealed:\n{}", joined(&child.stderr)));
        (child, port)
    }

    fn manifest_names_the_page(&self) -> bool {
        std::fs::read_to_string(self.site.join(".moss/build.nosync/hashes.json")).is_ok_and(|manifest| manifest.contains("notes/quiet-harbour-notes/index.html"))
    }
}

fn never_promoted(site: &Path) -> bool {
    !site.join(".moss/build.nosync/current").exists()
}

#[test]
fn a_page_deleted_while_the_folder_was_closed_is_gone_at_the_next_open() {
    let folder = Folder::new();
    let (first, port) = folder.serve(false);
    assert_eq!(status(port, PAGE), 200);
    drop(first);
    assert!(folder.manifest_names_the_page(), "the first session sealed the page");
    assert!(never_promoted(&folder.site), "the first session ends before its generation is promoted");

    std::fs::remove_file(folder.page_source()).unwrap();
    let (second, port) = folder.serve(false);
    assert_eq!(status(port, PAGE), 404, "the deleted page is still served:\n{}", joined(&second.stderr));
    assert_eq!(status(port, "/about/"), 200);
}

#[test]
fn a_page_deleted_before_the_last_build_promoted_stops_being_served() {
    let folder = Folder::new();
    let (served, port) = folder.serve(true);
    wait_for_line(&served.stderr, "Sweep active", Duration::from_secs(60))
        .unwrap_or_else(|| panic!("the watch never started:\n{}", joined(&served.stderr)));
    assert_eq!(status(port, PAGE), 200);

    std::fs::remove_file(folder.page_source()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while status(port, PAGE) != 404 {
        assert!(Instant::now() < deadline, "the deleted page is still served:\n{}", joined(&served.stderr));
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(never_promoted(&folder.site), "the rebuild ran before any generation was promoted");
    assert_eq!(status(port, "/about/"), 200);
}
