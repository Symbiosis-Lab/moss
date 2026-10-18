//! `registry_status` against a published snapshot. One test: the snapshot is
//! process-wide, so two tests publishing would race over what the other reads.

use moss_build::plugins::bundled::code_hash;
use moss_build::plugins::install::registry_client::cache::{Anchor, CachedRegistry};
use moss_build::plugins::install::registry_client::enforce::{publish, registry_status, RegistryStatus};
use moss_build::plugins::install::registry_client::index::{accept_index, accept_revoked};
use moss_build::plugins::types::PluginManifest;
use std::path::Path;

fn plugin(root: &Path, name: &str, version: &str, entry: &str) -> (std::path::PathBuf, PluginManifest) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let text = format!(r#"{{"name":"{name}","version":"{version}","entry":"index.js"}}"#);
    std::fs::write(dir.join("manifest.json"), &text).unwrap();
    std::fs::write(dir.join("index.js"), entry).unwrap();
    (dir, PluginManifest::parse(&text).unwrap())
}

fn listing(id: &str, version: &str, code: Option<&str>) -> String {
    let code = code.map(|c| format!(r#","code_sha256":"{c}""#)).unwrap_or_default();
    format!(
        r#"{{"type":"plugin","id":"{id}","display_name":"{id}","version":"{version}",
            "download_url":"https://example.invalid/{id}.zip","sha256":"aa"{code}}}"#
    )
}

#[test]
fn the_registry_status_of_a_plugin_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let (exact_dir, exact) = plugin(tmp.path(), "lantern", "1.0.0", "var A = 1;");
    let exact_hash = code_hash(&exact_dir, &exact).unwrap();
    let (edited_dir, edited) = plugin(tmp.path(), "kiln", "1.0.0", "var B = 1;");
    let (bare_dir, bare) = plugin(tmp.path(), "loom", "1.0.0", "var C = 1;");
    let (stale_dir, stale) = plugin(tmp.path(), "anvil", "1.0.0", "var D = 1;");
    let (_, unlisted) = plugin(tmp.path(), "unlisted", "1.0.0", "var E = 1;");
    let (gone_dir, gone) = plugin(tmp.path(), "ember", "1.0.0", "var F = 1;");

    let entries = [
        listing("lantern", "1.0.0", Some(&exact_hash)),
        listing("kiln", "1.0.0", Some("not-the-hash-of-this-directory")),
        listing("loom", "1.0.0", None),
        listing("anvil", "2.0.0", Some("whatever")),
        listing("ember", "1.0.0", None),
    ]
    .join(",");
    let index = accept_index(&format!(r#"{{"schema_version":1,"serial":1,"entries":[{entries}]}}"#), 0).unwrap();
    let revoked = accept_revoked(
        r#"{"schema_version":1,"serial":1,"revocations":[{"id":"ember","versions":["1.0.0"],"reason":"leaked keys"}]}"#,
        0,
        0,
    )
    .unwrap();
    publish(CachedRegistry {
        index: Some(index),
        revoked: Some(revoked),
        anchor: Anchor { highest_revoked_serial: 1, revocation_count: 1, ..Anchor::default() },
    });

    assert_eq!(registry_status(&exact, &exact_dir), RegistryStatus::ListedExact { version: "1.0.0".into() });
    assert_eq!(
        registry_status(&edited, &edited_dir),
        RegistryStatus::ListedVersion,
        "a listed hash that differs is only the name and version"
    );
    assert_eq!(registry_status(&bare, &bare_dir), RegistryStatus::ListedVersion);
    assert_eq!(registry_status(&stale, &stale_dir), RegistryStatus::Unknown, "another version is listed, not this one");
    assert_eq!(registry_status(&unlisted, tmp.path().join("unlisted").as_path()), RegistryStatus::Unknown);
    assert_eq!(
        registry_status(&gone, &gone_dir),
        RegistryStatus::Withdrawn { reason: "leaked keys".into() },
        "a withdrawal outranks the listing"
    );
}
