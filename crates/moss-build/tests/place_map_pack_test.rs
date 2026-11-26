//! A site whose place kind says `landscape = "detail"` draws from the pack in
//! `.moss/maps/`: a replaced pack re-renders every place page, and a pack that
//! fails its check fails `--strict` on every build and stops a publish.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use moss_build::build::BuildTrigger;
use moss_build::moss_paths::MossPaths;

/// The problem count `--strict` reads is one per process.
static SERIAL: Mutex<()> = Mutex::new(());

const CONFIG: &str = r#"schema_version = 6

[site]
lang = "en"

[terms.places]
type = "place"
fields = ["location"]
landscape = "detail"
"#;

fn site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".moss")).unwrap();
    fs::write(root.join(".moss/config.toml"), CONFIG).unwrap();
    fs::write(root.join(".moss/places.toml"), "[\"Riverside\"]\nlat = 1.0\nlng = 2.0\nprecision = \"city\"\n").unwrap();
    fs::write(root.join("index.md"), "# Notes\n").unwrap();
    fs::write(root.join("walk.md"), "---\ntitle: Walk\nlocation: Riverside\n---\nA walk.\n").unwrap();
    dir
}

fn repository_pack() -> Vec<u8> {
    fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/place-map/place-map-v1.bin.br")).unwrap()
}

/// Writes `stored` as the folder's one pack, under the digest it would have
/// if `named` were its bytes.
fn install(root: &Path, stored: &[u8], named: &[u8]) -> PathBuf {
    let maps = MossPaths::new(root).maps_dir();
    let _ = fs::remove_dir_all(&maps);
    fs::create_dir_all(&maps).unwrap();
    let path = maps.join(moss_build::build::place_map::landscape_file_name(named));
    fs::write(&path, stored).unwrap();
    path
}

/// The repository's pack with one header byte changed: the manifest digest,
/// which `decode` does not check, so it decodes to a pack with another
/// fingerprint.
fn another_pack() -> Vec<u8> {
    let mut raw = Vec::new();
    brotli::BrotliDecompress(&mut repository_pack().as_slice(), &mut raw).unwrap();
    raw[40] ^= 0xff;
    let mut stored = Vec::new();
    let params = brotli::enc::BrotliEncoderParams { quality: 1, ..Default::default() };
    brotli::BrotliCompress(&mut raw.as_slice(), &mut stored, &params).unwrap();
    stored
}

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
        trigger,
        reason: None,
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

/// The `_moss/map.<hash>/` folder the site's place pages draw from.
fn map_folder(root: &Path) -> String {
    let paths = MossPaths::new(root);
    let generation = paths.generation_dir(&paths.current_generation_id().expect("a promoted generation"));
    let mut pages = vec![generation];
    while let Some(dir) = pages.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pages.push(path);
            } else if path.extension().is_some_and(|ext| ext == "html") {
                let html = fs::read_to_string(&path).unwrap();
                if let Some(start) = html.find("_moss/map.") {
                    return html[start..].split('/').nth(1).unwrap().to_string();
                }
            }
        }
    }
    panic!("no page names a map folder")
}

#[test]
fn a_replaced_pack_re_renders_the_place_page_on_the_next_one_shot_build() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = site();
    let root = dir.path().canonicalize().unwrap();
    let pack = repository_pack();
    install(&root, &pack, &pack);
    build(&root, BuildTrigger::Unobserved);
    // The first unobserved build after a full one renders in full.
    build(&root, BuildTrigger::Unobserved);
    let warm = build(&root, BuildTrigger::Unobserved);
    assert_eq!(warm["counts"]["unchanged_at_entry"].as_u64(), Some(1), "an untouched site ends at its entry: {warm}");
    let before = map_folder(&root);

    let other = another_pack();
    install(&root, &other, &other);
    let record = build(&root, BuildTrigger::Unobserved);

    assert_ne!(record["counts"]["unchanged_at_entry"].as_u64(), Some(1), "a pack swap is a change: {record}");
    assert_ne!(map_folder(&root), before, "the page names the new pack's map folder");
}

#[test]
fn a_pack_that_fails_its_digest_fails_every_strict_build() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = site();
    let root = dir.path().canonicalize().unwrap();
    install(&root, &repository_pack(), b"other bytes");
    let _ = moss_build::build::cli_output::take_cli_problems();

    for attempt in ["first", "repeat"] {
        build(&root, BuildTrigger::Full);
        assert!(moss_build::build::cli_output::take_cli_problems() >= 1, "the {attempt} --strict build counts the bad pack");
    }
}
