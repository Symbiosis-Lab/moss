//! Tests for the two path-guard trust domains.
//!
//! The first-party cases came from `editor/path_guard.rs` (deleted 2026-08-06);
//! the plugin-sandbox cases from `plugins/plugin_config.rs`, where the policy
//! used to be hand-copied at each call site.

use super::*;

fn project_root() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

// ── First-party: path-traversal guard ──────────────────────────────────────

#[test]
fn rejects_dotdot_in_path() {
    let root = project_root();
    let err = validate_entry_path(root.path(), "../escape.md").unwrap_err();
    assert!(err.contains("traversal"), "got: {err}");
}

#[test]
fn rejects_embedded_dotdot() {
    let root = project_root();
    let err = validate_entry_path(root.path(), "/some/root/../escape.md").unwrap_err();
    assert!(err.contains("traversal"), "got: {err}");
}

#[test]
fn rejects_dotdot_in_middle_segment() {
    let root = project_root();
    let path = format!("{}/posts/../escape.md", root.path().display());
    let err = validate_entry_path(root.path(), &path).unwrap_err();
    assert!(err.contains("traversal"), "got: {err}");
}

// ── First-party: project-root containment ──────────────────────────────────

#[test]
fn rejects_path_outside_project() {
    let root = project_root();
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("file.md");
    let err = validate_entry_path(root.path(), path.to_str().unwrap()).unwrap_err();
    assert!(err.contains("within the project"), "got: {err}");
}

#[test]
fn rejects_relative_path() {
    let root = project_root();
    // A relative path never starts_with an absolute project root.
    let err = validate_entry_path(root.path(), "relative/file.md").unwrap_err();
    assert!(err.contains("within the project"), "got: {err}");
}

// ── First-party: happy path ────────────────────────────────────────────────

#[test]
fn accepts_absolute_path_inside_project_and_returns_it() {
    let root = project_root();
    let path = root.path().join("posts").join("hello.md");
    let result = validate_entry_path(root.path(), path.to_str().unwrap());
    assert_eq!(result.unwrap(), path);
}

#[test]
fn accepts_project_root_itself() {
    let root = project_root();
    let result = validate_entry_path(root.path(), root.path().to_str().unwrap());
    // The root IS within itself — boundary check passes.
    // Individual commands may add a "not the root" guard on top.
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
}

/// The first-party door does NOT fence off `.moss/` — that is the whole point
/// of splitting the two domains (moss#997). The editor's file tree can show
/// `.moss/config.toml`, and moss's own panels read their own data.
#[test]
fn first_party_may_reach_moss_internal_paths() {
    let root = project_root();
    let path = root.path().join(".moss").join("config.toml");
    let result = validate_entry_path(root.path(), path.to_str().unwrap());
    assert!(result.is_ok(), "expected Ok, got {:?}", result);
}

// ── Plugin sandbox boundaries (confirmed by pre-merge security review) ─────

/// The identity secret must not be reachable through project-file access.
/// Before this guard, `readFile(".moss/identity/secret-key")` returned the
/// raw hex scalar to any plugin — which falsified the whole "moss holds the
/// key, plugins never see it" contract.
#[test]
fn plugins_cannot_reach_the_identity_secret() {
    for path in [
        ".moss/identity/secret-key",
        "./.moss/identity/secret-key",
        ".MOSS/identity/secret-key",
        ".moss/plugins/other/config.json",
        ".moss/config.toml",
        // Backslash-separated forms normalize the same way, so a Windows-style
        // spelling cannot smuggle the first segment past the fence.
        ".moss\\identity\\secret-key",
    ] {
        assert!(
            PluginPath::sandboxed(path).is_err(),
            "must refuse project-file access to {path}"
        );
    }
}

/// Ordinary content paths stay reachable.
#[test]
fn ordinary_project_paths_are_still_allowed() {
    // `1:x` reads as a drive letter to a naive colon test, and is not one.
    for path in ["posts/hello.md", "assets/img.png", "mossy.md", "a/.moss-notes.md", "1:x.md"] {
        assert!(PluginPath::sandboxed(path).is_ok(), "must allow {path}");
    }
}

#[test]
fn sandboxed_rejects_empty_absolute_traversal_and_nul() {
    assert!(PluginPath::sandboxed("").is_err());
    assert!(PluginPath::sandboxed("/etc/passwd").is_err());
    assert!(PluginPath::sandboxed("\\windows\\system32").is_err());
    // Refused on EVERY platform, not only Windows: a config file naming a
    // drive letter travels between machines (see `is_rooted`).
    assert!(PluginPath::sandboxed("C:\\windows").is_err());
    // Any single letter is a drive, so `a:b.jpg` is drive `a:` — not a
    // filename. Windows forbids `:` in filenames, so nothing legitimate loses.
    assert!(PluginPath::sandboxed("a:b.jpg").is_err());
    assert!(PluginPath::sandboxed("../../etc/passwd").is_err());
    assert!(PluginPath::sandboxed("posts/../../etc/passwd").is_err());
    assert!(PluginPath::sandboxed("image\0.jpg").is_err());
}

/// A plugin must not be able to grant itself capabilities by rewriting the
/// manifest its grants are read from.
#[test]
fn plugins_cannot_rewrite_their_own_manifest() {
    assert!(PluginPath::storage("ipfs", "manifest.json").is_err());
    assert!(PluginPath::storage("ipfs", "./manifest.json").is_err());
    assert!(PluginPath::storage("ipfs", "MANIFEST.JSON").is_err());
    assert!(PluginPath::storage("ipfs", "manifest.json ").is_err());
    assert!(PluginPath::storage("ipfs", "manifest.json::$DATA").is_err());
    // ...but normal plugin storage still works.
    assert!(PluginPath::storage("ipfs", "config.json").is_ok());
    assert!(PluginPath::storage("ipfs", "data/state.json").is_ok());
}

/// The install receipt says which files an install laid down, and an update
/// reads it to tell the last version's code from what the plugin wrote. A
/// plugin that could write it could empty the list and have code a new version
/// deleted carried forward as if the plugin owned it.
#[test]
fn plugins_cannot_rewrite_the_record_of_what_their_install_delivered() {
    for spelling in [
        ".moss-install.json",
        "./.moss-install.json",
        ".MOSS-INSTALL.JSON",
        "data/.moss-install.json",
        // Two spellings Windows opens as the file above: the trailing dot it
        // strips, and the NTFS default data stream. A check that normalized
        // and then compared would let through whichever it had not heard of.
        ".moss-install.json.",
        ".moss-install.json::$DATA",
    ] {
        assert!(
            PluginPath::storage("ipfs", spelling).is_err(),
            "{spelling} must not be writable by the plugin it describes"
        );
    }
    assert!(PluginPath::storage("ipfs", "moss-install.json").is_ok());
}

#[test]
fn plugin_storage_name_must_be_one_segment() {
    for name in ["", "a/b", "a\\b", "..", "a\0b"] {
        assert!(
            PluginPath::storage(name, "config.json").is_err(),
            "must refuse plugin name {name:?}"
        );
    }
}

// ── Shared social-data exception (docs/reference/social-data-standard.md) ──

/// `write_project_file` / `read_project_file` try `shared_social_data` first
/// and fall back to `sandboxed` — so the fallback must still catch traversal
/// spelled through the shared-directory prefix.
#[test]
fn shared_social_data_still_rejects_traversal() {
    assert!(PluginPath::shared_social_data("matters", ".moss/data/social/../../../etc/passwd").is_err());
    assert!(PluginPath::sandboxed(".moss/data/social/../../../etc/passwd").is_err());
}

#[test]
fn shared_social_data_plugin_id_must_be_one_segment() {
    for id in ["", "a/b", "a\\b", "..", "a\0b"] {
        assert!(
            PluginPath::shared_social_data(id, "matters.json").is_err(),
            "must refuse plugin id {id:?}"
        );
    }
}

/// A plugin manifest naming itself "review" would otherwise pass the
/// cross-plugin check trivially — the id and the file it claims genuinely
/// agree — and land on first-party `review.json`
/// (`build::features::review`). The reserved-id list refuses this before
/// the filename comparison ever runs, case-insensitively (manifest names
/// are conventionally lowercase, but the fence does not trust that).
#[test]
fn shared_social_data_rejects_the_reserved_review_id() {
    for id in ["review", "Review", "REVIEW"] {
        assert!(
            PluginPath::shared_social_data(id, ".moss/data/social/review.json").is_err(),
            "must refuse reserved plugin id {id:?}"
        );
        assert!(
            PluginPath::shared_social_data(id, ".moss/social/review.json").is_err(),
            "must refuse reserved plugin id {id:?} in the legacy directory too"
        );
    }
}

/// The example cases this rule must get right — own file, sibling's file,
/// first-party `review.json`, the legacy directory, no plugin identity — are
/// not hand-duplicated here. This test and the TS mock's own test
/// (`plugins/matters/src/__tests__/social-integration.test.ts`) both read
/// `open/fixtures/social-data-fence-cases.json`, so the Rust guard and its
/// TS mirror cannot silently drift the way the mock and the real guard once
/// did (the regression this whole exception exists to fix).
///
/// `allowed` mirrors what `resolve_plugin_project_path` actually decides:
/// the shared door when a plugin id is given, falling back to the full
/// sandbox fence — not `shared_social_data` in isolation, so an ordinary
/// content path (no plugin id needed) is exercised too.
#[test]
fn shared_social_data_fixture_cases() {
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join("social-data-fence-cases.json");
    let raw = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", fixture_path.display()));
    let cases: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
    assert!(!cases.is_empty(), "fixture must not be empty");

    for case in &cases {
        let plugin_id = case["pluginId"].as_str();
        let path = case["path"].as_str().unwrap();
        let expected_allowed = case["allowed"].as_bool().unwrap();
        let note = case["note"].as_str().unwrap_or("");

        let actual_allowed = plugin_id
            .is_some_and(|id| PluginPath::shared_social_data(id, path).is_ok())
            || PluginPath::sandboxed(path).is_ok();

        assert_eq!(
            actual_allowed, expected_allowed,
            "pluginId={plugin_id:?} path={path:?} ({note}): expected allowed={expected_allowed}, got {actual_allowed}"
        );
    }
}

// ── Resolution ─────────────────────────────────────────────────────────────

#[test]
fn resolve_under_joins_onto_the_base() {
    // A non-existent base (nothing on disk could have been symlinked) falls
    // back to the lexical join — see the symlink-containment tests below for
    // the case where `base` does exist.
    let p = PluginPath::sandboxed("posts/hello.md").unwrap();
    assert_eq!(
        p.resolve_under(Path::new("/tmp/site-that-does-not-exist-in-this-test")).unwrap(),
        PathBuf::from("/tmp/site-that-does-not-exist-in-this-test/posts/hello.md")
    );
    assert_eq!(p.as_str(), "posts/hello.md");
}

// ── resolve_under: symlink containment ──────────────────────────────────────
//
// A validated PluginPath proves the SPELLING is safe; base.join alone never
// touches disk, so a symlinked directory component can redirect a
// lexically-safe path to a real location the caller was never granted.

/// The reviewer's exact scenario: `.moss/data/social` symlinked to
/// `.moss/identity` would otherwise let the shared social-data exception —
/// bound to `base` = the vault root, where `.moss/identity/` is a legitimate
/// subpath — write into the identity directory despite every string-level
/// check on `shared_social_data` passing.
#[cfg(unix)]
#[test]
fn shared_social_data_write_refused_when_its_directory_is_symlinked_into_identity() {
    let root = project_root();
    let identity_dir = root.path().join(".moss").join("identity");
    std::fs::create_dir_all(&identity_dir).unwrap();
    let secret_file = identity_dir.join("secret-key");
    std::fs::write(&secret_file, "d34db33f").unwrap();

    let data_dir = root.path().join(".moss").join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::os::unix::fs::symlink(&identity_dir, data_dir.join("social")).unwrap();

    let plugin_path = PluginPath::shared_social_data("matters", ".moss/data/social/matters.json").unwrap();
    let err = plugin_path
        .resolve_under(root.path())
        .expect_err("a symlinked .moss/data/social pointing into .moss/identity must be refused");
    assert!(err.contains("identity"), "got: {err}");

    // The refusal must be real, not cosmetic: nothing was written through
    // the symlink, and the secret is exactly what it was before.
    assert_eq!(
        std::fs::read_to_string(&secret_file).unwrap(),
        "d34db33f",
        "a refused resolve_under must not have touched the identity directory"
    );
    assert!(
        !identity_dir.join("matters.json").exists(),
        "the plugin's file must not have landed inside .moss/identity/"
    );
}

/// The same class of escape, but leaving the vault entirely — containment
/// under `base` must hold even when the target is not `.moss/identity/`.
#[cfg(unix)]
#[test]
fn resolve_under_refuses_a_symlink_escaping_the_vault_root() {
    let root = project_root();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("posts")).unwrap();

    let plugin_path = PluginPath::sandboxed("posts/hello.md").unwrap();
    let err = plugin_path
        .resolve_under(root.path())
        .expect_err("a symlinked directory pointing outside the vault must be refused");
    assert!(err.contains("outside"), "got: {err}");
    assert!(!outside.path().join("hello.md").exists());
}

/// The containment check must not refuse a symlink that resolves to
/// somewhere ELSE inside `base` — otherwise this is a refusal of every
/// symlink rather than of the ones that escape.
#[cfg(unix)]
#[test]
fn resolve_under_allows_a_symlink_that_stays_inside_base() {
    let root = project_root();
    let real_dir = root.path().join("real-posts");
    std::fs::create_dir_all(&real_dir).unwrap();
    std::os::unix::fs::symlink(&real_dir, root.path().join("posts")).unwrap();

    let plugin_path = PluginPath::sandboxed("posts/hello.md").unwrap();
    let resolved = plugin_path
        .resolve_under(root.path())
        .expect("a symlink resolving inside the vault must be allowed");
    assert_eq!(resolved, root.path().join("posts").join("hello.md"));
}

/// A brand-new plugin's storage directory (or the vault root of a project
/// that was just opened) legitimately does not exist yet at the moment
/// `resolve_under` runs — `write_file_with_dirs`'s `create_dir_all` creates
/// it AFTER this call returns. There is nothing on disk to have been
/// symlinked, so this must fall back to the lexical join rather than
/// refusing every plugin's first write.
#[test]
fn resolve_under_allows_writes_when_base_does_not_exist_yet() {
    let root = project_root();
    let not_yet_created = root.path().join("does-not-exist-yet");

    let plugin_path = PluginPath::storage("ipfs", "config.json").unwrap();
    let resolved = plugin_path
        .resolve_under(&not_yet_created)
        .expect("a not-yet-created base must not refuse the write");
    assert_eq!(resolved, not_yet_created.join("config.json"));
}

// ── Entry creation guards (create_file_inner / create_folder_inner) ────────

#[test]
fn sanitize_entry_name_rejects_traversal_and_separators() {
    assert!(sanitize_entry_name("../evil").is_err());
    assert!(sanitize_entry_name("a/b").is_err());
    assert!(sanitize_entry_name("a\\b").is_err());
    assert!(sanitize_entry_name("..").is_err(), "trailing-dot trim collapses '..' to empty");
    assert!(sanitize_entry_name(".").is_err());
    assert!(sanitize_entry_name("").is_err());
    assert!(sanitize_entry_name("   ").is_err());
    assert!(sanitize_entry_name("nul\0byte").is_err());
}

#[test]
fn sanitize_entry_name_trims_but_keeps_ordinary_names() {
    assert_eq!(sanitize_entry_name("  hello  ").unwrap(), "hello");
    assert_eq!(sanitize_entry_name("notes.").unwrap(), "notes");
    assert_eq!(sanitize_entry_name("交互").unwrap(), "交互");
    assert_eq!(sanitize_entry_name(".env").unwrap(), ".env");
}

#[test]
fn ensure_page_extension_appends_md_only_when_no_extension() {
    assert_eq!(ensure_page_extension("untitled".into()), "untitled.md");
    assert_eq!(ensure_page_extension("v1.2".into()), "v1.2.md");
    assert_eq!(ensure_page_extension("hello.md".into()), "hello.md");
    assert_eq!(ensure_page_extension("notes.txt".into()), "notes.txt");
    assert_eq!(ensure_page_extension("archive.tar.gz".into()), "archive.tar.gz");
    assert_eq!(ensure_page_extension("交互".into()), "交互.md");
}

#[test]
fn create_file_appends_md_and_refuses_overwrite() {
    let tmp = project_root();
    let root = tmp.path();
    let dir = root.to_string_lossy().to_string();

    let created = create_file_inner(root, &dir, "untitled").unwrap();
    assert!(created.ends_with("untitled.md"), "got {created}");
    // Monotonic mode: the filename IS the title — new files are EMPTY
    // (no frontmatter delimiters, no `title:` field).
    assert_eq!(std::fs::read_to_string(root.join("untitled.md")).unwrap(), "");

    // No-overwrite: the collision names the SANITIZED path, and the
    // frontend keys its localized message off "already exists".
    let err = create_file_inner(root, &dir, "untitled.md").unwrap_err();
    assert!(err.contains("already exists"), "got {err}");
    let err = create_file_inner(root, &dir, "untitled").unwrap_err();
    assert!(
        err.contains("already exists"),
        ".md enforcement must apply before the collision check; got {err}"
    );
}

#[test]
fn create_file_rejects_escape_attempts() {
    let tmp = project_root();
    let root = tmp.path();
    let dir = root.to_string_lossy().to_string();

    assert!(create_file_inner(root, &dir, "../outside").is_err());
    assert!(
        create_file_inner(root, "/somewhere/else", "x").is_err(),
        "dir outside the project root must be rejected"
    );
    assert!(!root.parent().unwrap().join("outside.md").exists());
}

#[test]
fn create_files_writes_all_or_nothing_and_makes_the_folder() {
    let tmp = project_root();
    let root = tmp.path();
    let dir = root.join("作者").to_string_lossy().to_string();
    let note = |name: &str, fm: serde_json::Value| NewFile {
        dir: dir.clone(),
        name: name.into(),
        frontmatter: fm.as_object().cloned().unwrap(),
    };
    let home = || note("作者", serde_json::json!({ "url": "authors", "listed": false }));
    let claim = |name: &str| note(name, serde_json::json!({ "author_page": "馬欣宜" }));

    // The second file fails validation, so the first is not written either.
    assert!(create_files_inner(root, &[home(), claim("../escape")]).is_err());
    assert!(create_files_inner(root, &[home(), home()]).is_err(), "one path twice would silently overwrite");
    assert!(!root.join("作者").exists(), "a rejected batch leaves no folder behind");

    let paths = create_files_inner(root, &[home(), claim("馬欣宜")]).unwrap();
    assert_eq!(paths.len(), 2);
    assert!(paths[1].ends_with("馬欣宜.md"), "paths come back in order, the one to open last");
    let written = std::fs::read_to_string(&paths[0]).unwrap();
    assert!(written.starts_with("---\n") && written.contains("url: authors") && written.contains("listed: false"), "{written}");
    assert_eq!(std::fs::read_to_string(&paths[1]).unwrap(), "---\nauthor_page: 馬欣宜\n---\n");
}

#[test]
fn create_folder_sanitizes_and_refuses_overwrite() {
    let tmp = project_root();
    let root = tmp.path();
    let dir = root.to_string_lossy().to_string();

    let created = create_folder_inner(root, &dir, "  works  ").unwrap();
    assert!(created.ends_with("works"), "got {created}");
    assert!(root.join("works").is_dir(), "no .md enforcement for folders");

    let err = create_folder_inner(root, &dir, "works").unwrap_err();
    assert!(err.contains("already exists"), "got {err}");
    assert!(create_folder_inner(root, &dir, "a/b").is_err());

    // A FILE with the target name is a collision too.
    std::fs::write(root.join("conflict"), "content").unwrap();
    let err = create_folder_inner(root, &dir, "conflict").unwrap_err();
    assert!(err.contains("already exists"), "got {err}");
}

/// Deleting an entry that is already gone succeeds: the state the delete
/// asked for — "not in the vault" — already holds. The 2026-09-05 shape:
/// Drive sync (or a first, slower delete) trashed `untitled.md` moments
/// before the command landed, and `recheck_canonical` turned the achieved
/// goal into a "Failed to resolve … os error 2" dialog.
#[test]
fn deleting_an_already_gone_entry_is_ok() {
    let root = project_root();
    let gone = root.path().join("posts").join("untitled.md");
    delete_entry_inner(root.path(), &gone.to_string_lossy()).unwrap();
}

/// The idempotency above must not weaken the guards: a traversal spelling or
/// an outside-root path is refused even when nothing exists there.
#[test]
fn already_gone_does_not_bypass_containment() {
    let root = project_root();
    let err = delete_entry_inner(root.path(), "/elsewhere/gone.md").unwrap_err();
    assert!(err.contains("within the project"), "got: {err}");
    let traversal = format!("{}/../gone.md", root.path().display());
    let err = delete_entry_inner(root.path(), &traversal).unwrap_err();
    assert!(err.contains("traversal"), "got: {err}");
}

// ── Not-yet-existing destinations ──────────────────────────────────────────
//
// `validate_entry_path` is lexical by design, so it passes a path whose
// spelling starts with the root even when a symlinked component sends the
// real write somewhere else. These pin the recheck that closes that.

/// `Path::exists()` FOLLOWS symlinks, so a dangling one reads as absent. A
/// walk-up that used it stepped over the single component that redirects the
/// write, and `save_editor_content` then created the link's target — an
/// attacker-chosen absolute path that does not exist yet, which is the shape
/// of `~/.ssh/authorized_keys` on a machine without one.
#[cfg(unix)]
#[test]
fn a_new_file_at_a_dangling_symlink_cannot_escape_the_vault() {
    let root = project_root();
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("planted.md"); // deliberately absent
    std::os::unix::fs::symlink(&victim, root.path().join("notes.md")).unwrap();

    // Through `validate_entry_path`, the guard every first-party caller
    // shares, not through the helper directly — the helper is an
    // implementation detail and a test of it proves nothing about the doors.
    let target = root.path().join("notes.md");
    let err = validate_entry_path(root.path(), &target.to_string_lossy())
        .expect_err("a dangling symlink out of the vault must be refused");
    assert!(err.contains("resolve") || err.contains("within the project"), "got: {err}");
    assert!(!victim.exists(), "nothing may be created outside the vault");
}

/// The same shape, but resolving back INSIDE the vault, must still be allowed
/// — otherwise the fix above is a refusal of every symlink rather than of the
/// ones that escape.
#[cfg(unix)]
#[test]
fn a_symlink_resolving_inside_the_vault_is_allowed() {
    let root = project_root();
    let inside = root.path().join("real.md");
    std::fs::write(&inside, "x").unwrap();
    std::os::unix::fs::symlink(&inside, root.path().join("alias.md")).unwrap();

    validate_entry_path(root.path(), &root.path().join("alias.md").to_string_lossy())
        .expect("a symlink resolving inside the vault must be allowed");
}

#[cfg(unix)]
#[test]
fn create_folder_cannot_write_through_a_symlinked_vault_dir() {
    let root = project_root();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("drafts")).unwrap();

    let err = create_folder_inner(
        root.path(),
        &root.path().join("drafts").to_string_lossy(),
        "stolen",
    )
    .unwrap_err();
    assert!(
        err.contains("within the project directory"),
        "create_folder must refuse a symlinked parent, got: {err}"
    );
    assert!(
        !outside.path().join("stolen").exists(),
        "nothing may be created outside the vault"
    );
}

#[cfg(unix)]
#[test]
fn create_file_cannot_write_through_a_symlinked_vault_dir() {
    let root = project_root();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("drafts")).unwrap();

    let err = create_file_inner(
        root.path(),
        &root.path().join("drafts").to_string_lossy(),
        "stolen",
    )
    .unwrap_err();
    assert!(
        err.contains("within the project directory"),
        "create_file must refuse a symlinked parent, got: {err}"
    );
    assert!(
        !outside.path().join("stolen.md").exists(),
        "nothing may be created outside the vault"
    );
}

#[test]
fn a_new_file_in_a_real_vault_dir_is_allowed() {
    let root = project_root();
    std::fs::create_dir(root.path().join("posts")).unwrap();
    let target = root.path().join("posts").join("new.md");
    recheck_canonical_allowing_missing(root.path(), &target)
        .expect("a new file beside real files must be allowed");
}

#[test]
fn a_new_file_in_nested_not_yet_created_dirs_is_allowed() {
    // Nothing below the root exists yet. A component that does not exist
    // cannot be a symlink, so this must not be refused.
    let root = project_root();
    let target = root.path().join("a").join("b").join("new.md");
    recheck_canonical_allowing_missing(root.path(), &target)
        .expect("a deep new path inside the vault must be allowed");
}

#[test]
fn delete_entry_inner_rejects_the_project_root_itself() {
    let dir = tempfile::tempdir().unwrap();
    let err = delete_entry_inner(dir.path(), dir.path().to_str().unwrap()).unwrap_err();
    assert!(err.contains("Cannot delete the project root"), "{err}");
}

// ── NsFileManager-trashItemAtURL-failure classification ─────────────────────
//
// The fixture below is the client's actual `trash::Error` Display text
// (moss's error message put it inside a nested toast), captured verbatim so
// a future change to `is_ns_file_manager_trash_failure`'s matching can't
// silently stop recognizing the real-world failure that motivated it:
// `trashItemAtURL` reported that the iCloud Drive volume has no Trash of its
// own. A first version of this classifier also required Apple's English
// "doesn't have one" phrase, which is `NSError.localizedDescription` text
// and therefore translated on the very machines — Chinese-language clients —
// this bug affects. `localized_no_trash_volume_error` below pins that: a
// plausible Chinese rendering of the same failure, keeping the crate's own
// (English, locale-stable) preamble, must still be recognized.

/// `trash::Error::Display` is `"Error during a \`trash\` operation: {self:?}"`
/// (see `trash-5.2.9/src/lib.rs`), and the macOS backend builds `Unknown`'s
/// description as `"While deleting '{path:?}', \`trashItemAtURL\` failed:
/// {err}"` (see `trash-5.2.9/src/macos/mod.rs`) — reconstructing that shape
/// here, rather than only matching on a substring, is what makes this test
/// pin the crate's actual output format and not just the words we search for.
fn client_no_trash_volume_error() -> trash::Error {
    trash::Error::Unknown {
        description: "While deleting '\"/Users/liuguo/Library/Mobile Documents/com~apple~CloudDocs/刘兆永的文档/刘兆永的网站/文稿/测试.md\"', `trashItemAtURL` failed: \u{201c}测试.md\u{201d} couldn't be moved to the trash because the volume \u{201c}Macintosh HD\u{201d} doesn't have one.".to_string(),
    }
}

/// The same real-world failure, but with the `NSError` tail rendered the
/// way macOS would on a Chinese-locale Mac (the crate's own English
/// preamble is untouched — only `localizedDescription` is ever translated).
fn localized_no_trash_volume_error() -> trash::Error {
    trash::Error::Unknown {
        description: "While deleting '\"/Users/liuguo/Library/Mobile Documents/com~apple~CloudDocs/刘兆永的文档/刘兆永的网站/文稿/测试.md\"', `trashItemAtURL` failed: 无法将\u{201c}测试.md\u{201d}移到废纸篓，因为\u{201c}Macintosh HD\u{201d}卷没有废纸篓。".to_string(),
    }
}

#[test]
fn recognizes_the_clients_trash_item_at_url_failure() {
    let e = client_no_trash_volume_error();
    // Pin the exact real-world text this classifier must keep recognizing.
    assert_eq!(
        e.to_string(),
        "Error during a `trash` operation: Unknown { description: \"While deleting '\\\"/Users/liuguo/Library/Mobile Documents/com~apple~CloudDocs/刘兆永的文档/刘兆永的网站/文稿/测试.md\\\"', `trashItemAtURL` failed: \u{201c}测试.md\u{201d} couldn't be moved to the trash because the volume \u{201c}Macintosh HD\u{201d} doesn't have one.\" }"
    );
    assert!(
        is_ns_file_manager_trash_failure(&e),
        "must recognize the client's real-world trashItemAtURL failure"
    );
}

/// The one that matters: on a Chinese-locale Mac, `NSError.localizedDescription`
/// arrives translated, so a classifier that requires the English "doesn't
/// have one" text never fires for moss's actual (Chinese-language) affected
/// users. Matching only the crate's own English preamble must still work.
#[test]
fn recognizes_a_localized_rendering_of_the_same_failure() {
    let e = localized_no_trash_volume_error();
    assert!(
        is_ns_file_manager_trash_failure(&e),
        "must recognize the failure even when NSError's localizedDescription is not English"
    );
}

#[test]
fn recognizes_any_ns_file_manager_trash_item_at_url_failure_not_just_missing_trash() {
    // A different trashItemAtURL failure (permission denied). There is no
    // error code to distinguish it from the "no Trash" case, and Finder
    // might succeed where NsFileManager doesn't for reasons this crate
    // can't tell us — so the fallback fires here too, deliberately.
    let e = trash::Error::Unknown {
        description: "While deleting '\"/tmp/foo.md\"', `trashItemAtURL` failed: \u{201c}foo.md\u{201d} couldn't be moved to the trash because you don\u{2019}t have permission to access it.".to_string(),
    };
    assert!(is_ns_file_manager_trash_failure(&e));
}

#[test]
fn does_not_misclassify_a_failure_that_never_reached_trash_item_at_url() {
    // Errors from the crate's own pre-flight path canonicalization (e.g. the
    // target vanished between our check and its) carry no `trashItemAtURL`
    // preamble. Retrying via Finder would hit the identical failure, since
    // Finder's route canonicalizes the same way before it ever runs.
    let e = trash::Error::CouldNotAccess { target: "/tmp/gone.md".to_string() };
    assert!(!is_ns_file_manager_trash_failure(&e));

    let os_err = trash::Error::Os { code: 1, description: "some other os error".to_string() };
    assert!(!is_ns_file_manager_trash_failure(&os_err));
}
