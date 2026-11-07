use super::*;
use std::sync::Arc;
use std::time::Duration;

use crate::build::cloud_prefetch::Prefetcher;
use crate::build::cloud_readiness::fetch::Wait;
use crate::build::icloud::{self, is_still_in_the_cloud};

/// A provider that delivers nothing, reported after a window short enough for a test.
fn stalled() -> Wait {
    let pool = Prefetcher::with_materializer(2, Arc::new(|_| Err(std::io::Error::from_raw_os_error(libc::ETIMEDOUT))), false);
    Wait::with(pool, is_still_in_the_cloud, Duration::from_millis(200), Duration::from_millis(10))
}

/// A provider that delivers each file once it is asked for.
fn delivering() -> Wait {
    let pool = Prefetcher::with_materializer(2, Arc::new(|path| {
        icloud::pretend::requested(path);
        Ok(())
    }), false);
    Wait::with(pool, is_still_in_the_cloud, Duration::from_secs(5), Duration::from_millis(10))
}

fn on_disk(dir: &Path) -> Result<SourceTree, String> {
    SourceTree::on_disk_with(dir, &stalled, "run `moss history --save` again", &mut |_| {})
}

#[test]
fn a_folder_never_built_is_read_and_hashed_from_disk() {
    let dir = tempfile::Builder::new().prefix("moss-tree-test-").tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), b"hello").unwrap();

    let tree = on_disk(dir.path()).unwrap();

    assert_eq!(tree.hashes().get("a.md"), Some(&format!("{:x}", Sha256::digest(b"hello"))));
    assert!(tree.generation_id().starts_with("source-"));
}

fn built_fixture() -> (tempfile::TempDir, HashMap<String, String>) {
    use crate::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use crate::ops::watch::sweep::walk::read_baseline_from_disk;

    let dir = tempfile::Builder::new().prefix("moss-tree-test-").tempdir().unwrap();
    let folder = dir.path();
    for subdir in ["posts", "images", ".moss/theme"] {
        std::fs::create_dir_all(folder.join(subdir)).unwrap();
    }
    for (path, bytes) in [
        ("index.md", "# Home\n"),
        ("about.md", "# About\n"),
        ("footer.md", "Made with moss.\n"),
        ("posts/one.md", "# One\n"),
        ("notes.pdf", "fixture document"),
        ("lab.ipynb", "{\"cells\": [], \"metadata\": {}, \"nbformat\": 4, \"nbformat_minor\": 5}"),
        ("data.json", "{}"),
        ("app.wasm", "wasm"),
        ("style.css", "body { margin: 0 }\n"),
        (".moss/theme/style.css", "@font-face { font-family: test; src: url(font.woff2) }\n"),
        (".moss/theme/font.woff2", "fixture font"),
    ] {
        std::fs::write(folder.join(path), bytes).unwrap();
        // Older than the build, as a real site's files are: a fresh mtime falls in
        // the racy window, where the stat tier always re-hashes and hides a stale entry.
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(folder.join(path)).unwrap().set_modified(long_ago).unwrap();
    }
    image::RgbImage::from_pixel(2, 2, image::Rgb([30, 120, 200]))
        .save(folder.join("images/photo.png")).unwrap();

    let config = PipelineConfig {
        root: crate::vault::paths::VaultRoot::resolve(folder),
        progress: crate::build::null_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: crate::build::ports::host::test_host_ports(),
        trigger: BuildTrigger::Full,
        // Wait for the seal tail to write hashes.json before reading it.
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    };
    crate::infra::home::with_moss_home(|home| {
        std::fs::write(home.join("assets"), "").unwrap();
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
            .block_on(run_pipeline(config)).expect("build");
    });

    let baseline = read_baseline_from_disk(folder)
        .expect("a completed build writes a readable hashes.json");
    let hashes: HashMap<_, _> = baseline.sources.into_iter()
        .map(|(path, meta)| (path, meta.hash)).collect();
    let mut paths: Vec<_> = hashes.keys().map(String::as_str).collect();
    paths.sort();
    assert_eq!(paths, [".moss/theme/font.woff2", ".moss/theme/style.css", "about.md", "app.wasm", "data.json", "footer.md", "images/photo.png", "index.md", "lab.ipynb", "notes.pdf", "posts/one.md"]);
    (dir, hashes)
}

#[test]
fn a_disk_tree_matches_sealed_sources_except_raw_synchronous_config() {
    let (dir, mut baseline) = built_fixture();
    // Builds hash rewritten CSS for reloads; history restores the author's raw bytes.
    for key in crate::build::manifest::SYNCHRONOUS_CONFIG_SOURCE_KEYS {
        if let Ok(bytes) = std::fs::read(dir.path().join(key)) {
            baseline.insert((*key).into(), format!("{:x}", Sha256::digest(bytes)));
        } else {
            baseline.remove(*key);
        }
    }

    assert_eq!(on_disk(dir.path()).unwrap().hashes(), baseline);
}

#[test]
fn a_disk_tree_tracks_edits_deletions_and_additions_without_a_rebuild() {
    let (dir, baseline) = built_fixture();
    let edited = b"# About\n\nEdited after the build.\n";
    let added = b"# Two\n";
    std::fs::write(dir.path().join("about.md"), edited).unwrap();
    std::fs::remove_file(dir.path().join("posts/one.md")).unwrap();
    std::fs::write(dir.path().join("posts/two.md"), added).unwrap();
    let theme = b"body { color: #333 }\n";
    std::fs::write(dir.path().join(".moss/theme/style.css"), theme).unwrap();

    let hashes = on_disk(dir.path()).unwrap().hashes();

    assert_eq!(hashes.get("about.md"), Some(&format!("{:x}", Sha256::digest(edited))));
    assert!(!hashes.contains_key("posts/one.md"), "deleted source must be absent");
    assert_eq!(hashes.get("posts/two.md"), Some(&format!("{:x}", Sha256::digest(added))));
    assert_eq!(hashes.get(".moss/theme/style.css"), Some(&format!("{:x}", Sha256::digest(theme))), "a source the sweep does not walk is re-judged too");
    let mut expected = baseline;
    expected.insert("about.md".into(), format!("{:x}", Sha256::digest(edited)));
    expected.remove("posts/one.md");
    expected.insert("posts/two.md".into(), format!("{:x}", Sha256::digest(added)));
    expected.insert(".moss/theme/style.css".into(), format!("{:x}", Sha256::digest(theme)));
    assert_eq!(hashes, expected, "every other source keeps its sealed hash");
}

#[test]
fn generation_id_is_deterministic_and_changes_with_a_source_hash() {
    let entries: Vec<_> = (0..32).map(|n| {
        (format!("posts/{n}.md"), SourceMetadata {
            hash: format!("{:x}", Sha256::digest(format!("# Post {n}\n").as_bytes())),
            ..Default::default()
        })
    }).collect();
    let sources: HashMap<_, _> = entries.iter().cloned().collect();
    let tree = SourceTree::from_sources(sources.clone());
    // Independent maps and reversed insertion order exercise canonical ordering.
    for _ in 0..8 {
        let equal = SourceTree::from_sources(entries.iter().rev().cloned().collect());
        assert_eq!(tree.generation_id(), equal.generation_id());
    }

    let mut changed = sources;
    changed.get_mut("posts/0.md").unwrap().hash = format!("{:x}", Sha256::digest(b"edited"));
    assert_ne!(tree.generation_id(), SourceTree::from_sources(changed).generation_id());
}

#[test]
fn a_disk_tree_hashes_raw_theme_bytes_after_a_build_rewrites_them() {
    let (dir, baseline) = built_fixture();
    let key = crate::build::manifest::USER_CSS_SOURCE_KEY;
    let raw = std::fs::read(dir.path().join(key)).unwrap();
    let raw_hash = format!("{:x}", Sha256::digest(&raw));
    assert_ne!(baseline.get(key), Some(&raw_hash), "fixture must exercise CSS rewriting");
    assert_eq!(on_disk(dir.path()).unwrap().hashes().get(key), Some(&raw_hash));

    std::fs::remove_file(dir.path().join(key)).unwrap();
    assert!(!on_disk(dir.path()).unwrap().hashes().contains_key(key));
}

#[test]
fn a_never_built_disk_tree_includes_synchronous_config_files() {
    let dir = tempfile::tempdir().unwrap();
    let keys = crate::build::manifest::SYNCHRONOUS_CONFIG_SOURCE_KEYS;
    for key in keys {
        let path = dir.path().join(key);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"raw config bytes").unwrap();
    }
    let hashes = on_disk(dir.path()).unwrap().hashes();
    for key in keys {
        assert_eq!(hashes.get(*key), Some(&format!("{:x}", Sha256::digest(b"raw config bytes"))));
    }
}

#[test]
fn a_disk_tree_holds_theme_files_added_or_deleted_since_the_build() {
    let (dir, _) = built_fixture();
    std::fs::write(dir.path().join(".moss/theme/added.woff2"), b"new font").unwrap();
    std::fs::remove_file(dir.path().join(".moss/theme/font.woff2")).unwrap();

    let hashes = on_disk(dir.path()).unwrap().hashes();

    assert_eq!(hashes.get(".moss/theme/added.woff2"), Some(&format!("{:x}", Sha256::digest(b"new font"))));
    assert!(!hashes.contains_key(".moss/theme/font.woff2"), "a deleted theme file must be absent");
}

// The real `.name.icloud` stub beside a missing file, which the test below
// pretends instead of making.
#[cfg(target_os = "macos")]
#[test]
fn a_config_file_evicted_to_the_cloud_keeps_its_recorded_hash() {
    let (dir, baseline) = built_fixture();
    let key = crate::build::manifest::USER_CSS_SOURCE_KEY;
    std::fs::rename(dir.path().join(key), dir.path().join(".moss/theme/.style.css.icloud")).unwrap();

    let hashes = on_disk(dir.path()).unwrap().hashes();

    assert_eq!(hashes.get(key), baseline.get(key), "offline is not deleted");
}

#[test]
fn a_config_file_in_the_cloud_keeps_its_recorded_hash_unread() {
    let (dir, baseline) = built_fixture();
    let key = crate::build::manifest::USER_CSS_SOURCE_KEY;
    let _in_cloud = icloud::pretend::evicted(&dir.path().join(key));

    let mut said = Vec::new();
    let hashes = SourceTree::on_disk_with(dir.path(), &stalled, "run `moss history --save` again", &mut |line| said.push(line.to_string())).unwrap().hashes();

    // Read, it would hash to its raw bytes, which the build recorded rewritten.
    assert_eq!(hashes.get(key), baseline.get(key));
    assert_eq!(said, [format!("kept the last built version of {key}: it is still in the cloud")]);
}

#[test]
fn a_built_page_in_the_cloud_keeps_its_recorded_hash_and_is_named() {
    let (dir, baseline) = built_fixture();
    let _in_cloud = icloud::pretend::evicted(&dir.path().join("about.md"));

    let mut said = Vec::new();
    let hashes = SourceTree::on_disk_with(dir.path(), &stalled, "run `moss history --save` again", &mut |line| said.push(line.to_string())).unwrap().hashes();

    assert_eq!(hashes.get("about.md"), baseline.get("about.md"));
    assert!(said.contains(&"kept the last built version of about.md: it is still in the cloud".to_string()), "{said:?}");
}

// A never-built folder has no recorded hash to fall back on, so a file the
// tree cannot read must stop it: left out, it would be recorded as deleted.
#[cfg(unix)]
#[test]
fn a_never_built_file_that_cannot_be_read_refuses_the_tree_and_names_it() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new().prefix("moss-tree-test-").tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), b"hello").unwrap();
    let locked = dir.path().join("b.md");
    std::fs::write(&locked, b"private").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let result = on_disk(dir.path());
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();

    let err = result.unwrap_err();
    assert!(err.contains("b.md"), "{err}");
}

#[test]
fn a_never_built_page_in_the_cloud_is_downloaded_before_it_is_hashed() {
    let dir = tempfile::Builder::new().prefix("moss-tree-test-").tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), b"hello").unwrap();
    let page = dir.path().join("b.md");
    std::fs::write(&page, b"arrived").unwrap();
    let _in_cloud = icloud::pretend::evicted_until_requested(&page);

    let tree = SourceTree::on_disk_with(dir.path(), &delivering, "run `moss history --save` again", &mut |_| {}).unwrap();

    assert_eq!(icloud::pretend::requests_for(&page), 1);
    assert_eq!(tree.hashes().get("b.md"), Some(&format!("{:x}", Sha256::digest(b"arrived"))));
}

#[test]
fn a_never_built_page_the_provider_does_not_deliver_refuses_the_tree_and_names_it() {
    let dir = tempfile::Builder::new().prefix("moss-tree-test-").tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), b"hello").unwrap();
    std::fs::write(dir.path().join("b.md"), b"in the cloud").unwrap();
    let _in_cloud = icloud::pretend::evicted(&dir.path().join("b.md"));

    let err = on_disk(dir.path()).unwrap_err();

    assert!(err.contains("is not delivering 1 of 1 cloud-only files"), "{err}");
    assert!(err.contains("Still in the cloud: b.md."), "{err}");
    assert!(err.contains("run `moss history --save` again"), "{err}");
}

#[cfg(target_os = "macos")]
#[test]
fn a_never_built_theme_file_still_in_icloud_refuses_the_tree() {
    let dir = tempfile::Builder::new().prefix("moss-tree-test-").tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".moss/theme")).unwrap();
    std::fs::write(dir.path().join("a.md"), b"hello").unwrap();
    std::fs::write(dir.path().join(".moss/theme/.font.woff2.icloud"), b"stub").unwrap();

    let err = on_disk(dir.path()).unwrap_err();

    assert!(err.contains("Still in the cloud: .moss/theme/font.woff2."), "{err}");
}
