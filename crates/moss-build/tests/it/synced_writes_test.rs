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

/// A comment refresh's outcome is this computer's: a refresh that brings no
/// new comments (here the server never answers) writes nothing the folder
/// syncs, however often it is tried.
#[test]
fn a_comment_refresh_that_changes_no_comment_writes_nothing_to_the_synced_folder() {
    let dir = site("comment_refresh");
    let server = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    fs::create_dir_all(dir.join(".moss")).unwrap();
    // A dated page, so the article map names one for the refresh to ask about.
    fs::write(dir.join("notes.md"), "---\ntitle: Notes\ndate: 2026-01-02\n---\n\nA note.\n").unwrap();
    fs::write(
        dir.join(".moss/config.toml"),
        format!("[site]\ndomain = \"example.com\"\n\n[services.comments]\nprovider = \"artalk\"\nserver_url = \"{server}\"\n"),
    )
    .unwrap();
    settle(&dir, false);
    let first = synced(&dir);
    build(&dir, false);
    assert_eq!(written(&first, &synced(&dir)), Vec::<String>::new());
    let state = moss_build::build::features::sync::load_comment_sync_state(dir.to_str().unwrap());
    assert!(state.last_error.is_some(), "the refresh ran and recorded its outcome on this computer");
    let _ = fs::remove_dir_all(&dir);
}

/// The agent guidance in `.moss/agents/` as another computer's moss wrote it,
/// naming that computer's binary: a build here leaves it as it is, so two
/// computers building one folder stop trading the line.
#[test]
fn a_build_leaves_the_agent_guidance_another_computer_wrote() {
    let dir = site("agent_guidance");
    settle(&dir, false);
    let guidance = dir.join(".moss/agents/SKILL.md");
    let ours = fs::read_to_string(&guidance).expect("a build writes the neutral guidance");
    let (front, rest) = ours.split_once("<!-- moss:agent-guidance v").unwrap();
    let (version, rest) = rest.split_once(' ').unwrap();
    let body = rest.split_once("-->\n").unwrap().1;
    let after_binding: String = body.split_inclusive('\n').skip_while(|l| l.starts_with("> ")).collect();
    let theirs_rest = format!(
        "{front}> Run moss as `/Users/someone-else/dev/moss/target/debug/moss`.\n> Every `moss …` command below means that binary.\n{after_binding}"
    );
    let stamp = format!("<!-- moss:agent-guidance v{version} h:{:016x} -->", xxhash_rust::xxh3::xxh3_64(theirs_rest.as_bytes()));
    let theirs = format!("{front}{stamp}\n{}", theirs_rest.strip_prefix(front).unwrap());
    fs::write(&guidance, &theirs).unwrap();
    let first = synced(&dir);
    build(&dir, false);
    assert_eq!(written(&first, &synced(&dir)), Vec::<String>::new());
    assert_eq!(fs::read_to_string(&guidance).unwrap(), theirs);
    let _ = fs::remove_dir_all(&dir);
}
