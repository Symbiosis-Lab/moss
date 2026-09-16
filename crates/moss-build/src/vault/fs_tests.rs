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

// ── Resolution ─────────────────────────────────────────────────────────────

#[test]
fn resolve_under_joins_onto_the_base() {
    let p = PluginPath::sandboxed("posts/hello.md").unwrap();
    assert_eq!(
        p.resolve_under(Path::new("/tmp/site")),
        PathBuf::from("/tmp/site/posts/hello.md")
    );
    assert_eq!(p.as_str(), "posts/hello.md");
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
