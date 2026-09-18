use super::*;

/// Feature 4: File Watching - File Filtering
/// Tests which files should trigger recompilation
/// Note: Only markdown files trigger rebuilds - other files (images, docx, etc.)
/// are handled during build but don't need to trigger rebuilds themselves
#[test]
fn test_file_watching_should_watch_file() {
    // Markdown files that should trigger recompilation
    assert!(
        should_watch_file("content/about.md"),
        "Should watch markdown files"
    );
    assert!(
        should_watch_file("posts/article.markdown"),
        "Should watch .markdown files"
    );

    // Non-content files should NOT trigger recompilation
    assert!(
        !should_watch_file("posts/article.pages"),
        "Should not watch Pages files"
    );
    assert!(
        !should_watch_file("documents/report.docx"),
        "Should not watch Word documents"
    );

    // Asset files ARE now watched (images, CSS, JS, fonts, etc.)
    assert!(
        should_watch_file("images/photo.jpg"),
        "Should watch image files"
    );
    assert!(
        should_watch_file("assets/logo.png"),
        "Should watch PNG images"
    );
    assert!(
        should_watch_file("styles/custom.svg"),
        "Should watch SVG files"
    );

    // Source HTML is build input and must trigger a rebuild. Generated HTML is
    // ignored at the .moss path boundary, not by pretending HTML is never input.
    assert!(
        should_watch_file("index.html"),
        "Should watch source-authored HTML files"
    );
    assert!(
        !path_is_watchable(
            std::path::Path::new("/vault"),
            std::path::Path::new("/vault/.moss/build/current/index.html")
        ),
        "Should ignore generated HTML at the .moss boundary"
    );
    // Note: node_modules filtering happens at gitignore level, not in should_watch_file
    // The function only checks file extensions, not directory paths
    assert!(
        should_watch_file("node_modules/package/file.js"),
        "JS files are watched (gitignore handles node_modules filtering)"
    );
    // Dot-dir filtering lives in `path_is_watchable`, not here: judged by file
    // type alone, extension-less `config` reads as a directory name.
    assert!(
        !path_is_watchable(std::path::Path::new("/vault"), std::path::Path::new("/vault/.git/config")),
        "Should ignore git files"
    );
    assert!(
        !should_watch_file(".DS_Store"),
        "Should ignore system files"
    );
    assert!(
        !should_watch_file("thumbs.db"),
        "Should ignore Windows thumbnails"
    );

    // Files without recognized extensions
    // Note: Root-level paths without extensions are treated as directories (permissive for custom structures)
    assert!(
        should_watch_file("README"),
        "Root-level paths without extensions are treated as potential directories"
    );
    assert!(
        !should_watch_file("config.txt"),
        "Should ignore non-content files"
    );
}

/// Feature 4b: Directory Watching - Directory Filtering
/// Tests which directories should trigger recompilation
#[test]
fn test_file_watching_should_watch_directory() {
    // Content directories that should trigger recompilation
    assert!(
        should_watch_file("content/"),
        "Should watch content directories"
    );
    assert!(should_watch_file("posts/"), "Should watch posts directory");
    assert!(
        should_watch_file("journal/"),
        "Should watch journal directory"
    );
    assert!(should_watch_file("blog/"), "Should watch blog directory");
    assert!(
        should_watch_file("articles/"),
        "Should watch articles directory"
    );
    assert!(should_watch_file("docs/"), "Should watch docs directory");
    assert!(should_watch_file("pages/"), "Should watch pages directory");
    assert!(
        should_watch_file("images/"),
        "Should watch images directory"
    );
    assert!(
        should_watch_file("assets/"),
        "Should watch assets directory"
    );
    assert!(should_watch_file("media/"), "Should watch media directory");

    // Root-level directories (permissive approach)
    assert!(
        should_watch_file("mycontent"),
        "Should watch root-level directories"
    );
    assert!(
        should_watch_file("writings"),
        "Should watch root-level directories"
    );

    // Nested content directories
    assert!(
        should_watch_file("/Users/user/site/posts/"),
        "Should watch nested posts directory"
    );
    assert!(
        should_watch_file("/Users/user/site/content/blog/"),
        "Should watch nested content directories"
    );

    // System directories - these are now filtered by gitignore matcher, not should_watch_file()
    // should_watch_file() returns true for directories, and gitignore matcher handles exclusions
    assert!(
        should_watch_file(".moss/"),
        "should_watch_file returns true - gitignore filters it"
    );
    assert!(
        should_watch_file("node_modules/"),
        "should_watch_file returns true - gitignore filters it"
    );
    assert!(
        should_watch_file(".git/"),
        "should_watch_file returns true - gitignore filters it"
    );
    assert!(
        should_watch_file(".moss/build/current/"),
        "should_watch_file returns true - gitignore filters it"
    );

    // Deeply nested directories - should_watch_file returns true, gitignore/depth filtering happens elsewhere
    assert!(
        should_watch_file("deeply/nested/path/unknown/"),
        "should_watch_file returns true for directories"
    );
}

/// Feature 4d: Asset File Watching
/// Tests that asset files (images, CSS, JS, etc.) trigger recompilation
#[test]
fn test_should_watch_asset_files() {
    // Image files
    assert!(
        should_watch_file("images/photo.jpg"),
        "Should watch JPEG images"
    );
    assert!(
        should_watch_file("images/photo.jpeg"),
        "Should watch JPEG images"
    );
    assert!(
        should_watch_file("assets/logo.png"),
        "Should watch PNG images"
    );
    assert!(
        should_watch_file("images/icon.gif"),
        "Should watch GIF images"
    );
    assert!(
        should_watch_file("images/graphic.svg"),
        "Should watch SVG images"
    );
    assert!(
        should_watch_file("images/photo.webp"),
        "Should watch WebP images"
    );
    assert!(
        should_watch_file("images/icon.ico"),
        "Should watch ICO files"
    );

    // CSS files
    assert!(should_watch_file("css/style.css"), "Should watch CSS files");

    // JavaScript files
    assert!(
        should_watch_file("js/script.js"),
        "Should watch JavaScript files"
    );

    // Font files
    assert!(
        should_watch_file("fonts/roboto.woff"),
        "Should watch WOFF fonts"
    );
    assert!(
        should_watch_file("fonts/roboto.woff2"),
        "Should watch WOFF2 fonts"
    );
    assert!(
        should_watch_file("fonts/roboto.ttf"),
        "Should watch TTF fonts"
    );
    assert!(
        should_watch_file("fonts/roboto.otf"),
        "Should watch OTF fonts"
    );
    assert!(
        should_watch_file("fonts/roboto.eot"),
        "Should watch EOT fonts"
    );

    // Video and audio files
    assert!(
        should_watch_file("media/video.mp4"),
        "Should watch MP4 videos"
    );
    assert!(
        should_watch_file("media/video.webm"),
        "Should watch WebM videos"
    );
    assert!(
        should_watch_file("media/audio.mp3"),
        "Should watch MP3 audio"
    );
    assert!(
        should_watch_file("media/audio.ogg"),
        "Should watch OGG audio"
    );
    assert!(
        should_watch_file("media/audio.wav"),
        "Should watch WAV audio"
    );

    // PDF files
    assert!(
        should_watch_file("documents/paper.pdf"),
        "Should watch PDF files"
    );

    // Case insensitivity
    assert!(
        should_watch_file("images/PHOTO.JPG"),
        "Should watch files with uppercase extensions"
    );
    assert!(
        should_watch_file("images/Photo.PNG"),
        "Should watch files with mixed-case extensions"
    );
}

/// Feature 4c: Directory Rename Scenarios
/// Tests rename events with directories
#[test]
fn test_directory_rename_scenarios() {
    // Test that directory paths without extensions are detected as directories
    let test_cases = vec![
        ("journal", true, "Root directory without slash"),
        ("posts/", true, "Directory with trailing slash"),
        ("content/subfolder", true, "Nested directory path"),
        ("/Users/user/site/blog", true, "Absolute directory path"),
        ("file.md", false, "File with extension"),
        (
            "README",
            true,
            "File without extension - treated as directory",
        ),
    ];

    for (path, expected_is_dir, description) in test_cases {
        let is_directory = path.ends_with('/') || !path.contains('.');
        assert_eq!(
            is_directory, expected_is_dir,
            "Failed for case: {}",
            description
        );
    }
}

/// Feature 7: File Event Processing Logic
/// Tests that different event types are processed correctly
#[test]
fn test_file_event_classification() {
    use notify::event::{CreateKind, ModifyKind, RemoveKind};
    use notify::EventKind;

    // Test that Modify events are classified correctly
    let modify_event = EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any));
    match modify_event {
        EventKind::Modify(ModifyKind::Name(_)) => panic!("Should not be a name modify"),
        EventKind::Modify(_) => {} // Expected path
        _ => panic!("Should be a modify event"),
    }

    // Test that Create events are classified correctly
    let create_event = EventKind::Create(CreateKind::File);
    match create_event {
        EventKind::Create(_) => {} // Expected
        _ => panic!("Should be a create event"),
    }

    // Test that Remove events are classified correctly
    let remove_event = EventKind::Remove(RemoveKind::File);
    match remove_event {
        EventKind::Remove(_) => {} // Expected
        _ => panic!("Should be a remove event"),
    }

    // Test that Rename events (ModifyKind::Name) are classified correctly
    let rename_event = EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both));
    match rename_event {
        EventKind::Modify(ModifyKind::Name(_)) => {} // Expected path for renames
        _ => panic!("Should be a name modify event"),
    }
}

/// Helper: evaluate the production classifier with a single file path so
/// existing tests stay focused on event-kind filtering.
fn should_recompile(kind: notify::EventKind) -> bool {
    let paths = vec![PathBuf::from("/some/file.md")];
    // is_dir returns false → file path → exercises the file-side branches.
    super::should_recompile_for_event_with(kind, &paths, |_| false)
}

/// WriteTime and Any metadata events trigger rebuilds (atomic saves on iCloud).
/// Other metadata events (permissions, ownership, etc.) are still filtered.
#[test]
fn test_metadata_events_filtering() {
    use notify::event::{MetadataKind, ModifyKind};
    use notify::EventKind;

    // Metadata(Any) — may indicate content change via atomic save
    assert!(
        should_recompile(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any))),
        "Modify(Metadata(Any)) should trigger a rebuild"
    );

    // Metadata(WriteTime) — common signal for atomic saves (write temp → rename)
    assert!(
        should_recompile(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::WriteTime
        ))),
        "Modify(Metadata(WriteTime)) should trigger a rebuild"
    );

    // Metadata(AccessTime) — noise, not a content change
    assert!(
        !should_recompile(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::AccessTime
        ))),
        "Modify(Metadata(AccessTime)) should not trigger a rebuild"
    );

    // Metadata(Permissions)
    assert!(
        !should_recompile(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Permissions
        ))),
        "Modify(Metadata(Permissions)) should not trigger a rebuild"
    );

    // Metadata(Ownership)
    assert!(
        !should_recompile(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Ownership
        ))),
        "Modify(Metadata(Ownership)) should not trigger a rebuild"
    );

    // Metadata(Extended) — iCloud xattr sync noise
    assert!(
        !should_recompile(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Extended
        ))),
        "Modify(Metadata(Extended)) should not trigger a rebuild"
    );

    // Metadata(Other) — catch-all for platform-specific metadata
    assert!(
        !should_recompile(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Other))),
        "Modify(Metadata(Other)) should not trigger a rebuild"
    );
}

/// `Metadata(Any)` on a directory is the iCloud materialization signal.
/// Suppress it so the rebuild-loop doesn't self-sustain. Folder structural
/// changes arrive via `Create`/`Remove`/`Modify(Name)`, not Metadata.
#[test]
fn test_metadata_any_on_directory_is_suppressed() {
    use notify::event::{MetadataKind, ModifyKind};
    use notify::EventKind;

    let kind = EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any));
    let dir_path = vec![PathBuf::from("/some/Yi-website")];

    assert!(
        !super::should_recompile_for_event_with(kind, &dir_path, |_| true),
        "Metadata(Any) on a directory should not trigger a rebuild (iCloud noise)"
    );
}

/// `Metadata(Any)` on a regular file still triggers a rebuild — that's the
/// Obsidian atomic-save case the catch-all branch was added to handle.
#[test]
fn test_metadata_any_on_file_still_triggers() {
    use notify::event::{MetadataKind, ModifyKind};
    use notify::EventKind;

    let kind = EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any));
    let file_path = vec![PathBuf::from("/some/note.md")];

    assert!(
        super::should_recompile_for_event_with(kind, &file_path, |_| false),
        "Metadata(Any) on a file should trigger a rebuild (atomic-save signal)"
    );
}

/// Mixed batch (one directory, one file) keeps the file's signal — at
/// least one path is a real content change candidate.
#[test]
fn test_metadata_any_mixed_batch_triggers() {
    use notify::event::{MetadataKind, ModifyKind};
    use notify::EventKind;

    let kind = EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any));
    let paths = vec![
        PathBuf::from("/some/Yi-website"),
        PathBuf::from("/some/note.md"),
    ];

    assert!(
        super::should_recompile_for_event_with(kind, &paths, |p| {
            p.to_string_lossy().ends_with("Yi-website")
        }),
        "Metadata(Any) on mixed dir+file batch should trigger (file is real signal)"
    );
}

/// Empty path lists fall through as accept — preserves the prior behavior
/// before the directory check was added.
#[test]
fn test_metadata_any_empty_paths_triggers() {
    use notify::event::{MetadataKind, ModifyKind};
    use notify::EventKind;

    let kind = EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any));
    let paths: Vec<PathBuf> = vec![];

    assert!(
        super::should_recompile_for_event_with(kind, &paths, |_| true),
        "Metadata(Any) with no paths should trigger (no signal to suppress on)"
    );
}

/// Directory check should NOT bypass create/remove/rename — those go
/// through different match arms and must keep firing for folders.
#[test]
fn test_directory_create_remove_rename_still_trigger() {
    use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
    use notify::EventKind;

    let dir_path = vec![PathBuf::from("/some/new-folder")];

    assert!(
        super::should_recompile_for_event_with(
            EventKind::Create(CreateKind::Folder),
            &dir_path,
            |_| true
        ),
        "Folder Create must still trigger"
    );
    assert!(
        super::should_recompile_for_event_with(
            EventKind::Remove(RemoveKind::Folder),
            &dir_path,
            |_| true
        ),
        "Folder Remove must still trigger"
    );
    assert!(
        super::should_recompile_for_event_with(
            EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
            &dir_path,
            |_| true
        ),
        "Folder Rename must still trigger"
    );
}

/// Data, Name, and Any modify events MUST still trigger rebuilds.
#[test]
fn test_data_name_any_modify_events_trigger_rebuild() {
    use notify::event::{DataChange, ModifyKind, RenameMode};
    use notify::EventKind;

    // Modify(Data(Any)) — actual file content change
    assert!(
        should_recompile(EventKind::Modify(ModifyKind::Data(DataChange::Any))),
        "Modify(Data(Any)) should trigger a rebuild"
    );

    // Modify(Data(Content))
    assert!(
        should_recompile(EventKind::Modify(ModifyKind::Data(DataChange::Content))),
        "Modify(Data(Content)) should trigger a rebuild"
    );

    // Modify(Name(Any)) — file rename
    assert!(
        should_recompile(EventKind::Modify(ModifyKind::Name(RenameMode::Any))),
        "Modify(Name(Any)) should trigger a rebuild"
    );

    // Modify(Name(Both)) — rename with both source and target
    assert!(
        should_recompile(EventKind::Modify(ModifyKind::Name(RenameMode::Both))),
        "Modify(Name(Both)) should trigger a rebuild"
    );

    // Modify(Any) — generic modify event
    assert!(
        should_recompile(EventKind::Modify(ModifyKind::Any)),
        "Modify(Any) should trigger a rebuild"
    );
}

/// Create and Remove events MUST still trigger rebuilds.
#[test]
fn test_create_and_remove_events_trigger_rebuild() {
    use notify::event::{CreateKind, RemoveKind};
    use notify::EventKind;

    // Create(File)
    assert!(
        should_recompile(EventKind::Create(CreateKind::File)),
        "Create(File) should trigger a rebuild"
    );

    // Create(Folder)
    assert!(
        should_recompile(EventKind::Create(CreateKind::Folder)),
        "Create(Folder) should trigger a rebuild"
    );

    // Create(Any)
    assert!(
        should_recompile(EventKind::Create(CreateKind::Any)),
        "Create(Any) should trigger a rebuild"
    );

    // Remove(File)
    assert!(
        should_recompile(EventKind::Remove(RemoveKind::File)),
        "Remove(File) should trigger a rebuild"
    );

    // Remove(Folder)
    assert!(
        should_recompile(EventKind::Remove(RemoveKind::Folder)),
        "Remove(Folder) should trigger a rebuild"
    );

    // Remove(Any)
    assert!(
        should_recompile(EventKind::Remove(RemoveKind::Any)),
        "Remove(Any) should trigger a rebuild"
    );

    // Access events should NOT trigger rebuilds
    assert!(
        !should_recompile(EventKind::Access(notify::event::AccessKind::Any)),
        "Access events should not trigger a rebuild"
    );

    // Other events should NOT trigger rebuilds
    assert!(
        !should_recompile(EventKind::Other),
        "Other events should not trigger a rebuild"
    );
}

/// `EventKind::Other` is dropped by `should_recompile_for_event` — which is
/// right, except when it carries the rescan flag. FSEvents sets that flag to
/// say "my buffer overflowed, I dropped events, go look for yourself," and a
/// mass cloud materialization is precisely the burst that overflows it. If
/// that flag were filtered along with the kind, every change in the dropped
/// window would be lost until the user happened to touch another file.
#[test]
fn a_rescan_flag_survives_the_filter_that_drops_its_event_kind() {
    use notify::event::Flag;

    let plain = DebouncedEvent::new(notify::Event::new(EventKind::Other), std::time::Instant::now());
    assert!(
        !watcher_lost_events(std::slice::from_ref(&plain)),
        "an ordinary event must not claim the watcher lost anything"
    );
    assert!(
        !should_recompile_for_event(EventKind::Other, &[]),
        "precondition: the kind alone is filtered out, so the flag is the only signal left"
    );

    let dropped = DebouncedEvent::new(
        notify::Event::new(EventKind::Other).set_flag(Flag::Rescan),
        std::time::Instant::now(),
    );
    assert!(
        watcher_lost_events(&[plain, dropped]),
        "one rescan-flagged event anywhere in the batch means the whole batch is untrustworthy"
    );
}

// ---------------------------------------------------------------
// compute_rebuild_event tests
// ---------------------------------------------------------------

/// When a folder is renamed, the old output paths disappear (deleted)
/// and new output paths appear (new). The rebuild event must NOT be
/// suppressed and must include deleted_paths so the frontend can
/// redirect the preview away from the now-missing page.
#[test]
fn test_folder_rename_produces_event_with_deleted_paths() {
    let mut previous = SiteHashes::new();
    previous.insert("old-folder/post/index.html".into(), "hash_a".into());
    previous.insert("index.html".into(), "hash_b".into());

    let mut current = SiteHashes::new();
    current.insert("new-folder/post/index.html".into(), "hash_a".into());
    current.insert("index.html".into(), "hash_b".into());

    let result = compute_rebuild_event(&current, &previous);

    // Must not be suppressed
    assert!(
        result.is_some(),
        "Folder rename must not suppress the event"
    );
    let event = result.unwrap();

    // Must contain the deleted output path
    let deleted = event.deleted_paths.expect("deleted_paths must be set");
    assert!(
        deleted.contains(&"old-folder/post/index.html".to_string()),
        "deleted_paths must include the old folder page"
    );
}

/// When only file content changes (no structural changes), the event
/// should contain changed_output_files but no deleted_paths.
#[test]
fn test_content_change_only_produces_changed_output_files() {
    let mut previous = SiteHashes::new();
    previous.insert("blog/post/index.html".into(), "hash_old".into());

    let mut current = SiteHashes::new();
    current.insert("blog/post/index.html".into(), "hash_new".into());

    let result = compute_rebuild_event(&current, &previous);

    assert!(result.is_some(), "Content change must produce an event");
    let event = result.unwrap();

    let changed = event
        .changed_output_files
        .expect("changed_output_files must be set");
    assert!(changed.contains(&"blog/post/index.html".to_string()));

    // No structural changes
    assert!(event.deleted_paths.is_none() || event.deleted_paths.as_ref().unwrap().is_empty());
}

/// When nothing changes (same hashes, no plugins), the event should
/// be suppressed (None).
#[test]
fn test_no_changes_suppresses_event() {
    let mut previous = SiteHashes::new();
    previous.insert("index.html".into(), "hash_a".into());

    let mut current = SiteHashes::new();
    current.insert("index.html".into(), "hash_a".into());

    let result = compute_rebuild_event(&current, &previous);

    assert!(result.is_none(), "No changes should suppress the event");
}

/// A search re-index alone must NOT refresh the preview.
///
/// Pagefind shards are content-addressed, so re-indexing renames every file it
/// touches: 85 entries between two consecutive harbor generations, all
/// create+delete, none of them a change to anything the open page renders.
/// Before the exclusion this flipped `has_changes` on its own, and the refresh
/// it forced is the one that flashes when the morph declines it.
#[test]
fn a_search_reindex_alone_suppresses_the_refresh() {
    let mut previous = SiteHashes::new();
    previous.insert("index.html".into(), "hash_a".into());
    previous.insert("_moss/pagefind/index/zh-hant_121c28e.pf_index".into(), "s1".into());
    previous.insert("_moss/pagefind/pagefind-entry.json".into(), "e1".into());

    let mut current = SiteHashes::new();
    current.insert("index.html".into(), "hash_a".into());
    // Same shard under a new content-addressed name, plus a rewritten entry.
    current.insert("_moss/pagefind/index/zh-hant_122bea1.pf_index".into(), "s2".into());
    current.insert("_moss/pagefind/pagefind-entry.json".into(), "e2".into());

    assert!(
        compute_rebuild_event(&current, &previous).is_none(),
        "shard churn is not a page change"
    );
}

/// The exclusion is scoped to the search mount, and does not swallow the page
/// change a build that ALSO re-indexed made.
#[test]
fn a_real_page_change_still_refreshes_across_a_reindex() {
    let mut previous = SiteHashes::new();
    previous.insert("blog/post/index.html".into(), "hash_old".into());
    previous.insert("_moss/pagefind/index/zh-hant_121c28e.pf_index".into(), "s1".into());

    let mut current = SiteHashes::new();
    current.insert("blog/post/index.html".into(), "hash_new".into());
    current.insert("_moss/pagefind/index/zh-hant_122bea1.pf_index".into(), "s2".into());

    let event = compute_rebuild_event(&current, &previous).expect("page change refreshes");
    let changed = event.changed_output_files.expect("changed_output_files must be set");
    assert_eq!(
        changed,
        vec!["blog/post/index.html".to_string()],
        "the payload names the page, not the shards"
    );
    assert!(
        event.deleted_paths.is_none(),
        "a retired shard is not a deleted page — deleted_paths drives preview redirect"
    );
}

/// A sibling of the mount is not the mount. `_moss/pagefinder/` and a page
/// literally named `_moss/pagefind` would both be swallowed by a bare
/// `starts_with`, which is why the predicate requires the separator.
#[test]
fn the_search_exclusion_stops_at_a_path_boundary() {
    use crate::build::served_path::ServedPath;
    assert!(ServedPath::is_search_asset("_moss/pagefind/pagefind.js"));
    assert!(!ServedPath::is_search_asset("_moss/pagefinder/x.js"));
    assert!(!ServedPath::is_search_asset("_moss/pagefind"));
    assert!(!ServedPath::is_search_asset("blog/_moss/pagefind/x"));
}

/// When a new page is added (exists in current but not previous),
/// the event should not be suppressed — the homepage listing may
/// have changed.
#[test]
fn test_new_page_produces_event() {
    let mut previous = SiteHashes::new();
    previous.insert("index.html".into(), "hash_a".into());

    let mut current = SiteHashes::new();
    current.insert("index.html".into(), "hash_a_updated".into());
    current.insert("new-post/index.html".into(), "hash_b".into());

    let result = compute_rebuild_event(&current, &previous);

    assert!(result.is_some(), "New page must not suppress the event");
}

/// .moss/ allowlist: only user-editable files trigger rebuilds.
#[test]
fn test_should_watch_moss_file() {
    // Allowed files
    assert!(should_watch_moss_file("config.toml"));
    assert!(should_watch_moss_file("theme/style.css"));
    assert!(should_watch_moss_file("theme/script.js"));
    assert!(should_watch_moss_file("assets/logo.png"));
    assert!(should_watch_moss_file("assets\\logo.png")); // Windows

    // Legacy root paths are NO LONGER accepted (migration removed)
    assert!(!should_watch_moss_file("style.css"));
    assert!(!should_watch_moss_file("script.js"));

    // Auto-managed paths must NOT trigger rebuilds
    assert!(!should_watch_moss_file("site/index.html"));
    assert!(!should_watch_moss_file("cache/videos/thumb.jpg"));
    assert!(!should_watch_moss_file("manifest.json"));

    // data/ is moss-written state (drafts auto-save every ~2s while composing;
    // deploy rewrites deployed-article-map.json). Only data/social/ is watched.
    assert!(!should_watch_moss_file("data/email/drafts/posts/hello.md"));
    assert!(!should_watch_moss_file("data/email/drafts/hello.md"));
    assert!(!should_watch_moss_file(
        "data\\email\\drafts\\posts\\hello.md"
    )); // Windows
    assert!(!should_watch_moss_file("data/email/something-else.json"));
    assert!(!should_watch_moss_file("data/subscribers.json"));
}

#[test]
fn watcher_covers_social_data_so_comment_sync_triggers_refresh() {
    // Auto-refresh contract (design §4): background sync writes
    // .moss/data/social/comment.json only when content changed; the watcher
    // must pick that up. The only-on-change guard in process_comments is
    // what prevents a rebuild loop — do not loosen either side.
    assert!(should_watch_moss_file("data/social/comment.json"));
    assert!(should_watch_moss_file("data/social/matters.json"));
}

/// Regression (B3): a deploy writes the deployed-article-map snapshot, which
/// used to wake moss's OWN watcher — every publish logged
/// `Rebuild triggered by: ["deployed-article-map.json"]` and burned a rebuild.
/// Every real inhabitant of `.moss/data/` except `social/` is moss-written
/// state, so the rule is an explicit allowlist, not allow-all-except-drafts.
#[test]
fn moss_written_data_state_must_not_trigger_rebuild() {
    // Where the snapshot lives now — the write that caused B3.
    assert!(!should_watch_moss_file("deploy/deployed-article-map.json"));
    // …and where an un-migrated project's copy still sits.
    assert!(!should_watch_moss_file("data/deployed-article-map.json"));
    assert!(!should_watch_moss_file("data/events.jsonl"));
    assert!(!should_watch_moss_file("data/events-cursor.json"));
    assert!(!should_watch_moss_file("data/redirects.json"));
    assert!(!should_watch_moss_file("data/email/subscribers.csv"));
    // iCloud conflict copies pile up in the same dir (~8MB observed).
    assert!(!should_watch_moss_file("data/deployed-article-map 2.json"));
    assert!(!should_watch_moss_file("data\\deployed-article-map.json")); // Windows
}

/// Regression: .moss/config.toml must trigger rebuild.
///
/// Before this fix, config.toml passed the path-level filter
/// (should_watch_moss_file) but was silently dropped by the
/// extension-level filter (should_watch_file) because .toml
/// wasn't a recognized extension. Service toggles (email
/// subscriptions, analytics, etc.) wrote to config.toml but
/// never triggered a recompile.
#[test]
fn test_moss_config_toml_triggers_rebuild() {
    // should_watch_file rejects .toml (it's not a content/asset extension)
    assert!(
        !should_watch_file("/Users/user/site/.moss/config.toml"),
        "should_watch_file alone rejects .toml"
    );

    // But the .moss/ allowlist accepts it
    assert!(
        should_watch_moss_file("config.toml"),
        "should_watch_moss_file allows config.toml"
    );

    // The combined check (as used in handle_file_event) must accept it.
    // This mirrors the logic in handle_file_event after the fix.
    let path = "/Users/user/site/.moss/config.toml";
    let accepted = if path.contains("/.moss/") {
        let after_moss = path.split("/.moss/").last().unwrap_or("");
        should_watch_moss_file(after_moss)
    } else {
        should_watch_file(path)
    };
    assert!(
        accepted,
        ".moss/config.toml must trigger rebuild (service toggles)"
    );
}

// -----------------------------------------------------------------------
// Content-hash gate tests (source_metadata_matches, path_to_relative_key,
// should_rebuild_for_paths, evaluate_gate). See plan
// docs/archive/2026-04-23-watcher-content-hash-gate.md.
// -----------------------------------------------------------------------

use crate::build::types::SourceMetadata;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

fn gate_write_file(dir: &std::path::Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    let mut f = fs::File::create(&p).unwrap();
    f.write_all(bytes).unwrap();
    p
}

fn gate_mtime_secs(p: &std::path::Path) -> u64 {
    fs::metadata(p)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn gate_mtime_nanos(p: &std::path::Path) -> u32 {
    fs::metadata(p)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos()
}

fn gate_sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

#[test]
fn source_matches_fast_path_when_size_and_mtime_equal() {
    let dir = tempfile::tempdir().unwrap();
    let p = gate_write_file(dir.path(), "a.md", b"hello");
    let meta = SourceMetadata {
        // Deliberately NOT the file's hash: with size + full-precision mtime
        // matching, the fast path must return Unchanged without reading the
        // bytes at all — a hash comparison here would say Changed.
        hash: "fast-path-must-not-hash".into(),
        size: 5,
        mtime: gate_mtime_secs(&p),
        mtime_nanos: Some(gate_mtime_nanos(&p)),
        ctime: None,
        inode: None,
    };
    assert_eq!(source_metadata_matches(&meta, &p), SourceCheck::Unchanged);
}

#[test]
fn source_unchanged_when_size_matches_but_mtime_differs_and_content_identical() {
    let dir = tempfile::tempdir().unwrap();
    let p = gate_write_file(dir.path(), "a.md", b"hello");
    let wrong_mtime = gate_mtime_secs(&p).wrapping_add(1);
    let meta = SourceMetadata {
        hash: gate_sha256_hex(b"hello"),
        size: 5, // matches
        mtime: wrong_mtime,
        mtime_nanos: None,
        ctime: None,
        inode: None,
    };
    assert_eq!(source_metadata_matches(&meta, &p), SourceCheck::Unchanged);
}

#[test]
fn source_changed_when_size_differs_short_circuits() {
    let dir = tempfile::tempdir().unwrap();
    let p = gate_write_file(dir.path(), "a.md", b"hello world");
    let meta = SourceMetadata {
        hash: gate_sha256_hex(b"hello"),
        size: 5, // wrong
        mtime: gate_mtime_secs(&p),
        mtime_nanos: None,
        ctime: None,
        inode: None,
    };
    assert_eq!(source_metadata_matches(&meta, &p), SourceCheck::Changed);
}

#[test]
fn source_changed_when_content_differs_despite_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let p = gate_write_file(dir.path(), "a.md", b"world");
    let wrong_mtime = gate_mtime_secs(&p).wrapping_add(1);
    let meta = SourceMetadata {
        hash: gate_sha256_hex(b"hello"),
        size: 5,
        mtime: wrong_mtime,
        mtime_nanos: None,
        ctime: None,
        inode: None,
    };
    assert_eq!(source_metadata_matches(&meta, &p), SourceCheck::Changed);
}

#[test]
fn source_changed_when_same_size_write_lands_in_recorded_mtime_second() {
    // The whole-second fast path cannot distinguish "the file we hashed" from
    // "a same-size rewrite in the same second" (e.g. undo restoring different
    // bytes of identical length). A manifest that carries only second
    // precision must NOT suppress on (size, secs) alone — it must hash.
    let dir = tempfile::tempdir().unwrap();
    let p = gate_write_file(dir.path(), "a.md", b"world");
    let meta = SourceMetadata {
        hash: gate_sha256_hex(b"hello"), // recorded bytes differ, same size
        size: 5,
        mtime: gate_mtime_secs(&p), // same whole second
        mtime_nanos: None,
        ctime: None,
        inode: None,
    };
    assert_eq!(source_metadata_matches(&meta, &p), SourceCheck::Changed);
}

#[test]
fn source_unknown_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("nope.md");
    let meta = SourceMetadata {
        hash: "x".into(),
        size: 0,
        mtime: 0,
        mtime_nanos: None,
        ctime: None,
        inode: None,
    };
    assert_eq!(source_metadata_matches(&meta, &p), SourceCheck::Unknown);
}

/// The forged-mtime hole (rsync -t, touch -r): userland can preserve size,
/// mtime and even nanoseconds, but not ctime. A ctime disagreement must
/// demote the fast path to the hash tier — where the changed bytes are seen.
#[test]
#[cfg(unix)] // stat_identity is (None, None) off unix, so no demotion can fire
fn a_ctime_disagreement_demotes_the_fast_path_to_the_hash_tier() {
    let dir = tempfile::tempdir().unwrap();
    let p = gate_write_file(dir.path(), "a.md", b"world");
    let meta = SourceMetadata {
        hash: gate_sha256_hex(b"hello"), // recorded bytes differ, same size
        size: 5,
        mtime: gate_mtime_secs(&p),
        mtime_nanos: Some(gate_mtime_nanos(&p)),
        ctime: Some(i64::MIN), // recorded ctime cannot match the file's
        inode: None,
    };
    assert_eq!(
        source_metadata_matches(&meta, &p),
        SourceCheck::Changed,
        "size+mtime agree but ctime does not — must hash, and the hash differs"
    );
}

/// Replace-via-rename (atomic save) changes the inode even when size and
/// mtime are preserved; same demotion rule as ctime.
#[test]
#[cfg(unix)] // stat_identity is (None, None) off unix, so no demotion can fire
fn an_inode_disagreement_demotes_the_fast_path_to_the_hash_tier() {
    let dir = tempfile::tempdir().unwrap();
    let p = gate_write_file(dir.path(), "a.md", b"world");
    let meta = SourceMetadata {
        hash: gate_sha256_hex(b"hello"),
        size: 5,
        mtime: gate_mtime_secs(&p),
        mtime_nanos: Some(gate_mtime_nanos(&p)),
        ctime: None,
        inode: Some(u64::MAX), // a different inode than the file's
    };
    assert_eq!(source_metadata_matches(&meta, &p), SourceCheck::Changed);
}

/// An AGREEING identity must not cost the fast path: with the file's real
/// ctime and inode recorded, size+mtime agreement still suppresses without
/// reading a byte (the recorded hash is deliberately wrong to prove it).
#[test]
fn an_agreeing_identity_keeps_the_fast_path() {
    let dir = tempfile::tempdir().unwrap();
    let p = gate_write_file(dir.path(), "a.md", b"hello");
    let (ctime, inode) = crate::build::types::stat_identity(&std::fs::metadata(&p).unwrap());
    let meta = SourceMetadata {
        hash: "fast-path-must-not-hash".into(),
        size: 5,
        mtime: gate_mtime_secs(&p),
        mtime_nanos: Some(gate_mtime_nanos(&p)),
        ctime,
        inode,
    };
    assert_eq!(source_metadata_matches(&meta, &p), SourceCheck::Unchanged);
}

/// git's racily-clean rule: an entry whose mtime falls within one
/// timestamp-granularity epsilon of the manifest's capture time could have
/// been rewritten after hashing without moving its mtime. Suspect entries
/// lose the fast path (and here the hash tier sees the changed bytes); a
/// comfortably-older entry keeps it.
#[test]
fn a_racy_mtime_is_hashed_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let p = gate_write_file(dir.path(), "a.md", b"world");
    let meta = SourceMetadata {
        hash: gate_sha256_hex(b"hello"), // same size, different bytes
        size: 5,
        mtime: gate_mtime_secs(&p),
        mtime_nanos: Some(gate_mtime_nanos(&p)),
        ctime: None,
        inode: None,
    };
    // Captured in the same instant the file was written: suspect → hash tier.
    let racy_capture = Some(gate_mtime_secs(&p));
    assert_eq!(
        source_metadata_matches_at(&meta, &p, racy_capture),
        SourceCheck::Changed
    );
    // Captured long after the write: the fast path stands (wrong hash never read).
    let settled_capture = Some(gate_mtime_secs(&p) + RACY_WRITE_EPSILON_SECS + 10);
    assert_eq!(
        source_metadata_matches_at(&meta, &p, settled_capture),
        SourceCheck::Unchanged
    );
}

/// The racy predicate's boundaries, pinned: no capture clock means nothing
/// is suspect (old manifests keep their fast path), and the epsilon is
/// inclusive on the boundary.
#[test]
fn mtime_is_racy_boundaries() {
    let m = |mtime: u64| SourceMetadata { mtime, ..Default::default() };
    assert!(!mtime_is_racy(&m(1000), None), "no clock, nothing suspect");
    assert!(mtime_is_racy(&m(1000), Some(1000)), "same instant is racy");
    assert!(
        mtime_is_racy(&m(1000), Some(1000 + RACY_WRITE_EPSILON_SECS)),
        "the boundary is inclusive"
    );
    assert!(!mtime_is_racy(&m(1000), Some(1000 + RACY_WRITE_EPSILON_SECS + 1)));
    assert!(
        mtime_is_racy(&m(1000 + RACY_WRITE_EPSILON_SECS), Some(1000)),
        "a write just after capture is inside the window"
    );
    assert!(
        !mtime_is_racy(&m(2000), Some(1000)),
        "a FUTURE-dated mtime far past the window is not racy: a fast-clock \
         device's sync would otherwise be re-hashed every pass forever"
    );
}

/// The absorb-once rule for provider re-materialization: an evict + identical
/// re-download rewrites ctime/inode, so the fast path is demoted and the hash
/// tier runs — ONCE. The verdict hands back the fresh stat record; with it
/// written into the baseline, the next look fast-paths again.
#[test]
fn a_hash_confirmed_match_hands_back_the_fresh_identity() {
    let dir = tempfile::tempdir().unwrap();
    let p = gate_write_file(dir.path(), "a.md", b"same bytes");
    let md = fs::metadata(&p).unwrap();
    let (fs_ctime, fs_inode) = crate::build::types::stat_identity(&md);

    // Baseline: right hash and size, but a stat identity from a previous life.
    let meta = SourceMetadata {
        hash: gate_sha256_hex(b"same bytes"),
        size: md.len(),
        mtime: 1_000, // long ago — mtime tier cannot match
        mtime_nanos: Some(0),
        ctime: fs_ctime.map(|c| c - 999),
        inode: fs_inode.map(|i| i + 1),
    };

    match source_metadata_verdict(&meta, &md, &p, None) {
        SourceVerdict::Unchanged { refreshed: Some(fresh) } => {
            assert_eq!(fresh.hash, meta.hash, "bytes did not change; hash carries over");
            assert_eq!(fresh.ctime, fs_ctime, "identity is the CURRENT one");
            assert_eq!(fresh.inode, fs_inode);
            // And the refreshed record fast-paths: same verdict, no refresh owed.
            assert_eq!(
                source_metadata_verdict(&fresh, &md, &p, None),
                SourceVerdict::Unchanged { refreshed: None },
                "the hash cost was absorbed by the write-back"
            );
        }
        other => panic!("expected hash-confirmed match with refresh, got {other:?}"),
    }
}

#[test]
fn path_to_relative_key_strips_folder_and_normalizes_slashes() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("posts");
    fs::create_dir_all(&sub).unwrap();
    let p = gate_write_file(&sub, "a.md", b"x");
    let rel = path_to_relative_key(dir.path(), &p).expect("some");
    assert_eq!(rel, "posts/a.md");
}

#[test]
fn path_to_relative_key_returns_none_for_path_outside_folder() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let p = gate_write_file(other.path(), "x.md", b"x");
    assert_eq!(path_to_relative_key(dir.path(), &p), None);
}

fn gate_write_manifest(folder: &std::path::Path, hashes: &crate::types::content::SiteHashes) {
    // load_previous_hashes reads from MossPaths::hashes(folder), which is
    // .moss/build/hashes.json. Mirror that layout in tests.
    let build = folder.join(".moss").join("build");
    fs::create_dir_all(&build).unwrap();
    fs::write(
        build.join("hashes.json"),
        serde_json::to_string(hashes).unwrap(),
    )
    .unwrap();
}

#[test]
fn gate_suppresses_when_every_path_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let p_a = gate_write_file(folder, "a.md", b"hello");
    let p_b = gate_write_file(folder, "b.md", b"world");

    let mut hashes = crate::types::content::SiteHashes::new();
    hashes.sources.insert(
        "a.md".into(),
        SourceMetadata {
            hash: gate_sha256_hex(b"hello"),
            size: 5,
            mtime: gate_mtime_secs(&p_a),
            mtime_nanos: None,
            ctime: None,
            inode: None,
        },
    );
    hashes.sources.insert(
        "b.md".into(),
        SourceMetadata {
            hash: gate_sha256_hex(b"world"),
            size: 5,
            mtime: gate_mtime_secs(&p_b),
            mtime_nanos: None,
            ctime: None,
            inode: None,
        },
    );
    gate_write_manifest(folder, &hashes);

    let paths = vec![p_a, p_b];
    assert!(!should_rebuild_for_paths(folder.to_str().unwrap(), &paths, None));
}

#[test]
fn gate_passes_when_any_path_is_changed() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let p = gate_write_file(folder, "a.md", b"hello changed");

    let mut hashes = crate::types::content::SiteHashes::new();
    hashes.sources.insert(
        "a.md".into(),
        SourceMetadata {
            hash: gate_sha256_hex(b"hello"),
            size: 5,
            mtime: 0,
            mtime_nanos: None,
            ctime: None,
            inode: None,
        },
    );
    gate_write_manifest(folder, &hashes);

    assert!(should_rebuild_for_paths(folder.to_str().unwrap(), &[p], None));
}

#[test]
fn gate_passes_when_path_is_unknown_to_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let p = gate_write_file(folder, "new.md", b"hi");
    gate_write_manifest(folder, &crate::types::content::SiteHashes::new());
    assert!(should_rebuild_for_paths(folder.to_str().unwrap(), &[p], None));
}

#[test]
fn gate_passes_when_manifest_file_absent() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let p = gate_write_file(folder, "a.md", b"hello");
    // No .moss/build/hashes.json at all.
    assert!(should_rebuild_for_paths(folder.to_str().unwrap(), &[p], None));
}

#[test]
fn gate_passes_when_path_is_outside_folder() {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let p = gate_write_file(other.path(), "foreign.md", b"x");
    gate_write_manifest(dir.path(), &crate::types::content::SiteHashes::new());
    assert!(should_rebuild_for_paths(dir.path().to_str().unwrap(), &[p], None));
}

#[test]
fn gate_prefers_in_memory_baseline_over_stale_disk_manifest() {
    // Undo-during-seal-tail: hashes.json is persisted by a DETACHED seal task
    // minutes after the build renders, so mid-tail it still describes the
    // build BEFORE last. The user edits C0→C1 (build renders C1, stash holds
    // C1), then undoes back to C0 while the disk manifest still says C0.
    // Compared against the disk manifest the revert is `Unchanged` and is
    // suppressed forever; compared against the stash it is a real change.
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let p = gate_write_file(folder, "a.md", b"hello"); // reverted bytes (C0)

    // Stale disk manifest: still matches the reverted bytes.
    let mut disk = crate::types::content::SiteHashes::new();
    disk.sources.insert(
        "a.md".into(),
        SourceMetadata {
            hash: gate_sha256_hex(b"hello"),
            size: 5,
            mtime: 0, // force the hash path, not the metadata fast path
            mtime_nanos: None,
            ctime: None,
            inode: None,
        },
    );
    gate_write_manifest(folder, &disk);

    // In-memory stash: the last build rendered the edited content (C1).
    let mut stash = crate::types::content::SiteHashes::new();
    stash.sources.insert(
        "a.md".into(),
        SourceMetadata {
            hash: gate_sha256_hex(b"HELLO"),
            size: 5,
            mtime: 0,
            mtime_nanos: None,
            ctime: None,
            inode: None,
        },
    );

    // Disk-only baseline reproduces the stale suppression...
    assert!(!should_rebuild_for_paths(folder.to_str().unwrap(), &[p.clone()], None));
    // ...the stash baseline must see the revert and rebuild.
    assert!(should_rebuild_for_paths(folder.to_str().unwrap(), &[p], Some(&stash)));
}

#[test]
fn gate_reads_stash_not_disk_when_stash_present() {
    // The converse: when a stash exists, the gate must not silently fall back
    // to disk. No manifest on disk at all (which alone would fail open and
    // rebuild) — a suppression can only come from consulting the stash.
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let p = gate_write_file(folder, "a.md", b"hello");

    let mut stash = crate::types::content::SiteHashes::new();
    stash.sources.insert(
        "a.md".into(),
        SourceMetadata {
            hash: gate_sha256_hex(b"hello"),
            size: 5,
            mtime: 0,
            mtime_nanos: None,
            ctime: None,
            inode: None,
        },
    );

    assert!(!should_rebuild_for_paths(folder.to_str().unwrap(), &[p], Some(&stash)));
}

// Pump-side gate (pump_gate) and Modify-only filter tests. The pump decides
// without file I/O; the hash tier (should_rebuild_for_paths, tested above)
// runs at the worker's admission over the paths the pump deferred.

use notify::event::{CreateKind, ModifyKind, RemoveKind};
use notify::EventKind;

/// The two halves compose to the old behavior: the pump defers a Modify, and
/// the worker-side hash check suppresses it when the bytes match the manifest.
#[test]
fn a_deferred_modify_on_unchanged_bytes_is_suppressed_at_admission() {
    let _guard = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let p = gate_write_file(folder, "a.md", b"hello");
    let mut hashes = crate::types::content::SiteHashes::new();
    hashes.sources.insert(
        "a.md".into(),
        SourceMetadata {
            hash: gate_sha256_hex(b"hello"),
            size: 5,
            mtime: gate_mtime_secs(&p),
            mtime_nanos: None,
            ctime: None,
            inode: None,
        },
    );
    gate_write_manifest(folder, &hashes);

    let verdict = pump_gate(
        EventKind::Modify(ModifyKind::Metadata(notify::event::MetadataKind::Any)),
        folder.to_str().unwrap(),
        std::slice::from_ref(&p),
    );
    assert_eq!(verdict, PumpGate::DeferHashCheck, "the pump must not run the hash check");
    assert!(
        !should_rebuild_for_paths(folder.to_str().unwrap(), &[p], None),
        "the deferred check suppresses the unchanged file"
    );
}

#[test]
fn pump_gate_proceeds_unconditionally_on_create_and_remove() {
    let _guard = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    // The hash gate can never vouch for a file the last build didn't see,
    // so neither kind defers to it.
    let create = pump_gate(
        EventKind::Create(CreateKind::File),
        folder.to_str().unwrap(),
        &[folder.join("new.md")],
    );
    assert_eq!(create, PumpGate::Proceed);
    let remove = pump_gate(
        EventKind::Remove(RemoveKind::File),
        folder.to_str().unwrap(),
        &[folder.join("gone.md")],
    );
    assert_eq!(remove, PumpGate::Proceed);
}

#[test]
fn pump_gate_does_not_hash_gate_background_passthrough_sources() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    for name in ["index.html", "app.js", "style.css", "clip.mp4"] {
        let verdict = pump_gate(
            EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Content)),
            folder.to_str().unwrap(),
            &[folder.join(name)],
        );
        assert_eq!(
            verdict,
            PumpGate::Proceed,
            "{name} is finalized after the synchronous source-hash stash"
        );
    }
}

#[test]
fn pump_gate_proceeds_when_kill_switch_set() {
    let _guard = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::set_var("MOSS_WATCH_NO_GATE", "1");
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let verdict = pump_gate(
        EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Content)),
        folder.to_str().unwrap(),
        &[folder.join("a.md")],
    );
    std::env::remove_var("MOSS_WATCH_NO_GATE");
    assert_eq!(
        verdict,
        PumpGate::Proceed,
        "with the gate disabled nothing may defer (a deferred check could still suppress)"
    );
}

/// A root agent-instruction file is excluded from watch triggers regardless
/// of who wrote it — the author's own edit to it must not rebuild the site
/// any more than moss's own guidance sync would, were it still writing here.
#[test]
fn pump_gate_suppresses_a_root_agent_instruction_file() {
    let _guard = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    for name in ["AGENTS.md", "CLAUDE.md", "GEMINI.md"] {
        let p = gate_write_file(folder, name, b"# moss");
        // Create, not Modify: the first write of the file is a create, and the
        // hash gate deliberately never suppresses those.
        let verdict = pump_gate(EventKind::Create(CreateKind::File), folder.to_str().unwrap(), &[p]);
        assert_eq!(verdict, PumpGate::Suppress, "{name} must not rebuild");
    }
}

/// The suppression is root-only, matching `scan::classify::skip_root_agent_config`.
/// `posts/AGENTS.md` is an ordinary article and must rebuild like any other.
#[test]
fn pump_gate_still_rebuilds_for_a_nested_agents_md() {
    let _guard = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    std::fs::create_dir_all(folder.join("posts")).unwrap();
    let p = gate_write_file(folder, "posts/AGENTS.md", b"# a real article");

    let verdict = pump_gate(EventKind::Create(CreateKind::File), folder.to_str().unwrap(), &[p]);
    assert_eq!(verdict, PumpGate::Proceed);
}

/// A rename carries both sides. Renaming the file into the site is a real
/// content change, so the all-paths test must not collapse it to Suppress —
/// it defers, and the missing from-side makes the hash check fail open.
#[test]
fn a_rename_of_an_agent_file_into_content_still_rebuilds() {
    let _guard = crate::ENV_TEST_MUTEX
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let to = gate_write_file(folder, "about.md", b"# now a page");
    let paths = vec![folder.join("AGENTS.md"), to];

    let verdict = pump_gate(
        EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both)),
        folder.to_str().unwrap(),
        &paths,
    );
    assert_eq!(verdict, PumpGate::DeferHashCheck);
    assert!(
        should_rebuild_for_paths(folder.to_str().unwrap(), &paths, None),
        "the deferred check must fail open on the vanished from-side"
    );
}

#[test]
fn should_gate_modify_event_is_modify_only() {
    assert!(should_gate_modify_event(EventKind::Modify(
        ModifyKind::Data(notify::event::DataChange::Content)
    )));
    assert!(should_gate_modify_event(EventKind::Modify(
        ModifyKind::Metadata(notify::event::MetadataKind::Any)
    )));
    assert!(!should_gate_modify_event(EventKind::Create(
        CreateKind::File
    )));
    assert!(!should_gate_modify_event(EventKind::Remove(
        RemoveKind::File
    )));
}

// ── build_rebuild_event_with_renames tests ─────────────────────────────
//
// The function takes the watcher's stitched rename pairs (see
// `extract_rename_pairs` and the `notify-debouncer-full` PATTERN 6
// citation) and resolves them through the manifest's source→output index.

/// moss uses pretty URLs by default: `posts/foo.md` outputs to
/// `posts/foo/index.html`. The fix's correctness depends on looking up
/// the right output path from the manifest. This test pins the pretty
/// URL case (the common case for non-index articles).
#[test]
fn build_rebuild_event_resolves_pretty_url_rename_to_output_pair() {
    let pairs = [(
        "blog/old-post.md".to_string(),
        "blog/new-post.md".to_string(),
    )];

    // Simulate the actual moss output: pretty URL form.
    let mut prev = SiteHashes::new();
    prev.insert("blog/old-post/index.html".into(), "hash_a".into());

    let mut new = SiteHashes::new();
    new.insert("blog/new-post/index.html".into(), "hash_b".into());

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs)
        .expect("event should emit for a resolved rename");

    // moved_output_paths is in OUTPUT domain (matches what extractPathFromUrl
    // normalizes the iframe URL to).
    let renamed = event
        .moved_output_paths
        .expect("moved_output_paths populated");
    assert_eq!(renamed.len(), 1);
    assert_eq!(
        renamed[0],
        (
            "blog/old-post/index.html".to_string(),
            "blog/new-post/index.html".to_string()
        )
    );

    // The rename-old output is suppressed from deleted_paths so the
    // frontend's deletion-priority check doesn't shadow the rename.
    let deleted = event.deleted_paths.unwrap_or_default();
    assert!(
        !deleted.iter().any(|p| p == "blog/old-post/index.html"),
        "rename-old output must be suppressed from deleted_paths, got {:?}",
        deleted
    );
}

/// Root-level files (e.g., `home.md` → `welcome.md`) AND index files
/// (`index.md`, `readme.md`) emit the FLAT `.html` form. Both must work.
#[test]
fn build_rebuild_event_resolves_flat_html_rename_to_output_pair() {
    let pairs = [("home.md".to_string(), "welcome.md".to_string())];

    let mut prev = SiteHashes::new();
    prev.insert("home.html".into(), "hash_a".into());

    let mut new = SiteHashes::new();
    new.insert("welcome.html".into(), "hash_b".into());

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs).expect("event emits");

    let renamed = event
        .moved_output_paths
        .expect("moved_output_paths populated");
    assert_eq!(
        renamed[0],
        ("home.html".to_string(), "welcome.html".to_string())
    );

    let deleted = event.deleted_paths.unwrap_or_default();
    assert!(!deleted.iter().any(|p| p == "home.html"));
}

/// decide_rebuild_event: with a fresh stash the edited page surfaces.
/// This is the primary contract test for the stale-hash race fix — it
/// covers the full chain (stash selection → diff → event) without needing
/// a tauri::AppHandle.
#[test]
fn decide_rebuild_event_with_fresh_stash_emits_changed_page() {
    let mut prev = SiteHashes::new();
    prev.insert("research/index.html".into(), "old-hash".into());
    prev.source_to_output
        .insert("research.md".into(), "research/index.html".into());

    // Fresh in-memory hashes: the page was re-rendered this build.
    let mut stash = SiteHashes::new();
    stash.insert("research/index.html".into(), "new-hash".into());
    stash
        .source_to_output
        .insert("research.md".into(), "research/index.html".into());

    let event = decide_rebuild_event(Some(stash), "irrelevant", &prev, &[])
        .expect("fresh stash with a changed page must emit a refresh event");
    assert_eq!(
        event.changed_output_files.as_deref(),
        Some(&["research/index.html".to_string()][..]),
        "changed page must be in changed_output_files"
    );
}

/// decide_rebuild_event: a stale stash (equal to previous) produces no
/// event — pinning the exact bug behavior so we know what we fixed.
#[test]
fn decide_rebuild_event_with_stale_stash_produces_no_event() {
    let mut prev = SiteHashes::new();
    prev.insert("research/index.html".into(), "same-hash".into());
    prev.source_to_output
        .insert("research.md".into(), "research/index.html".into());

    let stale = prev.clone(); // identical to prev — simulates the racy disk re-read
    assert!(
        decide_rebuild_event(Some(stale), "irrelevant", &prev, &[]).is_none(),
        "stale stash equal to prev must produce no event (documents the pre-fix bug)"
    );
}

/// A rebuild that CREATED a new source-derived page must emit a
/// FileChangeEvent — even when the output-file hash diff is empty (the new
/// page's output was carried forward / already folded into the baseline, so
/// `compute_rebuild_event`'s `files`-set diff alone would suppress). Without
/// at least one emitted event, PreviewFollower's FileChanged retry never
/// fires and a brand-new empty page's preview never resolves off the 404.
///
/// This locks the guarantee provided by the source-domain `source_creates`
/// path in `build_rebuild_event_with_renames`: the new source key is present
/// in `new.source_to_output` but absent from `previous`, so the event is
/// emitted (carrying `source_creates`) regardless of the output-hash diff.
#[test]
fn decide_rebuild_event_emits_for_new_page_even_with_no_output_hash_diff() {
    // Output files identical between builds (no `files`-set diff at all).
    let mut prev = SiteHashes::new();
    prev.insert("index.html".into(), "same-hash".into());
    prev.source_to_output
        .insert("index.md".into(), "index.html".into());

    let mut stash = SiteHashes::new();
    stash.insert("index.html".into(), "same-hash".into());
    stash
        .source_to_output
        .insert("index.md".into(), "index.html".into());
    // The brand-new empty page: a NEW source→output mapping, but its output
    // hash is not reflected as a `files`-set change this cycle.
    stash
        .source_to_output
        .insert("new-page.md".into(), "new-page/index.html".into());

    let event = decide_rebuild_event(Some(stash), "irrelevant", &prev, &[])
        .expect("a newly-created page must emit an event so the retry gets a trigger");
    assert_eq!(
        event.source_creates.as_deref(),
        Some(&["new-page.md".to_string()][..]),
        "the new page must surface as a source create"
    );
}

/// decide_rebuild_event: no stash → falls back to disk read. Uses a
/// temp dir to test the fallback path without a Tauri AppHandle.
#[test]
fn decide_rebuild_event_fallback_to_disk_when_no_stash() {
    // Use create_dir_all to ensure the parent exists (target/test-tmp may
    // not exist on a fresh checkout before any cargo build).
    let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
        .parent()
        .unwrap()
        .join("target/test-tmp");
    std::fs::create_dir_all(&parent).unwrap();
    let dir = tempfile::TempDir::new_in(&parent).unwrap();
    let folder = dir.path().to_str().unwrap();

    // Write a hashes.json so load_previous_hashes finds something.
    // (.moss/build/staging/ is not required by load_previous_hashes — only hashes.json is.)
    let hashes_path = dir.path().join(".moss/build/hashes.json");
    std::fs::create_dir_all(dir.path().join(".moss/build")).unwrap();
    let mut disk_hashes = SiteHashes::new();
    disk_hashes.insert("index.html".into(), "disk-hash".into());
    std::fs::write(&hashes_path, serde_json::to_string(&disk_hashes).unwrap()).unwrap();

    let mut prev = SiteHashes::new();
    prev.insert("index.html".into(), "old-hash".into()); // differs from disk

    // No stash → falls back to load_previous_hashes(folder) → disk_hashes.
    // disk_hashes["index.html"] = "disk-hash" ≠ prev["index.html"] = "old-hash"
    let event = decide_rebuild_event(None, folder, &prev, &[])
        .expect("disk fallback with a changed hash must emit an event");
    assert_eq!(
        event.changed_output_files.as_deref(),
        Some(&["index.html".to_string()][..])
    );
}



/// The stash must describe what the screen shows (build.rs guards the
/// `stash_content_hashes` call with this). A cloud-withheld build
/// (`publishable == false`) and a cancelled build never switch the preview
/// (`pipeline::run` only calls `switch_to` under `if publishable`, and the
/// cancelled early-return precedes both arms), so stashing their hashes made
/// the watcher's next refresh diff compare against a build the user never
/// saw — a later publishable build's `changed_output_files` then omitted
/// pages edited before the withheld build and the preview never refreshed.
#[test]
fn stash_only_when_the_preview_switched_to_this_build() {
    use crate::build::should_stash_hashes;

    // The one combination where the preview switched to this build's output.
    assert!(should_stash_hashes(true, false));

    // Withheld: staged but never shown — must not become the baseline.
    assert!(!should_stash_hashes(false, false));

    // Cancelled: pipeline returns publishable=false too, but the guard must
    // not rely on that coupling holding forever.
    assert!(!should_stash_hashes(false, true));
    assert!(!should_stash_hashes(true, true));
}



/// baseline_for_rebuild: when an in-memory baseline exists it is used
/// VERBATIM and the disk `hashes.json` is NOT consulted — even if disk is
/// poisoned with a future build's hashes (the detached-seal race). This is
/// the core of the "no refresh" fix.
#[test]
fn baseline_for_rebuild_prefers_in_memory_over_poisoned_disk() {
    // Set up a folder whose on-disk hashes.json is POISONED: it claims the
    // page already has the value THIS rebuild will produce ("v2"), as if a
    // late seal from a future state landed. A disk-loaded baseline would
    // therefore make the edited page look unchanged.
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path();
    std::fs::create_dir_all(folder.join(".moss/build/staging")).unwrap();
    std::fs::write(
        folder.join(".moss/build/hashes.json"),
        r#"{"files":{"research/index.html":"100644:v2"}}"#,
    )
    .unwrap();
    let output = folder.join(".moss/build/staging");

    // The race-free in-memory baseline holds the TRUE previous value ("v1").
    let mut in_mem = SiteHashes::new();
    in_mem.insert("research/index.html".into(), "100644:v1".into());

    let baseline = baseline_for_rebuild(Some(in_mem), folder.to_str().unwrap(), &output);
    assert_eq!(
        baseline
            .files
            .get("research/index.html")
            .map(String::as_str),
        Some("100644:v1"),
        "must use the in-memory baseline (v1), NOT the poisoned disk (v2)"
    );
}

/// baseline_for_rebuild: with NO in-memory baseline, falls back to disk —
/// preserving CLI/headless/external-build and first-rebuild behavior.
#[test]
fn baseline_for_rebuild_falls_back_to_disk_when_no_in_memory() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path();
    std::fs::create_dir_all(folder.join(".moss/build/staging")).unwrap();
    std::fs::write(
        folder.join(".moss/build/hashes.json"),
        r#"{"files":{"index.html":"100644:disk"}}"#,
    )
    .unwrap();
    let output = folder.join(".moss/build/staging");

    let baseline = baseline_for_rebuild(None, folder.to_str().unwrap(), &output);
    assert_eq!(
        baseline.files.get("index.html").map(String::as_str),
        Some("100644:disk"),
        "no in-memory baseline → load from disk"
    );
}

/// End-to-end logic of the fix: a real edit (X: v1 → v2) refreshes when the
/// baseline is the race-free in-memory previous (v1), even though a poisoned
/// disk would have said v2 (→ the spurious action=none the user sees). This
/// composes baseline_for_rebuild with decide_rebuild_event — the two halves
/// the rebuild path actually runs.
#[test]
fn baseline_race_fix_emits_refresh_for_edited_page() {
    // Prior build's race-free hashes (the in-memory baseline): X == v1.
    let mut prev = SiteHashes::new();
    prev.insert("research/index.html".into(), "100644:v1".into());
    prev.source_to_output
        .insert("research.md".into(), "research/index.html".into());

    // This rebuild (edit X → v2): new stash X == v2.
    let mut new = SiteHashes::new();
    new.insert("research/index.html".into(), "100644:v2".into());
    new.source_to_output
        .insert("research.md".into(), "research/index.html".into());

    // FIXED path: baseline = the in-memory prev (v1). baseline_for_rebuild
    // with Some(_) ignores disk entirely (the disk-fallback / poisoned-disk
    // paths are covered by the baseline_for_rebuild_* tests above), so we
    // pass a non-existent path to prove disk is never consulted here.
    let baseline = baseline_for_rebuild(
        Some(prev),
        "irrelevant",
        std::path::Path::new("/nonexistent"),
    );
    let event = decide_rebuild_event(Some(new), "irrelevant", &baseline, &[])
        .expect("edited page must refresh when baseline is the race-free previous");
    assert_eq!(
        event.changed_output_files.as_deref(),
        Some(&["research/index.html".to_string()][..]),
        "edited page must be in changed_output_files → preview refreshes"
    );

    // CONTRAST (documents the bug): a poisoned baseline that already equals
    // THIS build's output (what a late disk seal can produce) yields no
    // event → the spurious action=none the user experiences. (The disk
    // fallback path also drops source_to_output, but the file-hash equality
    // is the load-bearing case; build it as an exact clone to isolate it.)
    let mut new2 = SiteHashes::new();
    new2.insert("research/index.html".into(), "100644:v2".into());
    new2.source_to_output
        .insert("research.md".into(), "research/index.html".into());
    let poisoned = new2.clone(); // baseline already == this build's output
    assert!(
        decide_rebuild_event(Some(new2), "irrelevant", &poisoned, &[]).is_none(),
        "poisoned baseline equal to this build's output reproduces the no-refresh bug"
    );
}

/// In-memory carry-forward deletion. With the stale-hash-race fix the
/// rebuild event is computed from the build's IN-MEMORY hashes, where a
/// deleted page's OUTPUT still lingers in `files` (PendingManifest carries
/// the previous manifest forward; the mark-and-sweep prune only happens at
/// `seal`). So `get_deleted_files` (a `files`-set diff) cannot see the
/// deletion. The authoritative signal is `source_to_output`: it is cleared
/// and re-populated per build, so a page that was NOT rendered this build
/// is absent from `new.source_to_output` while present in `prev`. That
/// source-domain deletion must be mapped to its OUTPUT path and surfaced in
/// `deleted_paths` so the preview redirects home. Pre-fix this returned no
/// `deleted_paths` (it only set the source-domain `source_deletes`).
#[test]
fn build_rebuild_event_maps_source_to_output_deletion_to_deleted_paths() {
    let mut prev = SiteHashes::new();
    prev.insert("posts/gone/index.html".into(), "h1".into());
    prev.source_to_output
        .insert("posts/gone.md".into(), "posts/gone/index.html".into());
    // A surviving page (carried forward + still rendered this build).
    prev.insert("index.html".into(), "home1".into());
    prev.source_to_output
        .insert("index.md".into(), "index.html".into());

    let mut new = SiteHashes::new();
    // Carry-forward: the deleted page's output still lingers in `files`...
    new.insert("posts/gone/index.html".into(), "h1".into());
    new.insert("index.html".into(), "home1".into());
    // ...but its source is absent from this build's source_to_output.
    new.source_to_output
        .insert("index.md".into(), "index.html".into());

    let pairs: [(String, String); 0] = [];
    let event = build_rebuild_event_with_renames(&new, &prev, &pairs)
        .expect("a page deletion must emit a rebuild event");
    let deleted = event
        .deleted_paths
        .expect("deleted_paths populated for a removed page");
    assert!(
        deleted.iter().any(|p| p == "posts/gone/index.html"),
        "deleted page output must be in deleted_paths, got {deleted:?}"
    );
}

/// Regression lock for the stale-hash race. With the build's FRESH
/// (in-memory) hashes, an edited page surfaces in `changed_output_files`
/// (→ refresh). With a STALE snapshot equal to the previous manifest —
/// exactly what re-reading the not-yet-persisted `hashes.json` returned —
/// the diff is empty and no event fires. The fix feeds
/// `do_rebuild_and_notify` the in-memory hashes so the former case is what
/// actually happens; this test pins both ends of that contract.
#[test]
fn build_rebuild_event_reflects_fresh_hashes_not_stale_snapshot() {
    let mut prev = SiteHashes::new();
    prev.insert("research/index.html".into(), "old".into());
    prev.source_to_output
        .insert("research.md".into(), "research/index.html".into());

    // Fresh (in-memory) hashes: the edited page re-rendered to a new hash.
    let mut fresh = SiteHashes::new();
    fresh.insert("research/index.html".into(), "new".into());
    fresh
        .source_to_output
        .insert("research.md".into(), "research/index.html".into());

    let pairs: [(String, String); 0] = [];
    let event = build_rebuild_event_with_renames(&fresh, &prev, &pairs)
        .expect("fresh hashes with a changed page must emit a refresh");
    assert_eq!(
        event.changed_output_files.as_deref(),
        Some(&["research/index.html".to_string()][..]),
        "changed page must be in changed_output_files"
    );

    // Stale snapshot == prev (what the racy disk re-read returned): nothing.
    let stale = prev.clone();
    assert!(
        build_rebuild_event_with_renames(&stale, &prev, &pairs).is_none(),
        "a stale snapshot equal to prev must produce no event (the bug being fixed)"
    );
}

/// Source-stable slug change: the user removes frontmatter `title:` from
/// `works/article.md`. The source filename is unchanged but the output URL
/// moves (title-derived slug → filename-derived slug). Without pairing,
/// the old output ends up in `deleted_paths` and the frontend's deletion-
/// priority check redirects the preview to home, dragging the editor and
/// file tree along via the preview→editor sync. Pairing it as a rename
/// lets the frontend take the `update-url` branch.
#[test]
fn build_rebuild_event_pairs_source_stable_slug_change_as_rename() {
    // No FS rename — slug change comes entirely from the manifest diff.
    let pairs: [(String, String); 0] = [];

    let mut prev = SiteHashes::new();
    prev.insert("works/yi-liu/index.html".into(), "hash_a".into());
    prev.source_to_output
        .insert("works/article.md".into(), "works/yi-liu/index.html".into());

    let mut new = SiteHashes::new();
    new.insert("works/article/index.html".into(), "hash_b".into());
    new.source_to_output
        .insert("works/article.md".into(), "works/article/index.html".into());

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs)
        .expect("event emits for slug change");

    let renamed = event
        .moved_output_paths
        .expect("moved_output_paths populated");
    assert_eq!(
        renamed,
        vec![(
            "works/yi-liu/index.html".to_string(),
            "works/article/index.html".to_string(),
        )]
    );

    // Old output suppressed from deleted_paths. Since it was the only
    // deletion, the whole field collapses to None — exercises the
    // `if deleted.is_empty() { e.deleted_paths = None }` branch.
    assert!(
        event.deleted_paths.is_none(),
        "deleted_paths must clear to None when slug-change old output is the only entry, got {:?}",
        event.deleted_paths
    );
}

/// FS rename + source-stable slug change in the same rebuild. The watcher
/// stitches (foo.md, bar.md); a third source (article.md) has its title
/// removed in the same cycle. Both pairs must land in `moved_output_paths`,
/// neither duplicated, neither shadowing the other's old output in
/// `deleted_paths`.
#[test]
fn build_rebuild_event_combines_fs_rename_with_slug_change() {
    let pairs = [("blog/foo.md".to_string(), "blog/bar.md".to_string())];

    let mut prev = SiteHashes::new();
    prev.insert("blog/foo/index.html".into(), "hash_foo".into());
    prev.insert("works/yi-liu/index.html".into(), "hash_yiliu".into());
    prev.source_to_output
        .insert("blog/foo.md".into(), "blog/foo/index.html".into());
    prev.source_to_output
        .insert("works/article.md".into(), "works/yi-liu/index.html".into());

    let mut new = SiteHashes::new();
    new.insert("blog/bar/index.html".into(), "hash_bar".into());
    new.insert("works/article/index.html".into(), "hash_article".into());
    new.source_to_output
        .insert("blog/bar.md".into(), "blog/bar/index.html".into());
    new.source_to_output
        .insert("works/article.md".into(), "works/article/index.html".into());

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs).expect("event emits");

    let mut renamed = event
        .moved_output_paths
        .expect("moved_output_paths populated");
    renamed.sort();
    assert_eq!(
        renamed,
        vec![
            (
                "blog/foo/index.html".to_string(),
                "blog/bar/index.html".to_string(),
            ),
            (
                "works/yi-liu/index.html".to_string(),
                "works/article/index.html".to_string(),
            ),
        ],
        "FS rename and slug change must both pair, neither duplicated"
    );

    assert!(
        event.deleted_paths.is_none(),
        "both old outputs must clear from deleted_paths, got {:?}",
        event.deleted_paths
    );
}

/// Slug-dedup swap: two sources collide on their preferred slug, and the
/// rebuild reassigns which gets the unsuffixed form. Both sources have
/// stable filenames but their output URLs swap places. Both pairs must
/// land in `moved_output_paths` so a preview on either page tracks correctly.
/// Locks the invariant that the pairing loop scales across the manifest
/// rather than only handling one pair at a time.
#[test]
fn build_rebuild_event_pairs_slug_dedup_swap_for_both_sources() {
    let pairs: [(String, String); 0] = [];

    let mut prev = SiteHashes::new();
    prev.insert("post/index.html".into(), "hash_a_prev".into());
    prev.insert("post-2/index.html".into(), "hash_b_prev".into());
    prev.source_to_output
        .insert("a/post.md".into(), "post/index.html".into());
    prev.source_to_output
        .insert("b/post.md".into(), "post-2/index.html".into());

    let mut new = SiteHashes::new();
    new.insert("post/index.html".into(), "hash_b_new".into());
    new.insert("post-2/index.html".into(), "hash_a_new".into());
    new.source_to_output
        .insert("a/post.md".into(), "post-2/index.html".into());
    new.source_to_output
        .insert("b/post.md".into(), "post/index.html".into());

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs)
        .expect("event emits for slug-swap");

    let mut renamed = event
        .moved_output_paths
        .expect("moved_output_paths populated");
    renamed.sort();
    assert_eq!(
        renamed,
        vec![
            (
                "post-2/index.html".to_string(),
                "post/index.html".to_string(),
            ),
            (
                "post/index.html".to_string(),
                "post-2/index.html".to_string(),
            ),
        ],
        "both swap pairs must land in moved_output_paths",
    );

    // Each old output is also a new output (they swapped), so the diff
    // itself produces no deletions; deleted_paths should be None.
    assert!(
        event.deleted_paths.is_none(),
        "no path is truly deleted in a swap, got {:?}",
        event.deleted_paths
    );
}

/// A source whose output URL is unchanged across rebuilds must NOT be
/// emitted as a rename. Only sources with differing output paths qualify.
#[test]
fn build_rebuild_event_skips_pairing_when_output_unchanged() {
    let pairs: [(String, String); 0] = [];

    let mut prev = SiteHashes::new();
    prev.insert("blog/post/index.html".into(), "hash_a".into());
    prev.source_to_output
        .insert("blog/post.md".into(), "blog/post/index.html".into());

    let mut new = SiteHashes::new();
    new.insert("blog/post/index.html".into(), "hash_b".into()); // content changed, URL same
    new.source_to_output
        .insert("blog/post.md".into(), "blog/post/index.html".into());

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs)
        .expect("event emits for content change");

    // A pure content change must surface as changed_output_files, not as
    // a rename — otherwise we'd flood moved_output_paths on every edit.
    assert!(
        event.moved_output_paths.is_none(),
        "no rename for unchanged URL"
    );
}

/// If the rebuild's manifest doesn't contain the rename pair's source
/// (e.g., the rebuild hasn't caught up yet, or the source produces no
/// output — notebook/asset renames), the output-domain pair is silently
/// dropped (`moved_output_paths` stays empty). However, Task 3 (entry-id
/// PR-1) adds source-domain fields: `source_renames` still echoes the
/// watcher's pairs, so the EntryRegistry can track the rename even when
/// the output path can't be resolved.
///
/// Updated contract: a rename pair that can't resolve to output paths
/// returns `Some(event)` with `source_renames` set but no
/// `moved_output_paths` or `deleted_paths`.
#[test]
fn build_rebuild_event_drops_pair_when_output_not_in_manifest() {
    let pairs = [("blog/old.md".to_string(), "blog/new.md".to_string())];

    // Empty manifests — neither old nor new output is registered.
    let prev = SiteHashes::new();
    let new = SiteHashes::new();
    let event = build_rebuild_event_with_renames(&new, &prev, &pairs);

    // Output-domain pair can't be resolved — no moved_output_paths.
    // But source_renames is still populated for the EntryRegistry.
    let event = event.expect("rename pair always emits an event (source_renames populated)");
    assert!(
        event.moved_output_paths.is_none(),
        "output pair unresolvable — moved_output_paths stays None"
    );
    assert_eq!(
        event.source_renames.as_deref(),
        Some(&[("blog/old.md".to_string(), "blog/new.md".to_string())][..]),
        "source_renames echoes the input pair even when output is unresolvable"
    );
}

/// Unrelated changes (deletions, content changes) must still propagate
/// when a rename pair is also present.
#[test]
fn build_rebuild_event_preserves_unrelated_changes_alongside_rename() {
    let pairs = [("blog/foo.md".to_string(), "blog/bar.md".to_string())];

    let mut prev = SiteHashes::new();
    prev.insert("blog/foo/index.html".into(), "hash_a".into());
    prev.insert("unrelated.html".into(), "hash_b".into());
    prev.insert("deleted-elsewhere/index.html".into(), "hash_c".into());

    let mut new = SiteHashes::new();
    new.insert("blog/bar/index.html".into(), "hash_d".into());
    new.insert("unrelated.html".into(), "hash_e".into()); // different hash → "changed"
                                                          // deleted-elsewhere not in new → genuine deletion (not a rename)

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs).expect("event emits");

    // Rename pair emitted with output paths.
    let renamed = event.moved_output_paths.expect("moved_output_paths set");
    assert_eq!(renamed[0].0, "blog/foo/index.html");
    assert_eq!(renamed[0].1, "blog/bar/index.html");

    // The unrelated genuine deletion is preserved in deleted_paths.
    let deleted = event.deleted_paths.expect("deleted_paths preserved");
    assert!(deleted.iter().any(|p| p == "deleted-elsewhere/index.html"));
    // But the rename-old is NOT in deleted_paths.
    assert!(!deleted.iter().any(|p| p == "blog/foo/index.html"));

    // Unrelated content changes still propagate.
    let changed = event.changed_output_files.unwrap_or_default();
    assert!(changed.iter().any(|p| p == "unrelated.html"));
}

/// Empty input — no rename pairs and no hash diff — must return None
/// (suppress refresh).
#[test]
fn build_rebuild_event_no_pairs_no_diff_returns_none() {
    let prev = SiteHashes::new();
    let new = SiteHashes::new();
    let event = build_rebuild_event_with_renames(&new, &prev, &[]);
    assert!(
        event.is_none(),
        "no diff + no renames + no plugin change = no event"
    );
}

/// Unit test for the heuristic fallback: covers pretty URL preference,
/// flat fallback, non-md sources, and missing-from-manifest. Exercised
/// only when `source_to_output` is empty (legacy manifest pre-fix).
#[test]
fn find_output_for_source_fallback_handles_pretty_flat_and_missing() {
    let mut hashes = SiteHashes::new();
    hashes.insert("blog/pretty/index.html".into(), "h1".into());
    hashes.insert("flat-root.html".into(), "h2".into());

    // Pretty form preferred when both might match.
    assert_eq!(
        find_output_for_source("blog/pretty.md", &hashes),
        Some("blog/pretty/index.html".to_string())
    );

    // Flat form found when pretty form is absent.
    assert_eq!(
        find_output_for_source("flat-root.md", &hashes),
        Some("flat-root.html".to_string())
    );

    // Source with no output → None.
    assert_eq!(find_output_for_source("missing.md", &hashes), None);

    // Non-.md source → None (we only know how to map markdown).
    assert_eq!(find_output_for_source("image.jpg", &hashes), None);
}

/// Manifest lookup wins for cases the heuristic gets wrong:
/// slug-normalized capital/spaces/punctuation → kebab-case lowercase.
#[test]
fn find_output_for_source_resolves_slugified_titlecase_via_manifest() {
    let mut hashes = SiteHashes::new();
    // The manifest has `posts/hello-world/index.html` (slugified from `Hello World.md`),
    // not `Posts/Hello World/index.html` that the stem-based heuristic would try.
    hashes.insert("posts/hello-world/index.html".into(), "h1".into());
    hashes.source_to_output.insert(
        "Posts/Hello World.md".into(),
        "posts/hello-world/index.html".into(),
    );

    assert_eq!(
        find_output_for_source("Posts/Hello World.md", &hashes),
        Some("posts/hello-world/index.html".to_string())
    );
}

/// `readme.md` outputs to `index.html` (canonical home form), not
/// `readme/index.html` or `readme.html`. Heuristic would miss; manifest
/// catches it.
#[test]
fn find_output_for_source_resolves_readme_index_via_manifest() {
    let mut hashes = SiteHashes::new();
    hashes.insert("index.html".into(), "h1".into());
    hashes
        .source_to_output
        .insert("readme.md".into(), "index.html".into());

    assert_eq!(
        find_output_for_source("readme.md", &hashes),
        Some("index.html".to_string())
    );
}

/// Root `index.md` → `index.html`. Heuristic would compute `/index.html`
/// (leading slash from empty stem) — manifest gives the right answer.
#[test]
fn find_output_for_source_resolves_root_index_via_manifest() {
    let mut hashes = SiteHashes::new();
    hashes.insert("index.html".into(), "h1".into());
    hashes
        .source_to_output
        .insert("index.md".into(), "index.html".into());

    assert_eq!(
        find_output_for_source("index.md", &hashes),
        Some("index.html".to_string())
    );
}

/// Frontmatter `url: bar` overrides the slug. The heuristic doesn't
/// know about frontmatter; the manifest does.
#[test]
fn find_output_for_source_resolves_frontmatter_url_override_via_manifest() {
    let mut hashes = SiteHashes::new();
    hashes.insert("posts/bar/index.html".into(), "h1".into());
    hashes
        .source_to_output
        .insert("posts/foo.md".into(), "posts/bar/index.html".into());

    assert_eq!(
        find_output_for_source("posts/foo.md", &hashes),
        Some("posts/bar/index.html".to_string())
    );
}

/// Manifest lookup takes precedence over the heuristic when both could
/// resolve, so a stale heuristic match doesn't shadow the canonical
/// output. (This matters during transitional states where the
/// heuristic happens to match a different file.)
#[test]
fn find_output_for_source_prefers_manifest_over_heuristic() {
    let mut hashes = SiteHashes::new();
    // Both `posts/foo/index.html` (heuristic) and `posts/foo-2/index.html`
    // (e.g. duplicate-slug dedup) exist in the manifest. The
    // source_to_output map points to the canonical, dedup-resolved form.
    hashes.insert("posts/foo/index.html".into(), "h1".into());
    hashes.insert("posts/foo-2/index.html".into(), "h2".into());
    hashes
        .source_to_output
        .insert("posts/foo.md".into(), "posts/foo-2/index.html".into());

    assert_eq!(
        find_output_for_source("posts/foo.md", &hashes),
        Some("posts/foo-2/index.html".to_string()),
        "manifest must win over heuristic"
    );
}

/// End-to-end: a TitleCase rename that the heuristic alone could never
/// fix actually produces a usable rename signal when source_to_output
/// is populated (as it will be in any post-fix build).
#[test]
fn build_rebuild_event_resolves_slugified_rename_via_manifest() {
    let pairs = [(
        "Posts/Hello World.md".to_string(),
        "Posts/Goodbye World.md".to_string(),
    )];

    let mut prev = SiteHashes::new();
    prev.insert("posts/hello-world/index.html".into(), "h1".into());
    prev.source_to_output.insert(
        "Posts/Hello World.md".into(),
        "posts/hello-world/index.html".into(),
    );

    let mut new = SiteHashes::new();
    new.insert("posts/goodbye-world/index.html".into(), "h2".into());
    new.source_to_output.insert(
        "Posts/Goodbye World.md".into(),
        "posts/goodbye-world/index.html".into(),
    );

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs).expect("event emits");

    let renamed = event
        .moved_output_paths
        .expect("moved_output_paths populated");
    assert_eq!(
        renamed[0],
        (
            "posts/hello-world/index.html".to_string(),
            "posts/goodbye-world/index.html".to_string()
        )
    );

    let deleted = event.deleted_paths.unwrap_or_default();
    assert!(
        !deleted.iter().any(|p| p == "posts/hello-world/index.html"),
        "rename-old output suppressed via manifest lookup"
    );
}

/// Home-override page_map promotion: `en/Liu Guo.md` is configured
/// as the home for the `en/` locale (frontmatter `home: true`),
/// so `compute_home_overrides` promotes it to `en/index.html`
/// during page_map construction. The HTML write loop registers this
/// promoted mapping in `source_to_output`. The rename-hint resolver must
/// pick it up — the heuristic alone would try `en/Liu Guo/index.html`
/// (slugified incorrectly), which would never match the manifest.
#[test]
fn find_output_for_source_resolves_home_override_via_manifest() {
    let mut hashes = SiteHashes::new();
    hashes.insert("en/index.html".into(), "h1".into());
    hashes
        .source_to_output
        .insert("en/Liu Guo.md".into(), "en/index.html".into());

    assert_eq!(
        find_output_for_source("en/Liu Guo.md", &hashes),
        Some("en/index.html".to_string()),
        "translation-home promotion must be resolved via the manifest"
    );
}

/// Two renames within one debounce window — `a.md → b.md` then `c.md → a.md`
/// — both pairs land in the watcher's stitched-rename batch before the
/// rebuild fires. After the rebuild:
///   - previous_hashes had `a.md`'s output and `c.md`'s output
///   - new_hashes has `b.md`'s output and `a.md`'s output (now containing c's content)
/// Both pairs must resolve and emit; both rename-old outputs must be
/// suppressed from deleted_paths so the iframe can navigate cleanly.
/// Frontend pair iteration order matters: pair 1 fires first, so an iframe
/// at /a/ navigates to /b/ (the original a's content), which is correct.
#[test]
fn build_rebuild_event_handles_two_rename_collision_in_one_debounce() {
    let pairs = [
        ("a.md".to_string(), "b.md".to_string()),
        ("c.md".to_string(), "a.md".to_string()),
    ];

    // previous: a/index.html and c/index.html exist with their original content.
    let mut prev = SiteHashes::new();
    prev.insert("a/index.html".into(), "hash_a_orig".into());
    prev.insert("c/index.html".into(), "hash_c_orig".into());
    prev.source_to_output
        .insert("a.md".into(), "a/index.html".into());
    prev.source_to_output
        .insert("c.md".into(), "c/index.html".into());

    // new: b/index.html (from rename 1) and a/index.html (from rename 2,
    // now containing c's original content). c/index.html is gone.
    let mut new = SiteHashes::new();
    new.insert("b/index.html".into(), "hash_a_orig".into()); // a's content moved here
    new.insert("a/index.html".into(), "hash_c_orig".into()); // c's content now here
    new.source_to_output
        .insert("b.md".into(), "b/index.html".into());
    new.source_to_output
        .insert("a.md".into(), "a/index.html".into());

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs).expect("event emits");

    // Both pairs resolved to output domain.
    let renamed = event
        .moved_output_paths
        .expect("moved_output_paths populated");
    assert_eq!(renamed.len(), 2);
    assert_eq!(
        renamed[0],
        ("a/index.html".to_string(), "b/index.html".to_string()),
        "pair 1: a → b"
    );
    assert_eq!(
        renamed[1],
        ("c/index.html".to_string(), "a/index.html".to_string()),
        "pair 2: c → a"
    );

    // Both rename-old outputs suppressed from deleted_paths. The hash diff
    // would have produced deleted_paths = ["c/index.html"] (a/index.html
    // is technically "changed" since same path with different content,
    // not deleted). After suppression, deleted_paths is None or empty.
    let deleted = event.deleted_paths.unwrap_or_default();
    assert!(
        !deleted.iter().any(|p| p == "c/index.html"),
        "c/index.html (rename-old of pair 2) must be suppressed"
    );
    assert!(
        !deleted.iter().any(|p| p == "a/index.html"),
        "a/index.html (rename-old of pair 1) must be suppressed if present"
    );
}

/// Documents the current contract: `find_output_for_source` trusts the
/// manifest entry without a defensive `files.contains_key` check. This
/// is correct given the `clear-on-PendingManifest::new` invariant
/// (which prevents stale entries from accumulating). If a future change
/// re-introduces carry-forward of `source_to_output`, this test will
/// keep passing while the integration silently regresses — so the test
/// in `manifest.rs` (`pending_manifest_new_clears_source_to_output_carry_forward`)
/// is the load-bearing one. This test pins the lookup-layer contract.
#[test]
fn find_output_for_source_trusts_manifest_entry_without_files_check() {
    let mut hashes = SiteHashes::new();
    // Map points to an output that ISN'T in `files`. Should still return
    // the manifest entry — the lookup is the manifest's authority,
    // freshness is the registration phase's authority.
    hashes
        .source_to_output
        .insert("posts/foo.md".into(), "posts/foo/index.html".into());

    assert_eq!(
        find_output_for_source("posts/foo.md", &hashes),
        Some("posts/foo/index.html".to_string())
    );
}

// ── compute_source_change_set tests ──────────────────────────────────

/// Sources present in `new` but not `previous` are creates, EXCEPT
/// the NEW side of a rename pair (which is already in source_renames).
#[test]
fn compute_source_change_set_finds_creates_excluding_rename_targets() {
    let mut prev = SiteHashes::new();
    prev.source_to_output
        .insert("existing.md".into(), "existing/index.html".into());

    let mut new = SiteHashes::new();
    new.source_to_output
        .insert("existing.md".into(), "existing/index.html".into());
    new.source_to_output
        .insert("brand-new.md".into(), "brand-new/index.html".into());
    new.source_to_output
        .insert("renamed-to.md".into(), "renamed-to/index.html".into());

    let pairs = [("renamed-from.md".to_string(), "renamed-to.md".to_string())];

    let (creates, deletes) = compute_source_change_set(&new, &prev, &pairs);

    assert_eq!(
        creates,
        vec!["brand-new.md".to_string()],
        "renamed-to.md must be excluded — it's the new side of a rename"
    );
    assert!(deletes.is_empty(), "no deletes in this fixture");
}

/// Sources present in `previous` but not `new` are deletes, EXCEPT
/// the OLD side of a rename pair.
#[test]
fn compute_source_change_set_finds_deletes_excluding_rename_sources() {
    let mut prev = SiteHashes::new();
    prev.source_to_output
        .insert("existing.md".into(), "existing/index.html".into());
    prev.source_to_output
        .insert("gone-forever.md".into(), "gone-forever/index.html".into());
    prev.source_to_output
        .insert("renamed-from.md".into(), "renamed-from/index.html".into());

    let mut new = SiteHashes::new();
    new.source_to_output
        .insert("existing.md".into(), "existing/index.html".into());

    let pairs = [("renamed-from.md".to_string(), "renamed-to.md".to_string())];

    let (creates, deletes) = compute_source_change_set(&new, &prev, &pairs);

    assert!(creates.is_empty(), "no creates in this fixture");
    assert_eq!(
        deletes,
        vec!["gone-forever.md".to_string()],
        "renamed-from.md must be excluded — it's the old side of a rename"
    );
}

/// Both sides of a rename are excluded simultaneously from creates and
/// deletes; unrelated creates and deletes still surface.
#[test]
fn compute_source_change_set_dedupes_rename_pair_from_both_sides() {
    let mut prev = SiteHashes::new();
    prev.source_to_output
        .insert("renamed-from.md".into(), "from/index.html".into());
    prev.source_to_output
        .insert("also-gone.md".into(), "also-gone/index.html".into());

    let mut new = SiteHashes::new();
    new.source_to_output
        .insert("renamed-to.md".into(), "to/index.html".into());
    new.source_to_output
        .insert("also-new.md".into(), "also-new/index.html".into());

    let pairs = [("renamed-from.md".to_string(), "renamed-to.md".to_string())];

    let (creates, deletes) = compute_source_change_set(&new, &prev, &pairs);

    assert_eq!(creates, vec!["also-new.md".to_string()]);
    assert_eq!(deletes, vec!["also-gone.md".to_string()]);
}

/// When the two manifests are identical and there are no rename pairs,
/// both lists are empty.
#[test]
fn compute_source_change_set_returns_empty_for_no_changes() {
    let mut prev = SiteHashes::new();
    prev.source_to_output
        .insert("only-file.md".into(), "only-file/index.html".into());

    let mut new = SiteHashes::new();
    new.source_to_output
        .insert("only-file.md".into(), "only-file/index.html".into());

    let (creates, deletes) = compute_source_change_set(&new, &prev, &[]);

    assert!(creates.is_empty());
    assert!(deletes.is_empty());
}

/// End-to-end wiring test: simulates the full rename flow without going
/// through the watcher or Tauri command harness. Verifies that:
///   1. `path_to_relative_key` produces source-relative paths matching
///      what `extract_rename_pairs` produces from a stitched watcher event.
///   2. `find_output_for_source` resolves the source paths to the
///      pretty-URL output paths in the manifest (the case for non-index
///      articles — the common case).
///   3. The deletion-suppression strips the rename-old output path so
///      the frontend's deletion-priority doesn't shadow the rename.
///   4. `moved_output_paths` is in OUTPUT domain so the frontend's direct
///      match against `currentPath` (also output-domain) succeeds.
#[test]
fn full_rename_flow_pretty_url_resolves_to_output_pair_and_suppresses_deletion() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let project_root = dir.path();
    let blog = project_root.join("blog");
    std::fs::create_dir(&blog).unwrap();
    let old = blog.join("old-post.md");
    let new = blog.join("new-post.md");
    std::fs::write(&old, "# Old Post").unwrap();

    // Apply the same path normalization the watcher's `extract_rename_pairs`
    // applies: compute relative keys for both sides (`old` before rename
    // so canonicalize() works; `new` after rename — the watcher actually
    // sees both paths post-rename, so canonicalize either falls through
    // to the raw path or, in the test, succeeds because we rename below).
    let old_rel = path_to_relative_key(project_root, &old).unwrap();
    let parent_rel = path_to_relative_key(project_root, new.parent().unwrap()).unwrap();
    let new_rel = format!(
        "{}/{}",
        parent_rel,
        new.file_name().unwrap().to_string_lossy()
    );
    std::fs::rename(&old, &new).unwrap();

    let pairs = [(old_rel, new_rel)];

    // Construct hashes simulating moss's PRETTY URL output (the common
    // case for non-index articles). This is what `previous_hashes.files`
    // and `new_hashes.files` actually look like in a real rebuild — see
    // `src-tauri/src/build/markdown/pipeline.rs:559-565` ("non-index
    // files get wrapped in a subdirectory: article.md → article/index.html").
    let mut prev = SiteHashes::new();
    prev.insert("blog/old-post/index.html".into(), "hash_x".into());
    let mut current = SiteHashes::new();
    current.insert("blog/new-post/index.html".into(), "hash_y".into());

    let event = build_rebuild_event_with_renames(&current, &prev, &pairs)
        .expect("event emits for a rename");

    // moved_output_paths is in OUTPUT domain (matches what extractPathFromUrl
    // normalizes the iframe URL to: /blog/old-post/ → blog/old-post/index.html).
    let renamed = event
        .moved_output_paths
        .expect("moved_output_paths populated");
    assert_eq!(renamed.len(), 1);
    assert_eq!(
        renamed[0],
        (
            "blog/old-post/index.html".into(),
            "blog/new-post/index.html".into()
        )
    );

    // deleted_paths does NOT include the rename-old output path. The
    // frontend's deletion-priority check would otherwise direct-match
    // and redirect home before reading moved_output_paths.
    let deleted = event.deleted_paths.unwrap_or_default();
    assert!(
        !deleted.iter().any(|p| p == "blog/old-post/index.html"),
        "rename-old output must be suppressed from deleted_paths, got {:?}",
        deleted
    );
}

// ----- previous_hashes_for_diff -----

#[test]
fn previous_hashes_for_diff_loads_from_disk_when_output_exists() {
    // Output dir present + populated hashes.json → loaded entries flow
    // into the rebuild's pre/post diff baseline.
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path();
    std::fs::create_dir_all(folder.join(".moss/build/staging")).unwrap();
    std::fs::create_dir_all(folder.join(".moss/build")).unwrap();
    std::fs::write(
        folder.join(".moss/build/hashes.json"),
        r#"{"files":{"index.html":"100644:deadbeef"}}"#,
    )
    .unwrap();

    let output = folder.join(".moss/build/staging");
    let h = previous_hashes_for_diff(folder.to_str().unwrap(), &output);
    assert_eq!(
        h.files.get("index.html").map(String::as_str),
        Some("100644:deadbeef"),
        "should load entries from on-disk hashes.json when output dir exists"
    );
}

#[test]
fn previous_hashes_for_diff_returns_empty_when_output_missing() {
    // Output dir wiped externally (iCloud / antivirus / manual rm), but
    // hashes.json from the previous build still on disk. Without the
    // invariant, the rebuild's pre/post diff would compare a stale
    // manifest against the regenerated one and conclude "no changes" —
    // browser stays on its 404. The guard substitutes an empty manifest
    // so the post-diff sees all files as new and emits FileChanged.
    let tmp = tempfile::tempdir().unwrap();
    let folder = tmp.path();
    std::fs::create_dir_all(folder.join(".moss/build")).unwrap();
    // hashes.json present, claims files exist…
    std::fs::write(
        folder.join(".moss/build/hashes.json"),
        r#"{"files":{"index.html":"100644:deadbeef"}}"#,
    )
    .unwrap();
    // …but staging/ does not.
    let output = folder.join(".moss/build/staging");
    assert!(!output.exists(), "test setup: staging/ should be missing");

    let h = previous_hashes_for_diff(folder.to_str().unwrap(), &output);
    assert!(
        h.files.is_empty(),
        "should drop on-disk hashes when output is missing, got: {:?}",
        h.files
    );
}

// -----------------------------------------------------------------------
// extract_rename_pairs tests
//
// These tests synthesize the platform-specific events that
// `notify-debouncer-full` emits to validate moss's pair extraction.
// The crate itself stitches `Modify(Name(From))` + `Modify(Name(To))`
// into a single `Modify(Name(Both))` with `paths = [from, to]` (see
// module doc-comment for the platform table); these tests assume that
// contract and pin the moss-side handling. Cross-platform: the synthesized
// `DebouncedEvent` shape is identical on macOS, Linux, and Windows because
// the crate normalizes the stitched event to a single representation.
// -----------------------------------------------------------------------

use notify_debouncer_full::DebouncedEvent;

/// Helper: build a stitched `Modify(Name(Both))` event with two paths.
/// This matches the shape `notify-debouncer-full` emits after pairing
/// `From` + `To` events via the file-id cache (or via a platform-native
/// cookie/tracker when present).
fn synth_rename_both(from: &std::path::Path, to: &std::path::Path) -> DebouncedEvent {
    let event = notify::Event {
        kind: notify::EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::Both,
        )),
        paths: vec![from.to_path_buf(), to.to_path_buf()],
        attrs: Default::default(),
    };
    event.into()
}

/// Helper: build an UNpaired rename event (just `From` or just `To`).
/// Should NOT be extracted as a pair — that's what the debouncer's
/// stitching exists to prevent. If the debouncer fails to stitch (e.g.,
/// file-id cache miss), moss falls back to the existing manifest diff.
fn synth_rename_unpaired(
    path: &std::path::Path,
    mode: notify::event::RenameMode,
) -> DebouncedEvent {
    let event = notify::Event {
        kind: notify::EventKind::Modify(notify::event::ModifyKind::Name(mode)),
        paths: vec![path.to_path_buf()],
        attrs: Default::default(),
    };
    event.into()
}

/// Helper: build a content-change event (not a rename). Used to assert
/// that non-rename events don't accidentally surface as rename pairs.
fn synth_content_change(path: &std::path::Path) -> DebouncedEvent {
    let event = notify::Event {
        kind: notify::EventKind::Modify(notify::event::ModifyKind::Data(
            notify::event::DataChange::Content,
        )),
        paths: vec![path.to_path_buf()],
        attrs: Default::default(),
    };
    event.into()
}

/// The happy path: a single stitched rename event yields exactly one
/// `(from, to)` source-relative pair. The folder prefix is stripped.
///
/// On macOS, `tempfile::tempdir` returns `/var/folders/...` which
/// `canonicalize` resolves to `/private/var/folders/...`. So we
/// canonicalize the folder once and use the canonicalized path
/// throughout — matches the real watcher behavior (the watcher emits
/// already-canonicalized paths on macOS).
#[test]
fn extract_rename_pairs_yields_one_pair_for_stitched_event() {
    let dir = tempfile::tempdir().unwrap();
    let folder = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir(folder.join("posts")).unwrap();
    let from = folder.join("posts/old.md");
    let to = folder.join("posts/new.md");
    std::fs::write(&to, "hello").unwrap();

    let ev = synth_rename_both(&from, &to);
    let events = vec![&ev];
    let pairs = extract_rename_pairs(&events, folder.to_str().unwrap());
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        pairs[0],
        ("posts/old.md".to_string(), "posts/new.md".to_string())
    );
}

/// Notebook (.ipynb) renames go through the same code path; the watcher
/// emits the same `Modify(Name(Both))` shape regardless of extension.
/// (Resolving the pair to output paths is a different concern — handled
/// in `build_rebuild_event_with_renames`; notebooks may drop through if
/// `source_to_output` doesn't carry them yet.)
#[test]
fn extract_rename_pairs_handles_notebook_extension() {
    let dir = tempfile::tempdir().unwrap();
    let folder = std::fs::canonicalize(dir.path()).unwrap();
    let from = folder.join("analysis-old.ipynb");
    let to = folder.join("analysis-new.ipynb");
    std::fs::write(&to, "{}").unwrap();

    let ev = synth_rename_both(&from, &to);
    let events = vec![&ev];
    let pairs = extract_rename_pairs(&events, folder.to_str().unwrap());
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        pairs[0],
        (
            "analysis-old.ipynb".to_string(),
            "analysis-new.ipynb".to_string()
        )
    );
}

/// Asset (.jpg/.png/etc.) renames behave the same as markdown/notebook —
/// the watcher doesn't care about the extension; the pairing is by
/// inode/file-id.
#[test]
fn extract_rename_pairs_handles_asset_extension() {
    let dir = tempfile::tempdir().unwrap();
    let folder = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir(folder.join("images")).unwrap();
    let from = folder.join("images/old.jpg");
    let to = folder.join("images/new.jpg");
    std::fs::write(&to, &[0xff, 0xd8, 0xff]).unwrap(); // valid JPEG header

    let ev = synth_rename_both(&from, &to);
    let events = vec![&ev];
    let pairs = extract_rename_pairs(&events, folder.to_str().unwrap());
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        pairs[0],
        ("images/old.jpg".to_string(), "images/new.jpg".to_string())
    );
}

/// Unpaired `Modify(Name(From))` (debouncer couldn't stitch — file-id
/// cache miss, race with rapid create-then-delete, …) must NOT emit a
/// rename pair. The manifest diff still surfaces the deletion as
/// `deleted_paths` so the iframe redirects home — pre-fix fallback.
#[test]
fn extract_rename_pairs_skips_unstitched_from_event() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let path = folder.join("orphan.md");

    let ev = synth_rename_unpaired(&path, notify::event::RenameMode::From);
    let events = vec![&ev];
    let pairs = extract_rename_pairs(&events, folder.to_str().unwrap());
    assert_eq!(
        pairs.len(),
        0,
        "unpaired From event must not produce a pair"
    );
}

/// Unpaired `Modify(Name(To))` (file moved IN from outside the watched
/// tree) must NOT emit a rename pair — there's no `from` inside the
/// project; it's effectively a Create from moss's perspective.
#[test]
fn extract_rename_pairs_skips_unstitched_to_event() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let path = folder.join("arrived.md");
    std::fs::write(&path, "x").unwrap();

    let ev = synth_rename_unpaired(&path, notify::event::RenameMode::To);
    let events = vec![&ev];
    let pairs = extract_rename_pairs(&events, folder.to_str().unwrap());
    assert_eq!(pairs.len(), 0, "unpaired To event must not produce a pair");
}

/// Non-rename events (content changes, creates, removes) must not
/// accidentally fall into the rename pair extractor.
#[test]
fn extract_rename_pairs_ignores_non_rename_events() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let path = folder.join("post.md");
    std::fs::write(&path, "x").unwrap();

    let ev = synth_content_change(&path);
    let events = vec![&ev];
    let pairs = extract_rename_pairs(&events, folder.to_str().unwrap());
    assert_eq!(pairs.len(), 0, "content change is not a rename pair");
}

/// Multiple stitched renames in one batch each produce a pair, in
/// arrival order. The frontend iterates `moved_output_paths` in order and
/// applies the first match; preserving order keeps the
/// `a→b then c→a` collision case deterministic (see
/// `build_rebuild_event_handles_two_rename_collision_in_one_debounce`).
#[test]
fn extract_rename_pairs_preserves_batch_order() {
    let dir = tempfile::tempdir().unwrap();
    let folder = std::fs::canonicalize(dir.path()).unwrap();
    let from1 = folder.join("a.md");
    let to1 = folder.join("b.md");
    let from2 = folder.join("c.md");
    let to2 = folder.join("d.md");
    std::fs::write(&to1, "x").unwrap();
    std::fs::write(&to2, "x").unwrap();

    let ev1 = synth_rename_both(&from1, &to1);
    let ev2 = synth_rename_both(&from2, &to2);
    let events = vec![&ev1, &ev2];
    let pairs = extract_rename_pairs(&events, folder.to_str().unwrap());
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].0, "a.md");
    assert_eq!(pairs[0].1, "b.md");
    assert_eq!(pairs[1].0, "c.md");
    assert_eq!(pairs[1].1, "d.md");
}

/// A `Modify(Name(Both))` event with the wrong shape (e.g., only one
/// path because of a buggy backend) is skipped, not extracted. This
/// keeps the function defensive without panicking.
#[test]
fn extract_rename_pairs_skips_malformed_both_event() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let path = folder.join("malformed.md");

    // Synthesize a Both event with only one path — the debouncer should
    // never emit this, but we defend against it.
    let event = notify::Event {
        kind: notify::EventKind::Modify(notify::event::ModifyKind::Name(
            notify::event::RenameMode::Both,
        )),
        paths: vec![path.to_path_buf()],
        attrs: Default::default(),
    };
    let ev: DebouncedEvent = event.into();
    let events = vec![&ev];
    let pairs = extract_rename_pairs(&events, folder.to_str().unwrap());
    assert_eq!(pairs.len(), 0, "Both event with len != 2 must be skipped");
}

/// Pins moss's contract on the canonical post-stitching shape that
/// `notify-debouncer-full` emits regardless of source platform:
///   - macOS: FSEvents `From` + `To` pair, matched by file-id cache.
///   - Linux: inotify `IN_MOVED_FROM` + `IN_MOVED_TO` cookie pair.
///   - Windows: `FILE_ACTION_RENAMED_OLD_NAME` + `_NEW_NAME` sequence.
/// All three surface to moss as the same `Modify(Name(Both))`
/// `DebouncedEvent`. Upstream's stitching itself is tested by the
/// crate; this test only pins moss-side extraction.
#[test]
fn extract_rename_pairs_handles_canonical_stitched_event() {
    let dir = tempfile::tempdir().unwrap();
    let folder = std::fs::canonicalize(dir.path()).unwrap();
    let from = folder.join("cross-platform-old.md");
    let to = folder.join("cross-platform-new.md");
    std::fs::write(&to, "x").unwrap();

    let ev = synth_rename_both(&from, &to);
    let events = vec![&ev];
    let pairs = extract_rename_pairs(&events, folder.to_str().unwrap());
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        pairs[0],
        (
            "cross-platform-old.md".to_string(),
            "cross-platform-new.md".to_string()
        )
    );
}

/// End-to-end: a rebuild with one FS rename, one create, and one delete
/// produces a FileChangeEvent whose three source-domain fields each
/// carry exactly the right entries, deduped against the rename.
#[test]
fn build_rebuild_event_populates_source_domain_fields_with_dedup() {
    let pairs = [("blog/foo.md".to_string(), "blog/bar.md".to_string())];

    let mut prev = SiteHashes::new();
    prev.insert("blog/foo/index.html".into(), "hash_foo".into());
    prev.insert("gone-elsewhere/index.html".into(), "hash_gone".into());
    prev.source_to_output
        .insert("blog/foo.md".into(), "blog/foo/index.html".into());
    prev.source_to_output.insert(
        "gone-elsewhere.md".into(),
        "gone-elsewhere/index.html".into(),
    );

    let mut new = SiteHashes::new();
    new.insert("blog/bar/index.html".into(), "hash_bar".into());
    new.insert("brand-new/index.html".into(), "hash_brand".into());
    new.source_to_output
        .insert("blog/bar.md".into(), "blog/bar/index.html".into());
    new.source_to_output
        .insert("brand-new.md".into(), "brand-new/index.html".into());

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs).expect("event emits");

    assert_eq!(
        event.source_renames.as_deref(),
        Some(&[("blog/foo.md".to_string(), "blog/bar.md".to_string())][..]),
        "source_renames echoes the input rename pairs verbatim",
    );
    assert_eq!(
        event.source_creates.as_deref(),
        Some(&["brand-new.md".to_string()][..]),
        "source_creates excludes blog/bar.md (it's the new side of a rename)",
    );
    assert_eq!(
        event.source_deletes.as_deref(),
        Some(&["gone-elsewhere.md".to_string()][..]),
        "source_deletes excludes blog/foo.md (it's the old side of a rename)",
    );
}

/// When there are no source-domain changes (and no FS rename), the three
/// source-domain fields are None — matching the existing convention for
/// output-domain fields (Some(non_empty) or None, never Some(empty)).
#[test]
fn build_rebuild_event_leaves_source_domain_fields_none_when_unchanged() {
    let pairs: [(String, String); 0] = [];

    let mut prev = SiteHashes::new();
    prev.insert("only/index.html".into(), "hash_a".into());
    prev.source_to_output
        .insert("only.md".into(), "only/index.html".into());

    let mut new = SiteHashes::new();
    new.insert("only/index.html".into(), "hash_b".into()); // content changed, URL same
    new.source_to_output
        .insert("only.md".into(), "only/index.html".into());

    let event = build_rebuild_event_with_renames(&new, &prev, &pairs)
        .expect("event emits for content change");

    assert!(event.source_creates.is_none(), "no creates");
    assert!(event.source_deletes.is_none(), "no deletes");
    assert!(event.source_renames.is_none(), "no renames");
}

/// Paths outside the watched folder are skipped (returns None from
/// `path_to_relative_key`). A rename pair where one side escapes the
/// folder cannot be expressed in the source-domain pair format —
/// drop it rather than emit a malformed pair.
#[test]
fn extract_rename_pairs_skips_paths_outside_folder() {
    let dir = tempfile::tempdir().unwrap();
    let folder = std::fs::canonicalize(dir.path()).unwrap();
    let other = tempfile::tempdir().unwrap();
    let from = folder.join("inside.md");
    let to = other.path().join("outside.md");
    std::fs::write(&to, "x").unwrap();

    let ev = synth_rename_both(&from, &to);
    let events = vec![&ev];
    let pairs = extract_rename_pairs(&events, folder.to_str().unwrap());
    assert_eq!(
        pairs.len(),
        0,
        "pair with one side outside folder must be dropped"
    );
}

#[test]
fn windows_backslash_moss_cache_is_not_watchable() {
    // The runaway-rebuild bug: on Windows the watcher receives backslash paths,
    // so the `/.moss/` gate misses and `.moss\cache` (auto-managed) is wrongly
    // treated as watchable — moss's own cache writes then retrigger the build
    // in an infinite loop. After normalization it must be rejected.
    let root = Path::new(r"C:\site");
    assert!(!path_is_watchable(root, Path::new(r"C:\site\.moss\cache\thumb.jpg")));
    assert!(!path_passes_filter(root, Path::new(r"C:\site\.moss\cache\thumb.jpg")));
    // node_modules under a backslash path must also be rejected.
    assert!(!path_is_watchable(root, Path::new(r"C:\site\node_modules\pkg\index.js")));
}

#[test]
fn windows_backslash_user_files_are_watchable() {
    // User-editable .moss files and content survive the normalized gate.
    let root = Path::new(r"C:\site");
    assert!(path_is_watchable(root, Path::new(r"C:\site\.moss\theme\style.css")));
    assert!(path_is_watchable(root, Path::new(r"C:\site\post.md")));
    assert!(path_passes_filter(root, Path::new(r"C:\site\.moss\theme\style.css")));
}

// ── source_image_request_path ────────────────────────────────────────────

/// An image inside the root returns Some with a leading slash.
#[test]
fn source_image_request_path_in_root_image() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let img = root.join("assets").join("photo.jpg");
    std::fs::create_dir_all(img.parent().unwrap()).unwrap();
    std::fs::write(&img, b"fake").unwrap();

    let result = source_image_request_path(root, &img);
    assert_eq!(result, Some("/assets/photo.jpg".to_string()));
}

/// A non-image file returns None.
#[test]
fn source_image_request_path_non_image_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let md = root.join("post.md");
    std::fs::write(&md, b"content").unwrap();

    let result = source_image_request_path(root, &md);
    assert!(result.is_none(), "markdown is not an image");
}

/// A file outside the root returns None.
#[test]
fn source_image_request_path_outside_root_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let other_dir = tempfile::tempdir().unwrap();
    let img = other_dir.path().join("photo.png");
    std::fs::write(&img, b"fake").unwrap();

    let result = source_image_request_path(root, &img);
    assert!(result.is_none(), "file outside root must return None");
}

/// A deleted/missing file (no canonicalize possible) falls back to lexical
/// strip and still returns Some for an image extension.
#[test]
fn source_image_request_path_deleted_file_lexical_fallback() {
    let dir = tempfile::tempdir().unwrap();
    // Canonicalize the root so lexical comparison works.
    let root_canon = std::fs::canonicalize(dir.path()).unwrap();
    let deleted_img = root_canon.join("图片").join("摄影").join("X.jpeg");
    // Do NOT write the file — it doesn't exist (simulates a delete event).

    let result = source_image_request_path(&root_canon, &deleted_img);
    assert_eq!(result, Some("/图片/摄影/X.jpeg".to_string()));
}

/// Unicode path in the root produces the correct request path.
#[test]
fn source_image_request_path_unicode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sub = root.join("图片").join("摄影");
    std::fs::create_dir_all(&sub).unwrap();
    let img = sub.join("X.jpeg");
    std::fs::write(&img, b"fake").unwrap();

    let result = source_image_request_path(root, &img);
    assert_eq!(result, Some("/图片/摄影/X.jpeg".to_string()));
}

/// SVG is an image extension.
#[test]
fn source_image_request_path_svg() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let img = root.join("logo.svg");
    std::fs::write(&img, b"<svg/>").unwrap();

    let result = source_image_request_path(root, &img);
    assert_eq!(result, Some("/logo.svg".to_string()));
}

/// CSS is NOT an image extension — returns None.
#[test]
fn source_image_request_path_css_not_image() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let css = root.join("style.css");
    std::fs::write(&css, b"body{}").unwrap();

    let result = source_image_request_path(root, &css);
    assert!(result.is_none(), "CSS is not an image");
}

/// Build outputs under `.moss/` must never be emitted as SourceAssetChanged.
/// The watcher must exclude any path component that matches `is_excluded_dir_name`.
#[test]
fn source_image_request_path_excludes_moss_build_outputs() {
    let root = tempfile::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let p = root.path();
    std::fs::create_dir_all(p.join(".moss/build/staging/assets")).unwrap();
    std::fs::write(p.join(".moss/build/staging/assets/x.webp"), b"o").unwrap();
    std::fs::create_dir_all(p.join("图片")).unwrap();
    std::fs::write(p.join("图片/x.webp"), b"o").unwrap();
    // build output is excluded
    assert_eq!(
        source_image_request_path(p, &p.join(".moss/build/staging/assets/x.webp")),
        None
    );
    // real source still works
    assert_eq!(
        source_image_request_path(p, &p.join("图片/x.webp")),
        Some("/图片/x.webp".to_string())
    );
}

// ── source_asset_request_paths ────────────────────────────────────────────
//
// Reproduces the stale-editor-resolution bug (2026-08-14 folded-in bug): an
// external Finder move of a FOLDER of assets, or of a non-image asset,
// produced NO SourceAssetChanged (the old pass required an image extension on
// the event path), so the editor's reference resolver never revalidated when
// the follow-up rebuild's FileChanged was gated out or diff-suppressed.

/// A stitched folder rename (`Modify(Name(Both))`, paths = [from, to]) must
/// notify the editor for BOTH sides — this event's paths are directories, so
/// no per-child image-extension event will ever arrive.
#[test]
fn source_asset_request_paths_folder_rename_notifies_both_sides() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    // Only the to-side exists after the move (as in a real rename).
    std::fs::create_dir_all(root.join("pics")).unwrap();
    let kind = EventKind::Modify(ModifyKind::Name(RenameMode::Both));
    let paths = vec![root.join("img"), root.join("pics")];

    let out = source_asset_request_paths(&root, kind, &paths);
    assert_eq!(out, vec!["/img".to_string(), "/pics".to_string()]);
}

/// A non-image asset move (pdf) is asset-affecting: structural events emit.
#[test]
fn source_asset_request_paths_pdf_rename_emits() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/paper.pdf"), b"p").unwrap();
    let kind = EventKind::Modify(ModifyKind::Name(RenameMode::Both));
    let paths = vec![root.join("paper.pdf"), root.join("docs/paper.pdf")];

    let out = source_asset_request_paths(&root, kind, &paths);
    assert_eq!(out, vec!["/paper.pdf".to_string(), "/docs/paper.pdf".to_string()]);
}

/// An in-place content edit of a non-image asset does NOT emit (resolution is
/// path-keyed; only structural changes move the answer) — but an image content
/// edit still does (the editor busts `?_t=` to repaint fresh bytes).
#[test]
fn source_asset_request_paths_content_modify_images_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("paper.pdf"), b"p").unwrap();
    std::fs::write(root.join("photo.png"), b"i").unwrap();
    let kind = EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Content));

    let pdf = source_asset_request_paths(&root, kind, &[root.join("paper.pdf")]);
    assert!(pdf.is_empty(), "pdf content edit must not emit");
    let png = source_asset_request_paths(&root, kind, &[root.join("photo.png")]);
    assert_eq!(png, vec!["/photo.png".to_string()]);
}

/// Markdown never emits here — page saves/renames are the rebuild's job, and
/// emitting would re-resolve the editor cache on every save.
#[test]
fn source_asset_request_paths_markdown_never_emits() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("post.md"), b"x").unwrap();
    for kind in [
        EventKind::Create(notify::event::CreateKind::File),
        EventKind::Remove(notify::event::RemoveKind::File),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
        EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Content)),
    ] {
        let out = source_asset_request_paths(&root, kind, &[root.join("post.md")]);
        assert!(out.is_empty(), "markdown must not emit for {kind:?}");
    }
}

/// `.moss` build outputs stay excluded on the generalized pass too.
#[test]
fn source_asset_request_paths_excludes_moss_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(root.join(".moss/build/staging/assets")).unwrap();
    let kind = EventKind::Create(notify::event::CreateKind::Folder);
    let out = source_asset_request_paths(&root, kind, &[root.join(".moss/build/staging/assets")]);
    assert!(out.is_empty(), ".moss build outputs must never notify the editor");
}

/// A directory named with an unregistered "extension" (e.g. `2026.photos`)
/// still emits while it exists on disk (create / rename-to side).
#[test]
fn source_asset_request_paths_dotted_folder_probes_is_dir() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(root.join("2026.photos")).unwrap();
    let kind = EventKind::Create(notify::event::CreateKind::Folder);
    let out = source_asset_request_paths(&root, kind, &[root.join("2026.photos")]);
    assert_eq!(out, vec!["/2026.photos".to_string()]);
}

// ── collect_raw_create_keys ──────────────────────────────────────────────

/// Helper: build a Create event with a single path.
fn synth_create(path: &std::path::Path, kind: notify::event::CreateKind) -> DebouncedEvent {
    use notify_debouncer_full::DebouncedEvent;
    let event = notify::Event {
        kind: notify::EventKind::Create(kind),
        paths: vec![path.to_path_buf()],
        attrs: Default::default(),
    };
    event.into()
}

/// A Create(File) event for a real .md file under root → key returned, no leading slash.
#[test]
fn raw_create_key_md_file_returns_relative_key() {
    // Use a non-hidden prefix: macOS tempfile::tempdir() creates `.tmpXXXX` dirs
    // which path_is_watchable correctly rejects (dotfile rule). Builder::prefix
    // avoids the leading dot so the md file passes the watchable gate.
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let subdir = root.join("posts");
    std::fs::create_dir_all(&subdir).unwrap();
    let md = subdir.join("hello.md");
    std::fs::write(&md, b"content").unwrap();

    let ev = synth_create(&md, notify::event::CreateKind::File);
    let keys = collect_raw_create_keys(&[ev], root, |_| false);
    assert_eq!(keys, vec!["posts/hello.md".to_string()]);
}

/// Dir create with is_dir=true → excluded; Create(Any) where is_dir returns true → excluded.
#[test]
fn raw_create_key_dir_creates_excluded() {
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let subdir = root.join("images");
    std::fs::create_dir_all(&subdir).unwrap();

    // Folder create (CreateKind::Folder) with is_dir=true → excluded
    let ev_folder = synth_create(&subdir, notify::event::CreateKind::Folder);
    let keys = collect_raw_create_keys(&[ev_folder], root, |_| true);
    assert!(keys.is_empty(), "dir create must be excluded");

    // Create(Any) where is_dir returns true → also excluded
    let ev_any = synth_create(&subdir, notify::event::CreateKind::Any);
    let keys2 = collect_raw_create_keys(&[ev_any], root, |_| true);
    assert!(keys2.is_empty(), "Create(Any) for a dir must be excluded");
}

/// Path under .moss/data/social/comment.json → excluded via is_excluded_dir_name(".moss").
/// Uses the Builder prefix for consistency with sibling tests. The hidden `.tmp` root
/// wouldn't matter here anyway — the is_excluded_dir_name component loop is the guard,
/// not the dotfile rejection.
#[test]
fn raw_create_key_moss_dir_excluded() {
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let moss_data = root.join(".moss").join("data").join("social");
    std::fs::create_dir_all(&moss_data).unwrap();
    let comment = moss_data.join("comment.json");
    std::fs::write(&comment, b"{}").unwrap();

    let ev = synth_create(&comment, notify::event::CreateKind::File);
    let keys = collect_raw_create_keys(&[ev], root, |_| false);
    assert!(keys.is_empty(), ".moss subpath must be excluded");
}

/// node_modules path → excluded.
///
/// Uses a non-hidden temp prefix (`watch-test-`) so the root itself passes
/// `path_is_watchable`'s dotfile rule. Without this, the hidden `.tmpXXXX`
/// root would be rejected by the dotfile guard BEFORE the `node_modules`
/// component loop runs — meaning the test would still pass even if
/// node_modules exclusion were accidentally deleted.
#[test]
fn raw_create_key_node_modules_excluded() {
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let nm = root.join("node_modules").join("pkg");
    std::fs::create_dir_all(&nm).unwrap();
    let f = nm.join("index.js");
    std::fs::write(&f, b"x").unwrap();

    let ev = synth_create(&f, notify::event::CreateKind::File);
    let keys = collect_raw_create_keys(&[ev], root, |_| false);
    assert!(keys.is_empty(), "node_modules must be excluded");
}

/// A file under `.moss/theme/` is watchable by `path_is_watchable` (user-
/// editable), but the `is_hidden` component loop in `raw_create_key` sees
/// the `.moss` component and returns None — so it is correctly excluded from
/// `RawFileCreated` payloads. Pins that ALL `.moss/` subdirs are excluded
/// from the raw-create key, not just `data/social`.
///
/// Uses a non-hidden prefix so the dotfile guard in `path_is_watchable`
/// doesn't reject the root and accidentally mask a missing `.moss/` guard.
#[test]
fn raw_create_key_moss_theme_excluded() {
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let theme_dir = root.join(".moss").join("theme");
    std::fs::create_dir_all(&theme_dir).unwrap();
    let css = theme_dir.join("style.css");
    std::fs::write(&css, b"body{}").unwrap();

    let ev = synth_create(&css, notify::event::CreateKind::File);
    let keys = collect_raw_create_keys(&[ev], root, |_| false);
    assert!(
            keys.is_empty(),
            ".moss/theme/style.css must be excluded from RawFileCreated (is_hidden loop guards .moss/)"
        );
}

/// A root `AGENTS.md` is hidden from the file tree, so announcing its creation
/// promises a row that does not exist. This is not hypothetical: when moss
/// wrote this file itself (agent-file sync), the watcher saw a create for the
/// very file the tree refuses to show, and the editor's only consumer went
/// looking for a nav row to flash and found none (#955). The same file can
/// still appear today whenever an author or another agent creates it.
///
/// The guard is `is_hidden` — the predicate `list_tree` filters with — so the
/// two cannot drift apart again.
#[test]
fn raw_create_key_root_agent_config_excluded() {
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();

    for name in ["AGENTS.md", "CLAUDE.md", "GEMINI.md"] {
        let f = root.join(name);
        std::fs::write(&f, b"# instructions").unwrap();
        let ev = synth_create(&f, notify::event::CreateKind::File);
        let keys = collect_raw_create_keys(&[ev], root, |_| false);
        assert!(
            keys.is_empty(),
            "root {name} is hidden by list_tree and must not reach RawFileCreated, got: {keys:?}"
        );
    }
}

/// The root-only half of the rule above. `posts/AGENTS.md` is an ordinary
/// article that the tree DOES show and the build DOES publish — only the
/// source root is a location tooling claims (see `skip_root_agent_config`).
/// Suppressing it too would trade one lie for a worse one: a real file the
/// author created, silently never flashed.
#[test]
fn raw_create_key_nested_agent_config_still_announced() {
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let posts = root.join("posts");
    std::fs::create_dir_all(&posts).unwrap();
    let f = posts.join("AGENTS.md");
    std::fs::write(&f, b"# an essay about agents").unwrap();

    let ev = synth_create(&f, notify::event::CreateKind::File);
    let keys = collect_raw_create_keys(&[ev], root, |_| false);
    assert_eq!(
        keys,
        vec!["posts/AGENTS.md".to_string()],
        "a non-root agent-named file is an ordinary article and must still be announced"
    );
}

/// A dotfile (hidden file) under a non-hidden root is excluded by
/// `path_is_watchable`'s dotfile-component rule, so `collect_raw_create_keys`
/// returns empty. This confirms the dotfile guard is not limited to directory
/// names — any path COMPONENT starting with `.` (outside `.moss/`) triggers it.
///
/// Uses a non-hidden root prefix so the root itself passes the watchable gate
/// and the dotfile guard is exercised on the file's own component.
#[test]
fn raw_create_key_dotfile_excluded() {
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let posts = root.join("posts");
    std::fs::create_dir_all(&posts).unwrap();
    let secret = posts.join(".secret.md");
    std::fs::write(&secret, b"hidden").unwrap();

    let ev = synth_create(&secret, notify::event::CreateKind::File);
    let keys = collect_raw_create_keys(&[ev], root, |_| false);
    assert!(
        keys.is_empty(),
        "dotfile posts/.secret.md must be excluded by path_is_watchable's dotfile-component rule"
    );
}

/// Modify and Remove events → no key returned.
#[test]
fn raw_create_key_only_create_events_included() {
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let f = root.join("post.md");
    std::fs::write(&f, b"x").unwrap();

    // Modify event
    let ev_modify = {
        use notify_debouncer_full::DebouncedEvent;
        let event = notify::Event {
            kind: notify::EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Content,
            )),
            paths: vec![f.clone()],
            attrs: Default::default(),
        };
        event.into()
    };
    // Remove event
    let ev_remove = {
        use notify_debouncer_full::DebouncedEvent;
        let event = notify::Event {
            kind: notify::EventKind::Remove(notify::event::RemoveKind::File),
            paths: vec![f.clone()],
            attrs: Default::default(),
        };
        event.into()
    };

    let keys = collect_raw_create_keys(&[ev_modify, ev_remove], root, |_| false);
    assert!(keys.is_empty(), "Modify/Remove events must yield no key");
}

/// Duplicates within a batch → deduped to one entry.
#[test]
fn raw_create_key_deduplicates_within_batch() {
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let f = root.join("article.md");
    std::fs::write(&f, b"x").unwrap();

    let ev1 = synth_create(&f, notify::event::CreateKind::File);
    let ev2 = synth_create(&f, notify::event::CreateKind::Any);
    let keys = collect_raw_create_keys(&[ev1, ev2], root, |_| false);
    assert_eq!(keys.len(), 1, "duplicate path in batch must be deduped");
    assert_eq!(keys[0], "article.md");
}

/// All-folder/empty batch → empty Vec.
#[test]
fn raw_create_key_all_folders_returns_empty() {
    let dir = tempfile::Builder::new()
        .prefix("watch-test-")
        .tempdir()
        .unwrap();
    let root = dir.path();
    let subdir = root.join("new-section");
    std::fs::create_dir_all(&subdir).unwrap();

    let ev = synth_create(&subdir, notify::event::CreateKind::Folder);
    let keys = collect_raw_create_keys(&[ev], root, |_| true);
    assert!(keys.is_empty(), "all-folder batch must return empty Vec");
}
