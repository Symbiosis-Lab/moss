//! The app's admission verdict is enforced at the loader, not at one of its
//! callers, and it sees the whole manifest.
//!
//! Its own test binary on purpose. The installed check is process-wide and
//! set-once, so a test that installs one inside the library's unit-test binary
//! would decide the verdict for every other test that loads a plugin.

use std::fs;
use std::path::Path;

use moss_build::plugins::{admission, discovery, PluginManifest};

/// Stands in for the app's verdict. `matters` 1.0.0 is dead; so is anything
/// whose floor outruns this stand-in host. Everything else, including other
/// versions of the same id, is fine.
fn refuse_the_dead_and_the_too_new(manifest: &PluginManifest, _dir: &Path) -> Option<String> {
    if manifest.name == "matters" && manifest.version == "1.0.0" {
        return Some("revoked: leaked reader email addresses to a third party".to_string());
    }
    (manifest.min_moss_version.as_deref() == Some("99.0.0"))
        .then(|| "needs a moss that does not exist yet".to_string())
}

fn write_plugin(root: &Path, name: &str, version: &str) -> std::path::PathBuf {
    write_plugin_needing(root, name, version, None)
}

fn write_plugin_needing(
    root: &Path,
    name: &str,
    version: &str,
    min_moss_version: Option<&str>,
) -> std::path::PathBuf {
    let dir = root.join(name).join(version);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("index.js"), "export default {};").unwrap();
    let floor = min_moss_version
        .map(|v| format!(r#","min_moss_version":"{v}""#))
        .unwrap_or_default();
    fs::write(
        dir.join("manifest.json"),
        format!(
            r#"{{"name":"{name}","version":"{version}","entry":"index.js","capabilities":["syndicate"]{floor}}}"#
        ),
    )
    .unwrap();
    dir
}

/// One test, not several. The installed check is process-wide, so a sibling
/// test installing it concurrently would decide whether "nothing installed"
/// still loads — the assertion this opens with.
#[test]
fn the_apps_verdict_is_enforced_at_the_loader() {
    let tmp = tempfile::tempdir().unwrap();
    let doomed = write_plugin(tmp.path(), "matters", "1.0.0");
    let fixed = write_plugin(tmp.path(), "matters", "1.0.1");
    let other = write_plugin(tmp.path(), "github", "1.0.0");
    let too_new = write_plugin_needing(tmp.path(), "onionpress", "2.0.0", Some("99.0.0"));

    // No check installed is the state of every process that never fetched the
    // list. It must not mean "refuse everything".
    assert!(discovery::load_plugin(&doomed).is_ok());

    admission::install_check(refuse_the_dead_and_the_too_new);

    let err = discovery::load_plugin(&doomed).expect_err("a revoked version must not load");
    assert!(
        err.contains("leaked reader email addresses"),
        "the published reason has to reach the user: {err}"
    );

    // The verdict is asked with the whole manifest, so a ground that is not
    // id+version — here `min_moss_version` — can refuse at the same point.
    let err = discovery::load_plugin(&too_new)
        .expect_err("a plugin that outruns its host must not load");
    assert!(err.contains("does not exist yet"), "{err}");

    // Revocation is per version and per id, not a blanket ban on the plugin.
    assert!(discovery::load_plugin(&fixed).is_ok());
    assert!(discovery::load_plugin(&other).is_ok());

    // And the same gate seen through `discover_installed_plugins`, which is
    // what the manager, eager-init and deploy resolution read.
    let vault = tmp.path().join("vault");
    let plugins = vault.join(".moss").join("plugins");
    for (id, version) in [("matters", "1.0.0"), ("github", "2.1.0")] {
        let dir = plugins.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.js"), "export default {};").unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            format!(r#"{{"name":"{id}","version":"{version}","entry":"index.js"}}"#),
        )
        .unwrap();
    }

    let found = discovery::discover_installed_plugins(vault.to_str().unwrap()).unwrap();
    let ids: Vec<_> = found.iter().map(|p| p.manifest.name.as_str()).collect();
    assert_eq!(ids, ["github"], "the revoked plugin is still in the list");
}
