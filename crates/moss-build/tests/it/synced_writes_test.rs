//! What a build writes into the folder's synced `.moss/`, which a sync client
//! uploads to every machine that shares it. A build that changes nothing must
//! write nothing there, and the facts only true on this machine (keyed by its
//! own paths and mtimes) must never go there at all.

use moss_build::moss_paths::MossPaths;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

fn build(folder: &Path, preview: bool) {
    let turn = crate::site::turn();
    use moss_build::build::{BuildTrigger, PipelineConfig, PluginMode};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;

    let folder_path = folder.to_string_lossy().into_owned();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(crate::site::run(&turn, PipelineConfig {
        root: VaultRoot::resolve(PathBuf::from(&folder_path)),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder_path),
        trigger: BuildTrigger::Full, reason: None,
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
        let page = || fs::read_to_string(MossPaths::new(&dir).staging_dir().join("index.html")).expect("the home page");
        let before = page();
        fs::remove_dir_all(MossPaths::new(&dir).build_dir()).unwrap();
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

/// Every file under `.moss/` that records a file's stat (an mtime, an inode),
/// build output included. Two records of the build output carry one, and leave
/// the folder with it: the build manifest and each generation's own listing.
fn stat_keyed(dir: &Path) -> Vec<String> {
    let moss = dir.join(".moss");
    walkdir::WalkDir::new(&moss)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(&moss).unwrap().to_string_lossy().into_owned())
        .filter(|rel| rel != "build.nosync/hashes.json" && !(rel.starts_with("build.nosync/generations/.") && rel.ends_with(".whole.json")))
        .filter(|rel| fs::read(moss.join(rel)).is_ok_and(|bytes| bytes.windows(7).any(|w| w == b"\"mtime\"")))
        .collect()
}

/// This computer's facts about the folder live outside it: deleting them costs
/// a hash pass, never a page, and the synced folder does not notice. Nothing
/// left under `.moss/` is keyed by this computer's stats.
#[test]
fn a_build_without_this_computers_facts_publishes_the_same_pages() {
    let dir = site("facts_gone");
    let facts = moss_build::moss_paths::MossPaths::new(&dir).facts_dir();
    for preview in [false, true] {
        settle(&dir, preview);
        let page = || fs::read_to_string(MossPaths::new(&dir).staging_dir().join("index.html")).expect("the home page");
        let before = (page(), synced(&dir));
        assert!(facts.join("hash-index.json").exists(), "preview={preview}: the facts are where this test deletes them");
        fs::remove_dir_all(&facts).unwrap();
        build(&dir, preview);
        assert_eq!(page(), before.0, "preview={preview}");
        assert_eq!(written(&before.1, &synced(&dir)), Vec::<String>::new(), "preview={preview}");
        assert_eq!(stat_keyed(&dir), Vec::<String>::new(), "preview={preview}");
    }
    let _ = fs::remove_dir_all(&dir);
}

/// Synced files `written` since `before` that the slot pass made: its records
/// and the page blobs they name.
fn slot_pass_files(dir: &Path, before: &BTreeMap<String, (u64, i64, i64, u64)>) -> Vec<String> {
    let moss = dir.join(".moss");
    let after = synced(dir);
    after.iter().filter(|(p, s)| before.get(*p) != Some(s)).map(|(p, _)| p)
        .filter(|rel| {
            let bytes = fs::read(moss.join(rel)).unwrap_or_default();
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]).to_ascii_lowercase();
            (rel.starts_with("cache/transforms/") && bytes.windows(14).any(|w| w == b"html/slot-page"))
                || (rel.starts_with("cache/objects/") && head.trim_start().starts_with("<!doctype html"))
        })
        .cloned()
        .collect()
}

/// The slot pass's page blobs and records are this computer's: a build makes
/// them again from the folder. A first build leaves none in the synced folder,
/// and neither does the build after an upgrade, which changes the stylesheet
/// link on every page (here, the site's own stylesheet changes).
#[test]
fn the_slot_pass_writes_nothing_to_the_synced_folder() {
    let dir = site("slot_pass");
    let css = dir.join(".moss/theme/style.css");
    fs::create_dir_all(css.parent().unwrap()).unwrap();
    fs::write(&css, "body { color: #111; }\n").unwrap();
    let before = synced(&dir);
    settle(&dir, false);
    assert_eq!(slot_pass_files(&dir, &before), Vec::<String>::new(), "first build");
    let page = || fs::read_to_string(MossPaths::new(&dir).staging_dir().join("index.html")).expect("the home page");
    let first_page = page();
    fs::write(&css, "body { color: #222; }\n").unwrap();
    let before = synced(&dir);
    build(&dir, false);
    assert_ne!(page(), first_page, "the stylesheet link moved, as an upgrade moves it");
    assert_eq!(slot_pass_files(&dir, &before), Vec::<String>::new(), "upgrade");
    let _ = fs::remove_dir_all(&dir);
}

/// A folder an older moss built keeps this computer's facts in
/// `build.nosync/cache/`. The next build carries them out of the folder.
#[test]
fn a_build_moves_out_the_facts_an_older_moss_left_in_the_folder() {
    let dir = site("facts_left");
    settle(&dir, false);
    let paths = moss_build::moss_paths::MossPaths::new(&dir);
    let old = paths.cache_dir();
    fs::rename(paths.cache_hash_index(), old.join("hash-index.json")).unwrap();
    fs::write(old.join("staged-links-pages.json"), "{}").unwrap();
    build(&dir, false);
    assert!(!old.join("hash-index.json").exists() && !old.join("staged-links-pages.json").exists());
    assert!(paths.cache_hash_index().exists());
    let _ = fs::remove_dir_all(&dir);
}

/// What may be under the folder's `.moss/` after a build: the folder's own
/// files at the top, and these directories. Nothing a build makes for this
/// computer (the output, its records, this computer's facts) is among them.
const SYNCED_DIRS: &[&str] = &["agents/", "cache/", "data/", "history/", "identity/", "keys/", "deploy/records/"];

fn outside_synced_dirs(dir: &Path) -> Vec<String> {
    let moss = dir.join(".moss");
    walkdir::WalkDir::new(&moss)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| !e.file_type().is_dir())
        .map(|e| e.path().strip_prefix(&moss).unwrap().to_string_lossy().replace('\\', "/"))
        .filter(|rel| rel.contains('/') && !SYNCED_DIRS.iter().any(|kept| rel.starts_with(kept)))
        .collect()
}

#[cfg(unix)]
fn current_inode(paths: &MossPaths) -> u64 {
    fs::metadata(paths.generation_dir(&paths.current_generation_id().unwrap())).unwrap().ino()
}

/// The build output lives on this computer, not in the folder: after a first
/// build, and after the first open by this moss of a folder an older moss
/// built (its output in `.moss/build.nosync/`), the folder's `.moss/` holds
/// nothing else. On one volume the older output is renamed, so the last build
/// arrives whole: the same generation directory, served, no cold build.
#[test]
fn the_build_output_stays_out_of_the_folder() {
    let dir = site("output_out");
    build(&dir, false);
    assert_eq!(outside_synced_dirs(&dir), Vec::<String>::new(), "after a first build");
    let paths = MossPaths::new(&dir);
    assert!(paths.has_servable_generation());

    let generation = current_inode(&paths);
    fs::rename(paths.build_dir(), paths.legacy_build_dir()).unwrap();
    build(&dir, true);
    assert_eq!(outside_synced_dirs(&dir), Vec::<String>::new(), "after an upgrade open");
    assert_eq!(current_inode(&paths), generation, "the older output moved warm");
    let _ = fs::remove_dir_all(&dir);
}

/// The system may purge this computer's cache at any time. A purged output
/// reads as nothing built, and the next build makes it again.
#[test]
fn a_purged_build_output_is_built_again() {
    let dir = site("purged");
    build(&dir, false);
    let paths = MossPaths::new(&dir);
    fs::remove_dir_all(paths.build_dir()).unwrap();
    assert!(!paths.has_servable_generation(), "a purged root reads as nothing built");
    build(&dir, false);
    assert!(paths.has_servable_generation());
    let _ = fs::remove_dir_all(&dir);
}

/// A generation whose list of preview-only pages is gone (a partial purge)
/// is not one moss can vouch for: its drafts would read as published. It is
/// not served, and the next build replaces it.
#[test]
fn a_current_without_its_records_is_not_served_and_is_replaced() {
    let dir = site("partial_purge");
    build(&dir, false);
    let paths = MossPaths::new(&dir);
    let id = paths.current_generation_id().unwrap();
    fs::remove_file(paths.generations_dir().join(format!(".{id}.withheld.json"))).unwrap();
    assert!(!paths.has_servable_generation());
    assert_eq!(paths.initial_serve_dir(), paths.staging_dir());
    build(&dir, false);
    assert!(paths.has_servable_generation());
    let _ = fs::remove_dir_all(&dir);
}
