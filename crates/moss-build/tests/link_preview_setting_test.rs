//! The `[site].link_preview` setting. On by default, and `false` ships no
//! hover-preview script, no script tag and no previews manifest.
//!
//! Its own test binary, like the hero caption test: the build problem counter
//! is per thread, and each build runs on the test's thread.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use moss_build::build::cli_output::take_cli_problems;
use moss_build::build::{run_pipeline, BuildTrigger, PipelineConfig, PluginMode};
use moss_build::cli::host::cli_host_ports;
use moss_build::vault_root::VaultRoot;

/// Builds share the process-wide CLI problem bookkeeping, so run them one at a time.
static BUILD_LOCK: Mutex<()> = Mutex::new(());

struct Site {
    dir: PathBuf,
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Write `files` into a scratch folder and build it. Returns the guard that
/// removes the folder and the build's output directory.
fn build(files: &[(&str, &str)]) -> (Site, PathBuf) {
    let _guard = BUILD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("moss_link_preview_{}", uuid::Uuid::new_v4()));
    let site = Site { dir: dir.clone() };
    for (rel, body) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    take_cli_problems();
    let source = dir.to_string_lossy().to_string();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(run_pipeline(PipelineConfig {
        root: VaultRoot::resolve(&dir),
        progress: moss_build::build::stdout_sink(),
        plugins: PluginMode::Skip,
        watch: false,
        start_server: false,
        host: cli_host_ports(&source),
        trigger: BuildTrigger::Full,
        exits_after_build: true,
        site_url_override: None,
        server_port: None,
        admission_epoch: None,
        live_port: None,
    }))
    .expect("build succeeds");
    (site, dir.join(".moss/build.nosync/staging"))
}

/// Whether the build wrote a hashed `_moss/js/preview.<hash>.js`.
fn has_preview_script(out: &Path) -> bool {
    fs::read_dir(out.join("_moss/js"))
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("preview."))
        })
        .unwrap_or(false)
}

/// Whether the home page carries a `<script src>` that names the preview bundle.
fn index_loads_preview(out: &Path) -> bool {
    let html = fs::read_to_string(out.join("index.html")).unwrap();
    html.match_indices("<script").any(|(start, _)| {
        let tag = &html[start..];
        let end = tag.find('>').unwrap_or(tag.len());
        tag[..end].contains("src=") && tag[..end].contains("preview.")
    })
}

const PAGE: &str = "---\ntitle: Home\n---\n\nA paragraph that links to [[Piece One]].\n";
const WORK_PIECE: &str = "---\ntitle: Piece One\n---\n\nThe piece.\n";
const WORK_INDEX: &str = "---\ntitle: Work\n---\n\nAll work.\n";
const SITE_CONFIG: &str = "[site]\ndomain = \"example.com\"\n";

fn site_files<'a>(config: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        (".moss/config.toml", config),
        ("index.md", PAGE),
        ("work/Piece One.md", WORK_PIECE),
        ("work/index.md", WORK_INDEX),
    ]
}

#[test]
fn link_preview_defaults_on() {
    let (_site, out) = build(&site_files(SITE_CONFIG));

    assert!(
        has_preview_script(&out),
        "preview.js is written when the key is absent"
    );
    assert!(index_loads_preview(&out), "index.html loads preview.js");
    assert!(
        out.join("_moss/previews.json").exists(),
        "previews.json is written when the key is absent"
    );
}

#[test]
fn link_preview_false_ships_no_script_and_no_manifest() {
    let config = format!("{SITE_CONFIG}link_preview = false\n");
    let (_site, out) = build(&site_files(&config));

    assert!(
        !has_preview_script(&out),
        "no preview.js when link_preview = false"
    );
    assert!(
        !index_loads_preview(&out),
        "no preview script tag when link_preview = false"
    );
    assert!(
        !out.join("_moss/previews.json").exists(),
        "no previews.json when link_preview = false"
    );
}
