//! Unit tests for the watcher's subscription set (#960).
//!
//! The predicates moved here from `watch_tests.rs` keep their coverage there;
//! what is new is `watch_targets`, whose whole job is to never name a path moss
//! writes. The end-to-end proof — a real build producing zero events on a real
//! watcher — is `tests/watch_self_trigger_test.rs`.

use super::*;
use std::fs;
use tempfile::TempDir;

/// A project laid out the way a real one is by the time a build has run.
fn fixture() -> TempDir {
    let dir = tempfile::Builder::new().prefix("moss_scope").tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("index.md"), "# home").unwrap();
    fs::write(root.join("AGENTS.md"), "moss wrote this").unwrap();
    fs::create_dir_all(root.join("posts")).unwrap();
    fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
    fs::create_dir_all(root.join(".git/objects")).unwrap();
    for sub in [
        "build/staging",
        "build/generations/abc",
        "build/cache/objects",
        "identity",
        "keys",
        "plugins/github",
        "data/social",
        "theme/fonts",
        "assets",
    ] {
        fs::create_dir_all(root.join(".moss").join(sub)).unwrap();
    }
    fs::write(root.join(".moss/config.toml"), "").unwrap();
    // Finder's custom-icon carrier. moss writes it into the project root itself
    // on every publish (`stamp_published_folder`).
    // CR is an invalid filename character on Windows; the carrier (and the
    // publish that writes it) is macOS-only anyway.
    #[cfg(unix)]
    fs::write(root.join("Icon\r"), "").unwrap();
    dir
}

fn targets_of(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    watch_targets(root).into_iter().map(|(p, _)| p).collect()
}

/// The invariant, stated over the registration set instead of the event
/// stream: nothing moss writes is ever subscribed to.
#[test]
fn watch_targets_never_name_a_moss_written_path() {
    let dir = fixture();
    let root = dir.path();
    for path in targets_of(root) {
        let rel = path.strip_prefix(root).unwrap().to_string_lossy().to_string();
        // The root itself (Linux/Windows, NonRecursive) is the one target with
        // an empty relative path.
        if rel.is_empty() {
            continue;
        }
        assert!(
            crate::infra::moss_paths::is_watchable_rel(&rel),
            "watch_targets subscribed to {rel:?}, which the registry says moss owns — \
             a build writing there re-triggers the build (#960)"
        );
    }
}

#[test]
fn watch_targets_cover_content_and_the_user_editable_moss_files() {
    let dir = fixture();
    let root = dir.path();
    let targets = targets_of(root);

    assert!(targets.contains(&root.join("posts")), "content folders must be watched");
    for editable in [".moss/config.toml", ".moss/theme", ".moss/assets", ".moss/data/social"] {
        assert!(
            targets.contains(&root.join(editable)),
            "{editable} is the user's to edit and must stay watched"
        );
    }
}

#[test]
fn watch_targets_exclude_build_output_dotdirs_and_node_modules() {
    let dir = fixture();
    let root = dir.path();
    let targets = targets_of(root);

    for excluded in [".moss", ".moss/build.nosync", ".moss/identity", ".git", "node_modules", "AGENTS.md"]
    {
        assert!(
            !targets.contains(&root.join(excluded)),
            "{excluded} must not be watched"
        );
    }
}

/// Root-level markdown is content, and the two platform families cover it
/// differently — see the module doc. Whichever way, an edit to `index.md` has
/// to be observable.
#[test]
fn root_level_files_are_covered_on_every_platform() {
    let dir = fixture();
    let root = dir.path();
    let targets = watch_targets(root);

    if cfg!(target_os = "macos") {
        assert!(
            targets.iter().any(|(p, _)| *p == root.join("index.md")),
            "macOS cannot watch the root at all, so root-level files are individual targets"
        );
    } else {
        assert!(
            targets
                .iter()
                .any(|(p, m)| p == root && *m == RecursiveMode::NonRecursive),
            "a non-recursive root watch covers root-level files where the OS honours the mode"
        );
    }
}

/// Subscribing to a root-level file costs a rebuild that subscribing to a
/// directory does not: the reconciler treats a newly appeared target as a
/// content change and so bypasses every per-event filter. `Icon\r` is written
/// into the project root by moss's own publish, so admitting it would make
/// publishing rebuild the site it just published — instance 6 of the class
/// #960 is about.
#[test]
fn root_level_files_moss_writes_are_never_targets() {
    let dir = fixture();
    let root = dir.path();
    let targets = targets_of(root);

    assert!(
        !targets.contains(&root.join("Icon\r")),
        "moss writes Icon\\r on publish; watching it makes publishing trigger a rebuild"
    );
    assert!(
        !targets.contains(&root.join("AGENTS.md")),
        "AGENTS.md is moss-written and in the registry"
    );
}

/// The reconciler diffs `watch_targets` against what it already holds, so two
/// calls with nothing changed must be byte-identical or every tick churns the
/// whole subscription set.
#[test]
fn watch_targets_are_stable_across_calls() {
    let dir = fixture();
    assert_eq!(watch_targets(dir.path()), watch_targets(dir.path()));
}

#[test]
fn watch_targets_pick_up_a_new_top_level_folder() {
    let dir = fixture();
    let root = dir.path();
    assert!(!targets_of(root).contains(&root.join("essays")));
    fs::create_dir(root.join("essays")).unwrap();
    assert!(
        targets_of(root).contains(&root.join("essays")),
        "a folder dropped into the project must become a target on the next reconcile"
    );
}

/// Instance 3 of the class (#955), now caught at the event filter rather than
/// by a hand-maintained second list: a root `AGENTS.md` is excluded from watch
/// triggers regardless of who wrote it, and on Linux/Windows the non-recursive
/// root watch would otherwise hear every write to it.
#[test]
fn root_agent_files_are_recognised_as_moss_written() {
    let root = Path::new("/site");
    assert!(all_paths_moss_written(root, &[root.join("AGENTS.md")]));
    assert!(all_paths_moss_written(root, &[root.join(".moss/build.nosync/staging/i.html")]));

    // …but the same name one directory down is ordinary content, which is what
    // `editor::filesystem::a_nested_agents_md_is_an_ordinary_file` pins.
    assert!(!all_paths_moss_written(root, &[root.join("posts/AGENTS.md")]));
    assert!(!all_paths_moss_written(root, &[root.join("index.md")]));

    // A mixed event still counts: one real content path is enough to rebuild.
    assert!(!all_paths_moss_written(
        root,
        &[root.join("AGENTS.md"), root.join("index.md")]
    ));

    // The pathless rescan event makes no claim and must not be swallowed here.
    assert!(!all_paths_moss_written(root, &[]));
    // Neither must a path outside the project.
    assert!(!all_paths_moss_written(root, &[PathBuf::from("/elsewhere/x.md")]));
}

/// A `.moss/` path that no rule mentions — a directory a future moss version
/// writes — must be excluded before anyone remembers to add it. This is the
/// generalisation of the four exclusions that were each added after the fact.
#[test]
fn unknown_moss_subdirectories_are_not_watched() {
    let dir = fixture();
    let root = dir.path();
    fs::create_dir(root.join(".moss/something-new")).unwrap();
    assert!(!targets_of(root).contains(&root.join(".moss/something-new")));
    assert!(!should_watch_moss_file("something-new/state.json"));
}

/// Bug: `should_watch_file` judged its "is this a directory" heuristic over
/// the WHOLE absolute path. A project living at a dotted path
/// (`~/Sites/example.com/`) made every directory beneath it look like a file
/// with extension `com/photos`, so directory create/rename/delete events were
/// filtered out and never triggered a rebuild. The heuristic must be scoped
/// to the final path component only.
#[test]
fn dotted_ancestor_directories_do_not_defeat_the_directory_heuristic() {
    // Directories under a dotted project root are still directories.
    assert!(should_watch_file("/Users/x/Sites/example.com/photos"));
    assert!(should_watch_file("/Users/x/Sites/example.com/photos/"));
    // Files under a dotted root keep their extension semantics.
    assert!(should_watch_file("/Users/x/Sites/example.com/note.md"));
    assert!(!should_watch_file("/Users/x/Sites/example.com/note.xyz"));
    // A bare top-level dir is unchanged.
    assert!(should_watch_file("/Users/x/Sites/plainsite"));
    assert!(should_watch_file("photos"));
    // A dotted FINAL component with a non-content extension is still a file.
    assert!(!should_watch_file("/Users/x/Sites/example.com/archive.zip"));
}

/// Every extension the build pipeline consumes must pass the watcher filter —
/// otherwise editing such a file never triggers a rebuild.
#[test]
fn every_pipeline_consumed_extension_passes_the_watch_filter() {
    // Images the editor is notified about (watch.rs IMAGE_EXTENSIONS).
    for ext in super::super::IMAGE_EXTENSIONS {
        assert!(
            should_watch_file(&format!("/p/site/img.{ext}")), // allow:served-path-url-construct — a filesystem path handed to the watch filter, not an asset URL
            "image extension `{ext}` is consumed but filtered out"
        );
    }
    // Videos the media pipeline converts (media/pipeline.rs video_exts).
    for ext in ["mov", "mp4", "webm", "avi", "mkv", "m4v"] {
        assert!(
            should_watch_file(&format!("/p/site/clip.{ext}")), // allow:served-path-url-construct — a filesystem path handed to the watch filter, not an asset URL
            "video extension `{ext}` is consumed but filtered out"
        );
    }
    // Notebooks are rendered by the pipeline (build/notebook.rs).
    assert!(should_watch_file("/p/site/analysis.ipynb"));
}

/// A first-level directory that owns its own `.moss/` is a different site:
/// the outer vault must not subscribe to its subtree at all.
#[test]
fn watch_targets_skip_a_nested_moss_site() {
    let dir = fixture();
    let root = dir.path();
    fs::create_dir_all(root.join("inner-site/.moss")).unwrap();
    fs::write(root.join("inner-site/post.md"), "# inner").unwrap();

    let targets = targets_of(root);
    assert!(
        !targets.contains(&root.join("inner-site")),
        "a nested moss site must not be watched by the outer vault"
    );
    assert!(targets.contains(&root.join("posts")), "siblings stay watched");
}

/// The per-event half of the boundary: registration cannot reach a vault
/// nested two levels down (it lives inside a recursive target), so the event
/// filter has to recognize it.
#[test]
fn path_in_nested_vault_probes_every_level_between_root_and_path() {
    let dir = fixture();
    let root = dir.path();
    fs::create_dir_all(root.join("posts/inner/.moss")).unwrap();
    fs::write(root.join("posts/inner/note.md"), "# n").unwrap();

    assert!(path_in_nested_vault(root, &root.join("posts/inner/note.md")));
    assert!(path_in_nested_vault(root, &root.join("posts/inner/.moss/config.toml")));
    // The vault dir itself is the boundary, not inside it — the outer vault
    // may still hear (and ignore via registration) its creation.
    assert!(!path_in_nested_vault(root, &root.join("posts/inner")));
    assert!(!path_in_nested_vault(root, &root.join("posts/other.md")));
    assert!(!path_in_nested_vault(root, &root.join("index.md")));
    // Outside the root entirely.
    assert!(!path_in_nested_vault(root, std::path::Path::new("/elsewhere/x.md")));
}

#[test]
fn all_paths_in_nested_vault_requires_every_path_inside_and_a_nonempty_event() {
    let dir = fixture();
    let root = dir.path();
    fs::create_dir_all(root.join("inner/.moss")).unwrap();

    let inside = root.join("inner/a.md");
    let outside = root.join("index.md");
    assert!(all_paths_in_nested_vault(root, &[inside.clone()]));
    assert!(!all_paths_in_nested_vault(root, &[inside, outside]));
    assert!(!all_paths_in_nested_vault(root, &[]));
}

// ---------------------------------------------------------------------------
// watch_set_content_change — the sweep's every-tick top-level verdict.
// Ported from reconcile_tests.rs when phase 2 moved the verdict out of the
// watcher's select loop; the carve-outs are unchanged.
// ---------------------------------------------------------------------------

fn social_dir() -> PathBuf {
    PathBuf::from("/vault/.moss/data/social")
}

#[test]
fn a_deleted_top_level_entry_is_a_content_change() {
    let prev = vec![PathBuf::from("/vault/posts"), PathBuf::from("/vault/notes")];
    let desired = vec![PathBuf::from("/vault/posts")];
    assert!(watch_set_content_change(&prev, &desired, true, &social_dir()));
}

#[test]
fn an_appeared_top_level_entry_is_a_content_change() {
    // Its files were created before any watch landed on it, so their events
    // are gone — the appearance is the only signal left.
    let prev = vec![PathBuf::from("/vault/posts")];
    let desired = vec![PathBuf::from("/vault/posts"), PathBuf::from("/vault/new")];
    assert!(watch_set_content_change(&prev, &desired, true, &social_dir()));
}

#[test]
fn an_unchanged_set_is_not_a_content_change() {
    let set = vec![PathBuf::from("/vault/posts")];
    assert!(!watch_set_content_change(&set, &set.clone(), true, &social_dir()));
}

#[test]
fn root_gone_masks_every_disappearance() {
    // The vault root itself being unreadable makes everything "disappear" at
    // once. That is the project-unavailable verdict's problem; rebuilding a
    // missing project helps nobody.
    let prev = vec![PathBuf::from("/vault/posts"), PathBuf::from("/vault/notes")];
    assert!(!watch_set_content_change(&prev, &[], false, &social_dir()));
}

#[test]
fn social_changes_are_exempt_both_ways() {
    // Background social sync owns .moss/data/social/ and decides for itself
    // whether the preview refreshes.
    let with = vec![PathBuf::from("/vault/posts"), social_dir()];
    let without = vec![PathBuf::from("/vault/posts")];
    assert!(!watch_set_content_change(&with, &without, true, &social_dir()));
    assert!(!watch_set_content_change(&without, &with, true, &social_dir()));
}

// ── The relocation invariant ──────────────────────────────────────────────
//
// A predicate that judges a vault path must judge the path INSIDE the vault.
// The dotfile rule votes on every component it is shown, so an absolute path
// let every directory the user happens to keep the vault under cast a vote:
// a Google shared-drive vault lives below `.shortcut-targets-by-id`, and the
// whole vault was therefore condemned — the rebuild pump dropped every event,
// and the sweep that backstops the pump saw nothing either (#1080, #1067).
//
// The invariant, stated so it outlives the five call sites that had the bug:
// **a predicate's verdict on a file does not change when the vault is
// relocated.** The table below crosses every mount shape in
// [`VAULT_MOUNTS`] with files on both sides of every rule.

/// `(vault-relative path, watchable?, passes the extension filter?)`.
const FILES: &[(&str, bool, bool)] = &[
    // Ordinary content and assets — the population live preview exists for.
    ("index.md", true, true),
    ("notes/post.md", true, true),
    ("images/cover.jpg", true, true),
    ("clips/demo.mov", true, true),
    ("analysis.ipynb", true, true),
    // moss's own inputs that the user edits: watched by allowlist.
    (".moss/config.toml", true, true),
    (".moss/theme/style.css", true, true),
    (".moss/data/social/matters.json", true, true),
    // Genuinely excluded, and they must STAY excluded — a fix that inverts
    // the bug lets `.git` churn drive the rebuild loop it once drove.
    // Extension-less names read as directory names to the type filter, which
    // is why the dotfile rule is what excludes them and both are consulted.
    (".git/config", false, true),
    (".git/HEAD", false, true),
    ("node_modules/pkg/index.js", false, true),
    ("posts/.secret.md", false, false),
    (".DS_Store", false, false),
    // moss's own output: never an input, whatever it is named.
    (".moss/build.nosync/staging/index.html", false, false),
    (".moss/cache/objects/ab/cd1234", false, false),
    // Watchable, but nothing consumes the extension.
    ("report.docx", true, false),
];

#[test]
fn a_relocated_vault_gets_the_same_verdict_on_every_file() {
    for (rel, watchable, passes) in FILES {
        for (shape, mount) in VAULT_MOUNTS {
            let root = std::path::Path::new(mount);
            let path = mount_join(mount, rel);
            assert_eq!(
                path_is_watchable(root, &path),
                *watchable,
                "path_is_watchable: `{rel}` under {shape} ({mount})"
            );
            assert_eq!(
                path_passes_filter(root, &path),
                *passes,
                "path_passes_filter: `{rel}` under {shape} ({mount})"
            );
        }
    }
}

/// The `any_*` wrappers are what the rebuild pump actually calls, and the
/// pump is where the observed outage happened: one unwatchable path in a
/// batch must not condemn the batch, and a batch of nothing but excluded
/// paths must still be dropped.
#[test]
fn the_event_level_predicates_relocate_too() {
    for (shape, mount) in VAULT_MOUNTS {
        let root = std::path::Path::new(mount);
        let content = mount_join(mount, "notes/post.md");
        let unwatchable = mount_join(mount, ".git/HEAD");
        let unfiltered = mount_join(mount, "notes/scratch.xyz");
        assert!(
            any_path_watchable(root, &[unwatchable.clone(), content.clone()]),
            "a real edit riding alongside a `.git` write, under {shape}"
        );
        assert!(any_path_passes_filter(root, &[unfiltered.clone(), content]));
        assert!(!any_path_watchable(root, &[unwatchable]), "under {shape}");
        assert!(!any_path_passes_filter(root, &[unfiltered]), "under {shape}");
    }
}

/// A path that is not under the root at all cannot be answered relatively.
/// The fallback is the old whole-path judgement — no call site is worse off
/// than before the signature changed — and `rel_or_whole` says so in the log
/// once, because this is the one remaining way a folder could go quiet.
#[test]
fn a_path_outside_the_root_falls_back_to_the_whole_path() {
    let root = std::path::Path::new("/home/u/Sites/blog");
    assert!(path_is_watchable(root, std::path::Path::new("/elsewhere/post.md")));
    assert!(!path_is_watchable(root, std::path::Path::new("/elsewhere/.git/HEAD")));
    // A sibling that merely shares a textual prefix is NOT inside the vault.
    assert_eq!(vault_rel(root, std::path::Path::new("/home/u/Sites/blog-backup/x.md")), None);
    assert_eq!(
        vault_rel(root, std::path::Path::new("/home/u/Sites/blog/x.md")).as_deref(),
        Some("x.md")
    );
}

/// The `.moss/` allowlist keys on a `.moss` COMPONENT of the vault-relative
/// path. A substring gate (`contains("/.moss/")`) misses it, because a
/// vault-relative path has no leading slash — and the cost of missing it is
/// that the dotfile rule takes over and theme edits stop rebuilding.
#[test]
fn the_moss_allowlist_is_reached_from_the_vault_root() {
    let root = std::path::Path::new("/home/u/Sites/blog");
    assert!(path_is_watchable(root, &root.join(".moss/theme/style.css")));
    assert!(!path_is_watchable(root, &root.join(".moss/cache/thumb.jpg")));
    // And a vault kept under a directory of moss backups is not moss's own.
    let odd = std::path::Path::new("/home/u/.moss-backups/blog");
    assert!(path_is_watchable(odd, &odd.join("posts/hello.md")));
    assert!(path_passes_filter(odd, &odd.join("posts/hello.md")));
}
