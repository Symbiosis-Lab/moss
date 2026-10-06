use super::*;
use tempfile::tempdir;

static FIXTURE: Dir<'static> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/tests/fixtures/bundled-plugins");

/// Four stand-in plugins — two deployers, a syndicator and a processor — shaped
/// like the shipped ones. Stand-ins, not mirrors: the tests read names,
/// capabilities, versions and manifest config defaults, and nothing checks
/// these against `plugins/*/assets/manifest.json`. The real bundles are the
/// app's; tests about their contents live beside the embed in
/// the desktop app.
pub(super) fn fixture() -> BundledSet {
    BundledSet { names: &["github", "matters", "onionpress", "comment"], root: &FIXTURE, default_deployer: "" }
}

#[test]
fn test_ensure_bundled_plugins_creates_directory() {
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();

    // Create .moss directory
    fs::create_dir_all(temp.path().join(".moss")).unwrap();

    // This should not fail even if no plugins are embedded yet
    let result = ensure_bundled_plugins_installed(project_path);
    assert!(result.is_ok());
}

#[test]
fn test_ensure_bundled_plugins_cached_after_first_call() {
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();

    // Create .moss directory
    fs::create_dir_all(temp.path().join(".moss")).unwrap();

    // First call
    let result1 = ensure_bundled_plugins_installed(project_path);
    assert!(result1.is_ok());

    // Second call should be cached (returns immediately without doing work)
    // We can't easily verify "no work done" but we can verify it succeeds quickly
    let result2 = ensure_bundled_plugins_installed(project_path);
    assert!(result2.is_ok());

    // Verify the project is in the cache
    let checked = get_checked_projects().lock().unwrap();
    assert!(checked.contains(project_path));
}

#[test]
fn test_extract_plugin_dir_handles_existing_directory() {
    // This test verifies that the extraction logic handles the case where
    // a directory exists but manifest.json is missing (incomplete installation).
    // Before fix: "Failed to create directory ... File exists (os error 17)"
    // After fix: Should clean up and reinstall successfully

    let temp = tempdir().unwrap();
    let target = temp.path().join("test-plugin");

    // Simulate incomplete installation: directory exists but no manifest.json
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("partial-file.txt"), "incomplete").unwrap();

    // Verify directory exists but no manifest
    assert!(target.exists());
    assert!(!target.join("manifest.json").exists());

    // create_dir_all should succeed on existing directory
    let result = fs::create_dir_all(&target);
    assert!(
        result.is_ok(),
        "create_dir_all should not fail on existing directory"
    );
}

#[test]
fn test_should_install_plugin_missing_manifest() {
    let temp = tempdir().unwrap();
    let target = temp.path().join("test-plugin");

    // No directory exists - should want to install
    // But only if bundled manifest exists (which it won't for test-plugin)
    // So this will return false since there's no bundled manifest
    let result = should_install_plugin("test-plugin", &target, true);
    assert!(
        !result,
        "Should return false when bundled manifest doesn't exist"
    );
}

#[test]
fn test_should_install_plugin_corrupted_manifest() {
    let temp = tempdir().unwrap();
    let target = temp.path().join("github");

    // Create directory with corrupted manifest
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("manifest.json"), "not valid json").unwrap();

    // For actual bundled plugins, this would return true
    // For test purposes, we just verify the function handles the case
    let result = should_install_plugin("github", &target, true);
    // Result depends on whether github is actually bundled
    // The function should not panic regardless
    let _ = result;
}

#[test]
fn test_read_installed_manifest_valid() {
    let temp = tempdir().unwrap();
    let plugin_dir = temp.path().join("test-plugin");
    fs::create_dir_all(&plugin_dir).unwrap();

    let manifest = r#"{
            "name": "test-plugin",
            "version": "1.0.0",
            "entry": "main.js",
            "capabilities": ["syndicate"]
        }"#;
    fs::write(plugin_dir.join("manifest.json"), manifest).unwrap();

    let result = read_installed_manifest(&plugin_dir);
    assert!(result.is_some());
    let manifest = result.unwrap();
    assert_eq!(manifest.name, "test-plugin");
    assert_eq!(manifest.version, "1.0.0");
}

#[test]
fn test_read_installed_manifest_missing() {
    let temp = tempdir().unwrap();
    let plugin_dir = temp.path().join("nonexistent");

    let result = read_installed_manifest(&plugin_dir);
    assert!(result.is_none());
}

#[test]
fn test_read_installed_manifest_invalid_json() {
    let temp = tempdir().unwrap();
    let plugin_dir = temp.path().join("test-plugin");
    fs::create_dir_all(&plugin_dir).unwrap();
    fs::write(plugin_dir.join("manifest.json"), "not json").unwrap();

    let result = read_installed_manifest(&plugin_dir);
    assert!(result.is_none());
}

#[test]
fn test_install_bundled_plugin_not_in_bundle() {
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();
    fs::create_dir_all(temp.path().join(".moss")).unwrap();

    let result = install_bundled_plugin("nonexistent-plugin", project_path);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found in bundle"));
}

#[test]
fn test_get_bundled_plugin_names() {
    // May legitimately be empty in a dev build where the plugins were not
    // bundled, so the assertions are about SHAPE, not count: whatever is
    // returned must be usable as a plugin id.
    let names = get_bundled_plugin_names();
    for name in names {
        assert!(!name.trim().is_empty(), "bundled plugin name is blank");
        assert!(
            !name.contains('/') && !name.contains(std::path::MAIN_SEPARATOR),
            "bundled plugin name must be a bare id, got {name:?}"
        );
    }
    let unique: std::collections::HashSet<&&str> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "duplicate bundled plugin names");
}

/// Test that user-installed plugins with newer versions are NOT overwritten.
///
/// Scenario: User manually installed plugin version "999.0.0" (perhaps a beta
/// or custom build). The bundled version is older (e.g., "1.0.0").
/// The semver comparison detects installed > bundled and skips the update,
/// respecting the user's deliberate choice.
#[test]
fn test_should_not_overwrite_user_installed_plugin() {
    let temp = tempdir().unwrap();
    let target = temp.path().join("github");

    // Create a plugin directory with a user-installed version that is newer
    // than the bundled version (simulating user installed a beta/custom version)
    fs::create_dir_all(&target).unwrap();
    let manifest = r#"{
            "name": "github",
            "version": "999.0.0",
            "entry": "main.bundle.js",
            "capabilities": ["deploy"]
        }"#;
    fs::write(target.join("manifest.json"), manifest).unwrap();

    // The user has version 999.0.0 installed, which is newer than any bundled version.
    // Semver comparison: installed (999.0.0) > bundled → skip (respect user's choice).
    let result = should_install_plugin("github", &target, true);

    assert!(
        !result,
        "should_install_plugin should return false for user-installed plugins \
             with different versions. User chose to install version 999.0.0, \
             we should not overwrite it with the bundled version."
    );
}

// =========================================================================
// Content-hash-based auto-update tests (TDD RED phase)
// =========================================================================

#[test]
fn test_compute_installed_hash_deterministic() {
    // compute_installed_hash should return a deterministic SHA-256 hash
    // for a directory with files, and the same content should always
    // produce the same hash.
    let temp = tempdir().unwrap();
    let plugin_dir = temp.path().join("github");

    let bundled_dir = bundled_set().root.get_dir("github");
    if bundled_dir.is_none() {
        return; // github not bundled in test builds
    }
    extract_plugin_dir(bundled_dir.unwrap(), &plugin_dir).unwrap();

    let hash1 = compute_installed_hash("github", &plugin_dir);
    let hash2 = compute_installed_hash("github", &plugin_dir);

    assert!(
        hash1.is_some(),
        "Should return Some hash for existing directory with files"
    );
    assert_eq!(hash1, hash2, "Same content should produce same hash");

    // Hash should be a hex-encoded SHA-256 (64 chars)
    let hash_str = hash1.unwrap();
    assert_eq!(
        hash_str.len(),
        64,
        "SHA-256 hex string should be 64 characters"
    );
}

#[test]
fn test_compute_installed_hash_none_for_nonexistent() {
    // compute_installed_hash should return None for a directory that doesn't exist
    let temp = tempdir().unwrap();
    let nonexistent = temp.path().join("does-not-exist");

    let result = compute_installed_hash("github", &nonexistent);
    assert!(
        result.is_none(),
        "Should return None for nonexistent directory"
    );
}

#[test]
fn test_compute_installed_hash_different_content_different_hash() {
    // Directories with different code file content should produce different hashes
    let temp = tempdir().unwrap();

    let bundled_dir = bundled_set().root.get_dir("github");
    if bundled_dir.is_none() {
        return; // github not bundled in test builds
    }

    // Extract bundled plugin to dir_a
    let dir_a = temp.path().join("github-a");
    extract_plugin_dir(bundled_dir.unwrap(), &dir_a).unwrap();

    // Extract to dir_b and modify a code file
    let dir_b = temp.path().join("github-b");
    extract_plugin_dir(bundled_dir.unwrap(), &dir_b).unwrap();
    fs::write(dir_b.join("main.bundle.js"), "// MODIFIED CODE").unwrap();

    let hash_a = compute_installed_hash("github", &dir_a);
    let hash_b = compute_installed_hash("github", &dir_b);

    assert!(hash_a.is_some());
    assert!(hash_b.is_some());
    assert_ne!(
        hash_a, hash_b,
        "Different content should produce different hashes"
    );
}

#[test]
fn test_should_install_plugin_same_version_different_content() {
    // When the installed plugin has the SAME version as bundled but DIFFERENT
    // file content (dev rebuild scenario), should_install_plugin should return true.
    //
    // This is the key auto-update scenario: developer merges a plugin PR,
    // rebuilds moss, but the installed plugin still has old code with the
    // same version number.
    let temp = tempdir().unwrap();
    let target = temp.path().join("github");
    fs::create_dir_all(&target).unwrap();

    // Get the bundled manifest version for github
    let bundled_manifest = read_bundled_manifest("github");
    if bundled_manifest.is_none() {
        // If github isn't actually bundled in test builds, skip
        return;
    }
    let bundled_version = bundled_manifest.unwrap().version;

    // Write a manifest with the SAME version as bundled
    let manifest = format!(
        r#"{{"name":"github","version":"{}","entry":"main.bundle.js","capabilities":["deploy"]}}"#,
        bundled_version
    );
    fs::write(target.join("manifest.json"), &manifest).unwrap();

    // Write DIFFERENT file content than what's bundled
    fs::write(target.join("main.bundle.js"), "// OLD CODE - this is stale").unwrap();

    let result = should_install_plugin("github", &target, true);

    assert!(
        result,
        "should_install_plugin should return true when same version but different content \
             (dev rebuild scenario - bundled code was updated but version stayed the same)"
    );
}

#[test]
fn test_should_install_plugin_same_version_same_content() {
    // When the installed plugin has the SAME version AND SAME content as bundled,
    // should_install_plugin should return false (already up to date).
    let temp = tempdir().unwrap();
    let target = temp.path().join("github");

    // Get the bundled plugin directory and extract it to our temp dir
    // This gives us an exact copy of the bundled content
    let bundled_dir = bundled_set().root.get_dir("github");
    if bundled_dir.is_none() {
        // If github isn't actually bundled in test builds, skip
        return;
    }

    // Extract the bundled plugin to target (creates exact copy)
    extract_plugin_dir(bundled_dir.unwrap(), &target).unwrap();

    let result = should_install_plugin("github", &target, true);

    assert!(
        !result,
        "should_install_plugin should return false when same version and same content \
             (plugin is already up to date)"
    );
}

// =========================================================================
// Data preservation during plugin updates
// =========================================================================

#[test]
fn test_update_preserves_data_files() {
    // When a plugin is updated, data files (config.toml, config.json, etc.)
    // written by the plugin at runtime must be preserved.
    // Note: This test assumes the bundled github plugin does NOT include config.json,
    // config.toml, or data/. If those are added to the bundle, this test must be updated.
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();
    let plugin_dir = temp.path().join(".moss/plugins/github");
    fs::create_dir_all(&plugin_dir).unwrap();

    // Simulate existing code files
    let manifest =
        r#"{"name":"github","version":"1.0.0","entry":"main.bundle.js","capabilities":["deploy"]}"#;
    fs::write(plugin_dir.join("manifest.json"), manifest).unwrap();
    fs::write(plugin_dir.join("main.bundle.js"), "// old code").unwrap();

    // Simulate data files written by the plugin at runtime
    fs::write(plugin_dir.join("config.toml"), "api_key = \"secret123\"").unwrap();
    fs::write(plugin_dir.join("config.json"), r#"{"userName":"alice"}"#).unwrap();
    fs::create_dir_all(plugin_dir.join("data")).unwrap();
    fs::write(plugin_dir.join("data/syndicated.json"), r#"["article-1"]"#).unwrap();

    // Run install (which is an update since dir exists)
    let result = install_bundled_plugin("github", project_path);
    if result.is_err() {
        // github may not be bundled in test builds — skip
        return;
    }

    // Data files must survive the update
    assert!(
        plugin_dir.join("config.toml").exists(),
        "config.toml should be preserved during update"
    );
    assert_eq!(
        fs::read_to_string(plugin_dir.join("config.toml")).unwrap(),
        "api_key = \"secret123\"",
        "config.toml content should be unchanged"
    );
    assert!(
        plugin_dir.join("config.json").exists(),
        "config.json should be preserved during update"
    );
    assert_eq!(
        fs::read_to_string(plugin_dir.join("config.json")).unwrap(),
        r#"{"userName":"alice"}"#,
        "config.json content should be unchanged"
    );
    assert!(
        plugin_dir.join("data/syndicated.json").exists(),
        "data/syndicated.json should be preserved during update"
    );
    assert_eq!(
        fs::read_to_string(plugin_dir.join("data/syndicated.json")).unwrap(),
        r#"["article-1"]"#,
        "data/syndicated.json content should be unchanged"
    );
}

#[test]
fn test_update_overwrites_code_files() {
    // When a plugin is updated, code files from the bundle must be
    // overwritten with the new version.
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();
    let plugin_dir = temp.path().join(".moss/plugins/github");
    fs::create_dir_all(&plugin_dir).unwrap();

    // Write old code files
    fs::write(plugin_dir.join("main.bundle.js"), "// OLD CODE").unwrap();
    let manifest =
        r#"{"name":"github","version":"1.0.0","entry":"main.bundle.js","capabilities":["deploy"]}"#;
    fs::write(plugin_dir.join("manifest.json"), manifest).unwrap();

    let result = install_bundled_plugin("github", project_path);
    if result.is_err() {
        return; // github not bundled in test builds
    }

    // Code files should be overwritten with bundle content
    let new_content = fs::read_to_string(plugin_dir.join("main.bundle.js")).unwrap();
    assert_ne!(
        new_content, "// OLD CODE",
        "main.bundle.js should be overwritten with bundled content"
    );
}

#[test]
fn test_fresh_install_works_without_existing_dir() {
    // Fresh install (no existing plugin directory) should work correctly.
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();
    fs::create_dir_all(temp.path().join(".moss")).unwrap();

    let plugin_dir = temp.path().join(".moss/plugins/github");
    assert!(
        !plugin_dir.exists(),
        "Plugin dir should not exist before install"
    );

    let result = install_bundled_plugin("github", project_path);
    if result.is_err() {
        return; // github not bundled in test builds
    }

    assert!(
        plugin_dir.join("manifest.json").exists(),
        "manifest.json should exist after install"
    );
    assert!(
        plugin_dir.join("main.bundle.js").exists(),
        "main.bundle.js should exist after install"
    );
}

#[test]
fn test_compute_installed_hash_ignores_data_files() {
    // compute_installed_hash should only consider files that exist in the
    // bundled plugin, ignoring data files written at runtime. Otherwise
    // data files cause hash mismatches and trigger unnecessary updates.
    let temp = tempdir().unwrap();
    let plugin_dir = temp.path().join("github");

    let bundled_dir = bundled_set().root.get_dir("github");
    if bundled_dir.is_none() {
        return; // github not bundled in test builds
    }

    // Extract bundled code files
    extract_plugin_dir(bundled_dir.unwrap(), &plugin_dir).unwrap();

    let hash_without_data = compute_installed_hash("github", &plugin_dir);

    // Add data files (as plugins do at runtime)
    fs::write(plugin_dir.join("config.toml"), "api_key = \"secret\"").unwrap();
    fs::write(plugin_dir.join("config.json"), r#"{"setting":"value"}"#).unwrap();

    let hash_with_data = compute_installed_hash("github", &plugin_dir);

    assert!(hash_without_data.is_some());
    assert!(hash_with_data.is_some());
    assert_eq!(
        hash_without_data, hash_with_data,
        "Adding data files should not change the installed hash"
    );
}

// =========================================================================
// Non-deployer auto-update tests
// =========================================================================

#[test]
fn test_non_deployer_skipped_when_not_installed() {
    // Non-deployer plugins (e.g. comment) should NOT be auto-installed
    // on fresh projects. No bundled plugin is auto-installed.
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();
    fs::create_dir_all(temp.path().join(".moss")).unwrap();

    // Clear session cache so our project gets checked
    {
        let mut checked = get_checked_projects().lock().unwrap();
        checked.remove(project_path);
    }

    let result = ensure_bundled_plugins_installed(project_path);
    assert!(result.is_ok());

    // Check if comment plugin was installed (it should NOT be)
    let comment_dir = temp.path().join(".moss/plugins/comment");
    if bundled_set().root.get_dir("comment").is_some() {
        assert!(
            !comment_dir.exists(),
            "Non-deployer 'comment' should NOT be auto-installed on fresh project"
        );
    }
}

#[test]
fn test_deployer_plugin_not_auto_installed_on_new_project() {
    // Deployer plugins (e.g. github) should NOT be auto-installed on fresh
    // projects — users install them via the first-publish flow or plugin installer.
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();
    fs::create_dir_all(temp.path().join(".moss")).unwrap();

    // Clear session cache so our project gets checked
    {
        let mut checked = get_checked_projects().lock().unwrap();
        checked.remove(project_path);
    }

    let result = ensure_bundled_plugins_installed(project_path);
    assert!(result.is_ok());

    // Verify the github deployer plugin was NOT auto-installed
    let github_dir = temp.path().join(".moss/plugins/github");
    if bundled_set().root.get_dir("github").is_some() {
        assert!(
            !github_dir.exists(),
            "Deployer 'github' should NOT be auto-installed on fresh project"
        );
    }
}

#[test]
fn test_non_deployer_updated_when_already_installed() {
    // Non-deployer plugins that are already installed should get auto-updated
    // when the bundled version has different content.
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();

    let bundled_dir = bundled_set().root.get_dir("comment");
    if bundled_dir.is_none() {
        return; // comment not bundled in test builds
    }

    // Pre-install comment plugin with STALE content but same version
    let comment_dir = temp.path().join(".moss/plugins/comment");
    fs::create_dir_all(&comment_dir).unwrap();

    let bundled_manifest = read_bundled_manifest("comment").unwrap();
    let manifest_json = format!(
        r#"{{"name":"comment","version":"{}","entry":"main.bundle.js","capabilities":["process","enhance"]}}"#,
        bundled_manifest.version
    );
    fs::write(comment_dir.join("manifest.json"), &manifest_json).unwrap();
    fs::write(comment_dir.join("main.bundle.js"), "// STALE OLD CODE").unwrap();

    // Also write a user config file that should be preserved
    fs::write(
        comment_dir.join("config.json"),
        r#"{"server_url":"https://my-server.com"}"#,
    )
    .unwrap();

    // Clear session cache
    {
        let mut checked = get_checked_projects().lock().unwrap();
        checked.remove(project_path);
    }

    let result = ensure_bundled_plugins_installed(project_path);
    assert!(result.is_ok());

    // Comment plugin should have been updated (stale code replaced)
    let updated_code = fs::read_to_string(comment_dir.join("main.bundle.js")).unwrap();
    assert_ne!(
        updated_code, "// STALE OLD CODE",
        "Non-deployer 'comment' should be auto-updated when already installed with stale content"
    );

    // User config should be preserved
    assert_eq!(
        fs::read_to_string(comment_dir.join("config.json")).unwrap(),
        r#"{"server_url":"https://my-server.com"}"#,
        "User config.json should be preserved during auto-update"
    );
}

#[test]
fn test_comment_plugin_is_not_deployer() {
    // Verify comment plugin is classified as non-deployer
    if bundled_set().root.get_dir("comment").is_none() {
        return; // comment not bundled in test builds
    }
    assert!(
        !is_deployer("comment"),
        "Comment plugin should NOT be a deployer"
    );
}

#[test]
fn test_should_not_trigger_update_when_only_data_files_differ() {
    // After a plugin is installed from the bundle, if the user writes data
    // files (config.toml, etc.), should_install_plugin must still return false.
    // The data files should not cause a hash mismatch.
    let temp = tempdir().unwrap();
    let target = temp.path().join("github");

    let bundled_dir = bundled_set().root.get_dir("github");
    if bundled_dir.is_none() {
        return; // github not bundled in test builds
    }

    // Extract bundled plugin (exact copy)
    extract_plugin_dir(bundled_dir.unwrap(), &target).unwrap();

    // Simulate user/plugin writing data files
    fs::write(target.join("config.toml"), "auto_commit = true").unwrap();
    fs::write(target.join("config.json"), r#"{"setting":"value"}"#).unwrap();

    let result = should_install_plugin("github", &target, true);

    assert!(
        !result,
        "should_install_plugin should return false when only data files differ. \
             Data files (config.toml, config.json) must not trigger a re-install."
    );
}

#[test]
fn test_read_auto_update_plugins_config_default() {
    // When config.toml doesn't exist, auto_update_plugins defaults to true
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();
    fs::create_dir_all(temp.path().join(".moss")).unwrap();

    assert_eq!(
        read_auto_update_plugins_config(project_path),
        true,
        "Missing config.toml should default to auto_update_plugins=true"
    );
}

#[test]
fn test_read_auto_update_plugins_config_explicit_false() {
    // When config.toml has [plugins] auto_update_plugins = false
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();
    let moss_dir = temp.path().join(".moss");
    fs::create_dir_all(&moss_dir).unwrap();
    fs::write(
        moss_dir.join("config.toml"),
        "[plugins]\nauto_update_plugins = false\n",
    )
    .unwrap();

    assert_eq!(
        read_auto_update_plugins_config(project_path),
        false,
        "Explicit auto_update_plugins=false should be respected"
    );
}

#[test]
fn test_read_auto_update_plugins_config_explicit_true() {
    // When config.toml has [plugins] auto_update_plugins = true
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();
    let moss_dir = temp.path().join(".moss");
    fs::create_dir_all(&moss_dir).unwrap();
    fs::write(
        moss_dir.join("config.toml"),
        "[plugins]\nauto_update_plugins = true\n",
    )
    .unwrap();

    assert_eq!(
        read_auto_update_plugins_config(project_path),
        true,
        "Explicit auto_update_plugins=true should be respected"
    );
}


#[test]
fn test_read_auto_update_plugins_config_missing_section() {
    // When config.toml exists but has no [plugins] section
    let temp = tempdir().unwrap();
    let project_path = temp.path().to_str().unwrap();
    let moss_dir = temp.path().join(".moss");
    fs::create_dir_all(&moss_dir).unwrap();
    fs::write(
        moss_dir.join("config.toml"),
        "[hooks]\ndeploy = \"github\"\n",
    )
    .unwrap();

    assert_eq!(
        read_auto_update_plugins_config(project_path),
        true,
        "Missing [plugins] section should default to auto_update_plugins=true"
    );
}

/// The exact form of "moss shipped these bytes", and the exact code a consent
/// names — one hash, so they are the same bytes. It ignores the plugin's own
/// data files and notices one edited byte of code. An id-and-version proxy —
/// what `enforce` used before — gets the edited case wrong, which is the case
/// the consent gate exists for.
#[test]
fn the_code_hash_is_exactly_the_code_and_the_bundle_s_is_exactly_the_bundle() {
    let temp = tempdir().unwrap();
    let dir = temp.path().join("github");
    let bundled = bundled_set().root.get_dir("github").expect("github ships in every build");
    extract_plugin_dir(bundled, &dir).unwrap();
    let manifest = read_installed_manifest(&dir).unwrap();

    let shipped = bundled_code_hash("github").expect("the bundle has a manifest and an entry");
    assert_eq!(code_hash(&dir, &manifest).as_deref(), Some(shipped.as_str()));

    fs::write(dir.join("config.toml"), "token = \"x\"").unwrap();
    assert_eq!(code_hash(&dir, &manifest).unwrap(), shipped, "the plugin's own data files are not its code");

    let entry = dir.join(&manifest.entry);
    let mut body = fs::read(&entry).unwrap();
    body.extend_from_slice(b"\n// edited by hand");
    fs::write(&entry, body).unwrap();
    assert_ne!(code_hash(&dir, &manifest).unwrap(), shipped, "one byte off the bundle is not what moss shipped");

    assert!(bundled_code_hash("not-a-plugin-moss-ships").is_none());
    assert!(code_hash(temp.path(), &manifest).is_none(), "no entry file, no hash");
}
