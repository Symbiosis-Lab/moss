use super::*;
use std::io::Read;

fn source_site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".moss")).unwrap();
    std::fs::write(dir.path().join(".moss/config.toml"),
        "schema_version = 6\n[site]\nlang = \"en\"\nsearch = true\n[build]\npassthrough = [\"index.html\", \"guide/index.html\"]\n").unwrap();
    std::fs::write(dir.path().join("index.md"), "---\ntitle: Home\n---\nShadowed home prose.\n").unwrap();
    write_page(dir.path(), "index.html", "First homepage cobalt");
    write_page(dir.path(), "guide/index.html", "Old guide amber");
    dir
}

fn write_page(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("<!doctype html><html lang=\"en\"><head><title>Authored page</title></head><body><main>{text}</main></body></html>")).unwrap();
}

async fn deploy_build(root: &Path) -> SealedManifest {
    // Both hosted and plugin deploys hand this exact seal to their uploader.
    let root = crate::vault::paths::VaultRoot::resolve(root);
    require_sealed(build_and_seal(&root, crate::cli::host::cli_host_ports(root.as_str()),
        crate::build::PluginMode::Skip, false).await.unwrap()).unwrap()
}

fn fragments(root: &Path, sealed: &SealedManifest) -> Vec<serde_json::Value> {
    let generation = crate::moss_paths::MossPaths::new(root).generation_dir(sealed.generation_id());
    sealed.files().keys().filter(|path| path.ends_with(".pf_fragment")).map(|path| {
        let bytes = std::fs::read(generation.join(path)).unwrap();
        let mut decoded = String::new();
        flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut decoded).unwrap();
        // Pagefind prefixes the gzipped JSON with its file signature.
        serde_json::from_str(&decoded[decoded.find('{').unwrap()..]).unwrap()
    }).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deploy_search_contains_current_authored_html_after_an_edit() {
    let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _indexes = crate::build::feeds::search::lock_index_counter();
    let dir = source_site();
    let _session = crate::system::folder_session::register_session(&dir.path().to_string_lossy());
    deploy_build(dir.path()).await;
    // Seed a receipt from complete staging so the edit must replace an existing bundle.
    let old = deploy_build(dir.path()).await;
    assert!(fragments(dir.path(), &old).iter().any(|f| f["content"].as_str().unwrap().contains("Old guide amber")));
    write_page(dir.path(), "guide/index.html", "Current guide ultramarine");
    let sealed = deploy_build(dir.path()).await;
    let generation = crate::moss_paths::MossPaths::new(dir.path()).generation_dir(sealed.generation_id());
    assert!(std::fs::read_to_string(generation.join("guide/index.html")).unwrap().contains("Current guide ultramarine"));
    let found = fragments(dir.path(), &sealed);
    assert!(found.iter().any(|f| f["url"] == "/guide/" && f["content"].as_str().unwrap().contains("Current guide ultramarine")), "deploy search is older than its published page: {found:?}");
    assert!(!found.iter().any(|f| f["content"].as_str().unwrap().contains("Old guide amber")), "stale text shipped: {found:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_deploy_search_contains_the_authored_homepage() {
    let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _indexes = crate::build::feeds::search::lock_index_counter();
    let dir = source_site();
    let _session = crate::system::folder_session::register_session(&dir.path().to_string_lossy());
    let sealed = deploy_build(dir.path()).await;
    let found = fragments(dir.path(), &sealed);
    assert!(found.iter().any(|f| f["url"] == "/" && f["content"].as_str().unwrap().contains("First homepage cobalt")), "first deploy is missing its authored homepage: {found:?}");
    assert!(!found.iter().any(|f| f["content"].as_str().unwrap().contains("Shadowed home prose")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deploy_search_removes_deleted_authored_html_on_the_second_build() {
    let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _indexes = crate::build::feeds::search::lock_index_counter();
    let dir = source_site();
    let _session = crate::system::folder_session::register_session(&dir.path().to_string_lossy());
    write_page(dir.path(), "guide.html", "Standalone guide vermilion");
    let first = deploy_build(dir.path()).await;
    assert!(fragments(dir.path(), &first).iter().any(|f| f["url"] == "/guide/"));
    assert!(fragments(dir.path(), &first).iter().any(|f| f["content"].as_str().unwrap().contains("Standalone guide vermilion")));
    std::fs::remove_file(dir.path().join("guide/index.html")).unwrap();
    std::fs::remove_file(dir.path().join("guide.html")).unwrap();
    let second = deploy_build(dir.path()).await;
    let paths = crate::moss_paths::MossPaths::new(dir.path());
    for key in ["guide/index.html", "guide.html"] {
        assert!(!paths.staging_dir().join(key).exists(), "deleted page remains staged: {key}");
        assert!(!paths.generation_dir(second.generation_id()).join(key).exists());
        assert!(!second.files().contains_key(key));
    }
    let found = fragments(dir.path(), &second);
    assert!(!found.iter().any(|f| f["url"] == "/guide/" || {
        let text = f["content"].as_str().unwrap();
        text.contains("Old guide amber") || text.contains("Standalone guide vermilion")
    }), "deleted page shipped in search: {found:?}");
    assert!(found.iter().any(|f| f["url"] == "/"));
    assert_eq!(output_unchanged(dir.path()), 0);
}

fn output_unchanged(root: &Path) -> u64 {
    let profile = std::fs::read_to_string(crate::moss_paths::MossPaths::new(root).profile()).unwrap();
    let record: serde_json::Value = profile.lines().rev()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| record["kind"] == "build").unwrap();
    record["counts"]["output_unchanged"].as_u64().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_authored_html_only_edit_reports_output_changes() {
    let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _indexes = crate::build::feeds::search::lock_index_counter();
    let dir = source_site();
    let _session = crate::system::folder_session::register_session(&dir.path().to_string_lossy());
    deploy_build(dir.path()).await;
    deploy_build(dir.path()).await;
    assert_eq!(output_unchanged(dir.path()), 1, "an unchanged build establishes the comparison baseline");
    write_page(dir.path(), "guide/index.html", "Current guide ultramarine");
    deploy_build(dir.path()).await;
    assert_eq!(output_unchanged(dir.path()), 0, "the staged HTML hash must reach the output comparison");
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unchanged_rebuild_preserves_evicted_theme_index() {
    let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _indexes = crate::build::feeds::search::lock_index_counter();
    let dir = source_site();
    let _session = crate::system::folder_session::register_session(&dir.path().to_string_lossy());
    write_page(dir.path(), ".moss/theme/nested/index.html", "Theme shell");
    deploy_build(dir.path()).await;
    deploy_build(dir.path()).await;
    let paths = crate::moss_paths::MossPaths::new(dir.path());
    let key = "_moss/theme/nested/index.html";
    let bytes = std::fs::read(paths.staging_dir().join(key)).unwrap();
    std::fs::remove_file(dir.path().join(".moss/theme/nested/index.html")).unwrap();
    std::fs::write(dir.path().join(".moss/theme/nested/.index.html.icloud"), "").unwrap();
    deploy_build(dir.path()).await;
    assert_eq!(output_unchanged(dir.path()), 1);
    std::fs::write(dir.path().join("notes.md"), "---\ntitle: Notes\n---\nNew notes.\n").unwrap();
    let sealed = deploy_build(dir.path()).await;
    assert_eq!(output_unchanged(dir.path()), 0, "the sweep runs on this build");
    assert_eq!(std::fs::read(paths.staging_dir().join(key)).unwrap(), bytes);
    assert_eq!(std::fs::read(paths.generation_dir(sealed.generation_id()).join(key)).unwrap(), bytes);
    assert!(sealed.files().contains_key(key));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unchanged_rebuild_with_a_folder_redirect_reports_no_output_change() {
    let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _indexes = crate::build::feeds::search::lock_index_counter();
    let dir = source_site();
    let config = dir.path().join(".moss/config.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config, format!("{text}[redirects]\n\"/old/\" = \"/guide/\"\n")).unwrap();
    write_page(dir.path(), "guide/extra.html", "Extra guide teal");
    let _session = crate::system::folder_session::register_session(&dir.path().to_string_lossy());
    let first = deploy_build(dir.path()).await;
    assert!(first.files().contains_key("old/extra.html"), "the folder redirect forwards the page under it");
    let second = deploy_build(dir.path()).await;
    assert_eq!(output_unchanged(dir.path()), 1, "a forwarding page written after seal is not a deleted page");
    assert!(second.files().contains_key("old/extra.html"));
    std::fs::write(dir.path().join("notes.md"), "---\ntitle: Notes\n---\nNew notes.\n").unwrap();
    let paths = crate::moss_paths::MossPaths::new(dir.path());
    let forwarder = std::fs::read(paths.staging_dir().join("old/extra.html")).unwrap();
    let changed = deploy_build(dir.path()).await;
    assert_eq!(output_unchanged(dir.path()), 0);
    assert!(changed.files().contains_key("old/extra.html"), "a changed build still seals the forwarding page");
    assert_eq!(std::fs::read(paths.generation_dir(changed.generation_id()).join("old/extra.html")).unwrap(), forwarder);
}

#[cfg(target_os = "macos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rebuild_keeps_an_evicted_authored_page_staged() {
    let _builds = crate::ENV_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let _indexes = crate::build::feeds::search::lock_index_counter();
    let dir = source_site();
    let _session = crate::system::folder_session::register_session(&dir.path().to_string_lossy());
    write_page(dir.path(), "guide.html", "Standalone guide vermilion");
    deploy_build(dir.path()).await;
    let paths = crate::moss_paths::MossPaths::new(dir.path());
    let bytes = std::fs::read(paths.staging_dir().join("guide.html")).unwrap();
    std::fs::remove_file(dir.path().join("guide.html")).unwrap();
    std::fs::write(dir.path().join(".guide.html.icloud"), "").unwrap();
    let root = crate::vault::paths::VaultRoot::resolve(dir.path());
    let rebuild = || build_and_seal(&root, crate::cli::host::cli_host_ports(root.as_str()), crate::build::PluginMode::Skip, false);
    let _ = rebuild().await;
    assert_eq!(output_unchanged(dir.path()), 1, "an evicted page is offline, not deleted");
    std::fs::write(dir.path().join("notes.md"), "---\ntitle: Notes\n---\nNew notes.\n").unwrap();
    let _ = rebuild().await;
    assert_eq!(output_unchanged(dir.path()), 0);
    assert_eq!(std::fs::read(paths.staging_dir().join("guide.html")).unwrap(), bytes, "the sweep spares an evicted page");
}
