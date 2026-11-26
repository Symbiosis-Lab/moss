//! A sealed build keeps its parse cache in a file, so the first edit in the
//! next process replays every unchanged page instead of parsing the whole site
//! again, and renders what a full parse renders. The file is read only for a
//! build proved against the seal it was written for.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use moss_build::build::{parse_cache, BuildTrigger};
use moss_build::moss_paths::MossPaths;

/// The parse cache and the environment are one per process.
static SERIAL: Mutex<()> = Mutex::new(());

fn build(root: &Path, trigger: BuildTrigger) -> serde_json::Value {
    use moss_build::build::{run_pipeline, PipelineConfig};
    use moss_build::cli::host::cli_host_ports;
    use moss_build::vault_root::VaultRoot;
    let folder = root.to_string_lossy().into_owned();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(root),
        progress: moss_build::build::stdout_sink(),
        plugins: moss_build::build::PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&folder),
        trigger, reason: None,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
        rename_pairs: Vec::new(),
    }))
    .unwrap_or_else(|e| panic!("build failed: {e}"));
    let text = fs::read_to_string(MossPaths::new(root).profile()).unwrap();
    serde_json::from_str(text.lines().last().unwrap()).unwrap()
}

fn stopped_at_entry(record: &serde_json::Value) -> bool {
    record["counts"]["unchanged_at_entry"].as_u64() == Some(1)
}

/// Clear only the parse globals; OS-process coverage is below.
fn reset_parse_globals() {
    parse_cache::reset_for_tests();
}

fn generation(root: &Path) -> String {
    MossPaths::new(root).current_generation_id().expect("a promoted generation")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn pages(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        let hidden = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
        if path.is_dir() && !hidden {
            pages(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

/// A copy of `input` with a settled seal: a first build's media are new
/// against no manifest, so only the second seal is one an open can prove.
fn sealed_copy(input: &Path) -> tempfile::TempDir {
    let site = tempfile::tempdir().unwrap();
    copy_tree(input, site.path());
    build(site.path(), BuildTrigger::Unobserved);
    build(site.path(), BuildTrigger::Unobserved);
    site
}

fn edit(page: &Path) -> BuildTrigger {
    let mut text = fs::read_to_string(page).unwrap();
    text.push_str("\n\nOne more line.\n");
    fs::write(page, text).unwrap();
    BuildTrigger::ContentOnly(vec![page.to_path_buf()])
}

fn snapshot_inputs() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/snapshot-sites");
    let mut inputs: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path().join("input")).filter(|p| p.is_dir()).collect();
    inputs.sort();
    inputs
}

#[test]
fn the_first_edit_after_an_open_replays_the_kept_parse_and_renders_the_same_site() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut replayed_somewhere = false;
    for input in snapshot_inputs() {
        let name = input.parent().unwrap().file_name().unwrap().to_string_lossy().into_owned();
        let site = sealed_copy(&input);
        let root = site.path();
        let mut all = Vec::new();
        pages(root, &mut all);
        let Some(page) = all.first() else { continue };

        reset_parse_globals();
        let open = build(root, BuildTrigger::Unobserved);
        if !stopped_at_entry(&open) {
            continue;
        }
        build(root, edit(page));
        let stats = parse_cache::last_stats().expect("stats");
        assert!(stats.eligible, "{name}: {}", stats.reason);
        assert_eq!(stats.hits + 1, stats.total, "{name}: every page but the edited one replays, reparsed {:?}", stats.misses);
        replayed_somewhere |= stats.hits > 0;
        let replayed = generation(root);

        reset_parse_globals();
        build(root, BuildTrigger::Full);
        assert_eq!(generation(root), replayed, "{name}: the replayed parse renders what a full parse renders");
    }
    assert!(replayed_somewhere, "no fixture opened without building, so nothing was replayed");
}

/// A grid, an inline subscribe form and a hero drawn from a folder keep typed
/// blocks for the render phase, which the kept file does not hold: those pages
/// parse again, and the site still renders what a full parse renders.
#[test]
fn a_page_with_a_grid_a_subscribe_form_or_a_drawn_hero_parses_again_and_renders_the_same() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = tempfile::tempdir().unwrap();
    let root = site.path();
    fs::write(root.join("index.md"), "---\ntitle: Home\n---\n\nIntro.\n\n:::grid 2\n[About](about/)\n+++\n[Join](join/)\n:::\n\nAfter.\n").unwrap();
    fs::write(root.join("join.md"), "---\ntitle: Join\n---\n\nIntro.\n\n:::subscribe {button=\"Join\"}\n:::\n").unwrap();
    fs::write(root.join("about.md"), "---\ntitle: About\n---\n\nAbout us.\n").unwrap();
    fs::write(root.join("notes.md"), "---\ntitle: Notes\n---\n\nSome notes.\n").unwrap();
    fs::create_dir(root.join("work")).unwrap();
    fs::write(root.join("work/one.md"), "---\ntitle: One\n---\n\nThe first piece.\n").unwrap();
    fs::write(root.join("latest.md"), "---\ntitle: Latest\n---\n\n:::hero {from=\"work/\"}\n# Nothing yet\n:::\n").unwrap();
    build(root, BuildTrigger::Unobserved);
    build(root, BuildTrigger::Unobserved);

    reset_parse_globals();
    assert!(stopped_at_entry(&build(root, BuildTrigger::Unobserved)), "the open proves the seal");
    build(root, edit(&root.join("notes.md")));
    let stats = parse_cache::last_stats().expect("stats");
    assert!(stats.eligible, "{}", stats.reason);
    assert!(!stats.missed("about.md"), "a plain page replays: {stats:?}");
    assert!(stats.missed("index.md") && stats.missed("join.md"), "the grid and subscribe pages parse again: {stats:?}");
    assert!(stats.missed("latest.md"), "the drawn hero's page parses again: {stats:?}");
    let replayed = generation(root);

    reset_parse_globals();
    build(root, BuildTrigger::Full);
    assert_eq!(generation(root), replayed, "the replayed parse renders what a full parse renders");
}

/// An image the parse baked in changes between the seal and the next process.
/// Without a proof from an open, the kept parse is not read.
#[test]
fn a_build_no_open_proved_reads_nothing_kept() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let site = tempfile::tempdir().unwrap();
    let root = site.path();
    fs::write(root.join("index.md"), "---\ntitle: Home\n---\n\n![A square](square.png)\n").unwrap();
    fs::write(root.join("about.md"), "---\ntitle: About\n---\n\nAbout us.\n").unwrap();
    fs::write(root.join("square.png"), png(4)).unwrap();
    build(root, BuildTrigger::Unobserved);
    build(root, BuildTrigger::Unobserved);

    reset_parse_globals();
    fs::write(root.join("square.png"), png(8)).unwrap();
    build(root, edit(&root.join("about.md")));
    let stats = parse_cache::last_stats().expect("stats");
    assert_eq!(stats.hits, 0, "nothing proved the seal, so nothing kept is replayed: {stats:?}");
}

/// A `side`-pixel square PNG, so two sizes are two different images.
fn png(side: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbImage::from_pixel(side, side, image::Rgb([200, 80, 40]))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

/// Re-execute this test binary: no cache globals survive between phases.
fn child(root: &Path, phase: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fresh_process_child", "--ignored", "--nocapture"])
        .env("MOSS_CACHE_TEST_ROOT", root)
        .env("MOSS_CACHE_TEST_PHASE", phase)
        .output()
        .unwrap();
    assert!(output.status.success(), "{phase}: {}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

#[test]
#[ignore = "launched by fresh_process_output_and_replay_parity with an isolated site"]
fn fresh_process_child() {
    let root = PathBuf::from(std::env::var_os("MOSS_CACHE_TEST_ROOT").expect("child site"));
    match std::env::var("MOSS_CACHE_TEST_PHASE").unwrap().as_str() {
        "seed" => {
            build(&root, BuildTrigger::Unobserved);
            build(&root, BuildTrigger::Unobserved);
        }
        "replay" => {
            assert!(stopped_at_entry(&build(&root, BuildTrigger::Unobserved)), "fresh open must prove the seal");
            build(&root, edit(&root.join("about.md")));
            let stats = parse_cache::last_stats().expect("replay stats");
            assert!(stats.eligible, "{stats:?}");
            assert!(stats.hits > 0, "the fresh process must actually replay: {stats:?}");
            assert_eq!(stats.hits + 1, stats.total, "only the edited page reparses: {stats:?}");
        }
        "changed-image" => {
            fs::write(root.join("square.png"), png(8)).unwrap();
            build(&root, edit(&root.join("about.md")));
            let stats = parse_cache::last_stats().expect("refusal stats");
            assert_eq!(stats.hits, 0, "a changed image without a proven open refuses persisted entries: {stats:?}");
        }
        "full" => { build(&root, BuildTrigger::Full); }
        phase => panic!("unknown phase {phase}"),
    }
}

fn output_bytes(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn collect(dir: &Path, base: &Path, files: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, base, files);
            } else {
                files.insert(path.strip_prefix(base).unwrap().to_path_buf(), fs::read(path).unwrap());
            }
        }
    }
    let dir = MossPaths::new(root).current_ptr();
    let mut files = std::collections::BTreeMap::new();
    collect(&dir, &dir, &mut files);
    assert!(!files.is_empty());
    files
}

#[test]
fn fresh_process_output_and_replay_parity() {
    let site = tempfile::tempdir().unwrap();
    let root = site.path();
    fs::write(root.join("index.md"), "---\ntitle: Home\n---\n\n![A square](square.png)\n").unwrap();
    fs::write(root.join("about.md"), "---\ntitle: About\n---\n\nAbout us.\n").unwrap();
    fs::write(root.join("notes.md"), "---\ntitle: Notes\n---\n\nSome notes.\n").unwrap();
    fs::write(root.join("square.png"), png(4)).unwrap();
    child(root, "seed");
    child(root, "replay");
    let replayed = output_bytes(root);
    child(root, "full");
    assert_eq!(output_bytes(root), replayed, "persisted replay and fresh full parse must emit identical files");
    child(root, "changed-image");
    let changed = output_bytes(root);
    child(root, "full");
    assert_eq!(output_bytes(root), changed, "refusing the cache must render the changed image exactly like a full parse");
}
