//! What a build writes into the folder's synced `.moss/`, which a sync client
//! uploads to every machine that shares it. A build that changes nothing must
//! write nothing there, and the facts only true on this machine (keyed by its
//! own paths and mtimes) must never go there at all.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

fn build(folder: &Path, preview: bool) {
    use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let folder_path = folder.to_string_lossy().into_owned();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(PathBuf::from(&folder_path)),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder_path),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        // A server already serving the folder is what makes a build defer
        // picture placeholders, as the app's open does.
        server_port: preview.then_some(0),
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }))
    .expect("build");
}

/// A folder with a page, two pictures and a video in a subfolder: the video's
/// facts are keyed by its stat in every build, the pictures' in a preview.
fn site(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("moss_synced_writes_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("media")).unwrap();
    fs::write(dir.join("index.md"), "# Home\n\n![a](media/a.png)\n\n![b](media/b.jpg)\n").unwrap();
    image::RgbImage::from_fn(96, 64, |x, y| image::Rgb([x as u8, y as u8, 90])).save(dir.join("media/a.png")).unwrap();
    image::RgbImage::from_fn(80, 120, |x, y| image::Rgb([200, x as u8, y as u8])).save(dir.join("media/b.jpg")).unwrap();
    fs::write(dir.join("media/clip.mp4"), b"not really a video").unwrap();
    dir
}

/// Every file under the synced `.moss/` (all of it but `build.nosync`), by
/// path, with what tells a rewrite from an untouched file.
fn synced(dir: &Path) -> BTreeMap<String, (u64, i64, i64, u64)> {
    let moss = dir.join(".moss");
    walkdir::WalkDir::new(&moss)
        .into_iter()
        .filter_entry(|e| e.file_name() != "build.nosync")
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let m = e.metadata().unwrap();
            let rel = e.path().strip_prefix(&moss).unwrap().to_string_lossy().into_owned();
            (rel, (m.len(), m.mtime(), m.mtime_nsec(), m.ino()))
        })
        .collect()
}

fn written(before: &BTreeMap<String, (u64, i64, i64, u64)>, after: &BTreeMap<String, (u64, i64, i64, u64)>) -> Vec<String> {
    let mut out: Vec<String> = after.iter().filter(|(p, s)| before.get(*p) != Some(s))
        .map(|(p, _)| format!("{} {p}", if before.contains_key(p) { "rewritten" } else { "new" })).collect();
    out.extend(before.keys().filter(|p| !after.contains_key(*p)).map(|p| format!("removed {p}")));
    out
}

/// A built folder: the second build records what the first could not know yet
/// (a video's hash, a preview's deferred placeholders and the page they change),
/// once.
fn settle(dir: &Path, preview: bool) {
    build(dir, preview);
    build(dir, preview);
}

#[test]
fn an_identical_build_of_a_built_folder_writes_nothing_to_the_synced_folder() {
    let dir = site("second_build");
    settle(&dir, false);
    let first = synced(&dir);
    build(&dir, false);
    assert_eq!(written(&first, &synced(&dir)), Vec::<String>::new());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn reopening_an_unchanged_folder_writes_nothing_to_the_synced_folder() {
    let dir = site("reopen");
    settle(&dir, true);
    let first = synced(&dir);
    build(&dir, true);
    assert_eq!(written(&first, &synced(&dir)), Vec::<String>::new());
    let _ = fs::remove_dir_all(&dir);
}

/// Everything under `build.nosync` is this machine's and may be lost (a new
/// machine, a cleared cache): the next build publishes the same pages, and no
/// stat-keyed record was ever written where the folder syncs.
#[test]
fn a_build_without_this_machines_cache_publishes_the_same_pages() {
    let dir = site("nosync_gone");
    for preview in [false, true] {
        build(&dir, preview);
        build(&dir, preview);
        let page = || fs::read_to_string(dir.join(".moss/build.nosync/staging/index.html")).expect("the home page");
        let before = page();
        fs::remove_dir_all(dir.join(".moss/build.nosync")).unwrap();
        build(&dir, preview);
        assert_eq!(page(), before, "preview={preview}");
        let stat_keyed: Vec<String> = synced(&dir).into_keys().filter(|p| p.contains("stat-") || p.contains("stat:")).collect();
        assert_eq!(stat_keyed, Vec::<String>::new(), "preview={preview}");
    }
    let _ = fs::remove_dir_all(&dir);
}
