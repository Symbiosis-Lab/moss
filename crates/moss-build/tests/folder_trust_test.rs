//! A folder's plugins run when the user trusts the folder. One test binary,
//! one test: the record is process-wide, and turning it on would change what
//! every other test in a shared binary is allowed to load.

use std::path::{Path, PathBuf};

use moss_build::plugins::bundled::{code_hash, read_installed_manifest};
use moss_build::plugins::install::registry_client::cache::{Anchor, CachedRegistry};
use moss_build::plugins::install::registry_client::enforce::{
    folder_answer, install, install_headless, is_trusted, publish, set_folder_trust,
    verdict, Refusal,
};
use moss_build::plugins::install::registry_client::index::accept_revoked;
use moss_build::plugins::install::registry_client::trust;

/// Folders the earlier approvals name live outside the system temporary
/// directory, whose rows the migration drops.
fn site(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("folder-trust").join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".moss")).unwrap();
    root.canonicalize().unwrap()
}

fn plugin(site: &Path, id: &str, entry: &str) -> PathBuf {
    let dir = site.join(".moss/plugins").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), format!(r#"{{"name":"{id}","version":"1.0.0","entry":"index.js"}}"#)).unwrap();
    std::fs::write(dir.join("index.js"), entry).unwrap();
    dir
}

fn refusal(dir: &Path) -> Option<Refusal> {
    verdict(&read_installed_manifest(dir).unwrap(), dir)
}

fn hash(dir: &Path) -> String {
    code_hash(dir, &read_installed_manifest(dir).unwrap()).unwrap()
}

#[test]
fn a_folder_s_answer_decides_whether_its_plugins_run() {
    let scratch = tempfile::tempdir().unwrap();
    let app_data = scratch.path().join("app-data");
    install(None, Some(&app_data));

    // Untrusted until answered; trusted covers a plugin that arrives later.
    let a = site("a");
    let demo = plugin(&a, "demo", "var A = 1;");
    assert_eq!(refusal(&demo), Some(Refusal::Untrusted));
    assert_eq!(folder_answer(&a), None, "never asked");
    set_folder_trust(&a, true).unwrap();
    assert_eq!(refusal(&demo), None);
    let later = plugin(&a, "later", "var L = 1;");
    std::fs::write(demo.join("index.js"), "var A = 2;").unwrap();
    assert_eq!((refusal(&later), refusal(&demo)), (None, None), "new and changed code in a trusted folder runs");
    assert_eq!(trust::load(&app_data).answer(&a), Some(true), "recorded in app data");
    assert!(!a.join(".moss").read_dir().unwrap().any(|e| e.unwrap().file_name() != "plugins"), "and nothing in the folder");

    // The record follows the canonical folder.
    #[cfg(unix)]
    {
        let alias = scratch.path().join("alias");
        std::os::unix::fs::symlink(&a, &alias).unwrap();
        assert!(is_trusted(&alias));
        assert_eq!(refusal(&alias.join(".moss/plugins/demo")), None);
    }

    // A withdrawal binds a trusted folder.
    let list = accept_revoked(
        r#"{"schema_version":1,"serial":2,"revocations":[{"id":"later","versions":["1.0.0"],"reason":"leaked"}]}"#,
        0,
        0,
    )
    .unwrap();
    publish(CachedRegistry {
        index: None,
        revoked: Some(list),
        anchor: Anchor { highest_revoked_serial: 2, revocation_count: 1, ..Anchor::default() },
    });
    assert!(matches!(refusal(&later), Some(Refusal::Revoked { .. })));
    publish(CachedRegistry::default());

    // Taken back: the plugins stop.
    set_folder_trust(&a, false).unwrap();
    assert_eq!((refusal(&demo), folder_answer(&a)), (Some(Refusal::Untrusted), Some(false)));

    // Migration: a folder whose every plugin was approved at its current code
    // is trusted, and the app records it; one with a plugin never approved,
    // or approved at other code, is asked.
    let covered = site("covered");
    let x = plugin(&covered, "x", "var X = 1;");
    let partly = site("partly");
    let y = plugin(&partly, "y", "var Y = 1;");
    plugin(&partly, "z", "var Z = 1;");
    let moved = site("moved");
    let w = plugin(&moved, "w", "var W = 1;");
    let legacy_root = scratch.path().join("upgraded");
    std::fs::create_dir_all(&legacy_root).unwrap();
    std::fs::write(
        legacy_root.join(trust::LEGACY_FILE),
        serde_json::json!({"approvals": [
            {"dir": x, "code_sha256": hash(&x)},
            {"dir": y, "code_sha256": hash(&y)},
            {"dir": w, "code_sha256": "older code"},
        ]})
        .to_string(),
    )
    .unwrap();
    install(None, Some(&legacy_root));
    assert_eq!(refusal(&x), None, "settled before the app asks");
    assert_eq!(folder_answer(&covered), Some(true));
    assert_eq!(trust::load(&legacy_root).answer(&covered), Some(true), "and recorded at the app's first ask");
    assert_eq!((folder_answer(&partly), folder_answer(&moved)), (None, None));
    assert_eq!(refusal(&y), Some(Refusal::Untrusted));

    // A headless build reads the app's answers and writes none.
    install_headless(Some(&legacy_root), false);
    assert!(is_trusted(&covered) && !is_trusted(&partly));
    assert!(set_folder_trust(&partly, true).is_err());
    assert_eq!(trust::load(&legacy_root).answer(&partly), None);

    // A record that cannot be saved trusts nothing, in this process too.
    let blocked = scratch.path().join("blocked");
    std::fs::write(&blocked, "a file where the directory should be").unwrap();
    install(None, Some(&blocked));
    let c = site("c");
    let lamp = plugin(&c, "lamp", "var C = 1;");
    assert!(set_folder_trust(&c, true).is_err());
    assert_eq!(refusal(&lamp), Some(Refusal::Untrusted));
}
